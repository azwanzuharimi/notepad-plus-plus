// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{app_support_dir, attr};
use crate::encoding::{self, Enc};
use crate::session::{self, FileInfo, Session};
use crate::{ns, sci, App, Tab};
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSTabViewItem, NSView,
};
use objc2_foundation::{NSArray, NSRunLoopCommonModes};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SCI_GETDIRECTPOINTER: u32 = 2185;
const SCN_MODIFIED: u32 = 2008;
const SC_MOD_INSERTTEXT: i32 = 0x1;
const SC_MOD_DELETETEXT: i32 = 0x2;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
// The Windows FILETIME of 1970-01-01, in 100 ns units from 1601-01-01.
const EPOCH_FILETIME: u64 = 116_444_736_000_000_000;

// NppConstants.h BackupFeature: Preferences > Backup > Backup on save.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BackupFeature {
    #[default]
    None,
    Simple,
    Verbose,
}

// Preferences > Backup, with the defaults of Parameters.h NppGUI.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub backup: BackupFeature,
    pub use_dir: bool,
    pub backup_dir: String,
    pub snapshot_mode: bool,
    pub snapshot_timing_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            backup: BackupFeature::None,
            use_dir: false,
            backup_dir: String::new(),
            snapshot_mode: true,
            snapshot_timing_ms: 7000,
        }
    }
}

fn bool_attr(v: &str, default: bool) -> bool {
    match v {
        "yes" => true,
        "no" => false,
        _ => default,
    }
}

// Parameters.cpp feedGUIParameters: <GUIConfig name="Backup" ... /> of config.xml.
pub fn parse_settings(xml: &str) -> Settings {
    let mut s = Settings::default();
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if e.name().as_ref() == "GUIConfig" && attr(&e, "name") == "Backup" =>
            {
                s.backup = match attr(&e, "action").trim().parse::<i64>() {
                    Ok(1) => BackupFeature::Simple,
                    Ok(2) => BackupFeature::Verbose,
                    _ => BackupFeature::None,
                };
                s.use_dir = attr(&e, "useCustumDir") == "yes";
                s.backup_dir = attr(&e, "dir");
                s.snapshot_mode = bool_attr(&attr(&e, "isSnapshotMode"), s.snapshot_mode);
                if let Ok(n) = attr(&e, "snapshotBackupTiming").trim().parse() {
                    s.snapshot_timing_ms = n;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    s
}

// Parameters.cpp writeGUIParams: the element for config.xml.
#[allow(dead_code)]
pub fn gui_config(s: &Settings) -> String {
    let yes_no = |b| if b { "yes" } else { "no" };
    format!(
        "<GUIConfig name=\"Backup\" action=\"{}\" useCustumDir=\"{}\" dir=\"{}\" isSnapshotMode=\"{}\" snapshotBackupTiming=\"{}\" />",
        s.backup as u8,
        yes_no(s.use_dir),
        quick_xml::escape::escape(s.backup_dir.as_str()),
        yes_no(s.snapshot_mode),
        s.snapshot_timing_ms
    )
}

pub fn filetime(t: SystemTime) -> u64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => EPOCH_FILETIME + (d.as_nanos() / 100) as u64,
        Err(e) => EPOCH_FILETIME.saturating_sub((e.duration().as_nanos() / 100) as u64),
    }
}

// The last write time of a file as a FILETIME; None when it is not a file.
pub fn mtime(p: &Path) -> Option<u64> {
    let m = std::fs::metadata(p).ok().filter(|m| m.is_file())?;
    m.modified().ok().map(filetime)
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
    gmtoff: i64,
    zone: *const std::ffi::c_char,
}

extern "C" {
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
}

// wcsftime "%Y-%m-%d_%H%M%S" of the local time, as Notepad++ names backup files.
pub fn stamp(secs: i64) -> String {
    let mut t: Tm = unsafe { std::mem::zeroed() };
    if unsafe { localtime_r(&secs, &mut t) }.is_null() {
        return secs.to_string();
    }
    format!(
        "{:04}-{:02}-{:02}_{:02}{:02}{:02}",
        t.year + 1900,
        t.mon + 1,
        t.mday,
        t.hour,
        t.min,
        t.sec
    )
}

fn now_stamp() -> String {
    stamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64),
    )
}

// Buffer.cpp backupCurrentBuffer: "<name>@<timestamp>"; a name in use gets a number.
pub fn backup_name(name: &str, stamp: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = format!("{}@{stamp}", name.replace('/', "_"));
    (1..)
        .map(|k| match k {
            1 => base.clone(),
            k => format!("{base}_{k}"),
        })
        .find(|n| !taken(n))
        .unwrap_or(base)
}

// Parameters.cpp getSessionFromXmlTree: a backup path always points into the backup folder.
pub fn confine(dir: &Path, stored: &str) -> Option<PathBuf> {
    let name = stored.rsplit(['/', '\\']).next()?;
    (!matches!(name, "" | "." | "..")).then(|| dir.join(name))
}

// NppIO.cpp fileSave: the copy of the file on disk that Simple or Verbose makes before a save.
pub fn save_backup_path(file: &Path, s: &Settings, stamp: &str) -> Option<PathBuf> {
    let name = file.file_name()?.to_string_lossy();
    let dir = match (s.use_dir && !s.backup_dir.is_empty(), s.backup) {
        (_, BackupFeature::None) => return None,
        (true, _) => PathBuf::from(&s.backup_dir),
        (false, BackupFeature::Simple) => file.parent()?.to_path_buf(),
        (false, BackupFeature::Verbose) => file.parent()?.join("nppBackup"),
    };
    Some(dir.join(match s.backup {
        BackupFeature::Verbose => format!("{name}.{stamp}.bak"),
        _ => format!("{name}.bak"),
    }))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Restore {
    Skip,
    File,
    Backup { changed: bool },
}

// NppIO.cpp loadSession in snapshot mode: a backup wins; a changed file timestamp asks to reload.
pub fn restore(file: Option<u64>, backup: bool, stored: u64) -> Restore {
    match (file, backup) {
        (f, true) => Restore::Backup {
            changed: f.is_some_and(|m| stored != 0 && m != stored),
        },
        (Some(_), false) => Restore::File,
        (None, false) => Restore::Skip,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TabState {
    pub dirty: bool,
    pub untitled: bool,
    pub empty: bool,
    pub changed: bool,
    pub has_backup: bool,
    pub present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Keep,
    Write,
    Delete,
}

// A modified file, or an untitled tab with text, has a backup.
pub fn needs_backup(t: &TabState) -> bool {
    if t.untitled {
        !t.empty
    } else {
        t.dirty
    }
}

// Buffer.cpp backupCurrentBuffer for each tab: write changed text, delete the backup of a saved tab.
pub fn plan(tabs: &[TabState]) -> Vec<Step> {
    tabs.iter()
        .map(|t| match (needs_backup(t), t.has_backup) {
            (true, true) if !t.changed && t.present => Step::Keep,
            (true, _) => Step::Write,
            (false, true) => Step::Delete,
            (false, false) => Step::Keep,
        })
        .collect()
}

// NppIO.cpp fileCloseAll in snapshot mode: ask when the backup is missing or its last write failed.
pub fn ask_at_quit(t: &TabState) -> bool {
    needs_backup(t) && (t.changed || !t.has_backup || !t.present)
}

// The bytes a save writes; text that the encoding cannot hold is kept as UTF-8 with a BOM.
pub fn backup_bytes(text: &[u8], e: Enc) -> (Vec<u8>, bool) {
    match encoding::encode(text, e, false) {
        Ok(b) => (b, false),
        Err(_) => ([UTF8_BOM, text].concat(), true),
    }
}

// The decoding of a backup and the tab encoding: a UTF-8 BOM backup of a character set tab keeps the character set.
pub fn restored_enc(b: &[u8], cp: Option<u32>) -> (Enc, Enc) {
    match cp {
        Some(cp) if b.starts_with(UTF8_BOM) => (Enc::Utf8Bom, Enc::Cp(cp)),
        Some(cp) => (Enc::Cp(cp), Enc::Cp(cp)),
        None => {
            let e = encoding::detect(b);
            (e, e)
        }
    }
}

fn with_suffix(p: &Path, s: &str) -> PathBuf {
    let mut o = p.as_os_str().to_owned();
    o.push(s);
    PathBuf::from(o)
}

// A temporary file, then a rename: the old file stays when the write fails.
fn write_atomic(path: &Path, b: &[u8], sync: bool) -> std::io::Result<()> {
    use std::io::Write as _;
    let tmp = with_suffix(path, ".tmp");
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let r = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(b).and_then(|_| if sync { f.sync_all() } else { Ok(()) }))
        .and_then(|_| std::fs::rename(&tmp, path));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

fn copy_atomic(from: &Path, to: &Path) -> std::io::Result<()> {
    let tmp = with_suffix(to, ".tmp");
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d)?;
    }
    let r = std::fs::copy(from, &tmp).and_then(|_| std::fs::rename(&tmp, to));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

// Writes a backup to its old path, or to a new name in `dir` that no file or tab uses.
fn store(
    dir: &Path,
    old: Option<&Path>,
    name: &str,
    stamp: &str,
    used: &[PathBuf],
    bytes: &[u8],
    sync: bool,
) -> std::io::Result<PathBuf> {
    let path = match old {
        Some(p) => p.to_path_buf(),
        None => dir.join(backup_name(name, stamp, |n| {
            let p = dir.join(n);
            p.exists() || used.contains(&p)
        })),
    };
    write_atomic(&path, bytes, sync)?;
    Ok(path)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub enc: Enc,
    pub utf8: bool,
}

#[derive(Default)]
struct State {
    entries: RefCell<HashMap<usize, Entry>>,
    changed: RefCell<HashSet<usize>>,
    stamps: RefCell<HashMap<usize, u64>>,
    settings: RefCell<Option<Settings>>,
    armed: Cell<bool>,
    last_session: RefCell<String>,
}

thread_local! {
    static ST: State = State::default();
}

static APP: AtomicPtr<App> = AtomicPtr::new(std::ptr::null_mut());

fn key(v: &NSView) -> usize {
    v as *const NSView as usize
}

// The Scintilla object that sends the notifications of the view.
fn backend(v: &NSView) -> usize {
    sci::send(v, SCI_GETDIRECTPOINTER, 0, 0) as usize
}

pub fn entry(v: &NSView) -> Option<Entry> {
    ST.with(|s| s.entries.borrow().get(&key(v)).cloned())
}

// Buffer.cpp getLastModifiedFileTimestamp: the file time at the last load or save.
pub fn stamp_of(v: &NSView) -> u64 {
    ST.with(|s| s.stamps.borrow().get(&key(v)).copied().unwrap_or(0))
}

pub fn file_loaded(v: &NSView, p: Option<&Path>) {
    let k = key(v);
    let t = p.and_then(mtime);
    ST.with(|s| match t {
        Some(t) => s.stamps.borrow_mut().insert(k, t),
        None => s.stamps.borrow_mut().remove(&k),
    });
}

#[repr(C)]
struct ModNotify {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
    position: isize,
    ch: i32,
    modifiers: i32,
    modification_type: i32,
}

// NppNotification.cpp SCN_MODIFIED: the text changed after the last backup.
pub fn notify(scn: *const c_void) {
    let n = unsafe { &*(scn as *const ModNotify) };
    if n.code == SCN_MODIFIED && n.modification_type & (SC_MOD_INSERTTEXT | SC_MOD_DELETETEXT) != 0 {
        let _ = ST.try_with(|s| {
            if let Ok(mut c) = s.changed.try_borrow_mut() {
                c.insert(n.hwnd_from as usize);
            }
        });
    }
}

fn backup_dir() -> Option<PathBuf> {
    app_support_dir().map(|d| d.join("backup"))
}

// Preferences > Backup; read from config.xml at the first use.
pub fn settings() -> Settings {
    ST.with(|s| {
        s.settings
            .borrow_mut()
            .get_or_insert_with(|| {
                app_support_dir()
                    .and_then(|d| std::fs::read_to_string(d.join("config.xml")).ok())
                    .map_or_else(Settings::default, |x| parse_settings(&x))
            })
            .clone()
    })
}

// Parameters.h isSnapshotMode: the option, and the session is remembered.
pub fn snapshot_on() -> bool {
    settings().snapshot_mode && !session::no_session()
}

pub fn untitled_number(name: &str) -> Option<u32> {
    name.strip_prefix("new ")?.parse().ok()
}

// True when no backup state is borrowed, so a timer tick or the panic hook can run.
fn state_free() -> bool {
    ST.try_with(|s| {
        s.entries.try_borrow_mut().is_ok()
            && s.changed.try_borrow_mut().is_ok()
            && s.stamps.try_borrow_mut().is_ok()
            && s.settings.try_borrow_mut().is_ok()
            && s.last_session.try_borrow_mut().is_ok()
    })
    .unwrap_or(false)
}

pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        prev(info);
        emergency_backup();
    }));
}

// Best effort before the abort: only on the main thread, in snapshot mode, and when no state is borrowed.
fn emergency_backup() {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::SeqCst) || MainThreadMarker::new().is_none() {
        return;
    }
    let p = APP.load(Ordering::SeqCst);
    if p.is_null() {
        return;
    }
    let app = unsafe { &*p };
    if app.ivars().tabs.try_borrow_mut().is_ok() && state_free() && snapshot_on() {
        app.write_backups(true, false);
        app.write_session_now();
    }
}

impl App {
    // Notepad_plus_Window.cpp init: start the backup task in snapshot mode.
    pub(crate) fn start_backups(&self) {
        APP.store(self as *const App as *mut App, Ordering::SeqCst);
        self.arm_backup_timer();
    }

    // For the Preferences dialog: new settings apply at once.
    #[allow(dead_code)]
    pub(crate) fn set_backup_settings(&self, s: Settings) {
        ST.with(|st| *st.settings.borrow_mut() = Some(s));
        self.arm_backup_timer();
    }

    // The common modes also run the timer while a modal dialog shows.
    fn arm_backup_timer(&self) {
        if !snapshot_on() || ST.with(|s| s.armed.replace(true)) {
            return;
        }
        let secs = settings().snapshot_timing_ms.max(1000) as f64 / 1000.;
        let modes = NSArray::from_slice(&[unsafe { NSRunLoopCommonModes }]);
        let _: () = unsafe {
            msg_send![self, performSelector: sel!(backupTick:), withObject: None::<&AnyObject>, afterDelay: secs, inModes: &*modes]
        };
    }

    // Notepad_plus.cpp backupDocument: every N seconds, the backups and then session.xml.
    pub(crate) fn backup_tick(&self) {
        ST.with(|s| s.armed.set(false));
        if !snapshot_on() {
            return;
        }
        if self.ivars().tabs.try_borrow_mut().is_ok() && state_free() {
            self.write_backups(false, true);
            self.write_session_now();
        }
        self.arm_backup_timer();
    }

    fn tab_state(&self, t: &Tab) -> TabState {
        let e = entry(&t.view);
        let flagged = ST.with(|s| s.changed.borrow().contains(&backend(&t.view)));
        TabState {
            dirty: self.dirty(t),
            untitled: t.path.is_none(),
            empty: sci::length(&t.view) == 0,
            changed: flagged || e.as_ref().is_some_and(|e| e.enc != t.enc),
            has_backup: e.is_some(),
            present: e.is_some_and(|e| e.path.is_file()),
        }
    }

    // A failed write keeps the change flag, so the next tick and quit_session see it.
    pub(crate) fn write_backups(&self, sync: bool, alerts: bool) {
        let Some(dir) = backup_dir() else { return };
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        let states: Vec<TabState> = tabs.iter().map(|t| self.tab_state(t)).collect();
        for (t, step) in tabs.iter().zip(plan(&states)) {
            match step {
                Step::Keep => {}
                Step::Delete => self.drop_backup(&t.view),
                Step::Write => self.write_backup(&dir, t, sync, alerts),
            }
        }
    }

    fn write_backup(&self, dir: &Path, t: &Tab, sync: bool, alerts: bool) {
        let old = entry(&t.view);
        let used: Vec<PathBuf> =
            ST.with(|s| s.entries.borrow().values().map(|e| e.path.clone()).collect());
        let (bytes, utf8) = backup_bytes(&sci::bytes(&t.view), t.enc);
        let r = store(
            dir,
            old.as_ref().map(|e| e.path.as_path()),
            &t.name,
            &now_stamp(),
            &used,
            &bytes,
            sync,
        );
        let path = match r {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Cannot write the backup of {}: {e}", t.name);
                return;
            }
        };
        let b = backend(&t.view);
        ST.with(|s| {
            s.changed.borrow_mut().remove(&b);
            let e = Entry {
                path,
                enc: t.enc,
                utf8,
            };
            s.entries.borrow_mut().insert(key(&t.view), e)
        });
        let warn = utf8 && !old.is_some_and(|o| o.utf8) && !matches!(t.enc, Enc::Cp(_));
        if alerts && warn {
            let msg = format!(
                "The backup of \"{}\" is written as UTF-8 with BOM, because {} cannot hold some of its characters.",
                t.name,
                encoding::name(t.enc)
            );
            self.alert(&msg, "If the app restores this backup, the tab opens as UTF-8 with BOM.", &["OK"]);
        }
    }

    // FileManager::deleteBufferBackup.
    pub(crate) fn drop_backup(&self, v: &NSView) {
        if let Some(e) = ST.with(|s| s.entries.borrow_mut().remove(&key(v))) {
            let _ = std::fs::remove_file(&e.path);
        }
    }

    // NppIO.cpp fileClose: in snapshot mode a closed tab loses its backup.
    pub(crate) fn backup_closed(&self, item: &NSTabViewItem) {
        let view = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .find(|t| std::ptr::eq(&*t.item, item))
            .map(|t| t.view.clone());
        let Some(v) = view else { return };
        if snapshot_on() {
            self.drop_backup(&v);
        } else {
            ST.with(|s| s.entries.borrow_mut().remove(&key(&v)));
        }
        ST.with(|s| s.stamps.borrow_mut().remove(&key(&v)));
    }

    pub(crate) fn write_session_now(&self) {
        let Some(dir) = app_support_dir() else { return };
        if session::no_session() {
            return;
        }
        let xml = session::write_session(&self.current_session(false));
        if ST.with(|s| *s.last_session.borrow() == xml) {
            return;
        }
        match session::write_file(&dir.join("session.xml"), &xml, true) {
            Ok(()) => ST.with(|s| *s.last_session.borrow_mut() = xml),
            Err(e) => eprintln!("Cannot write session.xml: {e}"),
        }
    }

    // NppBigSwitch.cpp WM_CLOSE and NppIO.cpp fileCloseAll: in snapshot mode, quit keeps modified tabs as backups.
    pub(crate) fn quit_session(&self) -> Option<Session> {
        if !snapshot_on() {
            let session = self.current_session(false);
            let n = self.ivars().tabs.borrow().len();
            for i in 0..n {
                self.tab_view().selectTabViewItemAtIndex(i as isize);
                if !self.confirm_close(i) {
                    return None;
                }
            }
            return Some(session);
        }
        self.write_backups(true, false);
        let session = self.current_session(false);
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        for t in &tabs {
            if !ask_at_quit(&self.tab_state(t)) {
                continue;
            }
            self.tab_view().selectTabViewItem(Some(&t.item));
            let name = t
                .path
                .as_deref()
                .map_or(t.name.clone(), |p| p.display().to_string());
            let msg = format!("Your backup file cannot be found (deleted from outside).\nSave it otherwise your data will be lost\nDo you want to save file \"{name}\" ?");
            let r = self.alert("Save", &msg, &["Yes", "No", "Cancel"]);
            if r == NSAlertFirstButtonReturn {
                let i = self
                    .ivars()
                    .tabs
                    .borrow()
                    .iter()
                    .position(|x| std::ptr::eq(&*x.item, &*t.item));
                if !i.is_some_and(|i| self.save(i, false)) {
                    return None;
                }
            } else if r != NSAlertSecondButtonReturn {
                return None;
            }
        }
        Some(session)
    }

    // NppIO.cpp fileSave: Simple and Verbose copy the file on disk first; a failure asks to go on.
    pub(crate) fn backup_on_save(&self, file: &Path) -> bool {
        let Some(bak) = save_backup_path(file, &settings(), &now_stamp()) else {
            return true;
        };
        if copy_atomic(file, &bak).is_ok() {
            return true;
        }
        let msg = format!("The previous version of the file could not be saved into the backup directory at \"{}\".\n\nDo you want to save the current file anyway?", bak.display());
        self.alert("File Backup Failed", &msg, &["Yes", "No"]) == NSAlertFirstButtonReturn
    }

    // Buffer.cpp loadFile with a backup: the backup text, marked modified; the file path stays.
    // NppIO.cpp doOpen in snapshot mode: a tab that already has the file gets the backup text.
    pub(crate) fn restore_backup(&self, f: &FileInfo, p: &Path, bak: &Path) -> Option<usize> {
        let untitled = !p.is_absolute();
        let b = std::fs::read(bak).ok()?;
        let cp = u32::try_from(f.encoding)
            .ok()
            .filter(|&cp| encoding::supported(cp));
        let (dec, enc) = restored_enc(&b, cp);
        let open = if untitled { None } else { self.find_open(p, None) };
        let before = self.ivars().untitled.get();
        let i = match open {
            Some(i) => {
                self.load_into(i, &b, dec);
                i
            }
            None => {
                if !untitled {
                    self.recent_remove(p);
                }
                let (text, lost) = encoding::decode(&b, dec);
                self.add_tab((!untitled).then(|| p.to_path_buf()), enc, &text, lost);
                self.ivars().tabs.borrow().len().checked_sub(1)?
            }
        };
        let view = {
            let mut tabs = self.ivars().tabs.borrow_mut();
            let t = tabs.get_mut(i)?;
            t.enc = enc;
            t.enc_dirty = true;
            if untitled {
                t.name = f.filename.clone();
            }
            t.view.clone()
        };
        if untitled {
            let n = untitled_number(&f.filename).unwrap_or(0);
            self.ivars().untitled.set(before.max(n));
        }
        self.refresh_title(i);
        self.update_status();
        let (k, b) = (key(&view), backend(&view));
        ST.with(|s| {
            s.changed.borrow_mut().remove(&b);
            let e = Entry {
                path: bak.to_path_buf(),
                enc,
                utf8: false,
            };
            s.entries.borrow_mut().insert(k, e)
        });
        Some(i)
    }

    pub(crate) fn stored_backup(&self, f: &FileInfo) -> Option<PathBuf> {
        backup_dir()
            .and_then(|d| confine(&d, &f.backup_file_path))
            .filter(|p| p.is_file())
    }

    // Notepad_plus.cpp doReloadOrNot for a dirty document; No is the default button.
    pub(crate) fn ask_reload_restored(&self, item: &NSTabViewItem) {
        let i = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .position(|t| std::ptr::eq(&*t.item, item));
        let Some((i, t)) = i.and_then(|i| Some((i, self.tab(i)?))) else {
            return;
        };
        let Some(p) = t.path.clone() else { return };
        self.tab_view().selectTabViewItem(Some(item));
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&ns("Reload"));
        a.setInformativeText(&ns(&format!("\"{}\"\n\nThis file has been modified by another program.\nDo you want to reload it and lose the changes made in Notepad++?", p.display())));
        a.addButtonWithTitle(&ns("Yes")).setKeyEquivalent(&ns(""));
        a.addButtonWithTitle(&ns("No")).setKeyEquivalent(&ns("\r"));
        if a.runModal() != NSAlertFirstButtonReturn {
            return;
        }
        match std::fs::read(&p) {
            Ok(b) => {
                let e = match t.enc {
                    Enc::Cp(_) => t.enc,
                    _ => encoding::detect(&b),
                };
                self.load_into(i, &b, e);
                self.drop_backup(&t.view);
            }
            Err(err) => {
                self.alert(
                    &format!("Cannot open {}", p.display()),
                    &err.to_string(),
                    &["OK"],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(dirty: bool, untitled: bool, empty: bool, changed: bool, has_backup: bool) -> TabState {
        TabState {
            dirty,
            untitled,
            empty,
            changed,
            has_backup,
            present: has_backup,
        }
    }

    #[test]
    fn timer_plan() {
        let tabs = [
            st(true, false, false, true, false),
            st(true, false, false, false, true),
            st(true, false, false, true, true),
            st(false, false, false, false, true),
            st(false, false, false, true, false),
            st(true, true, false, true, false),
            st(false, true, false, false, true),
            st(true, true, true, true, true),
            st(false, true, true, false, false),
            TabState {
                present: false,
                ..st(true, false, false, false, true)
            },
        ];
        use Step::*;
        assert_eq!(
            plan(&tabs),
            [Write, Keep, Write, Delete, Keep, Write, Keep, Delete, Keep, Write]
        );
    }

    #[test]
    fn quit_questions() {
        assert!(!ask_at_quit(&st(true, false, false, false, true)));
        assert!(ask_at_quit(&st(true, false, false, true, true)));
        assert!(ask_at_quit(&st(true, false, false, false, false)));
        assert!(ask_at_quit(&TabState {
            present: false,
            ..st(true, false, false, false, true)
        }));
        assert!(ask_at_quit(&st(false, true, false, true, true)));
        assert!(!ask_at_quit(&st(false, false, false, true, false)));
        assert!(!ask_at_quit(&st(true, true, true, true, false)));
    }

    #[test]
    fn failed_write_keeps_old_backup() {
        use std::os::unix::fs::PermissionsExt;
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp/backup-readonly");
        if d.exists() {
            let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_dir_all(&d);
        }
        let old = store(&d, None, "a.txt", "S", &[], b"good", false).unwrap();
        assert_eq!(old, d.join("a.txt@S"));
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).unwrap();
        let again = store(&d, Some(&old), "a.txt", "T", &[], b"newer", true);
        let fresh = store(&d, None, "b.txt", "T", &[], b"x", false);
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(again.is_err() && fresh.is_err());
        assert_eq!(std::fs::read(&old).unwrap(), b"good");
        assert!(!d.join("a.txt@S.tmp").exists() && !d.join("b.txt@T").exists());
        let used = [d.join("c@S")];
        assert_eq!(store(&d, None, "c", "S", &used, b"c", false).unwrap(), d.join("c@S_2"));
    }

    #[test]
    fn restored_encodings() {
        assert_eq!(restored_enc(b"\xEF\xBB\xBF\xE4\xB8\xAD", Some(1252)), (Enc::Utf8Bom, Enc::Cp(1252)));
        assert_eq!(restored_enc(b"abc", Some(1251)), (Enc::Cp(1251), Enc::Cp(1251)));
        assert_eq!(restored_enc(b"\xEF\xBB\xBFabc", None), (Enc::Utf8Bom, Enc::Utf8Bom));
        assert_eq!(restored_enc(b"\xFF\xFEa\0", None), (Enc::Utf16Le, Enc::Utf16Le));
    }

    #[test]
    fn backup_file_names() {
        assert_eq!(
            backup_name("foo.h", "2026-10-10_134501", |_| false),
            "foo.h@2026-10-10_134501"
        );
        assert_eq!(
            backup_name("new 4", "2026-10-10_134501", |n| n
                == "new 4@2026-10-10_134501"),
            "new 4@2026-10-10_134501_2"
        );
        assert_eq!(backup_name("a/b", "s", |_| false), "a_b@s");
        let s = stamp(0);
        assert_eq!(s.len(), 17);
        assert!(s.starts_with("1970-01-01_") || s.starts_with("1969-12-31_"));
        let d = Path::new("/u/backup");
        assert_eq!(
            confine(d, "/u/backup/new 1@2026-10-10_134501"),
            Some(d.join("new 1@2026-10-10_134501"))
        );
        assert_eq!(confine(d, "/etc/passwd"), Some(d.join("passwd")));
        assert_eq!(
            confine(
                d,
                r"C:\Users\me\AppData\Roaming\Notepad++\backup\a.txt@2024-01-02_030405"
            ),
            Some(d.join("a.txt@2024-01-02_030405"))
        );
        assert_eq!(confine(d, ""), None);
        assert_eq!(confine(d, "/u/backup/.."), None);
    }

    #[test]
    fn restore_decisions() {
        use Restore::*;
        assert_eq!(restore(Some(10), true, 10), Backup { changed: false });
        assert_eq!(restore(Some(11), true, 10), Backup { changed: true });
        assert_eq!(restore(Some(11), true, 0), Backup { changed: false });
        assert_eq!(restore(None, true, 10), Backup { changed: false });
        assert_eq!(restore(Some(10), false, 10), File);
        assert_eq!(restore(None, false, 10), Skip);
    }

    #[test]
    fn settings_round_trip() {
        assert_eq!(parse_settings(""), Settings::default());
        let d = Settings::default();
        assert_eq!(
            gui_config(&d),
            "<GUIConfig name=\"Backup\" action=\"0\" useCustumDir=\"no\" dir=\"\" isSnapshotMode=\"yes\" snapshotBackupTiming=\"7000\" />"
        );
        let s = Settings {
            backup: BackupFeature::Verbose,
            use_dir: true,
            backup_dir: "/tmp/a & b".into(),
            snapshot_mode: false,
            snapshot_timing_ms: 3000,
        };
        let x = format!("<NotepadPlus><GUIConfigs><GUIConfig name=\"TabBar\" action=\"2\" />{}</GUIConfigs></NotepadPlus>", gui_config(&s));
        assert_eq!(parse_settings(&x), s);
        let bad =
            parse_settings("<GUIConfig name=\"Backup\" action=\"7\" isSnapshotMode=\"maybe\" />");
        assert_eq!(bad, d);
    }

    #[test]
    fn backup_on_save_paths() {
        let f = Path::new("/w/src/a.txt");
        let mut s = Settings::default();
        assert_eq!(save_backup_path(f, &s, "T"), None);
        s.backup = BackupFeature::Simple;
        assert_eq!(
            save_backup_path(f, &s, "T"),
            Some(PathBuf::from("/w/src/a.txt.bak"))
        );
        s.backup = BackupFeature::Verbose;
        assert_eq!(
            save_backup_path(f, &s, "2026-10-10_134501"),
            Some(PathBuf::from(
                "/w/src/nppBackup/a.txt.2026-10-10_134501.bak"
            ))
        );
        s.use_dir = true;
        s.backup_dir = "/bk".into();
        assert_eq!(
            save_backup_path(f, &s, "T"),
            Some(PathBuf::from("/bk/a.txt.T.bak"))
        );
        s.backup = BackupFeature::Simple;
        assert_eq!(
            save_backup_path(f, &s, "T"),
            Some(PathBuf::from("/bk/a.txt.bak"))
        );
    }

    #[test]
    fn backup_bytes_keep_text() {
        assert_eq!(
            backup_bytes(b"a\r\nb", Enc::Utf8),
            (b"a\r\nb".to_vec(), false)
        );
        assert_eq!(backup_bytes(b"a\nb", Enc::Utf16Le).0, b"\xFF\xFEa\0\n\0b\0");
        let (b, utf8) = backup_bytes("\u{4E2D}".as_bytes(), Enc::Cp(1252));
        assert!(utf8);
        assert_eq!(b, b"\xEF\xBB\xBF\xE4\xB8\xAD");
    }

    #[test]
    fn atomic_writes() {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp/backup-write");
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("x@1");
        write_atomic(&p, b"one", false).unwrap();
        write_atomic(&p, b"two", true).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert!(!d.join("x@1.tmp").exists());
        assert!(write_atomic(&d.join("missing-dir-file/\0"), b"x", false).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        copy_atomic(&p, &d.join("sub/x.bak")).unwrap();
        assert_eq!(std::fs::read(d.join("sub/x.bak")).unwrap(), b"two");
        assert!(copy_atomic(&d.join("none"), &d.join("y.bak")).is_err());
        assert!(!d.join("y.bak").exists());
        let t = mtime(&p).unwrap();
        assert!(t > EPOCH_FILETIME);
        assert_eq!(mtime(&d), None);
        assert_eq!(
            filetime(UNIX_EPOCH + std::time::Duration::from_secs(1)),
            EPOCH_FILETIME + 10_000_000
        );
    }
}
