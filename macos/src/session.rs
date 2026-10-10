// SPDX-License-Identifier: GPL-3.0-or-later
use crate::backup::{self, Restore};
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
    pub backup_file_path: String,
    pub original_timestamp: u64,
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

// A FILETIME half, as NppXml::uint64Attribute then DWORD.
fn dword(e: &BytesStart, key: &str) -> u64 {
    attr(e, key).trim().parse::<u64>().unwrap_or(0) & 0xFFFF_FFFF
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
                        backup_file_path: attr(&e, "backupFilePath"),
                        original_timestamp: dword(&e, "originalFileLastModifTimestamp")
                            | dword(&e, "originalFileLastModifTimestampHigh") << 32,
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
            "            <File firstVisibleLine=\"{}\" xOffset=\"{}\" startPos=\"{}\" endPos=\"{}\" selMode=\"{}\" lang=\"{}\" encoding=\"{}\" userReadOnly=\"{}\" filename=\"{}\" backupFilePath=\"{}\" originalFileLastModifTimestamp=\"{}\" originalFileLastModifTimestampHigh=\"{}\" />\r\n",
            f.first_visible_line,
            f.x_offset,
            f.start_pos,
            f.end_pos,
            f.sel_mode,
            escape(f.lang.as_str()),
            f.encoding,
            yes_no(f.read_only),
            escape(f.filename.as_str()),
            escape(f.backup_file_path.as_str()),
            f.original_timestamp & 0xFFFF_FFFF,
            f.original_timestamp >> 32,
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
    replace_element(existing, "History", &history_xml(rec))
}

// config.xml with the element `name` replaced by `xml`, or added at the end; the other elements stay as they are.
pub(crate) fn replace_element(existing: Option<&str>, name: &str, xml: &str) -> Result<String, String> {
    let Some(src) = existing.filter(|s| s.contains("<NotepadPlus")) else {
        return Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    {xml}\r\n</NotepadPlus>\r\n"
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
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == name => {
                w.get_mut()
                    .write_all(xml.as_bytes())
                    .map_err(|e| e.to_string())?;
                done = true;
                skip = matches!(ev, Event::Start(_));
                depth = 0;
                continue;
            }
            Event::End(e) if e.name().as_ref() == "NotepadPlus" && !done => {
                w.get_mut()
                    .write_all(format!("    {xml}\r\n").as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Event::Eof => break,
            _ => {}
        }
        w.write_event(ev).map_err(|e| e.to_string())?;
    }
    let out = String::from_utf8(w.into_inner()).map_err(|e| e.to_string())?;
    Ok(crate::prefs::keep_bom(src, out))
}

// Common.cpp BuildMenuFileName: `len` < 0 is the full path, 0 the file name, else a compacted path; macOS menus have no mnemonic.
pub fn menu_title(i: usize, p: &Path, len: i64) -> String {
    let full = p.to_string_lossy();
    let name = match len {
        0 => p.file_name().map_or(full.clone(), |n| n.to_string_lossy()).into_owned(),
        n if n > 0 => compact_path(&full, n as usize),
        _ => full.into_owned(),
    };
    let chars: Vec<char> = name.chars().collect();
    // Common.cpp MAX_PATH trimming of a long name.
    let name = if len <= 0 && chars.len() >= 260 {
        let head: String = chars[..127].iter().collect();
        let tail: String = chars[chars.len() - 130..].iter().collect();
        format!("{head}...{tail}")
    } else {
        name
    };
    format!("{}: {name}", i + 1)
}

// PathCompactPathEx: the start of the path, "...", then the file name, in at most `max` characters.
pub fn compact_path(path: &str, max: usize) -> String {
    let c: Vec<char> = path.chars().collect();
    if c.len() <= max {
        return path.to_string();
    }
    let file = c.iter().rposition(|&x| x == '/').unwrap_or(0);
    let tail = &c[file..];
    if tail.len() + 3 > max {
        let name = &c[(file + 1).min(c.len())..];
        let keep = max.saturating_sub(3).min(name.len());
        return name[..keep].iter().collect::<String>() + "...";
    }
    let keep = max - 3 - tail.len();
    c[..keep].iter().collect::<String>() + "..." + &tail.iter().collect::<String>()
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

// The text of config.xml; a read error is kept, and then the app does not write config.xml.
pub(crate) fn read_config() -> Option<String> {
    match app_support_dir().map(|d| read_file(&d.join("config.xml"))) {
        Some(Err(e)) => {
            S.with(|s| *s.config_error.borrow_mut() = Some(e));
            None
        }
        Some(Ok(x)) => x,
        None => None,
    }
}

// Writes the History and the settings to config.xml; nothing is written when config.xml could not be read.
pub(crate) fn save_config() -> Result<(), String> {
    let Some(dir) = app_support_dir() else {
        return Ok(());
    };
    if S.with(|s| s.config_error.borrow().is_some()) {
        return Ok(());
    }
    let path = dir.join("config.xml");
    let rec = recent();
    read_file(&path)
        .and_then(|old| write_history(old.as_deref(), &rec))
        .and_then(|x| crate::prefs::patch_config(Some(&x)))
        .and_then(|x| crate::filebrowser::patch_config(&x))
        .and_then(|x| write_file(&path, &x, false))
}

// Parameters.cpp feedFileListParameters; with CheckHistoryFiles, missing files leave the list at launch.
fn load_recent() -> Recent {
    let mut r = read_config().map_or_else(Recent::default, |x| parse_history(&x));
    if crate::prefs::get().check_history_files {
        r.files.retain(|p| p.exists());
    }
    r
}

// Started with -nosession, or session.xml cannot be read and is kept as it is.
pub fn no_session() -> bool {
    S.with(|s| s.no_session.get())
}

// The list loads from config.xml at the first use, also when a file opens before the launch ends.
fn with_recent<R>(f: impl FnOnce(&mut Recent) -> R) -> R {
    S.with(|s| f(s.recent.borrow_mut().get_or_insert_with(load_recent)))
}

fn recent() -> Recent {
    with_recent(|r| r.clone())
}

// The (menu text, language name) pairs of the Language menu.
pub(crate) fn lang_pairs(c: &Config) -> Vec<(String, String)> {
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
pub(crate) fn write_file(path: &Path, text: &str, backup: bool) -> Result<(), String> {
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
pub(crate) fn read_file(path: &Path) -> Result<Option<String>, String> {
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
            let info = format!("{e}\n\nThe settings and the recent files list are not saved.");
            self.alert("Cannot read config.xml", &info, &["OK"]);
        }
        let p = crate::prefs::get();
        let no_session = std::env::args_os().any(|a| a == "-nosession") || !p.remember_session;
        S.with(|s| s.no_session.set(no_session));
        let path = app_support_dir()
            .filter(|_| !no_session)
            .map(|d| d.join("session.xml"));
        let session = path.as_deref().and_then(read_session);
        if let (None, Some(p)) = (&session, path.filter(|p| p.exists())) {
            self.keep_unreadable_session(&p);
        }
        if let Some(s) = session {
            let opened = self.tab_view().selectedTabViewItem();
            let tabs = self.ivars().tabs.borrow().clone();
            let blank: Vec<_> = tabs
                .iter()
                .filter(|t| t.path.is_none() && !self.dirty(t) && sci::length(&t.view) == 0)
                .map(|t| t.item.clone())
                .collect();
            self.load_session(&s, backup::snapshot_on());
            if self.ivars().tabs.borrow().len() > tabs.len() {
                self.drop_tabs(&blank);
            }
            let top = self
                .ivars()
                .tabs
                .borrow()
                .iter()
                .filter(|t| t.path.is_none())
                .filter_map(|t| backup::untitled_number(&t.name))
                .max();
            let n = &self.ivars().untitled;
            n.set(n.get().max(top.unwrap_or(0)));
            if let Some(item) = opened.filter(|o| !blank.contains(o)) {
                self.tab_view().selectTabViewItem(Some(&item));
            }
            // Notepad_plus_Window.cpp: addNewDocumentOnStartup opens a new document after the session.
            if p.new_doc_on_startup && !self.ivars().tabs.borrow().is_empty() {
                self.add_tab(None, Enc::Utf8, b"", false);
            }
        }
        self.start_backups();
    }

    // An unreadable session.xml moves to session.xml.unreadable; if it cannot move, no session is written.
    fn keep_unreadable_session(&self, p: &Path) {
        let to = PathBuf::from(format!("{}.unreadable", p.display()));
        let info = if !to.exists() && std::fs::rename(p, &to).is_ok() {
            format!("The file is kept as {}.", to.display())
        } else {
            S.with(|s| s.no_session.set(true));
            "The session is not saved when the app quits.".to_string()
        };
        self.alert(&format!("Cannot read {}", p.display()), &info, &["OK"]);
    }

    // Quit: write session.xml (when RememberLastSession is on), then the History and the settings of config.xml.
    pub(crate) fn save_on_quit(&self, s: &Session) {
        let Some(dir) = app_support_dir() else {
            return;
        };
        let mut errors = vec![];
        if !S.with(|s| s.no_session.get()) && crate::prefs::get().remember_session {
            errors.extend(write_file(&dir.join("session.xml"), &write_session(s), true).err());
        }
        errors.extend(save_config().err());
        if !errors.is_empty() {
            self.alert(
                "Cannot save the session or the recent files list",
                &errors.join("\n"),
                &["OK"],
            );
        }
    }

    // Notepad_plus.cpp getCurrentOpenedFiles: the tabs with their positions; untitled tabs with text unless `only_existing`.
    pub(crate) fn current_session(&self, only_existing: bool) -> Session {
        let tabs = self.ivars().tabs.borrow().clone();
        let cur = self.current();
        let mut s = Session::default();
        for (i, t) in tabs.iter().enumerate() {
            let filename = match t.path.as_deref() {
                Some(p) if !only_existing || p.exists() => p.to_string_lossy().into_owned(),
                None if !only_existing && sci::length(&t.view) > 0 => t.name.clone(),
                _ => continue,
            };
            if Some(i) == cur {
                s.active = s.files.len();
            }
            let v = &t.view;
            let get = |m| sci::send(v, m, 0, 0);
            let b = backup::entry(v);
            s.files.push(FileInfo {
                filename,
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
                lang: self.udl_session_name(t).unwrap_or_else(|| {
                    lang_menu_text(
                        cfg(),
                        language::tab_language(t).map_or("normal", |l| l.name.as_str()),
                    )
                }),
                encoding: match t.enc {
                    Enc::Cp(cp) => cp as i64,
                    _ => -1,
                },
                read_only: t.ro,
                backup_file_path: b
                    .as_ref()
                    .map_or(String::new(), |b| b.path.to_string_lossy().into_owned()),
                original_timestamp: match (&b, t.mtime) {
                    (Some(_), Some(m)) => backup::filetime(m),
                    _ => 0,
                },
            });
        }
        s
    }

    // NppIO.cpp loadSession: missing files are skipped; in snapshot mode a backup opens as a modified tab.
    pub(crate) fn load_session(&self, s: &Session, snapshot: bool) {
        let mut active: Option<Retained<NSTabViewItem>> = None;
        let mut changed = vec![];
        for (k, f) in s.files.iter().enumerate() {
            let p = PathBuf::from(&f.filename);
            let bak = snapshot.then(|| self.stored_backup(f)).flatten();
            let from_file = |this: &Self| this.open_session_file(&p, f);
            let r = backup::restore(backup::mtime(&p), bak.is_some(), f.original_timestamp);
            let i = match r {
                Restore::Skip => None,
                Restore::File => from_file(self),
                Restore::Backup { changed: c } => {
                    match bak.and_then(|b| self.restore_backup(f, &p, &b)) {
                        Some(i) => {
                            if c {
                                changed.extend(self.tab(i).map(|t| t.item));
                            }
                            Some(i)
                        }
                        None if p.is_file() => from_file(self),
                        None => None,
                    }
                }
            };
            let Some(i) = i else { continue };
            let detected = self
                .tab(i)
                .and_then(|t| language::tab_language(&t))
                .map(|l| l.name.clone());
            let udl = self.restore_udl(i, &f.lang);
            if let Some(name) = lang_from_menu_text(cfg(), &f.lang)
                .filter(|n| !udl && Some(n) != detected.as_ref())
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
        for item in &changed {
            self.ask_reload_restored(item);
        }
        if let Some(item) = active {
            self.tab_view().selectTabViewItem(Some(&item));
        }
    }

    // A character set is used again if the file has no BOM.
    fn open_session_file(&self, p: &Path, f: &FileInfo) -> Option<usize> {
        let was_open = self.find_open(p, None).is_some();
        self.open_path(p);
        let i = self.find_open(p, None)?;
        let enc = u32::try_from(f.encoding)
            .ok()
            .filter(|&cp| encoding::supported(cp))
            .map(Enc::Cp);
        if let (Some(e), false) = (enc, was_open) {
            if let Some(b) = std::fs::read(p).ok().filter(|b| encoding::bom(b).is_none()) {
                self.load_into(i, &b, e);
            }
        }
        Some(i)
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
            Some(s) => self.load_session(&s, false),
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

    // Preferences > Recent Files History: (max. number of entries, in submenu, customLength).
    pub(crate) fn recent_options(&self) -> (usize, bool, i64) {
        with_recent(|r| (r.max, r.sub_menu, r.custom_length))
    }

    pub(crate) fn set_recent_options(&self, max: usize, sub_menu: bool, custom_length: i64) {
        with_recent(|r| {
            r.max = max.min(MAX_RECENT);
            r.sub_menu = sub_menu;
            r.custom_length = custom_length;
            r.files.truncate(r.max);
        });
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
                    item(mtm, &menu_title(k, p, rec.custom_length), sel!(openRecentFile:), "", t),
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
            backup_file_path: String::new(),
            original_timestamp: 0,
        }
    }

    #[test]
    fn session_backup_attributes() {
        let s = Session {
            active: 1,
            files: vec![
                FileInfo {
                    backup_file_path: "/u/backup/a.txt@2026-10-10_134501".into(),
                    original_timestamp: 0x01DC_3A2B_9F8E_7D6C,
                    ..info("/w/a.txt")
                },
                FileInfo {
                    backup_file_path: "/u/backup/new 1@2026-10-10_134507".into(),
                    ..info("new 1")
                },
            ],
        };
        let x = write_session(&s);
        assert!(x.contains("filename=\"/w/a.txt\" backupFilePath=\"/u/backup/a.txt@2026-10-10_134501\" originalFileLastModifTimestamp=\"2676915564\" originalFileLastModifTimestampHigh=\"31210027\""));
        assert!(x.contains("filename=\"new 1\" backupFilePath=\"/u/backup/new 1@2026-10-10_134507\" originalFileLastModifTimestamp=\"0\" originalFileLastModifTimestampHigh=\"0\""));
        assert_eq!(parse_session(&x), Some(s));
        let win = r#"<NotepadPlus><Session activeView="0"><mainView activeIndex="0">
            <File filename="new 2" backupFilePath="C:\Users\me\AppData\Roaming\Notepad++\backup\new 2@2024-01-02_030405" originalFileLastModifTimestamp="4294967295" originalFileLastModifTimestampHigh="4294967296" />
            </mainView></Session></NotepadPlus>"#;
        let f = &parse_session(win).unwrap().files[0];
        assert_eq!(f.filename, "new 2");
        assert!(f
            .backup_file_path
            .ends_with("backup\\new 2@2024-01-02_030405"));
        assert_eq!(f.original_timestamp, 0xFFFF_FFFF);
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
        assert_eq!(menu_title(0, Path::new("/a/b.txt"), -1), "1: /a/b.txt");
        assert_eq!(menu_title(9, Path::new("/c"), -1), "10: /c");
        assert_eq!(menu_title(1, Path::new("/a/b.txt"), 0), "2: b.txt");
        assert_eq!(menu_title(0, Path::new("/a/b.txt"), 100), "1: /a/b.txt");
        assert_eq!(
            menu_title(0, Path::new("/Users/me/projects/notes/todo.txt"), 20),
            "1: /Users/m.../todo.txt"
        );
        let long = format!("/{}", "x".repeat(300));
        let t = menu_title(0, Path::new(&long), -1);
        assert_eq!(t.chars().count(), 3 + 127 + 3 + 130);
        assert_eq!(compact_path("/a/verylongfilename.txt", 10), "verylon...");
        assert_eq!(compact_path("/a/b.txt", 8), "/a/b.txt");
        assert_eq!(compact_path("/abc/def/g.txt", 12).chars().count(), 12);
    }
}
