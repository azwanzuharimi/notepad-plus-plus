// SPDX-License-Identifier: GPL-3.0-or-later
use crate::encoding::{SC_EOL_CR, SC_EOL_CRLF, SC_EOL_LF};
use crate::sci::{self, send};
use crate::{item, nested, tagged};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSControlStateValue, NSControlStateValueOff, NSControlStateValueOn,
    NSEventModifierFlags, NSMenuItem, NSResponder, NSView,
};
use objc2_foundation::{NSDate, NSDateFormatter, NSDateFormatterStyle, NSObjectProtocol};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::{BuildHasher, Hasher};

pub const SCI_CUT: u32 = 2177;
const SCI_COPY: u32 = 2178;
pub const SCI_CLEAR: u32 = 2180;
const SCI_GETSELECTIONEMPTY: u32 = 2650;
pub const SCI_COPYALLOWLINE: u32 = 2519;
pub const SCI_LINEDELETE: u32 = 2338;
const SCI_LINEDUPLICATE: u32 = 2404;
const SCI_MOVESELECTEDLINESUP: u32 = 2620;
const SCI_MOVESELECTEDLINESDOWN: u32 = 2621;
const SCI_INSERTTEXT: u32 = 2003;
const SCI_GETANCHOR: u32 = 2009;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GOTOPOS: u32 = 2025;
const SCI_SETANCHOR: u32 = 2026;
const SCI_BEGINUNDOACTION: u32 = 2078;
const SCI_ENDUNDOACTION: u32 = 2079;
const SCI_GETTABWIDTH: u32 = 2121;
const SCI_SETLINEINDENTATION: u32 = 2126;
const SCI_GETLINEINDENTATION: u32 = 2127;
const SCI_GETLINEINDENTPOSITION: u32 = 2128;
const SCI_GETLINEENDPOSITION: u32 = 2136;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_SETSEL: u32 = 2160;
const SCI_REPLACESEL: u32 = 2170;
const SCI_SETTARGETRANGE: u32 = 2686;
const SCI_REPLACETARGET: u32 = 2194;
const SCI_APPENDTEXT: u32 = 2282;
const SCI_TARGETFROMSELECTION: u32 = 2287;
const SCI_LINESJOIN: u32 = 2288;
const SCI_LINESSPLIT: u32 = 2289;
const SCI_TAB: u32 = 2327;
const SCI_BACKTAB: u32 = 2328;
const SCI_LINELENGTH: u32 = 2350;
const SCI_GETSELECTIONMODE: u32 = 2423;
const SCI_SETEMPTYSELECTION: u32 = 2556;
const SCI_GETSELECTIONS: u32 = 2570;
const SCI_CHANGESELECTIONMODE: u32 = 2659;
const SCI_TARGETWHOLEDOCUMENT: u32 = 2690;
const SC_SEL_STREAM: usize = 0;
const SC_SEL_RECTANGLE: isize = 1;
const SC_SEL_THIN: isize = 3;

// Menu tags of the editOp: action.
const DEDUP: isize = 1;
const DEDUP_NEXT: isize = 2;
const SPLIT: isize = 3;
const JOIN: isize = 4;
const RM_EMPTY: isize = 5;
const RM_BLANK: isize = 6;
const LINE_ABOVE: isize = 7;
const LINE_BELOW: isize = 8;
const REVERSE: isize = 9;
const INDENT: isize = 10;
const OUTDENT: isize = 11;
const SORT: isize = 20;
const CASE: isize = 40;
const TRIM_TRAIL: isize = 60;
const TRIM_LEAD: isize = 61;
const TRIM_BOTH: isize = 62;
const EOL_TO_SPACE: isize = 63;
const TRIM_ALL: isize = 64;
const TAB_TO_SPACE: isize = 65;
const SPACE_TO_TAB: isize = 66;
const SPACE_TO_TAB_LEAD: isize = 67;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Case {
    Upper,
    Lower,
    ProperForce,
    ProperBlend,
    SentenceForce,
    SentenceBlend,
    Invert,
    Random,
}

const CASES: [(&str, Case); 8] = [
    ("UPPERCASE", Case::Upper),
    ("lowercase", Case::Lower),
    ("Proper Case", Case::ProperForce),
    ("Proper Case (blend)", Case::ProperBlend),
    ("Sentence case", Case::SentenceForce),
    ("Sentence case (blend)", Case::SentenceBlend),
    ("iNVERT cASE", Case::Invert),
    ("ranDOm CasE", Case::Random),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sort {
    Lex,
    LexIgnoreCase,
    Integer,
    DecimalComma,
    DecimalDot,
}

const SORTS: [(&str, Sort); 5] = [
    ("Lexicographically", Sort::Lex),
    ("Lex. %s Ignoring Case", Sort::LexIgnoreCase),
    ("As Integers", Sort::Integer),
    ("As Decimals (Comma)", Sort::DecimalComma),
    ("As Decimals (Dot)", Sort::DecimalDot),
];

type Unit = Result<char, u8>;

// Valid UTF-8 gives characters; each invalid byte stays a byte.
fn units(b: &[u8]) -> Vec<Unit> {
    let mut v = vec![];
    for c in b.utf8_chunks() {
        v.extend(c.valid().chars().map(Ok));
        v.extend(c.invalid().iter().map(|&x| Err(x)));
    }
    v
}

fn to_bytes(u: &[Unit]) -> Vec<u8> {
    let mut b = vec![];
    for x in u {
        match x {
            Ok(c) => b.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
            Err(x) => b.push(*x),
        }
    }
    b
}

fn single(mut it: impl Iterator<Item = char>) -> Option<char> {
    let c = it.next()?;
    it.next().is_none().then_some(c)
}

// Windows CharUpperW and CharLowerW map one character to one character.
fn upper(c: char) -> char {
    single(c.to_uppercase()).unwrap_or(c)
}

fn lower(c: char) -> char {
    single(c.to_lowercase()).unwrap_or(c)
}

fn rng() -> impl FnMut() -> bool {
    let mut x = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
        | 1;
    move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x & 1 == 1
    }
}

// Port of ScintillaEditView::changeCase.
pub fn convert_case(b: &[u8], case: Case, mut coin: impl FnMut() -> bool) -> Vec<u8> {
    let mut u = units(b);
    let n = u.len();
    let is = |u: &[Unit], i: usize, f: fn(char) -> bool| matches!(u[i], Ok(c) if f(c));
    let map = |u: &mut [Unit], i: usize, f: fn(char) -> char| {
        if let Ok(c) = u[i] {
            u[i] = Ok(f(c));
        }
    };
    let quote = |c: char| matches!(c, '\'' | '\u{2019}' | '\u{2018}');
    let force = matches!(case, Case::ProperForce | Case::SentenceForce);
    match case {
        Case::Upper => (0..n).for_each(|i| map(&mut u, i, upper)),
        Case::Lower => (0..n).for_each(|i| map(&mut u, i, lower)),
        Case::Invert => (0..n).for_each(|i| {
            if is(&u, i, char::is_lowercase) {
                map(&mut u, i, upper)
            } else {
                map(&mut u, i, lower)
            }
        }),
        Case::Random => (0..n).for_each(|i| {
            if is(&u, i, char::is_alphabetic) {
                map(&mut u, i, if coin() { upper } else { lower })
            }
        }),
        Case::ProperForce | Case::ProperBlend => {
            for i in 0..n {
                if !is(&u, i, char::is_alphabetic) {
                    continue;
                }
                if i >= 2 && is(&u, i - 1, quote) && is(&u, i - 2, char::is_alphanumeric) {
                    if force {
                        map(&mut u, i, lower)
                    }
                } else if i == 0 || !is(&u, i - 1, char::is_alphanumeric) {
                    map(&mut u, i, upper)
                } else if force {
                    map(&mut u, i, lower)
                }
            }
        }
        Case::SentenceForce | Case::SentenceBlend => {
            let (mut new, mut was_r, mut was_n) = (true, false, false);
            for i in 0..n {
                let c = u[i];
                if is(&u, i, char::is_alphabetic) {
                    if new {
                        map(&mut u, i, upper);
                        new = false;
                    } else if force {
                        map(&mut u, i, lower);
                    }
                    was_r = false;
                    was_n = false;
                    let space_or = |j: usize, x: &[char]| {
                        is(&u, j, char::is_whitespace) || matches!(u[j], Ok(c) if x.contains(&c))
                    };
                    if u[i] == Ok('i')
                        && i >= 1
                        && space_or(i - 1, &['(', '"'])
                        && i + 1 < n
                        && space_or(i + 1, &['\''])
                    {
                        u[i] = Ok('I');
                    }
                } else if matches!(c, Ok('.' | '!' | '?')) {
                    new = !(i + 1 == n || is(&u, i + 1, char::is_alphanumeric));
                } else if c == Ok('\r') {
                    new |= was_r;
                    was_r = true;
                } else if c == Ok('\n') {
                    new |= was_n;
                    was_n = true;
                }
            }
        }
    }
    to_bytes(&u)
}

fn is_eol(b: u8) -> bool {
    b == b'\r' || b == b'\n'
}

// Lines as Scintilla sees them: (text, line end), the last one has no line end.
fn lines(t: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut v = vec![];
    let mut s = 0;
    let mut i = 0;
    while i < t.len() {
        if is_eol(t[i]) {
            let e = if t[i] == b'\r' && t.get(i + 1) == Some(&b'\n') {
                i + 2
            } else {
                i + 1
            };
            v.push((&t[s..i], &t[i..e]));
            s = e;
            i = e;
        } else {
            i += 1;
        }
    }
    v.push((&t[s..], &t[t.len()..]));
    v
}

fn map_lines(t: &[u8], f: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let mut out = vec![];
    for (l, e) in lines(t) {
        out.extend(f(l));
        out.extend_from_slice(e);
    }
    out
}

fn split<'a>(t: &'a [u8], eol: &[u8]) -> Vec<&'a [u8]> {
    let mut v = vec![];
    let mut s = 0;
    let mut i = 0;
    while i + eol.len() <= t.len() {
        if &t[i..i + eol.len()] == eol {
            v.push(&t[s..i]);
            i += eol.len();
            s = i;
        } else {
            i += 1;
        }
    }
    v.push(&t[s..]);
    v
}

fn int_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let dig = |c: u8| c.is_ascii_digit();
    let num_end = |s: &[u8], i: usize| i + s[i..].iter().take_while(|c| dig(**c)).count();
    let (mut i, mut j) = (0, 0);
    loop {
        if i >= a.len() || j >= b.len() {
            return a[i.min(a.len())..].cmp(&b[j.min(b.len())..]);
        }
        let (mut an, mut bn) = (dig(a[i]), dig(b[j]));
        let (mut asg, mut bsg) = (1, 1);
        if !an && i + 1 < a.len() {
            an = a[i] == b'-' && dig(a[i + 1]);
            asg = -1;
        }
        if !bn && j + 1 < b.len() {
            bn = b[j] == b'-' && dig(b[j + 1]);
            bsg = -1;
        }
        if an != bn {
            let r = a[i].cmp(&b[j]);
            i += 1;
            j += 1;
            if r != Ordering::Equal {
                return r;
            }
        } else if an {
            if asg != bsg {
                return if asg == 1 {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
            }
            if asg == -1 {
                i += 1;
                j += 1;
            }
            let (ae, be) = (num_end(a, i), num_end(b, j));
            let (i0, j0) = (i, j);
            while i < a.len() && a[i] == b'0' {
                i += 1;
            }
            while j < b.len() && b[j] == b'0' {
                j += 1;
            }
            let (az, bz) = (i - i0, j - j0);
            let flip = |o: Ordering| if asg == -1 { o.reverse() } else { o };
            let r = (ae - i).cmp(&(be - j));
            if r != Ordering::Equal {
                return flip(r);
            }
            let r = flip(a[i..ae].cmp(&b[j..be])).then(bz.cmp(&az));
            if r != Ordering::Equal {
                return r;
            }
            i = ae;
            j = be;
        } else {
            if a[i] == b'-' {
                i += 1;
            }
            if b[j] == b'-' {
                j += 1;
            }
            let chunk = |s: &[u8], i: usize| {
                i + s[i..]
                    .iter()
                    .take_while(|c| !dig(**c) && **c != b'-')
                    .count()
            };
            let (ae, be) = (chunk(a, i), chunk(b, j));
            let r = a[i..ae].cmp(&b[j..be]);
            if r != Ordering::Equal {
                return r;
            }
            i = ae;
            j = be;
        }
    }
}

// Port of DecimalCommaSorter and DecimalDotSorter: wcstod on the leading number characters.
fn decimal(line: &[u8], comma: bool) -> Option<Option<f64>> {
    let ok: &[u8] = if comma {
        b" \t\r\n0123456789,-"
    } else {
        b" \t\r\n0123456789.-"
    };
    let s: Vec<u8> = line
        .iter()
        .take_while(|c| ok.contains(c))
        .map(|&c| if c == b',' { b'.' } else { c })
        .collect();
    let s = s.trim_ascii_start();
    if s.is_empty() {
        return Some(None);
    }
    let mut e = usize::from(s[0] == b'-');
    let int = s[e..].iter().take_while(|c| c.is_ascii_digit()).count();
    e += int;
    let mut frac = 0;
    if s.get(e) == Some(&b'.') {
        frac = s[e + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if frac > 0 || int > 0 {
            e += 1 + frac;
        }
    }
    if int + frac == 0 {
        return None;
    }
    std::str::from_utf8(&s[..e])
        .ok()?
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
        .map(Some)
}

fn fold(b: &[u8]) -> Vec<u8> {
    to_bytes(
        &units(b)
            .into_iter()
            .map(|u| u.map(lower))
            .collect::<Vec<_>>(),
    )
}

// Sorts like the Notepad++ sorters; an error gives the index of the line that is not a number.
// Stable merge sort that needs no total order: the Notepad++ integer comparator is not one, and sort_by can panic on it.
fn merge_sort<T: Clone>(v: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    if v.len() < 2 {
        return;
    }
    let mid = v.len() / 2;
    merge_sort(&mut v[..mid], less);
    merge_sort(&mut v[mid..], less);
    let mut out = Vec::with_capacity(v.len());
    let (mut i, mut j) = (0, mid);
    while i < mid && j < v.len() {
        if less(&v[j], &v[i]) {
            out.push(v[j].clone());
            j += 1;
        } else {
            out.push(v[i].clone());
            i += 1;
        }
    }
    out.extend_from_slice(&v[i..mid]);
    out.extend_from_slice(&v[j..]);
    v.clone_from_slice(&out);
}

pub fn sort_lines(v: &mut Vec<&[u8]>, how: Sort, desc: bool) -> Result<(), usize> {
    let less = |o: Ordering| if desc { o.is_gt() } else { o.is_lt() };
    match how {
        Sort::Lex => merge_sort(v, &|a, b| less(a.cmp(b))),
        Sort::LexIgnoreCase => {
            let mut k: Vec<(Vec<u8>, &[u8])> = v.iter().map(|a| (fold(a), *a)).collect();
            merge_sort(&mut k, &|a, b| less(a.0.cmp(&b.0)));
            *v = k.into_iter().map(|x| x.1).collect();
        }
        Sort::Integer => merge_sort(v, &|a, b| less(int_cmp(a, b))),
        Sort::DecimalComma | Sort::DecimalDot => {
            let mut nums = vec![];
            let mut empties = vec![];
            for (i, l) in v.iter().enumerate() {
                match decimal(l, how == Sort::DecimalComma) {
                    Some(Some(x)) => nums.push((x, *l)),
                    Some(None) => empties.push(*l),
                    None => return Err(i),
                }
            }
            merge_sort(&mut nums, &|a, b| less(a.0.total_cmp(&b.0)));
            let nums = nums.into_iter().map(|(_, l)| l);
            *v = if desc {
                nums.chain(empties).collect()
            } else {
                empties.into_iter().chain(nums).collect()
            };
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LineOp {
    Sort(Sort, bool),
    Reverse,
    Dedup,
}

// Port of ScintillaEditView::sortLines and removeAnyDuplicateLines on the text of the line range.
pub fn line_op(t: &[u8], eol: &[u8], whole: bool, op: LineOp) -> Result<Vec<u8>, usize> {
    let mut v = split(t, eol);
    if !whole && v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    match op {
        LineOp::Sort(how, desc) => sort_lines(&mut v, how, desc)?,
        LineOp::Reverse => v.reverse(),
        LineOp::Dedup => {
            let mut seen = HashSet::new();
            v.retain(|l| seen.insert(*l));
        }
    }
    let mut out = v.join(eol);
    if !whole {
        out.extend_from_slice(eol);
    }
    Ok(out)
}

// Port of Notepad_plus::removeDuplicateLines.
pub fn remove_consecutive_dups(t: &[u8]) -> Vec<u8> {
    let mut out: Vec<(&[u8], &[u8])> = vec![];
    for l in lines(t) {
        if let Some(p) = out.last_mut() {
            if p.0 == l.0 && (p.1 == l.1 || (l.1.is_empty() && !l.0.is_empty())) {
                *p = l;
                continue;
            }
        }
        out.push(l);
    }
    out.iter()
        .flat_map(|(l, e)| l.iter().chain(e.iter()))
        .copied()
        .collect()
}

// Port of Notepad_plus::removeEmptyLine; the text starts at a line start.
pub fn remove_empty_lines(t: &[u8], blank: bool, at_doc_end: bool) -> Vec<u8> {
    let mut r = vec![];
    let mut i = 0;
    while i < t.len() {
        loop {
            let j = if blank {
                i + t[i..]
                    .iter()
                    .take_while(|&&c| c == b' ' || c == b'\t')
                    .count()
            } else {
                i
            };
            let k = j + t[j..].iter().take_while(|&&c| is_eol(c)).count();
            if k == j {
                break;
            }
            i = k;
        }
        while i < t.len() && !is_eol(t[i]) {
            r.push(t[i]);
            i += 1;
        }
        let e = if t.get(i) == Some(&b'\r') && t.get(i + 1) == Some(&b'\n') {
            2
        } else {
            1
        };
        r.extend_from_slice(&t[i.min(t.len())..(i + e).min(t.len())]);
        i += e;
    }
    if at_doc_end {
        let ls = r.iter().rposition(|&c| is_eol(c)).map_or(0, |p| p + 1);
        let last = &r[ls..];
        let empty = if blank {
            last.iter().all(|&c| c == b' ' || c == b'\t')
        } else {
            last.is_empty()
        };
        if empty && ls > 0 {
            let mut p = ls;
            while p > 0 && is_eol(r[p - 1]) {
                p -= 1;
            }
            r.truncate(p);
        } else if empty && blank {
            r.clear();
        }
    }
    r
}

pub fn trim(t: &[u8], lead: bool, trail: bool) -> Vec<u8> {
    map_lines(t, |l| {
        let blank = |c: &u8| *c == b' ' || *c == b'\t';
        let s = if lead {
            l.iter().take_while(|c| blank(c)).count()
        } else {
            0
        };
        let e = if trail {
            l.len() - l[s..].iter().rev().take_while(|c| blank(c)).count()
        } else {
            l.len()
        };
        l[s..e].to_vec()
    })
}

// Port of the tab2Space part of Notepad_plus::wsTabConvert for one line.
pub fn tab_to_space(l: &[u8], tw: usize) -> Vec<u8> {
    let mut d = vec![];
    let mut col = 0;
    for &c in l {
        if c == b'\t' {
            let n = tw - col % tw;
            d.extend(std::iter::repeat_n(b' ', n));
            col += n;
        } else {
            d.push(c);
            if c & 0xC0 != 0x80 {
                col += 1;
            }
        }
    }
    d
}

// Port of the space2Tab part of Notepad_plus::wsTabConvert for one line.
pub fn space_to_tab(src: &[u8], tw: usize, leading: bool) -> Vec<u8> {
    let at = |i: usize| src.get(i).copied().unwrap_or(0);
    let tw = tw as isize;
    let mut d = vec![];
    let (mut col, mut stop, mut counter) = (0isize, tw - 1, 0usize);
    let mut non_space = false;
    let mut i = 0;
    while i < src.len() {
        if !non_space {
            let mut next = false;
            while at(i + counter) == b' ' {
                if col + counter as isize == stop {
                    stop += tw;
                    if counter >= 1 {
                        d.push(b'\t');
                        i += counter;
                        col += counter as isize + 1;
                        counter = 0;
                        next = true;
                        break;
                    } else if at(i + 1) == b' ' || at(i + 1) == b'\t' {
                        d.push(b'\t');
                        i += 1;
                        col += 1;
                        counter = 0;
                    } else {
                        d.push(src[i]);
                        col += 1;
                        counter = 0;
                        next = true;
                        break;
                    }
                } else {
                    counter += 1;
                }
            }
            if next {
                i += 1;
                continue;
            }
            if at(i) == b' ' && at(i + counter) == b'\t' {
                d.push(b'\t');
                i += counter + 1;
                col = stop + 1;
                stop += tw;
                counter = 0;
                continue;
            }
        }
        if leading {
            non_space = true;
        }
        let c = src[i];
        d.push(c);
        if c == b'\t' {
            col = stop + 1;
            stop += tw;
            counter = 0;
        } else {
            counter = 0;
            if c & 0xC0 != 0x80 {
                col += 1;
                if col % tw == 0 {
                    stop += tw;
                }
            }
        }
        i += 1;
    }
    d
}

fn s(v: &NSView, m: u32, w: isize, l: isize) -> isize {
    send(v, m, w as usize, l)
}

fn line_of(v: &NSView, p: isize) -> isize {
    s(v, sci::SCI_LINEFROMPOSITION, p, 0)
}

fn line_start(v: &NSView, l: isize) -> isize {
    s(v, sci::SCI_POSITIONFROMLINE, l, 0)
}

fn line_end(v: &NSView, l: isize) -> isize {
    s(v, SCI_GETLINEENDPOSITION, l, 0)
}

fn line_count(v: &NSView) -> isize {
    s(v, SCI_GETLINECOUNT, 0, 0)
}

fn eol(v: &NSView) -> &'static [u8] {
    match sci::eol_mode(v) {
        SC_EOL_CR => b"\r",
        SC_EOL_LF => b"\n",
        _ => b"\r\n",
    }
}

fn anchor_caret(v: &NSView) -> (isize, isize) {
    (s(v, SCI_GETANCHOR, 0, 0), s(v, SCI_GETCURRENTPOS, 0, 0))
}

fn set_sel(v: &NSView, a: isize, c: isize) {
    s(v, SCI_SETSEL, a, c);
}

fn block_mode(v: &NSView) -> bool {
    matches!(
        s(v, SCI_GETSELECTIONMODE, 0, 0),
        SC_SEL_RECTANGLE | SC_SEL_THIN
    )
}

fn has_selection(v: &NSView) -> bool {
    let (a, b) = sci::selection(v);
    a != b
}

// Port of ScintillaEditView::getSelectionLinesRange for the main selection.
fn sel_lines(v: &NSView) -> (isize, isize) {
    let (a, b) = sci::selection(v);
    let (l1, mut l2) = (line_of(v, a), line_of(v, b));
    if l1 != l2 && line_start(v, l2) == b {
        l2 -= 1;
    }
    (l1, l2)
}

// Replaces only the changed middle of the range, so the caret and the undo step stay small.
fn replace(v: &NSView, start: isize, old: &[u8], new: &[u8]) -> bool {
    let p = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let q = old[p..]
        .iter()
        .rev()
        .zip(new[p..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    if p == old.len() && p == new.len() {
        return false;
    }
    let mid = &new[p..new.len() - q];
    s(
        v,
        SCI_SETTARGETRANGE,
        start + p as isize,
        start + (old.len() - q) as isize,
    );
    s(
        v,
        SCI_REPLACETARGET,
        mid.len() as isize,
        mid.as_ptr() as isize,
    );
    true
}

fn undo<R>(v: &NSView, f: impl FnOnce() -> R) -> R {
    s(v, SCI_BEGINUNDOACTION, 0, 0);
    let r = f();
    s(v, SCI_ENDUNDOACTION, 0, 0);
    r
}

fn text(v: &NSView, a: isize, b: isize) -> Vec<u8> {
    sci::doc(v).range(a, b)
}

fn run_line_op(v: &NSView, op: LineOp) -> Result<(), usize> {
    if s(v, SCI_GETSELECTIONS, 0, 0) > 1 {
        return Ok(());
    }
    let has = has_selection(v);
    let n = line_count(v);
    let (l1, l2) = if has { sel_lines(v) } else { (0, n - 1) };
    if has && l1 == l2 {
        return Ok(());
    }
    let start = line_start(v, l1);
    let old = text(v, start, line_start(v, l2) + s(v, SCI_LINELENGTH, l2, 0));
    let whole = l2 == n - 1;
    let new = line_op(&old, eol(v), whole, op).map_err(|i| l1 as usize + i)?;
    undo(v, || replace(v, start, &old, &new));
    if has {
        let tail = if whole { 0 } else { eol(v).len() };
        set_sel(v, start, start + (new.len() - tail) as isize);
    }
    Ok(())
}

fn run_consecutive_dups(v: &NSView) {
    let (a, c) = anchor_caret(v);
    let n = line_count(v);
    let (l1, l2) = if a == c {
        (0, n - 1)
    } else {
        let (x, y) = sci::selection(v);
        let l2 = line_of(v, y);
        (
            line_of(v, x),
            if y == line_start(v, l2) { l2 - 1 } else { l2 },
        )
    };
    if l1 >= l2 {
        return;
    }
    let start = line_start(v, l1);
    let old = text(v, start, line_start(v, l2) + s(v, SCI_LINELENGTH, l2, 0));
    let new = remove_consecutive_dups(&old);
    undo(v, || replace(v, start, &old, &new));
    if a != c {
        set_sel(v, start, start + new.len() as isize);
    }
}

fn run_remove_empty(v: &NSView, blank: bool) {
    let (a, b) = sci::selection(v);
    let len = sci::length(v);
    let (start, end) = if a == b {
        (0, len)
    } else {
        (line_start(v, line_of(v, a)), b)
    };
    let old = text(v, start, end);
    let new = remove_empty_lines(&old, blank, end == len);
    undo(v, || replace(v, start, &old, &new));
    if a != b {
        set_sel(v, start, start + new.len() as isize);
    }
}

// Port of Notepad_plus::doTrim: the whole document, or the selected lines.
fn run_trim(v: &NSView, lead: bool, trail: bool) {
    if block_mode(v) {
        return;
    }
    let (a, c) = anchor_caret(v);
    let (start, end) = if a == c {
        (0, sci::length(v))
    } else {
        let (x, mut y) = sci::selection(v);
        let l2 = line_of(v, y);
        if y != line_start(v, l2) && y < line_end(v, l2) {
            y = line_end(v, l2);
        }
        (line_start(v, line_of(v, x)), y)
    };
    let old = text(v, start, end);
    let new = trim(&old, lead, trail);
    if undo(v, || replace(v, start, &old, &new)) && a != c {
        set_sel(v, start, start + new.len() as isize);
    }
}

fn run_eol_to_space(v: &NSView) {
    if block_mode(v) {
        return;
    }
    let (a, c) = anchor_caret(v);
    undo(v, || {
        s(
            v,
            if a == c {
                SCI_TARGETWHOLEDOCUMENT
            } else {
                SCI_TARGETFROMSELECTION
            },
            0,
            0,
        );
        s(v, SCI_LINESJOIN, 0, 0);
    });
}

fn run_trim_all(v: &NSView) {
    undo(v, || {
        let (a, c) = anchor_caret(v);
        run_trim(v, true, true);
        let (a2, c2) = anchor_caret(v);
        if a == c || a2 != c2 {
            run_eol_to_space(v);
        }
    });
}

// Port of Notepad_plus::wsTabConvert.
fn run_tabs(v: &NSView, f: impl Fn(&[u8]) -> Vec<u8>) {
    if block_mode(v) {
        return;
    }
    let (a, caret) = anchor_caret(v);
    let cur = line_of(v, caret);
    let whole = a == caret;
    let n = line_count(v);
    let (l1, l2, last) = if whole {
        (0, n - 1, n - 1)
    } else {
        let (x, y) = sci::selection(v);
        let l2 = line_of(v, y);
        (
            line_of(v, x),
            l2,
            if y == line_start(v, l2) { l2 - 1 } else { l2 },
        )
    };
    if last < l1 {
        return;
    }
    let start = line_start(v, l1);
    let old = text(v, start, line_end(v, last));
    let new = map_lines(&old, &f);
    let cur_start = line_start(v, cur);
    let prefix = f(&text(v, cur_start, caret)).len() as isize;
    if !undo(v, || replace(v, start, &old, &new)) {
        return;
    }
    if whole {
        s(
            v,
            SCI_GOTOPOS,
            line_start(v, cur) + prefix.min(line_end(v, cur) - line_start(v, cur)),
            0,
        );
    } else {
        let end = if last != l2 {
            line_start(v, l2)
        } else {
            line_end(v, l2)
        };
        set_sel(v, line_start(v, l1), end);
    }
}

fn run_case(v: &NSView, c: Case) {
    let (a, b) = sci::selection(v);
    if a >= b || block_mode(v) || s(v, SCI_GETSELECTIONS, 0, 0) > 1 {
        return;
    }
    let old = text(v, a, b);
    let new = convert_case(&old, c, rng());
    undo(v, || replace(v, a, &old, &new));
    set_sel(v, a, a + new.len() as isize);
}

// Port of ScintillaEditView::setLineIndent with the Notepad++ single line indent rule.
fn run_indent(v: &NSView, forward: bool) {
    let (a, b) = sci::selection(v);
    let line = line_of(v, a);
    if s(v, SCI_GETSELECTIONS, 0, 0) > 1 || line != line_of(v, b) {
        s(v, if forward { SCI_TAB } else { SCI_BACKTAB }, 0, 0);
        return;
    }
    let tw = s(v, SCI_GETTABWIDTH, 0, 0);
    let ind = s(v, SCI_GETLINEINDENTATION, line, 0) + if forward { tw } else { -tw };
    let before = s(v, SCI_GETLINEINDENTPOSITION, line, 0);
    s(v, SCI_SETLINEINDENTATION, line, ind.max(0));
    let after = s(v, SCI_GETLINEINDENTPOSITION, line, 0);
    let d = after - before;
    let fix = |p: isize| {
        if d > 0 && p >= before {
            p + d
        } else if d < 0 && p >= after {
            if p >= before {
                p + d
            } else {
                after
            }
        } else {
            p
        }
    };
    set_sel(v, fix(a), fix(b));
}

fn insert_at(v: &NSView, pos: isize, b: &[u8]) {
    let z = [b, b"\0"].concat();
    s(v, SCI_INSERTTEXT, pos, z.as_ptr() as isize);
}

// Port of insertNewLineAboveCurrentLine and insertNewLineBelowCurrentLine.
fn run_blank_line(v: &NSView, below: bool) {
    let e = eol(v);
    let cur = line_of(v, s(v, SCI_GETCURRENTPOS, 0, 0));
    if below {
        if cur == line_count(v) - 1 {
            s(v, SCI_APPENDTEXT, e.len() as isize, e.as_ptr() as isize);
        } else {
            insert_at(v, line_start(v, cur + 1), e);
        }
        s(v, SCI_SETEMPTYSELECTION, line_start(v, cur + 1), 0);
    } else {
        let pos = if cur == 0 { 0 } else { line_end(v, cur - 1) };
        insert_at(v, pos, e);
        s(v, SCI_SETEMPTYSELECTION, line_start(v, cur), 0);
    }
}

fn run_lines_target(v: &NSView, msg: u32) {
    let (l1, l2) = sel_lines(v);
    if (msg == SCI_LINESJOIN && l1 == l2)
        || (msg == SCI_LINESSPLIT && s(v, SCI_GETSELECTIONS, 0, 0) != 1)
    {
        return;
    }
    set_sel(v, line_start(v, l1), line_end(v, l2));
    s(v, SCI_TARGETFROMSELECTION, 0, 0);
    s(v, msg, 0, 0);
}

// Runs the editOp: menu command; an error gives the line index that numeric sorting cannot read.
pub fn run(v: &NSView, tag: isize) -> Result<(), usize> {
    let tw = s(v, SCI_GETTABWIDTH, 0, 0).max(1) as usize;
    match tag {
        DEDUP => return run_line_op(v, LineOp::Dedup),
        REVERSE => return run_line_op(v, LineOp::Reverse),
        t if (SORT..SORT + 10).contains(&t) => {
            let k = (t - SORT) as usize;
            return run_line_op(v, LineOp::Sort(SORTS[k / 2].1, k % 2 == 1));
        }
        t if (CASE..CASE + 8).contains(&t) => run_case(v, CASES[(t - CASE) as usize].1),
        DEDUP_NEXT => run_consecutive_dups(v),
        SPLIT => run_lines_target(v, SCI_LINESSPLIT),
        JOIN => run_lines_target(v, SCI_LINESJOIN),
        RM_EMPTY | RM_BLANK => run_remove_empty(v, tag == RM_BLANK),
        LINE_ABOVE | LINE_BELOW => run_blank_line(v, tag == LINE_BELOW),
        INDENT | OUTDENT => run_indent(v, tag == INDENT),
        TRIM_TRAIL => run_trim(v, false, true),
        TRIM_LEAD => run_trim(v, true, false),
        TRIM_BOTH => run_trim(v, true, true),
        EOL_TO_SPACE => run_eol_to_space(v),
        TRIM_ALL => run_trim_all(v),
        TAB_TO_SPACE => run_tabs(v, |l| tab_to_space(l, tw)),
        SPACE_TO_TAB => run_tabs(v, |l| space_to_tab(l, tw, false)),
        SPACE_TO_TAB_LEAD => run_tabs(v, |l| space_to_tab(l, tw, true)),
        _ => {}
    }
    Ok(())
}

// Port of Notepad_plus::beginOrEndSelect; `begin` keeps the start position and the column mode flag.
pub fn begin_end_select(
    v: &NSView,
    begin: Option<(isize, bool)>,
    column: bool,
) -> Option<(isize, bool)> {
    let cur = s(v, SCI_GETCURRENTPOS, 0, 0);
    let Some((start, _)) = begin else {
        return Some((cur, column));
    };
    send(
        v,
        SCI_CHANGESELECTIONMODE,
        if column {
            SC_SEL_RECTANGLE as usize
        } else {
            SC_SEL_STREAM
        },
        0,
    );
    if column {
        s(v, SCI_SETANCHOR, start, 0);
    } else {
        set_sel(v, start, cur);
    }
    None
}


// Notepad++ cuts or copies the whole line when nothing is selected.
pub fn cut_or_copy(v: &NSView, cut: bool) {
    if s(v, SCI_GETSELECTIONEMPTY, 0, 0) == 0 {
        s(v, if cut { SCI_CUT } else { SCI_COPY }, 0, 0);
    } else {
        s(v, SCI_COPYALLOWLINE, 0, 0);
        if cut {
            s(v, SCI_LINEDELETE, 0, 0);
        }
    }
}

pub fn insert_date_time(v: &NSView, long: bool) {
    let now = NSDate::now();
    let f =
        |d, t| NSDateFormatter::localizedStringFromDate_dateStyle_timeStyle(&now, d, t).to_string();
    let date = f(
        if long {
            NSDateFormatterStyle::LongStyle
        } else {
            NSDateFormatterStyle::ShortStyle
        },
        NSDateFormatterStyle::NoStyle,
    );
    let time = f(
        NSDateFormatterStyle::NoStyle,
        NSDateFormatterStyle::ShortStyle,
    );
    let z = format!("{time} {date}\0");
    undo(v, || s(v, SCI_REPLACESEL, 0, z.as_ptr() as isize));
}

fn key(i: Retained<NSMenuItem>, mods: NSEventModifierFlags) -> Retained<NSMenuItem> {
    i.setKeyEquivalentModifierMask(mods);
    i
}

pub fn edit_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let op = |title: &str, tag: isize| tagged(mtm, title, sel!(editOp:), tag, t);
    let sci_cmd = |title: &str, msg: u32| tagged(mtm, title, sel!(sciCommand:), msg as isize, t);
    let cmd = NSEventModifierFlags::Command;
    let (opt, shift, ctrl) = (
        NSEventModifierFlags::Option,
        NSEventModifierFlags::Shift,
        NSEventModifierFlags::Control,
    );
    let with_key = |i: Retained<NSMenuItem>, k: &str, m: NSEventModifierFlags| {
        i.setKeyEquivalent(&crate::ns(k));
        key(i, m)
    };
    let sep = || NSMenuItem::separatorItem(mtm);
    let mut sort = vec![sep()];
    for desc in [false, true] {
        let dir = if desc { "Descending" } else { "Ascending" };
        for (k, (name, _)) in SORTS.iter().enumerate() {
            let title = if name.contains("%s") {
                format!("Sort Lines {}", name.replace("%s", dir))
            } else {
                format!("Sort Lines {name} {dir}")
            };
            sort.push(op(&title, SORT + 2 * k as isize + desc as isize));
        }
        if !desc {
            sort.push(sep());
        }
    }
    let mut lines = vec![
        with_key(
            sci_cmd("Duplicate Current Line", SCI_LINEDUPLICATE),
            "d",
            cmd,
        ),
        op("Remove Duplicate Lines", DEDUP),
        op("Remove Consecutive Duplicate Lines", DEDUP_NEXT),
        op("Split Lines", SPLIT),
        op("Join Lines", JOIN),
        with_key(
            sci_cmd("Move Up Current Line", SCI_MOVESELECTEDLINESUP),
            "\u{f700}",
            ctrl | shift,
        ),
        with_key(
            sci_cmd("Move Down Current Line", SCI_MOVESELECTEDLINESDOWN),
            "\u{f701}",
            ctrl | shift,
        ),
        op("Remove Empty Lines", RM_EMPTY),
        op("Remove Empty Lines (Containing Blank characters)", RM_BLANK),
        with_key(
            op("Insert Blank Line Above Current", LINE_ABOVE),
            "\r",
            cmd | opt,
        ),
        with_key(
            op("Insert Blank Line Below Current", LINE_BELOW),
            "\r",
            cmd | opt | shift,
        ),
        op("Reverse Line Order", REVERSE),
    ];
    lines.extend(sort);
    let cases = CASES
        .iter()
        .enumerate()
        .map(|(k, (name, c))| {
            let i = op(name, CASE + k as isize);
            match c {
                Case::Upper => with_key(i, "u", cmd | shift),
                Case::SentenceForce => with_key(i, "u", cmd | opt),
                _ => i,
            }
        })
        .collect();
    let eols = [
        ("Windows (CR LF)", SC_EOL_CRLF),
        ("Unix (LF)", SC_EOL_LF),
        ("Macintosh (CR)", SC_EOL_CR),
    ]
    .iter()
    .map(|(n, m)| tagged(mtm, n, sel!(eolConvert:), *m as isize, t))
    .collect();
    vec![
        item(mtm, "Undo", sel!(undo:), "z", None),
        item(mtm, "Redo", sel!(redo:), "Z", None),
        sep(),
        item(mtm, "Cut", sel!(cut:), "x", t),
        item(mtm, "Copy", sel!(copy:), "c", t),
        item(mtm, "Paste", sel!(paste:), "v", None),
        sci_cmd("Delete", SCI_CLEAR),
        item(mtm, "Select All", sel!(selectAll:), "a", None),
        with_key(
            tagged(mtm, "Begin/End Select", sel!(beginEndSelect:), 0, t),
            "B",
            cmd,
        ),
        tagged(
            mtm,
            "Begin/End Select in Column Mode",
            sel!(beginEndSelect:),
            1,
            t,
        ),
        sep(),
        nested(
            mtm,
            "Insert",
            vec![
                tagged(mtm, "Date Time (short)", sel!(insertDateTime:), 0, t),
                tagged(mtm, "Date Time (long)", sel!(insertDateTime:), 1, t),
            ],
        ),
        nested(
            mtm,
            "Copy to Clipboard",
            vec![
                tagged(
                    mtm,
                    "Copy Current Full File path",
                    sel!(copyPathInfo:),
                    0,
                    t,
                ),
                tagged(mtm, "Copy Current Filename", sel!(copyPathInfo:), 1, t),
                tagged(mtm, "Copy Current Dir. Path", sel!(copyPathInfo:), 2, t),
            ],
        ),
        nested(
            mtm,
            "Indent",
            vec![
                op("Increase Line Indent", INDENT),
                op("Decrease Line Indent", OUTDENT),
            ],
        ),
        nested(mtm, "Convert Case to", cases),
        nested(mtm, "Line Operations", lines),
        crate::language::comment_menu(mtm, t),
        crate::autoc::autoc_menu(mtm, t),
        nested(mtm, "EOL Conversion", eols),
        nested(
            mtm,
            "Blank Operations",
            vec![
                op("Trim Trailing Space", TRIM_TRAIL),
                op("Trim Leading Space", TRIM_LEAD),
                op("Trim Leading and Trailing Space", TRIM_BOTH),
                op("EOL to Space", EOL_TO_SPACE),
                op("Trim both and EOL to Space", TRIM_ALL),
                sep(),
                op("TAB to Space", TAB_TO_SPACE),
                op("Space to TAB (All)", SPACE_TO_TAB),
                op("Space to TAB (Leading)", SPACE_TO_TAB_LEAD),
            ],
        ),
        sep(),
        crate::column::multi_select_menu(mtm, t, false),
        crate::column::multi_select_menu(mtm, t, true),
        crate::column::undo_item(mtm, t),
        crate::column::skip_item(mtm, t),
        sep(),
        crate::column::column_mode_item(mtm, t),
        crate::column::column_editor_item(mtm, t),
        sep(),
        nested(
            mtm,
            "Read-Only in Notepad++",
            vec![item(
                mtm,
                "Read-Only on Current Document",
                sel!(toggleReadOnly:),
                "",
                t,
            )],
        ),
    ]
}

fn same(a: &AnyObject, b: &AnyObject) -> bool {
    std::ptr::eq(a, b)
}

fn state(on: bool) -> NSControlStateValue {
    if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    }
}

impl crate::App {
    fn focused_editor(&self) -> Option<Retained<NSView>> {
        let w = self.ivars().window.get()?;
        let v = self.editor()?;
        let r = w.firstResponder()?;
        (w.isKeyWindow() && same(&r, &sci::content(&v))).then_some(v)
    }

    // The responder that gets a nil target action when this object does not take it.
    fn responder_for(&self, a: Sel) -> Option<Retained<NSResponder>> {
        let w = NSApplication::sharedApplication(self.mtm()).keyWindow()?;
        let mut r = w.firstResponder();
        while let Some(x) = r {
            if x.respondsToSelector(a) {
                return Some(x);
            }
            r = unsafe { x.nextResponder() };
        }
        None
    }

    // Cut and Copy go to the editor without a selection check, so the menu state cannot be old (issue 15).
    pub(crate) fn cut_or_copy(&self, a: Sel, sender: Option<&AnyObject>) {
        if let Some(v) = self.focused_editor() {
            cut_or_copy(&v, a == sel!(cut:));
        } else if let Some(r) = self.responder_for(a) {
            unsafe { NSApplication::sharedApplication(self.mtm()).sendAction_to_from(a, Some(&r), sender) };
        }
    }

    pub(crate) fn validate_edit(&self, item: &NSMenuItem) -> Option<bool> {
        let a = item.action()?;
        let ours = [
            sel!(sciCommand:),
            sel!(editOp:),
            sel!(beginEndSelect:),
            sel!(insertDateTime:),
            sel!(copyPathInfo:),
            sel!(toggleReadOnly:),
            sel!(cut:),
            sel!(copy:),
        ];
        if !ours.contains(&a) {
            return None;
        }
        let clip = a == sel!(cut:) || a == sel!(copy:);
        if clip && self.focused_editor().is_none() {
            return Some(self.responder_for(a).is_some_and(|r| unsafe {
                if r.respondsToSelector(sel!(validateMenuItem:)) {
                    msg_send![&r, validateMenuItem: item]
                } else if r.respondsToSelector(sel!(validateUserInterfaceItem:)) {
                    msg_send![&r, validateUserInterfaceItem: item]
                } else {
                    true
                }
            }));
        }
        let Some(t) = self.current().and_then(|i| self.tab(i)) else {
            return Some(false);
        };
        let ro = sci::read_only(&t.view);
        Some(if a == sel!(toggleReadOnly:) {
            item.setState(state(t.ro));
            !self.ivars().replacing.get()
        } else if a == sel!(beginEndSelect:) {
            let b = self.ivars().begin_select.get();
            let mine = b.is_some_and(|(_, col)| col == (item.tag() == 1));
            item.setState(state(mine));
            b.is_none() || mine
        } else if a == sel!(copyPathInfo:) || a == sel!(copy:) {
            true
        } else if a == sel!(sciCommand:) && item.tag() == SCI_CLEAR as isize {
            !ro && has_selection(&t.view)
        } else {
            !ro
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(s: &str, c: Case) -> String {
        String::from_utf8(convert_case(s.as_bytes(), c, || true)).unwrap()
    }

    #[test]
    fn simple_cases_map_one_char_to_one_char() {
        assert_eq!(case("abc ß é", Case::Upper), "ABC ß É");
        assert_eq!(case("ABC İ", Case::Lower), "abc İ");
        assert_eq!(case("aBc", Case::Invert), "AbC");
        assert_eq!(case("ab1", Case::Random), "AB1");
    }

    #[test]
    fn proper_case_follows_notepad_plus_plus() {
        assert_eq!(
            case("hELLO wORLD it's", Case::ProperForce),
            "Hello World It's"
        );
        assert_eq!(case("hELLO wORLD", Case::ProperBlend), "HELLO WORLD");
        assert_eq!(case("o'neil x1y", Case::ProperForce), "O'neil X1y");
    }

    #[test]
    fn sentence_case_follows_notepad_plus_plus() {
        assert_eq!(
            case("hELLO. wHAT i said? yes", Case::SentenceForce),
            "Hello. What I said? Yes"
        );
        assert_eq!(case("a.b c", Case::SentenceForce), "A.b c");
        assert_eq!(
            case("one\r\ntwo\r\n\r\nthree", Case::SentenceForce),
            "One\r\ntwo\r\n\r\nThree"
        );
        assert_eq!(case("aBC. dEF", Case::SentenceBlend), "ABC. DEF");
    }

    #[test]
    fn case_keeps_invalid_bytes() {
        assert_eq!(
            convert_case(b"a\xffb\xc3", Case::Upper, || true),
            b"A\xffB\xc3"
        );
    }

    #[test]
    fn sort_and_reverse_use_the_document_eol() {
        let t = b"b\r\na\r\nc";
        let sort = |t: &[u8], how, desc, whole| {
            line_op(t, b"\r\n", whole, LineOp::Sort(how, desc)).unwrap()
        };
        assert_eq!(sort(t, Sort::Lex, false, true), b"a\r\nb\r\nc");
        assert_eq!(sort(t, Sort::Lex, true, true), b"c\r\nb\r\na");
        assert_eq!(sort(b"b\r\na\r\n", Sort::Lex, false, false), b"a\r\nb\r\n");
        assert_eq!(sort(b"b\r\na\r\n", Sort::Lex, false, true), b"\r\na\r\nb");
        assert_eq!(
            line_op(t, b"\r\n", true, LineOp::Reverse).unwrap(),
            b"c\r\na\r\nb"
        );
    }

    #[test]
    fn sort_ignoring_case_is_stable() {
        let mut v: Vec<&[u8]> = vec![b"b", b"B", b"a", b"A"];
        sort_lines(&mut v, Sort::LexIgnoreCase, false).unwrap();
        assert_eq!(v, [b"a", b"A", b"b", b"B"]);
        sort_lines(&mut v, Sort::LexIgnoreCase, true).unwrap();
        assert_eq!(v, [b"b", b"B", b"a", b"A"]);
    }

    #[test]
    fn integer_sort_compares_numbers_in_text() {
        let mut v: Vec<&[u8]> = vec![b"x10", b"x9", b"-5", b"3", b"007", b"7", b"abc"];
        sort_lines(&mut v, Sort::Integer, false).unwrap();
        assert_eq!(v, [&b"-5"[..], b"3", b"007", b"7", b"abc", b"x9", b"x10"]);
        assert_eq!(int_cmp(b"12345678901234567890", b"9"), Ordering::Greater);
        assert_eq!(int_cmp(b"-10", b"-9"), Ordering::Less);
    }

    #[test]
    fn decimal_sort_puts_empty_lines_first_and_reports_bad_lines() {
        let mut v: Vec<&[u8]> = vec![b"2,5", b"", b"-1,25x", b"10"];
        sort_lines(&mut v, Sort::DecimalComma, false).unwrap();
        assert_eq!(v, [&b""[..], b"-1,25x", b"2,5", b"10"]);
        sort_lines(&mut v, Sort::DecimalComma, true).unwrap();
        assert_eq!(v, [&b"10"[..], b"2,5", b"-1,25x", b""]);
        let mut v: Vec<&[u8]> = vec![b"1.5", b" 0.5", b"-x"];
        assert_eq!(sort_lines(&mut v, Sort::DecimalDot, false), Err(2));
        assert_eq!(decimal(b"1.2.3", false), Some(Some(1.2)));
        assert_eq!(decimal(b"-", false), None);
        assert_eq!(decimal(b"x", false), Some(None));
        assert_eq!(decimal(b"5.", false), Some(Some(5.0)));
    }

    #[test]
    fn numeric_sorts_do_not_panic_on_random_lines() {
        let parts: [&[u8]; 9] = [b"x", b" ", b"9", b"A", b"1", b"-", b"0", b"b", b","];
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as usize
        };
        for _ in 0..200 {
            let n = 20 + next() % 280;
            let owned: Vec<Vec<u8>> = (0..n)
                .map(|_| (0..next() % 6).flat_map(|_| parts[next() % parts.len()].to_vec()).collect())
                .collect();
            for how in [Sort::Integer, Sort::DecimalComma, Sort::DecimalDot] {
                for desc in [false, true] {
                    let mut v: Vec<&[u8]> = owned.iter().map(|l| &l[..]).collect();
                    if sort_lines(&mut v, how, desc).is_ok() {
                        assert_eq!(v.len(), n);
                    }
                }
            }
        }
    }

    #[test]
    fn merge_sort_is_stable() {
        let mut v = vec![(1, 'a'), (0, 'b'), (1, 'c'), (0, 'd')];
        merge_sort(&mut v, &|a, b| a.0 < b.0);
        assert_eq!(v, [(0, 'b'), (0, 'd'), (1, 'a'), (1, 'c')]);
    }

    #[test]
    fn dedup_keeps_the_first_line() {
        let r = line_op(b"a\nb\na\nb\n", b"\n", true, LineOp::Dedup).unwrap();
        assert_eq!(r, b"a\nb\n");
    }

    #[test]
    fn consecutive_dups_keep_the_last_line_without_eol() {
        assert_eq!(remove_consecutive_dups(b"a\na\nb\na\n"), b"a\nb\na\n");
        assert_eq!(remove_consecutive_dups(b"x\na\r\na"), b"x\na");
        assert_eq!(remove_consecutive_dups(b"x\n\n"), b"x\n\n");
        assert_eq!(remove_consecutive_dups(b"a\r\na\n"), b"a\r\na\n");
    }

    #[test]
    fn remove_empty_lines_like_notepad_plus_plus() {
        assert_eq!(remove_empty_lines(b"\na\n\n\nb\n", false, true), b"a\nb");
        assert_eq!(
            remove_empty_lines(b"a\r\n  \r\nb\r\n\r\n", false, true),
            b"a\r\n  \r\nb"
        );
        assert_eq!(
            remove_empty_lines(b"a\r\n \t\r\nb\r\n  ", true, true),
            b"a\r\nb"
        );
        assert_eq!(remove_empty_lines(b"a\n\nb\n", false, false), b"a\nb\n");
        assert_eq!(remove_empty_lines(b"  ", true, true), b"");
    }

    #[test]
    fn trim_keeps_line_ends() {
        assert_eq!(trim(b" a \t\r\n\tb \n c", true, false), b"a \t\r\nb \nc");
        assert_eq!(trim(b" a \t\r\n\tb \n c", false, true), b" a\r\n\tb\n c");
        assert_eq!(trim(b"   \n", true, true), b"\n");
    }

    #[test]
    fn tabs_and_spaces_use_tab_stops() {
        assert_eq!(
            map_lines(b"\tx\r\nab\tc", |l| tab_to_space(l, 4)),
            b"    x\r\nab  c"
        );
        assert_eq!(tab_to_space("é\tx".as_bytes(), 4), "é   x".as_bytes());
        assert_eq!(space_to_tab(b"    x", 4, false), b"\tx");
        assert_eq!(space_to_tab(b"a   b", 4, false), b"a\tb");
        assert_eq!(space_to_tab(b"        x    y", 4, true), b"\t\tx    y");
        assert_eq!(space_to_tab(b"        x    y", 4, false), b"\t\tx\t y");
        assert_eq!(space_to_tab(b"a b", 4, false), b"a b");
    }
}
