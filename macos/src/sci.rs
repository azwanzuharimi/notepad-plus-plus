// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{Config, Style};
use crate::search::{Doc, Line};
use crate::{lang, view};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_app_kit::NSView;
use std::ffi::{c_void, CString};

pub const SCN_SAVEPOINTREACHED: u32 = 2002;
pub const SCN_SAVEPOINTLEFT: u32 = 2003;
pub const SCN_DOUBLECLICK: u32 = 2006;
pub const SCN_UPDATEUI: u32 = 2007;
pub const RESULTS_ID: usize = 1;
const SCI_CLEARALL: u32 = 2004;
const SCI_GETREADONLY: u32 = 2140;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_SETUNDOCOLLECTION: u32 = 2012;
const SCI_GOTOLINE: u32 = 2024;
const SCI_GETSELECTIONSTART: u32 = 2143;
const SCI_GETSELECTIONEND: u32 = 2145;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_SETSEL: u32 = 2160;
pub const SCI_LINEFROMPOSITION: u32 = 2166;
pub const SCI_POSITIONFROMLINE: u32 = 2167;
const SCI_SETREADONLY: u32 = 2171;
const SCI_REPLACETARGET: u32 = 2194;
const SCI_GETDOCPOINTER: u32 = 2357;
const SCI_SCROLLRANGE: u32 = 2569;
const SCI_SETIDENTIFIER: u32 = 2622;
const SCI_SETTARGETRANGE: u32 = 2686;
const SCI_GETLENGTH: u32 = 2006;
const SCI_CONVERTEOLS: u32 = 2029;
const SCI_GETEOLMODE: u32 = 2030;
const SCI_SETEOLMODE: u32 = 2031;
const SCI_GETCOLUMN: u32 = 2129;
const SCI_GETOVERTYPE: u32 = 2187;
const SCI_COUNTCHARACTERS: u32 = 2633;
const SCI_SETSAVEPOINT: u32 = 2014;
const SCI_SETCODEPAGE: u32 = 2037;
const SCI_STYLECLEARALL: u32 = 2050;
const SCI_STYLESETFORE: u32 = 2051;
const SCI_STYLESETBACK: u32 = 2052;
const SCI_STYLESETBOLD: u32 = 2053;
const SCI_STYLESETITALIC: u32 = 2054;
const SCI_STYLESETSIZE: u32 = 2055;
const SCI_STYLESETFONT: u32 = 2056;
const SCI_STYLESETEOLFILLED: u32 = 2057;
const SCI_STYLESETUNDERLINE: u32 = 2059;
const SCI_SETSELBACK: u32 = 2068;
const SCI_SETCARETFORE: u32 = 2069;
const SCI_SETCARETLINEVISIBLE: u32 = 2096;
const SCI_SETCARETLINEBACK: u32 = 2098;
const SCI_GETMODIFY: u32 = 2159;
const SCI_EMPTYUNDOBUFFER: u32 = 2175;
const SCI_GETTEXT: u32 = 2182;
const SCI_SETMARGINTYPEN: u32 = 2240;
const SCI_SETMARGINWIDTHN: u32 = 2242;
const SCI_APPENDTEXT: u32 = 2282;
const SCI_COLOURISE: u32 = 4003;
const SCI_SETPROPERTY: u32 = 4004;
const SCI_SETKEYWORDS: u32 = 4005;
const SCI_SETILEXER: u32 = 4033;
const SCI_ALLOCATESUBSTYLES: u32 = 4020;
const SCI_SETIDENTIFIERS: u32 = 4024;
const SCI_SETTABWIDTH: u32 = 2036;
const SCI_SETUSETABS: u32 = 2124;
const SCI_SETVIEWWS: u32 = 2021;
const SCI_SETWHITESPACEFORE: u32 = 2084;
const SCI_SETWHITESPACESIZE: u32 = 2086;
const SCI_SETVIEWEOL: u32 = 2356;
const SCI_SETREPRESENTATION: u32 = 2665;
const SCI_SETREPRESENTATIONAPPEARANCE: u32 = 2766;
const SCI_CLEARALLREPRESENTATIONS: u32 = 2770;
const SCI_SETINDENTATIONGUIDES: u32 = 2132;
const SCI_GETINDENTATIONGUIDES: u32 = 2133;
const SCI_SETWRAPMODE: u32 = 2268;
const SCI_SETWRAPVISUALFLAGS: u32 = 2460;
const SCI_SETWRAPVISUALFLAGSLOCATION: u32 = 2462;
const SCI_SETWRAPINDENTMODE: u32 = 2472;
const SCI_SETZOOM: u32 = 2373;
const SCI_MARKERDEFINE: u32 = 2040;
const SCI_MARKERSETFORE: u32 = 2041;
const SCI_MARKERSETBACK: u32 = 2042;
const SCI_MARKERSETBACKSELECTED: u32 = 2292;
const SCI_MARKERENABLEHIGHLIGHT: u32 = 2293;
const SCI_SETFOLDMARGINCOLOUR: u32 = 2290;
const SCI_SETFOLDMARGINHICOLOUR: u32 = 2291;
const SCI_SETMARGINMASKN: u32 = 2244;
const SCI_SETMARGINSENSITIVEN: u32 = 2246;
const SCI_SETFOLDFLAGS: u32 = 2233;
const SCI_SETAUTOMATICFOLD: u32 = 2663;
const SC_MASK_FOLDERS: isize = 0xFE000000;
const SC_FOLDFLAG_LINEAFTER_CONTRACTED: usize = 0x10;
const SC_AUTOMATICFOLD_ALL: usize = 7;
const SC_IV_LOOKFORWARD: usize = 2;
const SC_IV_LOOKBOTH: usize = 3;
const STYLE_DEFAULT: usize = 32;
const SC_CP_UTF8: usize = 65001;
const SC_MARGIN_NUMBER: isize = 1;

extern "C" {
    #[link_name = "OBJC_CLASS_$_ScintillaView"]
    static SCINTILLA_VIEW_CLASS: u8;
    fn CreateLexer(name: *const std::ffi::c_char) -> *mut c_void;
    fn npp_markings_new() -> *mut c_void;
    fn npp_markings_set(
        m: *mut c_void,
        counts: *const isize,
        n: isize,
        pairs: *const isize,
    ) -> *mut c_void;
}

pub fn new_view() -> Retained<NSView> {
    let cls = unsafe { &*(&raw const SCINTILLA_VIEW_CLASS as *const AnyClass) };
    unsafe { msg_send![cls, new] }
}

pub fn send(v: &NSView, msg: u32, w: usize, l: isize) -> isize {
    unsafe { msg_send![v, message: msg, wParam: w, lParam: l] }
}

fn send_str(v: &NSView, msg: u32, w: usize, s: &str) -> isize {
    let c = CString::new(s.replace('\0', "")).unwrap();
    send(v, msg, w, c.as_ptr() as isize)
}

pub fn content(v: &NSView) -> Retained<NSView> {
    unsafe { msg_send![v, content] }
}

pub fn set_delegate(v: &NSView, d: &AnyObject) {
    unsafe { msg_send![v, setDelegate: d] }
}

pub fn is_modified(v: &NSView) -> bool {
    send(v, SCI_GETMODIFY, 0, 0) != 0
}

// Scintilla refuses text changes in a read-only document, so a load clears that state for its duration.
fn writable(v: &NSView, f: impl FnOnce()) {
    let ro = read_only(v);
    set_read_only(v, false);
    f();
    set_read_only(v, ro);
}

pub fn set_bytes(v: &NSView, b: &[u8]) {
    writable(v, || {
        send(v, SCI_APPENDTEXT, b.len(), b.as_ptr() as isize);
    });
    send(v, SCI_EMPTYUNDOBUFFER, 0, 0);
    send(v, SCI_SETSAVEPOINT, 0, 0);
    reset_change_history(v);
}

pub fn eol_mode(v: &NSView) -> usize {
    send(v, SCI_GETEOLMODE, 0, 0) as usize
}

pub fn set_eol_mode(v: &NSView, mode: usize) {
    send(v, SCI_SETEOLMODE, mode, 0);
}

pub fn convert_eols(v: &NSView, mode: usize) {
    send(v, SCI_CONVERTEOLS, mode, 0);
    set_eol_mode(v, mode);
}

pub fn overtype(v: &NSView) -> bool {
    send(v, SCI_GETOVERTYPE, 0, 0) != 0
}

pub fn length(v: &NSView) -> isize {
    send(v, SCI_GETLENGTH, 0, 0)
}

// Ln, Col, Pos and, for a selection, its characters and lines as Notepad++ counts them.
pub fn position_info(v: &NSView) -> (isize, isize, isize, Option<(isize, isize)>) {
    let pos = send(v, SCI_GETCURRENTPOS, 0, 0);
    let line = send(v, SCI_LINEFROMPOSITION, pos as usize, 0);
    let col = send(v, SCI_GETCOLUMN, pos as usize, 0);
    let (s, e) = selection(v);
    let sel = (s != e).then(|| {
        let (l1, mut l2) = (
            send(v, SCI_LINEFROMPOSITION, s as usize, 0),
            send(v, SCI_LINEFROMPOSITION, e as usize, 0),
        );
        if l1 != l2 && send(v, SCI_POSITIONFROMLINE, l2 as usize, 0) == e {
            l2 -= 1;
        }
        (send(v, SCI_COUNTCHARACTERS, s as usize, e), l2 - l1 + 1)
    });
    (line + 1, col + 1, pos + 1, sel)
}

pub fn read_only(v: &NSView) -> bool {
    send(v, SCI_GETREADONLY, 0, 0) != 0
}

pub fn set_read_only(v: &NSView, on: bool) {
    send(v, SCI_SETREADONLY, on as usize, 0);
}

// Replaces all text as one undo step and keeps the modified state.
pub fn replace_text(v: &NSView, b: &[u8]) {
    let clean = !is_modified(v);
    send(v, SCI_SETTARGETRANGE, 0, length(v));
    send(v, SCI_REPLACETARGET, b.len(), b.as_ptr() as isize);
    if clean {
        set_save_point(v);
    }
}

pub fn reload(v: &NSView, b: &[u8]) {
    writable(v, || {
        send(v, SCI_CLEARALL, 0, 0);
        set_bytes(v, b);
    });
}

pub fn bytes(v: &NSView) -> Vec<u8> {
    let n = send(v, SCI_GETLENGTH, 0, 0) as usize;
    let mut b = vec![0u8; n + 1];
    send(v, SCI_GETTEXT, n + 1, b.as_mut_ptr() as isize);
    b.truncate(n);
    b
}

pub fn doc(v: &NSView) -> Doc {
    Doc::from_pointer(send(v, SCI_GETDOCPOINTER, 0, 0))
}

pub fn selection(v: &NSView) -> (isize, isize) {
    (
        send(v, SCI_GETSELECTIONSTART, 0, 0),
        send(v, SCI_GETSELECTIONEND, 0, 0),
    )
}

pub fn select(v: &NSView, (s, e): (isize, isize)) {
    send(v, SCI_SETSEL, s as usize, e);
    send(v, SCI_SCROLLRANGE, e as usize, s);
}

pub fn line_info(v: &NSView) -> (isize, isize) {
    let pos = send(v, SCI_GETCURRENTPOS, 0, 0);
    (
        send(v, SCI_LINEFROMPOSITION, pos as usize, 0) + 1,
        send(v, SCI_GETLINECOUNT, 0, 0),
    )
}

pub fn goto_line(v: &NSView, line: isize) {
    send(v, SCI_GOTOLINE, (line - 1).max(0) as usize, 0);
}

// Search results view: the Notepad++ searchResult lexer reads the match offsets through @MarkingsStruct.
pub fn setup_results(v: &NSView, cfg: &Config) -> usize {
    send(v, SCI_SETIDENTIFIER, RESULTS_ID, 0);
    send(v, SCI_SETUNDOCOLLECTION, 0, 0);
    apply_language(
        v,
        cfg,
        cfg.languages.iter().find(|l| l.name == "searchResult"),
    );
    send(v, SCI_SETMARGINWIDTHN, 0, 0);
    send(v, SCI_SETREADONLY, 1, 0);
    unsafe { npp_markings_new() as usize }
}

pub fn prepend_results(v: &NSView, markings: usize, lines: &[Line], text: &[u8]) {
    let counts: Vec<isize> = lines.iter().map(|l| l.marks.len() as isize).collect();
    let pairs: Vec<isize> = lines
        .iter()
        .flat_map(|l| l.marks.iter().flat_map(|&(s, e)| [s, e]))
        .collect();
    let m = unsafe {
        npp_markings_set(
            markings as *mut c_void,
            counts.as_ptr(),
            counts.len() as isize,
            pairs.as_ptr(),
        )
    };
    let (k, val) = (
        CString::new("@MarkingsStruct").unwrap(),
        CString::new(format!("{m:p}")).unwrap(),
    );
    send(
        v,
        SCI_SETPROPERTY,
        k.as_ptr() as usize,
        val.as_ptr() as isize,
    );
    send(v, SCI_SETREADONLY, 0, 0);
    send(v, SCI_SETTARGETRANGE, 0, 0);
    send(v, SCI_REPLACETARGET, text.len(), text.as_ptr() as isize);
    send(v, SCI_SETREADONLY, 1, 0);
    select(v, (0, 0));
    send(v, SCI_COLOURISE, 0, -1);
}

pub fn set_save_point(v: &NSView) {
    send(v, SCI_SETSAVEPOINT, 0, 0);
}

fn set_style(v: &NSView, s: &Style) {
    let id = s.id;
    if let Some(c) = s.fg {
        send(v, SCI_STYLESETFORE, id, c);
    }
    if let Some(c) = s.bg {
        send(v, SCI_STYLESETBACK, id, c);
    }
    if !s.font_name.is_empty() {
        send_str(v, SCI_STYLESETFONT, id, &s.font_name);
    }
    if let Some(sz) = s.font_size.filter(|&z| z > 0) {
        send(v, SCI_STYLESETSIZE, id, sz);
    }
    if let Some(f) = s.font_style {
        send(v, SCI_STYLESETBOLD, id, (f & 1) as isize);
        send(v, SCI_STYLESETITALIC, id, (f >> 1 & 1) as isize);
        send(v, SCI_STYLESETUNDERLINE, id, (f >> 2 & 1) as isize);
    }
}

pub fn apply_language(v: &NSView, cfg: &Config, lang: Option<&crate::config::Language>) {
    send(v, SCI_SETCODEPAGE, SC_CP_UTF8, 0);
    let global = |name: &str| cfg.global_styles.iter().find(|s| s.name == name);
    if let Some(d) = cfg.global_styles.iter().find(|s| s.id == STYLE_DEFAULT) {
        set_style(v, d);
    }
    send(v, SCI_STYLECLEARALL, 0, 0);
    for s in cfg.global_styles.iter().filter(|s| s.id > STYLE_DEFAULT) {
        set_style(v, s);
    }
    if let Some(c) = global("Current line background colour").and_then(|s| s.bg) {
        send(v, SCI_SETCARETLINEVISIBLE, 1, 0);
        send(v, SCI_SETCARETLINEBACK, c as usize, 0);
    }
    if let Some(c) = global("Caret colour").and_then(|s| s.fg) {
        send(v, SCI_SETCARETFORE, c as usize, 0);
    }
    if let Some(c) = global("Selected text colour").and_then(|s| s.bg) {
        send(v, SCI_SETSELBACK, 1, c);
    }
    let name = lang.map_or("normal", |l| l.name.as_str());
    let setup = lang::setup(cfg, name);
    let lexer = CString::new(setup.lexer).unwrap();
    send(v, SCI_SETILEXER, 0, unsafe { CreateLexer(lexer.as_ptr()) }
        as isize);
    for (key, value) in setup.props {
        let (k, val) = (CString::new(key).unwrap(), CString::new(value).unwrap());
        send(
            v,
            SCI_SETPROPERTY,
            k.as_ptr() as usize,
            val.as_ptr() as isize,
        );
    }
    for (i, words) in setup.keywords {
        send_str(v, SCI_SETKEYWORDS, i, &words);
    }
    for (base, lists) in lang::substyles(cfg, name) {
        let first = send(v, SCI_ALLOCATESUBSTYLES, base, lists.len() as isize);
        for (k, words) in lists.iter().enumerate().filter(|_| first >= 0) {
            send_str(v, SCI_SETIDENTIFIERS, first as usize + k, words);
        }
    }
    for id in setup.eol_filled {
        send(v, SCI_STYLESETEOLFILLED, id, 1);
    }
    for styler in setup.stylers {
        if let Some((_, styles)) = cfg.lexer_styles.iter().find(|(n, _)| n == styler) {
            styles.iter().for_each(|s| set_style(v, s));
        }
    }
    send(v, SCI_SETMARGINTYPEN, 0, SC_MARGIN_NUMBER);
    crate::prefs::line_number_width(v);
    setup_fold(v, cfg, name);
    setup_tabs(v, name);
    setup_indent_guides(
        v,
        view::python_style_indent(name),
        send(v, SCI_GETINDENTATIONGUIDES, 0, 0) != 0,
    );
    send(v, SCI_COLOURISE, 0, -1);
}

const FOLD_MARGIN: usize = 3;
// ScintillaEditView::_markersArray: the fold marker numbers, then the marker symbols of each folderMarkStyle.
const FOLD_MARKERS: [usize; 7] = [31, 30, 29, 28, 25, 26, 27];

pub fn fold_symbols(style: &str) -> [isize; 7] {
    match style {
        "simple" => [7, 8, 5, 5, 5, 5, 5],
        "arrow" => [6, 2, 5, 5, 5, 5, 5],
        "circle" => [20, 18, 9, 16, 19, 21, 17],
        _ => [14, 12, 9, 10, 13, 15, 11],
    }
}

// Fold margin, box markers, fold colours and fold properties as ScintillaEditView sets them.
pub fn setup_fold(v: &NSView, cfg: &Config, lang: &str) {
    for (k, val) in view::fold_props(lang) {
        let (k, val) = (CString::new(k).unwrap(), CString::new(val).unwrap());
        send(
            v,
            SCI_SETPROPERTY,
            k.as_ptr() as usize,
            val.as_ptr() as isize,
        );
    }
    let global = |name: &str| cfg.global_styles.iter().find(|s| s.name == name);
    let fold = global("Fold");
    let (fg, bg) = (
        fold.and_then(|s| s.bg).unwrap_or(0xFFFFFF),
        fold.and_then(|s| s.fg).unwrap_or(0x808080),
    );
    let active = global("Fold active").and_then(|s| s.fg).unwrap_or(0x0000FF);
    let style = crate::prefs::with(|p| p.folder_style.clone());
    for (n, m) in FOLD_MARKERS.into_iter().zip(fold_symbols(&style)) {
        send(v, SCI_MARKERDEFINE, n, m);
        send(v, SCI_MARKERSETFORE, n, fg);
        send(v, SCI_MARKERSETBACK, n, bg);
        send(v, SCI_MARKERSETBACKSELECTED, n, active);
    }
    send(v, SCI_MARKERENABLEHIGHLIGHT, 1, 0);
    let margin = global("Fold margin");
    send(
        v,
        SCI_SETFOLDMARGINCOLOUR,
        1,
        margin.and_then(|s| s.bg).unwrap_or(0x808080),
    );
    send(
        v,
        SCI_SETFOLDMARGINHICOLOUR,
        1,
        margin.and_then(|s| s.fg).unwrap_or(0xFFFFFF),
    );
    send(v, SCI_SETMARGINTYPEN, FOLD_MARGIN, 0);
    send(v, SCI_SETMARGINMASKN, FOLD_MARGIN, SC_MASK_FOLDERS);
    send(
        v,
        SCI_SETMARGINWIDTHN,
        FOLD_MARGIN,
        if view::needs_fold_margin(lang) && style != "none" { 14 } else { 0 },
    );
    send(v, SCI_SETMARGINSENSITIVEN, FOLD_MARGIN, 1);
    send(v, SCI_SETFOLDFLAGS, SC_FOLDFLAG_LINEAFTER_CONTRACTED, 0);
    send(v, SCI_SETAUTOMATICFOLD, SC_AUTOMATICFOLD_ALL, 0);
}

fn set_representation(v: &NSView, ch: &str, text: &str, plain: bool) {
    let (c, t) = (
        CString::new(ch.replace('\0', "")).unwrap(),
        CString::new(text).unwrap(),
    );
    send(
        v,
        SCI_SETREPRESENTATION,
        c.as_ptr() as usize,
        t.as_ptr() as isize,
    );
    if plain {
        send(v, SCI_SETREPRESENTATIONAPPEARANCE, c.as_ptr() as usize, 0);
    }
}

// Show Symbol: ScintillaEditView showWSAndTab, showEOL, showNpc and showCcUniEol.
pub fn setup_symbols(v: &NSView, cfg: &Config, ws: bool, eol: bool, npc: bool, cc: bool) {
    send(v, SCI_SETVIEWWS, ws as usize, 0);
    send(v, SCI_SETWHITESPACESIZE, 2, 0);
    if let Some(c) = cfg
        .global_styles
        .iter()
        .find(|s| s.name == "White space symbol")
        .and_then(|s| s.fg)
    {
        send(v, SCI_SETWHITESPACEFORE, 1, c);
    }
    send(v, SCI_SETVIEWEOL, eol as usize, 0);
    send(v, SCI_CLEARALLREPRESENTATIONS, 0, 0);
    if npc {
        view::npc_chars()
            .iter()
            .for_each(|(c, a)| set_representation(v, c, a, false));
    }
    for (c, a) in view::cc_chars() {
        set_representation(v, c, if cc { a } else { "\u{200B}" }, !cc);
    }
}

// ScintillaEditView::setTabSettings with the Lang and NppGUI values of Preferences > Indentation.
pub fn setup_tabs(v: &NSView, lang: &str) {
    const SCI_SETBACKSPACEUNINDENTS: u32 = 2262;
    let (width, use_tabs, bs) = crate::prefs::tab_settings(lang);
    send(v, SCI_SETTABWIDTH, width, 0);
    send(v, SCI_SETUSETABS, use_tabs as usize, 0);
    send(v, SCI_SETBACKSPACEUNINDENTS, bs as usize, 0);
}

pub fn setup_indent_guides(v: &NSView, look_forward: bool, on: bool) {
    let mode = match (on, look_forward) {
        (false, _) => 0,
        (true, true) => SC_IV_LOOKFORWARD,
        (true, false) => SC_IV_LOOKBOTH,
    };
    send(v, SCI_SETINDENTATIONGUIDES, mode, 0);
}

// Word wrap with the Line Wrap indent of Preferences > Editing 1, and the wrap symbol at the line end.
pub fn setup_wrap(v: &NSView, wrap: bool, symbol: bool) {
    send(v, SCI_SETWRAPINDENTMODE, crate::prefs::with(|p| p.wrap_indent()), 0);
    send(v, SCI_SETWRAPMODE, wrap as usize, 0);
    send(v, SCI_SETWRAPVISUALFLAGSLOCATION, 0, 0);
    send(v, SCI_SETWRAPVISUALFLAGS, symbol as usize, 0);
}

// Sets the zoom and fits the line number margin to it.
pub fn set_zoom(v: &NSView, zoom: isize) {
    send(v, SCI_SETZOOM, zoom as usize, 0);
    crate::prefs::line_number_width(v);
}

const SCI_SETCHANGEHISTORY: u32 = 2780;
const SCI_GETCHANGEHISTORY: u32 = 2781;

fn margin_style<'a>(cfg: &'a Config, name: &str) -> Option<&'a Style> {
    cfg.global_styles.iter().find(|s| s.name == name)
}

// Margin background as ScintillaEditView::performGlobalStyles reads it.
fn margin_back(cfg: &Config, name: &str) -> isize {
    margin_style(cfg, name)
        .or_else(|| margin_style(cfg, "Line number margin"))
        .and_then(|s| s.bg)
        .unwrap_or(0xE0E0E0)
}

// Notepad++ margin 1: bookmarks with the Notepad++ icon; a click toggles a bookmark.
pub fn setup_bookmark_margin(v: &NSView, cfg: &Config) {
    use crate::search_extras::{bookmark_icon, BOOKMARK_MARGIN, MARK_BOOKMARK};
    const SC_MARGIN_COLOUR: isize = 6;
    const SCI_SETMARGINBACKN: u32 = 2250;
    const SCI_MARKERSETALPHA: u32 = 2476;
    const SCI_RGBAIMAGESETWIDTH: u32 = 2624;
    const SCI_RGBAIMAGESETHEIGHT: u32 = 2625;
    const SCI_MARKERDEFINERGBAIMAGE: u32 = 2626;
    let m = BOOKMARK_MARGIN;
    send(v, SCI_SETMARGINTYPEN, m, SC_MARGIN_COLOUR);
    send(v, SCI_SETMARGINBACKN, m, margin_back(cfg, "Bookmark margin"));
    send(v, SCI_SETMARGINMASKN, m, 1 << MARK_BOOKMARK);
    send(v, SCI_SETMARGINWIDTHN, m, crate::prefs::bookmark_width());
    send(v, SCI_SETMARGINSENSITIVEN, m, 1);
    send(v, SCI_RGBAIMAGESETWIDTH, 14, 0);
    send(v, SCI_RGBAIMAGESETHEIGHT, 14, 0);
    send(v, SCI_MARKERDEFINERGBAIMAGE, MARK_BOOKMARK, bookmark_icon().as_ptr() as isize);
    send(v, SCI_MARKERSETALPHA, MARK_BOOKMARK, 70);
}

// Notepad++ margin 2: change history markers, on by default as in Notepad++.
pub fn setup_change_history(v: &NSView, cfg: &Config) {
    use crate::search_extras::{CHANGE_MARGIN, HISTORY_MASK};
    const SC_MARGIN_COLOUR: isize = 6;
    const SCI_SETMARGINBACKN: u32 = 2250;
    let m = CHANGE_MARGIN;
    send(v, SCI_SETMARGINTYPEN, m, SC_MARGIN_COLOUR);
    send(v, SCI_SETMARGINBACKN, m, margin_back(cfg, "Change History margin"));
    send(v, SCI_SETMARGINMASKN, m, HISTORY_MASK);
    send(v, SCI_SETMARGINWIDTHN, m, crate::prefs::change_margin_width());
    for (marker, name, rgb) in [
        (21, "Change History revert origin", 0xBFA040),
        (22, "Change History saved", 0x00A000),
        (23, "Change History modified", 0x0080FF),
        (24, "Change History revert modified", 0x00C0A0),
    ] {
        let s = margin_style(cfg, name);
        send(v, SCI_MARKERSETFORE, marker, s.and_then(|s| s.fg).unwrap_or(rgb));
        send(v, SCI_MARKERSETBACK, marker, s.and_then(|s| s.bg).unwrap_or(rgb));
    }
    send(v, SCI_SETCHANGEHISTORY, crate::prefs::with(|p| p.change_history_flags()), 0);
}

// Starts change history again from the current text; Scintilla needs an empty undo buffer for this.
pub fn reset_change_history(v: &NSView) {
    let flags = send(v, SCI_GETCHANGEHISTORY, 0, 0);
    send(v, SCI_SETCHANGEHISTORY, 0, 0);
    send(v, SCI_SETCHANGEHISTORY, flags as usize, 0);
}

// Multiple and rectangular selection settings as Notepad_plus::init sets them; Option+drag makes a rectangle, Cmd+click adds a caret.
pub fn setup_multi_selection(v: &NSView) {
    const SCI_SETMULTIPLESELECTION: u32 = 2563;
    const SCI_SETADDITIONALSELECTIONTYPING: u32 = 2565;
    const SCI_SETVIRTUALSPACEOPTIONS: u32 = 2596;
    const SCI_SETMULTIPASTE: u32 = 2614;
    const SCI_AUTOCSETMULTI: u32 = 2636;
    const SCI_SETMOUSESELECTIONRECTANGULARSWITCH: u32 = 2668;
    const SCVS_RECTANGULARSELECTION: usize = 1;
    const SC_MULTIPASTE_EACH: usize = 1;
    const SC_MULTIAUTOC_EACH: usize = 1;
    let (multi, virtual_space) = crate::prefs::with(|p| (p.multi_selection, p.virtual_space));
    send(v, SCI_SETMULTIPLESELECTION, multi as usize, 0);
    send(v, SCI_SETADDITIONALSELECTIONTYPING, 1, 0);
    send(v, SCI_SETVIRTUALSPACEOPTIONS, SCVS_RECTANGULARSELECTION | if virtual_space { 6 } else { 0 }, 0);
    send(v, SCI_SETMULTIPASTE, SC_MULTIPASTE_EACH, 0);
    send(v, SCI_AUTOCSETMULTI, SC_MULTIAUTOC_EACH, 0);
    send(v, SCI_SETMOUSESELECTIONRECTANGULARSWITCH, 1, 0);
}
