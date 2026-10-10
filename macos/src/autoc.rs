// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::attr;
use crate::sci::{self, send};
use crate::{language, macros, nested, ns, tagged, App};
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSEventModifierFlags, NSMenuItem, NSView};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::cell::RefCell;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void, CString};
use std::path::Path;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/apis.rs"));

const SCN_CHARADDED: u32 = 2001;
const SCN_CALLTIPCLICK: u32 = 2021;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GOTOPOS: u32 = 2025;
const SCI_AUTOCSHOW: u32 = 2100;
const SCI_AUTOCACTIVE: u32 = 2102;
const SCI_AUTOCSETSEPARATOR: u32 = 2106;
const SCI_AUTOCSETIGNORECASE: u32 = 2115;
const SCI_GETLINEENDPOSITION: u32 = 2136;
const SCI_REPLACETARGET: u32 = 2194;
const SCI_CALLTIPSHOW: u32 = 2200;
const SCI_CALLTIPCANCEL: u32 = 2201;
const SCI_CALLTIPACTIVE: u32 = 2202;
const SCI_CALLTIPSETHLT: u32 = 2204;
const SCI_WORDSTARTPOSITION: u32 = 2266;
const SCI_WORDENDPOSITION: u32 = 2267;
const SCI_AUTOCSETTYPESEPARATOR: u32 = 2286;
const SCI_REGISTERIMAGE: u32 = 2405;
const SCI_GETRANGEPOINTER: u32 = 2643;
const SCI_GETGAPPOSITION: u32 = 2644;
const SCI_AUTOCSETCASEINSENSITIVEBEHAVIOUR: u32 = 2634;
const SCI_AUTOCSETMULTI: u32 = 2636;
const SCI_SETTARGETRANGE: u32 = 2686;
const SC_MULTIAUTOC_EACH: usize = 1;

const FUNC_IMG_ID: usize = 1000;
const BOX_IMG_ID: usize = 1001;
const TYPE_SEP: char = '\x1E';
const MAX_PATH: isize = 260;
const MAX_PATH_ENTRIES: usize = 2000;

// Menu tags of the autoComplete: action.
const FUNC_COMPLETION: isize = 0;
const WORD_COMPLETION: isize = 1;
const PARAMS_HINT: isize = 2;
const PREV_HINT: isize = 3;
const NEXT_HINT: isize = 4;
const PATH_COMPLETION: isize = 5;
const FUNC_AND_WORD: isize = -1;

#[derive(Debug, Clone, PartialEq)]
pub struct Env {
    pub ignore_case: bool,
    pub start: u8,
    pub stop: u8,
    pub param: u8,
    pub terminal: u8,
    pub word_chars: Vec<u8>,
}

impl Default for Env {
    fn default() -> Self {
        Env {
            ignore_case: true,
            start: b'(',
            stop: b')',
            param: b',',
            terminal: b';',
            word_chars: vec![],
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overload {
    pub ret: String,
    pub descr: String,
    pub params: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct KeyWord {
    pub name: String,
    // The func attribute: Some(true) for "yes", Some(false) for another value, None when it is absent.
    pub func: Option<bool>,
    pub overloads: Vec<Overload>,
}

#[derive(Debug, Default)]
pub struct Api {
    pub env: Env,
    pub keywords: Vec<KeyWord>,
    // AutoCompletion::_keyWordArray: names with the image type, sorted.
    pub list: Vec<String>,
    pub tagged: HashSet<String>,
    pub max_len: usize,
}

fn opt_attr(e: &BytesStart, key: &str) -> Option<String> {
    e.try_get_attribute(key)
        .ok()
        .flatten()
        .map(|_| attr(e, key))
}

// Port of AutoCompletion::setLanguage and FunctionCallTip::loadFunction: reads an installer/APIs file.
pub fn parse_api(xml: &str) -> Option<Api> {
    let mut r = Reader::from_str(xml);
    let mut stack: Vec<Vec<u8>> = vec![];
    let (mut api, mut env_seen, mut ov_ok, mut roots) = (Api::default(), false, false, (0, 0));
    loop {
        let (e, open) = match r.read_event().ok()? {
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            Event::End(_) => {
                stack.pop();
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let name = e.name().as_ref().as_bytes().to_vec();
        let path: Vec<&[u8]> = stack.iter().map(|s| s.as_slice()).collect();
        match (path.as_slice(), name.as_slice()) {
            ([], b"NotepadPlus") => roots.0 += 1,
            ([b"NotepadPlus"], b"AutoComplete") if roots.0 == 1 => roots.1 += 1,
            ([b"NotepadPlus", b"AutoComplete"], b"Environment") if roots == (1, 1) && !env_seen => {
                env_seen = true;
                let first = |k: &str| opt_attr(&e, k).and_then(|v| v.bytes().next());
                let env = &mut api.env;
                env.ignore_case = opt_attr(&e, "ignoreCase").as_deref() != Some("no");
                env.start = first("startFunc").unwrap_or(env.start);
                env.stop = first("stopFunc").unwrap_or(env.stop);
                env.param = first("paramSeparator").unwrap_or(env.param);
                env.terminal = first("terminal").unwrap_or(env.terminal);
                env.word_chars = attr(&e, "additionalWordChar").into_bytes();
            }
            ([b"NotepadPlus", b"AutoComplete"], b"KeyWord") if roots == (1, 1) => {
                api.keywords.push(KeyWord {
                    name: attr(&e, "name"),
                    func: opt_attr(&e, "func").map(|f| f == "yes"),
                    overloads: vec![],
                })
            }
            ([b"NotepadPlus", b"AutoComplete", b"KeyWord"], b"Overload") if roots == (1, 1) => {
                ov_ok = false;
                if let (Some(ret), Some(k)) = (opt_attr(&e, "retVal"), api.keywords.last_mut()) {
                    ov_ok = true;
                    k.overloads.push(Overload {
                        ret,
                        descr: attr(&e, "descr"),
                        params: vec![],
                    });
                }
            }
            ([b"NotepadPlus", b"AutoComplete", b"KeyWord", b"Overload"], b"Param")
                if roots == (1, 1) && ov_ok =>
            {
                let o = api.keywords.last_mut().and_then(|k| k.overloads.last_mut());
                if let (Some(p), Some(o)) = (opt_attr(&e, "name"), o) {
                    o.params.push(p);
                }
            }
            _ => {}
        }
        if open {
            stack.push(name);
        }
    }
    if api.keywords.is_empty() {
        return None;
    }
    for k in api.keywords.iter().filter(|k| !k.name.is_empty()) {
        let id = if k.func == Some(true) {
            FUNC_IMG_ID
        } else {
            BOX_IMG_ID
        };
        api.list.push(format!("{}{TYPE_SEP}{id}", k.name));
        api.max_len = api.max_len.max(k.name.len());
    }
    sort_words(&mut api.list, api.env.ignore_case);
    api.tagged = api.list.iter().cloned().collect();
    Some(api)
}

impl Api {
    // FunctionCallTip::loadFunction: the first keyword with the name decides.
    pub fn function(&self, name: &str) -> Option<usize> {
        let i = self
            .keywords
            .iter()
            .position(|k| k.func.is_some() && same_name(&k.name, name, self.env.ignore_case))?;
        let k = &self.keywords[i];
        (k.func == Some(true) && !k.overloads.is_empty()).then_some(i)
    }
}

fn same_name(a: &str, b: &str, ignore_case: bool) -> bool {
    if ignore_case {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

// sortInsensitive in AutoCompletion.cpp, or a plain byte sort.
fn sort_words(v: &mut [String], ignore_case: bool) {
    if ignore_case {
        v.sort_by(|a, b| {
            a.bytes()
                .map(|c| c.to_ascii_uppercase())
                .cmp(b.bytes().map(|c| c.to_ascii_uppercase()))
        });
    } else {
        v.sort();
    }
}

// Notepad++ API file name of a language: the language name, with javascript.js read from javascript.xml.
pub fn api_file(lang: &str) -> &str {
    if lang == "javascript.js" {
        "javascript"
    } else {
        lang
    }
}

fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

// The document as Scintilla stores it: the text before and after the gap.
#[derive(Clone, Copy, Default)]
pub struct Text<'a>(pub &'a [u8], pub &'a [u8]);

impl Text<'_> {
    pub fn len(&self) -> usize {
        self.0.len() + self.1.len()
    }

    fn at(&self, i: usize) -> u8 {
        match i.checked_sub(self.0.len()) {
            None => self.0[i],
            Some(k) => self.1[k],
        }
    }

    pub fn range(&self, s: usize, e: usize) -> Cow<'_, [u8]> {
        let n = self.0.len();
        if e <= n {
            Cow::Borrowed(&self.0[s..e])
        } else if s >= n {
            Cow::Borrowed(&self.1[s - n..e - n])
        } else {
            Cow::Owned([&self.0[s..], &self.1[..e - n]].concat())
        }
    }
}

// AutoCompletion::getWordArray: words that start at a word start with the prefix, one character or more longer.
pub fn doc_words(doc: Text, prefix: &[u8], exclude: &[u8], match_case: bool) -> Vec<String> {
    const STOP: &[u8] = b" \t\n\r.,;:\"(){}=<>'+!?[]";
    let mut out: Vec<String> = vec![];
    if prefix.is_empty() || (crate::prefs::with(|p| p.autoc_ignore_numbers) && prefix.iter().all(u8::is_ascii_digit)) {
        return out;
    }
    let mut seen: HashSet<Cow<[u8]>> = HashSet::new();
    let same = |a: u8, b: u8| if match_case { a == b } else { a.eq_ignore_ascii_case(&b) };
    let (n, len) = (prefix.len(), doc.len());
    let mut i = 0;
    while i + n < len {
        let hit = (i == 0 || !is_word_byte(doc.at(i - 1))) && (0..n).all(|k| same(doc.at(i + k), prefix[k]));
        let mut j = i + n;
        if hit {
            while j < len && !STOP.contains(&doc.at(j)) {
                j += 1;
            }
        }
        if !hit || j == i + n {
            i += 1;
            continue;
        }
        let w = doc.range(i, j);
        if j - i < 256 && *w != *exclude && !seen.contains(&w) {
            out.push(String::from_utf8_lossy(&w).into_owned());
            seen.insert(w);
        }
        i = j;
    }
    out
}

// AutoCompletion::showAutoComplete: tags document words with the API image type, adds matching keywords, sorts.
pub fn word_list(
    words: Vec<String>,
    api: Option<&Api>,
    prefix: &[u8],
    keywords: bool,
) -> Vec<String> {
    let ignore_case = api.is_none_or(|a| a.env.ignore_case);
    let tagged = |w: String| {
        let Some(a) = api else { return w };
        [BOX_IMG_ID, FUNC_IMG_ID]
            .iter()
            .map(|id| format!("{w}{TYPE_SEP}{id}"))
            .find(|t| a.tagged.contains(t))
            .unwrap_or(w)
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = vec![];
    for w in words.into_iter().map(tagged) {
        if seen.insert(w.clone()) {
            out.push(w);
        }
    }
    if let Some(a) = api.filter(|_| keywords) {
        for k in &a.list {
            let head = &k.as_bytes()[..prefix.len().min(k.len())];
            let hit = if ignore_case {
                head.eq_ignore_ascii_case(prefix)
            } else {
                head == prefix
            };
            if hit && seen.insert(k.clone()) {
                out.push(k.clone());
            }
        }
    }
    sort_words(&mut out, ignore_case);
    out
}

// FunctionCallTip::getCursorFunction: the function name and the parameter index at the caret offset in the line.
pub fn cursor_function(line: &[u8], offset: usize, env: &Env) -> Option<(String, usize)> {
    if offset < 2 || line.len() + 3 >= 256 {
        return None;
    }
    let offset = offset.min(line.len());
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || env.word_chars.contains(&c);
    let mut tokens: Vec<(usize, usize, bool)> = vec![];
    let mut i = 0;
    while i < offset {
        let c = line[i];
        if word(c) {
            let s = i;
            while i < offset && word(line[i]) {
                i += 1;
            }
            tokens.push((s, i - s, true));
            continue;
        }
        if c == b'"' || c == b'\'' {
            i += 1;
            while i < offset && line[i] != c {
                if line[i] == b'\\' && i + 1 < offset {
                    i += 1;
                }
                i += 1;
            }
        } else if !b" \t\n\r".contains(&c) {
            tokens.push((i, 1, false));
        }
        i += 1;
    }
    #[derive(Clone, Copy)]
    struct V {
        last_id: isize,
        func: isize,
        param: usize,
    }
    let fresh = V {
        last_id: -1,
        func: -1,
        param: 0,
    };
    let (mut cur, mut stack) = (fresh, vec![]);
    for (k, &(s, _, is_id)) in tokens.iter().enumerate() {
        let k = k as isize;
        if is_id {
            cur.last_id = k;
            continue;
        }
        let c = line[s];
        if c == env.start {
            stack.push(cur);
            if k > 0 && cur.last_id == k - 1 {
                cur.func = cur.last_id;
                cur.param = 0;
            } else {
                cur.func = -1;
            }
        } else if c == env.param && cur.func > -1 {
            cur.param += 1;
        } else if c == env.stop {
            cur = stack.pop().unwrap_or(fresh);
        } else if c == env.terminal {
            stack.clear();
            cur = fresh;
        }
    }
    while cur.func == -1 {
        cur = stack.pop()?;
    }
    let (s, n, _) = tokens[cur.func as usize];
    Some((
        String::from_utf8_lossy(&line[s..s + n]).into_owned(),
        cur.param,
    ))
}

// FunctionCallTip::showCalltip: the text and the highlight range of the current parameter; it can change the overload.
pub fn calltip_text(
    name: &str,
    ovs: &[Overload],
    overload: &mut usize,
    param: usize,
    env: &Env,
) -> (String, Option<(usize, usize)>) {
    if param >= ovs[*overload].params.len() + 1 {
        if let Some(i) = ovs.iter().position(|o| param < o.params.len() + 1) {
            *overload = i;
        }
    }
    let o = &ovs[*overload];
    let mut t = String::new();
    if ovs.len() > 1 {
        t += &format!("\u{1}{} of {}\u{2}", *overload + 1, ovs.len());
    }
    t += &format!("{} {name} {}", o.ret, env.start as char);
    let mut hl = None;
    for (i, p) in o.params.iter().enumerate() {
        if i == param {
            hl = Some((t.len(), t.len() + p.len()));
        }
        t += p;
        if i + 1 < o.params.len() {
            t.push(env.param as char);
            t.push(' ');
        }
    }
    t.push(env.stop as char);
    if !o.descr.is_empty() {
        t += "\n";
        t += &o.descr;
    }
    (t, hl.filter(|(s, e)| s != e))
}

// Unix form of getRawPath: the last "/" or "~/" at the line start or after a space, a quote, or "(".
pub fn path_start(input: &str) -> Option<usize> {
    let ok = |i: usize| {
        i == 0
            || input[..i]
                .ends_with(|c: char| c == '\'' || c == '"' || c == '(' || c.is_whitespace())
    };
    input.char_indices().rev().find_map(|(i, c)| {
        let start = c == '/' || (c == '~' && input[i + 1..].starts_with('/'));
        (start && ok(i)).then_some(i)
    })
}

// getPathsForPathCompletion: the directory to list, as typed, for the raw path.
pub fn path_dir(
    raw: &str,
    is_dir: impl Fn(&str) -> bool,
    is_file: impl Fn(&str) -> bool,
) -> Option<String> {
    if is_file(raw.strip_suffix('/').unwrap_or(raw)) {
        None
    } else if is_dir(raw) {
        Some(raw.to_string())
    } else {
        raw.rfind('/').map(|i| raw[..i].to_string())
    }
}

pub fn with_slash(dir: &str) -> String {
    if dir.ends_with('/') {
        dir.to_string()
    } else {
        format!("{dir}/")
    }
}

fn expand(p: &str) -> String {
    match (p.strip_prefix('~'), std::env::var("HOME")) {
        (Some(rest), Ok(home)) if rest.is_empty() || rest.starts_with('/') => home + rest,
        _ => p.to_string(),
    }
}

// The entries as AutoCompletion::showPathCompletion lists them, sorted for the Scintilla list without case.
pub fn path_entries(dir: &str, mut names: Vec<(String, bool)>) -> String {
    names.sort_by_key(|(n, _)| n.to_lowercase());
    names
        .iter()
        .map(|(n, d)| format!("{dir}{n}{}", if *d { "/" } else { "" }))
        .collect::<Vec<_>>()
        .join("\n")
}

const AUTOC_SRC: &str = include_str!("../../PowerEditor/src/ScintillaComponent/AutoCompletion.cpp");

// Lines of a `static constexpr const char* name[]{ "..", ... };` XPM array in AutoCompletion.cpp.
pub fn xpm_lines(src: &str, name: &str) -> Vec<CString> {
    let Some(at) = src.find(&format!(" {name}[]")) else {
        return vec![];
    };
    let body = &src[at..];
    let body = &body[..body.find("};").unwrap_or(body.len())];
    body.split('"')
        .skip(1)
        .step_by(2)
        .filter_map(|l| CString::new(l).ok())
        .collect()
}

// Registers the list images once for each view.
fn register_images(v: &Retained<NSView>) {
    let new = IMAGES_SET.with(|set| {
        let mut set = set.borrow_mut();
        set.retain(|w| w.load().is_some());
        let new = !set.iter().any(|w| w.load().is_some_and(|x| Retained::as_ptr(&x) == Retained::as_ptr(v)));
        if new {
            set.push(Weak::from_retained(v));
        }
        new
    });
    if !new {
        return;
    }
    for (id, name) in [(FUNC_IMG_ID, "xpmfn"), (BOX_IMG_ID, "xpmbox")] {
        let lines = xpm_lines(AUTOC_SRC, name);
        let ptrs: Vec<*const c_char> = lines.iter().map(|l| l.as_ptr()).collect();
        if !ptrs.is_empty() {
            send(v, SCI_REGISTERIMAGE, id, ptrs.as_ptr() as isize);
        }
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<Rc<Api>>>> = RefCell::new(HashMap::new());
    static TIP: RefCell<Tip> = RefCell::new(Tip::default());
    static IMAGES_SET: RefCell<Vec<Weak<NSView>>> = const { RefCell::new(vec![]) };
}

pub fn api_for(lang: &str) -> Option<Rc<Api>> {
    let file = api_file(lang);
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(file.to_string())
            .or_insert_with(|| {
                let (_, xml) = APIS.iter().find(|(n, _)| n.eq_ignore_ascii_case(file))?;
                parse_api(xml).map(Rc::new)
            })
            .clone()
    })
}

// FunctionCallTip state for one view and one API.
#[derive(Default)]
struct Tip {
    owner: (usize, usize),
    name: String,
    func: Option<usize>,
    overload: usize,
    param: usize,
    cur: isize,
    start: isize,
    own: bool,
}

// SCNotification up to the ch field.
#[repr(C)]
struct CharNotify {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
    position: isize,
    ch: i32,
}

struct Ctx {
    v: Retained<NSView>,
    lang: String,
    api: Option<Rc<Api>>,
}

impl Ctx {
    fn s(&self, m: u32, w: usize, l: isize) -> isize {
        send(&self.v, m, w, l)
    }

    fn caret(&self) -> isize {
        self.s(SCI_GETCURRENTPOS, 0, 0)
    }

    // The document bytes on the two sides of the gap, so the gap does not move; valid until the next change.
    fn text(&self) -> Text<'_> {
        let gap = self.s(SCI_GETGAPPOSITION, 0, 0);
        let n = sci::length(&self.v);
        let part = |s: isize, e: isize| -> &[u8] {
            let p = self.s(SCI_GETRANGEPOINTER, s as usize, e - s) as *const u8;
            if p.is_null() || e <= s {
                return &[];
            }
            unsafe { std::slice::from_raw_parts(p, (e - s) as usize) }
        };
        Text(part(0, gap), part(gap, n))
    }

    fn word_start(&self, pos: isize) -> isize {
        self.s(SCI_WORDSTARTPOSITION, pos as usize, 1)
    }

    fn calltip_visible(&self) -> bool {
        self.s(SCI_CALLTIPACTIVE, 0, 0) != 0
    }

    fn show_list(&self, entered: isize, list: &str, sep: u8, ignore_case: bool) {
        self.s(SCI_AUTOCSETMULTI, SC_MULTIAUTOC_EACH, 0);
        self.s(SCI_AUTOCSETTYPESEPARATOR, TYPE_SEP as usize, 0);
        self.s(SCI_AUTOCSETSEPARATOR, sep as usize, 0);
        self.s(SCI_AUTOCSETIGNORECASE, ignore_case as usize, 0);
        self.s(
            SCI_AUTOCSETCASEINSENSITIVEBEHAVIOUR,
            ignore_case as usize,
            0,
        );
        let c = CString::new(list.replace('\0', "")).unwrap();
        self.s(SCI_AUTOCSHOW, entered as usize, c.as_ptr() as isize);
    }

    // AutoCompletion::showAutoComplete for the autocFunc, autocWord and autocFuncAndWord types.
    fn show_complete(&self, tag: isize, auto_insert: bool) -> bool {
        if tag == FUNC_COMPLETION && self.api.is_none() {
            return false;
        }
        let cur = self.caret();
        let start = self.word_start(cur);
        let len = (cur - start) as usize;
        if cur == start {
            return false;
        }
        let ignore_case = self.api.as_ref().is_none_or(|a| a.env.ignore_case);
        let words = if tag == FUNC_COMPLETION {
            let a = self.api.as_ref().unwrap();
            if len >= a.max_len {
                return false;
            }
            a.list.clone()
        } else {
            let end = self.s(SCI_WORDENDPOSITION, cur as usize, 1);
            if len >= 256 || (end - start) >= 256 {
                return false;
            }
            let doc = self.text();
            let prefix = doc.range(start as usize, cur as usize).into_owned();
            let exclude = doc.range(start as usize, end as usize);
            let match_case = !ignore_case || (self.api.is_none() && self.lang == "normal");
            let found = doc_words(doc, &prefix, &exclude, match_case);
            let list = word_list(found, self.api.as_deref(), &prefix, tag != WORD_COMPLETION);
            if list.is_empty() {
                return false;
            }
            if tag == WORD_COMPLETION && list.len() == 1 && auto_insert {
                let w = list[0].split(TYPE_SEP).next().unwrap_or_default();
                self.s(SCI_SETTARGETRANGE, start as usize, cur);
                let n = self.s(SCI_REPLACETARGET, w.len(), w.as_ptr() as isize);
                self.s(SCI_GOTOPOS, (start + n) as usize, 0);
                return true;
            }
            list
        };
        register_images(&self.v);
        self.show_list(cur - start, &words.join(" "), b' ', ignore_case);
        true
    }

    // AutoCompletion::showPathCompletion with Unix paths.
    fn show_path(&self) {
        let cur = self.caret();
        let line = sci::send(&self.v, sci::SCI_LINEFROMPOSITION, cur as usize, 0);
        let from = (cur - MAX_PATH).max(sci::send(
            &self.v,
            sci::SCI_POSITIONFROMLINE,
            line as usize,
            0,
        ));
        let input = String::from_utf8_lossy(&self.text().range(from as usize, cur as usize)).into_owned();
        let Some(raw) = path_start(&input).map(|i| &input[i..]) else {
            return;
        };
        let is_dir = |p: &str| Path::new(&expand(p)).is_dir();
        let is_file = |p: &str| Path::new(&expand(p)).is_file();
        let Some(dir) = path_dir(raw, is_dir, is_file).map(|d| with_slash(&d)) else {
            return;
        };
        let Ok(rd) = std::fs::read_dir(expand(&dir)) else {
            return;
        };
        let names = rd
            .flatten()
            .take(MAX_PATH_ENTRIES)
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    e.file_type().is_ok_and(|t| t.is_dir() || (t.is_symlink() && e.path().is_dir())),
                )
            })
            .collect();
        self.show_list(raw.len() as isize, &path_entries(&dir, names), b'\n', true);
    }

    fn with_tip<R>(&self, f: impl FnOnce(&mut Tip) -> R) -> R {
        let owner = (
            Retained::as_ptr(&self.v) as usize,
            self.api.as_ref().map_or(0, |a| Rc::as_ptr(a) as usize),
        );
        TIP.with(|t| {
            let mut t = t.borrow_mut();
            if t.owner != owner {
                *t = Tip {
                    owner,
                    ..Tip::default()
                };
            }
            f(&mut t)
        })
    }

    fn close_tip(&self) {
        let visible = self.calltip_visible();
        let cancel = self.with_tip(|t| {
            let own = visible && t.own;
            if own {
                t.own = false;
                t.overload = 0;
            }
            own
        });
        if cancel {
            self.s(SCI_CALLTIPCANCEL, 0, 0);
        }
    }

    fn show_tip(&self) {
        let Some(a) = self.api.as_ref() else { return };
        let visible = self.calltip_visible();
        let (start, text, hl) = self
            .with_tip(|t| {
                let f = &a.keywords[t.func?];
                let (text, hl) =
                    calltip_text(&t.name, &f.overloads, &mut t.overload, t.param, &a.env);
                if !visible {
                    t.start = t.cur;
                }
                t.own = true;
                Some((t.start, text, hl))
            })
            .unwrap_or_default();
        if text.is_empty() {
            return;
        }
        if visible {
            self.s(SCI_CALLTIPCANCEL, 0, 0);
        }
        let c = CString::new(text.replace('\0', "")).unwrap();
        self.s(SCI_CALLTIPSHOW, start as usize, c.as_ptr() as isize);
        if let Some((s, e)) = hl {
            self.s(SCI_CALLTIPSETHLT, s, e as isize);
        }
    }

    // FunctionCallTip::updateCalltip.
    fn update_tip(&self, ch: i32, need_shown: bool) -> bool {
        let Some(a) = self.api.clone() else {
            self.close_tip();
            return false;
        };
        let env = &a.env;
        if !need_shown
            && ch != env.start as i32
            && ch != env.param as i32
            && !self.calltip_visible()
        {
            return false;
        }
        let cur = self.caret();
        let line = self.s(sci::SCI_LINEFROMPOSITION, cur as usize, 0);
        let ls = self.s(sci::SCI_POSITIONFROMLINE, line as usize, 0);
        let le = self.s(SCI_GETLINEENDPOSITION, line as usize, 0);
        let found = cursor_function(
            &self.text().range(ls as usize, le as usize),
            (cur - ls) as usize,
            env,
        );
        let ok = found.is_some_and(|(name, param)| {
            self.with_tip(|t| {
                if !same_name(&t.name, &name, env.ignore_case) || t.func.is_none() {
                    t.func = a.function(&name);
                    t.name = name;
                    t.overload = 0;
                }
                t.param = param;
                t.cur = cur;
                t.func.is_some()
            })
        });
        if !ok {
            self.close_tip();
            return false;
        }
        self.show_tip();
        true
    }

    fn cycle_tip(&self, next: bool) {
        let Some(a) = self.api.as_ref() else { return };
        if !self.calltip_visible() {
            return;
        }
        self.with_tip(|t| {
            let n = t.func.map_or(0, |f| a.keywords[f].overloads.len());
            if n > 0 {
                t.overload = if next {
                    (t.overload + 1) % n
                } else {
                    t.overload.checked_sub(1).unwrap_or(n - 1)
                };
            }
        });
        self.show_tip();
    }
}

impl App {
    // Language of the current tab for auto-completion: the Language menu choice, else the file name.
    fn autoc_lang(&self) -> String {
        let t = self.current().and_then(|i| self.tab(i));
        t.as_ref()
            .and_then(language::tab_language)
            .map_or("normal".into(), |l| l.name.clone())
    }

    // Previous and Next Hint are on only while a call tip shows, so Opt+Up and Opt+Down reach other controls.
    pub(crate) fn validate_autoc(&self, item: &NSMenuItem) -> Option<bool> {
        let hint = item.action() == Some(sel!(autoComplete:)) && matches!(item.tag(), PREV_HINT | NEXT_HINT);
        hint.then(|| self.editor().is_some_and(|v| send(&v, SCI_CALLTIPACTIVE, 0, 0) != 0))
    }

    fn autoc_ctx(&self) -> Option<Ctx> {
        let lang = self.autoc_lang();
        Some(Ctx {
            v: self.editor()?,
            api: api_for(&lang),
            lang,
        })
    }

    pub(crate) fn autoc_cmd(&self, tag: isize) {
        let Some(c) = self.autoc_ctx() else { return };
        match tag {
            FUNC_COMPLETION => drop(c.show_complete(FUNC_COMPLETION, false)),
            WORD_COMPLETION => drop(c.show_complete(WORD_COMPLETION, true)),
            PARAMS_HINT => drop(c.update_tip(0, true)),
            PREV_HINT | NEXT_HINT => c.cycle_tip(tag == NEXT_HINT),
            PATH_COMPLETION => c.show_path(),
            _ => {}
        }
    }

    // SCN_CHARADDED runs AutoCompletion::update; SCN_CALLTIPCLICK changes the overload.
    pub(crate) fn autoc_notify(&self, scn: *const c_void) {
        let n = unsafe { &*(scn as *const CharNotify) };
        if n.id_from == sci::RESULTS_ID {
            return;
        }
        match (n.code, n.position) {
            (SCN_CALLTIPCLICK, 1) => self.autoc_cmd(PREV_HINT),
            (SCN_CALLTIPCLICK, 2) => self.autoc_cmd(NEXT_HINT),
            (SCN_CHARADDED, _) if n.ch != 0 && !macros::recording() => {
                self.editor().inspect(|v| crate::edit_assist::char_added(v, n.ch as u32, &self.autoc_lang()));
                let Some(c) = self.autoc_ctx() else { return };
                let p = crate::prefs::with(|p| p.auto_completion());
                if (p.func_params || c.calltip_visible()) && c.update_tip(n.ch, false) {
                    return;
                }
                if c.s(SCI_AUTOCACTIVE, 0, 0) != 0 {
                    return;
                }
                let cur = c.caret();
                let len = (cur - c.word_start(cur)) as usize;
                let tag = match p.action {
                    crate::prefs::AUTOC_FUNC => FUNC_COMPLETION,
                    crate::prefs::AUTOC_WORD => WORD_COMPLETION,
                    _ => FUNC_AND_WORD,
                };
                if p.action != crate::prefs::AUTOC_NONE && len < 64 && len >= p.from_len {
                    c.show_complete(tag, false);
                }
            }
            _ => {}
        }
    }
}

// Edit > Auto-Completion. Ctrl replaces Cmd for the Space keys, because Cmd+Space is Spotlight.
pub fn autoc_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    let (cmd, ctrl, opt, shift) = (
        NSEventModifierFlags::Command,
        NSEventModifierFlags::Control,
        NSEventModifierFlags::Option,
        NSEventModifierFlags::Shift,
    );
    let i = |title: &str, tag: isize, key: &str, mods: NSEventModifierFlags| {
        let i = tagged(mtm, title, sel!(autoComplete:), tag, t);
        i.setKeyEquivalent(&ns(key));
        i.setKeyEquivalentModifierMask(mods);
        i
    };
    nested(
        mtm,
        "Auto-Completion",
        vec![
            i("Function Completion", FUNC_COMPLETION, " ", ctrl),
            i("Word Completion", WORD_COMPLETION, "\r", cmd),
            i("Function Parameters Hint", PARAMS_HINT, " ", ctrl | shift),
            i(
                "Function Parameters Previous Hint",
                PREV_HINT,
                "\u{f700}",
                opt,
            ),
            i("Function Parameters Next Hint", NEXT_HINT, "\u{f701}", opt),
            i("Path Completion", PATH_COMPLETION, " ", ctrl | opt),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<NotepadPlus>
    <AutoComplete language="test">
        <Environment ignoreCase="no" startFunc="(" stopFunc=")" paramSeparator="," additionalWordChar = "." />
        <KeyWord name="abs" func="yes">
            <Overload retVal="int" descr="Absolute &amp; value">
                <Param name="int x" />
            </Overload>
            <Overload retVal="float">
                <Param name="float x" />
                <Param name="float y" />
            </Overload>
            <Overload descr="no retVal, skipped"><Param name="bad" /></Overload>
        </KeyWord>
        <KeyWord name="Abc" />
        <KeyWord name="and" func="no" />
        <KeyWord name="os.path" func="yes"><Overload retVal=""></Overload></KeyWord>
    </AutoComplete>
</NotepadPlus>"#;

    #[test]
    fn api_xml_parsed() {
        let a = parse_api(XML).unwrap();
        assert!(!a.env.ignore_case);
        assert_eq!(a.env.word_chars, b".");
        assert_eq!(a.env.terminal, b';');
        assert_eq!(a.keywords.len(), 4);
        let abs = &a.keywords[0];
        assert_eq!(abs.func, Some(true));
        assert_eq!(abs.overloads.len(), 2);
        assert_eq!(abs.overloads[0].descr, "Absolute & value");
        assert_eq!(abs.overloads[1].params, ["float x", "float y"]);
        assert_eq!(a.keywords[1].func, None);
        assert_eq!(a.max_len, 7);
        assert_eq!(
            a.list,
            [
                "Abc\x1E1001",
                "abs\x1E1000",
                "and\x1E1001",
                "os.path\x1E1000"
            ]
        );
        assert_eq!(a.function("abs"), Some(0));
        assert_eq!(a.function("ABS"), None);
        assert_eq!(a.function("and"), None);
        assert!(parse_api("<NotepadPlus><AutoComplete/></NotepadPlus>").is_none());
    }

    #[test]
    fn all_api_files_parse() {
        assert_eq!(APIS.len(), 34);
        for (name, xml) in APIS {
            let a = parse_api(xml).unwrap_or_else(|| panic!("{name}"));
            assert!(!a.list.is_empty(), "{name}");
        }
        let py = api_for("python").unwrap();
        assert!(!py.env.ignore_case);
        assert!(py.function("abs").is_some());
        assert!(api_for("baanc").is_some());
        assert!(api_for("javascript.js").is_some());
        assert!(api_for("normal").is_none());
        assert!(api_for("coffeescript").is_none());
    }

    #[test]
    fn keywords_by_prefix() {
        let a = parse_api(XML).unwrap();
        let l = word_list(vec![], Some(&a), b"ab", true);
        assert_eq!(l, ["abs\x1E1000"]);
        let mut ci = parse_api(XML).unwrap();
        ci.env.ignore_case = true;
        let l = word_list(vec!["abacus".into(), "abs".into()], Some(&ci), b"AB", true);
        assert_eq!(l, ["abacus", "Abc\x1E1001", "abs\x1E1000"]);
        let l = word_list(vec!["abs".into()], Some(&a), b"ab", false);
        assert_eq!(l, ["abs\x1E1000"]);
        assert_eq!(
            word_list(vec!["b".into(), "A".into()], None, b"", false),
            ["A", "b"]
        );
    }

    #[test]
    fn words_from_document() {
        let doc: &[u8] = b"foo foobar foo-bar xfoobaz Foobig foobar foo.x foo";
        let one = Text(doc, &[]);
        assert_eq!(doc_words(one, b"foo", b"foo", true), ["foobar", "foo-bar"]);
        assert_eq!(doc_words(one, b"foo", b"foobar", false), ["foo-bar", "Foobig"]);
        assert!(doc_words(Text(b"123 1234", &[]), b"12", b"", true).is_empty());
        assert_eq!(doc_words(Text(b"a_1 a_2", &[]), b"a", b"a_2", true), ["a_1"]);
        for gap in 0..=doc.len() {
            let split = Text(&doc[..gap], &doc[gap..]);
            assert_eq!(doc_words(split, b"foo", b"foo", false), doc_words(one, b"foo", b"foo", false), "{gap}");
            assert_eq!(&*split.range(3, 20), &doc[3..20]);
        }
    }

    #[test]
    fn many_words_build_fast() {
        let mut doc = Vec::new();
        for i in 0..50_000 {
            doc.extend_from_slice(format!("w{i} xyzzy{} ", i % 7).as_bytes());
        }
        while doc.len() < 5_000_000 {
            doc.extend_from_slice(b"filler text, (more) = filler;\n");
        }
        let php = api_for("php").unwrap();
        let t = std::time::Instant::now();
        let (a, b) = doc.split_at(doc.len() / 2);
        let words = doc_words(Text(a, b), b"w", b"", false);
        let list = word_list(words, Some(&php), b"w", true);
        let ms = t.elapsed().as_millis();
        assert!(list.len() >= 50_000);
        let limit = if cfg!(debug_assertions) { 5000 } else { 200 };
        assert!(ms < limit, "{ms} ms");
    }

    #[test]
    fn calltip_param_range() {
        let env = Env::default();
        let f = |s: &str| cursor_function(s.as_bytes(), s.len(), &env);
        assert_eq!(f("x = max(a, b"), Some(("max".into(), 1)));
        assert_eq!(f("max(a, min(b"), Some(("min".into(), 0)));
        assert_eq!(f("max(a, min(b, c), "), Some(("max".into(), 2)));
        assert_eq!(f("max(a, \"x,(y\", "), Some(("max".into(), 2)));
        assert_eq!(f("max(a); foo"), None);
        assert_eq!(f("(a, b"), None);
        assert_eq!(f("f"), None);
        let py = Env {
            word_chars: b".".to_vec(),
            ..Env::default()
        };
        assert_eq!(
            cursor_function(b"os.path.join(a, b)", 15, &py),
            Some(("os.path.join".into(), 1))
        );
        let a = parse_api(XML).unwrap();
        let ovs = &a.keywords[0].overloads;
        let mut o = 0;
        let (t, hl) = calltip_text("abs", ovs, &mut o, 0, &env);
        assert_eq!(t, "\u{1}1 of 2\u{2}int abs (int x)\nAbsolute & value");
        assert_eq!(&t[hl.unwrap().0..hl.unwrap().1], "int x");
        let (t, hl) = calltip_text("abs", ovs, &mut o, 1, &env);
        assert_eq!((o, hl), (0, None));
        assert!(t.contains("int abs (int x)"));
        calltip_text("abs", ovs, &mut o, 2, &env);
        assert_eq!(o, 1);
        let (t, hl) = calltip_text("abs", ovs, &mut o, 1, &env);
        assert_eq!(t, "\u{1}2 of 2\u{2}float abs (float x, float y)");
        assert_eq!(&t[hl.unwrap().0..hl.unwrap().1], "float y");
        assert_eq!(calltip_text("abs", ovs, &mut o, 5, &env).1, None);
    }

    #[test]
    fn path_parts() {
        assert_eq!(path_start("open /usr/lo"), Some(5));
        assert_eq!(path_start("/Users/a b/c"), Some(0));
        assert_eq!(path_start("x = \"~/Doc"), Some(5));
        assert_eq!(path_start("a/b"), None);
        let dirs = ["/usr", "/usr/", "/"];
        let is_dir = |p: &str| dirs.contains(&p);
        let is_file = |p: &str| p == "/etc/hosts";
        assert_eq!(path_dir("/usr/", is_dir, is_file), Some("/usr/".into()));
        assert_eq!(path_dir("/usr/lo", is_dir, is_file), Some("/usr".into()));
        assert_eq!(path_dir("/us", is_dir, is_file), Some("".into()));
        assert_eq!(path_dir("/etc/hosts", is_dir, is_file), None);
        assert_eq!(with_slash(""), "/");
        let e = path_entries(
            "/usr/",
            vec![
                ("local".into(), true),
                ("Bin".into(), true),
                ("a.txt".into(), false),
            ],
        );
        assert_eq!(e, "/usr/a.txt\n/usr/Bin/\n/usr/local/");
    }

    #[test]
    fn xpm_images_read() {
        let f = xpm_lines(AUTOC_SRC, "xpmfn");
        assert_eq!(f.len(), 1 + 36 + 16);
        assert_eq!(f[0].to_str().unwrap(), "16 16 36 1 ");
        assert_eq!(xpm_lines(AUTOC_SRC, "xpmbox").len(), 1 + 33 + 16);
    }
}
