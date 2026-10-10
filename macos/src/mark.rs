// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::Config;
use crate::panel::{self, Form};
use crate::search::{self, Doc, Mode, Opts};
use crate::search_extras::{word_selection, MARK_BOOKMARK};
use crate::{item, nested, ns, sci, tagged, tools, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSButton, NSEventModifierFlags, NSMenuItem, NSTextField, NSView};
use objc2_foundation::{NSArray, NSRunLoopCommonModes, NSString};
use std::cell::{Cell, OnceCell};

// SCE_UNIVERSAL_FOUND_STYLE_SMART and SCE_UNIVERSAL_FOUND_STYLE.
pub const SMART: usize = 29;
pub const FIND_MARK: usize = 31;
// SCE_UNIVERSAL_FOUND_STYLE_EXT1 to SCE_UNIVERSAL_FOUND_STYLE_EXT5.
pub const EXT: [usize; 5] = [25, 24, 23, 22, 21];
// SmartHighlighter MAXLINEHIGHLIGHT.
const MAX_LINES: isize = 400;

const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_INDICSETSTYLE: u32 = 2080;
const SCI_INDICSETFORE: u32 = 2082;
const SCI_GETFIRSTVISIBLELINE: u32 = 2152;
const SCI_SETSEL: u32 = 2160;
const SCI_SCROLLCARET: u32 = 2169;
const SCI_DOCLINEFROMVISIBLE: u32 = 2221;
const SCI_ENSUREVISIBLE: u32 = 2232;
const SCI_WORDSTARTPOSITION: u32 = 2266;
const SCI_WORDENDPOSITION: u32 = 2267;
const SCI_LINESONSCREEN: u32 = 2370;
const SCI_SETINDICATORCURRENT: u32 = 2500;
const SCI_INDICATORFILLRANGE: u32 = 2504;
const SCI_INDICATORCLEARRANGE: u32 = 2505;
const SCI_INDICATORVALUEAT: u32 = 2507;
const SCI_INDICATORSTART: u32 = 2508;
const SCI_INDICATOREND: u32 = 2509;
const SCI_INDICSETUNDER: u32 = 2510;
const SCI_INDICSETALPHA: u32 = 2523;
const SCI_MARKERGET: u32 = 2046;
const SCI_MARKERADD: u32 = 2043;
const SCI_MARKERDELETE: u32 = 2044;
const SCI_MARKERDELETEALL: u32 = 2045;
const INDIC_ROUNDBOX: isize = 7;

// Menu tags of the markCmd: action: group * 10 + style (0 to 4 for the 1st to 5th style).
const STYLE_ALL: isize = 0;
const STYLE_ONE: isize = 10;
const CLEAR: isize = 20;
const UP: isize = 30;
const DOWN: isize = 40;
const COPY: isize = 50;
// Style index of "Clear all Styles" and "All Styles"; FIND_STYLE is the Find Mark Style.
const ALL: isize = 5;
const FIND_STYLE: isize = 6;
pub const COPY_FIND_MARK: isize = COPY + FIND_STYLE;

pub struct MarkUi {
    form: Form,
    find: Retained<NSTextField>,
    bookmark: Retained<NSButton>,
    purge: Retained<NSButton>,
    whole: Retained<NSButton>,
    case: Retained<NSButton>,
    wrap: Retained<NSButton>,
    in_sel: Retained<NSButton>,
    modes: [Retained<NSButton>; 3],
    dot_nl: Retained<NSButton>,
    status: Retained<NSTextField>,
}

impl MarkUi {
    fn opts(&self) -> Opts {
        let mode = match self.modes.iter().position(|b| panel::on(b)) {
            Some(1) => Mode::Extended,
            Some(2) => Mode::Regex,
            _ => Mode::Normal,
        };
        Opts {
            find: panel::text(&self.find),
            whole_word: panel::on(&self.whole),
            match_case: panel::on(&self.case),
            wrap: panel::on(&self.wrap),
            mode,
            dot_nl: panel::on(&self.dot_nl),
            ..Default::default()
        }
    }

    fn set_status(&self, s: &str) {
        self.status.setStringValue(&ns(s));
    }
}

thread_local! {
    static UI: OnceCell<&'static MarkUi> = const { OnceCell::new() };
    // Notepad++ _disableSmartHiliteTmp: a jump to a style skips the next smart highlighting.
    static SKIP_SMART: Cell<bool> = const { Cell::new(false) };
}

// Matches of the text in the whole document, as FindReplaceDlg::markAll finds them.
pub fn occurrences(doc: &Doc, text: &str, whole: bool, case: bool) -> Vec<(isize, isize)> {
    let o = Opts {
        find: text.into(),
        whole_word: whole,
        match_case: case,
        wrap: true,
        ..Default::default()
    };
    search::process(doc, &o, false, false, (0, doc.len()))
        .unwrap_or_default()
        .into_iter()
        .filter(|(s, e)| e > s)
        .collect()
}

// Range that Mark All searches (FindReplaceDlg::processAll for ProcessMarkAll).
pub fn mark_range(sel: (isize, isize), len: isize, wrap: bool, in_sel: bool) -> (isize, isize) {
    if in_sel {
        sel
    } else if wrap {
        (0, len)
    } else {
        (sel.0, len)
    }
}

pub fn mark_status(n: usize, o: &Opts, in_sel: bool) -> String {
    let m = if n == 1 {
        "Mark: 1 match".to_string()
    } else {
        format!("Mark: {n} matches")
    };
    let scope = if in_sel {
        "in selected text"
    } else if o.wrap {
        "in entire file"
    } else {
        "from caret to end-of-file"
    };
    let lax = o.wrap && !o.match_case && !o.whole_word;
    if n == 0 && !lax {
        format!("{m} {scope}\n{}", search::NOT_FOUND_REASON)
    } else {
        format!("{m} {scope}")
    }
}

// Port of Notepad_plus::goToNextIndicator and goToPreviousIndicator with wrap: (anchor, caret) of the found range.
pub fn next_indicator(
    len: isize,
    pos: isize,
    up: bool,
    value: impl Fn(isize) -> bool,
    start: impl Fn(isize) -> isize,
    end: impl Fn(isize) -> isize,
) -> Option<(isize, isize)> {
    let mut inside = value(pos);
    let (mut s, mut e) = (start(pos), end(pos));
    if s == 0 && e == len - 1 {
        return None;
    }
    let at = if up {
        if s <= 0 {
            inside = value(len - 1);
            s = start(len - 1);
        }
        if inside {
            s = start((s - 1).max(0));
            if s <= 0 {
                s = start(len - 1);
            }
        }
        (s - 1).max(0)
    } else {
        if e >= len {
            inside = value(0);
            e = end(0);
        }
        if inside {
            e = end(e);
            if e >= len {
                e = end(0);
            }
        }
        e
    };
    let (s, e) = (start(at), end(at));
    value(s).then_some(if up { (e, s) } else { (s, e) })
}

// Styled ranges of one indicator, as ScintillaEditView::markedTextToClipboard walks them.
pub fn indicator_runs(
    value: impl Fn(isize) -> bool,
    end: impl Fn(isize) -> isize,
) -> Vec<(isize, isize)> {
    let mut out = vec![];
    let mut pos = end(0);
    if pos <= 0 {
        return out;
    }
    let mut on = value(0);
    let mut prev = if on { 0 } else { pos };
    loop {
        if on {
            out.push((prev, pos));
        }
        on = !on;
        prev = pos;
        pos = end(pos);
        if pos == prev {
            return out;
        }
    }
}

// Clipboard text of markedTextToClipboard; None when nothing is styled.
pub fn join_styled(mut items: Vec<(isize, String)>, sort: bool) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    if sort {
        items.sort();
    }
    let multi = items.len() > 1;
    let eol = items.iter().any(|(_, t)| t.contains(['\r', '\n']));
    let delim = if eol && multi { "\r\n----\r\n" } else { "\r\n" };
    let mut s = items
        .into_iter()
        .map(|(_, t)| t)
        .collect::<Vec<_>>()
        .join(delim);
    if multi {
        s += "\r\n";
    }
    Some(s)
}

fn send(v: &NSView, msg: u32, w: usize, l: isize) -> isize {
    sci::send(v, msg, w, l)
}

// Indicator styles of ScintillaEditView, with the colours of stylers.model.xml.
pub fn setup_indicators(v: &NSView, cfg: &Config) {
    for id in [SMART, FIND_MARK].into_iter().chain(EXT) {
        send(v, SCI_INDICSETSTYLE, id, INDIC_ROUNDBOX);
        send(v, SCI_INDICSETALPHA, id, 100);
        send(v, SCI_INDICSETUNDER, id, 1);
        if let Some(c) = cfg
            .global_styles
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.bg)
        {
            send(v, SCI_INDICSETFORE, id, c);
        }
    }
}

fn fill(v: &NSView, id: usize, (s, e): (isize, isize)) {
    send(v, SCI_SETINDICATORCURRENT, id, 0);
    send(v, SCI_INDICATORFILLRANGE, s as usize, e - s);
}

fn clear(v: &NSView, id: usize, (s, e): (isize, isize)) {
    send(v, SCI_SETINDICATORCURRENT, id, 0);
    send(v, SCI_INDICATORCLEARRANGE, s as usize, e - s);
}

fn clear_all(v: &NSView, id: usize) {
    clear(v, id, (0, sci::length(v)));
}

fn runs(v: &NSView, id: usize) -> Vec<(isize, isize)> {
    indicator_runs(
        |p| send(v, SCI_INDICATORVALUEAT, id, p) != 0,
        |p| send(v, SCI_INDICATOREND, id, p),
    )
}

fn style_id(k: isize) -> usize {
    EXT.get(k as usize).copied().unwrap_or(FIND_MARK)
}

// Lines from the line of `s` to the line of `e - 1`, as processRange marks them.
fn lines(doc: &Doc, (s, e): (isize, isize)) -> std::ops::RangeInclusive<isize> {
    doc.line_of(s)..=doc.line_of(e - 1)
}

fn has_bookmark(v: &NSView, l: isize) -> bool {
    send(v, SCI_MARKERGET, l as usize, 0) & (1 << MARK_BOOKMARK) != 0
}

fn jump(v: &NSView, id: usize, up: bool) {
    let len = sci::length(v);
    let pos = send(v, SCI_GETCURRENTPOS, 0, 0);
    let found = next_indicator(
        len,
        pos,
        up,
        |p| send(v, SCI_INDICATORVALUEAT, id, p) != 0,
        |p| send(v, SCI_INDICATORSTART, id, p),
        |p| send(v, SCI_INDICATOREND, id, p),
    );
    let Some((anchor, caret)) = found else { return };
    SKIP_SMART.set(true);
    let line = sci::doc(v).line_of(anchor.max(caret));
    send(v, SCI_ENSUREVISIBLE, line as usize, 0);
    send(v, SCI_SETSEL, anchor as usize, caret);
    send(v, SCI_SCROLLCARET, 0, 0);
}

fn copy_styled(v: &NSView, ids: &[usize]) {
    let doc = sci::doc(v);
    let items: Vec<(isize, String)> = ids
        .iter()
        .flat_map(|&id| runs(v, id))
        .map(|(s, e)| (s, String::from_utf8_lossy(&doc.range(s, e)).into_owned()))
        .collect();
    if let Some(s) = join_styled(items, ids.len() > 1) {
        tools::to_clipboard(&s);
    }
}

// FindReplaceDlg::clearMarks.
fn clear_marks(v: &NSView, in_sel: bool, bookmark: bool) {
    if in_sel {
        let sel = sci::selection(v);
        clear(v, FIND_MARK, sel);
        if bookmark {
            let doc = sci::doc(v);
            for l in lines(&doc, sel).filter(|&l| has_bookmark(v, l)) {
                send(v, SCI_MARKERDELETE, l as usize, MARK_BOOKMARK as isize);
            }
        }
    } else {
        clear_all(v, FIND_MARK);
        if bookmark {
            send(v, SCI_MARKERDELETEALL, MARK_BOOKMARK, 0);
        }
    }
}

impl App {
    fn mark_ui(&self) -> &'static MarkUi {
        UI.with(|c| *c.get_or_init(|| Box::leak(Box::new(self.build_mark_ui()))))
    }

    fn build_mark_ui(&self) -> MarkUi {
        let t: &AnyObject = self;
        let mtm = self.mtm();
        let f = Form::new(mtm, "Mark", 560., 340.);
        let noop = sel!(markModeChanged:);
        f.label("Find what:", 16., 16., 100.);
        let find = f.field(120., 16., 300.);
        let check = |title: &str, x: f64, top: f64| f.check(title, x, top, 200., t, noop);
        let bookmark = check("Bookmark line", 16., 56.);
        let purge = check("Purge for each search", 16., 80.);
        let whole = check("Match whole word only", 16., 104.);
        let case = check("Match case", 16., 128.);
        let wrap = check("Wrap around", 16., 152.);
        let in_sel = check("In selection", 240., 56.);
        panel::set_on(&wrap, true);
        f.label("Search Mode", 16., 184., 200.);
        let radio = |title: &str, top: f64, w: f64| {
            let b = unsafe {
                NSButton::radioButtonWithTitle_target_action(&ns(title), Some(t), Some(noop), mtm)
            };
            f.place(&b, 24., top, w, 20.);
            b
        };
        let modes = [
            radio("Normal", 206., 300.),
            radio("Extended (\\n, \\r, \\t, \\0, \\x...)", 228., 300.),
            radio("Regular expression", 250., 170.),
        ];
        panel::set_on(&modes[0], true);
        let dot_nl = f.check(". matches newline", 200., 250., 160., t, noop);
        dot_nl.setEnabled(false);
        let status = f.status(16., 284., 520., 40.);
        let (x, w) = (436., 110.);
        f.button("Mark All", x, 14., w, t, sel!(markAll:))
            .setKeyEquivalent(&ns("\r"));
        f.button("Clear all marks", x, 46., w, t, sel!(clearAllMarks:));
        f.button("Copy Marked Text", x, 78., w, t, sel!(copyMarkedText:));
        f.button("Close", x, 110., w, t, sel!(closePanel:))
            .setKeyEquivalent(&ns("\u{1b}"));
        MarkUi {
            form: f,
            find,
            bookmark,
            purge,
            whole,
            case,
            wrap,
            in_sel,
            modes,
            dot_nl,
            status,
        }
    }

    pub(crate) fn show_mark(&self) {
        let u = self.mark_ui();
        self.show_panel(&u.form, &u.find, &u.find);
    }

    pub(crate) fn mark_mode_changed(&self) {
        let u = self.mark_ui();
        u.dot_nl.setEnabled(panel::on(&u.modes[2]));
    }

    // FindReplaceDlg IDCMARKALL.
    pub(crate) fn mark_all(&self) {
        let u = self.mark_ui();
        u.set_status("");
        let Some(v) = self.editor() else { return };
        let o = u.opts();
        let doc = sci::doc(&v);
        let sel = sci::selection(&v);
        let in_sel = panel::on(&u.in_sel) && sel.0 != sel.1;
        let bookmark = panel::on(&u.bookmark);
        let range = mark_range(sel, doc.len(), o.wrap, in_sel);
        let found = if o.find.is_empty() || range.0 == range.1 {
            Ok(vec![])
        } else {
            if panel::on(&u.purge) {
                clear_marks(&v, in_sel, bookmark);
            }
            search::process(&doc, &o, false, false, range)
        };
        match found {
            Err(e) => u.set_status(&e),
            Ok(m) => {
                for &r in &m {
                    if r.1 > r.0 {
                        fill(&v, FIND_MARK, r);
                    }
                    if bookmark {
                        for l in lines(&doc, r).filter(|&l| !has_bookmark(&v, l)) {
                            send(&v, SCI_MARKERADD, l as usize, MARK_BOOKMARK as isize);
                        }
                    }
                }
                u.set_status(&mark_status(m.len(), &o, in_sel));
            }
        }
    }

    pub(crate) fn clear_all_marks(&self) {
        let u = self.mark_ui();
        if let Some(v) = self.editor() {
            let sel = sci::selection(&v);
            clear_marks(
                &v,
                panel::on(&u.in_sel) && sel.0 != sel.1,
                panel::on(&u.bookmark),
            );
        }
        u.set_status("");
    }

    pub(crate) fn mark_cmd(&self, tag: isize) {
        let Some(v) = self.editor() else { return };
        let (group, k) = (tag / 10 * 10, tag % 10);
        match group {
            STYLE_ALL => {
                let text = word_selection(&v);
                if !text.is_empty() {
                    let doc = sci::doc(&v);
                    for r in occurrences(&doc, &text, true, false) {
                        fill(&v, style_id(k), r);
                    }
                }
            }
            STYLE_ONE => {
                let (mut s, mut e) = sci::selection(&v);
                if s == e {
                    let caret = send(&v, SCI_GETCURRENTPOS, 0, 0);
                    s = send(&v, SCI_WORDSTARTPOSITION, caret as usize, 1);
                    e = send(&v, SCI_WORDENDPOSITION, caret as usize, 1);
                }
                if e > s {
                    fill(&v, style_id(k), (s, e));
                }
            }
            CLEAR if k == ALL => EXT.iter().for_each(|&id| clear_all(&v, id)),
            CLEAR => clear_all(&v, style_id(k)),
            UP | DOWN => jump(&v, style_id(k), group == UP),
            COPY if k == ALL => copy_styled(&v, &EXT),
            COPY => copy_styled(&v, &[style_id(k)]),
            _ => {}
        }
    }

    // SCN_UPDATEUI comes often, so the highlight runs once the notifications stop.
    pub(crate) fn schedule_smart_highlight(&self) {
        let none = None::<&AnyObject>;
        unsafe {
            let _: () = msg_send![class!(NSObject), cancelPreviousPerformRequestsWithTarget: self, selector: sel!(smartHighlight:), object: none];
            let modes = NSArray::from_slice(&[NSRunLoopCommonModes]);
            let _: () = msg_send![self, performSelector: sel!(smartHighlight:), withObject: none, afterDelay: 0.05f64, inModes: &*modes];
        }
    }

    // Port of SmartHighlighter::highlightView with the Notepad++ defaults: whole word, no match case.
    pub(crate) fn smart_highlight(&self) {
        let Some(v) = self.editor() else { return };
        if SKIP_SMART.replace(false) {
            return;
        }
        clear_all(&v, SMART);
        let (s, e) = sci::selection(&v);
        if s == e {
            return;
        }
        let caret = send(&v, SCI_GETCURRENTPOS, 0, 0);
        let ws = send(&v, SCI_WORDSTARTPOSITION, caret as usize, 1);
        let we = send(&v, SCI_WORDENDPOSITION, ws as usize, 1);
        if ws == we || ws != s || we != e {
            return;
        }
        let doc = sci::doc(&v);
        let o = Opts {
            find: String::from_utf8_lossy(&doc.range(s, e)).into_owned(),
            whole_word: true,
            wrap: true,
            ..Default::default()
        };
        let first = send(&v, SCI_GETFIRSTVISIBLELINE, 0, 0);
        let n = send(&v, SCI_LINESONSCREEN, 0, 0).min(MAX_LINES) + 1;
        let mut prev = -1;
        for vis in first..first + n {
            let l = send(&v, SCI_DOCLINEFROMVISIBLE, vis as usize, 0);
            if l == prev {
                continue;
            }
            prev = l;
            if l >= doc.lines() {
                break;
            }
            let start = doc.line_span(l).0;
            let end = if l + 1 < doc.lines() {
                doc.line_span(l + 1).0
            } else {
                doc.len()
            };
            for r in search::process(&doc, &o, false, false, (start, end)).unwrap_or_default() {
                if r.1 > r.0 {
                    fill(&v, SMART, r);
                }
            }
        }
    }
}

// "Mark..." for the Search menu: Notepad++ Ctrl+M is Cmd+Shift+M, because Cmd+M minimizes on macOS.
pub fn mark_item(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    item(mtm, "Mark...", sel!(showMark:), "M", t)
}

// The style token submenus of the Search menu and the separator after them.
pub fn style_menus(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let ord = ["1st", "2nd", "3rd", "4th", "5th"];
    let c = |title: &str, tag: isize| tagged(mtm, title, sel!(markCmd:), tag, t);
    let styles = |base: isize, fmt: &dyn Fn(&str) -> String| -> Vec<_> {
        ord.iter()
            .enumerate()
            .map(|(k, o)| c(&fmt(o), base + k as isize))
            .collect()
    };
    let ctrl = NSEventModifierFlags::Control;
    let opt = NSEventModifierFlags::Option;
    // Notepad++ Ctrl+1..5 and Ctrl+0 jump down; Ctrl+Shift jumps up, but AppKit does not match Shift with a digit.
    let jump = |base: isize, mods: NSEventModifierFlags| -> Vec<_> {
        let mut v = styles(base, &|o| format!("{o} Style"));
        v.push(c("Find Mark Style", base + FIND_STYLE));
        for (k, i) in v.iter().enumerate() {
            let key = if k < 5 {
                (k + 1).to_string()
            } else {
                "0".into()
            };
            i.setKeyEquivalent(&NSString::from_str(&key));
            i.setKeyEquivalentModifierMask(mods);
        }
        v
    };
    let mut clear = styles(CLEAR, &|o| format!("Clear {o} Style"));
    clear.push(c("Clear all Styles", CLEAR + ALL));
    let mut copy = styles(COPY, &|o| format!("{o} Style"));
    copy.push(c("All Styles", COPY + ALL));
    copy.push(c("Find Mark Style", COPY + FIND_STYLE));
    vec![
        nested(
            mtm,
            "Style All Occurrences of Token",
            styles(STYLE_ALL, &|o| format!("Using {o} Style")),
        ),
        nested(
            mtm,
            "Style One Token",
            styles(STYLE_ONE, &|o| format!("Using {o} Style")),
        ),
        nested(mtm, "Clear Style", clear),
        nested(mtm, "Jump Up", jump(UP, ctrl | opt)),
        nested(mtm, "Jump Down", jump(DOWN, ctrl)),
        nested(mtm, "Copy Styled Text", copy),
        NSMenuItem::separatorItem(mtm),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> Doc {
        Doc::new(s.as_bytes()).unwrap()
    }

    #[test]
    fn style_token_is_whole_word_and_ignores_case() {
        let d = doc("Foo foo food FOO xfoo");
        assert_eq!(
            occurrences(&d, "foo", true, false),
            [(0, 3), (4, 7), (13, 16)]
        );
        assert_eq!(occurrences(&d, "foo", true, true), [(4, 7)]);
        assert_eq!(occurrences(&d, "foo", false, false).len(), 5);
        assert!(occurrences(&d, "", true, false).is_empty());
        assert_eq!(
            occurrences(&doc("a.b a.b"), "a.b", true, false),
            [(0, 3), (4, 7)]
        );
    }

    #[test]
    fn mark_scope() {
        assert_eq!(mark_range((3, 5), 10, true, false), (0, 10));
        assert_eq!(mark_range((3, 5), 10, false, false), (3, 10));
        assert_eq!(mark_range((3, 5), 10, true, true), (3, 5));
        let o = Opts {
            wrap: true,
            ..Default::default()
        };
        assert_eq!(mark_status(1, &o, false), "Mark: 1 match in entire file");
        assert_eq!(mark_status(0, &o, false), "Mark: 0 matches in entire file");
        assert_eq!(mark_status(2, &o, true), "Mark: 2 matches in selected text");
        let strict = Opts {
            match_case: true,
            ..Default::default()
        };
        assert_eq!(
            mark_status(0, &strict, false),
            format!(
                "Mark: 0 matches from caret to end-of-file\n{}",
                search::NOT_FOUND_REASON
            )
        );
    }

    // Scintilla indicator runs for styled ranges in a document of length `len`.
    struct Runs {
        bounds: Vec<isize>,
        ranges: Vec<(isize, isize)>,
    }

    impl Runs {
        fn new(len: isize, ranges: &[(isize, isize)]) -> Runs {
            let mut bounds: Vec<isize> = [0, len]
                .into_iter()
                .chain(ranges.iter().flat_map(|&(s, e)| [s, e]))
                .collect();
            bounds.sort();
            bounds.dedup();
            Runs {
                bounds,
                ranges: ranges.to_vec(),
            }
        }

        fn run(&self, p: isize) -> (isize, isize) {
            let last = self.bounds.len() - 2;
            let i = self.bounds[..=last]
                .iter()
                .rposition(|&b| b <= p)
                .unwrap_or(0);
            (self.bounds[i], self.bounds[i + 1])
        }

        fn value(&self, p: isize) -> bool {
            let s = self.run(p).0;
            self.ranges.iter().any(|r| r.0 <= s && s < r.1)
        }

        fn next(&self, pos: isize, up: bool) -> Option<(isize, isize)> {
            let len = *self.bounds.last().unwrap();
            next_indicator(
                len,
                pos,
                up,
                |p| self.value(p),
                |p| self.run(p).0,
                |p| self.run(p).1,
            )
        }
    }

    #[test]
    fn jump_between_styled_ranges_and_wrap() {
        let r = Runs::new(30, &[(5, 8), (15, 18)]);
        assert_eq!(r.next(0, false), Some((5, 8)));
        assert_eq!(r.next(6, false), Some((15, 18)));
        assert_eq!(r.next(8, false), Some((15, 18)));
        assert_eq!(r.next(20, false), Some((5, 8)));
        assert_eq!(r.next(16, false), Some((5, 8)));
        assert_eq!(r.next(20, true), Some((18, 15)));
        assert_eq!(r.next(16, true), Some((8, 5)));
        assert_eq!(r.next(6, true), Some((18, 15)));
        assert_eq!(r.next(2, true), Some((18, 15)));
        assert_eq!(Runs::new(30, &[]).next(4, false), None);
        assert_eq!(Runs::new(30, &[]).next(4, true), None);
    }

    #[test]
    fn styled_runs_and_clipboard_text() {
        let r = Runs::new(30, &[(0, 3), (10, 12)]);
        assert_eq!(
            indicator_runs(|p| r.value(p), |p| r.run(p).1),
            [(0, 3), (10, 12)]
        );
        let r = Runs::new(30, &[(4, 6), (28, 30)]);
        assert_eq!(
            indicator_runs(|p| r.value(p), |p| r.run(p).1),
            [(4, 6), (28, 30)]
        );
        let r = Runs::new(30, &[]);
        assert!(indicator_runs(|p| r.value(p), |p| r.run(p).1).is_empty());
        assert_eq!(join_styled(vec![(0, "a".into())], false).unwrap(), "a");
        assert_eq!(
            join_styled(vec![(9, "b".into()), (2, "a".into())], true).unwrap(),
            "a\r\nb\r\n"
        );
        assert_eq!(
            join_styled(vec![(9, "b".into()), (2, "a\nx".into())], false).unwrap(),
            "b\r\n----\r\na\nx\r\n"
        );
        assert_eq!(join_styled(vec![], true), None);
    }
}
