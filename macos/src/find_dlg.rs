// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::attr;
use crate::panel::{self, Form};
use crate::search::{self, Mode, Opts};
use crate::{ns, sci, styler, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{sel, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSBezelStyle, NSBox, NSBoxType, NSButton, NSColor,
    NSComboBox, NSControl, NSControlSize, NSFont, NSSegmentedControl, NSSlider, NSTextAlignment,
    NSTextField, NSView, NSWindowDidBecomeKeyNotification, NSWindowDidResignKeyNotification,
};
use objc2_foundation::{NSNotificationCenter, NSPoint, NSRect, NSSize, NSString};
use quick_xml::escape::escape;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Find,
    Replace,
    Files,
    Projects,
    Mark,
}

// The tab order and titles of FindReplaceDlg (titleFind, titleReplace, ... of english.xml).
pub const TABS: [(Tab, &str); 5] = [
    (Tab::Find, "Find"),
    (Tab::Replace, "Replace"),
    (Tab::Files, "Find in Files"),
    (Tab::Projects, "Find in Projects"),
    (Tab::Mark, "Mark"),
];

// The controls that a tab shows or hides, as the enable*Func functions of FindReplaceDlg do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ctl {
    ReplaceLabel,
    ReplaceWith,
    FiltersLabel,
    Filters,
    FilterTip,
    DirLabel,
    Dir,
    Browse,
    DirFromDoc,
    Sub,
    Hidden,
    Project(usize),
    Bookmark,
    Purge,
    Backward,
    Whole,
    Case,
    Wrap,
    InSel,
    SelBox,
    FindNext,
    FindPrev,
    FindNext2,
    TwoButtons,
    Count,
    FindAllCur,
    FindAllOpen,
    Replace,
    ReplaceAll,
    ReplaceAllOpen,
    FifFindAll,
    ReplaceInFiles,
    ReplaceInProjects,
    MarkAll,
    ClearMarks,
    CopyMarked,
    Close,
}

// Frame of a control on a tab in dialog units of FindReplaceDlg.rc (x, y, width, height), or None when hidden.
pub fn frame(tab: Tab, c: Ctl, two: bool) -> Option<[f64; 4]> {
    use Tab::*;
    let files = matches!(tab, Files | Projects);
    let find = matches!(tab, Find | Replace);
    let on = |show: bool, r: [f64; 4]| show.then_some(r);
    match c {
        Ctl::ReplaceLabel => on(tab != Find && tab != Mark, [1., 40., 73., 8.]),
        Ctl::ReplaceWith => on(tab != Find && tab != Mark, [76., 38., 170., 12.]),
        Ctl::FiltersLabel => on(files, [1., 58., 73., 8.]),
        Ctl::Filters => on(files, [76., 56., 170., 12.]),
        Ctl::FilterTip => on(files, [252., 58., 10., 8.]),
        Ctl::DirLabel => on(tab == Files, [7., 76., 41., 8.]),
        Ctl::Dir => on(tab == Files, [50., 74., 196., 12.]),
        Ctl::Browse => on(tab == Files, [250., 73., 16., 14.]),
        Ctl::DirFromDoc => on(tab == Files, [270., 73., 16., 14.]),
        Ctl::Sub => on(tab == Files, [300., 90., 94., 10.]),
        Ctl::Hidden => on(tab == Files, [300., 102., 94., 10.]),
        Ctl::Project(i) => on(tab == Projects, [300., 78. + 12. * i as f64, 94., 10.]),
        Ctl::Bookmark => on(tab == Mark, [12., 54., 140., 10.]),
        Ctl::Purge => on(tab == Mark, [12., 66., 140., 10.]),
        Ctl::Backward => on(!files, [12., 78., 140., 10.]),
        Ctl::Whole => Some([12., 90., 140., 10.]),
        Ctl::Case => Some([12., 102., 140., 10.]),
        Ctl::Wrap => on(!files, [12., 114., 140., 10.]),
        Ctl::InSel => on(!files, [190., 58., 90., 10.]),
        // calcAndSetCtrlsPos: the frame around "In selection" and the buttons that use it.
        Ctl::SelBox => match tab {
            Find => Some([184., 32., 207., 47.]),
            Replace => Some([184., 50., 207., 22.]),
            Mark => Some([184., 32., 207., 40.]),
            _ => None,
        },
        Ctl::FindNext => on(find && !two, [298., 20., 91., 14.]),
        Ctl::FindPrev => on(find && two, [298., 20., 17., 14.]),
        Ctl::FindNext2 => on(find && two, [319., 20., 70., 14.]),
        Ctl::TwoButtons => on(find, [393., 22., 14., 10.]),
        Ctl::Count => on(tab == Find, [298., 38., 91., 14.]),
        Ctl::FindAllCur => on(tab == Find, [298., 56., 91., 21.]),
        Ctl::FindAllOpen => on(tab == Find, [298., 81., 91., 21.]),
        Ctl::Replace => on(tab == Replace, [298., 38., 91., 14.]),
        Ctl::ReplaceAll => on(tab == Replace, [298., 56., 91., 14.]),
        Ctl::ReplaceAllOpen => on(tab == Replace, [298., 74., 91., 21.]),
        Ctl::FifFindAll => on(files, [298., 20., 91., 14.]),
        Ctl::ReplaceInFiles => on(tab == Files, [298., 38., 91., 14.]),
        Ctl::ReplaceInProjects => on(tab == Projects, [298., 38., 91., 14.]),
        Ctl::MarkAll => on(tab == Mark, [298., 38., 91., 14.]),
        Ctl::ClearMarks => on(tab == Mark, [298., 56., 91., 14.]),
        Ctl::CopyMarked => on(tab == Mark, [298., 74., 91., 14.]),
        Ctl::Close => {
            let y = match tab {
                Find => 106.,
                Replace => 99.,
                Files | Projects => 56.,
                Mark => 92.,
            };
            Some([298., y, 91., 14.])
        }
    }
}

// setDefaultButton of each enable*Func: the button that the Return key presses.
pub fn default_button(tab: Tab, two: bool) -> Ctl {
    match tab {
        Tab::Find | Tab::Replace if two => Ctl::FindNext2,
        Tab::Find | Tab::Replace => Ctl::FindNext,
        Tab::Files | Tab::Projects => Ctl::FifFindAll,
        Tab::Mark => Ctl::MarkAll,
    }
}

// Enabled state of (Match whole word only, Backward direction and the up button, ". matches newline") for a search mode.
pub fn mode_enables(mode: Mode) -> (bool, bool, bool) {
    let re = mode == Mode::Regex;
    (!re, !re, re)
}

// FindStatus values that select the colour of the status bar text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    NoMessage,
    NotFound,
    Message,
    Reached,
}

// The FindStatus that Notepad++ gives each status text of this app.
pub fn status_kind(msg: &str) -> Status {
    if msg.is_empty() {
        return Status::NoMessage;
    }
    let reached = [
        search::END_REACHED,
        search::TOP_REACHED,
        search::REPLACE_END_REACHED,
        search::REPLACE_TOP_REACHED,
    ];
    let failed = [
        "Find: Can't find",
        "Find: Invalid",
        "Replace: no occurrence",
        "Replace: Cannot",
        "Replace All: Cannot",
        "Find in Files stopped",
    ];
    if reached.contains(&msg) {
        Status::Reached
    } else if failed.iter().any(|f| msg.starts_with(f)) {
        Status::NotFound
    } else {
        Status::Message
    }
}

// The status bar shows one line; the rest of the text is the tooltip, as Notepad++ shows the reason.
pub fn split_status(msg: &str) -> (&str, &str) {
    msg.split_once('\n').unwrap_or((msg, ""))
}

// The "Find status" colours of the theme (FINDDLG_STAUS*_COLOR), as BGR.
fn status_colour(k: Status) -> Option<Retained<NSColor>> {
    let (name, def) = match k {
        Status::NotFound => ("Find status: Not found", 0x0000FF),
        Status::Message => ("Find status: Message", 0xFF0000),
        Status::Reached => ("Find status: Search end reached", 0x008000),
        Status::NoMessage => return None,
    };
    let bgr = styler::cfg()
        .global_styles
        .iter()
        .find(|s| s.name == name)
        .and_then(|s| s.fg)
        .unwrap_or(def);
    let c = |s: isize| ((bgr >> s) & 0xFF) as f64 / 255.;
    Some(NSColor::colorWithSRGBRed_green_blue_alpha(
        c(0),
        c(8),
        c(16),
        1.,
    ))
}

// NB_MAX_FINDHISTORY_* of Parameters.cpp, in the order path, filter, find, replace.
const NB_MAX: [i64; 4] = [30, 20, 30, 30];
const LIST_TAGS: [&str; 4] = ["Path", "Filter", "Find", "Replace"];
const MAX_ATTRS: [&str; 4] = [
    "nbMaxFindHistoryPath",
    "nbMaxFindHistoryFilter",
    "nbMaxFindHistoryFind",
    "nbMaxFindHistoryReplace",
];
// FINDREPLACE_MAXLENGTH2SAVE - 1.
const MAX_SAVE_LEN: usize = 2047;
pub const PATHS: usize = 0;
pub const FILTERS: usize = 1;
pub const FINDS: usize = 2;
pub const REPLACES: usize = 3;

// FindHistory of Parameters.h: the <FindHistory> element of config.xml.
#[derive(Clone, Debug, PartialEq)]
pub struct FindHistory {
    pub max: [i64; 4],
    pub lists: [Vec<String>; 4],
    pub match_word: bool,
    pub match_case: bool,
    pub wrap: bool,
    pub direction_down: bool,
    pub fif_recursive: bool,
    pub fif_hidden: bool,
    pub fif_projects: [bool; 3],
    pub filter_follows_doc: bool,
    pub search_mode: i64,
    pub transparency_mode: i64,
    pub transparency: i64,
    pub dot_nl: bool,
    pub two_buttons: bool,
    pub regex_backward: bool,
    pub bookmark_line: bool,
    pub purge: bool,
}

impl Default for FindHistory {
    fn default() -> Self {
        FindHistory {
            max: [10; 4],
            lists: Default::default(),
            match_word: false,
            match_case: false,
            wrap: true,
            direction_down: true,
            fif_recursive: true,
            fif_hidden: false,
            fif_projects: [false; 3],
            filter_follows_doc: false,
            search_mode: 0,
            transparency_mode: 1,
            transparency: 150,
            dot_nl: false,
            two_buttons: false,
            regex_backward: false,
            bookmark_line: false,
            purge: false,
        }
    }
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn xml_attr(s: &str) -> String {
    escape(s)
        .replace('\n', "&#10;")
        .replace('\r', "&#13;")
        .replace('\t', "&#9;")
}

// NppParameters::feedFindHistoryParameters.
pub fn parse_history(xml: &str) -> FindHistory {
    let mut h = FindHistory::default();
    let mut r = Reader::from_str(xml);
    let mut inside = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "FindHistory" => {
                inside = true;
                let int = |k: &str| attr(&e, k).trim().parse::<i64>().ok();
                let bool_ = |k: &str, d: bool| match attr(&e, k).as_str() {
                    "yes" => true,
                    "no" => false,
                    _ => d,
                };
                let range = |k: &str, lo: i64, hi: i64, d: i64| {
                    int(k).filter(|v| (lo..=hi).contains(v)).unwrap_or(d)
                };
                for i in 0..4 {
                    h.max[i] = int(MAX_ATTRS[i]).unwrap_or(h.max[i]).min(NB_MAX[i]);
                }
                h.match_word = bool_("matchWord", false);
                h.match_case = bool_("matchCase", false);
                h.wrap = bool_("wrap", h.wrap);
                h.direction_down = bool_("directionDown", h.direction_down);
                h.fif_recursive = bool_("fifRecuisive", h.fif_recursive);
                h.fif_hidden = bool_("fifInHiddenFolder", false);
                for i in 0..3 {
                    h.fif_projects[i] = bool_(&format!("fifProjectPanel{}", i + 1), false);
                }
                h.filter_follows_doc = bool_("fifFilterFollowsDoc", false);
                h.search_mode = range("searchMode", 0, 2, h.search_mode);
                h.transparency_mode = range("transparencyMode", 0, 2, h.transparency_mode);
                h.transparency = range("transparency", 1, 200, h.transparency);
                h.dot_nl = bool_("dotMatchesNewline", false);
                h.two_buttons = bool_("isSearch2ButtonsMode", false);
                h.regex_backward = bool_("regexBackward4PowerUser", false);
                h.bookmark_line = bool_("bookmarkLine", false);
                h.purge = bool_("purge", false);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if inside => {
                let tag = e.name();
                if let Some(i) = LIST_TAGS.iter().position(|t| *t == tag.as_ref()) {
                    let has = e.try_get_attribute("name").ok().flatten().is_some();
                    if h.max[i] > 0 && has && (h.lists[i].len() as i64) < NB_MAX[i] {
                        h.lists[i].push(attr(&e, "name"));
                    }
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == "FindHistory" => inside = false,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    h
}

// NppParameters::writeFindHistory.
pub fn history_xml(h: &FindHistory) -> String {
    let mut s = String::from("<FindHistory");
    for i in 0..4 {
        s += &format!(" {}=\"{}\"", MAX_ATTRS[i], h.max[i]);
    }
    let bools = [
        ("matchWord", h.match_word),
        ("matchCase", h.match_case),
        ("wrap", h.wrap),
        ("directionDown", h.direction_down),
        ("fifRecuisive", h.fif_recursive),
        ("fifInHiddenFolder", h.fif_hidden),
        ("fifProjectPanel1", h.fif_projects[0]),
        ("fifProjectPanel2", h.fif_projects[1]),
        ("fifProjectPanel3", h.fif_projects[2]),
        ("fifFilterFollowsDoc", h.filter_follows_doc),
    ];
    for (k, v) in bools {
        s += &format!(" {k}=\"{}\"", yes_no(v));
    }
    s += &format!(
        " searchMode=\"{}\" transparencyMode=\"{}\" transparency=\"{}\"",
        h.search_mode, h.transparency_mode, h.transparency
    );
    let bools = [
        ("dotMatchesNewline", h.dot_nl),
        ("isSearch2ButtonsMode", h.two_buttons),
        ("regexBackward4PowerUser", h.regex_backward),
        ("bookmarkLine", h.bookmark_line),
        ("purge", h.purge),
    ];
    for (k, v) in bools {
        s += &format!(" {k}=\"{}\"", yes_no(v));
    }
    if h.lists.iter().all(|l| l.is_empty()) {
        return s + " />";
    }
    s += ">\r\n";
    for (i, l) in h.lists.iter().enumerate() {
        for v in l {
            s += &format!("        <{} name=\"{}\" />\r\n", LIST_TAGS[i], xml_attr(v));
        }
    }
    s + "    </FindHistory>"
}

// addText2Combo: the text goes to the top; an equal entry (CB_FINDSTRINGEXACT ignores case) goes away.
pub fn add_entry(list: &mut Vec<String>, text: &str) {
    if text.is_empty() {
        return;
    }
    let low = text.to_lowercase();
    if let Some(i) = list.iter().position(|s| s.to_lowercase() == low) {
        list.remove(i);
    }
    list.insert(0, text.to_string());
}

// fillComboHistory: the combo entries and the field text from a saved list.
pub fn load_list(saved: &[String]) -> (Vec<String>, String) {
    let mut items = vec![];
    for s in saved.iter().rev() {
        add_entry(&mut items, s);
    }
    let text = match saved.first() {
        Some(s) if s.is_empty() => String::new(),
        _ => items.first().cloned().unwrap_or_default(),
    };
    (items, text)
}

// saveComboHistory: an empty field first when `save_empty`, then at most `max` entries that are not too long.
pub fn save_list(items: &[String], field: &str, max: i64, save_empty: bool) -> Vec<String> {
    let mut out = vec![];
    if save_empty && field.is_empty() {
        out.push(String::new());
    }
    let n = (max.max(0) as usize).min(items.len());
    out.extend(
        items[..n]
            .iter()
            .filter(|s| s.chars().count() <= MAX_SAVE_LEN)
            .cloned(),
    );
    out
}

const SX: f64 = 1.5;
const SY: f64 = 1.6;
const TOP: f64 = 14.;
const STATUS_H: f64 = 22.;
const W: f64 = 411. * SX;
const H: f64 = 197. * SY + TOP + STATUS_H;

fn is_push(c: Ctl) -> bool {
    use Ctl::*;
    matches!(
        c,
        Browse
            | DirFromDoc
            | FindNext
            | FindPrev
            | FindNext2
            | Count
            | FindAllCur
            | FindAllOpen
            | Replace
            | ReplaceAll
            | ReplaceAllOpen
            | FifFindAll
            | ReplaceInFiles
            | ReplaceInProjects
            | MarkAll
            | ClearMarks
            | CopyMarked
            | Close
    )
}

// A rectangle in dialog units as an AppKit frame; push buttons get room for their bezel, combo boxes their height.
fn to_frame(r: [f64; 4], c: Option<Ctl>) -> NSRect {
    let [x, y, w, h] = r;
    let (mut x, mut y, mut w, mut h) = (x * SX, y * SY + TOP, w * SX, h * SY);
    match c {
        Some(c) if is_push(c) && h < 30. => (x, y, w, h) = (x - 3., y - 3., w + 6., h + 6.),
        Some(Ctl::ReplaceWith | Ctl::Filters | Ctl::Dir) | None if h < 22. && w > 100. => {
            (y, h) = (y - 3., 23.)
        }
        _ => {}
    }
    NSRect::new(NSPoint::new(x, H - y - h), NSSize::new(w, h))
}

pub struct FindDlg {
    pub form: Form,
    tabs: Retained<NSSegmentedControl>,
    pub find: Retained<NSComboBox>,
    pub replace: Retained<NSComboBox>,
    pub filters: Retained<NSComboBox>,
    pub dir: Retained<NSComboBox>,
    pub whole: Retained<NSButton>,
    pub case: Retained<NSButton>,
    pub wrap: Retained<NSButton>,
    pub backward: Retained<NSButton>,
    pub in_sel: Retained<NSButton>,
    pub bookmark: Retained<NSButton>,
    pub purge: Retained<NSButton>,
    pub sub: Retained<NSButton>,
    pub hidden: Retained<NSButton>,
    pub projects: [Retained<NSButton>; 3],
    pub modes: [Retained<NSButton>; 3],
    pub dot_nl: Retained<NSButton>,
    two: Retained<NSButton>,
    transparent: Retained<NSButton>,
    on_focus: Retained<NSButton>,
    always: Retained<NSButton>,
    slider: Retained<NSSlider>,
    status: Retained<NSTextField>,
    ctls: Vec<(Ctl, Retained<NSView>)>,
    tab: Cell<Tab>,
    lists: RefCell<[Vec<String>; 4]>,
    hist: FindHistory,
}

impl FindDlg {
    pub fn tab(&self) -> Tab {
        self.tab.get()
    }

    pub fn mode(&self) -> Mode {
        match self.modes.iter().position(|b| panel::on(b)) {
            Some(1) => Mode::Extended,
            Some(2) => Mode::Regex,
            _ => Mode::Normal,
        }
    }

    pub fn opts(&self) -> Opts {
        Opts {
            find: panel::text(&self.find),
            replace: panel::text(&self.replace),
            whole_word: panel::on(&self.whole),
            match_case: panel::on(&self.case),
            wrap: panel::on(&self.wrap),
            mode: self.mode(),
            dot_nl: panel::on(&self.dot_nl),
            in_sel: panel::on(&self.in_sel),
            backward: panel::on(&self.backward),
        }
    }

    pub fn set_status(&self, msg: &str) {
        let (line, tip) = split_status(msg);
        self.status.setStringValue(&ns(line));
        self.status
            .setToolTip((!tip.is_empty()).then(|| ns(tip)).as_deref());
        let colour = status_colour(status_kind(msg)).unwrap_or_else(NSColor::labelColor);
        self.status.setTextColor(Some(&colour));
    }

    fn combo(&self, i: usize) -> &NSComboBox {
        match i {
            PATHS => &self.dir,
            FILTERS => &self.filters,
            FINDS => &self.find,
            _ => &self.replace,
        }
    }

    fn fill_combo(&self, i: usize) {
        let c = self.combo(i);
        let text = c.stringValue();
        c.removeAllItems();
        for item in self.lists.borrow()[i].iter() {
            unsafe { c.addItemWithObjectValue(&ns(item)) };
        }
        c.setStringValue(&text);
    }

    // updateCombo: the text of the field goes to the top of its history.
    pub fn remember(&self, i: usize) {
        add_entry(&mut self.lists.borrow_mut()[i], &panel::text(self.combo(i)));
        self.fill_combo(i);
    }

    // setSearchText: the text goes into Find what and its history.
    pub fn set_find_text(&self, s: &str) {
        self.find.setStringValue(&ns(s));
        self.remember(FINDS);
    }

    // FindReplaceDlg::saveFindHistory and the options of the dialog.
    pub fn history(&self) -> FindHistory {
        let l = self.lists.borrow();
        let h = &self.hist;
        let mut lists: [Vec<String>; 4] = Default::default();
        for (i, empty) in [
            (PATHS, false),
            (FILTERS, true),
            (FINDS, false),
            (REPLACES, true),
        ] {
            lists[i] = save_list(&l[i], &panel::text(self.combo(i)), h.max[i], empty);
        }
        let on = |b: &NSButton| panel::on(b);
        FindHistory {
            lists,
            match_word: on(&self.whole),
            match_case: on(&self.case),
            wrap: on(&self.wrap),
            direction_down: !on(&self.backward),
            fif_recursive: on(&self.sub),
            fif_hidden: on(&self.hidden),
            fif_projects: [0, 1, 2].map(|i| on(&self.projects[i])),
            search_mode: self.modes.iter().position(|b| on(b)).unwrap_or(0) as i64,
            transparency_mode: match (on(&self.transparent), on(&self.always)) {
                (false, _) => 0,
                (true, false) => 1,
                (true, true) => 2,
            },
            transparency: self.slider.integerValue() as i64,
            dot_nl: on(&self.dot_nl),
            two_buttons: on(&self.two),
            bookmark_line: on(&self.bookmark),
            purge: on(&self.purge),
            ..h.clone()
        }
    }

    // The enable*Func functions: show the controls of the tab at their place and set the default button.
    pub fn layout(&self, tab: Tab) {
        self.tab.set(tab);
        let two = panel::on(&self.two);
        let def = default_button(tab, two);
        for (c, v) in &self.ctls {
            match frame(tab, *c, two) {
                Some(r) => {
                    v.setFrame(to_frame(r, Some(*c)));
                    v.setHidden(false);
                }
                None => v.setHidden(true),
            }
            if is_push(*c) && *c != Ctl::Close {
                if let Some(b) = v.downcast_ref::<NSButton>() {
                    b.setKeyEquivalent(&ns(if *c == def { "\r" } else { "" }));
                }
            }
        }
        let i = TABS.iter().position(|t| t.0 == tab).unwrap_or(0);
        self.tabs.setSelectedSegment(i as isize);
        self.form.panel.setTitle(&ns(TABS[i].1));
    }

    // The IDREGEXP, IDEXTENDED and IDNORMAL handlers, and IDC_TRANSPARENT_CHECK.
    pub fn refresh(&self) {
        let (word, back, dot) = mode_enables(self.mode());
        for (b, en) in [(&self.whole, word), (&self.backward, back)] {
            if !en {
                panel::set_on(b, false);
            }
            b.setEnabled(en);
        }
        self.dot_nl.setEnabled(dot);
        if let Some((_, v)) = self.ctls.iter().find(|(c, _)| *c == Ctl::FindPrev) {
            if let Some(b) = v.downcast_ref::<NSButton>() {
                b.setEnabled(back);
            }
        }
        let t = panel::on(&self.transparent);
        self.on_focus.setEnabled(t);
        self.always.setEnabled(t);
        self.slider.setEnabled(t);
    }

    // NppParameters::setTransparent and removeTransparent; `key` is false when the dialog loses the focus.
    pub fn apply_transparency(&self, key: bool) {
        let alpha = self.slider.doubleValue() / 255.;
        let a = match (panel::on(&self.transparent), panel::on(&self.always)) {
            (true, true) => alpha,
            (true, false) if !key => alpha,
            _ => 1.,
        };
        self.form.panel.setAlphaValue(a);
    }

    // The transparency check box gives "On losing focus", as IDC_TRANSPARENT_CHECK does.
    pub fn transparency_changed(&self, sender: Option<&AnyObject>) {
        let check = sender.is_some_and(|s| {
            std::ptr::eq(s as *const AnyObject as *const u8, Retained::as_ptr(&self.transparent) as *const u8)
        });
        if check {
            panel::set_on(&self.on_focus, panel::on(&self.transparent));
            panel::set_on(&self.always, false);
        }
        self.refresh();
        self.apply_transparency(self.form.panel.isKeyWindow());
    }

    // The Find in Projects tab is on when a project panel is open; the check boxes of closed panels go off.
    #[allow(dead_code)]
    pub fn set_project_panels(&self, open: [bool; 3]) {
        for (b, o) in self.projects.iter().zip(open) {
            if !o {
                panel::set_on(b, false);
            }
            b.setEnabled(o);
        }
        self.tabs.setEnabled_forSegment(open.iter().any(|&o| o), 3);
    }
}

thread_local! {
    static DLG: OnceCell<&'static FindDlg> = const { OnceCell::new() };
}

fn small(v: &NSView) {
    if let Some(c) = v.downcast_ref::<NSControl>() {
        c.setControlSize(NSControlSize::Small);
        c.setFont(Some(&NSFont::systemFontOfSize(
            NSFont::smallSystemFontSize(),
        )));
    }
    for s in v.subviews().iter() {
        small(&s);
    }
}

fn build(app: &App) -> FindDlg {
    let mtm = app.mtm();
    let t: &AnyObject = app;
    let form = Form::new(mtm, "Find", W, H);
    let content = form.panel.contentView().unwrap();
    let hist = crate::session::read_config()
        .map(|x| parse_history(&x))
        .unwrap_or_default();
    let mut ctls: Vec<(Ctl, Retained<NSView>)> = vec![];
    let place = |v: &NSView, r: [f64; 4]| {
        v.setFrame(to_frame(r, None));
        content.addSubview(v);
    };
    let label = |s: &str, right: bool| {
        let l = NSTextField::labelWithString(&ns(s), mtm);
        if right {
            l.setAlignment(NSTextAlignment::Right);
        }
        content.addSubview(&l);
        l
    };
    let check = |s: &str, action: Sel| {
        let b = unsafe {
            NSButton::checkboxWithTitle_target_action(&ns(s), Some(t), Some(action), mtm)
        };
        content.addSubview(&b);
        b
    };
    let radio = |s: &str, action: Sel| {
        let b = unsafe {
            NSButton::radioButtonWithTitle_target_action(&ns(s), Some(t), Some(action), mtm)
        };
        content.addSubview(&b);
        b
    };
    let button = |s: &str, action: Sel| {
        let b =
            unsafe { NSButton::buttonWithTitle_target_action(&ns(s), Some(t), Some(action), mtm) };
        content.addSubview(&b);
        b
    };
    let combo = || {
        let c = NSComboBox::new(mtm);
        c.setNumberOfVisibleItems(10);
        c.setCompletes(false);
        content.addSubview(&c);
        c
    };
    let refresh = sel!(searchModeChanged:);

    let tabs = NSSegmentedControl::new(mtm);
    tabs.setSegmentCount(TABS.len() as isize);
    for (i, (_, title)) in TABS.iter().enumerate() {
        tabs.setLabel_forSegment(&ns(title), i as isize);
    }
    tabs.setEnabled_forSegment(false, 3);
    unsafe {
        tabs.setTarget(Some(t));
        tabs.setAction(Some(sel!(findTab:)));
    }
    content.addSubview(&tabs);

    let find_label = label("Find what:", true);
    place(&find_label, [1., 22., 73., 8.]);
    let find = combo();
    place(&find, [76., 20., 170., 12.]);

    let mut add = |c: Ctl, v: &NSView| ctls.push((c, v.retain()));
    let sel_box = NSBox::new(mtm);
    sel_box.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
    content.addSubview(&sel_box);
    add(Ctl::SelBox, &sel_box);
    add(Ctl::ReplaceLabel, &label("Replace with:", true));
    let replace = combo();
    add(Ctl::ReplaceWith, &replace);
    add(Ctl::FiltersLabel, &label("Filters:", true));
    let filters = combo();
    add(Ctl::Filters, &filters);
    let tip = label("(?)", false);
    tip.setToolTip(Some(&ns("Find in cpp, cxx, h, hxx & hpp:\n*.cpp *.cxx *.h *.hxx *.hpp\n\nFind in all files except exe, obj & log:\n*.* !*.exe !*.obj !*.log\n\nFind in all files but exclude folders tests, bin & bin64:\n*.* !\\tests !\\bin*\n\nFind in all files but exclude all folders log or logs recursively:\n*.* !+\\log*")));
    add(Ctl::FilterTip, &tip);
    add(Ctl::DirLabel, &label("Directory:", true));
    let dir = combo();
    add(Ctl::Dir, &dir);
    add(Ctl::Browse, &button("...", sel!(browseDir:)));
    let from_doc = button("<<", sel!(setDirFromDoc:));
    from_doc.setToolTip(Some(&ns("Fill directory field based on active document")));
    add(Ctl::DirFromDoc, &from_doc);
    let sub = check("In all sub-folders", refresh);
    add(Ctl::Sub, &sub);
    let hidden = check("In hidden folders", refresh);
    add(Ctl::Hidden, &hidden);
    let projects = [0, 1, 2].map(|i| {
        let b = check(&format!("Project Panel {}", i + 1), refresh);
        b.setEnabled(false);
        b
    });
    for (i, b) in projects.iter().enumerate() {
        add(Ctl::Project(i), b);
    }
    let bookmark = check("Bookmark line", refresh);
    add(Ctl::Bookmark, &bookmark);
    let purge = check("Purge for each search", refresh);
    add(Ctl::Purge, &purge);
    let backward = check("Backward direction", refresh);
    add(Ctl::Backward, &backward);
    let whole = check("Match whole word only", refresh);
    add(Ctl::Whole, &whole);
    let case = check("Match case", refresh);
    add(Ctl::Case, &case);
    let wrap = check("Wrap around", refresh);
    add(Ctl::Wrap, &wrap);
    let in_sel = check("In selection", refresh);
    add(Ctl::InSel, &in_sel);
    add(
        Ctl::FindNext,
        &button("Find Next", sel!(dlgFindNext:)),
    );
    add(Ctl::FindPrev, &button("▲", sel!(dlgFindUp:)));
    add(
        Ctl::FindNext2,
        &button("▼ Find Next", sel!(dlgFindDown:)),
    );
    let two = check("", sel!(findTwoButtons:));
    two.setToolTip(Some(&ns("2 find buttons mode")));
    add(Ctl::TwoButtons, &two);
    add(Ctl::Count, &button("Count", sel!(count:)));
    let tall = |s: &str, a: Sel| {
        let b = button(s, a);
        b.setBezelStyle(NSBezelStyle::FlexiblePush);
        b
    };
    add(
        Ctl::FindAllCur,
        &tall("Find All in Current Document", sel!(findAllInCurrent:)),
    );
    add(
        Ctl::FindAllOpen,
        &tall("Find All in All Opened Documents", sel!(findAllInOpened:)),
    );
    add(Ctl::Replace, &button("Replace", sel!(replace:)));
    add(
        Ctl::ReplaceAll,
        &button("Replace All", sel!(replaceAll:)),
    );
    add(
        Ctl::ReplaceAllOpen,
        &tall(
            "Replace All in All Opened Documents",
            sel!(replaceAllInOpened:),
        ),
    );
    add(Ctl::FifFindAll, &button("Find All", sel!(findAll:)));
    add(
        Ctl::ReplaceInFiles,
        &button("Replace in Files", sel!(replaceInFiles:)),
    );
    add(
        Ctl::ReplaceInProjects,
        &button("Replace in Projects", sel!(replaceInFiles:)),
    );
    add(Ctl::MarkAll, &button("Mark All", sel!(markAll:)));
    add(
        Ctl::ClearMarks,
        &button("Clear all marks", sel!(clearAllMarks:)),
    );
    add(
        Ctl::CopyMarked,
        &button("Copy Marked Text", sel!(copyMarkedText:)),
    );
    let close = button("Close", sel!(closePanel:));
    close.setKeyEquivalent(&ns("\u{1b}"));
    add(Ctl::Close, &close);

    let mode_box = NSBox::new(mtm);
    mode_box.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
    place(&mode_box, [6., 131., 200., 48.]);
    place(&label("Search Mode", false), [12., 133., 150., 8.]);
    let modes = [
        "Normal",
        "Extended (\\n, \\r, \\t, \\0, \\x...)",
        "Regular expression",
    ]
    .map(|s| radio(s, refresh));
    place(&modes[0], [12., 143., 150., 10.]);
    place(&modes[1], [12., 155., 150., 10.]);
    place(&modes[2], [12., 167., 78., 10.]);
    let dot_nl = check(". matches newline", refresh);
    place(&dot_nl, [93., 167., 101., 10.]);

    let tr = sel!(findTransparency:);
    let tr_box = NSBox::new(mtm);
    tr_box.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
    place(&tr_box, [288., 131., 101., 48.]);
    let transparent = check("Transparency", tr);
    place(&transparent, [292., 133., 80., 10.]);
    let on_focus = radio("On losing focus", tr);
    place(&on_focus, [298., 144., 85., 10.]);
    let always = radio("Always", tr);
    place(&always, [298., 155., 85., 10.]);
    let slider = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(
            150.,
            20.,
            200.,
            Some(t),
            Some(tr),
            mtm,
        )
    };
    place(&slider, [295., 166., 85., 10.]);

    let line = NSBox::new(mtm);
    line.setBoxType(NSBoxType::Separator);
    line.setFrame(NSRect::new(NSPoint::new(0., STATUS_H), NSSize::new(W, 1.)));
    content.addSubview(&line);
    let status = NSTextField::labelWithString(&NSString::new(), mtm);
    status.setFrame(NSRect::new(
        NSPoint::new(6., 3.),
        NSSize::new(W - 12., STATUS_H - 6.),
    ));
    content.addSubview(&status);

    small(&content);
    tabs.sizeToFit();
    let ts = tabs.frame().size;
    tabs.setFrame(NSRect::new(NSPoint::new(8., H - 8. - ts.height), ts));

    let dlg = FindDlg {
        form,
        tabs,
        find,
        replace,
        filters,
        dir,
        whole,
        case,
        wrap,
        backward,
        in_sel,
        bookmark,
        purge,
        sub,
        hidden,
        projects,
        modes,
        dot_nl,
        two,
        transparent,
        on_focus,
        always,
        slider,
        status,
        ctls,
        tab: Cell::new(Tab::Find),
        lists: RefCell::new(Default::default()),
        hist,
    };
    dlg.load_history();
    unsafe {
        let nc = NSNotificationCenter::defaultCenter();
        for n in [
            NSWindowDidBecomeKeyNotification,
            NSWindowDidResignKeyNotification,
        ] {
            nc.addObserver_selector_name_object(
                t,
                sel!(findDlgKey:),
                Some(n),
                Some(&dlg.form.panel),
            );
        }
    }
    dlg
}

impl FindDlg {
    // FindReplaceDlg::fillFindHistory.
    fn load_history(&self) {
        let h = &self.hist;
        for i in 0..4 {
            let (items, text) = load_list(&h.lists[i]);
            self.lists.borrow_mut()[i] = items;
            self.combo(i).setStringValue(&ns(&text));
            self.fill_combo(i);
        }
        if panel::text(&self.filters).is_empty() {
            self.filters.setStringValue(&ns("*.*"));
        }
        let set = |b: &NSButton, v: bool| panel::set_on(b, v);
        set(&self.wrap, h.wrap);
        set(&self.whole, h.match_word);
        set(&self.case, h.match_case);
        set(&self.backward, !h.direction_down);
        set(&self.hidden, h.fif_hidden);
        set(&self.sub, h.fif_recursive);
        for i in 0..3 {
            set(&self.projects[i], h.fif_projects[i]);
        }
        set(&self.modes[h.search_mode.clamp(0, 2) as usize], true);
        set(&self.dot_nl, h.dot_nl);
        set(&self.bookmark, h.bookmark_line);
        set(&self.purge, h.purge);
        set(&self.two, h.two_buttons);
        self.slider
            .setIntegerValue(h.transparency.clamp(20, 200) as isize);
        set(&self.transparent, h.transparency_mode != 0);
        set(&self.on_focus, h.transparency_mode == 1);
        set(&self.always, h.transparency_mode == 2);
        self.refresh();
        self.layout(Tab::Find);
        self.apply_transparency(false);
    }
}

// The FindHistory element for session::save_config; config.xml keeps its old element when the dialog was not opened.
pub fn patch_config(x: &str) -> Result<String, String> {
    match DLG.with(|d| d.get().map(|d| d.history())) {
        Some(h) => crate::session::replace_element(Some(x), "FindHistory", &history_xml(&h)),
        None => Ok(x.to_string()),
    }
}

impl App {
    pub(crate) fn find_ui(&self) -> &'static FindDlg {
        DLG.with(|c| *c.get_or_init(|| Box::leak(Box::new(build(self)))))
    }

    // FindReplaceDlg::doDialog: the dialog on a tab, with the selected text in Find what.
    pub(crate) fn open_find_tab(&self, tab: Tab) {
        let u = self.find_ui();
        if tab == Tab::Files && panel::text(&u.dir).is_empty() {
            let dir = self
                .current()
                .and_then(|i| self.tab(i)?.path?.parent().map(|p| p.to_path_buf()));
            if let Some(d) = dir {
                u.dir.setStringValue(&ns(&d.to_string_lossy()));
            }
        }
        if let Some(s) = self.selected_line() {
            u.set_find_text(&s);
        }
        u.layout(tab);
        u.form.panel.makeKeyAndOrderFront(None);
        u.form.panel.makeFirstResponder(Some(&*u.find));
        unsafe { u.find.selectText(None) };
    }

    pub(crate) fn find_tab_changed(&self) {
        let u = self.find_ui();
        let i = u.tabs.selectedSegment().max(0) as usize;
        u.layout(TABS.get(i).map_or(Tab::Find, |t| t.0));
    }

    pub(crate) fn find_two_buttons(&self) {
        let u = self.find_ui();
        u.layout(u.tab());
    }

    // The options of the dialog for the editor; "In selection" needs selected text.
    fn dlg_opts(&self, v: &NSView) -> Opts {
        let mut o = self.find_ui().opts();
        let s = sci::selection(v);
        o.in_sel &= s.0 != s.1;
        o
    }

    fn remember_find_replace(&self) {
        let u = self.find_ui();
        u.remember(REPLACES);
        u.remember(FINDS);
    }

    // IDOK, IDC_FINDNEXT and IDC_FINDPREV; `up` None takes "Backward direction".
    pub(crate) fn dlg_find(&self, up: Option<bool>, cmd: isize) {
        self.remember_find_replace();
        let mut o = self.find_ui().opts();
        o.in_sel = false;
        let up = up.unwrap_or(o.backward);
        crate::macros::record_search(
            &Opts {
                backward: up,
                ..o.clone()
            },
            cmd,
            false,
        );
        self.find_with(&o, up);
    }

    // IDCCOUNTALL.
    pub(crate) fn dlg_count(&self) {
        let u = self.find_ui();
        u.remember(FINDS);
        let Some(v) = self.editor() else { return };
        let o = self.dlg_opts(&v);
        let doc = sci::doc(&v);
        let range = search::all_range(&o, sci::selection(&v), doc.len());
        u.set_status(&match search::process(&doc, &o, false, false, range) {
            Ok(m) => search::count_status(m.len(), &o),
            Err(e) => e,
        });
    }

    // IDREPLACE.
    pub(crate) fn dlg_replace(&self) {
        self.remember_find_replace();
        let u = self.find_ui();
        let Some(v) = self.editor() else { return };
        let o = self.dlg_opts(&v);
        if o.find.is_empty() {
            return;
        }
        crate::macros::record_search(&o, crate::macros::IDREPLACE, true);
        u.set_status(&self.replace_once(&v, &o).unwrap_or_else(|e| e));
    }

    // IDREPLACEALL.
    pub(crate) fn dlg_replace_all(&self) {
        self.remember_find_replace();
        let u = self.find_ui();
        let Some(v) = self.editor() else { return };
        let o = self.dlg_opts(&v);
        crate::macros::record_search(&o, crate::macros::IDREPLACEALL, true);
        let doc = sci::doc(&v);
        let range = search::all_range(&o, sci::selection(&v), doc.len());
        u.set_status(&match search::replace_all(&doc, &o, range) {
            Ok(n) => search::replace_all_status(n, &o),
            Err(e) => e,
        });
    }

    // Notepad_plus::findInCurrentFile and findInOpenedFiles: the hits go to the search results.
    pub(crate) fn dlg_find_all(&self, all_docs: bool) {
        let u = self.find_ui();
        u.remember(FINDS);
        u.set_status("");
        let Some(v) = self.editor() else { return };
        let mut o = self.dlg_opts(&v);
        if o.find.is_empty() {
            return;
        }
        let tabs: Vec<(Retained<NSView>, PathBuf)> = if all_docs {
            o.in_sel = false;
            let tabs = self.ivars().tabs.borrow();
            tabs.iter()
                .map(|t| {
                    (
                        t.view.clone(),
                        t.path.clone().unwrap_or_else(|| t.name.clone().into()),
                    )
                })
                .collect()
        } else {
            let Some(t) = self.current().and_then(|i| self.tab(i)) else {
                return;
            };
            vec![(
                t.view.clone(),
                t.path.clone().unwrap_or_else(|| t.name.clone().into()),
            )]
        };
        let (mut body, mut hits, mut files) = (vec![], 0, 0);
        for (view, path) in &tabs {
            let doc = sci::doc(view);
            let range = if o.in_sel {
                sci::selection(view)
            } else {
                (0, doc.len())
            };
            match search::find_all_lines(&doc, &o, path, range) {
                Ok(lines) if !lines.is_empty() => {
                    hits += lines
                        .iter()
                        .filter_map(|l| l.hit.as_ref())
                        .map(|h| h.1.len())
                        .sum::<usize>();
                    files += 1;
                    body.extend(lines);
                }
                Ok(_) => {}
                Err(e) => return u.set_status(&e),
            }
        }
        let mut lines = vec![search::Line {
            text: search::search_header(&o, hits, files, tabs.len()).into_bytes(),
            ..Default::default()
        }];
        lines.extend(body);
        self.show_results(lines);
        if hits > 0 {
            u.form.panel.orderOut(None);
        }
    }

    // IDC_REPLACE_OPENEDFILES with replaceInOpenDocsConfirmCheck.
    pub(crate) fn dlg_replace_all_opened(&self) {
        let u = self.find_ui();
        if self.ivars().fif_running.get() {
            return;
        }
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&ns("Are you sure?"));
        a.setInformativeText(&ns(
            "Are you sure you want to replace all occurrences in all open documents?",
        ));
        a.addButtonWithTitle(&ns("OK")).setKeyEquivalent(&ns(""));
        a.addButtonWithTitle(&ns("Cancel"))
            .setKeyEquivalent(&ns("\r"));
        if a.runModal() != NSAlertFirstButtonReturn {
            return;
        }
        u.set_status("");
        self.remember_find_replace();
        let o = Opts {
            in_sel: false,
            ..u.opts()
        };
        let views: Vec<(Retained<NSView>, bool)> = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .map(|t| (t.view.clone(), t.ro))
            .collect();
        let mut total = 0;
        for (v, ro) in views {
            let doc = sci::doc(&v);
            if ro || doc.read_only() {
                continue;
            }
            match search::replace_all(&doc, &o, (0, doc.len())) {
                Ok(n) => total += n,
                Err(e) => return u.set_status(&e),
            }
        }
        u.set_status(&search::replace_in_opened_status(total));
    }

    // IDD_FINDINFILES_SETDIRFROMDOC_BUTTON.
    pub(crate) fn set_dir_from_doc(&self) {
        let dir = self
            .current()
            .and_then(|i| self.tab(i)?.path?.parent().map(|p| p.to_path_buf()));
        if let Some(d) = dir.filter(|d| d.is_dir()) {
            self.find_ui().dir.setStringValue(&ns(&d.to_string_lossy()));
        }
    }

    // Find All and Replace in Projects of the Find in Projects tab; the Project Panels slice adds the search.
    pub(crate) fn projects_search(&self, _replace: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_history_round_trip() {
        let mut h = FindHistory {
            max: [5, 20, 30, 0],
            match_word: true,
            wrap: false,
            direction_down: false,
            fif_hidden: true,
            fif_projects: [false, true, false],
            search_mode: 2,
            transparency_mode: 2,
            transparency: 77,
            dot_nl: true,
            two_buttons: true,
            purge: true,
            ..Default::default()
        };
        h.lists[PATHS] = vec!["/a/b".into(), "/c \"q\" & <x>".into()];
        h.lists[FILTERS] = vec!["".into(), "*.rs *.toml".into()];
        h.lists[FINDS] = vec!["foo\nbar\tbaz".into(), "é€".into()];
        let x = history_xml(&h);
        assert!(
            x.starts_with("<FindHistory nbMaxFindHistoryPath=\"5\" nbMaxFindHistoryFilter=\"20\""),
            "{x}"
        );
        assert!(x.contains("fifRecuisive=\"yes\""), "{x}");
        let config = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    {x}\r\n</NotepadPlus>\r\n");
        let back = parse_history(&config);
        h.lists[REPLACES].clear();
        assert_eq!(back, h);
        let patched =
            crate::session::replace_element(Some(&config), "FindHistory", "<FindHistory />")
                .unwrap();
        assert_eq!(parse_history(&patched), FindHistory::default());
        assert_eq!(
            history_xml(&FindHistory::default())
                .matches("<FindHistory")
                .count(),
            1
        );
    }

    #[test]
    fn find_history_reads_notepad_plus_plus_values() {
        let x = r#"<NotepadPlus><FindHistory nbMaxFindHistoryPath="99" nbMaxFindHistoryFind="0" searchMode="7" transparencyMode="0" transparency="300" wrap="maybe">
            <Path name="p1" /><Path /><Find name="ignored" /><Replace name="r" /><Filter name="*.c" /></FindHistory></NotepadPlus>"#;
        let h = parse_history(x);
        assert_eq!(h.max, [30, 10, 0, 10]);
        assert_eq!(h.lists[PATHS], ["p1"]);
        assert!(h.lists[FINDS].is_empty());
        assert_eq!(h.lists[REPLACES], ["r"]);
        assert_eq!(h.lists[FILTERS], ["*.c"]);
        assert_eq!(
            (h.search_mode, h.transparency_mode, h.transparency),
            (0, 0, 150)
        );
        assert!(h.wrap && h.direction_down && h.fif_recursive);
        assert_eq!(parse_history("<NotepadPlus />"), FindHistory::default());
    }

    #[test]
    fn combo_history_entries() {
        let mut l = vec![];
        add_entry(&mut l, "a");
        add_entry(&mut l, "b");
        add_entry(&mut l, "");
        add_entry(&mut l, "A");
        assert_eq!(l, ["A", "b"]);
        assert_eq!(
            load_list(&["x".into(), "y".into(), "X".into()]),
            (vec!["x".into(), "y".into()], "x".into())
        );
        assert_eq!(
            load_list(&["".into(), "y".into()]),
            (vec!["y".into()], "".into())
        );
        assert_eq!(load_list(&[]), (vec![], "".into()));
        let items: Vec<String> = vec!["1".into(), "x".repeat(2048), "3".into(), "4".into()];
        assert_eq!(save_list(&items, "", 3, true), ["", "1", "3"]);
        assert_eq!(save_list(&items, "", 3, false), ["1", "3"]);
        assert_eq!(save_list(&items, "f", 10, true), ["1", "3", "4"]);
        assert!(save_list(&items, "", 0, false).is_empty());
    }

    #[test]
    fn tab_controls_and_option_state() {
        use Ctl::*;
        let shown = |tab: Tab, two: bool| -> Vec<Ctl> {
            let all = [
                ReplaceLabel,
                ReplaceWith,
                FiltersLabel,
                Filters,
                FilterTip,
                DirLabel,
                Dir,
                Browse,
                DirFromDoc,
                Sub,
                Hidden,
                Project(0),
                Project(1),
                Project(2),
                Bookmark,
                Purge,
                Backward,
                Whole,
                Case,
                Wrap,
                InSel,
                SelBox,
                FindNext,
                FindPrev,
                FindNext2,
                TwoButtons,
                Count,
                FindAllCur,
                FindAllOpen,
                Replace,
                ReplaceAll,
                ReplaceAllOpen,
                FifFindAll,
                ReplaceInFiles,
                ReplaceInProjects,
                MarkAll,
                ClearMarks,
                CopyMarked,
                Close,
            ];
            all.into_iter()
                .filter(|&c| frame(tab, c, two).is_some())
                .collect()
        };
        let common = [Whole, Case, Close];
        let with = |v: &[Ctl]| -> Vec<Ctl> {
            let mut all: Vec<Ctl> = v.to_vec();
            all.extend(common);
            all
        };
        let has_all = |got: Vec<Ctl>, want: Vec<Ctl>| {
            for c in &want {
                assert!(got.contains(c), "{c:?} missing in {got:?}");
            }
            assert_eq!(got.len(), want.len(), "{got:?}");
        };
        has_all(
            shown(Tab::Find, false),
            with(&[
                Backward,
                Wrap,
                InSel,
                SelBox,
                FindNext,
                TwoButtons,
                Count,
                FindAllCur,
                FindAllOpen,
            ]),
        );
        has_all(
            shown(Tab::Find, true),
            with(&[
                Backward,
                Wrap,
                InSel,
                SelBox,
                FindPrev,
                FindNext2,
                TwoButtons,
                Count,
                FindAllCur,
                FindAllOpen,
            ]),
        );
        has_all(
            shown(Tab::Replace, false),
            with(&[
                ReplaceLabel,
                ReplaceWith,
                Backward,
                Wrap,
                InSel,
                SelBox,
                FindNext,
                TwoButtons,
                Replace,
                ReplaceAll,
                ReplaceAllOpen,
            ]),
        );
        has_all(
            shown(Tab::Files, false),
            with(&[
                ReplaceLabel,
                ReplaceWith,
                FiltersLabel,
                Filters,
                FilterTip,
                DirLabel,
                Dir,
                Browse,
                DirFromDoc,
                Sub,
                Hidden,
                FifFindAll,
                ReplaceInFiles,
            ]),
        );
        has_all(
            shown(Tab::Projects, true),
            with(&[
                ReplaceLabel,
                ReplaceWith,
                FiltersLabel,
                Filters,
                FilterTip,
                Project(0),
                Project(1),
                Project(2),
                FifFindAll,
                ReplaceInProjects,
            ]),
        );
        has_all(
            shown(Tab::Mark, false),
            with(&[
                Bookmark, Purge, Backward, Wrap, InSel, SelBox, MarkAll, ClearMarks, CopyMarked,
            ]),
        );
        let close_y = |t| frame(t, Close, false).unwrap()[1];
        assert_eq!(
            [Tab::Find, Tab::Replace, Tab::Files, Tab::Mark].map(close_y),
            [106., 99., 56., 92.]
        );
        assert_eq!(default_button(Tab::Replace, true), FindNext2);
        assert_eq!(default_button(Tab::Projects, false), FifFindAll);
        assert_eq!(default_button(Tab::Mark, true), MarkAll);
        assert_eq!(mode_enables(Mode::Regex), (false, false, true));
        assert_eq!(mode_enables(Mode::Extended), (true, true, false));
        assert_eq!(
            TABS.map(|t| t.1),
            [
                "Find",
                "Replace",
                "Find in Files",
                "Find in Projects",
                "Mark"
            ]
        );
        let en = include_str!("../../PowerEditor/installer/nativeLang/english.xml");
        let rc = include_str!("../../PowerEditor/src/ScintillaComponent/FindReplaceDlg.rc");
        let find = en.lines().find(|l| l.contains("<Find title=")).unwrap();
        for (_, t) in TABS {
            assert!(find.contains(&format!("=\"{t}\"")), "{t}");
        }
        for l in [
            "&Find what:",
            "Rep&lace with:",
            "Filter&s:",
            "Dir&ectory:",
            "Backward direction",
            "In select&ion",
            "Find All in Current &Document",
            "Replace All in All Opened Doc&uments",
            "Transparenc&y",
            "On losing focus",
        ] {
            assert!(rc.contains(&format!("\"{l}\"")), "{l}");
        }
    }

    #[test]
    fn status_line_kinds() {
        assert_eq!(status_kind(""), Status::NoMessage);
        assert_eq!(status_kind(search::END_REACHED), Status::Reached);
        assert_eq!(status_kind(search::REPLACE_TOP_REACHED), Status::Reached);
        assert_eq!(
            status_kind("Find: Can't find the text \"x\""),
            Status::NotFound
        );
        assert_eq!(status_kind(search::REPLACE_READ_ONLY), Status::NotFound);
        assert_eq!(status_kind(search::REPLACE_ALL_READ_ONLY), Status::NotFound);
        assert_eq!(
            status_kind("Find: Invalid Regular Expression\nx"),
            Status::NotFound
        );
        assert_eq!(
            status_kind("Count: 0 matches in entire file"),
            Status::Message
        );
        assert_eq!(
            status_kind("Mark: 2 matches in selected text"),
            Status::Message
        );
        assert_eq!(
            status_kind(&search::replace_in_opened_status(3)),
            Status::Message
        );
        assert_eq!(split_status("a\nb\nc"), ("a", "b\nc"));
        assert_eq!(split_status("a"), ("a", ""));
    }
}
