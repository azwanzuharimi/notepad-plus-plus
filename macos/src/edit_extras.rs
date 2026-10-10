// SPDX-License-Identifier: GPL-3.0-or-later
use crate::edit::{merge_sort, undo};
use crate::sci::{self, send};
use crate::{item, nested, ns, tagged, tools, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, DefinedClass, MainThreadMarker};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSMenuItem,
    NSPasteboard, NSView, NSWorkspace,
};
use objc2_foundation::{NSArray, NSDateFormatter, NSURL};
use std::cmp::Ordering;
use std::ffi::{c_char, c_long, c_void};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

const SCI_ADDTEXT: u32 = 2001;
const SCI_GETLENGTH: u32 = 2006;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GETANCHOR: u32 = 2009;
const SCI_GOTOPOS: u32 = 2025;
const SCI_GETLINEENDPOSITION: u32 = 2136;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_SETSEL: u32 = 2160;
const SCI_REPLACESEL: u32 = 2170;
const SCI_GETTARGETSTART: u32 = 2191;
const SCI_GETTARGETEND: u32 = 2193;
const SCI_REPLACETARGET: u32 = 2194;
const SCI_LINELENGTH: u32 = 2350;
const SCI_GETSELECTIONMODE: u32 = 2423;
const SCI_GETSELECTIONS: u32 = 2570;
const SCI_GETSELECTIONNCARET: u32 = 2577;
const SCI_GETSELECTIONNANCHOR: u32 = 2579;
const SCI_GETSELECTIONNCARETVIRTUALSPACE: u32 = 2581;
const SCI_GETSELECTIONNANCHORVIRTUALSPACE: u32 = 2583;
const SCI_GETSELECTIONNSTART: u32 = 2585;
const SCI_GETSELECTIONNEND: u32 = 2587;
const SCI_SETRECTANGULARSELECTIONCARET: u32 = 2588;
const SCI_GETRECTANGULARSELECTIONCARET: u32 = 2589;
const SCI_SETRECTANGULARSELECTIONANCHOR: u32 = 2590;
const SCI_GETRECTANGULARSELECTIONANCHOR: u32 = 2591;
const SCI_SETRECTANGULARSELECTIONCARETVIRTUALSPACE: u32 = 2592;
const SCI_SETRECTANGULARSELECTIONANCHORVIRTUALSPACE: u32 = 2594;
const SCI_GETSELECTIONEMPTY: u32 = 2650;
const SCI_SETTARGETRANGE: u32 = 2686;
const SC_SEL_RECTANGLE: isize = 1;
const SC_SEL_THIN: isize = 3;

// Menu tags of the onSelection: action.
const OPEN_FILE: isize = 0;
const OPEN_FOLDER: isize = 1;
const SEARCH_INTERNET: isize = 2;
const CHANGE_SEARCH_ENGINE: isize = 3;

// Fields of a SYSTEMTIME that the Notepad++ date and time pictures use; the week day counts from Sunday.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Time {
    pub year: i64,
    pub month: usize,
    pub day: u32,
    pub week_day: usize,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

// The locale names that the Windows date pictures use.
#[derive(Clone, Debug, Default)]
pub struct Names {
    pub days: Vec<String>,
    pub short_days: Vec<String>,
    pub months: Vec<String>,
    pub short_months: Vec<String>,
    pub am: String,
    pub pm: String,
    pub era: String,
}

// preferenceDlg.cpp _BTTF_time, the time of the preview in Preferences.
pub const BTTF_TIME: Time = Time {
    year: 1985,
    month: 10,
    day: 26,
    week_day: 6,
    hour: 16,
    minute: 24,
    second: 42,
};

// Common.cpp timeFmtEscapeChar.
const ESC: char = '\u{1}';

// One pass of GetTimeFormatEx (h, H, m, s) or GetDateFormatEx (d, M, y, g) on a picture.
fn picture(fmt: &str, time: bool, t: &Time, n: &Names) -> String {
    let c: Vec<char> = fmt.chars().collect();
    let specs: &[char] = if time {
        &['h', 'H', 'm', 's']
    } else {
        &['d', 'M', 'y', 'g']
    };
    let name = |v: &[String], i: usize| v.get(i).cloned().unwrap_or_default();
    let num = |v: u32, pad: bool| if pad { format!("{v:02}") } else { v.to_string() };
    let mut out = String::new();
    let mut quoted = false;
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '\'' {
            if c.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
            } else {
                quoted = !quoted;
                i += 1;
            }
            continue;
        }
        if quoted || !specs.contains(&ch) {
            out.push(ch);
            i += 1;
            continue;
        }
        let k = c[i..].iter().take_while(|&&x| x == ch).count();
        i += k;
        let two = k >= 2;
        out += &match ch {
            'h' => num(if t.hour % 12 == 0 { 12 } else { t.hour % 12 }, two),
            'H' => num(t.hour, two),
            'm' => num(t.minute, two),
            's' => num(t.second, two),
            'd' => match k {
                1 | 2 => num(t.day, two),
                3 => name(&n.short_days, t.week_day),
                _ => name(&n.days, t.week_day),
            },
            'M' => match k {
                1 | 2 => num(t.month as u32, two),
                3 => name(&n.short_months, t.month.wrapping_sub(1)),
                _ => name(&n.months, t.month.wrapping_sub(1)),
            },
            'y' => match k {
                1 => (t.year.rem_euclid(10)).to_string(),
                2 => format!("{:02}", t.year.rem_euclid(100)),
                _ => t.year.to_string(),
            },
            _ => n.era.clone(),
        };
    }
    out
}

// Port of Common.cpp getDateTimeStrFrom: time pass, then date pass, then the AM/PM text for 't' and 'tt'.
pub fn date_time_str(fmt: &str, t: &Time, n: &Names) -> String {
    let midday_fmt = fmt.contains('t');
    let f: String = fmt.chars().map(|c| if c == 't' { ESC } else { c }).collect();
    let s = picture(&picture(&f, true, t, n), false, t, n);
    if !midday_fmt {
        return s;
    }
    let midday = if t.hour < 12 { &n.am } else { &n.pm };
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != ESC {
            out.push(c);
        } else if midday.is_empty() {
        } else if it.peek() == Some(&ESC) {
            it.next();
            out += midday;
        } else {
            out.extend(midday.chars().next());
        }
    }
    out
}

#[repr(C)]
struct Tm {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
    isdst: i32,
    gmtoff: c_long,
    zone: *const c_char,
}

extern "C" {
    fn time(t: *mut i64) -> i64;
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
}

fn now() -> Time {
    let t = unsafe { time(std::ptr::null_mut()) };
    let mut tm: Tm = unsafe { std::mem::zeroed() };
    unsafe { localtime_r(&t, &mut tm) };
    let u = |v: i32| v.max(0) as u32;
    Time {
        year: tm.year as i64 + 1900,
        month: u(tm.mon) as usize + 1,
        day: u(tm.mday),
        week_day: u(tm.wday) as usize,
        hour: u(tm.hour),
        minute: u(tm.min),
        second: u(tm.sec),
    }
}

fn names() -> Names {
    let f = NSDateFormatter::new();
    let v = |a: Retained<NSArray<objc2_foundation::NSString>>| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Names {
        days: v(f.weekdaySymbols()),
        short_days: v(f.shortWeekdaySymbols()),
        months: v(f.monthSymbols()),
        short_months: v(f.shortMonthSymbols()),
        am: f.AMSymbol().to_string(),
        pm: f.PMSymbol().to_string(),
        era: v(f.eraSymbols()).get(1).cloned().unwrap_or_default(),
    }
}

// The result line under the Custom format field in Preferences.
pub fn date_time_preview(fmt: &str) -> String {
    date_time_str(fmt, &BTTF_TIME, &names())
}

// Port of Common.cpp buf2Clipboard: each name ends with CR LF.
pub fn names_text<S: AsRef<str>>(names: &[S]) -> String {
    let mut s = String::new();
    for n in names {
        s += n.as_ref();
        if !s.is_empty() && !s.ends_with("\r\n") {
            s += "\r\n";
        }
    }
    s
}

// public.html or public.rtf data as Notepad++ pastes CF_HTML and CF_RTF: the bytes up to the first NUL.
pub fn markup_bytes(d: &[u8]) -> Vec<u8> {
    let utf16 = |be: bool| {
        let u: Vec<u16> = d[2..]
            .chunks_exact(2)
            .map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&u).into_bytes()
    };
    let b = match d {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        _ => d.to_vec(),
    };
    b.split(|&c| c == 0).next().unwrap_or_default().to_vec()
}

// Port of IDM_EDIT_REDACT_SELECTION for one selection: each character except CR and LF becomes the symbol.
pub fn redact(b: &[u8], symbol: &str) -> Vec<u8> {
    let mut out = vec![];
    let mut put = |c: Option<u8>| match c {
        Some(c @ (b'\r' | b'\n')) => out.push(c),
        _ => out.extend_from_slice(symbol.as_bytes()),
    };
    for c in b.utf8_chunks() {
        for ch in c.valid().chars() {
            put(ch.is_ascii().then_some(ch as u8));
        }
        c.invalid().iter().for_each(|_| put(None));
    }
    out
}

// Percent-encoding of all bytes except the URL unreserved characters.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

// Port of the NPPM_GETFILENAMEATCURSOR rule: text with a slash as it is, else the file name around the caret column.
pub fn file_name_at(word: &str, line: &[u8], col: usize) -> String {
    if word.contains(['\\', '/']) {
        return word.to_string();
    }
    let at = |i: usize| line.get(i).copied().unwrap_or(0);
    let starts = b" \t[(\"<>";
    let ends = b" \t:()[]<>\"\r\n";
    let mut start = col;
    while start > 0 && !starts.contains(&at(start)) {
        start -= 1;
    }
    if starts.contains(&at(start)) {
        start += 1;
    }
    let mut end = col;
    while at(end) != 0 && !ends.contains(&at(end)) {
        end += 1;
    }
    match end.checked_sub(start) {
        None => word.to_string(),
        Some(_) => String::from_utf8_lossy(line.get(start..end).unwrap_or_default()).into_owned(),
    }
}

// The path without "." and with ".." removed, as std::filesystem::path::lexically_normal does.
fn lexically_normal(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

// The macOS form of FILE_ATTRIBUTE_READONLY: the file has no owner write permission.
pub fn file_read_only(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o200 == 0)
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithBytes(a: *const c_void, b: *const u8, n: isize, e: u32, ext: u8) -> *const c_void;
    fn CFStringGetLength(s: *const c_void) -> isize;
    fn CFStringCompareWithOptionsAndLocale(a: *const c_void, b: *const c_void, r: CfRange, o: usize, l: *const c_void) -> isize;
    fn CFLocaleCopyCurrent() -> *const c_void;
    #[cfg(test)]
    fn CFLocaleCreate(a: *const c_void, id: *const c_void) -> *const c_void;
    fn CFRelease(o: *const c_void);
}

#[repr(C)]
struct CfRange {
    loc: isize,
    len: isize,
}

const UTF8: u32 = 0x0800_0100;
const COMPARE_CASE_INSENSITIVE: usize = 1;
const COMPARE_NUMERICALLY: usize = 64;

struct Cf(*const c_void);

impl Cf {
    fn str(s: &str) -> Cf {
        Cf(unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0) })
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

// A CoreFoundation locale for the comparisons of Sort Lines In Locale Order.
pub struct Locale(Cf);

impl Locale {
    pub fn current() -> Locale {
        Locale(Cf(unsafe { CFLocaleCopyCurrent() }))
    }

    #[cfg(test)]
    fn named(id: &str) -> Locale {
        let s = Cf::str(id);
        Locale(Cf(unsafe { CFLocaleCreate(std::ptr::null(), s.0) }))
    }

    // SortLocale defaults: ignore case, digits as numbers, keep diacritics and symbols.
    fn compare(&self, a: &Cf, b: &Cf) -> Ordering {
        if a.0.is_null() || b.0.is_null() {
            return a.0.is_null().cmp(&b.0.is_null()).reverse();
        }
        let r = CfRange {
            loc: 0,
            len: unsafe { CFStringGetLength(a.0) },
        };
        let o = COMPARE_CASE_INSENSITIVE | COMPARE_NUMERICALLY;
        unsafe { CFStringCompareWithOptionsAndLocale(a.0, b.0, r, o, self.0 .0) }.cmp(&0)
    }
}

// Port of the sort in SortLocale::sort: a stable sort of the lines by key; the last line gets `eol` while it moves.
pub fn locale_sorted(contents: &[&[u8]], keys: &[String], desc: bool, eol: Option<&[u8]>, loc: &Locale) -> (Vec<u8>, Vec<usize>) {
    let cf: Vec<Cf> = keys.iter().map(|k| Cf::str(k)).collect();
    let mut order: Vec<usize> = (0..contents.len()).collect();
    merge_sort(&mut order, &|a: &usize, b: &usize| {
        let o = loc.compare(&cf[*a], &cf[*b]);
        if desc {
            o.is_gt()
        } else {
            o.is_lt()
        }
    });
    let mut out = vec![];
    for &i in &order {
        out.extend_from_slice(contents[i]);
        if i + 1 == contents.len() {
            out.extend_from_slice(eol.unwrap_or_default());
        }
    }
    if eol.is_some() {
        if out.last() == Some(&b'\n') {
            out.pop();
        }
        if out.last() == Some(&b'\r') {
            out.pop();
        }
    }
    (out, order)
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

fn eol_bytes(v: &NSView) -> &'static [u8] {
    match sci::eol_mode(v) {
        crate::encoding::SC_EOL_CR => b"\r",
        crate::encoding::SC_EOL_LF => b"\n",
        _ => b"\r\n",
    }
}

// Port of SortLocale::sort; an error gives the warning text.
fn sort_locale(v: &NSView, desc: bool, loc: &Locale) -> Result<(), &'static str> {
    let count = s(v, SCI_GETLINECOUNT, 0, 0);
    let length = s(v, SCI_GETLENGTH, 0, 0);
    let (mut forward, mut nosel, mut missing_eol) = (true, false, false);
    let rect = matches!(s(v, SCI_GETSELECTIONMODE, 0, 0), SC_SEL_RECTANGLE | SC_SEL_THIN);
    let (top, bottom, start, end, lines);
    if rect {
        lines = s(v, SCI_GETSELECTIONS, 0, 0);
        if lines < 2 {
            return Ok(());
        }
        nosel = s(v, SCI_GETSELECTIONEMPTY, 0, 0) != 0;
        let rsa = s(v, SCI_GETRECTANGULARSELECTIONANCHOR, 0, 0);
        let rsc = s(v, SCI_GETRECTANGULARSELECTIONCARET, 0, 0);
        forward = rsc > rsa;
        top = line_of(v, if forward { rsa } else { rsc });
        bottom = line_of(v, if forward { rsc } else { rsa });
        start = line_start(v, top);
        if bottom == count - 1 {
            end = length;
            missing_eol = true;
        } else {
            end = line_start(v, bottom + 1);
        }
    } else {
        if s(v, SCI_GETSELECTIONS, 0, 0) != 1 {
            return Err("Sorting multiple selections is not supported.");
        }
        let anchor = s(v, SCI_GETANCHOR, 0, 0);
        let caret = s(v, SCI_GETCURRENTPOS, 0, 0);
        let mut b;
        if anchor == caret {
            nosel = true;
            (top, start, end) = (0, 0, length);
            b = count - 1;
            if line_start(v, b) == end {
                b -= 1;
            } else {
                missing_eol = true;
            }
        } else {
            forward = anchor < caret;
            top = line_of(v, anchor.min(caret));
            start = line_start(v, top);
            let mut e = anchor.max(caret);
            b = line_of(v, e);
            if line_start(v, b) == e {
                b -= 1;
            } else if b == count - 1 {
                missing_eol = true;
                e = length;
            } else {
                e = line_start(v, b + 1);
            }
            end = e;
        }
        bottom = b;
        lines = bottom - top + 1;
        if lines < 2 {
            return Ok(());
        }
    }
    let lines = lines as usize;
    let text = sci::doc(v).range(start, end);
    let piece = |a: isize, n: isize| {
        let a = (a - start).max(0) as usize;
        text.get(a..a + n.max(0) as usize).unwrap_or_default()
    };
    let mut contents = vec![&[][..]; lines];
    let mut keys = vec![String::new(); lines];
    let mut index = vec![0isize; lines];
    let mut next = end;
    for n in (0..lines).rev() {
        let line = top + n as isize;
        let ls = line_start(v, line);
        let (ks, kend) = if rect {
            index[n] = if forward { n } else { lines - 1 - n } as isize;
            let ks = s(v, SCI_GETSELECTIONNSTART, index[n], 0);
            let ke = if nosel {
                s(v, SCI_GETLINEENDPOSITION, line, 0)
            } else {
                s(v, SCI_GETSELECTIONNEND, index[n], 0)
            };
            (ks, ke)
        } else {
            index[n] = n as isize;
            (ls, s(v, SCI_GETLINEENDPOSITION, line, 0))
        };
        contents[n] = piece(ls, next - ls);
        keys[n] = String::from_utf8_lossy(piece(ks, kend - ks)).into_owned();
        next = ls;
    }
    let eol = missing_eol.then(|| eol_bytes(v));
    let (sorted, order) = locale_sorted(&contents, &keys, desc, eol, loc);
    let mut restore = (0, 0, 0, 0);
    let mut caret_line = 0;
    if rect {
        let (ix_top, ix_bottom) = (index[order[0]], index[order[lines - 1]]);
        let (ix_a, ix_c) = if forward { (ix_top, ix_bottom) } else { (ix_bottom, ix_top) };
        let a = s(v, SCI_GETSELECTIONNANCHOR, ix_a, 0);
        let c = s(v, SCI_GETSELECTIONNCARET, ix_c, 0);
        restore = (
            a - line_start(v, line_of(v, a)),
            s(v, SCI_GETSELECTIONNANCHORVIRTUALSPACE, ix_a, 0),
            c - line_start(v, line_of(v, c)),
            s(v, SCI_GETSELECTIONNCARETVIRTUALSPACE, ix_c, 0),
        );
    } else if nosel {
        let c = s(v, SCI_GETCURRENTPOS, 0, 0);
        caret_line = line_of(v, c);
        restore.2 = c - line_start(v, caret_line);
        if let Some(n) = order.iter().position(|&i| index[i] == caret_line) {
            caret_line = n as isize;
        }
    }
    undo(v, || {
        s(v, SCI_SETTARGETRANGE, start, end);
        s(v, SCI_REPLACETARGET, sorted.len() as isize, sorted.as_ptr() as isize);
    });
    if rect {
        let a = restore.0 + line_start(v, if forward { top } else { bottom });
        let c = restore.2 + line_start(v, if forward { bottom } else { top });
        s(v, SCI_SETRECTANGULARSELECTIONANCHOR, a, 0);
        s(v, SCI_SETRECTANGULARSELECTIONCARET, c, 0);
        s(v, SCI_SETRECTANGULARSELECTIONANCHORVIRTUALSPACE, restore.1, 0);
        s(v, SCI_SETRECTANGULARSELECTIONCARETVIRTUALSPACE, restore.3, 0);
    } else if nosel {
        s(v, SCI_GOTOPOS, line_start(v, caret_line) + restore.2, 0);
    } else {
        let (ts, te) = (s(v, SCI_GETTARGETSTART, 0, 0), s(v, SCI_GETTARGETEND, 0, 0));
        if forward {
            s(v, SCI_SETSEL, ts, te);
        } else {
            s(v, SCI_SETSEL, te, ts);
        }
    }
    Ok(())
}

fn read_markup(pb: &NSPasteboard, rtf: bool) -> Option<Vec<u8>> {
    let ty = if rtf { "public.rtf" } else { "public.html" };
    Some(markup_bytes(&pb.dataForType(&ns(ty))?.to_vec()))
}

fn with_nul(b: &[u8]) -> Vec<u8> {
    let mut z = b.to_vec();
    z.push(0);
    z
}

pub fn date_time_custom_item(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    item(mtm, "Date Time (customized)", sel!(insertDateTimeCustom:), "", t)
}

pub fn copy_all_items(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    vec![
        NSMenuItem::separatorItem(mtm),
        tagged(mtm, "Copy All Filenames", sel!(copyAllNames:), 0, t),
        tagged(mtm, "Copy All File Paths", sel!(copyAllNames:), 1, t),
    ]
}

pub fn locale_sort_item(mtm: MainThreadMarker, t: Option<&AnyObject>, desc: bool) -> Retained<NSMenuItem> {
    let title = if desc {
        "Sort Lines In Locale Order Descending"
    } else {
        "Sort Lines In Locale Order Ascending"
    };
    tagged(mtm, title, sel!(sortLocale:), desc as isize, t)
}

pub fn paste_markup_items(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    vec![
        tagged(mtm, "Paste HTML Content", sel!(pasteMarkup:), 0, t),
        tagged(mtm, "Paste RTF Content", sel!(pasteMarkup:), 1, t),
        NSMenuItem::separatorItem(mtm),
    ]
}

pub fn on_selection_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    let op = |title: &str, tag: isize| tagged(mtm, title, sel!(onSelection:), tag, t);
    nested(
        mtm,
        "On Selection",
        vec![
            op("Open File", OPEN_FILE),
            op("Open Containing Folder in Finder", OPEN_FOLDER),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Redact Selection \u{2588} (Shift: \u{25CF})", sel!(redactSelection:), "", t),
            NSMenuItem::separatorItem(mtm),
            op("Search on Internet", SEARCH_INTERNET),
            op("Change Search Engine...", CHANGE_SEARCH_ENGINE),
        ],
    )
}

pub fn read_only_all_items(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    vec![
        tagged(mtm, "Read-Only for All Documents", sel!(readOnlyAll:), 1, t),
        tagged(mtm, "Clear Read-Only for All Documents", sel!(readOnlyAll:), 0, t),
    ]
}

pub fn file_read_only_item(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    item(mtm, "Read-Only Attribute in macOS", sel!(toggleFileReadOnly:), "", t)
}

impl App {
    // IDM_EDIT_INSERT_DATETIME_CUSTOMIZED.
    pub(crate) fn insert_date_time_custom(&self) {
        let Some(v) = self.editor() else { return };
        let fmt = crate::prefs::with(|p| p.date_time_format.clone());
        let t = date_time_str(&fmt, &now(), &names());
        undo(&v, || {
            send(&v, SCI_REPLACESEL, 0, c"".as_ptr() as isize);
            send(&v, SCI_ADDTEXT, t.len(), t.as_ptr() as isize);
        });
    }

    // IDM_EDIT_COPY_ALL_NAMES and IDM_EDIT_COPY_ALL_PATHS, in tab order.
    pub(crate) fn copy_all_names(&self, paths: bool) {
        let s = self.all_names_text(paths);
        if !s.is_empty() {
            tools::to_clipboard(&s);
        }
    }

    fn all_names_text(&self, paths: bool) -> String {
        let names: Vec<String> = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .map(|t| match (&t.path, paths) {
                (Some(p), true) => p.display().to_string(),
                _ => t.name.clone(),
            })
            .collect();
        names_text(&names)
    }

    // Notepad_plus::changeReadOnlyUserModeForAllOpenedTabs.
    pub(crate) fn read_only_all(&self, on: bool) {
        let views: Vec<_> = self
            .ivars()
            .tabs
            .borrow_mut()
            .iter_mut()
            .map(|t| {
                t.ro = on;
                (t.view.clone(), t.read_only())
            })
            .collect();
        let replacing = self.ivars().replacing.get();
        views.iter().for_each(|(v, ro)| sci::set_read_only(v, *ro || replacing));
        self.refresh_labels();
    }

    // IDM_EDIT_TOGGLESYSTEMREADONLY: macOS has no read-only attribute, so this toggles the owner write permission.
    pub(crate) fn toggle_file_read_only(&self) {
        let Some(i) = self.current() else { return };
        let Some(t) = self.tab(i) else { return };
        let Some(p) = t.path.as_deref() else { return };
        let r = std::fs::metadata(p).and_then(|m| {
            let mut perm = m.permissions();
            perm.set_mode(perm.mode() ^ 0o200);
            std::fs::set_permissions(p, perm)
        });
        match r {
            Ok(()) => {
                let ro = {
                    let mut tabs = self.ivars().tabs.borrow_mut();
                    let Some(t) = tabs.get_mut(i) else { return };
                    t.file_ro = file_read_only(p);
                    t.read_only()
                };
                sci::set_read_only(&t.view, ro || self.ivars().replacing.get());
                self.refresh_title(i);
                self.update_status();
            }
            Err(e) => {
                self.alert("Changing file read-only attribute failed", &e.to_string(), &["OK"]);
            }
        }
    }

    // IDM_EDIT_SORTLINES_LOCALE_ASCENDING and IDM_EDIT_SORTLINES_LOCALE_DESCENDING.
    pub(crate) fn sort_locale(&self, desc: bool) {
        let Some(v) = self.editor() else { return };
        if let Err(msg) = sort_locale(&v, desc, &Locale::current()) {
            self.alert("Sort not performed", msg, &["OK"]);
        }
    }

    // IDM_EDIT_PASTE_AS_HTML and IDM_EDIT_PASTE_AS_RTF: the raw markup text.
    pub(crate) fn paste_markup(&self, rtf: bool) {
        self.paste_markup_from(&NSPasteboard::generalPasteboard(), rtf);
    }

    fn paste_markup_from(&self, pb: &NSPasteboard, rtf: bool) {
        let Some(v) = self.editor() else { return };
        let Some(b) = read_markup(pb, rtf) else {
            return;
        };
        let z = with_nul(&b);
        send(&v, SCI_REPLACESEL, 0, z.as_ptr() as isize);
    }

    // IDM_EDIT_REDACT_SELECTION; Shift gives the bullet.
    pub(crate) fn redact_selection(&self) {
        let Some(v) = self.editor() else { return };
        let shift = NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Shift);
        let symbol = if shift { "\u{25CF}" } else { "\u{2588}" };
        undo(&v, || {
            for i in 0..s(&v, SCI_GETSELECTIONS, 0, 0) {
                let a = s(&v, SCI_GETSELECTIONNSTART, i, 0);
                let b = s(&v, SCI_GETSELECTIONNEND, i, 0);
                if a >= b {
                    continue;
                }
                let r = redact(&sci::doc(&v).range(a, b), symbol);
                s(&v, SCI_SETTARGETRANGE, a, b);
                s(&v, SCI_REPLACETARGET, r.len() as isize, r.as_ptr() as isize);
            }
        });
    }

    // The path of IDM_EDIT_OPENSELECTEDFILETOEDIT: the file name at the caret, else relative to the folder of the file.
    fn selected_path(&self, v: &NSView) -> PathBuf {
        let word = self.run_value(5);
        let pos = s(v, SCI_GETCURRENTPOS, 0, 0);
        let line = line_of(v, pos);
        let ls = line_start(v, line);
        let text = sci::doc(v).range(ls, ls + s(v, SCI_LINELENGTH, line, 0));
        let name = file_name_at(&word, &text, (pos - ls).max(0) as usize);
        let expanded = match (name.strip_prefix("~/"), std::env::var_os("HOME")) {
            (Some(rest), Some(home)) => Path::new(&home).join(rest),
            _ => PathBuf::from(&name),
        };
        if expanded.exists() {
            return expanded;
        }
        let dir = self
            .current()
            .and_then(|i| self.tab(i)?.path?.parent().map(|p| p.display().to_string()))
            .unwrap_or_default();
        PathBuf::from(format!("{dir}/{name}"))
    }

    pub(crate) fn on_selection(&self, tag: isize) {
        if tag == CHANGE_SEARCH_ENGINE {
            self.show_preferences_page("Search Engine");
            return;
        }
        let Some(v) = self.editor() else { return };
        if s(&v, SCI_GETSELECTIONS, 0, 0) != 1 {
            return;
        }
        if tag == SEARCH_INTERNET {
            let mut url = crate::prefs::with(|p| p.search_engine_url());
            for (i, (name, _)) in crate::run::VARS.iter().enumerate() {
                let var = format!("$({name})");
                if url.contains(&var) {
                    url = url.replace(&var, &url_encode(&self.run_value(i)));
                }
            }
            if let Some(u) = NSURL::URLWithString(&ns(&url)) {
                NSWorkspace::sharedWorkspace().openURL(&u);
            }
            return;
        }
        let p = self.selected_path(&v);
        let ok = if tag == OPEN_FOLDER { p.exists() } else { p.is_file() };
        if !ok {
            self.alert("Open Path", "The path you're trying to open doesn't exist.", &["OK"]);
        } else if tag == OPEN_FOLDER {
            let url = NSURL::fileURLWithPath(&ns(&lexically_normal(&p).to_string_lossy()));
            NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
        } else {
            self.open_path(&p);
        }
    }

    pub(crate) fn validate_edit_extras(&self, item: &NSMenuItem) -> Option<bool> {
        let a = item.action()?;
        let ours = [
            sel!(insertDateTimeCustom:),
            sel!(copyAllNames:),
            sel!(sortLocale:),
            sel!(pasteMarkup:),
            sel!(onSelection:),
            sel!(redactSelection:),
            sel!(readOnlyAll:),
            sel!(toggleFileReadOnly:),
        ];
        if !ours.contains(&a) {
            return None;
        }
        if a == sel!(onSelection:) && item.tag() == CHANGE_SEARCH_ENGINE {
            return Some(true);
        }
        let Some(t) = self.current().and_then(|i| self.tab(i)) else {
            return Some(false);
        };
        let ro = sci::read_only(&t.view);
        Some(if a == sel!(toggleFileReadOnly:) {
            item.setState(if t.file_ro {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            t.path.as_deref().is_some_and(Path::is_file)
        } else if a == sel!(redactSelection:) {
            let (x, y) = sci::selection(&t.view);
            !ro && x != y
        } else if [sel!(insertDateTimeCustom:), sel!(sortLocale:), sel!(pasteMarkup:)].contains(&a) {
            !ro
        } else if a == sel!(readOnlyAll:) {
            !self.monitored(&t.item)
        } else {
            true
        })
    }

    pub(crate) fn tab_file_read_only(&self, i: usize) -> bool {
        self.tab(i).is_some_and(|t| t.file_ro)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en() -> Names {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect();
        Names {
            days: s(&["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]),
            short_days: s(&["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]),
            months: s(&["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]),
            short_months: s(&["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]),
            am: "AM".into(),
            pm: "PM".into(),
            era: "A.D.".into(),
        }
    }

    #[test]
    fn date_time_matches_the_preferences_examples() {
        let f = |fmt: &str| date_time_str(fmt, &BTTF_TIME, &en());
        assert_eq!(f("yyyy-MM-dd HH:mm:ss"), "1985-10-26 16:24:42");
        assert_eq!(f("H:m d/M/yyyy"), "16:24 26/10/1985");
        assert_eq!(f("MMM d, yyyy  tt h:m"), "Oct 26, 1985  PM 4:24");
    }

    #[test]
    fn date_time_pictures() {
        let t = Time {
            year: 2007,
            month: 3,
            day: 4,
            week_day: 0,
            hour: 0,
            minute: 5,
            second: 9,
        };
        let f = |fmt: &str| date_time_str(fmt, &t, &en());
        assert_eq!(f("dddd, MMMM dd yy g"), "Sunday, March 04 07 A.D.");
        assert_eq!(f("ddd MM y yyyyy"), "Sun 03 7 2007");
        assert_eq!(f("hh:mm:ss t h"), "12:05:09 A 12");
        assert_eq!(f("HHH mmm sss"), "00 05 09");
        assert_eq!(f("'MM' 'h'h ''"), "03 h12 ");
        assert_eq!(f("''''"), "'");
        assert_eq!(f("'at' H"), "aA 0");
        let mut n = en();
        n.am.clear();
        assert_eq!(date_time_str("H tt:mm", &t, &n), "0 :05");
    }

    #[test]
    fn copy_all_names_ends_each_name_with_crlf() {
        assert_eq!(names_text(&["a.txt", "new 1"]), "a.txt\r\nnew 1\r\n");
        assert_eq!(names_text(&["", "b"]), "b\r\n");
        assert_eq!(names_text::<&str>(&[]), "");
    }

    #[test]
    fn markup_is_cut_at_nul_and_utf16_is_decoded() {
        assert_eq!(markup_bytes(b"<b>x</b>\0junk"), b"<b>x</b>");
        assert_eq!(markup_bytes(b"{\\rtf1 \xe9}"), b"{\\rtf1 \xe9}");
        assert_eq!(markup_bytes(&[0xFF, 0xFE, b'<', 0, 0xE9, 0]), "<\u{e9}".as_bytes());
        assert_eq!(markup_bytes(&[0xFE, 0xFF, 0, b'a']), b"a");
    }

    #[test]
    fn markup_comes_from_its_pasteboard_type() {
        let pb = NSPasteboard::pasteboardWithUniqueName();
        pb.clearContents();
        pb.setData_forType(Some(&objc2_foundation::NSData::with_bytes(b"<i>h</i>")), &ns("public.html"));
        assert_eq!(read_markup(&pb, false).as_deref(), Some(&b"<i>h</i>"[..]));
        assert_eq!(read_markup(&pb, true), None);
        let _: () = unsafe { objc2::msg_send![&pb, releaseGlobally] };
    }

    #[test]
    fn redact_keeps_line_ends() {
        assert_eq!(redact("a\u{e9}\r\nb".as_bytes(), "#"), b"##\r\n#");
        assert_eq!(redact(b"x\xffy", "."), b"...");
    }

    #[test]
    fn url_encode_keeps_unreserved_characters() {
        assert_eq!(url_encode("a b&c=d/\u{e9}~"), "a%20b%26c%3Dd%2F%C3%A9~");
    }

    #[test]
    fn file_name_at_the_caret() {
        let line = b"see (docs/a.txt) and \"b c.md\" x\r\n";
        assert_eq!(file_name_at("docs", line, 9), "docs/a.txt");
        assert_eq!(file_name_at("readme", b"open readme.txt now", 11), "readme.txt");
        assert_eq!(file_name_at("x", b"a:b", 1), "a");
        assert_eq!(file_name_at("x", b" :", 1), "");
        assert_eq!(file_name_at("x", b"a b", 1), "x");
        assert_eq!(file_name_at("/tmp/x y", b"", 0), "/tmp/x y");
    }

    #[test]
    fn file_read_only_reads_the_owner_write_bit() {
        let d = std::env::temp_dir().join(format!("npp-ro-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(!file_read_only(&f));
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(file_read_only(&f));
        assert!(!file_read_only(&d));
        assert!(!file_read_only(&d.join("missing")));
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn lexically_normal_removes_dots() {
        assert_eq!(lexically_normal(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(lexically_normal(Path::new("../x")), PathBuf::from("../x"));
    }

    fn sorted(lines: &[&str], desc: bool, loc: &str) -> Vec<String> {
        let contents: Vec<&[u8]> = lines.iter().map(|l| l.as_bytes()).collect();
        let keys: Vec<String> = lines.iter().map(|l| l.trim_end().to_string()).collect();
        let (_, order) = locale_sorted(&contents, &keys, desc, None, &Locale::named(loc));
        order.iter().map(|&i| keys[i].clone()).collect()
    }

    #[test]
    fn locale_sort_ignores_case_and_reads_numbers() {
        let v = ["b", "B", "a10", "a9", "A2", "\u{e9}", "e", "f"];
        assert_eq!(sorted(&v, false, "en_US"), ["A2", "a9", "a10", "b", "B", "e", "\u{e9}", "f"]);
        assert_eq!(sorted(&v, true, "en_US"), ["f", "\u{e9}", "e", "b", "B", "a10", "a9", "A2"]);
    }

    #[test]
    fn locale_sort_uses_the_locale() {
        let v = ["\u{e4}", "z", "a"];
        assert_eq!(sorted(&v, false, "en_US"), ["a", "\u{e4}", "z"]);
        assert_eq!(sorted(&v, false, "sv_SE"), ["a", "z", "\u{e4}"]);
    }

    #[test]
    fn locale_sort_moves_the_missing_line_end() {
        let contents: [&[u8]; 3] = [b"c\r\n", b"a\r\n", b"b"];
        let keys = ["c".to_string(), "a".into(), "b".into()];
        let (t, order) = locale_sorted(&contents, &keys, false, Some(b"\r\n"), &Locale::named("en_US"));
        assert_eq!(t, b"a\r\nb\r\nc");
        assert_eq!(order, [1, 2, 0]);
    }
}
