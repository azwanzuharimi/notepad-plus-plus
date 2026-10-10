// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::Language;
use crate::search::Doc;

const SCFIND_WORDSTART: u32 = 0x0010_0000;

// Anchor and caret.
pub type Sel = (isize, isize);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cmd {
    Toggle,
    Comment,
    Uncomment,
    Stream,
    StreamUncomment,
}

pub const CMDS: [(&str, Cmd); 5] = [
    ("Toggle Single Line Comment", Cmd::Toggle),
    ("Single Line Comment", Cmd::Comment),
    ("Single Line Uncomment", Cmd::Uncomment),
    ("Block Comment", Cmd::Stream),
    ("Block Uncomment", Cmd::StreamUncomment),
];

// Runs one Comment/Uncomment command as one undo step; None when the text and selection stay the same.
pub fn run(d: &Doc, l: &Language, cmd: Cmd, sel: Sel) -> Option<Sel> {
    if d.read_only() {
        return None;
    }
    d.undo_group(true);
    let r = match cmd {
        Cmd::Stream => stream(d, l, sel),
        Cmd::StreamUncomment => stream_uncomment(d, l, true, sel),
        c => block(d, l, c, sel),
    };
    d.undo_group(false);
    r
}

// Notepad++ reads SCI_GETSELECTIONSTART/END (a rectangle gives its first and last line) and only checks if the caret is before the end.
pub fn from_limits(start: isize, end: isize, caret: isize) -> Sel {
    if caret < end {
        (end, start)
    } else {
        (start, end)
    }
}

fn bounds((anchor, caret): Sel) -> (isize, isize, bool) {
    let (s, e) = (anchor.min(caret), anchor.max(caret));
    (s, e, caret < e)
}

fn result(s: isize, e: isize, move_caret: bool) -> Sel {
    if move_caret {
        (e, s)
    } else {
        (s, e)
    }
}

fn indent(d: &Doc, ls: isize, le: isize) -> isize {
    ls + d
        .range(ls, le)
        .iter()
        .take_while(|&&c| c == b' ' || c == b'\t')
        .count() as isize
}

// Port of Notepad_plus::doBlockComment.
fn block(d: &Doc, l: &Language, mode: Cmd, sel: Sel) -> Option<Sel> {
    let adv = if !l.comment_line.is_empty() {
        false
    } else if l.comment_start.is_empty() || l.comment_end.is_empty() {
        return None;
    } else if mode == Cmd::Uncomment {
        return stream_uncomment(d, l, false, sel);
    } else {
        true
    };
    let baanc = l.name == "baanc";
    let comment = if baanc {
        l.comment_line.clone()
    } else {
        format!("{} ", l.comment_line)
    };
    let cl = comment.len() as isize;
    let (st, et) = (l.comment_start.as_bytes(), l.comment_end.as_bytes());
    let adv_start = format!("{} ", l.comment_start);
    let adv_end = format!(" {}", l.comment_end);
    let (asl, ael) = (adv_start.len() as isize, adv_end.len() as isize);
    let (mut s, mut e, move_caret) = bounds(sel);
    let first = d.line_of(s);
    let mut last = d.line_of(e);
    if last > first && e == d.line_span(last).0 {
        last -= 1;
    }
    let avoid_indent = baanc || l.name == "fortran77";
    let mut uncommented = 0;
    for i in first..=last {
        let (ls, le) = d.line_span(i);
        let mut li = indent(d, ls, le);
        if li == le && !baanc {
            continue;
        }
        if avoid_indent {
            li = ls;
        }
        let text = d.range(li, le);
        if mode != Cmd::Comment && !adv {
            let n = (if baanc { cl } else { cl - 1 }) as usize;
            if text.len() >= n && text[..n].eq_ignore_ascii_case(&comment.as_bytes()[..n]) {
                let len = if baanc || text.get(cl as usize - 1) == Some(&b' ') {
                    cl
                } else {
                    cl - 1
                };
                d.replace(li, len, b"", false);
                if i == first {
                    if s > li + len {
                        s -= len;
                    } else if s > li {
                        s = li;
                    }
                }
                if i == last {
                    if e > li + len {
                        e -= len;
                    } else if e > li {
                        e = li;
                        if li == ls && i != first {
                            e += 1;
                        }
                    }
                } else {
                    e -= len;
                }
                uncommented += 1;
                continue;
            }
        }
        if mode != Cmd::Comment && adv {
            let (sn, en) = (st.len(), et.len());
            if text.len() >= sn + en
                && text[..sn].eq_ignore_ascii_case(st)
                && text[text.len() - en..].eq_ignore_ascii_case(et)
            {
                let start_len = if text[sn] == b' ' { asl } else { asl - 1 };
                let end_len = if text[text.len() - en - 1] == b' ' {
                    ael
                } else {
                    ael - 1
                }
                .min(text.len() as isize - start_len);
                d.replace(le - end_len, end_len, b"", false);
                d.replace(li, start_len, b"", false);
                let both = start_len + end_len;
                if i == first {
                    if s > le - end_len {
                        s = le - both;
                    } else if s > li + start_len {
                        s -= start_len;
                    } else if s > li {
                        s = li;
                    }
                }
                if i == last {
                    if e > le {
                        e -= both;
                    } else if e > le - end_len {
                        e = le - both;
                    } else if e > li + start_len {
                        e -= start_len;
                    } else if e > li {
                        e = li;
                        if li == ls && i != first {
                            e += 1;
                        }
                    }
                } else {
                    e -= both;
                }
                uncommented += 1;
                continue;
            }
        }
        if mode == Cmd::Uncomment {
            continue;
        }
        if !adv {
            d.replace(li, 0, comment.as_bytes(), false);
            if i == first && s >= li {
                s += cl;
            }
            if i != last || e >= li {
                e += cl;
            }
        } else {
            d.replace(le, 0, adv_end.as_bytes(), false);
            d.replace(li, 0, adv_start.as_bytes(), false);
            if i == first && s >= li {
                s += asl;
            }
            if i != last || e > le {
                e += asl + ael;
            } else if e >= li {
                e += asl;
            }
        }
    }
    let out = result(s, e, move_caret);
    if mode == Cmd::Uncomment && uncommented == 0 {
        return stream_uncomment(d, l, false, out);
    }
    Some(out)
}

// Port of Notepad_plus::doStreamComment.
fn stream(d: &Doc, l: &Language, sel: Sel) -> Option<Sel> {
    if l.comment_start.is_empty() || l.comment_end.is_empty() {
        return if l.comment_line.is_empty() {
            None
        } else {
            block(d, l, Cmd::Comment, sel)
        };
    }
    let start = format!("{} ", l.comment_start);
    let end = format!(" {}", l.comment_end);
    let (mut s, mut e, move_caret) = bounds(sel);
    if e <= s {
        let (ls, le) = d.line_span(d.line_of(s));
        s = indent(d, ls, le);
        e = le;
    }
    d.replace(s, 0, start.as_bytes(), false);
    s += start.len() as isize;
    e += start.len() as isize;
    d.replace(e, 0, end.as_bytes(), false);
    Some(result(s, e, move_caret))
}

// Port of Notepad_plus::undoStreamComment: removes each stream comment around or inside the selection.
fn stream_uncomment(d: &Doc, l: &Language, try_block: bool, sel: Sel) -> Option<Sel> {
    if l.comment_start.is_empty() || l.comment_end.is_empty() {
        return if !l.comment_line.is_empty() && try_block {
            block(d, l, Cmd::Uncomment, sel)
        } else {
            None
        };
    }
    let (st, et) = (l.comment_start.as_bytes(), l.comment_end.as_bytes());
    let find = |tok: &[u8], from: isize, to: isize| {
        d.find(from, to, tok, SCFIND_WORDSTART)
            .ok()
            .flatten()
            .map(|m| m.0)
    };
    let mut sel = sel;
    let mut changed = false;
    loop {
        let (s, e, move_caret) = bounds(sel);
        let len = d.len();
        let probe = |p: isize| {
            (
                find(st, p, 0),
                find(et, p, 0),
                find(st, p, len),
                find(et, p, len),
            )
        };
        let around = |(sb, eb, sa, ea): (
            Option<isize>,
            Option<isize>,
            Option<isize>,
            Option<isize>,
        )| {
            match (sb, ea) {
                (Some(sb), Some(ea))
                    if eb.is_none_or(|eb| sb >= eb) && sa.is_none_or(|sa| ea <= sa) =>
                {
                    Some((sb, ea))
                }
                _ => None,
            }
        };
        let at_start = probe(s);
        let found = around(at_start).or_else(|| {
            let at_end = probe(e);
            around(at_end).or(match (at_start.2, at_end.1, at_start.3) {
                (Some(sa), Some(eb), Some(ea)) if sa < e && eb > s => Some((sa, ea)),
                _ => None,
            })
        });
        let Some((ps, mut pe)) = found else {
            return changed.then_some(sel);
        };
        let mut sl = st.len() as isize;
        let mut el = et.len() as isize;
        if pe > 0 && d.range(pe - 1, pe) == b" " {
            el += 1;
            pe -= 1;
        }
        d.replace(pe, el, b"", false);
        if ps + sl < d.len() && d.range(ps + sl, ps + sl + 1) == b" " {
            sl += 1;
        }
        d.replace(ps, sl, b"", false);
        if d.len() >= len {
            return changed.then_some(sel);
        }
        changed = true;
        let sm = if s <= ps {
            0
        } else if s >= ps + sl {
            -sl
        } else {
            -(s - ps)
        };
        let em = if e >= pe + el {
            -(sl + el)
        } else if e <= pe {
            -sl
        } else {
            -(sl + e - pe)
        };
        sel = result((s + sm).max(0), (e + em).max(0), move_caret);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load;

    // Runs a command on text where '~' is the anchor and '^' is the caret; returns the text with the new marks.
    fn go(lang: &str, cmd: Cmd, marked: &str) -> String {
        let c = load();
        let l = c.languages.iter().find(|l| l.name == lang).unwrap();
        let (text, sel) = unmark(marked);
        let d = Doc::new(text.as_bytes()).unwrap();
        let out = run(&d, l, cmd, sel).unwrap_or(sel);
        mark(&String::from_utf8(d.text()).unwrap(), out)
    }

    fn unmark(m: &str) -> (String, Sel) {
        let c = m.find('^').unwrap();
        let a = m.find('~').unwrap_or(c);
        let pos = |p: usize, other: usize| (p - (other < p) as usize) as isize;
        let text = m.replace(['^', '~'], "");
        (text, (pos(a, c), pos(c, a)))
    }

    fn mark(text: &str, (a, c): Sel) -> String {
        let mut t = text.to_string();
        if a == c {
            t.insert(c as usize, '^');
        } else if a < c {
            t.insert(c as usize, '^');
            t.insert(a as usize, '~');
        } else {
            t.insert(a as usize, '~');
            t.insert(c as usize, '^');
        }
        t
    }

    #[test]
    fn selection_limits_keep_caret_side() {
        for (s, e, c) in [(2, 9, 9), (2, 9, 2), (2, 9, 5), (4, 4, 4)] {
            assert_eq!(bounds(from_limits(s, e, c)), (s, e, c < e));
        }
    }

    #[test]
    fn marks_round_trip() {
        for m in ["ab^c", "a~bc^d", "a^bc~d", "^"] {
            let (t, s) = unmark(m);
            assert_eq!(mark(&t, s), m);
        }
    }

    #[test]
    fn comment_keeps_indent_and_skips_empty_lines() {
        assert_eq!(
            go("cpp", Cmd::Comment, "~  a;\n\n\tb;\n^"),
            "~  // a;\n\n\t// b;\n^"
        );
    }

    #[test]
    fn selection_end_at_column_zero_skips_that_line() {
        assert_eq!(go("python", Cmd::Comment, "~a\n^b\n"), "# ~a\n^b\n");
        assert_eq!(go("python", Cmd::Comment, "a^\nb\n"), "# a^\nb\n");
    }

    #[test]
    fn toggle_uncomments_with_or_without_space() {
        assert_eq!(go("cpp", Cmd::Toggle, "~// a\n//b\nc^"), "~a\nb\n// c^");
        assert_eq!(go("cpp", Cmd::Toggle, "  // x^"), "  x^");
    }

    #[test]
    fn uncomment_is_case_insensitive() {
        assert_eq!(go("batch", Cmd::Uncomment, "rem echo^"), "echo^");
        assert_eq!(go("batch", Cmd::Comment, "echo^"), "REM echo^");
    }

    #[test]
    fn caret_at_start_stays_at_start() {
        assert_eq!(go("python", Cmd::Comment, "^a\nb~\n"), "# ^a\n# b~\n");
    }

    #[test]
    fn baanc_and_fortran77_comment_at_column_zero() {
        assert_eq!(go("baanc", Cmd::Comment, "  a^"), "|  a^");
        assert_eq!(go("baanc", Cmd::Comment, "^"), "|^");
        assert_eq!(go("baanc", Cmd::Uncomment, "|  a^"), "  a^");
        assert_eq!(go("fortran77", Cmd::Comment, "   x = 1^"), "C    x = 1^");
    }

    #[test]
    fn advanced_mode_wraps_each_line() {
        assert_eq!(
            go("css", Cmd::Comment, "~a {}\nb {}^"),
            "/* ~a {} */\n/* b {}^ */"
        );
        assert_eq!(go("css", Cmd::Toggle, "/* a {} */^"), "a {}^");
        assert_eq!(go("html", Cmd::Toggle, "<p>^"), "<!-- <p>^ -->");
    }

    #[test]
    fn single_line_uncomment_falls_back_to_stream() {
        assert_eq!(go("css", Cmd::Uncomment, "/* a^ */"), "a^");
        assert_eq!(go("cpp", Cmd::Uncomment, "x /* a^ */ y"), "x a^ y");
    }

    #[test]
    fn stream_comment_selection_and_line() {
        assert_eq!(go("cpp", Cmd::Stream, "a ~bc^ d"), "a /* ~bc^ */ d");
        assert_eq!(go("cpp", Cmd::Stream, "  ab^c\n"), "  /* ~abc^ */\n");
        assert_eq!(go("python", Cmd::Stream, "x^"), "# x^");
        assert_eq!(go("normal", Cmd::Stream, "x^"), "x^");
    }

    #[test]
    fn stream_uncomment_removes_all_in_selection() {
        assert_eq!(
            go("cpp", Cmd::StreamUncomment, "~/* a */ b /* c */^"),
            "~a b c^"
        );
        assert_eq!(go("cpp", Cmd::StreamUncomment, "/*a^*/"), "a^");
        assert_eq!(go("cpp", Cmd::StreamUncomment, "a^"), "a^");
        assert_eq!(go("python", Cmd::StreamUncomment, "# a^"), "a^");
    }

    #[test]
    fn stream_token_needs_word_start() {
        assert_eq!(go("cpp", Cmd::StreamUncomment, "a=/* b^ */"), "a=/* b^ */");
    }
}
