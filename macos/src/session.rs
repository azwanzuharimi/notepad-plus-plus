// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::Config;
use crate::config::{app_support_dir, attr};
use crate::encoding::{self, Enc};
use crate::language::{self, Entry};
use crate::{cfg, fileops, item, ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlertFirstButtonReturn, NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem,
    NSModalResponseOK, NSOpenPanel, NSSavePanel, NSTabViewItem,
};
use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use std::cell::{Cell, RefCell};
use std::io::ErrorKind;
use std::io::Write as _;
use std::path::{Path, PathBuf};

const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GETANCHOR: u32 = 2009;
const SCI_SETANCHOR: u32 = 2026;
const SCI_SETCURRENTPOS: u32 = 2141;
const SCI_GETFIRSTVISIBLELINE: u32 = 2152;
const SCI_VISIBLEFROMDOCLINE: u32 = 2220;
const SCI_DOCLINEFROMVISIBLE: u32 = 2221;
const SCI_GETWRAPMODE: u32 = 2269;
const SCI_CANCEL: u32 = 2325;
const SCI_SETXOFFSET: u32 = 2397;
const SCI_GETXOFFSET: u32 = 2398;
const SCI_CHOOSECARETX: u32 = 2399;
const SCI_SETSELECTIONMODE: u32 = 2422;
const SCI_GETSELECTIONMODE: u32 = 2423;
const SCI_SETFIRSTVISIBLELINE: u32 = 2613;

// Parameters.cpp SESSION_BACKUP_EXT.
const BAK: &str = ".inCaseOfCorruption.bak";
// NppConstants.h NB_MAX_LRF_FILE.
const MAX_RECENT: usize = 30;
// Menu tags of the recent file items: BASE + index, and BASE + 100 for the other items.
const BASE: isize = 0x4C52_0000;
const OTHER: isize = BASE + 100;

#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub filename: String,
    pub first_visible_line: isize,
    pub x_offset: isize,
    pub start_pos: isize,
    pub end_pos: isize,
    pub sel_mode: isize,
    pub lang: String,
    pub encoding: i64,
    pub read_only: bool,
}

// The files of mainView then subView; `active` is an index in `files`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Session {
    pub active: usize,
    pub files: Vec<FileInfo>,
}

fn num(e: &BytesStart, key: &str, default: i64) -> i64 {
    attr(e, key).trim().parse().unwrap_or(default)
}

// Parameters.cpp getSessionFromXmlTree; None when the root is not NotepadPlus/Session.
pub fn parse_session(xml: &str) -> Option<Session> {
    let mut r = Reader::from_str(xml);
    let (mut path, mut views): (Vec<String>, [(i64, Vec<FileInfo>); 2]) =
        (vec![], Default::default());
    let (mut found, mut active_view) = (false, 0);
    loop {
        let (e, empty) = match r.read_event().ok()? {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(_) => {
                path.pop();
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let name = e.name().as_ref().to_string();
        let at: Vec<&str> = path.iter().map(String::as_str).collect();
        match (at.as_slice(), name.as_str()) {
            (["NotepadPlus"], "Session") => {
                found = true;
                active_view = num(&e, "activeView", 0);
            }
            (["NotepadPlus", "Session"], "mainView" | "subView") => {
                views[(name == "subView") as usize].0 = num(&e, "activeIndex", 0);
            }
            (["NotepadPlus", "Session", v @ ("mainView" | "subView")], "File") => {
                let filename = attr(&e, "filename");
                if !filename.is_empty() {
                    views[(*v == "subView") as usize].1.push(FileInfo {
                        filename,
                        first_visible_line: num(&e, "firstVisibleLine", 0) as isize,
                        x_offset: num(&e, "xOffset", 0) as isize,
                        start_pos: num(&e, "startPos", 0) as isize,
                        end_pos: num(&e, "endPos", 0) as isize,
                        sel_mode: num(&e, "selMode", 0) as isize,
                        lang: attr(&e, "lang"),
                        encoding: num(&e, "encoding", -1),
                        read_only: attr(&e, "userReadOnly") == "yes",
                    });
                }
            }
            _ => {}
        }
        if !empty {
            path.push(name);
        }
    }
    let [(main_i, main), (sub_i, sub)] = views;
    let active = if active_view == 1 && !sub.is_empty() {
        main.len() + sub_i.max(0) as usize
    } else {
        main_i.max(0) as usize
    };
    found.then(|| Session {
        active,
        files: main.into_iter().chain(sub).collect(),
    })
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

// Parameters.cpp writeSession: all files go to mainView, because this app has one view.
pub fn write_session(s: &Session) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    <Session activeView=\"0\">\r\n");
    out += &format!("        <mainView activeIndex=\"{}\">\r\n", s.active);
    for f in &s.files {
        out += &format!(
            "            <File firstVisibleLine=\"{}\" xOffset=\"{}\" startPos=\"{}\" endPos=\"{}\" selMode=\"{}\" lang=\"{}\" encoding=\"{}\" userReadOnly=\"{}\" filename=\"{}\" />\r\n",
            f.first_visible_line,
            f.x_offset,
            f.start_pos,
            f.end_pos,
            f.sel_mode,
            escape(f.lang.as_str()),
            f.encoding,
            yes_no(f.read_only),
            escape(f.filename.as_str()),
        );
    }
    out + "        </mainView>\r\n        <subView activeIndex=\"0\" />\r\n    </Session>\r\n</NotepadPlus>\r\n"
}

// lastRecentFileList.cpp; `files` holds the newest file first.
#[derive(Debug, Clone, PartialEq)]
pub struct Recent {
    pub files: Vec<PathBuf>,
    pub max: usize,
    pub sub_menu: bool,
    pub custom_length: i64,
}

impl Default for Recent {
    fn default() -> Self {
        Recent {
            files: vec![],
            max: 10,
            sub_menu: false,
            custom_length: -1,
        }
    }
}

impl Recent {
    pub fn add(&mut self, p: &Path) {
        if self.max == 0 {
            return;
        }
        self.remove(p);
        self.files.insert(0, p.to_path_buf());
        self.files.truncate(self.max);
    }

    pub fn remove(&mut self, p: &Path) {
        self.files.retain(|f| !fileops::same_file(f, p));
    }
}

// Parameters.cpp feedFileListParameters; the file lists the oldest file first.
pub fn parse_history(xml: &str) -> Recent {
    let mut rec = Recent::default();
    let mut r = Reader::from_str(xml);
    let mut in_history = false;
    let mut paths = vec![];
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "History" => {
                rec.max = num(&e, "nbMaxFile", 10).clamp(0, MAX_RECENT as i64) as usize;
                rec.sub_menu = attr(&e, "inSubMenu") == "yes";
                rec.custom_length = num(&e, "customLength", -1);
                in_history = true;
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if in_history && e.name().as_ref() == "File" =>
            {
                let f = attr(&e, "filename");
                if !f.is_empty() && paths.len() < MAX_RECENT {
                    paths.push(PathBuf::from(f));
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == "History" => in_history = false,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    paths.iter().for_each(|p| rec.add(p));
    rec
}

fn history_xml(r: &Recent) -> String {
    let mut s = format!(
        "<History nbMaxFile=\"{}\" inSubMenu=\"{}\" customLength=\"{}\">\r\n",
        r.max,
        yes_no(r.sub_menu),
        r.custom_length
    );
    for f in r.files.iter().rev() {
        s += &format!(
            "        <File filename=\"{}\" />\r\n",
            escape(f.to_string_lossy().as_ref())
        );
    }
    s + "    </History>"
}

// config.xml with a new History element; the other elements stay as they are.
pub fn write_history(existing: Option<&str>, rec: &Recent) -> Result<String, String> {
    let Some(src) = existing.filter(|s| s.contains("<NotepadPlus")) else {
        return Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    {}\r\n</NotepadPlus>\r\n",
            history_xml(rec)
        ));
    };
    let mut r = Reader::from_str(src);
    let mut w = Writer::new(Vec::new());
    let (mut skip, mut depth, mut done) = (false, 0, false);
    loop {
        let ev = r.read_event().map_err(|e| e.to_string())?;
        if skip {
            match ev {
                Event::Start(_) => depth += 1,
                Event::End(_) if depth == 0 => skip = false,
                Event::End(_) => depth -= 1,
                Event::Eof => return Err("config.xml: unexpected end".into()),
                _ => {}
            }
            continue;
        }
        match &ev {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == "History" => {
                w.get_mut()
                    .write_all(history_xml(rec).as_bytes())
                    .map_err(|e| e.to_string())?;
                done = true;
                skip = matches!(ev, Event::Start(_));
                depth = 0;
                continue;
            }
            Event::End(e) if e.name().as_ref() == "NotepadPlus" && !done => {
                w.get_mut()
                    .write_all(format!("    {}\r\n", history_xml(rec)).as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Event::Eof => break,
            _ => {}
        }
        w.write_event(ev).map_err(|e| e.to_string())?;
    }
    String::from_utf8(w.into_inner()).map_err(|e| e.to_string())
}

// Common.cpp BuildMenuFileName with the full path; macOS menus have no mnemonic.
pub fn menu_title(i: usize, p: &Path) -> String {
    format!("{}: {}", i + 1, p.display())
}

// Load Session... and Save Session..., after Move to Trash as in Notepad_plus.rc.
pub fn session_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    vec![
        NSMenuItem::separatorItem(mtm),
        item(mtm, "Load Session...", sel!(loadSession:), "", t),
        item(mtm, "Save Session...", sel!(saveSession:), "", t),
    ]
}

#[derive(Default)]
struct State {
    recent: RefCell<Option<Recent>>,
    config_error: RefCell<Option<String>>,
    no_session: Cell<bool>,
}

thread_local! {
    static S: State = State::default();
}

fn load_recent() -> Recent {
    match app_support_dir().map(|d| read_file(&d.join("config.xml"))) {
        Some(Err(e)) => {
            S.with(|s| *s.config_error.borrow_mut() = Some(e));
            Recent::default()
        }
        Some(Ok(Some(x))) => parse_history(&x),
        _ => Recent::default(),
    }
}

// The list loads from config.xml at the first use, also when a file opens before the launch ends.
fn with_recent<R>(f: impl FnOnce(&mut Recent) -> R) -> R {
    S.with(|s| f(s.recent.borrow_mut().get_or_insert_with(load_recent)))
}

fn recent() -> Recent {
    with_recent(|r| r.clone())
}

// The (menu text, language name) pairs of the Language menu.
fn lang_pairs(c: &Config) -> Vec<(String, String)> {
    language::menu_entries(c)
        .into_iter()
        .flat_map(|e| match e {
            Entry::Item(t, n) => vec![(t, n)],
            Entry::Group(_, v) => v,
            Entry::Separator => vec![],
        })
        .collect()
}

// Notepad_plus.cpp getLangFromMenu: the session stores the Language menu text.
pub fn lang_menu_text(c: &Config, name: &str) -> String {
    lang_pairs(c)
        .into_iter()
        .find(|(_, n)| n == name)
        .map_or(name.to_string(), |(t, _)| t)
}

// NppIO.cpp loadSession getLangFromMenuName; a language name of this app also matches.
pub fn lang_from_menu_text(c: &Config, text: &str) -> Option<String> {
    lang_pairs(c)
        .into_iter()
        .find(|(t, _)| t == text)
        .map(|(_, n)| n)
        .or_else(|| {
            c.languages
                .iter()
                .find(|l| l.name == text)
                .map(|l| l.name.clone())
        })
}

fn file_menu(mtm: MainThreadMarker) -> Option<Retained<NSMenu>> {
    NSApplication::sharedApplication(mtm)
        .mainMenu()?
        .itemWithTitle(&ns("File"))?
        .submenu()
}

// Parameters.cpp writeSession: an optional copy of the old file, then a write through a temporary file.
fn write_file(path: &Path, text: &str, backup: bool) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let with = |ext: &str| PathBuf::from(format!("{}{ext}", path.display()));
    std::fs::create_dir_all(path.parent().unwrap_or(path)).map_err(err)?;
    if backup && path.exists() {
        std::fs::copy(path, with(BAK)).map_err(err)?;
    }
    std::fs::write(with(".tmp"), text).map_err(err)?;
    std::fs::rename(with(".tmp"), path).map_err(err)
}

// None when the file does not exist; an error when it exists but cannot be read as UTF-8.
fn read_file(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

// Parameters.cpp load: when session.xml does not load, the backup copy replaces it.
fn read_session(path: &Path) -> Option<Session> {
    let s = read_file(path)
        .ok()
        .flatten()
        .and_then(|x| parse_session(&x));
    if s.is_some() {
        return s;
    }
    let bak = PathBuf::from(format!("{}{BAK}", path.display()));
    let s = std::fs::read_to_string(&bak)
        .ok()
        .and_then(|x| parse_session(&x))?;
    let _ = std::fs::rename(&bak, path);
    Some(s)
}

impl App {
    // Startup: read the recent files, then open the last session unless -nosession is given.
    pub(crate) fn start_session(&self) {
        self.refresh_recent_menu();
        if let Some(e) = S.with(|s| s.config_error.borrow().clone()) {
            let info = format!("{e}\n\nThe recent files list is not saved when the app quits.");
            self.alert("Cannot read config.xml", &info, &["OK"]);
        }
        let no_session = std::env::args_os().any(|a| a == "-nosession");
        S.with(|s| s.no_session.set(no_session));
        let session = app_support_dir()
            .filter(|_| !no_session)
            .and_then(|d| read_session(&d.join("session.xml")));
        if let Some(s) = session {
            let opened = self.tab_view().selectedTabViewItem();
            self.load_session(&s);
            if let Some(item) = opened {
                self.tab_view().selectTabViewItem(Some(&item));
            }
        }
    }

    // Quit: write session.xml and the History of config.xml.
    pub(crate) fn save_on_quit(&self, s: &Session) {
        let Some(dir) = app_support_dir() else {
            return;
        };
        let mut errors = vec![];
        if !S.with(|s| s.no_session.get()) {
            errors.extend(write_file(&dir.join("session.xml"), &write_session(s), true).err());
        }
        if S.with(|s| s.config_error.borrow().is_none()) {
            let path = dir.join("config.xml");
            let rec = recent();
            errors.extend(
                read_file(&path)
                    .and_then(|old| write_history(old.as_deref(), &rec))
                    .and_then(|x| write_file(&path, &x, false))
                    .err(),
            );
        }
        if !errors.is_empty() {
            self.alert(
                "Cannot save the session or the recent files list",
                &errors.join("\n"),
                &["OK"],
            );
        }
    }

    // Notepad_plus.cpp getCurrentOpenedFiles: the tabs that have a file, with their positions.
    pub(crate) fn current_session(&self, only_existing: bool) -> Session {
        let tabs = self.ivars().tabs.borrow().clone();
        let cur = self.current();
        let mut s = Session::default();
        for (i, t) in tabs.iter().enumerate() {
            let Some(p) = t.path.as_deref().filter(|p| !only_existing || p.exists()) else {
                continue;
            };
            if Some(i) == cur {
                s.active = s.files.len();
            }
            let v = &t.view;
            let get = |m| sci::send(v, m, 0, 0);
            s.files.push(FileInfo {
                filename: p.to_string_lossy().into_owned(),
                first_visible_line: sci::send(
                    v,
                    SCI_DOCLINEFROMVISIBLE,
                    get(SCI_GETFIRSTVISIBLELINE) as usize,
                    0,
                ),
                x_offset: get(SCI_GETXOFFSET),
                start_pos: get(SCI_GETANCHOR),
                end_pos: get(SCI_GETCURRENTPOS),
                sel_mode: get(SCI_GETSELECTIONMODE),
                lang: lang_menu_text(
                    cfg(),
                    language::tab_language(t).map_or("normal", |l| l.name.as_str()),
                ),
                encoding: match t.enc {
                    Enc::Cp(cp) => cp as i64,
                    _ => -1,
                },
                read_only: t.ro,
            });
        }
        s
    }

    // NppIO.cpp loadSession: missing files are skipped; a character set is used again if the file has no BOM.
    pub(crate) fn load_session(&self, s: &Session) {
        let mut active: Option<Retained<NSTabViewItem>> = None;
        for (k, f) in s.files.iter().enumerate() {
            let p = PathBuf::from(&f.filename);
            if !p.is_file() {
                continue;
            }
            let was_open = self.find_open(&p, None).is_some();
            self.open_path(&p);
            let Some(i) = self.find_open(&p, None) else {
                continue;
            };
            let enc = u32::try_from(f.encoding)
                .ok()
                .filter(|&cp| encoding::supported(cp))
                .map(Enc::Cp);
            if let (Some(e), false) = (enc, was_open) {
                if let Some(b) = std::fs::read(&p)
                    .ok()
                    .filter(|b| encoding::bom(b).is_none())
                {
                    self.load_into(i, &b, e);
                }
            }
            let detected = self
                .tab(i)
                .and_then(|t| language::tab_language(&t))
                .map(|l| l.name.clone());
            if let Some(name) =
                lang_from_menu_text(cfg(), &f.lang).filter(|n| Some(n) != detected.as_ref())
            {
                if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
                    t.lang = Some(name);
                }
                self.apply_tab_language(i);
            }
            if f.read_only {
                if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
                    t.ro = true;
                }
            }
            if let Some(t) = self.tab(i) {
                restore_position(&t.view, f);
                if k == s.active || active.is_none() {
                    active = Some(t.item);
                }
            }
        }
        if let Some(item) = active {
            self.tab_view().selectTabViewItem(Some(&item));
        }
    }

    pub(crate) fn load_session_file(&self) {
        let p = NSOpenPanel::openPanel(self.mtm());
        p.setTitle(Some(&ns("Load Session")));
        if p.runModal() != NSModalResponseOK {
            return;
        }
        let Some(path) = p.URL().and_then(|u| u.path()) else {
            return;
        };
        match std::fs::read_to_string(path.to_string())
            .ok()
            .and_then(|x| parse_session(&x))
        {
            Some(s) => self.load_session(&s),
            None => {
                self.alert(
                    "Could not Load Session",
                    "Session file is either corrupted or not valid.",
                    &["OK"],
                );
            }
        }
    }

    pub(crate) fn save_session_file(&self) {
        let p = NSSavePanel::savePanel(self.mtm());
        p.setTitle(Some(&ns("Save Session")));
        if p.runModal() != NSModalResponseOK {
            return;
        }
        let Some(path) = p.URL().and_then(|u| u.path()) else {
            return;
        };
        let path = path.to_string();
        if let Err(e) = std::fs::write(&path, write_session(&self.current_session(true))) {
            self.alert(&format!("Cannot save {path}"), &e.to_string(), &["OK"]);
        }
    }

    pub(crate) fn recent_add(&self, p: &Path) {
        with_recent(|r| r.add(p));
        self.refresh_recent_menu();
    }

    pub(crate) fn recent_remove(&self, p: &Path) {
        with_recent(|r| r.remove(p));
        self.refresh_recent_menu();
    }

    // NppIO.cpp doClose: a closed file that still exists goes to the top of the list.
    pub(crate) fn recent_closed(&self, item: &NSTabViewItem) {
        let t = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .find(|t| std::ptr::eq(&*t.item, item))
            .cloned();
        if let Some(p) = t.and_then(|t| t.path).filter(|p| p.exists()) {
            self.recent_add(&p);
        }
    }

    // NppIO.cpp fileSaveAs: the old file goes to the list, and the new file leaves it.
    pub(crate) fn recent_saved_as(&self, old: Option<&Path>, new: &Path) {
        if let Some(o) = old {
            self.recent_add(o);
        }
        self.recent_remove(new);
    }

    // NppIO.cpp doOpen: a missing file leaves the list, then Notepad++ offers to create it.
    pub(crate) fn open_recent(&self, p: &Path) {
        self.recent_remove(p);
        if !p.exists() && self.find_open(p, None).is_none() {
            let dir = p.parent().unwrap_or(p);
            if !dir.is_dir() {
                let msg = format!(
                    "\"{}\" cannot be opened:\nFolder \"{}\" doesn't exist.",
                    p.display(),
                    dir.display()
                );
                self.alert("Cannot open file", &msg, &["OK"]);
                return;
            }
            let ask = format!("\"{}\" doesn't exist. Create it?", p.display());
            if self.alert("Create new file", &ask, &["Yes", "No"]) != NSAlertFirstButtonReturn {
                return;
            }
            let created = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(p);
            if created.is_err() {
                self.alert(
                    "Create new file",
                    &format!("Cannot create the file \"{}\".", p.display()),
                    &["OK"],
                );
                return;
            }
        }
        self.open_path(p);
    }

    pub(crate) fn open_recent_index(&self, tag: isize) {
        if let Some(p) = recent().files.get((tag - BASE) as usize) {
            self.open_recent(p);
        }
    }

    pub(crate) fn restore_recent_closed(&self) {
        if let Some(p) = recent().files.first() {
            self.open_recent(p);
        }
    }

    // NppCommands.cpp IDM_OPEN_ALL_RECENT_FILE: the oldest file opens first.
    pub(crate) fn open_all_recent(&self) {
        for p in recent().files.iter().rev() {
            self.open_recent(p);
        }
    }

    pub(crate) fn empty_recent(&self) {
        with_recent(|r| r.files.clear());
        self.refresh_recent_menu();
    }

    // lastRecentFileList.cpp updateMenu: the recent items go at the end of the File menu.
    fn refresh_recent_menu(&self) {
        let mtm = self.mtm();
        let Some(m) = file_menu(mtm) else { return };
        for i in (0..m.numberOfItems()).rev() {
            if m.itemAtIndex(i)
                .is_some_and(|it| (BASE..=OTHER).contains(&it.tag()))
            {
                m.removeItemAtIndex(i);
            }
        }
        let rec = recent();
        if rec.files.is_empty() {
            return;
        }
        let t: Option<&AnyObject> = Some(self);
        let tag = |i: Retained<NSMenuItem>, n| {
            i.setTag(n);
            i
        };
        let mut files: Vec<_> = rec
            .files
            .iter()
            .enumerate()
            .map(|(k, p)| {
                tag(
                    item(mtm, &menu_title(k, p), sel!(openRecentFile:), "", t),
                    BASE + k as isize,
                )
            })
            .collect();
        let restore = item(
            mtm,
            "Restore Recent Closed File",
            sel!(restoreRecentClosed:),
            "t",
            t,
        );
        restore.setKeyEquivalentModifierMask(
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
        );
        files.extend([
            NSMenuItem::separatorItem(mtm),
            restore,
            item(mtm, "Open All Recent Files", sel!(openAllRecent:), "", t),
            item(mtm, "Empty Recent Files List", sel!(emptyRecent:), "", t),
        ]);
        let mut top = vec![NSMenuItem::separatorItem(mtm)];
        if rec.sub_menu {
            top.push(crate::nested(mtm, "Recent Files", files));
        } else {
            top.extend(files);
        }
        for i in top {
            if !(BASE..OTHER).contains(&i.tag()) {
                i.setTag(OTHER);
            }
            m.addItem(&i);
        }
    }
}

// ScintillaEditView.cpp restoreCurrentPosPreStep; the wrap post step is not done.
fn restore_position(v: &objc2_app_kit::NSView, f: &FileInfo) {
    let set = |m, w: isize| sci::send(v, m, w as usize, 0);
    set(SCI_SETSELECTIONMODE, f.sel_mode);
    set(SCI_SETANCHOR, f.start_pos);
    set(SCI_SETCURRENTPOS, f.end_pos);
    set(SCI_CANCEL, 0);
    if set(SCI_GETWRAPMODE, 0) == 0 {
        set(SCI_SETXOFFSET, f.x_offset);
    }
    set(SCI_CHOOSECARETX, 0);
    let line = set(SCI_VISIBLEFROMDOCLINE, f.first_visible_line);
    set(SCI_SETFIRSTVISIBLELINE, line);
    if f.read_only {
        sci::set_read_only(v, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str) -> FileInfo {
        FileInfo {
            filename: name.into(),
            first_visible_line: 0,
            x_offset: 0,
            start_pos: 0,
            end_pos: 0,
            sel_mode: 0,
            lang: String::new(),
            encoding: -1,
            read_only: false,
        }
    }

    #[test]
    fn session_round_trip() {
        let s = Session {
            active: 1,
            files: vec![
                FileInfo {
                    first_visible_line: 12,
                    x_offset: 40,
                    start_pos: 100,
                    end_pos: 120,
                    sel_mode: 1,
                    lang: "python".into(),
                    encoding: 1251,
                    read_only: true,
                    ..info("/tmp/a & \"b\" <c>.py")
                },
                info("/tmp/x.txt"),
            ],
        };
        let x = write_session(&s);
        assert!(x.contains("filename=\"/tmp/a &amp; &quot;b&quot; &lt;c&gt;.py\""));
        assert!(x.contains("userReadOnly=\"yes\""));
        assert_eq!(parse_session(&x), Some(s));
    }

    #[test]
    fn windows_session() {
        let x = r#"<?xml version="1.0" encoding="UTF-8" ?>
<NotepadPlus>
    <Session activeView="1">
        <mainView activeIndex="0">
            <File firstVisibleLine="3" xOffset="0" scrollWidth="1792" startPos="55" endPos="60" selMode="0" offset="0" wrapCount="1" lang="C++" encoding="-1" userReadOnly="no" filename="C:\src\main.cpp" backupFilePath="" originalFileLastModifTimestamp="0" originalFileLastModifTimestampHigh="0" tabColourId="-1" RTL="no" tabPinned="no">
                <Mark line="4" />
                <Fold line="10" />
            </File>
            <File filename="" />
        </mainView>
        <subView activeIndex="1">
            <File startPos="7" endPos="7" lang="Normal text" encoding="932" filename="D:\notes.txt" />
            <File filename="D:\b.txt" userReadOnly="yes" />
        </subView>
    </Session>
</NotepadPlus>"#;
        let s = parse_session(x).unwrap();
        let names: Vec<_> = s.files.iter().map(|f| f.filename.as_str()).collect();
        assert_eq!(names, ["C:\\src\\main.cpp", "D:\\notes.txt", "D:\\b.txt"]);
        assert_eq!(s.active, 2);
        assert_eq!(
            (
                s.files[0].first_visible_line,
                s.files[0].start_pos,
                s.files[0].end_pos
            ),
            (3, 55, 60)
        );
        assert_eq!(s.files[0].lang, "C++");
        assert_eq!(s.files[1].encoding, 932);
        assert_eq!(s.files[2].encoding, -1);
        assert!(s.files[2].read_only && !s.files[0].read_only);
        assert_eq!(
            parse_session("<NotepadPlus><GUIConfigs/></NotepadPlus>"),
            None
        );
        assert_eq!(parse_session("not xml <<"), None);
        let empty = parse_session("<NotepadPlus><Session activeView=\"0\"><mainView activeIndex=\"0\" /><subView activeIndex=\"0\" /></Session></NotepadPlus>");
        assert_eq!(empty, Some(Session::default()));
    }

    fn tmp(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        d
    }

    #[test]
    fn recent_add_move_trim_dedupe() {
        let d = tmp("session-recent");
        let mut r = Recent {
            max: 3,
            ..Recent::default()
        };
        for n in ["a", "b", "c"] {
            r.add(&d.join(n));
        }
        assert_eq!(r.files, [d.join("c"), d.join("b"), d.join("a")]);
        r.add(&d.join("sub/../a"));
        assert_eq!(r.files, [d.join("sub/../a"), d.join("c"), d.join("b")]);
        r.add(&d.join("e"));
        assert_eq!(r.files, [d.join("e"), d.join("sub/../a"), d.join("c")]);
        r.remove(&d.join("./c"));
        assert_eq!(r.files, [d.join("e"), d.join("sub/../a")]);
        let mut none = Recent {
            max: 0,
            ..Recent::default()
        };
        none.add(&d.join("a"));
        assert!(none.files.is_empty());
    }

    #[test]
    fn history_read_and_write() {
        let x = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    <GUIConfigs>\r\n        <GUIConfig name=\"TabBar\" dragAndDrop=\"yes\" />\r\n    </GUIConfigs>\r\n    <History nbMaxFile=\"2\" inSubMenu=\"yes\" customLength=\"-1\">\r\n        <File filename=\"/old.txt\" />\r\n        <File filename=\"/mid.txt\" />\r\n        <File filename=\"/new.txt\" />\r\n    </History>\r\n</NotepadPlus>\r\n";
        let r = parse_history(x);
        assert_eq!(
            r.files,
            [PathBuf::from("/new.txt"), PathBuf::from("/mid.txt")]
        );
        assert_eq!((r.max, r.sub_menu), (2, true));
        let out = write_history(Some(x), &r).unwrap();
        assert!(out.contains("<GUIConfig name=\"TabBar\" dragAndDrop=\"yes\" />"));
        assert!(!out.contains("old.txt"));
        assert_eq!(out.matches("<History").count(), 1);
        assert!(out.find("/mid.txt").unwrap() < out.find("/new.txt").unwrap());
        assert_eq!(parse_history(&out), r);
        let fresh = write_history(None, &Recent::default()).unwrap();
        assert!(fresh.contains("<History nbMaxFile=\"10\" inSubMenu=\"no\" customLength=\"-1\">"));
        let added = write_history(Some("<NotepadPlus><FindHistory /></NotepadPlus>"), &r).unwrap();
        assert!(added.contains("<FindHistory />") && added.contains("<History nbMaxFile=\"2\""));
        assert_eq!(
            parse_history("<NotepadPlus><History nbMaxFile=\"99\" /></NotepadPlus>").max,
            30
        );
        assert_eq!(parse_history("").max, 10);
    }

    #[test]
    fn settings_files() {
        let d = tmp("session-files");
        let path = d.join("session.xml");
        assert_eq!(read_file(&path), Ok(None));
        std::fs::write(d.join("utf16.xml"), [0xFF, 0xFE, b'<', 0]).unwrap();
        assert!(read_file(&d.join("utf16.xml")).is_err());
        let one = Session {
            active: 0,
            files: vec![info("/a.txt")],
        };
        write_file(&path, &write_session(&one), true).unwrap();
        assert!(!d.join("session.xml.inCaseOfCorruption.bak").exists());
        write_file(&path, &write_session(&Session::default()), true).unwrap();
        let bak = d.join("session.xml.inCaseOfCorruption.bak");
        assert_eq!(read_session(&bak), Some(one.clone()));
        assert!(!d.join("session.xml.tmp").exists());
        std::fs::write(&path, "<NotepadPlus><Sess").unwrap();
        assert_eq!(read_session(&path), Some(one.clone()));
        assert!(!bak.exists());
        assert_eq!(read_session(&path), Some(one));
        assert_eq!(read_session(&d.join("none.xml")), None);
    }

    #[test]
    fn session_lang_names() {
        let c = crate::config::load();
        assert_eq!(lang_menu_text(&c, "cpp"), "C++");
        assert_eq!(lang_menu_text(&c, "python"), "Python");
        assert_eq!(lang_menu_text(&c, "normal"), "None (Normal Text)");
        assert_eq!(lang_from_menu_text(&c, "C++").as_deref(), Some("cpp"));
        assert_eq!(
            lang_from_menu_text(&c, "None (Normal Text)").as_deref(),
            Some("normal")
        );
        assert_eq!(lang_from_menu_text(&c, "python").as_deref(), Some("python"));
        assert_eq!(lang_from_menu_text(&c, "My UDL"), None);
        assert_eq!(lang_from_menu_text(&c, ""), None);
    }

    #[test]
    fn menu_titles() {
        assert_eq!(menu_title(0, Path::new("/a/b.txt")), "1: /a/b.txt");
        assert_eq!(menu_title(9, Path::new("/c")), "10: /c");
    }
}
