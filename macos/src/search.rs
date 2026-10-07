// SPDX-License-Identifier: GPL-3.0-or-later
use std::ffi::{c_char, c_int, c_void, CStr};
use std::path::{Path, PathBuf};

const SCFIND_WHOLEWORD: u32 = 0x2;
const SCFIND_MATCHCASE: u32 = 0x4;
const SCFIND_REGEXP: u32 = 0x0020_0000;
const SCFIND_POSIX: u32 = 0x0040_0000;
const SCFIND_REGEXP_DOTMATCHESNL: u32 = 0x1000_0000;
const SCFIND_REGEXP_EMPTYMATCH_MASK: u32 = 0xE000_0000;
const SCFIND_REGEXP_EMPTYMATCH_NOTAFTERMATCH: u32 = 0x2000_0000;
const SCFIND_REGEXP_EMPTYMATCH_ALL: u32 = 0x4000_0000;
const SCFIND_REGEXP_EMPTYMATCH_ALLOWATSTART: u32 = 0x8000_0000;
const SCFIND_REGEXP_SKIPCRLFASONE: u32 = 0x0800_0000;
const LINE_MAX: usize = 2048 - 4;

#[repr(C)]
pub struct RawDoc {
    _p: [u8; 0],
}

extern "C" {
    fn npp_doc_new(text: *const u8, len: isize) -> *mut RawDoc;
    fn npp_doc_free(d: *mut RawDoc);
    fn npp_doc_from_pointer(p: *mut c_void) -> *mut RawDoc;
    fn npp_doc_length(d: *mut RawDoc) -> isize;
    fn npp_doc_text(d: *mut RawDoc, buf: *mut u8, pos: isize, len: isize);
    fn npp_doc_char_at(d: *mut RawDoc, pos: isize) -> c_int;
    fn npp_doc_line_from_pos(d: *mut RawDoc, pos: isize) -> isize;
    fn npp_doc_line_start(d: *mut RawDoc, line: isize) -> isize;
    fn npp_doc_line_end(d: *mut RawDoc, line: isize) -> isize;
    fn npp_doc_lines(d: *mut RawDoc) -> isize;
    fn npp_doc_find(
        d: *mut RawDoc,
        min: isize,
        max: isize,
        s: *const u8,
        len: *mut isize,
        flags: c_int,
    ) -> isize;
    fn npp_regex_error() -> *const c_char;
    fn npp_doc_replace(
        d: *mut RawDoc,
        pos: isize,
        len: isize,
        s: *const u8,
        slen: isize,
        regex: c_int,
    ) -> isize;
    fn npp_doc_undo_group(d: *mut RawDoc, begin: c_int);
    #[cfg(test)]
    fn npp_doc_undo(d: *mut RawDoc);
}

pub struct Doc {
    p: *mut RawDoc,
    owned: bool,
}

impl Drop for Doc {
    fn drop(&mut self) {
        if self.owned {
            unsafe { npp_doc_free(self.p) }
        }
    }
}

impl Doc {
    pub fn new(b: &[u8]) -> Option<Doc> {
        let p = unsafe { npp_doc_new(b.as_ptr(), b.len() as isize) };
        (!p.is_null()).then_some(Doc { p, owned: true })
    }

    pub fn from_pointer(p: isize) -> Doc {
        Doc {
            p: unsafe { npp_doc_from_pointer(p as *mut c_void) },
            owned: false,
        }
    }

    pub fn len(&self) -> isize {
        unsafe { npp_doc_length(self.p) }
    }

    pub fn range(&self, pos: isize, end: isize) -> Vec<u8> {
        let mut b = vec![0u8; (end - pos).max(0) as usize];
        unsafe { npp_doc_text(self.p, b.as_mut_ptr(), pos, b.len() as isize) };
        b
    }

    pub fn text(&self) -> Vec<u8> {
        self.range(0, self.len())
    }

    fn char_at(&self, pos: isize) -> u8 {
        unsafe { npp_doc_char_at(self.p, pos) as u8 }
    }

    pub fn line_of(&self, pos: isize) -> isize {
        unsafe { npp_doc_line_from_pos(self.p, pos) }
    }

    pub fn line_span(&self, line: isize) -> (isize, isize) {
        unsafe {
            (
                npp_doc_line_start(self.p, line),
                npp_doc_line_end(self.p, line),
            )
        }
    }

    fn lines(&self) -> isize {
        unsafe { npp_doc_lines(self.p) }
    }

    pub fn find(
        &self,
        min: isize,
        max: isize,
        pat: &[u8],
        flags: u32,
    ) -> Result<Option<(isize, isize)>, String> {
        let z = nul(pat);
        let mut len = pat.len() as isize;
        let pos = unsafe { npp_doc_find(self.p, min, max, z.as_ptr(), &mut len, flags as c_int) };
        match pos {
            -1 => Ok(None),
            p if p < -1 => Err(unsafe { CStr::from_ptr(npp_regex_error()) }
                .to_string_lossy()
                .into_owned()),
            p => Ok(Some((p, p + len))),
        }
    }

    pub fn replace(&self, pos: isize, len: isize, with: &[u8], regex: bool) -> isize {
        let z = nul(with);
        unsafe {
            npp_doc_replace(
                self.p,
                pos,
                len,
                z.as_ptr(),
                with.len() as isize,
                regex as c_int,
            )
        }
    }

    pub fn undo_group(&self, begin: bool) {
        unsafe { npp_doc_undo_group(self.p, begin as c_int) }
    }
}

// Fills the case conversion tables on the main thread before any search thread starts.
pub fn warm_up() {
    if let Some(d) = Doc::new(b"A") {
        let _ = d.find(0, 1, b"a", 0);
    }
}

fn nul(b: &[u8]) -> Vec<u8> {
    let mut v = b.to_vec();
    v.push(0);
    v
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Mode {
    #[default]
    Normal,
    Extended,
    Regex,
}

#[derive(Clone, Default, Debug)]
pub struct Opts {
    pub find: String,
    pub replace: String,
    pub whole_word: bool,
    pub match_case: bool,
    pub wrap: bool,
    pub mode: Mode,
    pub dot_nl: bool,
}

impl Opts {
    fn flags(&self) -> u32 {
        let re = self.mode == Mode::Regex;
        (if self.whole_word { SCFIND_WHOLEWORD } else { 0 })
            | (if self.match_case { SCFIND_MATCHCASE } else { 0 })
            | (if re { SCFIND_REGEXP | SCFIND_POSIX } else { 0 })
            | (if re && self.dot_nl {
                SCFIND_REGEXP_DOTMATCHESNL
            } else {
                0
            })
    }

    fn bytes(&self, s: &str) -> Vec<u8> {
        if self.mode == Mode::Extended {
            extended(s)
        } else {
            s.as_bytes().to_vec()
        }
    }

    pub fn replace_bytes(&self) -> Vec<u8> {
        self.bytes(&self.replace)
    }

    pub fn regex(&self) -> bool {
        self.mode == Mode::Regex
    }

    fn lax(&self) -> bool {
        self.wrap && !self.match_case && !self.whole_word
    }
}

// Port of Searching::convertExtendedToString.
pub fn extended(q: &str) -> Vec<u8> {
    let c: Vec<char> = q.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '\\' && i + 1 < c.len() {
            i += 1;
            let cur = c[i];
            match cur {
                'r' => out.push('\r'),
                'n' => out.push('\n'),
                '0' => out.push('\0'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                'b' | 'd' | 'o' | 'x' | 'u' => {
                    let (size, base) = match cur {
                        'b' => (8, 2),
                        'o' => (3, 8),
                        'd' => (3, 10),
                        'x' => (2, 16),
                        _ => (4, 16),
                    };
                    match c.get(i + 1..i + 1 + size).and_then(|d| read_base(d, base)) {
                        Some(v) => {
                            out.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                            i += size;
                        }
                        None => {
                            out.push('\\');
                            out.push(cur);
                        }
                    }
                }
                _ => {
                    out.push('\\');
                    out.push(cur);
                }
            }
        } else {
            out.push(c[i]);
        }
        i += 1;
    }
    out.into_bytes()
}

fn read_base(digits: &[char], base: i64) -> Option<u32> {
    let max = '0' as i64 + base - 1;
    let mut v = 0i64;
    for &ch in digits {
        let mut c = ch as i64;
        if c >= 'A' as i64 {
            c = (c & 0xdf) - ('A' as i64 - '0' as i64 - 10);
        } else if c > '9' as i64 {
            return None;
        }
        if c < '0' as i64 || c > max {
            return None;
        }
        v = v * base + (c - '0' as i64);
    }
    Some(v as u32)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Wrap {
    No,
    End,
    Top,
}

pub type Found = ((isize, isize), Wrap);

#[derive(Clone, Copy, PartialEq)]
pub enum Next {
    Find,
    AfterReplace,
    ForReplace,
}

// Port of FindReplaceDlg::processFindNext.
pub fn find_next(
    doc: &Doc,
    o: &Opts,
    sel: (isize, isize),
    up: bool,
    kind: Next,
) -> Result<Option<Found>, String> {
    let pat = o.bytes(&o.find);
    if pat.is_empty() {
        return Ok(None);
    }
    let len = doc.len();
    let (s, e) = match (kind != Next::Find, up) {
        (true, true) => (sel.1, 0),
        (true, false) => (sel.0, len),
        (false, true) => ((sel.1 - 1).max(0), 0),
        (false, false) => (sel.1, len),
    };
    let mut flags = o.flags()
        | SCFIND_REGEXP_SKIPCRLFASONE
        | match kind {
            Next::Find => SCFIND_REGEXP_EMPTYMATCH_ALL,
            Next::AfterReplace => SCFIND_REGEXP_EMPTYMATCH_NOTAFTERMATCH,
            Next::ForReplace => {
                SCFIND_REGEXP_EMPTYMATCH_ALL | SCFIND_REGEXP_EMPTYMATCH_ALLOWATSTART
            }
        };
    if s > 0 && doc.char_at(s - 1) == b'\r' && doc.char_at(s) == b'\n' {
        flags &= !SCFIND_REGEXP_EMPTYMATCH_MASK;
    }
    if let Some(m) = doc.find(s, e, &pat, flags)? {
        return Ok(Some((m, Wrap::No)));
    }
    if !o.wrap {
        return Ok(None);
    }
    let (s, e, w) = if up {
        (len, 0, Wrap::Top)
    } else {
        (0, len, Wrap::End)
    };
    Ok(doc.find(s, e, &pat, flags)?.map(|m| (m, w)))
}

// Port of FindReplaceDlg::processRange for count, replace all and find all.
pub fn process(
    doc: &Doc,
    o: &Opts,
    replace: bool,
    allow_empty: bool,
    (mut start, mut end): (isize, isize),
) -> Result<Vec<(isize, isize)>, String> {
    let pat = o.bytes(&o.find);
    let with = o.bytes(&o.replace);
    let mut out = vec![];
    if pat.is_empty() || start == end {
        return Ok(out);
    }
    let mut flags = o.flags() | SCFIND_REGEXP_SKIPCRLFASONE;
    if allow_empty {
        flags |= SCFIND_REGEXP_EMPTYMATCH_NOTAFTERMATCH;
    }
    while let Some((s, e)) = doc.find(start, end, &pat, flags)? {
        if e > end {
            break;
        }
        out.push((s, e));
        let n = e - s;
        let delta = if replace {
            let r = doc.replace(s, n, &with, o.regex());
            if r < 0 {
                return Err("Replace failed".into());
            }
            r - n
        } else {
            0
        };
        if s + n == end {
            break;
        }
        start = s + n + delta;
        end += delta;
    }
    Ok(out)
}

pub fn replace_all(doc: &Doc, o: &Opts, range: (isize, isize)) -> Result<usize, String> {
    doc.undo_group(true);
    let r = process(doc, o, true, true, range);
    doc.undo_group(false);
    r.map(|v| v.len())
}

pub fn commafy(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn scope(o: &Opts) -> &'static str {
    if o.wrap {
        "in entire file"
    } else {
        "from caret to end-of-file"
    }
}

pub const NOT_FOUND_REASON: &str = "The given occurrence cannot be found. You may have forgotten to check \"Wrap around\" (to ON), \"Match case\" (to OFF), or \"Match whole word only\" (to OFF).";

fn with_reason(msg: String, n: usize, o: &Opts) -> String {
    if n == 0 && !o.lax() {
        format!("{msg}\n{NOT_FOUND_REASON}")
    } else {
        msg
    }
}

pub fn count_status(n: usize, o: &Opts) -> String {
    let m = if n == 1 {
        "Count: 1 match".to_string()
    } else {
        format!("Count: {} matches", commafy(n))
    };
    with_reason(format!("{m} {}", scope(o)), n, o)
}

pub fn replace_all_status(n: usize, o: &Opts) -> String {
    let m = if n == 1 {
        "Replace All: 1 occurrence was replaced".to_string()
    } else {
        format!("Replace All: {n} occurrences were replaced")
    };
    with_reason(format!("{m} {}", scope(o)), n, o)
}

pub fn replace_not_found_status(o: &Opts) -> String {
    with_reason(
        format!("Replace: no occurrence was found {}", scope(o)),
        0,
        o,
    )
}

pub fn not_found_status(o: &Opts) -> String {
    let mut t: String = o.find.clone();
    if t.chars().count() > 32 {
        t = t.chars().take(28).collect::<String>() + "...";
    }
    with_reason(format!("Find: Can't find the text \"{t}\""), 0, o)
}

pub fn regex_error_status(e: &str) -> String {
    format!("Find: Invalid Regular Expression\n{e}")
}

pub const END_REACHED: &str = "Find: Reached document end, first occurrence from the top found.";
pub const TOP_REACHED: &str =
    "Find:  Reached document beginning, first occurrence from the bottom found.";
pub const REPLACE_END_REACHED: &str = "Replace: Reached document end, started from top.";
pub const REPLACE_TOP_REACHED: &str = "Replace: Reached document beginning, started from bottom.";

pub fn replace_in_files_status(n: usize, skipped: &[PathBuf]) -> String {
    let mut m = if n == 1 {
        "Replace in Files: 1 occurrence was replaced.".to_string()
    } else {
        format!("Replace in Files: {n} occurrences were replaced.")
    };
    if !skipped.is_empty() {
        let names: Vec<String> = skipped
            .iter()
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        m += &format!(
            "\nSkipped (unsaved changes in an open tab): {}",
            names.join(", ")
        );
    }
    m
}

pub fn patterns(filters: &str) -> Vec<String> {
    let mut v: Vec<String> = filters
        .split(|c: char| c.is_whitespace() || c == ';')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if v.iter().all(|p| p.starts_with('!')) {
        v.insert(0, "*.*".into());
    }
    v
}

fn wildcard(p: &[char], n: &[char]) -> bool {
    match p.first() {
        None => n.is_empty(),
        Some('*') => (0..=n.len()).any(|i| wildcard(&p[1..], &n[i..])),
        Some('?') => !n.is_empty() && wildcard(&p[1..], &n[1..]),
        Some(c) => n.first() == Some(c) && wildcard(&p[1..], &n[1..]),
    }
}

fn spec(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    wildcard(&p, &n) || (p.ends_with(&['.', '*']) && wildcard(&p[..p.len() - 2], &n))
}

// Port of matchInList.
pub fn match_file(name: &str, pats: &[String]) -> bool {
    let mut hit = false;
    for p in pats {
        if p.len() > 1 && p.starts_with('!') {
            if spec(&p[1..], name) {
                return false;
            }
        } else if spec(p, name) {
            hit = true;
        }
    }
    hit
}

// Port of matchInExcludeDirList, with '/' accepted as well as '\'.
fn excluded_dir(name: &str, pats: &[String], level: usize) -> bool {
    pats.iter().any(|p| {
        if let Some(d) = p.strip_prefix("!+\\").or(p.strip_prefix("!+/")) {
            !d.is_empty() && spec(d, name)
        } else if let Some(d) = p.strip_prefix("!\\").or(p.strip_prefix("!/")) {
            level == 1 && !d.is_empty() && spec(d, name)
        } else {
            false
        }
    })
}

pub fn walk(dir: &Path, pats: &[String], sub: bool, hidden: bool) -> Vec<PathBuf> {
    let mut out = vec![];
    walk_level(dir, pats, sub, hidden, 1, &mut out);
    out
}

fn walk_level(
    dir: &Path,
    pats: &[String],
    sub: bool,
    hidden: bool,
    level: usize,
    out: &mut Vec<PathBuf>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if (hidden || !name.starts_with('.')) && sub && !excluded_dir(&name, pats, level) {
                walk_level(&e.path(), pats, sub, hidden, level + 1, out);
            }
        } else if e.path().is_file() && match_file(&name, pats) {
            out.push(e.path());
        }
    }
}

pub fn is_binary(b: &[u8]) -> bool {
    b.iter().take(8000).any(|&c| c == 0)
}

#[derive(Clone, Debug, Default)]
pub struct Line {
    pub text: Vec<u8>,
    pub marks: Vec<(isize, isize)>,
    pub hit: Option<(PathBuf, Vec<(isize, isize)>)>,
}

fn hits_str(n: usize) -> String {
    if n == 1 {
        "(1 hit)".into()
    } else {
        format!("({n} hits)")
    }
}

fn digits(n: isize) -> usize {
    n.max(1).to_string().len().min(7)
}

// Port of Finder::foundLine: "\tLine N: text" with the match offsets inside the result line.
pub fn found_line(
    line_no: isize,
    total: isize,
    text: &[u8],
    segs: &[(isize, isize)],
) -> (Vec<u8>, Vec<(isize, isize)>) {
    let mut cut = text.len().min(LINE_MAX);
    while cut < text.len() && cut > 0 && (text[cut] & 0xC0) == 0x80 {
        cut -= 1;
    }
    let pad = digits(total).saturating_sub(digits(line_no));
    let mut out = format!("\tLine {}{line_no}: ", " ".repeat(pad)).into_bytes();
    let h = out.len() as isize;
    out.extend_from_slice(&text[..cut]);
    let marks = segs
        .iter()
        .map(|&(s, e)| (h + s, h + e.min(cut as isize)))
        .collect();
    (out, marks)
}

fn mode_info(o: &Opts) -> String {
    let mut m = match o.mode {
        Mode::Normal => "Normal".to_string(),
        Mode::Extended => "Extended".to_string(),
        Mode::Regex => {
            if o.dot_nl {
                "RegEx.".into()
            } else {
                "RegEx".into()
            }
        }
    };
    let opts: Vec<&str> = [(o.match_case, "Case"), (o.whole_word, "Word")]
        .iter()
        .filter(|x| x.0)
        .map(|x| x.1)
        .collect();
    if !opts.is_empty() {
        m += ": ";
        m += &opts.join("/");
    }
    format!("[{m}]")
}

pub fn search_header(o: &Opts, hits: usize, files: usize, searched: usize) -> String {
    let name: String = o.find.chars().filter(|&c| c != '\r' && c != '\n').collect();
    format!(
        "Search \"{name}\" ({} {} in {} {} of {} searched) {}",
        commafy(hits),
        if hits == 1 { "hit" } else { "hits" },
        commafy(files),
        if files == 1 { "file" } else { "files" },
        commafy(searched),
        mode_info(o)
    )
}

// Finds matches of one document as result lines (file header first), one line per found line.
pub fn find_all_lines(doc: &Doc, o: &Opts, path: &Path) -> Result<Vec<Line>, String> {
    let found = process(doc, o, false, true, (0, doc.len()))?;
    if found.is_empty() {
        return Ok(vec![]);
    }
    let total = doc.lines();
    let mut lines = vec![Line {
        text: format!("  {} {}", path.display(), hits_str(found.len())).into_bytes(),
        ..Default::default()
    }];
    let mut grouped: Vec<(isize, Vec<(isize, isize)>)> = vec![];
    for (s, e) in found {
        let l = doc.line_of(s);
        match grouped.last_mut() {
            Some((pl, r)) if *pl == l => r.push((s, e)),
            _ => grouped.push((l, vec![(s, e)])),
        }
    }
    for (l, ranges) in grouped {
        let (ls, le) = doc.line_span(l);
        let segs: Vec<_> = ranges.iter().map(|&(s, e)| (s - ls, e - ls)).collect();
        let (text, marks) = found_line(l + 1, total, &doc.range(ls, le), &segs);
        lines.push(Line {
            text,
            marks,
            hit: Some((path.to_path_buf(), ranges)),
        });
    }
    Ok(lines)
}

pub struct FifOut {
    pub lines: Vec<Line>,
    pub count: usize,
    pub changed: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

pub struct FifArgs {
    pub dir: PathBuf,
    pub filters: String,
    pub sub: bool,
    pub hidden: bool,
    pub opts: Opts,
    pub replace: bool,
    pub skip: Vec<PathBuf>,
}

pub fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

// Find in Files and Replace in Files; replace writes only files that change.
pub fn find_in_files(a: &FifArgs) -> Result<FifOut, String> {
    find_in_files_raw(a).map_err(|e| regex_error_status(&e))
}

fn find_in_files_raw(a: &FifArgs) -> Result<FifOut, String> {
    let files = walk(&a.dir, &patterns(&a.filters), a.sub, a.hidden);
    let mut body = vec![];
    let (mut count, mut nfiles) = (0, 0);
    let (mut changed, mut skipped) = (vec![], vec![]);
    for f in &files {
        let c = canonical(f);
        if a.replace && a.skip.contains(&c) {
            skipped.push(f.clone());
            continue;
        }
        let Ok(b) = std::fs::read(f) else { continue };
        if is_binary(&b) {
            continue;
        }
        let Some(doc) = Doc::new(&b) else { continue };
        if a.replace {
            let n = process(&doc, &a.opts, true, true, (0, doc.len()))?.len();
            if n > 0 {
                if let Err(e) = std::fs::write(f, doc.text()) {
                    return Err(format!("{}: {e}", f.display()));
                }
                changed.push(c);
            }
            count += n;
        } else {
            let lines = find_all_lines(&doc, &a.opts, f)?;
            if !lines.is_empty() {
                count += lines
                    .iter()
                    .filter_map(|l| l.hit.as_ref())
                    .map(|h| h.1.len())
                    .sum::<usize>();
                nfiles += 1;
                body.extend(lines);
            }
        }
    }
    let mut lines = vec![];
    if !a.replace {
        lines.push(Line {
            text: search_header(&a.opts, count, nfiles, files.len()).into_bytes(),
            ..Default::default()
        });
        lines.extend(body);
    }
    Ok(FifOut {
        lines,
        count,
        changed,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(find: &str) -> Opts {
        Opts {
            find: find.into(),
            wrap: true,
            ..Default::default()
        }
    }

    fn count(text: &str, o: &Opts) -> usize {
        let d = Doc::new(text.as_bytes()).unwrap();
        process(&d, o, false, false, (0, d.len())).unwrap().len()
    }

    fn replaced(text: &str, o: &Opts) -> (usize, String) {
        let d = Doc::new(text.as_bytes()).unwrap();
        let n = replace_all(&d, o, (0, d.len())).unwrap();
        (n, String::from_utf8(d.text()).unwrap())
    }

    #[test]
    fn extended_conversion() {
        assert_eq!(extended(r"a\nb\r\t\\"), b"a\nb\r\t\\");
        assert_eq!(extended(r"\0"), b"\0");
        assert_eq!(extended(r"\x41é"), "A\u{e9}".as_bytes());
        assert_eq!(extended(r"\d065\o101\b01000001"), b"AAA");
        assert_eq!(extended(r"\xZ1\q"), br"\xZ1\q");
        assert_eq!(extended(r"\x4"), br"\x4");
        assert_eq!(extended(r"end\"), br"end\");
        assert_eq!(extended(r"\xff"), "\u{ff}".as_bytes());
    }

    #[test]
    fn filter_parsing() {
        assert_eq!(patterns("*.txt;*.md  *.rs"), ["*.txt", "*.md", "*.rs"]);
        assert_eq!(patterns(""), ["*.*"]);
        assert_eq!(patterns("!*.log"), ["*.*", "!*.log"]);
        let p = patterns("*.* !*.log");
        assert!(match_file("a.txt", &p));
        assert!(match_file("Makefile", &p));
        assert!(!match_file("x.LOG", &p));
        let p = patterns("*.TXT");
        assert!(match_file("a.txt", &p));
        assert!(!match_file("a.txt.bak", &p));
        assert!(match_file("ab.c", &patterns("a?.c")));
        assert!(!match_file("abc.c", &patterns("a?.c")));
    }

    fn tmp(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&d);
        for f in [
            "a.txt",
            "b.md",
            "sub/c.txt",
            "sub/deep/d.txt",
            ".hid/e.txt",
            "skip/f.txt",
            "sub/skip/g.txt",
        ] {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "foo bar\nbaz foo foo\n").unwrap();
        }
        std::fs::write(d.join("bin.txt"), b"foo\0bar").unwrap();
        d
    }

    fn names(v: &[PathBuf], root: &Path) -> Vec<String> {
        v.iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn file_walk() {
        let d = tmp("walk");
        let p = patterns("*.txt");
        assert_eq!(names(&walk(&d, &p, false, false), &d), ["a.txt", "bin.txt"]);
        assert_eq!(
            names(&walk(&d, &p, true, false), &d),
            [
                "a.txt",
                "bin.txt",
                "skip/f.txt",
                "sub/c.txt",
                "sub/deep/d.txt",
                "sub/skip/g.txt"
            ]
        );
        assert!(names(&walk(&d, &p, true, true), &d).contains(&".hid/e.txt".to_string()));
        let ex1 = patterns("*.txt !\\skip");
        assert_eq!(
            names(&walk(&d, &ex1, true, false), &d),
            [
                "a.txt",
                "bin.txt",
                "sub/c.txt",
                "sub/deep/d.txt",
                "sub/skip/g.txt"
            ]
        );
        let exall = patterns("*.txt !+\\skip");
        assert_eq!(
            names(&walk(&d, &exall, true, false), &d),
            ["a.txt", "bin.txt", "sub/c.txt", "sub/deep/d.txt"]
        );
    }

    #[test]
    fn result_line_format() {
        let (t, m) = found_line(7, 120, b"abc foo", &[(4, 7)]);
        assert_eq!(t, b"\tLine   7: abc foo");
        assert_eq!(m, [(15, 18)]);
        let (t, _) = found_line(12, 9, b"x", &[(0, 1)]);
        assert_eq!(t, b"\tLine 12: x");
        let o = Opts {
            find: "foo".into(),
            match_case: true,
            whole_word: true,
            ..Default::default()
        };
        assert_eq!(
            search_header(&o, 3, 2, 5),
            "Search \"foo\" (3 hits in 2 files of 5 searched) [Normal: Case/Word]"
        );
        let o = Opts {
            find: "a\nb".into(),
            mode: Mode::Regex,
            dot_nl: true,
            ..Default::default()
        };
        assert_eq!(
            search_header(&o, 1, 1, 1),
            "Search \"ab\" (1 hit in 1 file of 1 searched) [RegEx.]"
        );
        assert_eq!(commafy(1234567), "1,234,567");
        assert_eq!(
            count_status(3, &opts("x")),
            "Count: 3 matches in entire file"
        );
        assert_eq!(
            replace_all_status(1, &opts("x")),
            "Replace All: 1 occurrence was replaced in entire file"
        );
    }

    #[test]
    fn find_in_files_output() {
        let d = tmp("fif");
        let a = FifArgs {
            dir: d.clone(),
            filters: "*.txt".into(),
            sub: false,
            hidden: false,
            opts: opts("foo"),
            replace: false,
            skip: vec![],
        };
        let out = find_in_files(&a).unwrap();
        let text: Vec<String> = out
            .lines
            .iter()
            .map(|l| String::from_utf8_lossy(&l.text).into_owned())
            .collect();
        assert_eq!(
            text[0],
            "Search \"foo\" (3 hits in 1 file of 2 searched) [Normal]"
        );
        assert_eq!(text[1], format!("  {} (3 hits)", d.join("a.txt").display()));
        assert_eq!(text[2], "\tLine 1: foo bar");
        assert_eq!(text[3], "\tLine 2: baz foo foo");
        assert_eq!(out.lines[3].marks, [(13, 16), (17, 20)]);
        assert_eq!(out.lines[3].hit.as_ref().unwrap().1, [(12, 15), (16, 19)]);
        let r = FifArgs {
            replace: true,
            opts: Opts {
                replace: "X".into(),
                ..opts("foo")
            },
            ..a
        };
        assert_eq!(find_in_files(&r).unwrap().count, 3);
        assert_eq!(
            std::fs::read_to_string(d.join("a.txt")).unwrap(),
            "X bar\nbaz X X\n"
        );
        assert_eq!(std::fs::read(d.join("bin.txt")).unwrap(), b"foo\0bar");
        assert_eq!(
            std::fs::read_to_string(d.join("b.md")).unwrap(),
            "foo bar\nbaz foo foo\n"
        );
    }

    #[test]
    fn replace_in_files_skips_listed_files() {
        let d = tmp("skip");
        let a = FifArgs {
            dir: d.clone(),
            filters: "*.txt".into(),
            sub: true,
            hidden: false,
            opts: Opts {
                replace: "X".into(),
                ..opts("foo")
            },
            replace: true,
            skip: vec![canonical(&d.join("sub/c.txt"))],
        };
        let out = find_in_files(&a).unwrap();
        assert_eq!(out.skipped, [d.join("sub/c.txt")]);
        assert_eq!(
            std::fs::read_to_string(d.join("sub/c.txt")).unwrap(),
            "foo bar\nbaz foo foo\n"
        );
        assert_eq!(
            std::fs::read_to_string(d.join("a.txt")).unwrap(),
            "X bar\nbaz X X\n"
        );
        assert!(out.changed.contains(&canonical(&d.join("a.txt"))));
        assert!(!out.changed.contains(&canonical(&d.join("sub/c.txt"))));
        assert!(replace_in_files_status(out.count, &out.skipped)
            .ends_with("\nSkipped (unsaved changes in an open tab): c.txt"));
    }

    #[test]
    fn normal_word_case() {
        let t = "Foo foo food FOO";
        assert_eq!(count(t, &opts("foo")), 4);
        assert_eq!(
            count(
                t,
                &Opts {
                    whole_word: true,
                    ..opts("foo")
                }
            ),
            3
        );
        assert_eq!(
            count(
                t,
                &Opts {
                    match_case: true,
                    ..opts("foo")
                }
            ),
            2
        );
        assert_eq!(
            count(
                t,
                &Opts {
                    match_case: true,
                    whole_word: true,
                    ..opts("foo")
                }
            ),
            1
        );
    }

    #[test]
    fn boost_regex_back_reference() {
        let o = Opts {
            mode: Mode::Regex,
            replace: "<$1>".into(),
            ..opts(r"(\w+) \1")
        };
        assert_eq!(count("the the cat cat dog", &o), 2);
        assert_eq!(
            replaced("the the cat cat dog", &o),
            (2, "<the> <cat> dog".into())
        );
        let o = Opts {
            mode: Mode::Regex,
            replace: r"\2-\1".into(),
            ..opts(r"(\d+)x(\d+)")
        };
        assert_eq!(replaced("3x4 10x20", &o).1, "4-3 20-10");
        let bad = Opts {
            mode: Mode::Regex,
            ..opts("(")
        };
        let d = Doc::new(b"abc").unwrap();
        assert!(process(&d, &bad, false, false, (0, 3)).is_err());
        let dot = Opts {
            mode: Mode::Regex,
            ..opts("a.b")
        };
        assert_eq!(count("a\nb", &dot), 0);
        assert_eq!(
            count(
                "a\nb",
                &Opts {
                    dot_nl: true,
                    ..dot
                }
            ),
            1
        );
    }

    #[test]
    fn extended_crlf() {
        let o = Opts {
            mode: Mode::Extended,
            replace: r"\n".into(),
            ..opts(r"\r\n")
        };
        assert_eq!(count("a\r\nb\r\nc", &o), 2);
        assert_eq!(replaced("a\r\nb\r\nc", &o), (2, "a\nb\nc".into()));
    }

    #[test]
    fn replace_all_is_one_undo_step() {
        let d = Doc::new(b"a a a").unwrap();
        assert_eq!(
            replace_all(
                &d,
                &Opts {
                    replace: "bb".into(),
                    ..opts("a")
                },
                (0, 5)
            )
            .unwrap(),
            3
        );
        assert_eq!(d.text(), b"bb bb bb");
        unsafe { npp_doc_undo(d.p) };
        assert_eq!(d.text(), b"a a a");
    }

    #[test]
    fn find_next_wraps() {
        let d = Doc::new(b"foo bar foo").unwrap();
        let o = opts("foo");
        assert_eq!(
            find_next(&d, &o, (0, 0), false, Next::Find).unwrap(),
            Some(((0, 3), Wrap::No))
        );
        assert_eq!(
            find_next(&d, &o, (0, 3), false, Next::Find).unwrap(),
            Some(((8, 11), Wrap::No))
        );
        assert_eq!(
            find_next(&d, &o, (8, 11), false, Next::Find).unwrap(),
            Some(((0, 3), Wrap::End))
        );
        assert_eq!(
            find_next(&d, &o, (0, 3), true, Next::Find).unwrap(),
            Some(((8, 11), Wrap::Top))
        );
        let nowrap = Opts { wrap: false, ..o };
        assert_eq!(
            find_next(&d, &nowrap, (8, 11), false, Next::Find).unwrap(),
            None
        );
    }
}
