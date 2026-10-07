// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{Config, Style};
use crate::lang;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_app_kit::NSView;
use std::ffi::{c_void, CString};

pub const SCN_SAVEPOINTREACHED: u32 = 2002;
pub const SCN_SAVEPOINTLEFT: u32 = 2003;
const SCI_GETLENGTH: u32 = 2006;
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
const SCI_TEXTWIDTH: u32 = 2276;
const SCI_APPENDTEXT: u32 = 2282;
const SCI_COLOURISE: u32 = 4003;
const SCI_SETPROPERTY: u32 = 4004;
const SCI_SETKEYWORDS: u32 = 4005;
const SCI_SETILEXER: u32 = 4033;
const STYLE_DEFAULT: usize = 32;
const STYLE_LINENUMBER: usize = 33;
const SC_CP_UTF8: usize = 65001;
const SC_MARGIN_NUMBER: isize = 1;

extern "C" {
    #[link_name = "OBJC_CLASS_$_ScintillaView"]
    static SCINTILLA_VIEW_CLASS: u8;
    fn CreateLexer(name: *const std::ffi::c_char) -> *mut c_void;
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

pub fn set_bytes(v: &NSView, b: &[u8]) {
    send(v, SCI_APPENDTEXT, b.len(), b.as_ptr() as isize);
    send(v, SCI_EMPTYUNDOBUFFER, 0, 0);
    send(v, SCI_SETSAVEPOINT, 0, 0);
}

pub fn bytes(v: &NSView) -> Vec<u8> {
    let n = send(v, SCI_GETLENGTH, 0, 0) as usize;
    let mut b = vec![0u8; n + 1];
    send(v, SCI_GETTEXT, n + 1, b.as_mut_ptr() as isize);
    b.truncate(n);
    b
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
        send_str(v, SCI_SETKEYWORDS, i, words);
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
    let w = send_str(v, SCI_TEXTWIDTH, STYLE_LINENUMBER, "_99999");
    send(v, SCI_SETMARGINWIDTHN, 0, w);
    send(v, SCI_COLOURISE, 0, -1);
}
