// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{app_support_dir, attr};
use crate::encoding::{self, Enc};
use crate::session::{read_config, read_file, save_config, write_file};
use crate::{cfg, item, ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSControl, NSControlTextEditingDelegate, NSFont, NSLineBreakMode,
    NSMenuItem,
    NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSScrollView, NSSlider, NSTabView,
    NSTabViewItem, NSTabViewType, NSTableColumn, NSTableView, NSTableViewDataSource,
    NSTableViewDelegate, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// A config.xml value: bool is yes/no, Show is show/hide (STR_BOOL_SHOWHIDE).
pub trait Val: Sized {
    fn put(&self) -> String;
    fn take(s: &str) -> Option<Self>;
}

impl Val for bool {
    fn put(&self) -> String {
        if *self { "yes" } else { "no" }.into()
    }
    fn take(s: &str) -> Option<Self> {
        match s.trim() {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Show(pub bool);

impl Val for Show {
    fn put(&self) -> String {
        if self.0 { "show" } else { "hide" }.into()
    }
    fn take(s: &str) -> Option<Self> {
        match s.trim() {
            "show" => Some(Show(true)),
            "hide" => Some(Show(false)),
            _ => None,
        }
    }
}

impl Val for i64 {
    fn put(&self) -> String {
        self.to_string()
    }
    fn take(s: &str) -> Option<Self> {
        s.trim().parse().ok()
    }
}

impl Val for String {
    fn put(&self) -> String {
        self.clone()
    }
    fn take(s: &str) -> Option<Self> {
        Some(s.to_string())
    }
}

// Each field: default, then the <GUIConfig name="..."> and its attribute ("" is the element text).
macro_rules! prefs {
    ($($f:ident: $t:ty = $d:expr => $g:literal $a:literal,)*) => {
        #[derive(Debug, Clone, PartialEq)]
        pub struct Prefs {
            $(pub $f: $t,)*
        }

        impl Default for Prefs {
            fn default() -> Self {
                Prefs { $($f: $d,)* }
            }
        }

        impl Prefs {
            pub const KEYS: &'static [(&'static str, &'static str)] = &[$(($g, $a),)*];

            pub fn get(&self, g: &str, a: &str) -> String {
                $(if ($g, $a) == (g, a) {
                    return self.$f.put();
                })*
                String::new()
            }

            pub fn set(&mut self, g: &str, a: &str, v: &str) {
                $(if ($g, $a) == (g, a) {
                    if let Some(x) = Val::take(v) {
                        self.$f = x;
                    }
                    return;
                })*
            }
        }
    };
}

const SVP: &str = "ScintillaPrimaryView";
const URI_SCHEMES: &str = "svn:// cvs:// git:// imap:// irc:// irc6:// ircs:// ldap:// ldaps:// news: telnet:// gopher:// ssh:// sftp:// smb:// skype: snmp:// spotify: steam:// sms: slack:// chrome:// bitcoin:";

// Defaults of NppGUI, ScintillaViewParams, NewDocDefaultSettings and MatchedPairConf in Parameters.h.
prefs! {
    current_line: i64 = 1 => "ScintillaPrimaryView" "currentLineIndicator",
    current_line_frame: i64 = 1 => "ScintillaPrimaryView" "currentLineFrameWidth",
    caret_width: i64 = 1 => "Caret" "width",
    caret_blink: i64 = 600 => "Caret" "blinkRate",
    line_wrap: String = "aligned".into() => "ScintillaPrimaryView" "lineWrapMethod",
    smooth_font: bool = false => "ScintillaPrimaryView" "smoothFont",
    virtual_space: bool = false => "ScintillaPrimaryView" "virtualSpace",
    copy_cut_line: bool = true => "ScintillaPrimaryView" "lineCopyCutWithoutSelection",
    scroll_beyond: bool = true => "ScintillaPrimaryView" "scrollBeyondLastLine",
    multi_selection: bool = true => "ScintillaPrimaryView" "multiSelection",
    column_to_multi: bool = true => "ScintillaPrimaryView" "columnSel2MultiEdit",
    folder_style: String = "box".into() => "ScintillaPrimaryView" "folderMarkStyle",
    edge_columns: String = String::new() => "ScintillaPrimaryView" "edgeMultiColumnPos",
    edge_bg: bool = false => "ScintillaPrimaryView" "isEdgeBgMode",
    change_history: i64 = 1 => "ScintillaPrimaryView" "isChangeHistoryEnabled",
    line_numbers: Show = Show(true) => "ScintillaPrimaryView" "lineNumberMargin",
    line_numbers_dynamic: bool = true => "ScintillaPrimaryView" "lineNumberDynamicWidth",
    padding_left: i64 = 0 => "ScintillaPrimaryView" "paddingLeft",
    padding_right: i64 = 0 => "ScintillaPrimaryView" "paddingRight",
    bookmark_margin: Show = Show(true) => "ScintillaPrimaryView" "bookMarkMargin",
    new_eol: i64 = 0 => "NewDocDefaultSettings" "format",
    new_encoding: i64 = 4 => "NewDocDefaultSettings" "encoding",
    new_lang: i64 = 0 => "NewDocDefaultSettings" "lang",
    new_codepage: i64 = -1 => "NewDocDefaultSettings" "codepage",
    open_ansi_as_utf8: bool = true => "NewDocDefaultSettings" "openAnsiAsUTF8",
    new_doc_on_startup: bool = false => "NewDocDefaultSettings" "addNewDocumentOnStartup",
    open_save_dir: i64 = 0 => "openSaveDir" "value",
    default_dir: String = String::new() => "openSaveDir" "defaultDirPath",
    last_used_dir: String = String::new() => "openSaveDir" "lastUsedDirPath",
    check_history_files: bool = false => "CheckHistoryFiles" "",
    tab_size: i64 = 4 => "TabSetting" "size",
    tab_replace: bool = false => "TabSetting" "replaceBySpace",
    backspace_unindent: bool = false => "TabSetting" "backspaceUnindent",
    mark_all_case: bool = false => "MarkAll" "matchCase",
    mark_all_word: bool = true => "MarkAll" "wholeWordOnly",
    tags_match: bool = true => "TagsMatchHighLight" "",
    tags_attrs: bool = true => "TagsMatchHighLight" "TagAttrHighLight",
    tags_non_html: bool = false => "TagsMatchHighLight" "HighLightNonHtmlZone",
    smart_hl: bool = true => "SmartHighLight" "",
    smart_hl_case: bool = false => "SmartHighLight" "matchCase",
    smart_hl_word: bool = true => "SmartHighLight" "wholeWordOnly",
    smart_hl_find: bool = false => "SmartHighLight" "useFindSettings",
    smart_hl_other_view: bool = false => "SmartHighLight" "onAnotherView",
    fill_find: bool = true => "Searching" "fillFindFieldWithSelected",
    fill_find_caret: bool = true => "Searching" "fillFindFieldSelectCaret",
    fill_find_max: i64 = 1024 => "Searching" "fillFindWhatThreshold",
    remember_session: bool = true => "RememberLastSession" "",
    snapshot_mode: bool = true => "Backup" "isSnapshotMode",
    snapshot_timing: i64 = 7000 => "Backup" "snapshotBackupTiming",
    backup_action: i64 = 0 => "Backup" "action",
    backup_use_dir: bool = false => "Backup" "useCustumDir",
    backup_dir: String = String::new() => "Backup" "dir",
    autoc_action: i64 = 3 => "auto-completion" "autoCAction",
    autoc_from: i64 = 1 => "auto-completion" "triggerFromNbChar",
    autoc_ignore_numbers: bool = true => "auto-completion" "autoCIgnoreNumbers",
    autoc_enter: bool = true => "auto-completion" "insertSelectedItemUseENTER",
    autoc_tab: bool = true => "auto-completion" "insertSelectedItemUseTAB",
    autoc_brief: bool = false => "auto-completion" "autoCBrief",
    func_params: bool = true => "auto-completion" "funcParams",
    insert_parentheses: bool = false => "auto-insert" "parentheses",
    insert_brackets: bool = false => "auto-insert" "brackets",
    insert_curly: bool = false => "auto-insert" "curlyBrackets",
    insert_quotes: bool = false => "auto-insert" "quotes",
    insert_double_quotes: bool = false => "auto-insert" "doubleQuotes",
    insert_tag: bool = false => "auto-insert" "htmlXmlTag",
    url_style: i64 = 2 => "URL" "",
    uri_schemes: String = URI_SCHEMES.into() => "uriCustomizedSchemes" "",
    search_engine: i64 = 2 => "searchEngine" "searchEngineChoice",
    search_engine_custom: String = String::new() => "searchEngine" "searchEngineCustom",
    auto_detect: String = "yes".into() => "Auto-detection" "",
    date_time_format: String = "yyyy-MM-dd HH:mm:ss".into() => "insertDateTime" "customizedFormat",
    date_time_reverse: bool = false => "insertDateTime" "reverseDefaultOrder",
}

// NppGUI::AutocStatus.
pub const AUTOC_NONE: i64 = 0;
#[allow(dead_code)]
pub const AUTOC_FUNC: i64 = 1;
#[allow(dead_code)]
pub const AUTOC_WORD: i64 = 2;
pub const AUTOC_BOTH: i64 = 3;

// NppConstants.h ChangeDetect.
const CD_ENABLED_OLD: u8 = 1;
const CD_ENABLED_NEW: u8 = 2;
const CD_AUTO_UPDATE: u8 = 4;
const CD_GO2END: u8 = 8;
const DETECT_NAMES: [(&str, u8); 9] = [
    ("no", 0),
    ("yes", CD_ENABLED_NEW),
    ("auto", CD_ENABLED_NEW | CD_AUTO_UPDATE),
    ("Update2End", CD_ENABLED_NEW | CD_GO2END),
    (
        "autoUpdate2End",
        CD_ENABLED_NEW | CD_AUTO_UPDATE | CD_GO2END,
    ),
    ("yesOld", CD_ENABLED_OLD),
    ("autoOld", CD_ENABLED_OLD | CD_AUTO_UPDATE),
    ("Update2EndOld", CD_ENABLED_OLD | CD_GO2END),
    (
        "autoUpdate2EndOld",
        CD_ENABLED_OLD | CD_AUTO_UPDATE | CD_GO2END,
    ),
];

// Parameters.cpp feedGUIParameters "Auto-detection": no text keeps cdEnabledNew, an unknown text is cdDisabled.
pub fn detect_bits(s: &str) -> u8 {
    if s.trim().is_empty() {
        return CD_ENABLED_NEW;
    }
    DETECT_NAMES
        .iter()
        .find(|(n, _)| *n == s.trim())
        .map_or(0, |(_, b)| *b)
}

// Parameters.cpp createXmlTreeFromGUIParams "Auto-detection".
pub fn detect_name(bits: u8) -> &'static str {
    let b = if bits & CD_ENABLED_OLD != 0 {
        bits & !CD_ENABLED_NEW
    } else if bits & CD_ENABLED_NEW != 0 {
        bits
    } else {
        0
    };
    DETECT_NAMES
        .iter()
        .find(|(_, x)| *x == b)
        .map_or("no", |(n, _)| n)
}

// File Status Auto-Detection, as Preferences > MISC. shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoDetect {
    pub enabled: bool,
    pub all_files: bool,
    pub silent: bool,
    pub go_to_end: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub struct SmartHighlight {
    pub enabled: bool,
    pub match_case: bool,
    pub whole_word: bool,
    pub use_find_settings: bool,
    pub another_view: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub struct TagsMatch {
    pub enabled: bool,
    pub attributes: bool,
    pub non_html_zone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub struct AutoCompletion {
    pub action: i64,
    pub from_len: usize,
    pub ignore_numbers: bool,
    pub use_enter: bool,
    pub use_tab: bool,
    pub brief: bool,
    pub func_params: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub struct AutoInsert {
    pub parentheses: bool,
    pub brackets: bool,
    pub curly_brackets: bool,
    pub quotes: bool,
    pub double_quotes: bool,
    pub html_xml_tag: bool,
}

#[allow(dead_code)]
impl Prefs {
    pub fn smart_highlight(&self) -> SmartHighlight {
        SmartHighlight {
            enabled: self.smart_hl,
            match_case: self.smart_hl_case,
            whole_word: self.smart_hl_word,
            use_find_settings: self.smart_hl_find,
            another_view: self.smart_hl_other_view,
        }
    }

    // Style All Occurrences of Token: (match case, match whole word only).
    pub fn mark_all(&self) -> (bool, bool) {
        (self.mark_all_case, self.mark_all_word)
    }

    pub fn tags_match(&self) -> TagsMatch {
        TagsMatch {
            enabled: self.tags_match,
            attributes: self.tags_attrs,
            non_html_zone: self.tags_non_html,
        }
    }

    pub fn auto_completion(&self) -> AutoCompletion {
        AutoCompletion {
            action: self.autoc_action.clamp(AUTOC_NONE, AUTOC_BOTH),
            from_len: self.autoc_from.clamp(1, 9) as usize,
            ignore_numbers: self.autoc_ignore_numbers,
            use_enter: self.autoc_enter,
            use_tab: self.autoc_tab,
            brief: self.autoc_brief,
            func_params: self.func_params,
        }
    }

    pub fn auto_insert(&self) -> AutoInsert {
        AutoInsert {
            parentheses: self.insert_parentheses,
            brackets: self.insert_brackets,
            curly_brackets: self.insert_curly,
            quotes: self.insert_quotes,
            double_quotes: self.insert_double_quotes,
            html_xml_tag: self.insert_tag,
        }
    }

    pub fn clickable_links(&self) -> bool {
        self.url_style != 0
    }

    pub fn auto_detect(&self) -> AutoDetect {
        let b = detect_bits(&self.auto_detect);
        AutoDetect {
            enabled: b & (CD_ENABLED_OLD | CD_ENABLED_NEW) != 0,
            all_files: b & CD_ENABLED_OLD != 0,
            silent: b & CD_AUTO_UPDATE != 0,
            go_to_end: b & CD_GO2END != 0,
        }
    }

    // NppCommands.cpp IDM_EDIT_SEARCHONINTERNET: the URL with $(CURRENT_WORD).
    pub fn search_engine_url(&self) -> String {
        let google = "https://www.google.com/search?q=$(CURRENT_WORD)";
        match self.search_engine {
            0 => {
                let u: String = self
                    .search_engine_custom
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                if u.starts_with("http://") || u.starts_with("https://") {
                    u
                } else {
                    google.into()
                }
            }
            1 | 3 => "https://duckduckgo.com/?q=$(CURRENT_WORD)".into(),
            4 => "https://search.yahoo.com/search?q=$(CURRENT_WORD)".into(),
            5 => "https://stackoverflow.com/search?q=$(CURRENT_WORD)".into(),
            _ => google.into(),
        }
    }

    // Vertical edge columns; Notepad++ ignores columns above 8192.
    pub fn edges(&self) -> Vec<usize> {
        edge_list(&self.edge_columns)
    }

    // SCI_SETCHANGEHISTORY flags of isChangeHistoryEnabled: 1 margin, 2 text, 3 both.
    pub fn change_history_flags(&self) -> usize {
        let m = self.change_history.clamp(0, 3) as usize;
        if m == 0 {
            0
        } else {
            1 | (m & 1) << 1 | (m & 2) << 1
        }
    }

    // ScintillaEditView::setWrapMode: SC_WRAPINDENT_FIXED, SAME or INDENT.
    pub fn wrap_indent(&self) -> usize {
        match self.line_wrap.as_str() {
            "default" => 0,
            "indent" => 2,
            _ => 1,
        }
    }

    // EolType of NewDocDefaultSettings as a Scintilla EOL mode; both use 0 CR LF, 1 CR, 2 LF.
    pub fn new_doc_eol(&self) -> usize {
        self.new_eol.clamp(0, 2) as usize
    }

    // UniMode and codepage of NewDocDefaultSettings.
    pub fn new_doc_enc(&self) -> Enc {
        if let Ok(cp) = u32::try_from(self.new_codepage) {
            if encoding::supported(cp) {
                return Enc::Cp(cp);
            }
        }
        match self.new_encoding {
            0 => Enc::Ansi,
            1 => Enc::Utf8Bom,
            2 => Enc::Utf16Be,
            3 => Enc::Utf16Le,
            _ => Enc::Utf8,
        }
    }
}

pub fn edge_list(s: &str) -> Vec<usize> {
    s.split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse().ok())
        .filter(|&n| n <= 8192)
        .collect()
}

// A config.xml element: the value of its name attribute, attributes to set, and its new text.
#[derive(Debug, Clone, PartialEq)]
pub struct Elem {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: Option<String>,
}

fn same_path(stack: &[String], path: &[&str]) -> bool {
    stack.len() == path.len() && stack.iter().zip(path).all(|(a, b)| a == b)
}

// The <tag name="..."> elements directly inside `path`, with all their attributes and their text.
pub fn read_elems(xml: &str, path: &[&str], tag: &str) -> Vec<Elem> {
    let mut r = Reader::from_str(xml);
    let (mut stack, mut out): (Vec<String>, Vec<Elem>) = (vec![], vec![]);
    let mut open: Option<Elem> = None;
    let elem = |e: &BytesStart| Elem {
        name: attr(e, "name"),
        attrs: e
            .attributes()
            .flatten()
            .map(|a| a.key.as_ref().to_string())
            .map(|k| (k.clone(), attr(e, &k)))
            .collect(),
        text: Some(String::new()),
    };
    loop {
        let ev = match r.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(ev) => ev,
        };
        let here = open.is_none() && same_path(&stack, path);
        match ev {
            Event::Empty(e) if here && e.name().as_ref() == tag => out.push(elem(&e)),
            Event::Start(e) => {
                if here && e.name().as_ref() == tag {
                    open = Some(elem(&e));
                }
                stack.push(e.name().as_ref().to_string());
            }
            Event::Text(t) => {
                if let Some(x) = open.as_mut().and_then(|o| o.text.as_mut()) {
                    x.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(g) => {
                if let Some(x) = open.as_mut().and_then(|o| o.text.as_mut()) {
                    x.push_str(quick_xml::escape::resolve_predefined_entity(&g).unwrap_or(""));
                }
            }
            Event::End(_) => {
                stack.pop();
                if same_path(&stack, path) {
                    out.extend(open.take());
                }
            }
            _ => {}
        }
    }
    out
}

// The start tag `e` with the attributes of `el` changed or added; the other attributes stay.
fn patched(e: &BytesStart, el: &Elem) -> BytesStart<'static> {
    let mut b = BytesStart::new(e.name().as_ref().to_string());
    let mut done = vec![];
    for a in e.attributes().flatten() {
        let k = a.key.as_ref().to_string();
        match el.attrs.iter().find(|(x, _)| *x == k) {
            Some((_, v)) => {
                b.push_attribute((k.as_str(), v.as_str()));
                done.push(k);
            }
            None => b.push_attribute(a),
        }
    }
    for (k, v) in el.attrs.iter().filter(|(k, _)| !done.contains(k)) {
        b.push_attribute((k.as_str(), v.as_str()));
    }
    b
}

// An empty element written as Notepad++ (TinyXML) writes it: a space before "/>".
fn spaced(b: BytesStart) -> BytesStart<'static> {
    let n = b.name().as_ref().len();
    let content = b.trim_end().to_string() + " ";
    BytesStart::from_content(content, n)
}

fn write_elem(w: &mut Writer<Vec<u8>>, tag: &str, el: &Elem) -> std::io::Result<()> {
    let mut b = BytesStart::new(tag);
    b.push_attribute(("name", el.name.as_str()));
    for (k, v) in &el.attrs {
        b.push_attribute((k.as_str(), v.as_str()));
    }
    match &el.text {
        Some(t) => {
            w.write_event(Event::Start(b))?;
            w.write_event(Event::Text(BytesText::new(t)))?;
            w.write_event(Event::End(BytesEnd::new(tag)))
        }
        None => w.write_event(Event::Empty(spaced(b))),
    }
}

// Writes the elements of `elems` that are not in `seen`, one on each line with `indent`.
fn write_missing(
    w: &mut Writer<Vec<u8>>,
    tag: &str,
    elems: &[Elem],
    seen: &[String],
    indent: &str,
    end: &str,
) -> std::io::Result<()> {
    use std::io::Write as _;
    for el in elems.iter().filter(|e| !seen.contains(&e.name)) {
        w.get_mut().write_all(indent.as_bytes())?;
        write_elem(w, tag, el)?;
        w.get_mut().write_all(end.as_bytes())?;
    }
    Ok(())
}

// The document with each element of `elems` changed or added in `path`; all other content stays as it is.
pub fn patch(
    src: Option<&str>,
    path: &[&str],
    tag: &str,
    elems: &[Elem],
) -> Result<String, String> {
    use std::io::Write as _;
    let err = |e: std::io::Error| e.to_string();
    let ind = |n: usize| "    ".repeat(n);
    let mut w = Writer::new(Vec::new());
    let Some(src) = src.filter(|s| s.contains(&format!("<{}", path[0]))) else {
        w.get_mut()
            .write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n")
            .map_err(err)?;
        for (k, p) in path.iter().enumerate() {
            w.get_mut()
                .write_all(format!("{}<{p}>\r\n", ind(k)).as_bytes())
                .map_err(err)?;
        }
        write_missing(&mut w, tag, elems, &[], &ind(path.len()), "\r\n").map_err(err)?;
        for (k, p) in path.iter().enumerate().rev() {
            w.get_mut()
                .write_all(format!("{}</{p}>\r\n", ind(k)).as_bytes())
                .map_err(err)?;
        }
        return String::from_utf8(w.into_inner()).map_err(|e| e.to_string());
    };
    let mut r = Reader::from_str(src);
    let mut stack: Vec<String> = vec![];
    let mut seen: Vec<String> = vec![];
    let (mut container, mut skip): (bool, Option<usize>) = (false, None);
    loop {
        let ev = r.read_event().map_err(|e| e.to_string())?;
        if let Some(d) = skip {
            skip = match ev {
                Event::Start(_) => Some(d + 1),
                Event::End(_) if d == 0 => None,
                Event::End(_) => Some(d - 1),
                Event::Eof => return Err("unexpected end of the file".into()),
                _ => Some(d),
            };
            continue;
        }
        let name = |e: &BytesStart| e.name().as_ref().to_string();
        match &ev {
            Event::Start(e) | Event::Empty(e)
                if same_path(&stack, path) && e.name().as_ref() == tag =>
            {
                if let Some(el) = elems.iter().find(|x| x.name == attr(e, "name")) {
                    seen.push(el.name.clone());
                    let b = patched(e, el);
                    let start = matches!(ev, Event::Start(_));
                    match &el.text {
                        Some(t) => {
                            w.write_event(Event::Start(b)).map_err(err)?;
                            w.write_event(Event::Text(BytesText::new(t))).map_err(err)?;
                            w.write_event(Event::End(BytesEnd::new(tag))).map_err(err)?;
                            skip = start.then_some(0);
                        }
                        None if start => {
                            w.write_event(Event::Start(b)).map_err(err)?;
                            stack.push(tag.into());
                        }
                        None => w.write_event(Event::Empty(spaced(b))).map_err(err)?,
                    }
                    continue;
                }
                if matches!(ev, Event::Start(_)) {
                    stack.push(tag.into());
                }
            }
            Event::Empty(e) if same_path(&[stack.clone(), vec![name(e)]].concat(), path) => {
                container = true;
                w.write_event(Event::Start(e.borrow())).map_err(err)?;
                w.get_mut().write_all(b"\r\n").map_err(err)?;
                write_missing(&mut w, tag, elems, &seen, &ind(path.len()), "\r\n").map_err(err)?;
                w.get_mut()
                    .write_all(ind(path.len() - 1).as_bytes())
                    .map_err(err)?;
                w.write_event(Event::End(e.to_end())).map_err(err)?;
                continue;
            }
            Event::Start(e) => {
                stack.push(name(e));
                container |= same_path(&stack, path);
            }
            Event::End(_) => {
                if same_path(&stack, path) {
                    let end = format!("\r\n{}", ind(path.len() - 1));
                    write_missing(&mut w, tag, elems, &seen, &ind(1), &end).map_err(err)?;
                } else if !container && same_path(&stack, &path[..path.len() - 1]) {
                    let k = path.len() - 1;
                    let open = format!("{}<{}>\r\n", ind(k), path[k]);
                    w.get_mut().write_all(open.as_bytes()).map_err(err)?;
                    write_missing(&mut w, tag, elems, &seen, &ind(k + 1), "\r\n").map_err(err)?;
                    let close = format!("{}</{}>\r\n", ind(k), path[k]);
                    w.get_mut().write_all(close.as_bytes()).map_err(err)?;
                    container = true;
                }
                stack.pop();
            }
            Event::Eof if stack.is_empty() => break,
            Event::Eof => return Err("unexpected end of the file".into()),
            _ => {}
        }
        w.write_event(ev).map_err(err)?;
    }
    let out = String::from_utf8(w.into_inner()).map_err(|e| e.to_string())?;
    Ok(keep_bom(src, out))
}

// A UTF-8 BOM at the start of the old file stays.
pub fn keep_bom(old: &str, new: String) -> String {
    if old.starts_with('\u{FEFF}') && !new.starts_with('\u{FEFF}') {
        format!("\u{FEFF}{new}")
    } else {
        new
    }
}

pub const GUI_PATH: [&str; 2] = ["NotepadPlus", "GUIConfigs"];
const LANGS_PATH: [&str; 2] = ["NotepadPlus", "Languages"];

// Parameters.cpp feedGUIParameters and feedScintillaParam for the settings of this app; an empty text keeps the default.
pub fn from_config(xml: &str) -> Prefs {
    let elems = read_elems(xml, &GUI_PATH, "GUIConfig");
    let mut p = Prefs::default();
    for (g, a) in Prefs::KEYS {
        let Some(e) = elems.iter().find(|e| e.name == *g) else {
            continue;
        };
        let v = if a.is_empty() {
            e.text.clone().filter(|t| !t.trim().is_empty())
        } else {
            e.attrs.iter().find(|(k, _)| k == a).map(|(_, v)| v.clone())
        };
        if let Some(v) = v {
            p.set(g, a, &v);
        }
    }
    p
}

// The GUIConfig elements of the settings, in the order of Prefs::KEYS.
pub fn config_elems(p: &Prefs) -> Vec<Elem> {
    let mut out: Vec<Elem> = vec![];
    for (g, a) in Prefs::KEYS {
        if !out.iter().any(|e| e.name == *g) {
            out.push(Elem {
                name: g.to_string(),
                attrs: vec![],
                text: None,
            });
        }
        let Some(e) = out.iter_mut().find(|e| e.name == *g) else {
            continue;
        };
        if a.is_empty() {
            e.text = Some(p.get(g, a));
        } else {
            e.attrs.push((a.to_string(), p.get(g, a)));
        }
    }
    out
}

// config.xml with the GUIConfig elements of the current settings.
pub fn patch_config(src: Option<&str>) -> Result<String, String> {
    patch(src, &GUI_PATH, "GUIConfig", &config_elems(&get()))
}

#[derive(Default)]
struct State {
    prefs: Option<Prefs>,
    langs: Option<Vec<Elem>>,
    langs_error: Option<String>,
}

thread_local! {
    static S: RefCell<State> = RefCell::default();
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn load() -> Prefs {
    read_config().map_or_else(Prefs::default, |x| from_config(&x))
}

// Runs `f` with the settings, read from config.xml at the first use; `f` must not change them.
pub fn with<R>(f: impl FnOnce(&Prefs) -> R) -> R {
    S.with(|s| {
        if s.borrow().prefs.is_none() {
            let p = load();
            s.borrow_mut().prefs = Some(p);
        }
        match s.borrow().prefs.as_ref() {
            Some(p) => f(p),
            None => f(&Prefs::default()),
        }
    })
}

// A copy of the settings when they are loaded and not borrowed; for the panic hook.
pub fn try_get() -> Option<Prefs> {
    S.try_with(|s| s.try_borrow().ok()?.prefs.clone())
        .ok()
        .flatten()
}

// A copy of the settings.
pub fn get() -> Prefs {
    with(Prefs::clone)
}

fn update(f: impl FnOnce(&mut Prefs)) {
    S.with(|s| f(s.borrow_mut().prefs.get_or_insert_with(load)));
}

fn langs_path() -> Option<PathBuf> {
    app_support_dir().map(|d| d.join("langs.xml"))
}

// The tab settings of one language in the user langs.xml, where Notepad++ keeps them.
fn user_lang_tab(lang: &str) -> Option<(i64, bool)> {
    S.with(|s| {
        if s.borrow().langs.is_none() {
            let r = langs_path().map_or(Ok(None), |p| read_file(&p));
            let x = r.as_ref().ok().cloned().flatten().unwrap_or_default();
            let mut s = s.borrow_mut();
            s.langs_error = r.err();
            s.langs = Some(read_elems(&x, &LANGS_PATH, "Language"));
        }
        let s = s.borrow();
        let e = s.langs.as_ref()?.iter().find(|e| e.name == lang)?;
        let get = |k: &str| e.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
        Some((
            get("tabSettings").and_then(|v| v.trim().parse().ok()).unwrap_or(-1),
            get("backspaceUnindent") == Some("yes"),
        ))
    })
}

// Lang::getTabInfo and _isBackspaceUnindent of a language: the user langs.xml, else langs.model.xml.
pub fn lang_tab(lang: &str) -> (i64, bool) {
    user_lang_tab(lang)
        .unwrap_or_else(|| (crate::view::model_tab_info(lang).map_or(-1, |i| i as i64), false))
}

fn lang_default(info: i64) -> bool {
    info == -1 || info & 0x7F == 0
}

// ScintillaEditView::setTabSettings: (tab width, use tabs, backspace unindents).
pub fn tab_settings(lang: &str) -> (usize, bool, bool) {
    let (info, bs) = lang_tab(lang);
    if !lang_default(info) {
        return ((info & 0x7F) as usize, info & 0x80 == 0, bs);
    }
    with(|p| {
        let size = if p.tab_size > 0 { p.tab_size as usize } else { 4 };
        (size, !p.tab_replace, p.backspace_unindent)
    })
}

// NppParameters::insertTabInfo: writes the tab settings of a language to langs.xml.
fn set_lang_tab(lang: &str, info: i64, bs: bool) -> Result<(), String> {
    let el = Elem {
        name: lang.into(),
        attrs: vec![
            ("tabSettings".into(), info.to_string()),
            ("backspaceUnindent".into(), bs.put()),
        ],
        text: None,
    };
    S.with(|s| {
        let mut s = s.borrow_mut();
        let list = s.langs.get_or_insert_with(Vec::new);
        list.retain(|e| e.name != lang);
        list.push(el.clone());
    });
    if let Some(e) = S.with(|s| s.borrow().langs_error.clone()) {
        return Err(e);
    }
    let Some(path) = langs_path() else {
        return Ok(());
    };
    // Parameters.cpp load: a missing langs.xml starts as a copy of langs.model.xml.
    read_file(&path)
        .map(|old| old.unwrap_or_else(|| crate::view::LANGS.to_string()))
        .and_then(|old| patch(Some(&old), &LANGS_PATH, "Language", &[el]))
        .and_then(|x| write_file(&path, &x, false))
}

const NAME_INFO: &str =
    include_str!("../../PowerEditor/src/ScintillaComponent/ScintillaEditView.cpp");

// ScintillaEditView::_langNameInfoArray in LangType order: (language name, short name).
pub fn lang_types() -> &'static [(String, String)] {
    static T: OnceLock<Vec<(String, String)>> = OnceLock::new();
    T.get_or_init(|| {
        let block = NAME_INFO
            .split("_langNameInfoArray[L_EXTERNAL + 1] = {")
            .nth(1)
            .unwrap_or("");
        block[..block.find("};").unwrap_or(0)]
            .lines()
            .filter_map(|l| l.trim().strip_prefix("{L\""))
            .map(|l| {
                let f: Vec<&str> = l.split('"').step_by(2).collect();
                let at = |k: usize| f.get(k).unwrap_or(&"").to_string();
                (at(0), at(1))
            })
            .collect()
    })
}

// The language of a new document (NewDocDefaultSettings lang); None for normal text.
pub fn new_doc_language() -> Option<&'static crate::config::Language> {
    let i = usize::try_from(get().new_lang).ok().filter(|&i| i > 0)?;
    let name = &lang_types().get(i)?.0;
    cfg().languages.iter().find(|l| &l.name == name)
}

// FileManager::resolveLoadedEncoding: 7-bit text opens as UTF-8 only with "Apply to opened ANSI files".
pub fn opened_encoding(e: Enc, bytes: &[u8]) -> Enc {
    if e == Enc::Utf8 && bytes.is_ascii() && !get().open_ansi_as_utf8 {
        Enc::Ansi
    } else {
        e
    }
}

// The folder for an Open or Save dialog (CustomFileDialog and NppParameters::setWorkingDir).
pub fn dialog_dir(current: Option<&Path>) -> Option<PathBuf> {
    let p = get();
    let dir = match p.open_save_dir {
        1 => PathBuf::from(&p.last_used_dir),
        2 => PathBuf::from(&p.default_dir),
        _ => current?.parent()?.to_path_buf(),
    };
    dir.is_dir().then_some(dir)
}

// With "Remember last used directory", the folder of a file chosen in a dialog is kept.
pub fn used_file(file: &Path) {
    if get().open_save_dir == 1 {
        if let Some(d) = file.parent() {
            update(|p| p.last_used_dir = d.to_string_lossy().into_owned());
        }
    }
}

pub const MARGIN_LINE_NUMBER: usize = 0;
const SCI_GETFIRSTVISIBLELINE: u32 = 2152;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_LINESONSCREEN: u32 = 2370;
const SCI_DOCLINEFROMVISIBLE: u32 = 2221;
const SCI_TEXTWIDTH: u32 = 2276;
const SCI_SETMARGINWIDTHN: u32 = 2242;
const STYLE_LINENUMBER: usize = 33;

fn digits(n: isize) -> usize {
    n.max(0).to_string().len()
}

// ScintillaEditView::updateLineNumberWidth.
pub fn line_number_width(v: &NSView) {
    let (show, dynamic) = with(|p| (p.line_numbers.0, p.line_numbers_dynamic));
    let w = if !show {
        0
    } else {
        let s = |m, w: usize| sci::send(v, m, w, 0);
        let n = if dynamic {
            let on_screen = s(SCI_LINESONSCREEN, 0);
            let last = s(
                SCI_DOCLINEFROMVISIBLE,
                (on_screen + s(SCI_GETFIRSTVISIBLELINE, 0) + 1) as usize,
            );
            digits(last).max(3)
        } else {
            digits(s(SCI_GETLINECOUNT, 0)).max(4)
        };
        8 + n as isize * sci::send(v, SCI_TEXTWIDTH, STYLE_LINENUMBER, c"8".as_ptr() as isize)
    };
    sci::send(v, SCI_SETMARGINWIDTHN, MARGIN_LINE_NUMBER, w);
}

// Applies the editor settings to one editor, as Notepad++ does at start and from the Preferences dialog.
pub fn apply_editor(v: &NSView, lang: &str, c: &crate::config::Config) {
    const SCI_SETCARETPERIOD: u32 = 2076;
    const SCI_SETCARETLINEVISIBLE: u32 = 2096;
    const SCI_SETCARETWIDTH: u32 = 2188;
    const SCI_SETEDGECOLUMN: u32 = 2361;
    const SCI_SETEDGEMODE: u32 = 2363;
    const SCI_SETMARGINLEFT: u32 = 2155;
    const SCI_SETMARGINRIGHT: u32 = 2157;
    const SCI_SETENDATLASTLINE: u32 = 2277;
    const SCI_SETMULTIPLESELECTION: u32 = 2563;
    const SCI_SETVIRTUALSPACEOPTIONS: u32 = 2596;
    const SCI_SETFONTQUALITY: u32 = 2611;
    const SCI_SETCARETSTYLE: u32 = 2512;
    const SCI_SETCARETLINEFRAME: u32 = 2704;
    const SCI_MULTIEDGEADDLINE: u32 = 2694;
    const SCI_MULTIEDGECLEARALL: u32 = 2695;
    const SCI_GETCHANGEHISTORY: u32 = 2781;
    const SCI_SETCHANGEHISTORY: u32 = 2780;
    const CARETSTYLE_LINE: usize = 1;
    const CARETSTYLE_BLOCK: usize = 2;
    const CARETSTYLE_BLOCK_AFTER: usize = 0x100;
    const SCVS_RECTANGULARSELECTION: usize = 1;
    const SCVS_USERACCESSIBLE: usize = 2;
    const SCVS_NOWRAPLINESTART: usize = 4;
    const SC_EFF_QUALITY_LCD_OPTIMIZED: usize = 3;
    const EDGE_BACKGROUND: usize = 2;
    const EDGE_MULTILINE: usize = 3;
    let p = get();
    let s = |m, w: usize, l: isize| sci::send(v, m, w, l);
    match p.caret_width {
        4 => {
            s(SCI_SETCARETWIDTH, 1, 0);
            s(SCI_SETCARETSTYLE, CARETSTYLE_BLOCK, 0);
        }
        5 => {
            s(SCI_SETCARETWIDTH, 1, 0);
            s(
                SCI_SETCARETSTYLE,
                CARETSTYLE_BLOCK | CARETSTYLE_BLOCK_AFTER,
                0,
            );
        }
        w => {
            s(SCI_SETCARETSTYLE, CARETSTYLE_LINE, 0);
            s(SCI_SETCARETWIDTH, w.clamp(0, 3) as usize, 0);
        }
    }
    s(SCI_SETCARETPERIOD, p.caret_blink.max(0) as usize, 0);
    s(SCI_SETCARETLINEVISIBLE, (p.current_line != 0) as usize, 0);
    let frame = if p.current_line == 2 {
        p.current_line_frame.clamp(1, 6)
    } else {
        0
    };
    s(SCI_SETCARETLINEFRAME, frame as usize, 0);
    s(SCI_SETMULTIPLESELECTION, p.multi_selection as usize, 0);
    let vs = if p.virtual_space {
        SCVS_RECTANGULARSELECTION | SCVS_USERACCESSIBLE | SCVS_NOWRAPLINESTART
    } else {
        SCVS_RECTANGULARSELECTION
    };
    s(SCI_SETVIRTUALSPACEOPTIONS, vs, 0);
    s(SCI_SETENDATLASTLINE, !p.scroll_beyond as usize, 0);
    s(
        SCI_SETFONTQUALITY,
        if p.smooth_font {
            SC_EFF_QUALITY_LCD_OPTIMIZED
        } else {
            0
        },
        0,
    );
    // NppBigSwitch.cpp NPPM_INTERNAL_EDGEMULTISETSIZE.
    s(SCI_MULTIEDGECLEARALL, 0, 0);
    let colour = c
        .global_styles
        .iter()
        .find(|st| st.name == "Edge colour")
        .and_then(|st| st.fg)
        .unwrap_or(0xC0C0C0);
    let edges = p.edges();
    edges.iter().for_each(|&c| {
        s(SCI_MULTIEDGEADDLINE, c, colour);
    });
    let mode = match edges.len() {
        0 => 0,
        1 if p.edge_bg => {
            s(SCI_SETEDGECOLUMN, edges.first().copied().unwrap_or(0), 0);
            EDGE_BACKGROUND
        }
        _ => EDGE_MULTILINE,
    };
    s(SCI_SETEDGEMODE, mode, 0);
    s(SCI_SETMARGINLEFT, 0, p.padding_left.clamp(0, 30) as isize);
    s(SCI_SETMARGINRIGHT, 0, p.padding_right.clamp(0, 30) as isize);
    line_number_width(v);
    s(
        SCI_SETMARGINWIDTHN,
        crate::search_extras::BOOKMARK_MARGIN,
        bookmark_width(),
    );
    s(
        SCI_SETMARGINWIDTHN,
        crate::search_extras::CHANGE_MARGIN,
        change_margin_width(),
    );
    // Change history starts only with an empty undo history, so a change of an enabled history is applied.
    if s(SCI_GETCHANGEHISTORY, 0, 0) & 1 != 0 {
        s(SCI_SETCHANGEHISTORY, p.change_history_flags(), 0);
    }
    sci::setup_fold(v, c, lang);
    sci::setup_tabs(v, lang);
}

pub fn bookmark_width() -> isize {
    if with(|p| p.bookmark_margin.0) {
        16
    } else {
        0
    }
}

pub fn change_margin_width() -> isize {
    if with(|p| p.change_history_flags() & 2 != 0) {
        9
    } else {
        0
    }
}

// Preferences pages that apply on macOS, in the Notepad++ order (preferenceDlg.cpp).
pub const PAGES: [&str; 16] = [
    "General",
    "Editing 1",
    "Editing 2",
    "Margins/Border/Edge",
    "New Document",
    "Default Directory",
    "Recent Files History",
    "Indentation",
    "Highlighting",
    "Searching",
    "Backup",
    "Auto-Completion",
    "Multi-Instance & Date",
    "Cloud & Link",
    "Search Engine",
    "MISC.",
];

// Settings menu; the Style Configurator item goes after Preferences..., as in Notepad_plus.rc.
pub fn settings_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    vec![
        item(mtm, "Preferences...", sel!(showPreferences:), ",", t),
        crate::style_dlg::configurator_item(mtm, t),
        NSMenuItem::separatorItem(mtm),
        crate::style_dlg::import_menu(mtm, t),
    ]
}

define_class!(
    // A view with the origin at the top left, for the Preferences page layout.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NppPrefsFlipped"]
    struct Flipped;

    impl Flipped {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

define_class!(
    // The page list of the Preferences window; a selection shows its page.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NppPrefsList"]
    struct PrefsList;

    unsafe impl NSObjectProtocol for PrefsList {}

    unsafe impl NSTableViewDataSource for PrefsList {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _t: &NSTableView) -> isize {
            PAGES.len() as isize
        }

        #[unsafe(method_id(tableView:objectValueForTableColumn:row:))]
        fn value(&self, _t: &NSTableView, _c: Option<&NSTableColumn>, row: isize) -> Option<Retained<AnyObject>> {
            let name = usize::try_from(row).ok().and_then(|r| PAGES.get(r));
            name.map(|n| Retained::into_super(Retained::into_super(ns(n))))
        }
    }

    unsafe impl NSControlTextEditingDelegate for PrefsList {}

    unsafe impl NSTableViewDelegate for PrefsList {
        #[unsafe(method(tableViewSelectionDidChange:))]
        fn changed(&self, n: &NSNotification) {
            let Some(t) = n.object().and_then(|o| o.downcast::<NSTableView>().ok()) else {
                return;
            };
            let row = t.selectedRow();
            if let Some(p) = UI.with(|u| u.borrow().as_ref().map(|u| u.pages.clone())) {
                if row >= 0 && (row as usize) < PAGES.len() {
                    p.selectTabViewItemAtIndex(row);
                }
            }
        }
    }
);

fn flipped(mtm: MainThreadMarker) -> Retained<NSView> {
    let v: Retained<Flipped> = unsafe { msg_send![Flipped::alloc(mtm), init] };
    v.into_super()
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

#[derive(Clone)]
enum Bind {
    Check(&'static str, &'static str),
    Bit(&'static str, &'static str, i64),
    Radio(&'static str, &'static str, &'static str),
    Num(&'static str, &'static str, i64, i64),
    Text(&'static str, &'static str),
    Popup(&'static str, &'static str, Vec<String>),
    Slider(&'static str, &'static str),
    Blink,
    NoHistoryCheck,
    EdgeText,
    AutocOn,
    UrlOn,
    Detect,
    DetectBit(u8),
    Enc(i64),
    CpRadio,
    CpPopup(Vec<i64>),
    NewLang(Vec<i64>),
    RecentMax,
    RecentSub,
    RecentLen(i64),
    RecentLenNum,
    TabLang(Vec<String>),
    TabDefault,
    TabSize,
    TabSpace(bool),
    TabBackspace,
    Browse,
    SnapshotSecs,
    BackupBrowse,
}

struct Ctl {
    c: Retained<NSControl>,
    b: Bind,
    echo: Option<Retained<NSTextField>>,
}

struct Ui {
    window: Retained<NSWindow>,
    table: Retained<NSTableView>,
    pages: Retained<NSTabView>,
    _list: Retained<PrefsList>,
    ctls: Vec<Ctl>,
    warned: bool,
}

struct Col {
    view: Retained<NSView>,
    x: f64,
    y: f64,
}

impl Col {
    fn put(&self, v: &NSView, dx: f64, w: f64, h: f64) {
        v.setFrame(rect(self.x + dx, self.y, w, h));
        self.view.addSubview(v);
    }
}

struct Build<'a> {
    mtm: MainThreadMarker,
    t: &'a AnyObject,
    ctls: Vec<Ctl>,
}

impl Build<'_> {
    fn reg(&mut self, c: Retained<NSControl>, b: Bind, action: Sel) -> Retained<NSControl> {
        c.setTag(self.ctls.len() as isize);
        unsafe {
            c.setTarget(Some(self.t));
            c.setAction(Some(action));
        }
        self.ctls.push(Ctl {
            c: c.clone(),
            b,
            echo: None,
        });
        c
    }

    fn label_at(&self, col: &Col, text: &str, dx: f64, w: f64) -> Retained<NSTextField> {
        let l = NSTextField::labelWithString(&ns(text), self.mtm);
        col.put(&l, dx, w, 20.);
        l
    }

    fn label(&self, col: &mut Col, text: &str, w: f64) {
        self.label_at(col, text, 0., w);
        col.y += 22.;
    }

    fn check(&mut self, col: &mut Col, title: &str, b: Bind) {
        let c =
            unsafe { NSButton::checkboxWithTitle_target_action(&ns(title), None, None, self.mtm) };
        c.sizeToFit();
        col.put(&c, 0., c.frame().size.width, 20.);
        self.reg(c.into_super(), b, sel!(prefChanged:));
        col.y += 22.;
    }

    fn radio(&mut self, col: &mut Col, title: &str, b: Bind) {
        let c = unsafe {
            NSButton::radioButtonWithTitle_target_action(&ns(title), None, None, self.mtm)
        };
        c.sizeToFit();
        col.put(&c, 0., c.frame().size.width, 20.);
        self.reg(c.into_super(), b, sel!(prefChanged:));
        col.y += 22.;
    }

    fn radio_at(&mut self, col: &Col, dx: f64, w: f64, title: &str, b: Bind) {
        let c = unsafe {
            NSButton::radioButtonWithTitle_target_action(&ns(title), None, None, self.mtm)
        };
        col.put(&c, dx, w, 20.);
        self.reg(c.into_super(), b, sel!(prefChanged:));
    }

    fn field_at(&mut self, col: &Col, dx: f64, w: f64, b: Bind) {
        let f = NSTextField::textFieldWithString(&NSString::new(), self.mtm);
        col.put(&f, dx, w, 22.);
        self.reg(f.into_super(), b, sel!(prefChanged:));
    }

    fn popup_at(&mut self, col: &Col, dx: f64, w: f64, items: &[String], b: Bind) {
        let p = NSPopUpButton::new(self.mtm);
        if let Some(m) = p.menu() {
            for i in items {
                unsafe { m.addItemWithTitle_action_keyEquivalent(&ns(i), None, &NSString::new()) };
            }
        }
        col.put(&p, dx, w, 26.);
        self.reg(p.into_super().into_super(), b, sel!(prefChanged:));
    }

    fn slider_at(&mut self, col: &Col, dx: f64, w: f64, min: f64, max: f64, b: Bind, echo: bool) {
        let s = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(
                min, min, max, None, None, self.mtm,
            )
        };
        s.setContinuous(false);
        col.put(&s, dx, w, 22.);
        self.reg(s.into_super(), b, sel!(prefChanged:));
        if echo {
            let l = self.label_at(col, "", dx + w + 6., 30.);
            if let Some(c) = self.ctls.last_mut() {
                c.echo = Some(l);
            }
        }
    }

    fn button_at(&mut self, col: &Col, dx: f64, w: f64, title: &str, b: Bind, action: Sel) {
        let c =
            unsafe { NSButton::buttonWithTitle_target_action(&ns(title), None, None, self.mtm) };
        col.put(&c, dx, w, 26.);
        self.reg(c.into_super(), b, action);
    }

    // A Notepad++ GROUPBOX: a title and its controls in one view, so that its radio buttons make one group.
    fn group(&mut self, col: &mut Col, title: &str, w: f64, f: impl FnOnce(&mut Self, &mut Col)) {
        let g = flipped(self.mtm);
        let mut inner = Col {
            view: g.clone(),
            x: 10.,
            y: 0.,
        };
        if !title.is_empty() {
            let l = self.label_at(&inner, title, -10., w);
            l.setFont(Some(
                &NSFont::boldSystemFontOfSize(NSFont::systemFontSize()),
            ));
            inner.y = 24.;
        }
        f(self, &mut inner);
        let h = inner.y + 4.;
        g.setFrame(rect(col.x, col.y, w, h));
        col.view.addSubview(&g);
        col.y += h + 12.;
    }
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// Language menu items for the New Document default language: (menu text, LangType).
fn new_doc_langs() -> Vec<(String, i64)> {
    let types = lang_types();
    let mut v: Vec<(String, i64)> = crate::session::lang_pairs(cfg())
        .into_iter()
        .filter(|(_, n)| n != "normal")
        .filter_map(|(t, n)| Some((t, types.iter().position(|(x, _)| *x == n)? as i64)))
        .collect();
    v.sort_by_key(|(t, _)| t.to_lowercase());
    v.insert(0, (crate::session::lang_menu_text(cfg(), "normal"), 0));
    v
}

// Languages of the Indentation list (IndentationSubDlg): (language name, short name); L_JS_EMBEDDED is left out.
fn indent_langs() -> Vec<(String, String)> {
    cfg()
        .languages
        .iter()
        .filter(|l| l.name != "javascript")
        .filter_map(|l| {
            let short = &lang_types().iter().find(|(n, _)| *n == l.name)?.1;
            (!short.is_empty()).then(|| (l.name.clone(), short.clone()))
        })
        .collect()
}

// GeneralSubDlg: the Localization list.
fn general_page(b: &mut Build, c: &mut Col, _: &mut Col) {
    b.label_at(c, "Localization", 0., 100.);
    c.put(&crate::l10n::popup(b.mtm), 104., 220., 26.);
}

fn build_pages(b: &mut Build) -> Vec<Retained<NSView>> {
    let mtm = b.mtm;
    let page = |b: &mut Build, f: &dyn Fn(&mut Build, &mut Col, &mut Col)| {
        let v = flipped(mtm);
        let mut c1 = Col {
            view: v.clone(),
            x: 10.,
            y: 10.,
        };
        let mut c2 = Col {
            view: v.clone(),
            x: 300.,
            y: 10.,
        };
        f(b, &mut c1, &mut c2);
        v
    };
    let sv = SVP;
    vec![
        page(b, &general_page),
        page(b, &|b, c, c2| {
            b.group(c, "Current Line Indicator", 270., |b, g| {
                b.radio(g, "None", Bind::Radio(sv, "currentLineIndicator", "0"));
                b.radio(
                    g,
                    "Highlight Background",
                    Bind::Radio(sv, "currentLineIndicator", "1"),
                );
                b.radio(g, "Frame", Bind::Radio(sv, "currentLineIndicator", "2"));
                b.label_at(g, "Width:", 20., 50.);
                b.slider_at(
                    g,
                    70.,
                    120.,
                    1.,
                    6.,
                    Bind::Slider(sv, "currentLineFrameWidth"),
                    true,
                );
                g.y += 26.;
            });
            b.group(c, "Caret Settings", 270., |b, g| {
                b.label_at(g, "Width:", 0., 70.);
                b.popup_at(
                    g,
                    74.,
                    120.,
                    &strings(&["0", "1", "2", "3", "Block", "Block After"]),
                    Bind::Popup("Caret", "width", strings(&["0", "1", "2", "3", "4", "5"])),
                );
                g.y += 30.;
                b.label_at(g, "Blink rate:", 0., 70.);
                b.label_at(g, "F", 74., 12.);
                b.slider_at(g, 88., 120., 50., 2500., Bind::Blink, false);
                b.label_at(g, "S", 212., 12.);
                g.y += 26.;
            });
            b.group(c, "Line Wrap", 270., |b, g| {
                b.radio(g, "Default", Bind::Radio(sv, "lineWrapMethod", "default"));
                b.radio(g, "Aligned", Bind::Radio(sv, "lineWrapMethod", "aligned"));
                b.radio(g, "Indent", Bind::Radio(sv, "lineWrapMethod", "indent"));
            });
            b.check(c2, "Enable smooth font", Bind::Check(sv, "smoothFont"));
            b.check(c2, "Enable virtual space", Bind::Check(sv, "virtualSpace"));
            b.check(
                c2,
                "Enable Copy/Cut Line without selection",
                Bind::Check(sv, "lineCopyCutWithoutSelection"),
            );
            b.check(
                c2,
                "Enable scrolling beyond last line",
                Bind::Check(sv, "scrollBeyondLastLine"),
            );
        }),
        page(b, &|b, c, _| {
            b.group(c, "Multi-Editing", 420., |b, g| {
                b.check(
                    g,
                    "Enable Multi-Editing (Ctrl+Mouse click/selection)",
                    Bind::Check(sv, "multiSelection"),
                );
                g.x += 14.;
                b.check(
                    g,
                    "Enable Column Selection to Multi-Editing",
                    Bind::Check(sv, "columnSel2MultiEdit"),
                );
            });
        }),
        page(b, &|b, c, c2| {
            b.group(c, "Fold Margin Style", 270., |b, g| {
                for (t, v) in [
                    ("Simple", "simple"),
                    ("Arrow", "arrow"),
                    ("Circle tree", "circle"),
                    ("Box tree", "box"),
                    ("None", "none"),
                ] {
                    b.radio(g, t, Bind::Radio(sv, "folderMarkStyle", v));
                }
            });
            b.group(c, "Vertical Edge Settings", 270., |b, g| {
                b.field_at(g, 0., 240., Bind::EdgeText);
                g.y += 28.;
                b.check(g, "Background mode", Bind::Check(sv, "isEdgeBgMode"));
            });
            b.group(c, "Change History", 270., |b, g| {
                b.check(
                    g,
                    "Show in the margin",
                    Bind::Bit(sv, "isChangeHistoryEnabled", 1),
                );
                b.check(
                    g,
                    "Show in the text",
                    Bind::Bit(sv, "isChangeHistoryEnabled", 2),
                );
            });
            b.group(c2, "Line Number", 260., |b, g| {
                b.check(g, "Display", Bind::Check(sv, "lineNumberMargin"));
                g.x += 14.;
                b.radio(
                    g,
                    "Dynamic width",
                    Bind::Radio(sv, "lineNumberDynamicWidth", "yes"),
                );
                b.radio(
                    g,
                    "Constant width",
                    Bind::Radio(sv, "lineNumberDynamicWidth", "no"),
                );
            });
            b.group(c2, "Padding", 260., |b, g| {
                b.label_at(g, "Left", 0., 40.);
                b.slider_at(g, 44., 150., 0., 30., Bind::Slider(sv, "paddingLeft"), true);
                g.y += 26.;
                b.label_at(g, "Right", 0., 40.);
                b.slider_at(
                    g,
                    44.,
                    150.,
                    0.,
                    30.,
                    Bind::Slider(sv, "paddingRight"),
                    true,
                );
                g.y += 26.;
            });
            b.check(c2, "Display bookmark", Bind::Check(sv, "bookMarkMargin"));
        }),
        page(b, &|b, c, c2| {
            let nd = "NewDocDefaultSettings";
            b.group(c, "Format (Line ending)", 260., |b, g| {
                b.radio(g, "Windows (CR LF)", Bind::Radio(nd, "format", "0"));
                b.radio(g, "Unix (LF)", Bind::Radio(nd, "format", "2"));
                b.radio(g, "Macintosh (CR)", Bind::Radio(nd, "format", "1"));
            });
            let langs = new_doc_langs();
            let names: Vec<String> = langs.iter().map(|l| l.0.clone()).collect();
            b.label_at(c, "Default language:", 0., 120.);
            b.popup_at(
                c,
                120.,
                160.,
                &names,
                Bind::NewLang(langs.iter().map(|l| l.1).collect()),
            );
            c.y += 40.;
            b.group(c2, "Encoding", 270., |b, g| {
                b.radio(g, "ANSI", Bind::Enc(0));
                b.radio(g, "UTF-8", Bind::Enc(4));
                g.x += 14.;
                b.check(
                    g,
                    "Apply to opened ANSI files",
                    Bind::Check(nd, "openAnsiAsUTF8"),
                );
                g.x -= 14.;
                b.radio(g, "UTF-8 with BOM", Bind::Enc(1));
                b.radio(g, "UTF-16 Big Endian with BOM", Bind::Enc(2));
                b.radio(g, "UTF-16 Little Endian with BOM", Bind::Enc(3));
                b.radio_at(g, 0., 24., "", Bind::CpRadio);
                let cps: Vec<(&str, u32)> = encoding::CHARSETS
                    .iter()
                    .flat_map(|(_, l)| l.iter().copied())
                    .collect();
                let names: Vec<String> = cps.iter().map(|(n, _)| n.to_string()).collect();
                b.popup_at(
                    g,
                    24.,
                    200.,
                    &names,
                    Bind::CpPopup(cps.iter().map(|c| c.1 as i64).collect()),
                );
                g.y += 30.;
            });
            c.y = c.y.max(c2.y);
            b.check(
                c,
                "Always open a new document in addition at startup",
                Bind::Check(nd, "addNewDocumentOnStartup"),
            );
        }),
        page(b, &|b, c, _| {
            b.group(c, "Default Open/Save file Directory", 480., |b, g| {
                b.radio(
                    g,
                    "Follow current document",
                    Bind::Radio("openSaveDir", "value", "0"),
                );
                b.radio(
                    g,
                    "Remember last used directory",
                    Bind::Radio("openSaveDir", "value", "1"),
                );
                b.radio_at(g, 0., 24., "", Bind::Radio("openSaveDir", "value", "2"));
                b.field_at(g, 24., 360., Bind::Text("openSaveDir", "defaultDirPath"));
                b.button_at(g, 390., 50., "...", Bind::Browse, sel!(prefBrowse:));
                g.y += 30.;
            });
        }),
        page(b, &|b, c, _| {
            b.group(c, "Recent Files History", 480., |b, g| {
                b.check(g, "Don't check at launch time", Bind::NoHistoryCheck);
                b.label_at(g, "Max. number of entries:", 0., 160.);
                b.field_at(g, 164., 40., Bind::RecentMax);
                b.label_at(g, "(0 – 30)", 210., 80.);
                g.y += 30.;
                b.group(g, "Display", 440., |b, d| {
                    b.check(d, "In Submenu", Bind::RecentSub);
                    b.radio(d, "Only File Name", Bind::RecentLen(0));
                    b.radio(d, "Full File Name Path", Bind::RecentLen(-1));
                    b.radio_at(d, 0., 230., "Customize Maximum Length:", Bind::RecentLen(1));
                    b.field_at(d, 234., 50., Bind::RecentLenNum);
                    b.label_at(d, "(1 – 259)", 290., 80.);
                    d.y += 28.;
                });
            });
        }),
        page(b, &|b, c, _| {
            b.group(c, "Indent Settings", 560., |b, g| {
                let langs = indent_langs();
                let mut names = vec!["[Default]".to_string()];
                names.extend(langs.iter().map(|l| l.1.clone()));
                let mut keys = vec![String::new()];
                keys.extend(langs.into_iter().map(|l| l.0));
                b.popup_at(g, 0., 220., &names, Bind::TabLang(keys));
                g.y += 34.;
                b.check(g, "Use default value", Bind::TabDefault);
                b.label_at(g, "Indent size:", 0., 90.);
                b.field_at(g, 94., 40., Bind::TabSize);
                g.y += 28.;
                b.label(g, "Indent using:", 200.);
                g.x += 14.;
                b.radio(g, "Tab character", Bind::TabSpace(false));
                b.radio(g, "Space character(s)", Bind::TabSpace(true));
                g.x -= 14.;
                b.check(
                    g,
                    "Backspace key unindents instead of removing single space",
                    Bind::TabBackspace,
                );
            });
        }),
        page(b, &|b, c, c2| {
            b.group(c, "Style All Occurrences of Token", 260., |b, g| {
                b.check(g, "Match case", Bind::Check("MarkAll", "matchCase"));
                b.check(
                    g,
                    "Match whole word only",
                    Bind::Check("MarkAll", "wholeWordOnly"),
                );
            });
            b.group(c, "Highlight Matching Tags", 260., |b, g| {
                b.check(g, "Enable", Bind::Check("TagsMatchHighLight", ""));
                b.check(
                    g,
                    "Highlight tag attributes",
                    Bind::Check("TagsMatchHighLight", "TagAttrHighLight"),
                );
                b.check(
                    g,
                    "Highlight comment/php/asp zone",
                    Bind::Check("TagsMatchHighLight", "HighLightNonHtmlZone"),
                );
            });
            b.group(c2, "Smart Highlighting", 270., |b, g| {
                b.check(g, "Enable", Bind::Check("SmartHighLight", ""));
                b.check(
                    g,
                    "Highlight another view",
                    Bind::Check("SmartHighLight", "onAnotherView"),
                );
                b.group(g, "Matching", 240., |b, m| {
                    b.check(m, "Match case", Bind::Check("SmartHighLight", "matchCase"));
                    b.check(
                        m,
                        "Match whole word only",
                        Bind::Check("SmartHighLight", "wholeWordOnly"),
                    );
                    b.check(
                        m,
                        "Use Find dialog settings",
                        Bind::Check("SmartHighLight", "useFindSettings"),
                    );
                });
            });
        }),
        page(b, &|b, c, _| {
            b.group(c, "When Find Dialog is Invoked", 560., |b, g| {
                b.check(
                    g,
                    "Fill Find Field with Selected Text",
                    Bind::Check("Searching", "fillFindFieldWithSelected"),
                );
                b.field_at(
                    g,
                    14.,
                    50.,
                    Bind::Num("Searching", "fillFindWhatThreshold", 1, 9999),
                );
                b.label_at(
                    g,
                    ": Max Characters to Auto-Fill Find Field from Selection",
                    66.,
                    400.,
                );
                g.y += 28.;
                g.x += 14.;
                b.check(
                    g,
                    "Select Word Under Caret when Nothing Selected",
                    Bind::Check("Searching", "fillFindFieldSelectCaret"),
                );
            });
        }),
        page(b, &|b, c, _| {
            b.group(c, "Session snapshot and periodic backup", 520., |b, g| {
                b.check(
                    g,
                    "Remember current session for next launch",
                    Bind::Check("RememberLastSession", ""),
                );
                b.check(
                    g,
                    "Enable session snapshot and periodic backup",
                    Bind::Check("Backup", "isSnapshotMode"),
                );
                b.label_at(g, "Trigger backup on modification in every", 14., 260.);
                b.field_at(g, 278., 40., Bind::SnapshotSecs);
                b.label_at(g, "seconds", 324., 80.);
                g.y += 28.;
                let path = app_support_dir().map(|d| d.join("backup/"));
                b.label_at(g, "Backup path:", 14., 90.);
                let l = b.label_at(
                    g,
                    &path.map_or(String::new(), |p| p.display().to_string()),
                    108.,
                    400.,
                );
                l.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
                l.setToolTip(Some(&l.stringValue()));
                g.y += 26.;
            });
            b.group(c, "Backup on save", 520., |b, g| {
                b.radio(g, "None", Bind::Radio("Backup", "action", "0"));
                b.radio(g, "Simple backup", Bind::Radio("Backup", "action", "1"));
                b.radio(g, "Verbose backup", Bind::Radio("Backup", "action", "2"));
                g.y += 6.;
                b.check(
                    g,
                    "Custom Backup Directory",
                    Bind::Check("Backup", "useCustumDir"),
                );
                b.label_at(g, "Directory:", 14., 70.);
                b.field_at(g, 88., 340., Bind::Text("Backup", "dir"));
                b.button_at(g, 434., 50., "...", Bind::BackupBrowse, sel!(prefChanged:));
                g.y += 30.;
            });
        }),
        page(b, &|b, c, c2| {
            let ac = "auto-completion";
            b.group(c, "Auto-Completion", 290., |b, g| {
                b.check(g, "Enable auto-completion on each input", Bind::AutocOn);
                g.x += 14.;
                b.radio(
                    g,
                    "Function completion",
                    Bind::Radio(ac, "autoCAction", "1"),
                );
                b.radio(g, "Word completion", Bind::Radio(ac, "autoCAction", "2"));
                b.radio(
                    g,
                    "Function and word completion",
                    Bind::Radio(ac, "autoCAction", "3"),
                );
                g.x -= 14.;
                b.check(
                    g,
                    "Make auto-completion list brief",
                    Bind::Check(ac, "autoCBrief"),
                );
                b.check(
                    g,
                    "Function parameters hint on input",
                    Bind::Check(ac, "funcParams"),
                );
                b.label_at(g, "From", 0., 40.);
                b.slider_at(
                    g,
                    44.,
                    120.,
                    1.,
                    9.,
                    Bind::Slider(ac, "triggerFromNbChar"),
                    true,
                );
                b.label_at(g, "th character", 200., 90.);
                g.y += 26.;
            });
            b.group(c2, "Insert Selection", 260., |b, g| {
                b.check(g, "TAB", Bind::Check(ac, "insertSelectedItemUseTAB"));
                b.check(g, "ENTER", Bind::Check(ac, "insertSelectedItemUseENTER"));
            });
            b.check(c2, "Ignore numbers", Bind::Check(ac, "autoCIgnoreNumbers"));
            c2.y += 10.;
            b.group(c2, "Auto-Insert", 260., |b, g| {
                for (t, a) in [
                    (" (", "parentheses"),
                    (" [", "brackets"),
                    (" {", "curlyBrackets"),
                    (" \"", "doubleQuotes"),
                    (" '", "quotes"),
                    (" html/xml close tag", "htmlXmlTag"),
                ] {
                    b.check(g, t, Bind::Check("auto-insert", a));
                }
            });
        }),
        page(b, &|b, c, _| {
            let dt = "insertDateTime";
            b.group(c, "Customize insert Date Time", 420., |b, g| {
                b.check(
                    g,
                    "Reverse default date time order (short & long formats)",
                    Bind::Check(dt, "reverseDefaultOrder"),
                );
                for (f, r) in [
                    ("yyyy-MM-dd HH:mm:ss", "1985-10-26 16:24:42"),
                    ("H:m d/M/yyyy", "16:24 26/10/1985"),
                    ("MMM d, yyyy  tt h:m", "Oct 26, 1985  PM 4:24"),
                ] {
                    b.label_at(g, f, 20., 170.).setAlignment(objc2_app_kit::NSTextAlignment::Right);
                    b.label_at(g, r, 210., 190.);
                    g.y += 20.;
                }
                g.y += 8.;
                b.label_at(g, "Custom format:", 0., 106.).setAlignment(objc2_app_kit::NSTextAlignment::Right);
                b.field_at(g, 110., 280., Bind::Text(dt, "customizedFormat"));
                g.y += 26.;
                let l = b.label_at(g, "", 110., 290.);
                if let Some(c) = b.ctls.last_mut() {
                    c.echo = Some(l);
                }
                g.y += 22.;
            });
        }),
        page(b, &|b, c, _| {
            b.group(c, "Clickable Link Settings", 560., |b, g| {
                b.check(g, "Enable", Bind::UrlOn);
                b.label(g, "URI customized schemes:", 300.);
                b.field_at(g, 0., 520., Bind::Text("uriCustomizedSchemes", ""));
                g.y += 28.;
            });
        }),
        page(b, &|b, c, _| {
            let se = "searchEngine";
            b.group(
                c,
                "Search Engine (for command \"Search on Internet\")",
                560.,
                |b, g| {
                    b.radio(g, "DuckDuckGo", Bind::Radio(se, "searchEngineChoice", "1"));
                    b.radio(g, "Google", Bind::Radio(se, "searchEngineChoice", "2"));
                    b.radio(g, "Yahoo!", Bind::Radio(se, "searchEngineChoice", "4"));
                    b.radio(
                        g,
                        "Stack Overflow",
                        Bind::Radio(se, "searchEngineChoice", "5"),
                    );
                    b.radio(
                        g,
                        "Set your search engine here:",
                        Bind::Radio(se, "searchEngineChoice", "0"),
                    );
                    b.field_at(g, 14., 400., Bind::Text(se, "searchEngineCustom"));
                    g.y += 28.;
                    b.label(
                        g,
                        "Example: https://www.google.com/search?q=$(CURRENT_WORD)",
                        520.,
                    );
                },
            );
        }),
        page(b, &|b, c, _| {
            b.group(c, "File Status Auto-Detection", 300., |b, g| {
                b.popup_at(
                    g,
                    0.,
                    240.,
                    &strings(&[
                        "Enable for current file",
                        "Enable for all opened files",
                        "Disable",
                    ]),
                    Bind::Detect,
                );
                g.y += 32.;
                b.check(g, "Update silently", Bind::DetectBit(CD_AUTO_UPDATE));
                b.check(
                    g,
                    "Scroll to the last line after update",
                    Bind::DetectBit(CD_GO2END),
                );
            });
        }),
    ]
}

fn on(c: &NSControl) -> bool {
    let s: isize = unsafe { msg_send![c, state] };
    s == 1
}

fn set_on(c: &NSControl, v: bool) {
    let _: () = unsafe { msg_send![c, setState: v as isize] };
}

fn popup_index(c: &NSControl) -> isize {
    unsafe { msg_send![c, indexOfSelectedItem] }
}

fn select_index(c: &NSControl, i: usize) {
    let _: () = unsafe { msg_send![c, selectItemAtIndex: i as isize] };
}

fn set_text(c: &NSControl, s: &str) {
    c.setStringValue(&ns(s));
}

// The language selected in the Indentation list, and whether it uses the default value.
fn tab_lang(ctls: &[Ctl]) -> Option<(String, i64, bool)> {
    let c = ctls.iter().find(|c| matches!(c.b, Bind::TabLang(_)))?;
    let Bind::TabLang(keys) = &c.b else {
        return None;
    };
    let name = keys.get(usize::try_from(popup_index(&c.c)).ok()?)?;
    if name.is_empty() {
        return None;
    }
    let (info, bs) = lang_tab(name);
    Some((name.clone(), info, bs))
}

fn recent_len_text(len: i64) -> String {
    if len > 0 {
        len.to_string()
    } else {
        String::new()
    }
}

// Shows the current settings in the controls, and enables the controls as preferenceDlg.cpp does.
fn refresh(app: &App) {
    let p = get();
    let (max, sub, len) = app.recent_options();
    UI.with(|u| {
        let u = u.borrow();
        let Some(u) = u.as_ref() else { return };
        let lang = tab_lang(&u.ctls);
        let detect = detect_bits(&p.auto_detect);
        for k in &u.ctls {
            let c = &k.c;
            let num = |g: &str, a: &str| p.get(g, a).trim().parse::<i64>().unwrap_or(0);
            let mut enabled = true;
            match &k.b {
                Bind::Check(g, a) => {
                    let v = p.get(g, a);
                    set_on(c, v == "yes" || v == "show");
                    enabled = match (*g, *a) {
                        (
                            "auto-completion",
                            "autoCBrief"
                            | "insertSelectedItemUseTAB"
                            | "insertSelectedItemUseENTER",
                        ) => p.autoc_action != 0,
                        ("NewDocDefaultSettings", "openAnsiAsUTF8") => {
                            p.new_encoding == 4 && p.new_codepage == -1
                        }
                        ("Backup", "isSnapshotMode") => p.remember_session,
                        ("Backup", "useCustumDir") => p.backup_action != 0,
                        _ => true,
                    };
                }
                Bind::Bit(g, a, bit) => set_on(c, num(g, a) & bit != 0),
                Bind::Radio(g, a, v) => {
                    set_on(c, p.get(g, a) == *v);
                    enabled = match *a {
                        "autoCAction" => p.autoc_action != 0,
                        "lineNumberDynamicWidth" => p.line_numbers.0,
                        _ => true,
                    };
                }
                Bind::Num(g, a, _, _) => set_text(c, &p.get(g, a)),
                Bind::Text(g, a) => {
                    set_text(c, &p.get(g, a));
                    enabled = match *g {
                        "openSaveDir" => p.open_save_dir == 2,
                        "uriCustomizedSchemes" => p.url_style != 0,
                        "searchEngine" => p.search_engine == 0,
                        "Backup" => p.backup_action != 0 && p.backup_use_dir,
                        _ => true,
                    };
                }
                Bind::Popup(g, a, vals) => {
                    select_index(c, vals.iter().position(|v| *v == p.get(g, a)).unwrap_or(0))
                }
                Bind::Slider(g, a) => {
                    c.setIntegerValue(num(g, a) as isize);
                    enabled = *a != "currentLineFrameWidth" || p.current_line == 2;
                }
                Bind::Blink => c.setIntegerValue(if p.caret_blink == 0 {
                    2500
                } else {
                    p.caret_blink as isize
                }),
                Bind::NoHistoryCheck => set_on(c, !p.check_history_files),
                Bind::EdgeText => set_text(
                    c,
                    &p.edges()
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                Bind::AutocOn => set_on(c, p.autoc_action != 0),
                Bind::UrlOn => set_on(c, p.url_style != 0),
                Bind::Detect => select_index(
                    c,
                    if detect & CD_ENABLED_OLD != 0 {
                        1
                    } else if detect & CD_ENABLED_NEW != 0 {
                        0
                    } else {
                        2
                    },
                ),
                Bind::DetectBit(bit) => {
                    set_on(c, detect & bit != 0);
                    enabled = detect != 0;
                }
                Bind::Enc(v) => set_on(c, p.new_codepage == -1 && p.new_encoding == *v),
                Bind::CpRadio => set_on(c, p.new_codepage != -1),
                Bind::CpPopup(vals) => {
                    select_index(
                        c,
                        vals.iter().position(|v| *v == p.new_codepage).unwrap_or(0),
                    );
                    enabled = p.new_codepage != -1;
                }
                Bind::NewLang(vals) => {
                    select_index(c, vals.iter().position(|v| *v == p.new_lang).unwrap_or(0))
                }
                Bind::RecentMax => set_text(c, &max.to_string()),
                Bind::RecentSub => set_on(c, sub),
                Bind::RecentLen(v) => set_on(c, v.signum() == len.signum()),
                Bind::RecentLenNum => {
                    set_text(c, &recent_len_text(len));
                    enabled = len > 0;
                }
                Bind::TabLang(_) => {}
                Bind::TabDefault => {
                    c.setHidden(lang.is_none());
                    set_on(c, lang.as_ref().is_some_and(|l| lang_default(l.1)));
                }
                Bind::TabSize | Bind::TabSpace(_) | Bind::TabBackspace => {
                    let (size, spaces, bs) = match &lang {
                        Some((_, info, bs)) if !lang_default(*info) => {
                            (info & 0x7F, info & 0x80 != 0, *bs)
                        }
                        _ => (p.tab_size, p.tab_replace, p.backspace_unindent),
                    };
                    match k.b {
                        Bind::TabSize => set_text(c, &size.to_string()),
                        Bind::TabSpace(s) => set_on(c, s == spaces),
                        _ => set_on(c, bs),
                    }
                    enabled = lang.as_ref().is_none_or(|l| !lang_default(l.1));
                }
                Bind::Browse => enabled = p.open_save_dir == 2,
                Bind::SnapshotSecs => {
                    set_text(c, &(p.snapshot_timing / 1000).to_string());
                    enabled = p.snapshot_mode;
                }
                Bind::BackupBrowse => enabled = p.backup_action != 0 && p.backup_use_dir,
            }
            c.setEnabled(enabled);
            if let Some(e) = &k.echo {
                let t = match &k.b {
                    Bind::Text("insertDateTime", _) => crate::edit_extras::date_time_preview(&p.date_time_format),
                    _ => c.integerValue().to_string(),
                };
                e.setStringValue(&ns(&t));
            }
        }
    });
}

impl App {
    pub(crate) fn show_preferences(&self) {
        if UI.with(|u| u.borrow().is_none()) {
            let ui = self.build_preferences();
            UI.with(|u| *u.borrow_mut() = Some(ui));
        }
        refresh(self);
        if let Some(w) = UI.with(|u| u.borrow().as_ref().map(|u| u.window.clone())) {
            w.makeKeyAndOrderFront(None);
        }
    }

    // IDM_EDIT_CHANGESEARCHENGINE: Preferences with the named page.
    pub(crate) fn show_preferences_page(&self, name: &str) {
        self.show_preferences();
        let Some(row) = PAGES.iter().position(|p| *p == name) else { return };
        if let Some(t) = UI.with(|u| u.borrow().as_ref().map(|u| u.table.clone())) {
            t.selectRowIndexes_byExtendingSelection(
                &objc2_foundation::NSIndexSet::indexSetWithIndex(row),
                false,
            );
            t.scrollRowToVisible(row as isize);
        }
    }

    fn build_preferences(&self) -> Ui {
        let mtm = self.mtm();
        let w = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0., 0., 820., 560.),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { w.setReleasedWhenClosed(false) };
        w.setTitle(&ns("Preferences"));
        let content = flipped(mtm);
        w.setContentView(Some(&content));
        let table = NSTableView::new(mtm);
        let col = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns("page"));
        col.setWidth(170.);
        col.setEditable(false);
        table.addTableColumn(&col);
        table.setHeaderView(None);
        let list: Retained<PrefsList> = unsafe { msg_send![PrefsList::alloc(mtm), init] };
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*list)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*list)));
        }
        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setHasVerticalScroller(true);
        scroll.setFrame(rect(10., 10., 180., 500.));
        content.addSubview(&scroll);
        let pages = NSTabView::new(mtm);
        pages.setTabViewType(NSTabViewType::NoTabsNoBorder);
        pages.setFrame(rect(200., 10., 610., 500.));
        content.addSubview(&pages);
        let mut b = Build {
            mtm,
            t: self,
            ctls: vec![],
        };
        for v in build_pages(&mut b) {
            let item = NSTabViewItem::new();
            item.setView(Some(&v));
            pages.addTabViewItem(&item);
        }
        let close = unsafe {
            NSButton::buttonWithTitle_target_action(
                &ns("Close"),
                Some(&w),
                Some(sel!(performClose:)),
                mtm,
            )
        };
        close.setFrame(rect(720., 520., 90., 28.));
        content.addSubview(&close);
        table.reloadData();
        table.selectRowIndexes_byExtendingSelection(
            &objc2_foundation::NSIndexSet::indexSetWithIndex(0),
            false,
        );
        Ui {
            window: w,
            table,
            pages,
            _list: list,
            ctls: b.ctls,
            warned: false,
        }
    }

    pub(crate) fn pref_browse(&self) {
        let p = NSOpenPanel::openPanel(self.mtm());
        p.setCanChooseDirectories(true);
        p.setCanChooseFiles(false);
        if p.runModal() == NSModalResponseOK {
            if let Some(path) = p.URL().and_then(|u| u.path()) {
                update(|x| {
                    x.default_dir = path.to_string();
                    x.open_save_dir = 2;
                });
                self.pref_saved();
            }
        }
    }

    // Saves config.xml, then shows and applies the settings, as each Notepad++ preference applies at once.
    fn pref_saved(&self) {
        if let Err(e) = save_config() {
            self.alert("Cannot save the settings", &e, &["OK"]);
        }
        refresh(self);
        self.apply_prefs();
    }

    pub(crate) fn apply_prefs(&self) {
        self.apply_view_all();
        self.backup_settings_changed();
    }

    pub(crate) fn pref_changed(&self, c: &NSControl) {
        let Some((b, warned)) = UI.with(|u| {
            let u = u.borrow();
            let u = u.as_ref()?;
            Some((
                u.ctls.get(usize::try_from(c.tag()).ok()?)?.b.clone(),
                u.warned,
            ))
        }) else {
            return;
        };
        let p = get();
        let txt = c.stringValue().to_string();
        let int = txt.trim().parse::<i64>().ok();
        let idx = || usize::try_from(popup_index(c)).unwrap_or(0);
        let (max, sub, len) = self.recent_options();
        let lang = UI.with(|u| u.borrow().as_ref().and_then(|u| tab_lang(&u.ctls)));
        let mut lang_write: Option<(String, i64, bool)> = None;
        match b {
            Bind::Check(g, a) => {
                let cur = p.get(g, a);
                let v = if cur == "show" || cur == "hide" {
                    Show(on(c)).put()
                } else {
                    on(c).put()
                };
                update(|x| x.set(g, a, &v));
                // BackupSubDlg: without the session there is no snapshot mode.
                if g == "RememberLastSession" && !on(c) {
                    update(|x| x.snapshot_mode = false);
                }
            }
            Bind::Bit(g, a, bit) => {
                let old = p.get(g, a).trim().parse::<i64>().unwrap_or(0);
                let new = if on(c) { old | bit } else { old & !bit };
                // MarginsBorderEdgeSubDlg: change history that was off starts at the next launch.
                if old == 0 && new != 0 && !warned {
                    UI.with(|u| u.borrow_mut().as_mut().map(|u| u.warned = true));
                    self.alert(
                        "Notepad++ needs to be relaunched",
                        "You have to restart Notepad++ to enable Change History.",
                        &["OK"],
                    );
                }
                update(|x| x.set(g, a, &new.to_string()));
            }
            Bind::Radio(g, a, v) => update(|x| x.set(g, a, v)),
            Bind::Num(g, a, min, max) => {
                if let Some(n) = int {
                    update(|x| x.set(g, a, &n.clamp(min, max).to_string()));
                }
            }
            Bind::Text(g, a) => update(|x| x.set(g, a, &txt)),
            Bind::Popup(g, a, vals) => {
                if let Some(v) = vals.get(idx()) {
                    update(|x| x.set(g, a, v));
                }
            }
            Bind::Slider(g, a) => update(|x| x.set(g, a, &c.integerValue().to_string())),
            Bind::Blink => {
                let n = c.integerValue() as i64;
                update(|x| x.caret_blink = if n >= 2500 { 0 } else { n });
            }
            Bind::NoHistoryCheck => update(|x| x.check_history_files = !on(c)),
            Bind::EdgeText => update(|x| {
                x.edge_columns = edge_list(&txt).iter().map(|n| format!("{n} ")).collect()
            }),
            Bind::AutocOn => update(|x| {
                x.autoc_action = if on(c) { AUTOC_BOTH } else { AUTOC_NONE };
                x.autoc_brief &= on(c);
                x.autoc_ignore_numbers = false;
            }),
            Bind::UrlOn => update(|x| x.url_style = if on(c) { 2 } else { 0 }),
            Bind::Detect => {
                let flags = detect_bits(&p.auto_detect) & (CD_AUTO_UPDATE | CD_GO2END);
                let bits = match idx() {
                    0 => CD_ENABLED_NEW | flags,
                    1 => CD_ENABLED_OLD | flags,
                    _ => 0,
                };
                update(|x| x.auto_detect = detect_name(bits).into());
            }
            Bind::DetectBit(bit) => {
                let old = detect_bits(&p.auto_detect);
                let bits = if on(c) { old | bit } else { old & !bit };
                update(|x| x.auto_detect = detect_name(bits).into());
            }
            Bind::Enc(v) => update(|x| {
                x.new_encoding = v;
                x.new_codepage = -1;
            }),
            Bind::CpRadio => {
                let cp = UI.with(|u| {
                    let u = u.borrow();
                    let k = u
                        .as_ref()?
                        .ctls
                        .iter()
                        .find(|k| matches!(k.b, Bind::CpPopup(_)))?;
                    let Bind::CpPopup(vals) = &k.b else {
                        return None;
                    };
                    vals.get(usize::try_from(popup_index(&k.c)).unwrap_or(0))
                        .copied()
                });
                update(|x| {
                    x.new_encoding = 0;
                    x.new_codepage = cp.unwrap_or(-1);
                });
            }
            Bind::CpPopup(vals) => update(|x| {
                x.new_encoding = 0;
                x.new_codepage = vals.get(idx()).copied().unwrap_or(-1);
            }),
            Bind::NewLang(vals) => update(|x| x.new_lang = vals.get(idx()).copied().unwrap_or(0)),
            Bind::RecentMax => {
                if let Some(n) = int {
                    self.set_recent_options(n.clamp(0, 30) as usize, sub, len);
                }
            }
            Bind::RecentSub => self.set_recent_options(max, on(c), len),
            Bind::RecentLen(v) => {
                let n = match v {
                    1 if len > 0 => len,
                    1 => 100,
                    v => v,
                };
                self.set_recent_options(max, sub, n);
            }
            Bind::RecentLenNum => {
                if let Some(n) = int {
                    self.set_recent_options(max, sub, if n == 0 { 100 } else { n.clamp(1, 259) });
                }
            }
            Bind::TabLang(_) => {}
            Bind::TabDefault => {
                if let Some((name, _, _)) = lang {
                    let info = (p.tab_size.clamp(1, 0x7F)) | if p.tab_replace { 0x80 } else { 0 };
                    lang_write = Some(if on(c) {
                        (name, -1, false)
                    } else {
                        (name, info, p.backspace_unindent)
                    });
                }
            }
            Bind::TabSize | Bind::TabSpace(_) | Bind::TabBackspace => match lang {
                Some((name, info, bs)) if !lang_default(info) => {
                    let (mut size, mut sp, mut bs) = (info & 0x7F, info & 0x80 != 0, bs);
                    match b {
                        Bind::TabSize => size = int.filter(|&n| n >= 1).unwrap_or(size).min(0x7F),
                        Bind::TabSpace(s) => sp = s,
                        _ => bs = on(c),
                    }
                    lang_write = Some((name, size | if sp { 0x80 } else { 0 }, bs));
                }
                _ => update(|x| match b {
                    Bind::TabSize => x.tab_size = int.filter(|&n| n >= 1).unwrap_or(x.tab_size),
                    Bind::TabSpace(s) => x.tab_replace = s,
                    _ => x.backspace_unindent = on(c),
                }),
            },
            Bind::Browse => {}
            Bind::SnapshotSecs => {
                if let Some(n) = int {
                    update(|x| x.snapshot_timing = n.clamp(1, 1_000_000) * 1000);
                }
            }
            Bind::BackupBrowse => {
                let o = NSOpenPanel::openPanel(self.mtm());
                o.setCanChooseDirectories(true);
                o.setCanChooseFiles(false);
                o.setMessage(Some(&ns("Select a folder as backup directory")));
                if o.runModal() != NSModalResponseOK {
                    return;
                }
                let Some(path) = o.URL().and_then(|u| u.path()) else {
                    return;
                };
                update(|x| x.backup_dir = path.to_string());
            }
        }
        if let Some((name, info, bs)) = lang_write {
            if let Err(e) = set_lang_tab(&name, info, bs) {
                self.alert("Cannot save the tab settings to langs.xml", &e, &["OK"]);
            }
        }
        self.pref_saved();
    }

    // The text that fills the Find field (FindReplaceDlg::setSearchTextWithSettings), on one line.
    pub(crate) fn find_fill_text(&self, v: &NSView) -> Option<String> {
        const SCI_GETCURRENTPOS: u32 = 2008;
        const SCI_WORDSTARTPOSITION: u32 = 2266;
        const SCI_WORDENDPOSITION: u32 = 2267;
        const SCI_COUNTCHARACTERS: u32 = 2633;
        let p = get();
        if !p.fill_find {
            return None;
        }
        let (mut s, mut e) = sci::selection(v);
        if s == e && p.fill_find_caret {
            let pos = sci::send(v, SCI_GETCURRENTPOS, 0, 0) as usize;
            let (ws, we) = (
                sci::send(v, SCI_WORDSTARTPOSITION, pos, 1),
                sci::send(v, SCI_WORDENDPOSITION, pos, 1),
            );
            if ws < we {
                sci::select(v, (ws, we));
                (s, e) = (ws, we);
            }
        }
        let b = sci::doc(v).range(s, e);
        let n = sci::send(v, SCI_COUNTCHARACTERS, s as usize, e);
        (s < e && n as i64 <= p.fill_find_max && !b.contains(&b'\n') && !b.contains(&b'\r'))
            .then(|| String::from_utf8_lossy(&b).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARAMS: &str = include_str!("../../PowerEditor/src/Parameters.h");
    const CONSTS: &str = include_str!("../../PowerEditor/src/MISC/Common/NppConstants.h");

    // The default value of a member in the NppGUI, ScintillaViewParams, NewDocDefaultSettings or MatchedPairConf block.
    fn member(name: &str) -> String {
        let blocks: String = [
            "struct NppGUI final",
            "struct ScintillaViewParams",
            "struct NewDocDefaultSettings final",
            "class MatchedPairConf final",
        ]
        .iter()
        .map(|b| {
            let t = &PARAMS[PARAMS.find(b).unwrap()..];
            t[..t.find("\n};").unwrap()].to_string()
        })
        .collect();
        let line = blocks
            .lines()
            .find(|l| l.contains(&format!(" {name} = ")))
            .unwrap_or_else(|| panic!("{name}"));
        let v = line
            .split(" = ")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .trim();
        let v = v.rsplit("::").next().unwrap();
        let v = v.strip_prefix("L\"").map_or(v, |x| x.trim_end_matches('"'));
        let named = [
            ("true", "yes"),
            ("false", "no"),
            ("LINEHILITE_HILITE", "1"),
            ("LINEWRAP_ALIGNED", "aligned"),
            ("FOLDER_STYLE_BOX", "box"),
            ("margin", "1"),
            ("osdefault", "0"),
            ("uniUTF8_NoBOM", "4"),
            ("L_TEXT", "0"),
            ("dir_followCurrent", "0"),
            ("autoc_both", "3"),
            ("urlUnderLineFg", "2"),
            ("se_google", "2"),
            ("cdEnabledNew", "yes"),
            ("bak_none", "0"),
        ];
        if let Some((_, x)) = named.iter().find(|(k, _)| *k == v) {
            return x.to_string();
        }
        if v.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            let l = CONSTS
                .lines()
                .find(|l| l.contains(&format!(" {v} = ")))
                .unwrap();
            return l
                .split(" = ")
                .nth(1)
                .unwrap()
                .trim_end_matches(';')
                .trim()
                .to_string();
        }
        v.to_string()
    }

    #[test]
    fn defaults_match_parameters_h() {
        let members = [
            ("currentLineIndicator", "_currentLineHiliteMode"),
            ("currentLineFrameWidth", "_currentLineFrameWidth"),
            ("width", "_caretWidth"),
            ("blinkRate", "_caretBlinkRate"),
            ("lineWrapMethod", "_lineWrapMethod"),
            ("smoothFont", "_doSmoothFont"),
            ("virtualSpace", "_virtualSpace"),
            (
                "lineCopyCutWithoutSelection",
                "_lineCopyCutWithoutSelection",
            ),
            ("scrollBeyondLastLine", "_scrollBeyondLastLine"),
            ("multiSelection", "_multiSelection"),
            ("columnSel2MultiEdit", "_columnSel2MultiEdit"),
            ("folderMarkStyle", "_folderStyle"),
            ("isEdgeBgMode", "_isEdgeBgMode"),
            (
                "isChangeHistoryEnabled",
                "_isChangeHistoryEnabled4NextSession",
            ),
            ("lineNumberMargin", "_lineNumberMarginShow"),
            ("lineNumberDynamicWidth", "_lineNumberMarginDynamicWidth"),
            ("paddingLeft", "_paddingLeft"),
            ("paddingRight", "_paddingRight"),
            ("bookMarkMargin", "_bookMarkMarginShow"),
            ("format", "_format"),
            ("encoding", "_unicodeMode"),
            ("lang", "_lang"),
            ("codepage", "_codepage"),
            ("openAnsiAsUTF8", "_openAnsiAsUtf8"),
            ("addNewDocumentOnStartup", "_addNewDocumentOnStartup"),
            ("value", "_openSaveDir"),
            ("CheckHistoryFiles", "_checkHistoryFiles"),
            ("size", "_tabSize"),
            ("replaceBySpace", "_tabReplacedBySpace"),
            ("backspaceUnindent", "_backspaceUnindent"),
            ("MarkAll.matchCase", "_markAllCaseSensitive"),
            ("MarkAll.wholeWordOnly", "_markAllWordOnly"),
            ("TagsMatchHighLight", "_enableTagsMatchHilite"),
            ("TagAttrHighLight", "_enableTagAttrsHilite"),
            ("HighLightNonHtmlZone", "_enableHiliteNonHTMLZone"),
            ("SmartHighLight", "_enableSmartHilite"),
            ("SmartHighLight.matchCase", "_smartHiliteCaseSensitive"),
            ("SmartHighLight.wholeWordOnly", "_smartHiliteWordOnly"),
            ("useFindSettings", "_smartHiliteUseFindSettings"),
            ("onAnotherView", "_smartHiliteOnAnotherView"),
            ("fillFindFieldWithSelected", "_fillFindFieldWithSelected"),
            ("fillFindFieldSelectCaret", "_fillFindFieldSelectCaret"),
            ("fillFindWhatThreshold", "_fillFindWhatThreshold"),
            ("RememberLastSession", "_rememberLastSession"),
            ("isSnapshotMode", "_isSnapshotMode"),
            ("snapshotBackupTiming", "_snapshotBackupTiming"),
            ("action", "_backup"),
            ("useCustumDir", "_useDir"),
            ("autoCAction", "_autocStatus"),
            ("triggerFromNbChar", "_autocFromLen"),
            ("autoCIgnoreNumbers", "_autocIgnoreNumbers"),
            ("insertSelectedItemUseENTER", "_autocInsertSelectedUseENTER"),
            ("insertSelectedItemUseTAB", "_autocInsertSelectedUseTAB"),
            ("autoCBrief", "_autocBrief"),
            ("funcParams", "_funcParams"),
            ("parentheses", "_doParentheses"),
            ("brackets", "_doBrackets"),
            ("curlyBrackets", "_doCurlyBrackets"),
            ("quotes", "_doQuotes"),
            ("doubleQuotes", "_doDoubleQuotes"),
            ("htmlXmlTag", "_doHtmlXmlTag"),
            ("URL", "_styleURL"),
            ("uriCustomizedSchemes", "_uriSchemes"),
            ("searchEngineChoice", "_searchEngineChoice"),
            ("Auto-detection", "_fileAutoDetection"),
            ("customizedFormat", "_dateTimeFormat"),
            ("reverseDefaultOrder", "_dateTimeReverseDefaultOrder"),
        ];
        let no_default = [
            "edgeMultiColumnPos",
            "defaultDirPath",
            "lastUsedDirPath",
            "searchEngineCustom",
            "dir",
        ];
        let p = Prefs::default();
        for (g, a) in Prefs::KEYS {
            if no_default.contains(a) {
                assert_eq!(p.get(g, a), "", "{g} {a}");
                continue;
            }
            let k = if a.is_empty() {
                g.to_string()
            } else {
                a.to_string()
            };
            let m = members
                .iter()
                .find(|(n, _)| *n == format!("{g}.{a}") || *n == k)
                .unwrap_or_else(|| panic!("{g} {a}"))
                .1;
            let got = p.get(g, a).replace("show", "yes").replace("hide", "no");
            assert_eq!(got, member(m), "{g} {a} {m}");
        }
    }

    const WINDOWS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    <GUIConfigs>\r\n        <GUIConfig name=\"ToolBar\" visible=\"yes\">standard</GUIConfig>\r\n        <GUIConfig name=\"TabSetting\" replaceBySpace=\"yes\" size=\"2\" backspaceUnindent=\"no\" />\r\n        <GUIConfig name=\"SmartHighLight\" matchCase=\"yes\" wholeWordOnly=\"yes\" useFindSettings=\"no\" onAnotherView=\"no\">no</GUIConfig>\r\n        <GUIConfig name=\"auto-insert\" parentheses=\"yes\" brackets=\"no\" curlyBrackets=\"no\" quotes=\"no\" doubleQuotes=\"no\" htmlXmlTag=\"no\">\r\n            <UserDefinePair open=\"&lt;\" close=\"&gt;\" />\r\n        </GUIConfig>\r\n        <GUIConfig name=\"ScintillaPrimaryView\" lineNumberMargin=\"hide\" bookMarkMargin=\"show\" folderMarkStyle=\"circle\" edgeMultiColumnPos=\"80 120 \" zoom=\"3\" />\r\n        <GUIConfig name=\"Auto-detection\">autoUpdate2End</GUIConfig>\r\n        <GUIConfig name=\"DarkMode\" enable=\"no\" darkThemeName=\"DarkModeDefault.xml\" lightThemeName=\"\" />\r\n        <GUIConfig name=\"uriCustomizedSchemes\" />\r\n    </GUIConfigs>\r\n    <FindHistory nbMaxFindHistoryPath=\"10\" />\r\n    <History nbMaxFile=\"10\" inSubMenu=\"no\" customLength=\"-1\">\r\n        <File filename=\"C:\\a &amp; b.txt\" />\r\n    </History>\r\n    <ProjectPanels />\r\n</NotepadPlus>\r\n";

    #[test]
    fn reads_a_windows_config() {
        let p = from_config(WINDOWS);
        assert_eq!((p.tab_size, p.tab_replace), (2, true));
        assert!(!p.smart_hl && p.smart_hl_case && p.smart_hl_word);
        assert!(p.insert_parentheses && !p.insert_brackets);
        assert_eq!(p.line_numbers, Show(false));
        assert_eq!(p.folder_style, "circle");
        assert_eq!(p.edges(), [80, 120]);
        assert_eq!(p.uri_schemes, URI_SCHEMES);
        let d = p.auto_detect();
        assert!(d.enabled && !d.all_files && d.silent && d.go_to_end);
        assert_eq!(p.caret_blink, 600);
    }

    #[test]
    fn round_trip_keeps_unknown_content() {
        let mut p = from_config(WINDOWS);
        p.caret_width = 4;
        p.default_dir = "/Users/me/a & \"b\"".into();
        p.smart_hl = true;
        p.uri_schemes = "ssh://".into();
        let out = patch(Some(WINDOWS), &GUI_PATH, "GUIConfig", &config_elems(&p)).unwrap();
        assert_eq!(from_config(&out), p);
        for keep in [
            "<GUIConfig name=\"ToolBar\" visible=\"yes\">standard</GUIConfig>",
            "<UserDefinePair open=\"&lt;\" close=\"&gt;\" />",
            "zoom=\"3\"",
            "darkThemeName=\"DarkModeDefault.xml\" lightThemeName=\"\"",
            "<FindHistory nbMaxFindHistoryPath=\"10\" />",
            "<File filename=\"C:\\a &amp; b.txt\" />",
            "<ProjectPanels />",
        ] {
            assert!(out.contains(keep), "{keep}");
        }
        assert_eq!(out.matches("name=\"SmartHighLight\"").count(), 1);
        assert!(out.contains(">yes</GUIConfig>"));
        assert!(out.contains("<GUIConfig name=\"Caret\" width=\"4\" blinkRate=\"600\" />"));
        assert_eq!(
            patch(Some(&out), &GUI_PATH, "GUIConfig", &config_elems(&p)).unwrap(),
            out
        );
        let parsed = crate::session::parse_history(&out);
        assert_eq!(parsed.files.len(), 1);
    }

    #[test]
    fn keeps_utf8_bom() {
        let x = "\u{FEFF}<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    <GUIConfigs />\r\n</NotepadPlus>\r\n";
        let h = crate::session::write_history(Some(x), &crate::session::Recent::default()).unwrap();
        let out = patch(Some(&h), &GUI_PATH, "GUIConfig", &config_elems(&Prefs::default())).unwrap();
        assert!(out.starts_with("\u{FEFF}<?xml") && !out[3..].contains('\u{FEFF}'));
        assert_eq!(from_config(&out), Prefs::default());
        let plain = patch(Some(&x[3..]), &GUI_PATH, "GUIConfig", &[]).unwrap();
        assert!(!plain.starts_with('\u{FEFF}'));
    }

    #[test]
    fn writes_new_files_and_containers() {
        let p = Prefs::default();
        let fresh = patch(None, &GUI_PATH, "GUIConfig", &config_elems(&p)).unwrap();
        assert!(fresh.starts_with("<?xml"));
        assert_eq!(from_config(&fresh), p);
        let no_gui = "<NotepadPlus>\r\n    <History nbMaxFile=\"5\" />\r\n</NotepadPlus>\r\n";
        let out = patch(Some(no_gui), &GUI_PATH, "GUIConfig", &config_elems(&p)).unwrap();
        assert!(out.contains("<History nbMaxFile=\"5\" />") && out.contains("<GUIConfigs>"));
        assert_eq!(from_config(&out), p);
        let empty = "<NotepadPlus><GUIConfigs /></NotepadPlus>";
        let out = patch(Some(empty), &GUI_PATH, "GUIConfig", &config_elems(&p)).unwrap();
        assert_eq!(from_config(&out), p);
        assert!(patch(
            Some("<NotepadPlus><GUIConfigs>"),
            &GUI_PATH,
            "GUIConfig",
            &[]
        )
        .is_err());
        let one = crate::styler::set_gui_config(Some(WINDOWS), "DarkMode", &[("enable", "yes".into())]).unwrap();
        assert!(one.contains("<GUIConfig name=\"DarkMode\" enable=\"yes\" darkThemeName=\"DarkModeDefault.xml\" lightThemeName=\"\" />"));
    }

    #[test]
    fn langs_xml_tab_settings() {
        let langs = "<NotepadPlus>\r\n    <Languages>\r\n        <Language name=\"python\" ext=\"py\" commentLine=\"#\" tabSettings=\"132\">\r\n            <Keywords name=\"instre1\">and as</Keywords>\r\n        </Language>\r\n    </Languages>\r\n</NotepadPlus>\r\n";
        let el = Elem {
            name: "python".into(),
            attrs: vec![
                ("tabSettings".into(), "-1".into()),
                ("backspaceUnindent".into(), "yes".into()),
            ],
            text: None,
        };
        let out = patch(Some(langs), &LANGS_PATH, "Language", &[el.clone()]).unwrap();
        assert!(out.contains("<Language name=\"python\" ext=\"py\" commentLine=\"#\" tabSettings=\"-1\" backspaceUnindent=\"yes\">"));
        assert!(out.contains("<Keywords name=\"instre1\">and as</Keywords>"));
        let read = read_elems(&out, &LANGS_PATH, "Language");
        assert_eq!(read.len(), 1);
        assert!(read[0].attrs.contains(&("tabSettings".into(), "-1".into())));
        let fresh = patch(None, &LANGS_PATH, "Language", &[el]).unwrap();
        assert_eq!(
            read_elems(&fresh, &LANGS_PATH, "Language")[0].name,
            "python"
        );
    }

    #[test]
    fn new_langs_xml_starts_from_the_model() {
        let el = Elem {
            name: "python".into(),
            attrs: vec![("tabSettings".into(), "2".into()), ("backspaceUnindent".into(), "no".into())],
            text: None,
        };
        let out = patch(Some(crate::view::LANGS), &LANGS_PATH, "Language", &[el]).unwrap();
        let model = read_elems(crate::view::LANGS, &LANGS_PATH, "Language");
        let langs = read_elems(&out, &LANGS_PATH, "Language");
        assert_eq!(langs.len(), model.len());
        let py = langs.iter().find(|e| e.name == "python").unwrap();
        assert!(py.attrs.contains(&("tabSettings".into(), "2".into())));
        assert_eq!(out.matches("<Keywords").count(), crate::view::LANGS.matches("<Keywords").count());
    }

    #[test]
    fn auto_detection_names() {
        for (n, b) in DETECT_NAMES {
            assert_eq!(detect_bits(n), b);
            assert_eq!(detect_name(b), n);
        }
        assert_eq!(detect_bits("bad"), 0);
        assert_eq!(detect_bits(""), CD_ENABLED_NEW);
        assert_eq!(detect_name(CD_AUTO_UPDATE), "no");
    }

    #[test]
    fn value_helpers() {
        let mut p = Prefs::default();
        assert_eq!(p.change_history_flags(), 3);
        p.change_history = 2;
        assert_eq!(p.change_history_flags(), 5);
        p.change_history = 3;
        assert_eq!(p.change_history_flags(), 7);
        p.change_history = 0;
        assert_eq!(p.change_history_flags(), 0);
        assert_eq!(p.wrap_indent(), 1);
        assert_eq!(p.new_doc_enc(), Enc::Utf8);
        p.new_codepage = 1251;
        assert_eq!(p.new_doc_enc(), Enc::Cp(1251));
        assert_eq!(edge_list("80, 120 9000 x"), [80, 120]);
        assert_eq!(
            p.search_engine_url(),
            "https://www.google.com/search?q=$(CURRENT_WORD)"
        );
        p.search_engine = 0;
        p.search_engine_custom = " https://example.com/?q=$(CURRENT_WORD) ".into();
        assert_eq!(
            p.search_engine_url(),
            "https://example.com/?q=$(CURRENT_WORD)"
        );
        let mut q = Prefs::default();
        q.set("ScintillaPrimaryView", "lineNumberMargin", "yes");
        assert_eq!(q.line_numbers, Show(true));
        q.set("Caret", "width", "x");
        assert_eq!(q.caret_width, 1);
    }

    #[test]
    fn lang_type_table() {
        let t = lang_types();
        assert_eq!(t[0].0, "normal");
        assert_eq!(t[3], ("cpp".to_string(), "C++".to_string()));
        assert_eq!(t[15].0, "udf");
        assert_eq!(t[22].0, "python");
        assert!(new_doc_langs()
            .iter()
            .any(|(n, i)| n == "Python" && *i == 22));
        assert!(indent_langs()
            .iter()
            .any(|(n, s)| n == "python" && s == "Python"));
        assert!(!indent_langs().iter().any(|(n, _)| n == "javascript"));
    }
}
