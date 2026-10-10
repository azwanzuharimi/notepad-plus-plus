// SPDX-License-Identifier: GPL-3.0-or-later
use crate::search::{Doc, Mode, Opts};
use crate::{item, nested, ns, sci, tagged, tools, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, DefinedClass, MainThreadMarker};
use objc2_app_kit::{
    NSBeep, NSEventModifierFlags, NSMenuItem, NSPasteboard, NSPasteboardTypeString, NSResponder,
    NSView,
};
use std::ffi::c_void;
use std::sync::OnceLock;

pub const MARK_BOOKMARK: usize = 20;
pub const BOOKMARK_MARGIN: usize = 1;
pub const CHANGE_MARGIN: usize = 2;
// Markers SC_MARKNUM_HISTORY_REVERTED_TO_ORIGIN (21) to SC_MARKNUM_HISTORY_REVERTED_TO_MODIFIED (24).
pub const HISTORY_MASK: isize = 0b1111 << 21;
const BOOKMARK_MASK: isize = 1 << MARK_BOOKMARK;
const FILL_FINDWHAT_THRESHOLD: usize = 1024;
const SCN_MARGINCLICK: u32 = 2010;
const SCI_GETCHARAT: u32 = 2007;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GOTOLINE: u32 = 2024;
const SCI_GOTOPOS: u32 = 2025;
const SCI_MARKERADD: u32 = 2043;
const SCI_MARKERDELETE: u32 = 2044;
const SCI_MARKERDELETEALL: u32 = 2045;
const SCI_MARKERGET: u32 = 2046;
const SCI_MARKERNEXT: u32 = 2047;
const SCI_MARKERPREVIOUS: u32 = 2048;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_SETSEL: u32 = 2160;
const SCI_EMPTYUNDOBUFFER: u32 = 2175;
const SCI_ENSUREVISIBLEENFORCEPOLICY: u32 = 2234;
const SCI_WORDSTARTPOSITION: u32 = 2266;
const SCI_WORDENDPOSITION: u32 = 2267;
const SCI_BRACEMATCH: u32 = 2353;
const SCI_CHOOSECARETX: u32 = 2399;
const SCI_GETDIRECTPOINTER: u32 = 2185;
const SCI_SETUNDOSAVEPOINT: u32 = 2791;

// Menu tags of the searchCmd: action.
const SELECT_NEXT: isize = 0;
const SELECT_PREV: isize = 1;
const VOLATILE_NEXT: isize = 2;
const VOLATILE_PREV: isize = 3;
const RESULTS_WINDOW: isize = 4;
const NEXT_RESULT: isize = 5;
const PREV_RESULT: isize = 6;
const GOTO_BRACE: isize = 7;
const SELECT_BRACES: isize = 8;
const NEXT_CHANGE: isize = 10;
const PREV_CHANGE: isize = 11;
const CLEAR_CHANGES: isize = 12;
const TOGGLE_BOOKMARK: isize = 20;
const NEXT_BOOKMARK: isize = 21;
const PREV_BOOKMARK: isize = 22;
const CLEAR_BOOKMARKS: isize = 23;
const CUT_MARKED: isize = 24;
const COPY_MARKED: isize = 25;
const PASTE_MARKED: isize = 26;
const REMOVE_MARKED: isize = 27;
const REMOVE_UNMARKED: isize = 28;
const INVERSE_MARKS: isize = 29;

// SCNotification up to the margin field.
#[repr(C)]
struct MarginNotify {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
    position: isize,
    ch: i32,
    modifiers: i32,
    modification_type: i32,
    text: *const u8,
    length: isize,
    lines_added: isize,
    message: i32,
    w_param: usize,
    l_param: isize,
    line: isize,
    fold_level_now: i32,
    fold_level_prev: i32,
    margin: i32,
}

const ICONS: &str = include_str!("../../PowerEditor/src/rgba_icons.h");

// Bytes of a `static const unsigned char name[N] = { 0x.., ... };` array in rgba_icons.h.
pub fn icon_bytes(src: &str, name: &str) -> Vec<u8> {
    let Some(at) = src.find(&format!(" {name}[")) else {
        return vec![];
    };
    let body = &src[at..];
    let (Some(a), Some(b)) = (body.find('{'), body.find('}')) else {
        return vec![];
    };
    body[a + 1..b]
        .split(',')
        .filter_map(|t| u8::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok())
        .collect()
}

// Notepad++ bookmark14 icon: 14 x 14 RGBA.
pub fn bookmark_icon() -> &'static [u8] {
    static I: OnceLock<Vec<u8>> = OnceLock::new();
    I.get_or_init(|| icon_bytes(ICONS, "bookmark14"))
}

// Port of Notepad_plus::findMatchingBracePos: the character before the caret has priority.
pub fn brace_at(before: u8, after: u8, caret: isize) -> Option<isize> {
    const BRACES: &[u8] = b"[](){}";
    if caret > 0 && BRACES.contains(&before) {
        Some(caret - 1)
    } else if BRACES.contains(&after) {
        Some(caret)
    } else {
        None
    }
}

// Port of Notepad_plus::changedHistoryGoTo: skips the changed block of the current line and wraps.
pub fn next_change(
    changed: impl Fn(isize) -> bool,
    cur: isize,
    count: isize,
    up: bool,
) -> Option<isize> {
    let prev = |from: isize| (0..=from).rev().find(|&l| changed(l));
    if !up {
        let mut block = cur;
        for l in cur..count {
            if changed(l) {
                if l != block {
                    return Some(l);
                }
                block += 1;
            }
        }
        return (0..=cur).find(|&l| changed(l));
    }
    let mut block = cur;
    loop {
        match prev(block) {
            Some(l) if l == block => block -= 1,
            Some(l) => return Some(l),
            None => break,
        }
    }
    prev(count - 1)
}

// Lines of the search results panel.
#[derive(Clone, Copy)]
pub enum Row<'a> {
    Search,
    File,
    Hit(&'a [(isize, isize)]),
}

enum At {
    Front,
    Between(usize),
    Inside(usize),
    Behind(usize),
}

// Port of Finder::getCurrentPosInLineInfo.
fn pos_info(marks: &[(isize, isize)], pos: isize, has_sel: bool) -> At {
    let mut last_end = 0;
    for (i, &(s, e)) in marks.iter().enumerate() {
        let n = i + 1;
        if last_end <= pos && pos < s {
            return if n == 1 {
                At::Front
            } else {
                At::Between(n - 1)
            };
        }
        if s <= pos && pos <= e {
            return if pos == s && !has_sel {
                At::Between(n - 1)
            } else if pos == e && !has_sel {
                At::Between(n)
            } else {
                At::Inside(n)
            };
        }
        if e < pos && n == marks.len() {
            return At::Behind(n);
        }
        last_end = e;
    }
    At::Front
}

// Port of Finder::gotoNextFoundResult: the result line and the 1-based hit in it, inside the current search.
pub fn next_result(
    rows: &[Row],
    line: usize,
    pos: isize,
    has_sel: bool,
    up: bool,
) -> Option<(usize, usize)> {
    if line >= rows.len() {
        return None;
    }
    let hit = |l: isize| match rows.get(l as usize) {
        Some(Row::Hit(m)) if l >= 0 => Some(*m),
        _ => None,
    };
    let is_search = |l: &usize| matches!(rows[*l], Row::Search);
    let min = (0..=line).rev().find(is_search).unwrap_or(line) as isize;
    let max = (line + 1..rows.len())
        .find(is_search)
        .map_or(rows.len() - 1, |l| l - 1) as isize;
    let init = line as isize;
    let anchor = |mut l: isize| {
        if l > max && !up {
            l = min;
        }
        while hit(l).is_none() {
            l += if up { -1 } else { 1 };
            if l > max {
                l = min;
            } else if l < min {
                l = max;
            }
            if l == init {
                break;
            }
        }
        l
    };
    let mut lno = anchor(init);
    let (pos, has_sel) = if lno != init {
        (if up { isize::MAX } else { 0 }, false)
    } else {
        (pos, has_sel)
    };
    let marks = hit(lno)?;
    let occ = match (up, pos_info(marks, pos, has_sel)) {
        (false, At::Front) => 1,
        (false, At::Between(a) | At::Inside(a)) if a < marks.len() => a + 1,
        (false, _) => {
            lno = anchor(lno + 1);
            1
        }
        (true, At::Between(a) | At::Behind(a)) => a,
        (true, At::Inside(a)) if a > 1 => a - 1,
        (true, _) => {
            if lno < 1 {
                return None;
            }
            lno = anchor(lno - 1);
            hit(lno)?.len()
        }
    };
    let mut occ = occ.max(1);
    // A hit past the cut of a long result line has no text to select, so go on to the next hit.
    let shown = |l: isize, k: usize| hit(l).and_then(|m| m.get(k - 1)).is_some_and(|&(s, e)| s < e);
    for _ in 0..=rows.iter().map(|r| match r { Row::Hit(m) => m.len(), _ => 1 }).sum::<usize>() {
        if shown(lno, occ) {
            return Some((lno as usize, occ));
        }
        let n = hit(lno)?.len();
        if up && occ > 1 {
            occ -= 1;
        } else if up {
            if lno < 1 {
                return None;
            }
            lno = anchor(lno - 1);
            occ = hit(lno)?.len();
        } else if occ < n {
            occ += 1;
        } else {
            lno = anchor(lno + 1);
            occ = 1;
        }
    }
    None
}

fn full_line(doc: &Doc, l: isize) -> (isize, isize) {
    let s = doc.line_span(l).0;
    let e = if l + 1 < doc.lines() {
        doc.line_span(l + 1).0
    } else {
        doc.len()
    };
    (s, e)
}

// Text of the lines with their line ends, in line order.
pub fn lines_text(doc: &Doc, lines: &[isize]) -> Vec<u8> {
    lines
        .iter()
        .flat_map(|&l| {
            let (s, e) = full_line(doc, l);
            doc.range(s, e)
        })
        .collect()
}

// Deletes the lines with their line ends from the bottom up as one undo action; `before` runs on each line first.
pub fn delete_lines(doc: &Doc, lines: &[isize], mut before: impl FnMut(isize)) {
    doc.undo_group(true);
    for &l in lines.iter().rev() {
        before(l);
        let (s, e) = full_line(doc, l);
        doc.replace(s, e - s, b"", false);
    }
    doc.undo_group(false);
}

// Replaces the text of each line, but not its line end, as one undo action.
pub fn replace_lines(doc: &Doc, lines: &[isize], with: &[u8]) {
    doc.undo_group(true);
    for &l in lines.iter().rev() {
        let (s, e) = doc.line_span(l);
        doc.replace(s, e - s, with, false);
    }
    doc.undo_group(false);
}

// Lines 0..n that are not in the sorted list.
pub fn others(lines: &[isize], n: isize) -> Vec<isize> {
    (0..n).filter(|l| lines.binary_search(l).is_err()).collect()
}

fn current_line(v: &NSView) -> isize {
    let pos = sci::send(v, SCI_GETCURRENTPOS, 0, 0);
    sci::send(v, sci::SCI_LINEFROMPOSITION, pos as usize, 0)
}

fn go_to_line(v: &NSView, l: isize) {
    sci::send(v, SCI_ENSUREVISIBLEENFORCEPOLICY, l as usize, 0);
    sci::send(v, SCI_GOTOLINE, l as usize, 0);
}

fn has_bookmark(v: &NSView, l: isize) -> bool {
    sci::send(v, SCI_MARKERGET, l as usize, 0) & BOOKMARK_MASK != 0
}

fn delete_bookmark(v: &NSView, l: isize) {
    while has_bookmark(v, l) {
        sci::send(v, SCI_MARKERDELETE, l as usize, MARK_BOOKMARK as isize);
    }
}

fn toggle_bookmark(v: &NSView, l: isize) {
    if has_bookmark(v, l) {
        delete_bookmark(v, l);
    } else {
        sci::send(v, SCI_MARKERADD, l as usize, MARK_BOOKMARK as isize);
    }
}

fn bookmarked(v: &NSView) -> Vec<isize> {
    let mut out = vec![];
    let mut l = 0;
    loop {
        let n = sci::send(v, SCI_MARKERNEXT, l, BOOKMARK_MASK);
        if n < 0 {
            return out;
        }
        out.push(n);
        l = n as usize + 1;
    }
}

// Port of Notepad_plus::bookmarkNext.
fn next_bookmark(v: &NSView, forward: bool) {
    let line = current_line(v);
    let (start, retry, msg) = if forward {
        (line + 1, 0, SCI_MARKERNEXT)
    } else {
        (
            line - 1,
            sci::send(v, SCI_GETLINECOUNT, 0, 0),
            SCI_MARKERPREVIOUS,
        )
    };
    let mut n = sci::send(v, msg, start as usize, BOOKMARK_MASK);
    if n < 0 {
        n = sci::send(v, msg, retry as usize, BOOKMARK_MASK);
    }
    if n >= 0 {
        go_to_line(v, n);
        sci::send(v, SCI_CHOOSECARETX, 0, 0);
    }
}

// Selection text; with no selection, selects the word at the caret first (getSelectedTextToWChar).
pub(crate) fn word_selection(v: &NSView) -> String {
    let (mut s, mut e) = sci::selection(v);
    if s == e {
        let caret = sci::send(v, SCI_GETCURRENTPOS, 0, 0);
        let ws = sci::send(v, SCI_WORDSTARTPOSITION, caret as usize, 1);
        let we = sci::send(v, SCI_WORDENDPOSITION, caret as usize, 1);
        if ws != we {
            sci::select(v, (ws, we));
            (s, e) = (ws, we);
        }
    }
    String::from_utf8_lossy(&sci::doc(v).range(s, e)).into_owned()
}

fn brace(v: &NSView, select: bool) {
    let caret = sci::send(v, SCI_GETCURRENTPOS, 0, 0);
    if sci::length(v) == 0 {
        return;
    }
    let ch = |p: isize| sci::send(v, SCI_GETCHARAT, p as usize, 0) as u8;
    let before = if caret > 0 { ch(caret - 1) } else { 0 };
    let Some(at) = brace_at(before, ch(caret), caret) else {
        return;
    };
    let opp = sci::send(v, SCI_BRACEMATCH, at as usize, 0);
    if opp < 0 {
        return;
    }
    if select {
        sci::send(v, SCI_SETSEL, at.min(opp) as usize, at.max(opp) + 1);
    } else {
        sci::send(v, SCI_GOTOPOS, opp as usize, 0);
    }
    sci::send(v, SCI_CHOOSECARETX, 0, 0);
}

fn go_to_change(v: &NSView, up: bool) {
    let changed = |l: isize| sci::send(v, SCI_MARKERGET, l as usize, 0) & HISTORY_MASK != 0;
    let count = sci::send(v, SCI_GETLINECOUNT, 0, 0);
    match next_change(changed, current_line(v), count, up) {
        Some(l) => go_to_line(v, l),
        None => NSBeep(),
    }
}

// Port of Notepad_plus::clearChangesHistory; a modified document stays modified.
fn clear_changes(v: &NSView) {
    let pos = sci::send(v, SCI_GETCURRENTPOS, 0, 0);
    let modified = sci::is_modified(v);
    sci::send(v, SCI_EMPTYUNDOBUFFER, 0, 0);
    sci::reset_change_history(v);
    if modified {
        sci::send(v, SCI_SETUNDOSAVEPOINT, -1isize as usize, 0);
    }
    sci::send(v, SCI_GOTOPOS, pos as usize, 0);
}

fn clipboard() -> Option<String> {
    let pb = NSPasteboard::generalPasteboard();
    pb.stringForType(unsafe { NSPasteboardTypeString })
        .map(|s| s.to_string())
}

impl App {
    pub(crate) fn search_cmd(&self, tag: isize) {
        match tag {
            RESULTS_WINDOW => return self.focus_results(),
            NEXT_RESULT | PREV_RESULT => return self.go_to_result(tag == PREV_RESULT),
            _ => {}
        }
        let Some(v) = self.editor() else { return };
        let doc = sci::doc(&v);
        match tag {
            SELECT_NEXT | SELECT_PREV => {
                let s = word_selection(&v);
                if s.is_empty() || s.chars().count() > FILL_FINDWHAT_THRESHOLD {
                    return;
                }
                let c = &self.find_ui().c;
                c.find.setStringValue(&ns(&s));
                let o = Opts {
                    find: s,
                    mode: Mode::Normal,
                    ..c.opts()
                };
                self.find_with(&o, tag == SELECT_PREV);
            }
            VOLATILE_NEXT | VOLATILE_PREV => {
                let s = word_selection(&v);
                if s.is_empty() {
                    return;
                }
                let o = Opts {
                    find: s,
                    wrap: true,
                    ..Default::default()
                };
                self.find_with(&o, tag == VOLATILE_PREV);
            }
            GOTO_BRACE | SELECT_BRACES => brace(&v, tag == SELECT_BRACES),
            NEXT_CHANGE | PREV_CHANGE => go_to_change(&v, tag == PREV_CHANGE),
            CLEAR_CHANGES => clear_changes(&v),
            TOGGLE_BOOKMARK => toggle_bookmark(&v, current_line(&v)),
            NEXT_BOOKMARK | PREV_BOOKMARK => next_bookmark(&v, tag == NEXT_BOOKMARK),
            CLEAR_BOOKMARKS => {
                sci::send(&v, SCI_MARKERDELETEALL, MARK_BOOKMARK, 0);
            }
            INVERSE_MARKS => (0..doc.lines()).for_each(|l| toggle_bookmark(&v, l)),
            COPY_MARKED => {
                tools::to_clipboard(&String::from_utf8_lossy(&lines_text(&doc, &bookmarked(&v))))
            }
            _ if doc.read_only() => {}
            CUT_MARKED => {
                let lines = bookmarked(&v);
                let text = lines_text(&doc, &lines);
                delete_lines(&doc, &lines, |l| delete_bookmark(&v, l));
                tools::to_clipboard(&String::from_utf8_lossy(&text));
            }
            PASTE_MARKED => {
                if let Some(s) = clipboard() {
                    replace_lines(&doc, &bookmarked(&v), s.as_bytes());
                }
            }
            REMOVE_MARKED => delete_lines(&doc, &bookmarked(&v), |l| delete_bookmark(&v, l)),
            REMOVE_UNMARKED => delete_lines(&doc, &others(&bookmarked(&v), doc.lines()), |_| {}),
            _ => {}
        }
    }

    // Toggles a bookmark on a click in the bookmark margin without modifier keys.
    pub(crate) fn margin_click(&self, scn: *const c_void) {
        let n = unsafe { &*(scn as *const MarginNotify) };
        if n.code != SCN_MARGINCLICK || n.margin != BOOKMARK_MARGIN as i32 || n.modifiers != 0 {
            return;
        }
        let from = n.hwnd_from as isize;
        let view = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .find(|t| sci::send(&t.view, SCI_GETDIRECTPOINTER, 0, 0) == from)
            .map(|t| t.view.clone());
        if let Some(v) = view {
            let l = sci::send(&v, sci::SCI_LINEFROMPOSITION, n.position as usize, 0);
            toggle_bookmark(&v, l);
        }
    }

    // Focuses the results panel, or the editor when the panel has the focus.
    fn focus_results(&self) {
        let (rv, _) = self.ivars().results.get().unwrap();
        let w = self.ivars().window.get().unwrap();
        let content = sci::content(rv);
        let fr = w.firstResponder();
        if fr
            .as_deref()
            .map(|r| r as *const NSResponder as *const c_void)
            == Some(&*content as *const NSView as *const c_void)
        {
            self.focus();
            return;
        }
        self.reveal_results();
        w.makeFirstResponder(Some(&content));
    }

    fn go_to_result(&self, up: bool) {
        let rv = &self.ivars().results.get().unwrap().0;
        let doc = sci::doc(rv);
        let hit = {
            let lines = self.ivars().result_lines.borrow();
            let rows: Vec<Row> = lines
                .iter()
                .enumerate()
                .map(|(i, l)| match &l.hit {
                    Some(_) => Row::Hit(&l.marks),
                    None => {
                        let s = doc.line_span(i as isize).0;
                        if doc.range(s, s + 1) == b"S" {
                            Row::Search
                        } else {
                            Row::File
                        }
                    }
                })
                .collect();
            let pos = sci::send(rv, SCI_GETCURRENTPOS, 0, 0);
            let line = doc.line_of(pos);
            let (s, e) = sci::selection(rv);
            let start = doc.line_span(line).0;
            next_result(&rows, line as usize, pos - start, s != e, up)
                .map(|(l, occ)| (l, lines[l].marks[occ - 1]))
        };
        let Some((l, (ms, me))) = hit else { return };
        let (ls, le) = doc.line_span(l as isize);
        if ls + ms < (ls + me).min(le) {
            sci::select(rv, (ls + ms, (ls + me).min(le)));
        }
        self.ivars().pending_hit.set((l as isize, me));
        self.open_result(sel!(openResult:), None);
    }
}

pub fn search_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let (cmd, opt, shift) = (
        NSEventModifierFlags::Command,
        NSEventModifierFlags::Option,
        NSEventModifierFlags::Shift,
    );
    let none = NSEventModifierFlags::empty();
    let c = |title: &str, tag: isize, key: &str, mods: NSEventModifierFlags| {
        let i = tagged(mtm, title, sel!(searchCmd:), tag, t);
        if !key.is_empty() {
            i.setKeyEquivalent(&ns(key));
            i.setKeyEquivalentModifierMask(mods);
        }
        i
    };
    let (f2, f3, f4, f7) = ("\u{F705}", "\u{F706}", "\u{F707}", "\u{F70A}");
    let replace = item(mtm, "Replace...", sel!(showReplace:), "f", t);
    replace.setKeyEquivalentModifierMask(cmd | opt);
    vec![
        item(mtm, "Find...", sel!(showFind:), "f", t),
        item(mtm, "Find in Files...", sel!(showFindInFiles:), "F", t),
        item(mtm, "Find Next", sel!(findNext:), "g", t),
        item(mtm, "Find Previous", sel!(findPrevious:), "G", t),
        c("Select and Find Next", SELECT_NEXT, f3, cmd),
        c("Select and Find Previous", SELECT_PREV, f3, cmd | shift),
        c("Find (Volatile) Next", VOLATILE_NEXT, f3, cmd | opt),
        c(
            "Find (Volatile) Previous",
            VOLATILE_PREV,
            f3,
            cmd | opt | shift,
        ),
        replace,
        c("Search Results Window", RESULTS_WINDOW, f7, none),
        c("Next Search Result", NEXT_RESULT, f4, none),
        c("Previous Search Result", PREV_RESULT, f4, shift),
        item(mtm, "Go to...", sel!(goToLine:), "l", t),
        c("Go to Matching Brace", GOTO_BRACE, "b", cmd),
        c(
            "Select All In-between {} [] or ()",
            SELECT_BRACES,
            "b",
            cmd | opt,
        ),
        crate::mark::mark_item(mtm, t),
        NSMenuItem::separatorItem(mtm),
        nested(
            mtm,
            "Change History",
            vec![
                c("Go to Next Change", NEXT_CHANGE, "", none),
                c("Go to Previous Change", PREV_CHANGE, "", none),
                c("Clear Change History", CLEAR_CHANGES, "", none),
            ],
        ),
        NSMenuItem::separatorItem(mtm),
    ]
    .into_iter()
    .chain(crate::mark::style_menus(mtm, t))
    .chain([nested(
            mtm,
            "Bookmark",
            vec![
                c("Toggle Bookmark", TOGGLE_BOOKMARK, f2, cmd),
                c("Next Bookmark", NEXT_BOOKMARK, f2, none),
                c("Previous Bookmark", PREV_BOOKMARK, f2, shift),
                c("Clear All Bookmarks", CLEAR_BOOKMARKS, "", none),
                c("Cut Bookmarked Lines", CUT_MARKED, "", none),
                c("Copy Bookmarked Lines", COPY_MARKED, "", none),
                c(
                    "Paste to (Replace) Bookmarked Lines",
                    PASTE_MARKED,
                    "",
                    none,
                ),
                c("Remove Bookmarked Lines", REMOVE_MARKED, "", none),
                c("Remove Non-Bookmarked Lines", REMOVE_UNMARKED, "", none),
                c("Inverse Bookmarks", INVERSE_MARKS, "", none),
            ],
        )])
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> Doc {
        Doc::new(s.as_bytes()).unwrap()
    }

    #[test]
    fn bookmark_icon_is_14_by_14_rgba() {
        assert_eq!(bookmark_icon().len(), 14 * 14 * 4);
        assert_eq!(icon_bytes(ICONS, "bookmark18").len(), 18 * 18 * 4);
    }

    #[test]
    fn brace_before_caret_has_priority() {
        assert_eq!(brace_at(b')', b'(', 3), Some(2));
        assert_eq!(brace_at(b'a', b'{', 3), Some(3));
        assert_eq!(brace_at(b'a', b'b', 3), None);
        assert_eq!(brace_at(b'(', b'x', 0), None);
    }

    #[test]
    fn next_change_skips_current_block_and_wraps() {
        let set = [2isize, 3, 7];
        let ch = |l: isize| set.contains(&l);
        assert_eq!(next_change(ch, 0, 10, false), Some(2));
        assert_eq!(next_change(ch, 2, 10, false), Some(7));
        assert_eq!(next_change(ch, 7, 10, false), Some(2));
        assert_eq!(next_change(ch, 3, 10, true), Some(7));
        assert_eq!(next_change(ch, 8, 10, true), Some(7));
        assert_eq!(next_change(ch, 7, 10, true), Some(3));
        assert_eq!(next_change(|_| false, 4, 10, false), None);
        assert_eq!(next_change(|_| false, 4, 10, true), None);
    }

    #[test]
    fn next_result_walks_hits_of_current_search() {
        let a: &[(isize, isize)] = &[(10, 12), (20, 22)];
        let b: &[(isize, isize)] = &[(10, 13)];
        let rows = [
            Row::Search,
            Row::File,
            Row::Hit(a),
            Row::Hit(b),
            Row::Search,
            Row::File,
            Row::Hit(b),
        ];
        assert_eq!(next_result(&rows, 0, 0, false, false), Some((2, 1)));
        assert_eq!(next_result(&rows, 2, 12, true, false), Some((2, 2)));
        assert_eq!(next_result(&rows, 2, 22, true, false), Some((3, 1)));
        assert_eq!(next_result(&rows, 3, 13, true, false), Some((2, 1)));
        assert_eq!(next_result(&rows, 2, 12, true, true), Some((3, 1)));
        assert_eq!(next_result(&rows, 2, 22, true, true), Some((2, 1)));
        assert_eq!(next_result(&rows, 0, 0, false, true), Some((3, 1)));
        assert_eq!(next_result(&rows, 6, 13, true, false), Some((6, 1)));
        assert_eq!(next_result(&rows, 7, 0, false, false), None);
        assert_eq!(next_result(&[Row::Search], 0, 0, false, false), None);
        let cut: &[(isize, isize)] = &[(3000, 2044)];
        let rows = [Row::Search, Row::File, Row::Hit(a), Row::Hit(cut), Row::Hit(b)];
        assert_eq!(next_result(&rows, 2, 22, true, false), Some((4, 1)));
        assert_eq!(next_result(&rows, 4, 13, true, true), Some((2, 2)));
        assert_eq!(next_result(&[Row::Search, Row::Hit(cut)], 0, 0, false, false), None);
    }

    #[test]
    fn copy_and_cut_lines_keep_line_ends() {
        let d = doc("a\r\nb\r\nc\r\nd");
        assert_eq!(lines_text(&d, &[1, 3]), b"b\r\nd");
        let mut seen = vec![];
        delete_lines(&d, &[1, 3], |l| seen.push(l));
        assert_eq!(seen, [3, 1]);
        assert_eq!(d.text(), b"a\r\nc\r\n");
        d.undo();
        assert_eq!(d.text(), b"a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn remove_non_bookmarked_is_one_undo_action() {
        let d = doc("a\nb\nc\nd\n");
        delete_lines(&d, &others(&[1, 2], d.lines()), |_| {});
        assert_eq!(d.text(), b"b\nc\n");
        d.undo();
        assert_eq!(d.text(), b"a\nb\nc\nd\n");
    }

    #[test]
    fn paste_replaces_line_text_only() {
        let d = doc("a\nbb\nc");
        replace_lines(&d, &[1, 2], b"X");
        assert_eq!(d.text(), b"a\nX\nX");
        d.undo();
        assert_eq!(d.text(), b"a\nbb\nc");
    }

    #[test]
    fn read_only_document_does_not_change() {
        let d = doc("a\nb\n");
        d.set_read_only(true);
        delete_lines(&d, &[0], |_| {});
        replace_lines(&d, &[1], b"X");
        assert_eq!(d.text(), b"a\nb\n");
    }

    #[test]
    fn others_is_the_complement() {
        assert_eq!(others(&[0, 2], 4), [1, 3]);
        assert_eq!(others(&[], 2), [0, 1]);
    }
}
