// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, sel, DefinedClass};
use objc2_app_kit::{NSView, NSWorkspace};
use objc2_foundation::{NSArray, NSRunLoopCommonModes, NSURL};
use std::cell::RefCell;
use std::ffi::c_void;

// URL_INDIC in Notepad++ resource.h.
pub const URL_INDIC: usize = 8;

const SCI_SETSEL: u32 = 2160;
const SCI_SETCURRENTPOS: u32 = 2141;
const SCI_SETANCHOR: u32 = 2026;
const SCI_GETFIRSTVISIBLELINE: u32 = 2152;
const SCI_LINEFROMPOSITION: u32 = 2166;
const SCI_POSITIONFROMLINE: u32 = 2167;
const SCI_GETLINEENDPOSITION: u32 = 2136;
const SCI_GETDIRECTPOINTER: u32 = 2185;
const SCI_DOCLINEFROMVISIBLE: u32 = 2221;
const SCI_SETWORDCHARS: u32 = 2077;
const SCI_LINESONSCREEN: u32 = 2370;
const SCI_STYLEGETFORE: u32 = 2481;
const SCI_INDICSETSTYLE: u32 = 2080;
const SCI_INDICGETSTYLE: u32 = 2081;
const SCI_INDICSETALPHA: u32 = 2523;
const SCI_INDICSETHOVERSTYLE: u32 = 2680;
const SCI_INDICGETHOVERSTYLE: u32 = 2681;
const SCI_INDICSETHOVERFORE: u32 = 2682;
const SCI_INDICSETFLAGS: u32 = 2684;
const SCI_SETINDICATORCURRENT: u32 = 2500;
const SCI_SETINDICATORVALUE: u32 = 2502;
const SCI_INDICATORFILLRANGE: u32 = 2504;
const SCI_INDICATORCLEARRANGE: u32 = 2505;
const SCI_INDICATORALLONFOR: u32 = 2506;
const SCI_INDICATORSTART: u32 = 2508;
const SCI_INDICATOREND: u32 = 2509;
const INDIC_PLAIN: isize = 0;
const INDIC_HIDDEN: isize = 5;
const INDIC_FULLBOX: isize = 16;
const INDIC_EXPLORERLINK: isize = 23;
const SC_INDICFLAG_VALUEFORE: isize = 1;
const STYLE_DEFAULT: usize = 32;
const SCMOD_CTRL: i32 = 2;

// NppConstants.h urlMode.
const URL_NO_UNDERLINE_FG: i64 = 1;
const URL_UNDERLINE_FG: i64 = 2;
const URL_NO_UNDERLINE_BG: i64 = 3;
const URL_UNDERLINE_BG: i64 = 4;

thread_local! {
    static PENDING: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
}

#[repr(C)]
struct Notify {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
    position: isize,
    ch: i32,
    modifiers: i32,
}

// The url style from the Cloud & Link page flags: (enabled, no underline, fullbox).
pub fn url_style(on: bool, no_underline: bool, fullbox: bool) -> i64 {
    match (on, fullbox, no_underline) {
        (false, _, _) => 0,
        (_, true, true) => URL_NO_UNDERLINE_BG,
        (_, true, false) => URL_UNDERLINE_BG,
        (_, false, true) => URL_NO_UNDERLINE_FG,
        _ => URL_UNDERLINE_FG,
    }
}

pub fn no_underline(style: i64) -> bool {
    style == URL_NO_UNDERLINE_FG || style == URL_NO_UNDERLINE_BG
}

pub fn fullbox(style: i64) -> bool {
    style == URL_NO_UNDERLINE_BG || style == URL_UNDERLINE_BG
}

fn scheme_start(c: char) -> bool {
    c.is_ascii_alphabetic()
}

fn scheme_delimiter(c: char) -> bool {
    !(c.is_ascii_alphanumeric() || c == '_')
}

fn text_char(c: char) -> bool {
    !(c <= ' '
        || matches!(
            c,
            '\u{A0}' | '\u{2002}'
                ..='\u{200B}'
                    | '\u{3000}'
                    | '\u{202F}'
                    | '\u{205F}'
                    | '\u{FEFF}'
                    | '"'
                    | '#'
                    | '<'
                    | '>'
                    | '{'
                    | '}'
                    | '?'
                    | '\u{7F}'
        ))
}

fn query_delimiter(c: char) -> bool {
    matches!(c, '&' | '+' | '=' | ';')
}

fn scheme_supported(t: &[char], schemes: &str) -> bool {
    format!("ftp:// http:// https:// mailto: file:// {schemes}")
        .split(' ')
        .filter(|s| !s.is_empty())
        .any(|s| {
            let s: Vec<char> = s.chars().collect();
            s.len() <= t.len()
                && t.iter()
                    .zip(&s)
                    .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
        })
}

// Notepad_plus.cpp scanToUrlStart: Ok((distance, scheme length)) or Err(distance to the end).
fn scan_start(t: &[char], start: usize, schemes: &str) -> Result<(usize, usize), usize> {
    let mut p0 = None;
    for p in start..t.len() {
        match p0 {
            None => {
                if scheme_start(t[p]) && (p == 0 || scheme_delimiter(t[p - 1])) {
                    p0 = Some(p);
                }
            }
            Some(s) => {
                if t[p] == ':' && scheme_supported(&t[s..], schemes) {
                    return Ok((s - start, p - s + 1));
                }
                if !scheme_start(t[p]) {
                    p0 = None;
                }
            }
        }
    }
    Err(t.len().saturating_sub(start))
}

// Notepad_plus.cpp scanToUrlEnd.
fn scan_end(t: &[char], start: usize) -> usize {
    #[derive(PartialEq)]
    enum S {
        Path,
        Query,
        AfterDelim,
        Quotes,
        AfterQuotes,
        Fragment,
    }
    let mut s = S::Path;
    let mut q = '\0';
    for (p, &c) in t.iter().enumerate().skip(start) {
        let stop = match s {
            S::Path => {
                if c == '?' {
                    s = S::Query;
                } else if c == '#' {
                    s = S::Fragment;
                }
                s == S::Path && !text_char(c)
            }
            S::Query => {
                if c == '#' {
                    s = S::Fragment;
                } else if query_delimiter(c) {
                    s = S::AfterDelim;
                }
                s == S::Query && !text_char(c)
            }
            S::AfterDelim => {
                let close = match c {
                    '\'' | '"' | '`' => Some(c),
                    '(' => Some(')'),
                    '[' => Some(']'),
                    '{' => Some('}'),
                    _ => None,
                };
                if let Some(x) = close {
                    q = x;
                    s = S::Quotes;
                    false
                } else if text_char(c) {
                    s = S::Query;
                    false
                } else {
                    true
                }
            }
            S::Quotes => {
                if c == q {
                    s = S::AfterQuotes;
                }
                c < ' '
            }
            S::AfterQuotes => {
                s = S::AfterDelim;
                !query_delimiter(c)
            }
            S::Fragment => c != '?' && !text_char(c),
        };
        if stop {
            return p - start;
        }
    }
    t.len() - start
}

// Notepad_plus.cpp removeUnwantedTrailingCharFromUrl.
fn trim_one(t: &[char], len: &mut usize) -> bool {
    if *len < 2 {
        return false;
    }
    let l = *len - 1;
    if ".,:;?!#".contains(t[l]) {
        *len = l;
        return true;
    }
    for (close, open) in [(')', '('), (']', '[')] {
        if t[l] == close {
            let mut count = 0;
            for &c in t[..l].iter().rev() {
                if c == close {
                    count += 1;
                }
                if c == open {
                    if count == 0 {
                        return false;
                    }
                    count -= 1;
                }
            }
            if count != 0 {
                return false;
            }
            *len = l;
            return true;
        }
    }
    false
}

// Notepad_plus.cpp isUrl: (is a URL, segment length).
fn is_url(t: &[char], start: usize, schemes: &str) -> (bool, usize) {
    let (dist, scheme) = match scan_start(t, start, schemes) {
        Ok(x) => x,
        Err(d) => return (false, d),
    };
    if dist > 0 {
        return (false, dist);
    }
    let end = scan_end(t, start + scheme);
    if end > 0 {
        // ponytail: InternetCrackUrl reduced to "text after the slashes"; add host checks if false links show.
        let mut len = end + scheme;
        if t[start + scheme..start + len].iter().all(|&c| c == '/') {
            return (false, len);
        }
        for q in ['\'', '`'] {
            if start > 0 && t[start - 1] == q && t[start + len - 1] == q {
                len -= 1;
            }
        }
        while trim_one(&t[start..], &mut len) {}
        return (true, len);
    }
    let mut len = 1;
    while start + len < t.len() && scheme_start(t[start + len]) {
        len += 1;
    }
    (false, len)
}

// The byte ranges of the URLs in UTF-8 text.
pub fn find_urls(b: &[u8], schemes: &str) -> Vec<(usize, usize)> {
    let mut at = Vec::new();
    let mut off = 0;
    for ch in b.utf8_chunks() {
        at.extend(ch.valid().char_indices().map(|(i, c)| (off + i, c)));
        off += ch.valid().len();
        at.extend(ch.invalid().iter().map(|_| (off, '\u{FFFD}')));
        off += ch.invalid().len();
    }
    let t: Vec<char> = at.iter().map(|x| x.1).collect();
    let byte = |i: usize| at.get(i).map_or(b.len(), |x| x.0);
    let mut out = Vec::new();
    let mut start = 0;
    while start < t.len() {
        let (url, len) = is_url(&t, start, schemes);
        if len == 0 {
            break;
        }
        if url {
            out.push((byte(start), byte(start + len)));
        }
        start += len;
    }
    out
}

// The selection of a Ctrl double click in NppNotification.cpp, as (anchor, caret) in `b`.
pub fn delimiter_range(b: &[u8], click: usize, left: u8, right: u8) -> Option<(usize, usize)> {
    if left == right {
        if click >= b.len() {
            return None;
        }
        let escaped = |i: usize| left == b'"' && i > 0 && b[i - 1] == b'\\';
        let l = (0..=click).rev().find(|&i| b[i] == left && !escaped(i))?;
        let r = (click..b.len()).find(|&i| b[i] == right && !escaped(i))?;
        return Some((l + 1, r));
    }
    let mut stack = Vec::new();
    let mut best: Option<(usize, usize)> = None;
    for (i, &c) in b.iter().enumerate() {
        if c == left {
            stack.push(i);
        } else if c == right {
            if let Some(m) = stack.pop() {
                if m <= click && i >= click && best.is_none_or(|(l, _)| m > l) {
                    best = Some((m, i));
                }
            }
        }
    }
    best.map(|(l, r)| (l + 1, r))
}

// ScintillaEditView::setWordChars: the default list plus the added characters it lacks.
pub fn word_chars(default: &[u8], added: &str) -> Vec<u8> {
    let mut v = default.to_vec();
    v.extend(added.bytes().filter(|c| !default.contains(c)));
    v
}

// The Scintilla default word characters, which ScintillaEditView keeps as _defaultCharList.
pub fn default_word_chars() -> Vec<u8> {
    (1..=255u8).filter(|&c| c >= 0x80 || c.is_ascii_alphanumeric() || c == b'_').collect()
}

pub fn set_word_chars(v: &NSView) {
    let (use_default, added) = crate::prefs::with(|p| (p.word_char_default, p.word_chars_added.clone()));
    let d = default_word_chars();
    let mut list = if use_default { d } else { word_chars(&d, &added) };
    list.push(0);
    sci::send(v, SCI_SETWORDCHARS, 0, list.as_ptr() as isize);
}

// Notepad_plus::addHotSpot on the visible lines of one editor.
pub fn add_hot_spot(v: &NSView) {
    let s = |m: u32, w: usize, l: isize| sci::send(v, m, w, l);
    let style = crate::prefs::with(|p| p.url_style);
    let ind = if no_underline(style) {
        INDIC_HIDDEN
    } else {
        INDIC_PLAIN
    };
    let hover = if fullbox(style) {
        INDIC_FULLBOX
    } else {
        INDIC_EXPLORERLINK
    };
    if s(SCI_INDICGETSTYLE, URL_INDIC, 0) != ind || s(SCI_INDICGETHOVERSTYLE, URL_INDIC, 0) != hover
    {
        s(SCI_INDICSETSTYLE, URL_INDIC, ind);
        s(SCI_INDICSETHOVERSTYLE, URL_INDIC, hover);
        s(SCI_INDICSETALPHA, URL_INDIC, 70);
        s(SCI_INDICSETFLAGS, URL_INDIC, SC_INDICFLAG_VALUEFORE);
        let fg = crate::styler::cfg()
            .global_styles
            .iter()
            .find(|x| x.name == "URL hovered")
            .and_then(|x| x.fg);
        s(SCI_INDICSETHOVERFORE, URL_INDIC, fg.unwrap_or(0x808080));
    }
    let top = s(SCI_GETFIRSTVISIBLELINE, 0, 0);
    let first = s(SCI_DOCLINEFROMVISIBLE, top as usize, 0);
    let last = s(SCI_DOCLINEFROMVISIBLE, (top + s(SCI_LINESONSCREEN, 0, 0)) as usize, 0);
    let start = s(SCI_POSITIONFROMLINE, first as usize, 0);
    let end = s(SCI_GETLINEENDPOSITION, last as usize, 0);
    if start >= end {
        return;
    }
    s(SCI_SETINDICATORCURRENT, URL_INDIC, 0);
    s(SCI_INDICATORCLEARRANGE, start as usize, end - start);
    if style == 0 || !crate::large_file::allow_clickable_link(v) {
        return;
    }
    s(
        SCI_SETINDICATORVALUE,
        s(SCI_STYLEGETFORE, STYLE_DEFAULT, 0) as usize,
        0,
    );
    let schemes = crate::prefs::with(|p| p.uri_schemes.clone());
    for (a, b) in find_urls(&sci::doc(v).range(start, end), &schemes) {
        s(SCI_INDICATORFILLRANGE, start as usize + a, (b - a) as isize);
    }
}

fn url_at(v: &NSView, pos: isize) -> Option<String> {
    if pos < 0 || sci::send(v, SCI_INDICATORALLONFOR, pos as usize, 0) & (1 << URL_INDIC) == 0 {
        return None;
    }
    let a = sci::send(v, SCI_INDICATORSTART, URL_INDIC, pos);
    let b = sci::send(v, SCI_INDICATOREND, URL_INDIC, pos);
    (a <= pos && pos <= b).then(|| String::from_utf8_lossy(&sci::doc(v).range(a, b)).into_owned())
}

impl App {
    fn editor_from(&self, from: isize) -> Option<Retained<NSView>> {
        let tabs = self.ivars().tabs.borrow();
        tabs.iter()
            .find(|t| sci::send(&t.view, SCI_GETDIRECTPOINTER, 0, 0) == from)
            .map(|t| t.view.clone())
    }

    // SCN_UPDATEUI comes often, so the URL scan runs once the notifications stop.
    pub(crate) fn links_notify(&self, scn: *const c_void) {
        let n = unsafe { &*(scn as *const Notify) };
        if n.id_from == sci::RESULTS_ID {
            return;
        }
        if n.code == sci::SCN_DOUBLECLICK {
            self.links_double_click(n);
            return;
        }
        if n.code != sci::SCN_UPDATEUI {
            return;
        }
        let from = n.hwnd_from as isize;
        PENDING.with(|p| {
            let mut p = p.borrow_mut();
            if !p.contains(&from) {
                p.push(from);
            }
        });
        let none = None::<&AnyObject>;
        unsafe {
            let _: () = msg_send![class!(NSObject), cancelPreviousPerformRequestsWithTarget: self, selector: sel!(addHotSpot:), object: none];
            let modes = NSArray::from_slice(&[NSRunLoopCommonModes]);
            let _: () = msg_send![self, performSelector: sel!(addHotSpot:), withObject: none, afterDelay: 0.05f64, inModes: &*modes];
        }
    }

    pub(crate) fn add_hot_spots(&self) {
        for from in PENDING.with(|p| std::mem::take(&mut *p.borrow_mut())) {
            if let Some(v) = self.editor_from(from) {
                add_hot_spot(&v);
            }
        }
    }

    fn links_double_click(&self, n: &Notify) {
        let Some(v) = self.editor_from(n.hwnd_from as isize) else {
            return;
        };
        if n.modifiers == SCMOD_CTRL {
            self.delimiter_select(&v, n.position);
            return;
        }
        if crate::prefs::with(|p| p.url_style) == 0 || !crate::large_file::allow_clickable_link(&v)
        {
            return;
        }
        let Some(url) = url_at(&v, n.position) else {
            return;
        };
        sci::send(&v, SCI_SETSEL, n.position as usize, n.position);
        if let Some(u) = NSURL::URLWithString(&ns(&url)) {
            NSWorkspace::sharedWorkspace().openURL(&u);
        }
    }

    fn delimiter_select(&self, v: &NSView, pos: isize) {
        let (l, r, whole) = crate::prefs::with(|p| (p.delim_left, p.delim_right, p.delim_doc));
        let (Ok(l), Ok(r)) = (u8::try_from(l), u8::try_from(r)) else {
            return;
        };
        let pos = if pos < 0 { sci::selection(v).1 } else { pos };
        let (base, end) = if whole {
            (0, sci::length(v))
        } else {
            let line = sci::send(v, SCI_LINEFROMPOSITION, pos as usize, 0);
            (
                sci::send(v, SCI_POSITIONFROMLINE, line as usize, 0),
                sci::send(v, SCI_POSITIONFROMLINE, line as usize + 1, 0).max(pos),
            )
        };
        let b = sci::doc(v).range(base, end);
        if let Some((a, c)) = delimiter_range(&b, (pos - base) as usize, l, r) {
            sci::send(v, SCI_SETCURRENTPOS, base as usize + c, 0);
            sci::send(v, SCI_SETANCHOR, base as usize + a, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMES: &str = "svn:// ssh://";

    fn urls(s: &str) -> Vec<&str> {
        find_urls(s.as_bytes(), SCHEMES)
            .iter()
            .map(|&(a, b)| &s[a..b])
            .collect()
    }

    #[test]
    fn finds_urls_like_notepad_plus_plus() {
        assert_eq!(
            urls("see https://example.com/a?b=1&c=2#top."),
            ["https://example.com/a?b=1&c=2#top"]
        );
        assert_eq!(
            urls("(http://x.org/wiki/A_(b))"),
            ["http://x.org/wiki/A_(b)"]
        );
        assert_eq!(
            urls("'ftp://a.b/c' and `file:///tmp/x`"),
            ["ftp://a.b/c", "file:///tmp/x"]
        );
        assert_eq!(
            urls("mailto:me@x.org, SSH://host"),
            ["mailto:me@x.org", "SSH://host"]
        );
        assert_eq!(urls("xhttp://no gopher://no http://"), Vec::<&str>::new());
        assert_eq!(urls("é http://ü.de/ö end"), ["http://ü.de/ö"]);
        assert_eq!(urls("a <http://x.y/z> b"), ["http://x.y/z"]);
        assert_eq!(urls("http://q.x/?a='b c'&d"), ["http://q.x/?a='b c'&d"]);
    }

    #[test]
    fn url_style_round_trips() {
        for st in 0..=4 {
            assert_eq!(url_style(st != 0, no_underline(st), fullbox(st)), st);
        }
    }

    #[test]
    fn delimiter_selection() {
        let b = b"f(a, g(b), c)";
        assert_eq!(delimiter_range(b, 3, b'(', b')'), Some((2, 12)));
        assert_eq!(delimiter_range(b, 7, b'(', b')'), Some((7, 8)));
        assert_eq!(delimiter_range(b, 0, b'(', b')'), None);
        let q = br#"x = "a \" b" + "c""#;
        assert_eq!(delimiter_range(q, 6, b'"', b'"'), Some((5, 11)));
        assert_eq!(delimiter_range(q, 99, b'"', b'"'), None);
    }

    #[test]
    fn word_chars_add_only_new_chars() {
        let d = default_word_chars();
        assert_eq!(d.len(), 26 * 2 + 10 + 1 + 128);
        assert!(!d.contains(&b'-') && d.contains(&b'_') && d.contains(&0xE9));
        assert_eq!(word_chars(b"ab_", "-a$"), b"ab_-$");
        assert_eq!(word_chars(b"ab_", ""), b"ab_");
    }
}
