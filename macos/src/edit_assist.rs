// SPDX-License-Identifier: GPL-3.0-or-later
use crate::sci;
use crate::search::Doc;
use objc2_app_kit::NSView;
use std::cell::RefCell;

const SCI_INSERTTEXT: u32 = 2003;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GOTOPOS: u32 = 2025;
const SCI_GETSTYLEINDEXAT: u32 = 2038;
const SCI_BEGINUNDOACTION: u32 = 2078;
const SCI_ENDUNDOACTION: u32 = 2079;
const SCI_GETTABWIDTH: u32 = 2121;
const SCI_SETLINEINDENTATION: u32 = 2126;
const SCI_GETLINEINDENTPOSITION: u32 = 2128;
const SCI_GETCOLUMN: u32 = 2129;
const SCI_SETHIGHLIGHTGUIDE: u32 = 2134;
const SCI_GETINDENTATIONGUIDES: u32 = 2133;
const SCI_GETSELECTIONSTART: u32 = 2143;
const SCI_GETSELECTIONEND: u32 = 2145;
const SCI_SETSEL: u32 = 2160;
const SCI_LINEFROMPOSITION: u32 = 2166;
const SCI_BRACEHIGHLIGHT: u32 = 2351;
const SCI_BRACEBADLIGHT: u32 = 2352;
const SCI_BRACEMATCH: u32 = 2353;
const SCI_GETSELECTIONS: u32 = 2570;
const SCI_SETSELECTIONNSTART: u32 = 2584;
const SCI_GETSELECTIONNSTART: u32 = 2585;
const SCI_SETSELECTIONNEND: u32 = 2586;
const SCI_GETSELECTIONNEND: u32 = 2587;
const SCI_DELETERANGE: u32 = 2645;
const SC_EOL_CRLF: usize = 0;
const SC_EOL_CR: usize = 1;
const SCE_P_OPERATOR: isize = 10;
const SCE_HJ_START: isize = 40;
const SCFIND_REGEXP_POSIX: u32 = 0x0020_0000 | 0x0040_0000;
// AutoCompletion.h tagMaxLen.
const TAG_MAX_LEN: isize = 256;

// Preferences > Indentation > Auto-indent: NppGUI::_maintainIndent.
pub const INDENT_NONE: u8 = 0;
pub const INDENT_BASIC: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    CLike,
    // C-like languages without single line control structures: Perl, Rust, PowerShell, JSON, JSON5.
    CLikeBraces,
    Python,
    Other,
}

// The language groups of Notepad_plus::maintainIndentation.
pub fn kind(lang: &str) -> Kind {
    match lang {
        "perl" | "rust" | "powershell" | "json" | "json5" => Kind::CLikeBraces,
        "c" | "cpp" | "java" | "cs" | "objc" | "php" | "javascript" | "javascript.js" | "jsp"
        | "css" | "typescript" | "go" | "swift" => Kind::CLike,
        "python" => Kind::Python,
        _ => Kind::Other,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    InsertEol(isize, &'static [u8]),
    Indent(isize, isize),
}

fn at(d: &Doc, p: isize) -> u8 {
    if p < 0 || p >= d.len() {
        return 0;
    }
    d.range(p, p + 1).first().copied().unwrap_or(0)
}

fn line_len(d: &Doc, l: isize) -> isize {
    let (s, e) = d.line_span(l);
    e - s
}

// Document::GetLineIndentation.
pub fn line_indent(d: &Doc, l: isize, tab: isize) -> isize {
    let (s, e) = d.line_span(l);
    let mut n = 0;
    for c in d.range(s, e) {
        match c {
            b' ' => n += 1,
            b'\t' => n = (n / tab.max(1) + 1) * tab.max(1),
            _ => break,
        }
    }
    n
}

fn search(d: &Doc, from: isize, to: isize, re: &str) -> Option<(isize, isize)> {
    d.find(from, to, re.as_bytes(), SCFIND_REGEXP_POSIX)
        .ok()
        .flatten()
}

// Notepad_plus::isConditionExprLine.
fn is_condition_line(d: &Doc, l: isize) -> bool {
    if l < 0 || l >= d.lines() {
        return false;
    }
    let (s, e) = d.line_span(l);
    let re = "((else[ \t]+)?if|for|while)[ \t]*[(].*[)][ \t]*|else[ \t]*";
    search(d, s, e, re).is_some_and(|(_, end)| end == e)
}

// Notepad_plus::findMachedBracePos, backward only.
fn find_open_brace(d: &Doc, from: isize, to: isize) -> isize {
    if from <= to {
        return -1;
    }
    let mut balance = 0;
    let mut i = from;
    while i >= to {
        match at(d, i) {
            b'{' if balance == 0 => return i,
            b'{' => balance -= 1,
            b'}' => balance += 1,
            _ => {}
        }
        i -= 1;
    }
    -1
}

pub struct IndentCtx {
    pub mode: u8,
    pub kind: Kind,
    pub eol: usize,
    pub tab: isize,
}

// Notepad_plus::maintainIndentation: the line indentation changes after `ch` is typed at `caret`.
pub fn maintain_indent(
    d: &Doc,
    c: &IndentCtx,
    ch: u32,
    caret: isize,
    py_operator: &dyn Fn(isize) -> bool,
) -> Vec<Act> {
    let mut out = vec![];
    if c.mode == INDENT_NONE {
        return out;
    }
    let is_eol =
        (c.eol != SC_EOL_CR && ch == '\n' as u32) || (c.eol == SC_EOL_CR && ch == '\r' as u32);
    let cur = d.line_of(caret);
    let mut prev = cur - 1;
    let tab = c.tab;
    if is_eol && prev >= 0 && line_len(d, prev) == 0 {
        return out;
    }
    let skip_empty = |mut p: isize| {
        while p >= 0 && line_len(d, p) == 0 {
            p -= 1;
        }
        p
    };
    let prev_indent = |p: isize| if p >= 0 { line_indent(d, p, tab) } else { 0 };
    if c.mode == INDENT_BASIC || c.kind == Kind::Other {
        if is_eol {
            let amount = prev_indent(skip_empty(prev));
            if amount > 0 {
                out.push(Act::Indent(cur, amount));
            }
        }
        return out;
    }
    if c.kind == Kind::Python {
        if is_eol {
            prev = skip_empty(prev);
            let amount = prev_indent(prev);
            let (s, e) = d.line_span(prev.max(0));
            let colon = search(d, s, e, ":[ \t]*(#|$)").map(|(p, _)| p);
            if colon.is_some_and(py_operator) {
                out.push(Act::Indent(cur, amount + tab));
            } else if amount > 0 {
                out.push(Act::Indent(cur, amount));
            }
        }
        return out;
    }
    if is_eol {
        prev = skip_empty(prev);
        let amount = prev_indent(prev);
        let prev_char = at(d, caret - if c.eol == SC_EOL_CRLF { 3 } else { 2 });
        let next_char = at(d, caret);
        if prev_char == b'{' {
            if next_char == b'}' {
                let eol: &'static [u8] = match c.eol {
                    SC_EOL_CRLF => b"\r\n",
                    SC_EOL_CR => b"\r",
                    _ => b"\n",
                };
                out.push(Act::InsertEol(caret, eol));
                out.push(Act::Indent(cur + 1, amount));
            }
            out.push(Act::Indent(cur, amount + tab));
        } else if next_char == b'{' || c.kind == Kind::CLikeBraces {
            out.push(Act::Indent(cur, amount));
        } else if is_condition_line(d, prev) {
            out.push(Act::Indent(cur, amount + tab));
        } else if amount > 0 {
            if prev > 0 && is_condition_line(d, prev - 1) {
                out.push(Act::Indent(cur, amount - tab));
            } else {
                out.push(Act::Indent(cur, amount));
            }
        }
    } else if ch == '{' as u32 {
        let start = d.line_span(cur).0;
        let mut i = caret - 2;
        while i > 0 && i >= start {
            if !matches!(at(d, i), b' ' | b'\t') {
                return out;
            }
            i -= 1;
        }
        prev = skip_empty(prev);
        let mut amount = 0;
        if prev >= 0 {
            amount = line_indent(d, prev, tab);
            let (s, e) = d.line_span(prev);
            if search(d, s, e, "[ \t]*\\{.*").is_some_and(|(_, end)| end == e) {
                amount += tab;
            }
        }
        out.push(Act::Indent(cur, amount));
    } else if ch == '}' as u32 {
        let start = if caret != 0 { caret - 1 } else { caret };
        let pos = find_open_brace(d, start - 1, 0);
        if pos == -1 {
            return out;
        }
        let line = d.line_of(pos);
        if line == cur {
            return out;
        }
        out.push(Act::Indent(cur, line_indent(d, line, tab)));
    }
    out
}

// ScintillaEditView::setLineIndent: set the indentation and keep the selections at the same text.
fn set_line_indent(v: &NSView, line: isize, indent: isize) {
    let indent = indent.max(0);
    let shift = |p: isize, before: isize, after: isize| {
        if after > before && p >= before {
            p + after - before
        } else if after < before && p >= after {
            if p >= before {
                p + after - before
            } else {
                after
            }
        } else {
            p
        }
    };
    let n = sci::send(v, SCI_GETSELECTIONS, 0, 0);
    if n == 1 {
        let s = sci::send(v, SCI_GETSELECTIONSTART, 0, 0);
        let e = sci::send(v, SCI_GETSELECTIONEND, 0, 0);
        let before = sci::send(v, SCI_GETLINEINDENTPOSITION, line as usize, 0);
        sci::send(v, SCI_SETLINEINDENTATION, line as usize, indent);
        let after = sci::send(v, SCI_GETLINEINDENTPOSITION, line as usize, 0);
        sci::send(
            v,
            SCI_SETSEL,
            shift(s, before, after) as usize,
            shift(e, before, after),
        );
        return;
    }
    sci::send(v, SCI_BEGINUNDOACTION, 0, 0);
    for i in 0..n.max(0) as usize {
        let s = sci::send(v, SCI_GETSELECTIONNSTART, i, 0);
        let e = sci::send(v, SCI_GETSELECTIONNEND, i, 0);
        let l = sci::send(v, SCI_LINEFROMPOSITION, s as usize, 0);
        let before = sci::send(v, SCI_GETLINEINDENTPOSITION, l as usize, 0);
        sci::send(v, SCI_SETLINEINDENTATION, l as usize, indent);
        let after = sci::send(v, SCI_GETLINEINDENTPOSITION, l as usize, 0);
        sci::send(v, SCI_SETSELECTIONNSTART, i, shift(s, before, after));
        sci::send(v, SCI_SETSELECTIONNEND, i, shift(e, before, after));
    }
    sci::send(v, SCI_ENDUNDOACTION, 0, 0);
}

// MatchedPairConf of Preferences > Auto-Completion > Auto-Insert.
#[derive(Debug, Clone, Default)]
pub struct Pairs {
    pub user: Vec<(u8, u8)>,
    pub parentheses: bool,
    pub brackets: bool,
    pub curly: bool,
    pub quotes: bool,
    pub double_quotes: bool,
    pub tag: bool,
}

impl Pairs {
    pub fn any(&self) -> bool {
        !self.user.is_empty()
            || self.parentheses
            || self.brackets
            || self.curly
            || self.quotes
            || self.double_quotes
            || self.tag
    }
}

// InsertedMatchedChars: the opening characters typed with an automatic closing character, and their positions.
#[derive(Debug, Default)]
pub struct Inserted(Vec<(u8, isize)>);

impl Inserted {
    fn remove_invalid(&mut self, d: &Doc, c: u8, pos: isize) {
        if c == b'\n' || c == b'\r' {
            self.0.clear();
            return;
        }
        let line = d.line_of(pos);
        self.0.retain(|&(_, p)| p < pos && d.line_of(p) == line);
    }

    fn add(&mut self, d: &Doc, c: u8, pos: isize) {
        self.remove_invalid(d, c, pos);
        self.0.push((c, pos));
    }

    // InsertedMatchedChars::search: the position of `end` after spaces from `pos`, or -1.
    fn search(&mut self, d: &Doc, start: u8, end: u8, pos: isize) -> isize {
        let line = d.line_of(pos);
        let mut i = self.0.len();
        while i > 0 {
            i -= 1;
            let (c, p) = self.0[i];
            if c != start {
                continue;
            }
            self.0.remove(i);
            if p >= pos || d.line_of(p) != line {
                continue;
            }
            let line_end = d.line_span(line).1;
            for j in pos..=line_end {
                match at(d, j) {
                    b' ' => continue,
                    x if x == end => return j,
                    _ => return -1,
                }
            }
        }
        -1
    }
}

#[derive(Debug, PartialEq)]
pub enum Typed {
    Nothing,
    Insert(Vec<u8>),
    // Delete the closing character at this position and put the caret there.
    Skip(isize),
}

// AutoCompletion::getCloseTag: the close tag for the tag that `>` at `caret - 1` ends.
pub fn close_tag(d: &Doc, caret: isize, is_html: bool) -> Option<Vec<u8>> {
    let prev = at(d, caret - 2);
    if (at(d, caret - 3) == b'-' && prev == b'-') || prev == b'/' {
        return None;
    }
    let (s, e) = search(d, caret, 0, "<[^\\s>]*")?;
    if e - s < 2 || e - s > TAG_MAX_LEN - 2 {
        return None;
    }
    let head = d.range(s, e);
    if matches!(head[1], b'/' | b'?') || head.starts_with(b"<!--") {
        return None;
    }
    const VOID: [&str; 16] = [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "keygen", "link", "meta",
        "param", "source", "track", "wbr", "!doctype",
    ];
    let name = &head[1..];
    if is_html
        && VOID
            .iter()
            .any(|t| name.len() >= t.len() && name[..t.len()].eq_ignore_ascii_case(t.as_bytes()))
    {
        return None;
    }
    let mut t = b"</".to_vec();
    t.extend_from_slice(name);
    t.push(b'>');
    Some(t)
}

// AutoCompletion::insertMatchedChars for `ch` typed before `caret`; `tag` is Some(is_html) in HTML and XML.
pub fn matched_chars(
    d: &Doc,
    st: &mut Inserted,
    p: &Pairs,
    ch: u32,
    caret: isize,
    tag: Option<bool>,
) -> Typed {
    if ch > 127 {
        if !st.0.is_empty() {
            st.remove_invalid(d, 0, caret - 1);
        }
        return Typed::Nothing;
    }
    let ch = ch as u8;
    let prev = at(d, caret - 2);
    let next = at(d, caret);
    let prev_blank = matches!(prev, b' ' | b'\t' | b'\n' | b'\r' | 0);
    let next_blank = matches!(next, b' ' | b'\t' | b'\n' | b'\r') || caret == d.len();
    let next_close = matches!(next, b')' | b']' | b'}');
    let sandwich = matches!((prev, next), (b'(', b')') | (b'[', b']') | (b'{', b'}'));
    if let Some(&(_, close)) = p.user.iter().find(|(o, _)| *o == ch && next_blank) {
        return Typed::Insert(vec![close]);
    }
    let add = |st: &mut Inserted, s: &'static [u8]| {
        st.add(d, ch, caret - 1);
        Typed::Insert(s.to_vec())
    };
    match ch {
        b'(' | b'[' | b'{' => {
            let (on, s): (bool, &'static [u8]) = match ch {
                b'(' => (p.parentheses, b")"),
                b'[' => (p.brackets, b"]"),
                _ => (p.curly, b"}"),
            };
            if on && (next_blank || next_close) {
                return add(st, s);
            }
            Typed::Nothing
        }
        b'"' | b'\'' => {
            let (on, s): (bool, &'static [u8]) = if ch == b'"' {
                (p.double_quotes, b"\"")
            } else {
                (p.quotes, b"'")
            };
            if !on {
                return Typed::Nothing;
            }
            if !st.0.is_empty() {
                let pos = st.search(d, ch, ch, caret);
                if pos != -1 {
                    return Typed::Skip(pos);
                }
            }
            let ok = (prev_blank && next_blank)
                || sandwich
                || (prev == b'(' && next_blank)
                || (prev_blank && next == b')')
                || (prev == b'[' && next_blank)
                || (prev_blank && next == b']')
                || (prev == b'{' && next_blank)
                || (prev_blank && next == b'}');
            if ok {
                return add(st, s);
            }
            Typed::Nothing
        }
        b'>' => match tag {
            Some(html) if p.tag => close_tag(d, caret, html).map_or(Typed::Nothing, Typed::Insert),
            _ => Typed::Nothing,
        },
        b')' | b']' | b'}' => {
            if st.0.is_empty() {
                return Typed::Nothing;
            }
            let (on, start) = match ch {
                b')' => (p.parentheses, b'('),
                b']' => (p.brackets, b'['),
                _ => (p.curly, b'{'),
            };
            if !on {
                return Typed::Nothing;
            }
            match st.search(d, start, ch, caret) {
                -1 => Typed::Nothing,
                pos => Typed::Skip(pos),
            }
        }
        _ => {
            if !st.0.is_empty() {
                st.remove_invalid(d, ch, caret - 1);
            }
            Typed::Nothing
        }
    }
}

// Parameters.cpp feedGUIParameters "auto-insert": the UserDefinePair elements with open and close in 0..=127.
pub fn user_pairs(xml: &str) -> Vec<(u8, u8)> {
    let path = [
        crate::prefs::GUI_PATH[0],
        crate::prefs::GUI_PATH[1],
        "GUIConfig",
    ];
    let num = |e: &crate::prefs::Elem, k: &str| {
        e.attrs
            .iter()
            .find(|(a, _)| a == k)
            .and_then(|(_, v)| v.trim().parse::<i64>().ok())
            .filter(|n| (0..=127).contains(n))
            .map(|n| n as u8)
    };
    crate::prefs::read_elems(xml, &path, "UserDefinePair")
        .iter()
        .filter_map(|e| Some((num(e, "open")?, num(e, "close")?)))
        .collect()
}

thread_local! {
    static STATE: RefCell<(usize, Inserted)> = RefCell::default();
    static USER_PAIRS: Vec<(u8, u8)> = crate::session::read_config().map_or_else(Vec::new, |x| user_pairs(&x));
}

// NppNotification.cpp SCN_CHARADDED: maintainIndentation, then insertMatchedChars when one selection shows.
pub fn char_added(v: &NSView, ch: u32, lang: &str) {
    let (mode, auto) = crate::prefs::with(|p| (p.auto_indent(), p.auto_insert()));
    if mode != INDENT_NONE {
        let c = IndentCtx {
            mode,
            kind: kind(lang),
            eol: sci::eol_mode(v),
            tab: sci::send(v, SCI_GETTABWIDTH, 0, 0),
        };
        let caret = sci::send(v, SCI_GETCURRENTPOS, 0, 0);
        let op = |p: isize| sci::send(v, SCI_GETSTYLEINDEXAT, p as usize, 0) == SCE_P_OPERATOR;
        let acts = maintain_indent(&sci::doc(v), &c, ch, caret, &op);
        for a in acts {
            match a {
                Act::InsertEol(p, eol) => {
                    let mut z = eol.to_vec();
                    z.push(0);
                    sci::send(v, SCI_INSERTTEXT, p as usize, z.as_ptr() as isize);
                }
                Act::Indent(l, n) => set_line_indent(v, l, n),
            }
        }
    }
    let pairs = Pairs {
        user: USER_PAIRS.with(Vec::clone),
        parentheses: auto.parentheses,
        brackets: auto.brackets,
        curly: auto.curly_brackets,
        quotes: auto.quotes,
        double_quotes: auto.double_quotes,
        tag: auto.html_xml_tag,
    };
    if !pairs.any() || sci::send(v, SCI_GETSELECTIONS, 0, 0) > 1 {
        return;
    }
    let caret = sci::send(v, SCI_GETCURRENTPOS, 0, 0);
    let tag = match lang {
        "html" => {
            (sci::send(v, SCI_GETSTYLEINDEXAT, caret as usize, 0) < SCE_HJ_START).then_some(true)
        }
        "xml" => Some(false),
        _ => None,
    };
    let id = v as *const NSView as usize;
    let typed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.0 != id {
            *s = (id, Inserted::default());
        }
        matched_chars(&sci::doc(v), &mut s.1, &pairs, ch, caret, tag)
    });
    match typed {
        Typed::Nothing => {}
        Typed::Insert(mut t) => {
            t.push(0);
            sci::send(v, SCI_INSERTTEXT, caret as usize, t.as_ptr() as isize);
        }
        Typed::Skip(p) => {
            sci::send(v, SCI_DELETERANGE, p as usize, 1);
            sci::send(v, SCI_GOTOPOS, p as usize, 0);
        }
    }
}

// Notepad_plus::braceMatch on SCN_UPDATEUI.
pub fn brace_match(v: &NSView) {
    let caret = sci::send(v, SCI_GETCURRENTPOS, 0, 0);
    let d = sci::doc(v);
    let is_brace = |c: u8| matches!(c, b'[' | b']' | b'(' | b')' | b'{' | b'}');
    let mut at_caret = -1;
    if d.len() > 0 && caret > 0 && is_brace(at(&d, caret - 1)) {
        at_caret = caret - 1;
    }
    if d.len() > 0 && at_caret < 0 && is_brace(at(&d, caret)) {
        at_caret = caret;
    }
    drop(d);
    let opposite = if at_caret >= 0 {
        sci::send(v, SCI_BRACEMATCH, at_caret as usize, 0)
    } else {
        -1
    };
    if at_caret != -1 && opposite == -1 {
        sci::send(v, SCI_BRACEBADLIGHT, at_caret as usize, 0);
        sci::send(v, SCI_SETHIGHLIGHTGUIDE, 0, 0);
        return;
    }
    sci::send(v, SCI_BRACEHIGHLIGHT, at_caret as usize, opposite);
    if sci::send(v, SCI_GETINDENTATIONGUIDES, 0, 0) != 0 {
        let a = sci::send(v, SCI_GETCOLUMN, at_caret as usize, 0);
        let b = sci::send(v, SCI_GETCOLUMN, opposite as usize, 0);
        sci::send(v, SCI_SETHIGHLIGHTGUIDE, a.min(b) as usize, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LF: usize = 2;

    fn ctx(mode: u8, kind: Kind) -> IndentCtx {
        IndentCtx {
            mode,
            kind,
            eol: LF,
            tab: 4,
        }
    }

    // The actions after `ch` is typed at the `|` of `text`.
    fn indent(text: &str, ch: char, c: &IndentCtx, op: bool) -> Vec<Act> {
        let caret = text.find('|').unwrap() as isize;
        let d = Doc::new(text.replace('|', "").as_bytes()).unwrap();
        maintain_indent(&d, c, ch as u32, caret, &|_| op)
    }

    #[test]
    fn basic_keeps_previous_indent() {
        let c = ctx(INDENT_BASIC, Kind::CLike);
        assert_eq!(
            indent("    if (a) {\n|", '\n', &c, false),
            [Act::Indent(1, 4)]
        );
        assert_eq!(indent("\tx\n|", '\n', &c, false), [Act::Indent(1, 4)]);
        assert_eq!(indent("x\n|", '\n', &c, false), []);
        assert_eq!(indent("    x\n\n|", '\n', &c, false), []);
        assert_eq!(indent("    x\n|", 'a', &c, false), []);
        assert_eq!(
            indent("    x\n|", '\n', &ctx(INDENT_NONE, Kind::CLike), false),
            []
        );
    }

    #[test]
    fn c_like_enter() {
        let c = ctx(1, Kind::CLike);
        assert_eq!(indent("  f() {\n|", '\n', &c, false), [Act::Indent(1, 6)]);
        assert_eq!(
            indent("  f() {\n|}", '\n', &c, false),
            [
                Act::InsertEol(8, b"\n"),
                Act::Indent(2, 2),
                Act::Indent(1, 6)
            ]
        );
        assert_eq!(indent("  if (a)\n|", '\n', &c, false), [Act::Indent(1, 6)]);
        assert_eq!(indent("  } else\n|", '\n', &c, false), [Act::Indent(1, 6)]);
        assert_eq!(indent("  IF (a) \n|", '\n', &c, false), [Act::Indent(1, 6)]);
        assert_eq!(
            indent("  if (a) b();\n|", '\n', &c, false),
            [Act::Indent(1, 2)]
        );
        assert_eq!(
            indent("if (a)\n    b();\n|", '\n', &c, false),
            [Act::Indent(2, 0)]
        );
        assert_eq!(indent("  x;\n|{", '\n', &c, false), [Act::Indent(1, 2)]);
        let r = ctx(1, Kind::CLikeBraces);
        assert_eq!(indent("  if (a)\n|", '\n', &r, false), [Act::Indent(1, 2)]);
        let crlf = IndentCtx {
            eol: SC_EOL_CRLF,
            ..ctx(1, Kind::CLike)
        };
        assert_eq!(indent("{\r\n|", '\n', &crlf, false), [Act::Indent(1, 4)]);
    }

    #[test]
    fn c_like_braces() {
        let c = ctx(1, Kind::CLike);
        assert_eq!(
            indent("  if (a)\n  {|", '{', &c, false),
            [Act::Indent(1, 2)]
        );
        assert_eq!(indent("  f() {\n{|", '{', &c, false), [Act::Indent(1, 6)]);
        assert_eq!(indent("  x\n a{|", '{', &c, false), []);
        assert_eq!(
            indent("  f() {\n      x;\n      }|", '}', &c, false),
            [Act::Indent(2, 2)]
        );
        assert_eq!(indent("  f() { }|", '}', &c, false), []);
        assert_eq!(indent("  x\n  }|", '}', &c, false), []);
    }

    #[test]
    fn python_colon() {
        let c = ctx(1, Kind::Python);
        assert_eq!(indent("  if a:\n|", '\n', &c, true), [Act::Indent(1, 6)]);
        assert_eq!(
            indent("  if a:  # x\n|", '\n', &c, true),
            [Act::Indent(1, 6)]
        );
        assert_eq!(
            indent("  s = 'a:'\n|", '\n', &c, false),
            [Act::Indent(1, 2)]
        );
        assert_eq!(indent("  a[1:2]\n|", '\n', &c, true), [Act::Indent(1, 2)]);
        assert_eq!(indent("x\n|", '\n', &c, false), []);
    }

    #[test]
    fn language_groups() {
        assert_eq!(kind("cpp"), Kind::CLike);
        assert_eq!(kind("javascript.js"), Kind::CLike);
        assert_eq!(kind("rust"), Kind::CLikeBraces);
        assert_eq!(kind("python"), Kind::Python);
        assert_eq!(kind("html"), Kind::Other);
    }

    fn tag(text: &str, html: bool) -> Option<String> {
        let caret = text.len() as isize;
        let d = Doc::new(text.as_bytes()).unwrap();
        close_tag(&d, caret, html).map(|t| String::from_utf8(t).unwrap())
    }

    #[test]
    fn close_tags() {
        assert_eq!(tag("<div>", true).as_deref(), Some("</div>"));
        assert_eq!(tag("x <a href=\"y\">", true).as_deref(), Some("</a>"));
        assert_eq!(tag("<ns:item>", false).as_deref(), Some("</ns:item>"));
        assert_eq!(tag("<br>", true), None);
        assert_eq!(tag("<br>", false).as_deref(), Some("</br>"));
        assert_eq!(tag("<colgroup>", true), None);
        assert_eq!(tag("<IMG src=x>", true), None);
        assert_eq!(tag("<!DOCTYPE html>", true), None);
        assert_eq!(tag("<a/>", true), None);
        assert_eq!(tag("<!-- x -->", true), None);
        assert_eq!(tag("</a>", true), None);
        assert_eq!(tag("<?xml?>", false), None);
        assert_eq!(tag("a <>", false), None);
        assert_eq!(tag("text>", false), None);
    }

    fn pairs() -> Pairs {
        Pairs {
            user: vec![(b'*', b'*')],
            parentheses: true,
            brackets: true,
            curly: true,
            quotes: true,
            double_quotes: true,
            tag: true,
        }
    }

    // Types `keys` one by one, like Scintilla before SCN_CHARADDED, and runs matched_chars.
    fn typed(start: &str, keys: &str, p: &Pairs) -> String {
        let mut text = start.replace('|', "").into_bytes();
        let mut caret = start.find('|').unwrap();
        let mut st = Inserted::default();
        for k in keys.bytes() {
            text.insert(caret, k);
            caret += 1;
            let d = Doc::new(&text).unwrap();
            match matched_chars(&d, &mut st, p, k as u32, caret as isize, Some(true)) {
                Typed::Nothing => {}
                Typed::Insert(t) => {
                    text.splice(caret..caret, t);
                }
                Typed::Skip(pos) => {
                    text.remove(pos as usize);
                    caret = pos as usize;
                }
            }
        }
        text.insert(caret, b'|');
        String::from_utf8(text).unwrap()
    }

    #[test]
    fn matched_pairs() {
        let p = pairs();
        assert_eq!(typed("|", "(", &p), "(|)");
        assert_eq!(typed("|", "()", &p), "()|");
        assert_eq!(typed("|", "( )", &p), "( )|");
        assert_eq!(typed("|", "f(a[1])", &p), "f(a[1])|");
        assert_eq!(typed("|x", "(", &p), "(|x");
        assert_eq!(typed("|", "\"a\"", &p), "\"a\"|");
        assert_eq!(typed("a|", "'", &p), "a'|");
        assert_eq!(typed("|", "('", &p), "('|')");
        assert_eq!(typed("|", "{\n}", &p), "{\n}|}");
        assert_eq!(typed("|", "*", &p), "*|*");
        assert_eq!(typed("|x", "*", &p), "*|x");
        assert_eq!(typed("|", "<p>", &p), "<p>|</p>");
        let off = Pairs {
            parentheses: false,
            ..pairs()
        };
        assert_eq!(typed("|", "(", &off), "(|");
        assert_eq!(typed("|", "[)", &off), "[)|]");
        assert!(!Pairs::default().any());
    }

    #[test]
    fn user_pairs_from_config() {
        let xml = r#"<NotepadPlus><GUIConfigs><GUIConfig name="auto-insert" parentheses="no">
            <UserDefinePair open="42" close="42" /><UserDefinePair open="300" close="1" />
            <UserDefinePair open="&lt;" close="&gt;" /></GUIConfig></GUIConfigs></NotepadPlus>"#;
        assert_eq!(user_pairs(xml), [(b'*', b'*')]);
    }
}
