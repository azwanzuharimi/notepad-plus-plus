// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{cfg, item, lang, nested, ns, sci, search, tagged, App, Tab};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags,
    NSFloatingWindowLevel, NSMenuItem, NSNormalWindowLevel, NSView, NSWindow,
};
use objc2_foundation::{
    NSDate, NSDateFormatter, NSDateFormatterStyle, NSDictionary, NSNumber, NSUserDefaults,
};
use std::path::Path;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const WS: usize = 0;
pub const EOL: usize = 1;
pub const NPC: usize = 2;
pub const CC: usize = 3;
pub const ALL: usize = 4;
pub const GUIDES: usize = 5;
pub const WRAP_SYMBOL: usize = 6;
pub const WRAP: usize = 7;

const FIRST: usize = 9;
const LAST: usize = 10;
const NEXT: usize = 11;
const PREV: usize = 12;
const TO_START: usize = 0;
const TO_END: usize = 1;
const FORWARD: usize = 2;
const BACKWARD: usize = 3;

const SCI_GETLINECOUNT: u32 = 2154;
const SCI_GETLENGTH: u32 = 2006;
const SCI_GETENDSTYLED: u32 = 2028;
const SCI_COLOURISE: u32 = 4003;
const SCI_ZOOMIN: u32 = 2333;
const SCI_ZOOMOUT: u32 = 2334;
const SCI_GETZOOM: u32 = 2374;
const SCI_GETFOLDLEVEL: u32 = 2223;
const SCI_GETFOLDPARENT: u32 = 2225;
const SCI_GETFOLDEXPANDED: u32 = 2230;
const SCI_TOGGLEFOLD: u32 = 2231;
const SCI_FOLDALL: u32 = 2662;
const SCI_SCROLLCARET: u32 = 2169;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_LINEFROMPOSITION: u32 = 2166;
const SCI_GETSELECTIONS: u32 = 2570;
const SCI_GETSELECTIONNSTART: u32 = 2585;
const SCI_GETSELECTIONNEND: u32 = 2587;
const SCI_GETLINESELSTARTPOSITION: u32 = 2424;
const SCI_GETLINESELENDPOSITION: u32 = 2425;
const SCI_SELECTIONISRECTANGLE: u32 = 2372;
const SC_FOLDLEVELBASE: isize = 0x400;
const SC_FOLDLEVELHEADERFLAG: isize = 0x2000;
const SC_FOLDLEVELNUMBERMASK: isize = 0x0FFF;
const SC_FOLDACTION_EXPAND: usize = 1;
const SC_FOLDACTION_CONTRACT_EVERY_LEVEL: usize = 4;

// Global View options: Show Symbol, indent guide, wrap symbol and word wrap flags by tag, and the zoom.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Opts {
    pub on: [bool; 8],
    pub zoom: isize,
}

// ScintillaViewParams defaults: control characters and indent guides show.
impl Default for Opts {
    fn default() -> Self {
        let mut on = [false; 8];
        on[CC] = true;
        on[GUIDES] = true;
        Opts { on, zoom: 0 }
    }
}

pub fn checked(o: &Opts, tag: usize) -> bool {
    if tag == ALL {
        o.on[..ALL].iter().all(|&b| b)
    } else {
        o.on[tag]
    }
}

pub fn toggle(mut o: Opts, tag: usize) -> Opts {
    if tag == ALL {
        let v = !checked(&o, ALL);
        o.on[..ALL].fill(v);
    } else {
        o.on[tag] = !o.on[tag];
    }
    o
}

const NPC_SOURCE: &str =
    include_str!("../../PowerEditor/src/ScintillaComponent/ScintillaEditView.cpp");

// Rows of g_ccUniEolChars and g_nonPrintingChars in ScintillaEditView.cpp: (UTF-8 character, abbreviation).
fn table(name: &str) -> Vec<(String, String)> {
    NPC_SOURCE
        .split(name)
        .nth(1)
        .unwrap_or("")
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with("} };"))
        .filter_map(|l| {
            let f: Vec<&str> = l.trim().strip_prefix("{\"")?.split('"').collect();
            let cp = u32::from_str_radix(f.get(4)?.strip_prefix("U+")?, 16).ok()?;
            Some((char::from_u32(cp)?.to_string(), f.get(2)?.to_string()))
        })
        .collect()
}

pub fn cc_chars() -> &'static [(String, String)] {
    static T: OnceLock<Vec<(String, String)>> = OnceLock::new();
    T.get_or_init(|| table("g_ccUniEolChars{"))
}

pub fn npc_chars() -> &'static [(String, String)] {
    static T: OnceLock<Vec<(String, String)>> = OnceLock::new();
    T.get_or_init(|| table("g_nonPrintingChars{"))
}

// Fold properties from the ScintillaEditView lexer setters for each language.
pub fn fold_props(lang: &str) -> Vec<(&'static str, &'static str)> {
    let base = vec![("fold", "1"), ("fold.compact", "0")];
    let comment = ("fold.comment", "1");
    let pre = ("fold.preprocessor", "1");
    let extra: Vec<(&str, &str)> = match lang {
        "normal" | "nfo" | "tcl" => return vec![],
        "c" | "cpp" | "java" | "rc" | "cs" | "actionscript" | "swift" | "go" | "javascript"
        | "javascript.js" | "objc" | "typescript" => {
            vec![comment, ("fold.cpp.comment.explicit", "0"), pre]
        }
        "html" | "php" | "asp" | "jsp" | "xml" => {
            vec![("fold.html", "1"), ("fold.hypertext.comment", "1")]
        }
        "json" | "json5" => vec![],
        "python" => vec![comment, ("fold.quotes.python", "1")],
        "pascal" | "autoit" | "verilog" | "fcST" => vec![comment, pre],
        "asm" => vec![
            comment,
            ("fold.asm.syntax.based", "1"),
            ("fold.asm.comment.multiline", "1"),
            ("fold.asm.comment.explicit", "1"),
        ],
        "baanc" => vec![
            comment,
            pre,
            ("fold.baan.syntax.based", "1"),
            ("fold.baan.keywords.based", "1"),
            ("fold.baan.sections", "1"),
            ("fold.baan.inner.level", "1"),
        ],
        "raku" => vec![
            comment,
            ("fold.raku.comment.multiline", "1"),
            ("fold.raku.comment.pod", "1"),
        ],
        _ if lang::lexer_name(lang) == "null" => return vec![],
        _ => vec![comment],
    };
    [base, extra].concat()
}

pub(crate) const LANGS: &str = include_str!("../../PowerEditor/src/langs.model.xml");

// The tabSettings value of a language in langs.model.xml, when Lang::setTabInfo uses it.
pub fn model_tab_info(lang: &str) -> Option<usize> {
    LANGS
        .lines()
        .find(|l| l.trim().starts_with(&format!("<Language name=\"{lang}\" ")))
        .and_then(|l| {
            l.split("tabSettings=\"")
                .nth(1)?
                .split('"')
                .next()?
                .parse::<usize>()
                .ok()
        })
        .filter(|i| i & 0x7F != 0)
}

// ScintillaEditView::isNeededFolderMargin.
pub fn needs_fold_margin(lang: &str) -> bool {
    ![
        "nfo",
        "batch",
        "normal",
        "makefile",
        "haskell",
        "smalltalk",
        "kix",
        "ada",
    ]
    .contains(&lang)
}

// ScintillaEditView::isPythonStyleIndentation.
pub fn python_style_indent(lang: &str) -> bool {
    [
        "python",
        "coffeescript",
        "haskell",
        "c",
        "cpp",
        "objc",
        "cs",
        "java",
        "php",
        "javascript",
        "javascript.js",
        "makefile",
        "asn1",
        "gdscript",
    ]
    .contains(&lang)
}

// ScintillaEditView::isFoldIndentationBased, by Lexilla lexer name.
pub fn indent_based(lexer: &str) -> bool {
    ["python", "coffeescript", "haskell", "nimrod", "vb", "yaml"].contains(&lexer)
}

// Header lines that Fold Level and Unfold Level change, from ScintillaEditView::foldLevel and foldIndentationBasedLevel.
pub fn level_headers(levels: &[isize], level: usize, indent: bool) -> Vec<usize> {
    let mut stack: Vec<isize> = vec![];
    let mut out = vec![];
    for (line, &l) in levels.iter().enumerate() {
        if l & SC_FOLDLEVELHEADERFLAG == 0 {
            continue;
        }
        if indent {
            let n = l & SC_FOLDLEVELNUMBERMASK;
            while stack.last().is_some_and(|&t| n <= t) {
                stack.pop();
            }
            stack.push(n);
            if stack.len() == level + 1 {
                out.push(line);
            }
        } else if (l - SC_FOLDLEVELBASE) & SC_FOLDLEVELNUMBERMASK == level as isize {
            out.push(line);
        }
    }
    out
}

// Tab to select for 1st..9th, First, Last, Next and Previous Tab (NppCommands.cpp IDM_VIEW_TAB*).
pub fn tab_target(tag: usize, cur: usize, n: usize) -> Option<usize> {
    match tag {
        _ if n == 0 => None,
        0..=8 if tag < n => Some(tag),
        0..=8 => (n > 1).then_some(n - 1),
        FIRST => Some(0),
        LAST => Some(n - 1),
        NEXT => Some((cur + 1) % n),
        PREV => Some((cur + n - 1) % n),
        _ => None,
    }
}

// New place of the current tab for Move to Start, Move to End, Move Tab Forward and Move Tab Backward.
pub fn move_target(tag: usize, cur: usize, n: usize) -> Option<usize> {
    let to = match tag {
        TO_START => 0,
        TO_END => n.checked_sub(1)?,
        FORWARD => cur + 1,
        BACKWARD => cur.checked_sub(1)?,
        _ => return None,
    };
    (to != cur && to < n).then_some(to)
}

// Characters without line endings, as Notepad++ countUtf8Characters counts them.
pub fn count_chars(b: &[u8]) -> usize {
    b.iter()
        .filter(|&&c| c & 0xC0 != 0x80 && c != b'\n' && c != b'\r')
        .count()
}

const WORD_BREAKS: &[u8] = b" \t\\.,;:!?()+\r\n-*/=][{}&~\"'`|@$%<>^";

// Runs of characters outside the Notepad++ wordCount regex class.
pub fn count_words(b: &[u8]) -> usize {
    b.split(|c| WORD_BREAKS.contains(c))
        .filter(|w| !w.is_empty())
        .count()
}

pub struct Counts {
    pub chars: usize,
    pub words: usize,
    pub lines: usize,
    pub length: usize,
    pub sel_chars: usize,
    pub sel_bytes: usize,
    pub ranges: usize,
}

// Text of the Notepad++ Summary message box.
pub fn summary_text(file: Option<(&str, &str, &str)>, c: &Counts) -> String {
    let n = search::commafy;
    let mut s = String::new();
    if let Some((path, created, modified)) = file {
        s += &format!("Full file path: {path}\nCreated: {created}\nModified: {modified}\n");
    }
    s += &format!(
        "Characters (without line endings): {}\nWords: {}\nLines: {}\nDocument length: {}\n{} selected characters ({} bytes) in {} ranges",
        n(c.chars),
        n(c.words),
        n(c.lines),
        n(c.length),
        n(c.sel_chars),
        n(c.sel_bytes),
        n(c.ranges)
    );
    s
}

fn send(v: &NSView, msg: u32, w: usize, l: isize) -> isize {
    sci::send(v, msg, w, l)
}

fn date(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0., |d| d.as_secs_f64());
    let d = NSDate::dateWithTimeIntervalSince1970(secs);
    NSDateFormatter::localizedStringFromDate_dateStyle_timeStyle(
        &d,
        NSDateFormatterStyle::ShortStyle,
        NSDateFormatterStyle::MediumStyle,
    )
    .to_string()
}

fn lang_of(t: &Tab) -> &'static str {
    crate::language::tab_language(t).map_or("normal", |l| l.name.as_str())
}

fn fold_line(v: &NSView, line: isize, expand: bool) {
    let h = if send(v, SCI_GETFOLDLEVEL, line as usize, 0) & SC_FOLDLEVELHEADERFLAG != 0 {
        line
    } else {
        send(v, SCI_GETFOLDPARENT, line as usize, 0)
    };
    if h >= 0 && (send(v, SCI_GETFOLDEXPANDED, h as usize, 0) != 0) != expand {
        send(v, SCI_TOGGLEFOLD, h as usize, 0);
    }
}

fn style_all(v: &NSView) {
    if send(v, SCI_GETENDSTYLED, 0, 0) < send(v, SCI_GETLENGTH, 0, 0) {
        send(v, SCI_COLOURISE, 0, -1);
    }
}

impl App {
    fn view_opts(&self) -> Opts {
        self.ivars().view.get()
    }

    // Applies the global View options to one editor.
    pub(crate) fn apply_view(&self, v: &NSView, lang: &str, c: &crate::config::Config) {
        let o = self.view_opts();
        sci::setup_symbols(v, c, o.on[WS], o.on[EOL], o.on[NPC], o.on[CC]);
        sci::setup_indent_guides(v, python_style_indent(lang), o.on[GUIDES]);
        sci::setup_wrap(v, o.on[WRAP], o.on[WRAP_SYMBOL]);
        sci::set_zoom(v, o.zoom);
        crate::prefs::apply_editor(v, lang, c);
    }

    pub(crate) fn apply_view_all(&self) {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        tabs.iter()
            .for_each(|t| self.apply_view(&t.view, lang_of(t), cfg()));
    }

    pub(crate) fn view_option(&self, tag: usize) {
        self.ivars().view.set(toggle(self.view_opts(), tag));
        self.apply_view_all();
    }

    pub(crate) fn zoom(&self, tag: isize) {
        let Some(v) = self.editor() else { return };
        let mut o = self.view_opts();
        o.zoom = match tag {
            1 => {
                send(&v, SCI_ZOOMIN, 0, 0);
                send(&v, SCI_GETZOOM, 0, 0)
            }
            -1 => {
                send(&v, SCI_ZOOMOUT, 0, 0);
                send(&v, SCI_GETZOOM, 0, 0)
            }
            _ => 0,
        };
        self.ivars().view.set(o);
        self.apply_view_all();
    }

    pub(crate) fn always_on_top(&self) {
        let w = self.ivars().window.get().unwrap();
        w.setLevel(if w.level() == NSFloatingWindowLevel {
            NSNormalWindowLevel
        } else {
            NSFloatingWindowLevel
        });
    }

    // AppKit changes a Ctrl+Cmd+F item at launch and adds a second item, so the key is set after the launch.
    pub(crate) fn full_screen_key(&self) {
        let bar = objc2_app_kit::NSApplication::sharedApplication(self.mtm()).mainMenu();
        let Some(m) = bar.and_then(|b| crate::l10n::bar_menu(&b, "View")) else {
            return;
        };
        if let Some(i) = m.itemArray().iter().find(|i| i.action() == Some(sel!(fullScreen:))) {
            i.setKeyEquivalent(&ns("f"));
            i.setKeyEquivalentModifierMask(NSEventModifierFlags::Control | NSEventModifierFlags::Command);
        }
    }

    pub(crate) fn full_screen(&self) {
        self.ivars().window.get().unwrap().toggleFullScreen(None);
    }

    pub(crate) fn select_tab(&self, tag: usize) {
        let r = self.view_range(self.active_view());
        let cur = self.current().map_or(0, |c| c - r.start);
        if let Some(t) = tab_target(tag, cur, r.len()).and_then(|i| self.tab(r.start + i)) {
            self.tab_view().selectTabViewItem(Some(&t.item));
        }
    }

    pub(crate) fn move_tab(&self, tag: usize) {
        let r = self.view_range(self.active_view());
        let Some(cur) = self.current() else { return };
        if let Some(to) = move_target(tag, cur - r.start, r.len()) {
            self.move_tab_to(cur, r.start + to);
        }
    }

    // Moves the tab at `from` to `to` in the same view and selects it.
    pub(crate) fn move_tab_to(&self, from: usize, to: usize) {
        let p = self.pane_of(from);
        let start = self.view_range(p).start;
        let t = {
            let mut tabs = self.ivars().tabs.borrow_mut();
            let t = tabs.remove(from);
            tabs.insert(to, t.clone());
            t
        };
        let tv = self.doc_tabs(p);
        tv.removeTabViewItem(&t.item);
        tv.insertTabViewItem_atIndex(&t.item, (to - start) as isize);
        tv.selectTabViewItem(Some(&t.item));
    }

    pub(crate) fn fold_all(&self, expand: bool) {
        let Some(v) = self.editor() else { return };
        let action = if expand { SC_FOLDACTION_EXPAND } else { 0 };
        send(
            &v,
            SCI_FOLDALL,
            action | SC_FOLDACTION_CONTRACT_EVERY_LEVEL,
            0,
        );
        if expand {
            send(&v, SCI_SCROLLCARET, 0, 0);
        }
    }

    pub(crate) fn fold_current(&self, expand: bool) {
        let Some(v) = self.editor() else { return };
        style_all(&v);
        let pos = send(&v, SCI_GETCURRENTPOS, 0, 0);
        fold_line(&v, send(&v, SCI_LINEFROMPOSITION, pos as usize, 0), expand);
    }

    pub(crate) fn fold_level(&self, level: usize, expand: bool) {
        let Some(t) = self.current().and_then(|i| self.tab(i)) else {
            return;
        };
        let v = &t.view;
        style_all(v);
        let levels: Vec<isize> = (0..send(v, SCI_GETLINECOUNT, 0, 0))
            .map(|l| send(v, SCI_GETFOLDLEVEL, l as usize, 0))
            .collect();
        let indent = indent_based(lang::lexer_name(lang_of(&t)));
        for line in level_headers(&levels, level, indent) {
            fold_line(v, line as isize, expand);
        }
    }

    pub(crate) fn summary(&self) {
        let Some(t) = self.current().and_then(|i| self.tab(i)) else {
            return;
        };
        let v = &t.view;
        let b = sci::bytes(v);
        let (mut sel_chars, mut sel_bytes) = (0, 0);
        let n = send(v, SCI_GETSELECTIONS, 0, 0) as usize;
        for i in 0..n {
            let (s, e) = (
                send(v, SCI_GETSELECTIONNSTART, i, 0),
                send(v, SCI_GETSELECTIONNEND, i, 0),
            );
            sel_bytes += (e - s) as usize;
            let (l1, l2) = (
                send(v, SCI_LINEFROMPOSITION, s as usize, 0),
                send(v, SCI_LINEFROMPOSITION, e as usize, 0),
            );
            for l in l1..=l2 {
                let ls = send(v, SCI_GETLINESELSTARTPOSITION, l as usize, 0);
                let le = send(v, SCI_GETLINESELENDPOSITION, l as usize, 0);
                if ls >= 0 && ls <= le {
                    sel_chars += count_chars(&b[ls as usize..le as usize]);
                }
            }
        }
        let ranges = match n {
            1 => (sel_bytes > 0) as usize,
            _ if send(v, SCI_SELECTIONISRECTANGLE, 0, 0) != 0 => 1,
            _ => n,
        };
        let counts = Counts {
            chars: count_chars(&b),
            words: count_words(&b),
            lines: send(v, SCI_GETLINECOUNT, 0, 0) as usize,
            length: b.len(),
            sel_chars,
            sel_bytes,
            ranges,
        };
        let meta = t
            .path
            .as_deref()
            .and_then(|p| Some((p, std::fs::metadata(p).ok()?)));
        let file = meta.map(|(p, m): (&Path, _)| {
            let d = |r: std::io::Result<SystemTime>| r.map(date).unwrap_or_default();
            (p.display().to_string(), d(m.created()), d(m.modified()))
        });
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&ns("Summary"));
        a.setInformativeText(&ns(&summary_text(
            file.as_ref()
                .map(|(p, c, m)| (p.as_str(), c.as_str(), m.as_str())),
            &counts,
        )));
        a.runModal();
        self.focus();
    }

    // Sets the checkmarks of the View toggles; returns false for other items.
    pub(crate) fn validate_view(&self, item: &NSMenuItem) -> bool {
        let on = match item.action() {
            Some(a) if a == sel!(viewOption:) => checked(&self.view_opts(), item.tag() as usize),
            Some(a) if a == sel!(toggleDocList:) => self.panel_visible(crate::docking::LEFT),
            Some(a) if a == sel!(toggleFunctionList:) => self.panel_visible(crate::docking::RIGHT),
            Some(a) if self.panel_checked(a).is_some() => self.panel_checked(a) == Some(true),
            Some(a) if a == sel!(alwaysOnTop:) => self
                .ivars()
                .window
                .get()
                .is_some_and(|w| w.level() == NSFloatingWindowLevel),
            _ => return false,
        };
        item.setState(if on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        true
    }
}

fn keyed(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    tag: isize,
    key: &str,
    mods: NSEventModifierFlags,
    t: Option<&AnyObject>,
) -> Retained<NSMenuItem> {
    let i = item(mtm, title, action, key, t);
    i.setTag(tag);
    i.setKeyEquivalentModifierMask(mods);
    i
}

pub fn view_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    // Stop AppKit from adding its own full screen and window tab items to the View menu.
    NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
    let off: &AnyObject = &NSNumber::new_bool(false);
    let d = NSDictionary::from_slices(&[&*ns("NSFullScreenMenuItemEverywhere")], &[off]);
    unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&d) };
    let cmd = NSEventModifierFlags::Command;
    let opt = NSEventModifierFlags::Option;
    let ctrl = NSEventModifierFlags::Control;
    let sep = || NSMenuItem::separatorItem(mtm);
    let opt_item = |title: &str, tag: usize| tagged(mtm, title, sel!(viewOption:), tag as isize, t);
    let symbols = vec![
        opt_item("Show Space and Tab", WS),
        opt_item("Show End of Line", EOL),
        opt_item("Show Non-Printing Characters", NPC),
        opt_item("Show Control Characters & Unicode EOL", CC),
        opt_item("Show All Characters", ALL),
        sep(),
        opt_item("Show Indent Guide", GUIDES),
        opt_item("Show Wrap Symbol", WRAP_SYMBOL),
    ];
    let zoom = vec![
        keyed(mtm, "Zoom In", sel!(zoom:), 1, "=", cmd, t),
        keyed(mtm, "Zoom Out", sel!(zoom:), -1, "-", cmd, t),
        keyed(mtm, "Restore Default Zoom", sel!(zoom:), 0, "0", cmd, t),
    ];
    let ordinals = [
        "1st", "2nd", "3rd", "4th", "5th", "6th", "7th", "8th", "9th",
    ];
    let mut tabs: Vec<_> = ordinals
        .iter()
        .enumerate()
        .map(|(i, o)| {
            keyed(
                mtm,
                &format!("{o} Tab"),
                sel!(selectTab:),
                i as isize,
                &(i + 1).to_string(),
                cmd,
                t,
            )
        })
        .collect();
    let page_down = "\u{F72D}";
    let page_up = "\u{F72C}";
    let shift = cmd | NSEventModifierFlags::Shift;
    tabs.extend([
        sep(),
        tagged(mtm, "First Tab", sel!(selectTab:), FIRST as isize, t),
        tagged(mtm, "Last Tab", sel!(selectTab:), LAST as isize, t),
        keyed(
            mtm,
            "Next Tab",
            sel!(selectTab:),
            NEXT as isize,
            "}",
            cmd,
            t,
        ),
        keyed(
            mtm,
            "Previous Tab",
            sel!(selectTab:),
            PREV as isize,
            "{",
            cmd,
            t,
        ),
        sep(),
        tagged(mtm, "Move to Start", sel!(moveTab:), TO_START as isize, t),
        tagged(mtm, "Move to End", sel!(moveTab:), TO_END as isize, t),
        keyed(
            mtm,
            "Move Tab Forward",
            sel!(moveTab:),
            FORWARD as isize,
            page_down,
            shift,
            t,
        ),
        keyed(
            mtm,
            "Move Tab Backward",
            sel!(moveTab:),
            BACKWARD as isize,
            page_up,
            shift,
            t,
        ),
    ]);
    let levels = |action: Sel, mods: NSEventModifierFlags| -> Vec<_> {
        (1..=8)
            .map(|n| {
                // Ctrl+Opt+Cmd+8 is the macOS Invert colors shortcut.
                let key = if n == 8 && mods.contains(ctrl) {
                    String::new()
                } else {
                    n.to_string()
                };
                keyed(mtm, &n.to_string(), action, n - 1, &key, mods, t)
            })
            .collect()
    };
    vec![
        keyed(mtm, "Show Toolbar", sel!(toggleToolbarShown:), 0, "t", opt | cmd, None),
        sep(),
        item(mtm, "Always on Top", sel!(alwaysOnTop:), "", t),
        item(mtm, "Toggle Full Screen Mode", sel!(fullScreen:), "", t),
        keyed(mtm, "Post-It", sel!(postIt:), 0, "\u{F70F}", NSEventModifierFlags::empty(), t),
        item(mtm, "Distraction Free Mode", sel!(distractionFree:), "", t),
        sep(),
        nested(mtm, "Show Symbol", symbols),
        nested(mtm, "Zoom", zoom),
        nested(mtm, "Tab", tabs),
        opt_item("Word wrap", WRAP),
        keyed(mtm, "Hide Lines", sel!(hideLines:), 0, "h", opt | cmd, t),
        sep(),
        keyed(mtm, "Fold All", sel!(foldAll:), 0, "0", opt | cmd, t),
        keyed(
            mtm,
            "Unfold All",
            sel!(foldAll:),
            1,
            "0",
            ctrl | opt | cmd,
            t,
        ),
        keyed(
            mtm,
            "Fold Current Level",
            sel!(foldCurrent:),
            0,
            "f",
            ctrl | opt,
            t,
        ),
        keyed(
            mtm,
            "Unfold Current Level",
            sel!(foldCurrent:),
            1,
            "F",
            ctrl | opt,
            t,
        ),
        nested(mtm, "Fold Level", levels(sel!(foldLevel:), opt | cmd)),
        nested(
            mtm,
            "Unfold Level",
            levels(sel!(unfoldLevel:), ctrl | opt | cmd),
        ),
        sep(),
        item(mtm, "Summary...", sel!(summary:), "", t),
        sep(),
        item(mtm, "Folder as Workspace", sel!(toggleFolderAsWorkspace:), "", t),
        item(mtm, "Document Map", sel!(toggleDocMap:), "", t),
        item(mtm, "Document List", sel!(toggleDocList:), "", t),
        item(mtm, "Function List", sel!(toggleFunctionList:), "", t),
        sep(),
        item(mtm, "Monitoring (tail -f)", sel!(monitoring:), "", t),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles() {
        let o = Opts::default();
        assert!(checked(&o, CC) && checked(&o, GUIDES) && !checked(&o, ALL));
        let o = toggle(toggle(toggle(o, WS), EOL), NPC);
        assert!(checked(&o, ALL));
        let o = toggle(o, ALL);
        assert!(!o.on[WS] && !o.on[EOL] && !o.on[NPC] && !o.on[CC] && o.on[GUIDES]);
        let o = toggle(o, ALL);
        assert!(o.on[..ALL].iter().all(|&b| b));
        assert!(toggle(Opts::default(), WRAP).on[WRAP]);
    }

    #[test]
    fn npc_tables() {
        let cc = cc_chars();
        assert_eq!(cc.len(), 64);
        assert_eq!(cc[0], ("\0".to_string(), "NUL".to_string()));
        assert!(cc.contains(&("\u{85}".to_string(), "NEL".to_string())));
        assert_eq!(cc[63], ("\u{2029}".to_string(), "PS".to_string()));
        let npc = npc_chars();
        assert_eq!(npc.len(), 49);
        assert_eq!(npc[0], ("\u{a0}".to_string(), "NBSP".to_string()));
        assert!(npc.contains(&("\u{2004}".to_string(), "3/MSP".to_string())));
    }

    #[test]
    fn fold_properties() {
        let cpp = fold_props("cpp");
        assert!(cpp.contains(&("fold", "1")) && cpp.contains(&("fold.preprocessor", "1")));
        assert!(cpp.contains(&("fold.cpp.comment.explicit", "0")));
        let html = fold_props("php");
        assert!(html.contains(&("fold.html", "1")) && !html.contains(&("fold.comment", "1")));
        assert_eq!(fold_props("json"), [("fold", "1"), ("fold.compact", "0")]);
        assert!(fold_props("python").contains(&("fold.quotes.python", "1")));
        assert_eq!(
            fold_props("lua"),
            [("fold", "1"), ("fold.compact", "0"), ("fold.comment", "1")]
        );
        assert!(fold_props("normal").is_empty() && fold_props("tcl").is_empty());
        assert_eq!(fold_props("raku").len(), 5);
        assert!(
            !needs_fold_margin("normal") && !needs_fold_margin("batch") && needs_fold_margin("cpp")
        );
        assert!(python_style_indent("cpp") && !python_style_indent("lua"));
        assert!(indent_based("python") && !indent_based("cpp"));
        assert_eq!(model_tab_info("python"), Some(0x84));
        assert_eq!(model_tab_info("yaml"), Some(0x84));
        assert_eq!(model_tab_info("cpp"), None);
        assert_eq!(model_tab_info("normal"), None);
    }

    const H: isize = SC_FOLDLEVELHEADERFLAG;
    const B: isize = SC_FOLDLEVELBASE;

    #[test]
    fn fold_levels_by_number() {
        let l = [
            B | H,
            (B + 1) | H,
            B + 2,
            (B + 1) | H,
            B + 2,
            B + 1,
            B | H,
            B + 1,
        ];
        assert_eq!(level_headers(&l, 0, false), [0, 6]);
        assert_eq!(level_headers(&l, 1, false), [1, 3]);
        assert!(level_headers(&l, 2, false).is_empty());
    }

    #[test]
    fn fold_levels_by_indent() {
        let l = [
            B | H,
            (B + 4) | H,
            B + 8,
            (B + 4) | H,
            (B + 12) | H,
            B + 16,
            B | H,
            (B + 2) | H,
            B + 4,
        ];
        assert_eq!(level_headers(&l, 0, true), [0, 6]);
        assert_eq!(level_headers(&l, 1, true), [1, 3, 7]);
        assert_eq!(level_headers(&l, 2, true), [4]);
    }

    #[test]
    fn tab_targets() {
        assert_eq!(tab_target(0, 2, 5), Some(0));
        assert_eq!(tab_target(8, 0, 5), Some(4));
        assert_eq!(tab_target(3, 0, 1), None);
        assert_eq!(tab_target(NEXT, 4, 5), Some(0));
        assert_eq!(tab_target(PREV, 0, 5), Some(4));
        assert_eq!(tab_target(LAST, 0, 5), Some(4));
        assert_eq!(tab_target(FIRST, 3, 5), Some(0));
        assert_eq!(move_target(FORWARD, 4, 5), None);
        assert_eq!(move_target(FORWARD, 1, 5), Some(2));
        assert_eq!(move_target(BACKWARD, 0, 5), None);
        assert_eq!(move_target(TO_START, 3, 5), Some(0));
        assert_eq!(move_target(TO_END, 4, 5), None);
        assert_eq!(move_target(TO_END, 1, 5), Some(4));
    }

    #[test]
    fn counting() {
        assert_eq!(count_chars(b"ab\r\ncd"), 4);
        assert_eq!(count_chars("日本\né".as_bytes()), 3);
        assert_eq!(count_words(b"Hello, world!"), 2);
        assert_eq!(count_words(b"a-b c_d foo.bar(baz)"), 6);
        assert_eq!(count_words(b"it's \"x\"\r\n\ty"), 4);
        assert_eq!(count_words("日本語 テキスト a\u{a0}b".as_bytes()), 3);
        assert_eq!(count_words(b""), 0);
        assert_eq!(count_words(b" ;; "), 0);
    }

    #[test]
    fn summary_message() {
        let c = Counts {
            chars: 1234,
            words: 5,
            lines: 3,
            length: 1240,
            sel_chars: 0,
            sel_bytes: 0,
            ranges: 0,
        };
        let s = summary_text(
            Some(("/a/b.txt", "1/2/26 1:00:00 PM", "1/3/26 2:00:00 PM")),
            &c,
        );
        assert!(s.starts_with("Full file path: /a/b.txt\nCreated: 1/2/26 1:00:00 PM\nModified: "));
        assert!(s.contains("Characters (without line endings): 1,234\nWords: 5\nLines: 3\nDocument length: 1,240\n"));
        assert!(s.ends_with("0 selected characters (0 bytes) in 0 ranges"));
        assert!(summary_text(None, &c).starts_with("Characters"));
    }
}
