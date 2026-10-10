// SPDX-License-Identifier: GPL-3.0-or-later
mod dialog;
pub mod model;

use crate::config::{app_support_dir, Config};
use crate::{cfg, item, lang, nested, ns, sci, styler, tagged, App, Tab};
use model::{UStyle, Udl, COLORSTYLE_BG, COLORSTYLE_FG};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSMenuItem, NSView, NSWorkspace,
};
use objc2_foundation::NSURL;
use std::cell::RefCell;
use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};

// Tab.lang of a UDL tab; an empty name is the language of the Define dialog (IDM_LANG_USER).
pub const PREFIX: &str = "udl:";
const USER_DEFINED: &str = "User-Defined";
const COLLECTION: &str = "https://github.com/notepad-plus-plus/userDefinedLanguages";
// The files that the Notepad++ installer puts in the userDefineLangs folder.
const PREINSTALLED: [(&str, &str); 2] = [
    (
        "markdown._preinstalled.udl.xml",
        include_str!("../../../PowerEditor/bin/userDefineLangs/markdown._preinstalled.udl.xml"),
    ),
    (
        "markdown._preinstalled_DM.udl.xml",
        include_str!("../../../PowerEditor/bin/userDefineLangs/markdown._preinstalled_DM.udl.xml"),
    ),
];
const SCI_STYLESETFORE: u32 = 2051;
const SCI_STYLESETBACK: u32 = 2052;
const SCI_STYLESETBOLD: u32 = 2053;
const SCI_STYLESETITALIC: u32 = 2054;
const SCI_STYLESETSIZE: u32 = 2055;
const SCI_STYLESETFONT: u32 = 2056;
const SCI_STYLESETUNDERLINE: u32 = 2059;
const SCI_GETDOCPOINTER: u32 = 2357;
const SCI_COLOURISE: u32 = 4003;
const SCI_SETPROPERTY: u32 = 4004;
const SCI_SETKEYWORDS: u32 = 4005;
const SCI_SETILEXER: u32 = 4033;

extern "C" {
    fn CreateLexer(name: *const c_char) -> *mut c_void;
}

pub struct Entry {
    pub udl: Udl,
    pub id: usize,
    pub file: PathBuf,
}

// NppParameters _userLangArray and the UDL files; ids stand for the name pointer that LexUser uses as a cache key.
pub struct Store {
    pub langs: Vec<Entry>,
    prologs: Vec<(PathBuf, String)>,
    unreadable: Vec<PathBuf>,
    next_id: usize,
    pub scratch: Udl,
    pub current: usize,
    pub dirty: Vec<PathBuf>,
    dir: Option<PathBuf>,
}

thread_local! {
    static STORE: RefCell<Option<Store>> = const { RefCell::new(None) };
}

pub fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    STORE.with(|c| {
        f(c.borrow_mut()
            .get_or_insert_with(|| Store::load(app_support_dir())))
    })
}

fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

impl Store {
    // NppParameters::load: userDefineLang.xml, then userDefineLangs/*.xml.
    fn load(dir: Option<PathBuf>) -> Store {
        let mut s = Store {
            langs: vec![],
            prologs: vec![],
            unreadable: vec![],
            next_id: 1,
            scratch: Udl::new("new user define"),
            current: 0,
            dirty: vec![],
            dir: dir.clone(),
        };
        let Some(dir) = dir else { return s };
        let folder = dir.join("userDefineLangs");
        if !folder.exists() {
            for (name, text) in PREINSTALLED {
                let _ = crate::session::write_file(&folder.join(name), text, false);
            }
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")))
            .collect();
        files.sort();
        files.insert(0, dir.join("userDefineLang.xml"));
        for f in files {
            s.read(&f);
        }
        s
    }

    fn read(&mut self, f: &Path) -> usize {
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    self.unreadable.push(f.into());
                }
                return 0;
            }
        };
        let Some(v) = model::parse(&text) else {
            self.unreadable.push(f.into());
            return 0;
        };
        self.prologs.push((f.into(), model::prolog(&text)));
        let before = self.langs.len();
        for udl in v.into_iter().take(model::MAX_LANGS.saturating_sub(before)) {
            self.langs.push(Entry {
                udl,
                id: self.next_id,
                file: f.into(),
            });
            self.next_id += 1;
        }
        self.langs.len() - before
    }

    pub fn folder(&self) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join("userDefineLangs"))
    }

    fn default_file(&self) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join("userDefineLang.xml"))
    }

    // NppParameters writeDefaultUDL and writeNonDefaultUDL for one file: a file without UDL is deleted.
    pub fn save(&self, f: &Path) -> Result<(), String> {
        if self.unreadable.iter().any(|u| u == f) {
            return Err(format!(
                "{} cannot be read, so it is not changed.",
                f.display()
            ));
        }
        let langs: Vec<&Udl> = self
            .langs
            .iter()
            .filter(|e| e.file == f)
            .map(|e| &e.udl)
            .collect();
        if langs.is_empty() {
            return match std::fs::remove_file(f) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("{}: {e}", f.display()))
                }
                _ => Ok(()),
            };
        }
        let prolog = self
            .prologs
            .iter()
            .find(|(p, _)| p == f)
            .map_or(model::DECLARATION.to_string(), |(_, s)| s.clone());
        crate::session::write_file(f, &model::to_xml(&prolog, &langs), false)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.langs.iter().any(|e| e.udl.name == name)
    }

    // NppParameters::addUserLangToEnd: the new language goes to userDefineLang.xml.
    pub fn add(&mut self, udl: Udl) -> Result<usize, String> {
        if self.langs.len() >= model::MAX_LANGS {
            return Err(format!(
                "You can have {} languages at most.",
                model::MAX_LANGS
            ));
        }
        let file = self.default_file().ok_or("No settings folder")?;
        self.langs.push(Entry {
            udl,
            id: self.next_id,
            file: file.clone(),
        });
        self.next_id += 1;
        if let Err(e) = self.save(&file) {
            self.langs.pop();
            return Err(e);
        }
        Ok(self.langs.len() - 1)
    }

    // Copies an UDL file into the userDefineLangs folder and loads it.
    pub fn import(&mut self, src: &Path) -> Result<usize, String> {
        let text = std::fs::read_to_string(src).map_err(|e| e.to_string())?;
        if model::parse(&text).is_none_or(|v| v.is_empty()) {
            return Err("Failed to import.".into());
        }
        let folder = self.folder().ok_or("No settings folder")?;
        let stem = src.file_stem().unwrap_or_default().to_string_lossy();
        let dest = (1..)
            .map(|n| match n {
                1 => folder.join(format!("{stem}.xml")),
                n => folder.join(format!("{stem} ({n}).xml")),
            })
            .find(|p| !p.exists())
            .ok_or("No free file name")?;
        crate::session::write_file(&dest, &text, false)?;
        let n = self.read(&dest);
        if n == 0 {
            self.prologs.retain(|(p, _)| *p != dest);
            let _ = std::fs::remove_file(&dest);
        }
        Ok(n)
    }

    // The language that the Define dialog shows: the unsaved one or a saved one.
    pub fn dialog_lang(&self) -> (Udl, usize) {
        match self.current.checked_sub(1).and_then(|i| self.langs.get(i)) {
            Some(e) => (e.udl.clone(), e.id),
            None => (self.scratch.clone(), 0),
        }
    }
}

fn bgr(c: u32) -> isize {
    (((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)) as isize
}

// ScintillaEditView::setSpecialStyle.
fn set_style(v: &NSView, s: &UStyle) {
    let id = s.id;
    if s.color_style & COLORSTYLE_FG != 0 {
        sci::send(v, SCI_STYLESETFORE, id, bgr(s.fg.unwrap_or(0xFFFFFF)));
    }
    if s.color_style & COLORSTYLE_BG != 0 {
        sci::send(v, SCI_STYLESETBACK, id, bgr(s.bg.unwrap_or(0xFFFFFF)));
    }
    if !s.font_name.is_empty() {
        let f = c(&s.font_name);
        sci::send(v, SCI_STYLESETFONT, id, f.as_ptr() as isize);
    }
    if s.font_style != -1 {
        sci::send(v, SCI_STYLESETBOLD, id, (s.font_style & 1) as isize);
        sci::send(v, SCI_STYLESETITALIC, id, (s.font_style & 2) as isize);
        sci::send(v, SCI_STYLESETUNDERLINE, id, (s.font_style & 4) as isize);
    }
    if s.font_size > 0 {
        sci::send(v, SCI_STYLESETSIZE, id, s.font_size as isize);
    }
}

// ScintillaEditView::defineDocType for L_USER with setUserLexer.
pub fn apply(v: &NSView, cfg: &Config, u: &Udl, udl_id: usize) {
    sci::apply_language(v, cfg, None);
    let user = c("user");
    sci::send(v, SCI_SETILEXER, 0, unsafe { CreateLexer(user.as_ptr()) }
        as isize);
    let buffer = (sci::send(v, SCI_GETDOCPOINTER, 0, 0) as usize >> 4) & 0x7FFF_FFFF;
    let (props, words) = model::lexer_setup(u, udl_id, buffer);
    for (k, val) in props {
        let (k, val) = (c(&k), c(&val));
        sci::send(
            v,
            SCI_SETPROPERTY,
            k.as_ptr() as usize,
            val.as_ptr() as isize,
        );
    }
    for (i, w) in words {
        let w = c(&w);
        sci::send(v, SCI_SETKEYWORDS, i, w.as_ptr() as isize);
    }
    u.styles.iter().for_each(|s| set_style(v, s));
    sci::setup_fold(v, cfg, "user");
    sci::send(v, SCI_COLOURISE, 0, -1);
}

// The UDL of a tab: the Language menu choice, else Buffer::setFileName (an UDL extension wins over the built-in ones).
// None is not a UDL tab; Some(None) is the unsaved language of the Define dialog.
fn which(s: &Store, t: &Tab) -> Option<Option<usize>> {
    match t.lang.as_deref() {
        Some(l) => match l.strip_prefix(PREFIX)? {
            "" => Some(s.current.checked_sub(1).filter(|&i| i < s.langs.len())),
            name => s.langs.iter().position(|e| e.udl.name == name).map(Some),
        },
        None => {
            let name = t
                .path
                .as_deref()
                .and_then(Path::file_name)
                .map_or(t.name.clone(), |n| n.to_string_lossy().into_owned());
            model::for_file_name(s.langs.iter().map(|e| &e.udl), &name, styler::dark()).map(Some)
        }
    }
}

impl Store {
    fn get(&self, w: Option<usize>) -> (&Udl, usize) {
        match w.and_then(|i| self.langs.get(i)) {
            Some(e) => (&e.udl, e.id),
            None => (&self.scratch, 0),
        }
    }
}

fn udl_name(t: &Tab) -> Option<String> {
    with(|s| which(s, t).map(|w| s.get(w).0.name.clone()))
}

fn lang_items(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let names: Vec<String> = with(|s| s.langs.iter().map(|e| e.udl.name.clone()).collect());
    names
        .iter()
        .enumerate()
        .map(|(i, n)| tagged(mtm, n, sel!(setUdl:), i as isize, t))
        .collect()
}

// The end of the compact Language menu of Notepad_plus.rc, with the UDL names before "User-Defined" (Notepad_plus.cpp).
pub fn menu_items(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let mut v = vec![
        NSMenuItem::separatorItem(mtm),
        nested(
            mtm,
            "User Defined Language",
            vec![
                item(mtm, "Define your language...", sel!(defineUdl:), "", t),
                item(
                    mtm,
                    "Open User Defined Language folder...",
                    sel!(openUdlFolder:),
                    "",
                    t,
                ),
                item(
                    mtm,
                    "Notepad++ User Defined Languages Collection",
                    sel!(udlCollection:),
                    "",
                    t,
                ),
            ],
        ),
    ];
    v.extend(lang_items(mtm, t));
    v.push(item(mtm, USER_DEFINED, sel!(userDefined:), "", t));
    v
}

impl App {
    pub(crate) fn apply_udl_at(&self, i: usize) -> bool {
        self.apply_udl_with(i, cfg())
    }

    // The UDL part of ScintillaEditView::defineDocType; false for a tab without a UDL.
    pub(crate) fn apply_udl_with(&self, i: usize, c: &Config) -> bool {
        let Some(t) = self.tab(i) else { return false };
        let done = with(|s| {
            let w = which(s, &t)?;
            let (u, id) = s.get(w);
            apply(&t.view, c, u, id);
            Some(())
        });
        if done.is_some() {
            self.apply_view(&t.view, "user", c);
        }
        done.is_some()
    }

    // Notepad_plus::getLangDesc for L_USER.
    pub(crate) fn udl_status(&self, t: &Tab) -> Option<String> {
        let name = udl_name(t)?;
        let desc = lang::long_name("udf");
        Some(match t.lang.as_deref() {
            Some(PREFIX) => desc,
            _ => format!("{desc} - {name}"),
        })
    }

    // Notepad_plus::getLangFromMenu for the session file: the UDL name, or "User-Defined".
    pub(crate) fn udl_session_name(&self, t: &Tab) -> Option<String> {
        let name = udl_name(t)?;
        Some(match t.lang.as_deref() {
            Some(PREFIX) => USER_DEFINED.into(),
            _ => name,
        })
    }

    // NppIO.cpp loadSession: a UDL name sets that UDL, "User-Defined" gives Normal Text; built-in menu names come first.
    pub(crate) fn restore_udl(&self, i: usize, lang: &str) -> bool {
        let Some(t) = self.tab(i) else { return false };
        let set = if lang == USER_DEFINED {
            "normal".to_string()
        } else if crate::session::lang_from_menu_text(cfg(), lang).is_none()
            && with(|s| s.exists(lang))
        {
            format!("{PREFIX}{lang}")
        } else {
            return false;
        };
        if udl_name(&t).as_deref() != Some(lang) || lang == USER_DEFINED {
            if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
                t.lang = Some(set);
            }
            self.sync_clones(i);
            self.apply_tab_language(i);
        }
        true
    }

    pub(crate) fn reapply_udl_tabs(&self) {
        let n = self.ivars().tabs.borrow().len();
        for i in 0..n {
            self.apply_udl_at(i);
        }
        self.update_status();
    }

    pub(crate) fn reapply_all_tabs(&self) {
        let n = self.ivars().tabs.borrow().len();
        for i in 0..n {
            self.apply_tab_language(i);
        }
        self.update_status();
    }

    fn set_tab_lang(&self, lang: String) {
        let Some(i) = self.current() else { return };
        if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
            t.lang = Some(lang);
        }
        self.apply_tab_language(i);
        self.refresh_title(i);
        self.update_status();
    }

    // NppCommands.cpp IDM_LANG_USER + 1 and up: the UDL stays when the file name changes.
    pub(crate) fn set_udl(&self, tag: isize) {
        if let Some(name) = with(|s| s.langs.get(tag as usize).map(|e| e.udl.name.clone())) {
            self.set_tab_lang(format!("{PREFIX}{name}"));
        }
    }

    // IDM_LANG_USER: the language that the Define dialog shows.
    pub(crate) fn user_defined(&self) {
        self.set_tab_lang(PREFIX.into());
    }

    pub(crate) fn open_udl_folder(&self) {
        let Some(dir) = with(|s| s.folder()) else {
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        NSWorkspace::sharedWorkspace()
            .openURL(&NSURL::fileURLWithPath(&ns(&dir.to_string_lossy())));
    }

    pub(crate) fn udl_collection(&self) {
        if let Some(url) = NSURL::URLWithString(&ns(COLLECTION)) {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }

    // Rebuilds the UDL items of the Language menu after a language is added, renamed or removed.
    pub(crate) fn refresh_udl_menu(&self) {
        let mtm = self.mtm();
        let Some(menu) = NSApplication::sharedApplication(mtm)
            .mainMenu()
            .and_then(|m| crate::l10n::bar_menu(&m, "Language"))
        else {
            return;
        };
        for i in (0..menu.numberOfItems()).rev() {
            if menu.itemAtIndex(i).and_then(|it| it.action()) == Some(sel!(setUdl:)) {
                menu.removeItemAtIndex(i);
            }
        }
        let at = (0..menu.numberOfItems())
            .find(|&i| menu.itemAtIndex(i).and_then(|it| it.action()) == Some(sel!(userDefined:)))
            .unwrap_or(menu.numberOfItems());
        for (k, it) in lang_items(mtm, Some(self)).iter().enumerate() {
            menu.insertItem_atIndex(it, at + k as isize);
        }
    }

    // Checkmarks for the UDL items; built-in language items are off on a UDL tab.
    pub(crate) fn validate_udl(&self, item: &NSMenuItem) -> Option<bool> {
        let action = item.action()?;
        let tab = self.current().and_then(|i| self.tab(i));
        let mark = |on: bool| {
            item.setState(if on {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            })
        };
        if action == sel!(defineUdl:) {
            mark(self.udl_dialog_visible());
            return Some(true);
        }
        if action == sel!(openUdlFolder:) || action == sel!(udlCollection:) {
            return Some(true);
        }
        let udl = tab.as_ref().and_then(|t| with(|s| which(s, t)));
        if action == sel!(setUdl:) {
            let menu_set = tab
                .as_ref()
                .is_some_and(|t| t.lang.as_deref() != Some(PREFIX));
            mark(menu_set && udl == Some(Some(item.tag() as usize)));
            return Some(tab.is_some());
        }
        if action == sel!(userDefined:) {
            mark(
                tab.as_ref()
                    .is_some_and(|t| t.lang.as_deref() == Some(PREFIX)),
            );
            return Some(tab.is_some());
        }
        if action == sel!(setLanguage:) && udl.is_some() {
            mark(false);
            return Some(true);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("npp-udl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn store_files() {
        let dir = temp_dir("store");
        let s = Store::load(Some(dir.clone()));
        let names: Vec<&str> = s.langs.iter().map(|e| e.udl.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Markdown (preinstalled)",
                "Markdown (preinstalled dark mode)"
            ]
        );
        assert_eq!(s.langs[0].id, 1);
        assert!(dir
            .join("userDefineLangs/markdown._preinstalled.udl.xml")
            .exists());
        drop(s);
        std::fs::write(
            dir.join("userDefineLang.xml"),
            "<NotepadPlus><bad></NotepadPlus>",
        )
        .unwrap();
        let mut s = Store::load(Some(dir.clone()));
        assert_eq!(s.langs.len(), 2);
        let err = s.add(Udl::new("New")).unwrap_err();
        assert!(err.contains("cannot be read"), "{err}");
        assert_eq!(s.langs.len(), 2);
        std::fs::remove_file(dir.join("userDefineLang.xml")).unwrap();
        let mut s = Store::load(Some(dir.clone()));
        let i = s.add(s.langs[0].udl.copy_as("Mine")).unwrap();
        assert_eq!(i, 2);
        assert!(s.exists("Mine"));
        let again = Store::load(Some(dir.clone()));
        assert_eq!(again.langs[0].udl.name, "Mine");
        assert_eq!(again.langs[0].udl.keywords, s.langs[2].udl.keywords);
        let src = dir.join("userDefineLangs/markdown._preinstalled.udl.xml");
        assert_eq!(s.import(&src).unwrap(), 1);
        assert!(dir
            .join("userDefineLangs/markdown._preinstalled.udl (2).xml")
            .exists());
        s.langs.retain(|e| e.udl.name != "Mine");
        s.save(&dir.join("userDefineLang.xml")).unwrap();
        assert!(!dir.join("userDefineLang.xml").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
