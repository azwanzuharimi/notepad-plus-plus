// SPDX-License-Identifier: GPL-3.0-or-later
use crate::panel::{self, Form};
use crate::search::{self, Mode, Next, Opts};
use crate::shortcuts::{self, Macro, Shortcuts, Step, TYPE_MENU, TYPE_S, TYPE_SNR};
use crate::{ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSButton, NSMenu,
    NSMenuDidSendActionNotification, NSMenuItem, NSPopUpButton, NSTextField, NSView,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};

pub const SCN_MACRORECORD: u32 = 2009;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_BEGINUNDOACTION: u32 = 2078;
const SCI_ENDUNDOACTION: u32 = 2079;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_STARTRECORD: u32 = 3001;
const SCI_STOPRECORD: u32 = 3002;
const FIXED_ITEMS: isize = 5;

const IDC_FRCOMMAND_INIT: i32 = 1700;
const IDC_FRCOMMAND_EXEC: i32 = 1701;
const IDC_FRCOMMAND_BOOLEANS: i32 = 1702;
const IDFINDWHAT: i32 = 1601;
const IDREPLACEWITH: i32 = 1602;
const IDNORMAL: i32 = 1625;
const IDOK: isize = 1;
const IDREPLACE: isize = 1608;
const IDREPLACEALL: isize = 1609;
const IDC_FINDPREV: isize = 1721;
const IDC_FINDNEXT: isize = 1723;
const IDF_WHOLEWORD: isize = 1;
const IDF_MATCHCASE: isize = 2;
const IDF_WRAP: isize = 256;
const IDF_WHICH_DIRECTION: isize = 512;
const IDF_REDOTMATCHNL: isize = 1024;

// Menu actions that call Scintilla directly on macOS, so Scintilla does not record them.
const SCI_ACTIONS: [(&str, i32); 2] = [("paste:", 2179), ("selectAll:", 2013)];
// Notepad++ does not record Cut and Copy as commands: Scintilla records SCI_CUT, SCI_COPY or the line commands.
const NOT_RECORDED: [&str; 2] = ["cut:", "copy:"];

// Notepad++ commands (menuCmdID.h) that this app has in its menus; a tag of -1 matches all tags.
fn menu_cmds() -> Vec<(&'static str, i32, &'static str, isize)> {
    let mut v = vec![
        ("IDM_FILE_NEW", 41001, "newDocument:", -1),
        ("IDM_FILE_CLOSE", 41003, "closeTab:", -1),
        ("IDM_FILE_CLOSEALL", 41004, "closeMultiple:", 0),
        ("IDM_FILE_CLOSEALL_BUT_CURRENT", 41005, "closeMultiple:", 1),
        ("IDM_FILE_CLOSEALL_TOLEFT", 41009, "closeMultiple:", 2),
        ("IDM_FILE_CLOSEALL_TORIGHT", 41018, "closeMultiple:", 3),
        ("IDM_FILE_CLOSEALL_UNCHANGED", 41024, "closeMultiple:", 4),
        ("IDM_FILE_SAVE", 41006, "saveDocument:", -1),
        ("IDM_FILE_SAVEALL", 41007, "saveAll:", -1),
        ("IDM_FILE_RELOAD", 41014, "reloadFromDisk:", -1),
        ("IDM_EDIT_CUT", 42001, "cut:", -1),
        ("IDM_EDIT_COPY", 42002, "copy:", -1),
        ("IDM_EDIT_UNDO", 42003, "undo:", -1),
        ("IDM_EDIT_REDO", 42004, "redo:", -1),
        ("IDM_EDIT_PASTE", 42005, "paste:", -1),
        ("IDM_EDIT_SELECTALL", 42007, "selectAll:", -1),
        ("IDM_SEARCH_FINDNEXT", 43002, "findNext:", -1),
        ("IDM_SEARCH_FINDPREV", 43010, "findPrevious:", -1),
        ("IDM_VIEW_ALWAYSONTOP", 44034, "alwaysOnTop:", -1),
        ("IDM_VIEW_FULLSCREENTOGGLE", 44032, "fullScreen:", -1),
        (
            "IDM_VIEW_WRAP",
            44022,
            "viewOption:",
            crate::view::WRAP as isize,
        ),
        ("IDM_VIEW_FOLDALL", 44010, "foldAll:", 0),
        ("IDM_VIEW_UNFOLDALL", 44029, "foldAll:", 1),
        ("IDM_VIEW_FOLD_CURRENT", 44030, "foldCurrent:", 0),
        ("IDM_VIEW_UNFOLD_CURRENT", 44031, "foldCurrent:", 1),
        ("IDM_VIEW_TAB_NEXT", 44095, "selectTab:", 11),
        ("IDM_VIEW_TAB_PREV", 44096, "selectTab:", 12),
        ("IDM_VIEW_GOTO_START", 10005, "moveTab:", 0),
        ("IDM_VIEW_GOTO_END", 10006, "moveTab:", 1),
        ("IDM_VIEW_TAB_MOVEFORWARD", 44098, "moveTab:", 2),
        ("IDM_VIEW_TAB_MOVEBACKWARD", 44099, "moveTab:", 3),
        ("IDM_FORMAT_TODOS", 45001, "eolConvert:", 0),
        ("IDM_FORMAT_TOUNIX", 45002, "eolConvert:", 2),
        ("IDM_FORMAT_TOMAC", 45003, "eolConvert:", 1),
    ];
    for n in 0..8 {
        v.push(("IDM_VIEW_FOLD_", 44051 + n, "foldLevel:", n as isize));
        v.push(("IDM_VIEW_UNFOLD_", 44061 + n, "unfoldLevel:", n as isize));
    }
    for n in 0..9 {
        v.push(("IDM_VIEW_TAB", 44086 + n, "selectTab:", n as isize));
    }
    v
}

pub fn menu_step(action: &str, tag: isize) -> Option<Step> {
    if NOT_RECORDED.contains(&action) {
        return None;
    }
    if let Some((_, m)) = SCI_ACTIONS.iter().find(|a| a.0 == action) {
        return Some(Step::new(shortcuts::TYPE_L, *m, 0, 0, ""));
    }
    menu_cmds()
        .into_iter()
        .find(|c| c.2 == action && (c.3 == -1 || c.3 == tag))
        .map(|c| Step::menu(c.1))
}

fn menu_action(id: i32) -> Option<(&'static str, isize)> {
    menu_cmds()
        .into_iter()
        .find(|c| c.1 == id)
        .map(|c| (c.2, c.3))
}

// Port of the "Run until the end of file" loop of the WM_MACRODLGRUNMACRO handler in NppBigSwitch.cpp.
#[derive(Debug, Default)]
pub struct UntilEof {
    last: isize,
    cur: isize,
    d_last: isize,
    d_cur: isize,
    up: bool,
    n: usize,
}

impl UntilEof {
    pub fn new(line_count: isize, cur_line: isize) -> UntilEof {
        UntilEof {
            last: line_count - 1,
            cur: cur_line,
            ..UntilEof::default()
        }
    }

    // Call after each run; true means run again.
    pub fn again(&mut self, line_count: isize, cur_line: isize) -> bool {
        self.n += 1;
        if self.n > 2 && self.up != (self.d_cur < 0) && self.d_last >= 0 {
            return false;
        }
        self.up = self.d_cur < 0;
        self.d_last = line_count - 1 - self.last;
        self.d_cur = cur_line - self.cur;
        if self.d_cur == 0 && self.d_last >= 0 {
            return false;
        }
        if self.d_last < self.d_cur {
            self.last += self.d_last;
        }
        self.cur += self.d_cur;
        !(self.cur > self.last
            || self.cur < 0
            || (self.d_cur == 0 && self.cur == 0 && (self.d_last >= 0 || self.up)))
    }
}

#[derive(Default, Clone)]
struct SnR {
    find: String,
    replace: String,
    flags: isize,
    mode: isize,
}

impl SnR {
    fn opts(&self) -> Opts {
        Opts {
            find: self.find.clone(),
            replace: self.replace.clone(),
            whole_word: self.flags & IDF_WHOLEWORD != 0,
            match_case: self.flags & IDF_MATCHCASE != 0,
            wrap: self.flags & IDF_WRAP != 0,
            mode: match self.mode {
                1 => Mode::Extended,
                2 => Mode::Regex,
                _ => Mode::Normal,
            },
            dot_nl: self.flags & IDF_REDOTMATCHNL != 0,
        }
    }
}

#[repr(C)]
struct Scn {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
    position: isize,
    ch: i32,
    modifiers: i32,
    modification_type: i32,
    text: *const c_char,
    length: isize,
    lines_added: isize,
    message: i32,
    w_param: usize,
    l_param: isize,
}

struct MultiUi {
    form: Form,
    list: Retained<NSPopUpButton>,
    times: Retained<NSTextField>,
    multi: Retained<NSButton>,
}

#[derive(Default)]
struct State {
    recording: Cell<bool>,
    saved: Cell<bool>,
    current: RefCell<Vec<Step>>,
    store: RefCell<Shortcuts>,
    menus: OnceCell<[Retained<NSMenu>; 2]>,
    load_error: RefCell<Option<String>>,
    multi: OnceCell<MultiUi>,
}

thread_local! {
    static S: State = State::default();
}

pub fn with_store<R>(f: impl FnOnce(&mut Shortcuts) -> R) -> R {
    S.with(|s| f(&mut s.store.borrow_mut()))
}

fn file() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/Application Support/notepadpp-mac/shortcuts.xml"),
    )
}

// The text of the file, or None when the file does not exist.
fn read_file(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read(path) {
        Ok(b) => String::from_utf8(b)
            .map(Some)
            .map_err(|_| format!("{}: the file is not UTF-8.", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

// Writes a temporary file and renames it, so that an error cannot leave a part of the file.
fn write_file(path: &Path, text: &str) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(path.parent().unwrap()).map_err(err)?;
    let tmp = path.with_extension("xml.tmp");
    std::fs::write(&tmp, text).map_err(err)?;
    std::fs::rename(&tmp, path).map_err(err)
}

fn load() -> Result<Shortcuts, String> {
    let Some(path) = file() else {
        return Ok(shortcuts::defaults());
    };
    match read_file(&path)? {
        None => Ok(shortcuts::defaults()),
        Some(t) => shortcuts::parse(&t).map_err(|e| format!("{}: {e}", path.display())),
    }
}

fn save() -> Result<(), String> {
    if let Some(e) = S.with(|s| s.load_error.borrow().clone()) {
        return Err(format!(
            "{e}\nThe app does not change this file. Correct or move it, then start the app again."
        ));
    }
    let path = file().ok_or("HOME is not set")?;
    let old = read_file(&path)?;
    let text = with_store(|s| shortcuts::write(old.as_deref(), s))?;
    write_file(&path, &text)
}

fn find_item(m: &NSMenu, action: Sel, tag: isize) -> Option<(Retained<NSMenu>, isize)> {
    for i in 0..m.numberOfItems() {
        let it = m.itemAtIndex(i)?;
        if it.action() == Some(action) && (tag == -1 || it.tag() == tag) {
            return Some((m.retain(), i));
        }
        if let Some(r) = it.submenu().and_then(|s| find_item(&s, action, tag)) {
            return Some(r);
        }
    }
    None
}

// Adds the items with Notepad++ FolderName grouping: next items with the same folder share one submenu.
fn add_items(
    mtm: MainThreadMarker,
    m: &NSMenu,
    items: Vec<(String, String)>,
    action: Sel,
    t: Option<&AnyObject>,
) {
    let mut folder: Option<(String, Retained<NSMenu>)> = None;
    for (i, (name, f)) in items.into_iter().enumerate() {
        let it = crate::tagged(mtm, &name, action, i as isize, t);
        if f.is_empty() {
            folder = None;
            m.addItem(&it);
            continue;
        }
        if folder.as_ref().is_none_or(|x| x.0 != f) {
            let top = crate::nested(mtm, &f, vec![]);
            m.addItem(&top);
            folder = Some((f, top.submenu().unwrap()));
        }
        folder.as_ref().unwrap().1.addItem(&it);
    }
}

fn rebuild(mtm: MainThreadMarker, t: Option<&AnyObject>) {
    let Some([mac, run]) = S.with(|s| s.menus.get().cloned()) else {
        return;
    };
    let (ms, cs) = with_store(|s| {
        (
            s.macros
                .iter()
                .map(|m| (m.name.clone(), m.folder.clone()))
                .collect::<Vec<_>>(),
            s.commands
                .iter()
                .map(|c| (c.name.clone(), c.folder.clone()))
                .collect::<Vec<_>>(),
        )
    });
    while mac.numberOfItems() > FIXED_ITEMS {
        mac.removeItemAtIndex(FIXED_ITEMS);
    }
    if !ms.is_empty() {
        mac.addItem(&NSMenuItem::separatorItem(mtm));
        add_items(mtm, &mac, ms, sel!(macroRunSaved:), t);
    }
    while run.numberOfItems() > 1 {
        run.removeItemAtIndex(1);
    }
    if !cs.is_empty() {
        run.addItem(&NSMenuItem::separatorItem(mtm));
        add_items(mtm, &run, cs, sel!(runUserCommand:), t);
    }
}

// Port of Notepad_plus::checkMacroState; Start and Stop share the Notepad++ toggle key, so only the enabled one keeps it.
fn check_state() {
    S.with(|s| {
        let Some([m, _]) = s.menus.get() else { return };
        let (rec, empty) = (s.recording.get(), s.current.borrow().is_empty());
        let has_saved = !s.store.borrow().macros.is_empty();
        let on = [
            !rec,
            rec,
            !empty && !rec,
            !empty && !rec && !s.saved.get(),
            (!empty && !rec) || has_saved,
        ];
        for (i, e) in on.iter().enumerate() {
            if let Some(it) = m.itemAtIndex(i as isize) {
                it.setEnabled(*e);
                if i < 2 {
                    it.setKeyEquivalent(&ns(if *e { "R" } else { "" }));
                }
            }
        }
    });
}

pub fn menus(mtm: MainThreadMarker, bar: &NSMenu, t: Option<&AnyObject>) {
    let items = [
        ("Start Recording", sel!(macroToggleRecord:), ""),
        ("Stop Recording", sel!(macroToggleRecord:), ""),
        ("Playback", sel!(macroPlayback:), "P"),
        ("Save Current Recorded Macro...", sel!(macroSave:), ""),
        ("Run a Macro Multiple Times...", sel!(macroShowMulti:), ""),
    ];
    let mac = crate::nested(
        mtm,
        "Macro",
        items
            .iter()
            .map(|(n, a, k)| crate::item(mtm, n, *a, k, t))
            .collect(),
    );
    let run = crate::nested(mtm, "Run", crate::run::run_menu_items(mtm, t));
    run.submenu()
        .unwrap()
        .itemAtIndex(0)
        .unwrap()
        .setKeyEquivalentModifierMask(objc2_app_kit::NSEventModifierFlags::empty());
    bar.addItem(&mac);
    bar.addItem(&run);
    let mac = mac.submenu().unwrap();
    mac.setAutoenablesItems(false);
    S.with(|s| {
        let _ = s.menus.set([mac, run.submenu().unwrap()]);
        match load() {
            Ok(st) => *s.store.borrow_mut() = st,
            Err(e) => *s.load_error.borrow_mut() = Some(e),
        }
    });
    rebuild(mtm, t);
    check_state();
    if let Some(t) = t {
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                t,
                sel!(macroMenuDidSend:),
                Some(NSMenuDidSendActionNotification),
                None,
            )
        };
        if S.with(|s| s.load_error.borrow().is_some()) {
            let _: () = unsafe {
                msg_send![t, performSelector: sel!(macroLoadError:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]
            };
        }
    }
}

fn recording() -> bool {
    S.with(|s| s.recording.get())
}

impl App {
    pub(crate) fn macro_show_load_error(&self) {
        if let Some(e) = S.with(|s| s.load_error.borrow().clone()) {
            self.alert("Cannot read shortcuts.xml", &format!("{e}\nThe saved macros and commands are not loaded. The app does not change the file."), &["OK"]);
        }
    }

    // Records in a new tab too while a recording runs.
    pub(crate) fn macro_arm(&self, v: &NSView) {
        if recording() {
            sci::send(v, SCI_STARTRECORD, 0, 0);
        }
    }

    fn macro_tab_views(&self) -> Vec<Retained<NSView>> {
        self.ivars()
            .tabs
            .borrow()
            .iter()
            .map(|t| t.view.clone())
            .collect()
    }

    fn macro_arm_tabs(&self) {
        self.macro_tab_views()
            .iter()
            .for_each(|v| _ = sci::send(v, SCI_STARTRECORD, 0, 0));
    }

    pub(crate) fn macro_toggle_record(&self) {
        let rec = recording();
        if rec {
            for v in self.macro_tab_views() {
                sci::send(&v, SCI_STOPRECORD, 0, 0);
            }
        } else {
            S.with(|s| s.current.borrow_mut().clear());
            self.macro_arm_tabs();
        }
        S.with(|s| {
            s.recording.set(!rec);
            s.saved.set(false);
        });
        check_state();
        self.macro_fill_list();
    }

    // Port of the SCN_MACRORECORD handler of NppNotification.cpp.
    pub(crate) fn macro_record(&self, scn: *const c_void) {
        let n = unsafe { &*(scn as *const Scn) };
        // CLEARALL and APPENDTEXT come only from the app (for example Reload from Disk), not from the user.
        if !recording() || n.id_from == sci::RESULTS_ID || matches!(n.message, 2004 | 2282) {
            return;
        }
        let s = (n.l_param != 0 && shortcuts::is_string_message(n.message)).then(|| {
            let p = n.l_param as *const c_char;
            // ADDTEXT, ADDSTYLEDTEXT and APPENDTEXT give the length in wParam.
            if matches!(n.message, 2001 | 2002 | 2282) {
                let b = unsafe { std::slice::from_raw_parts(p as *const u8, n.w_param) };
                String::from_utf8_lossy(b).into_owned()
            } else {
                unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
            }
        });
        let crlf = self
            .editor()
            .is_some_and(|v| sci::eol_mode(&v) == crate::encoding::SC_EOL_CRLF);
        S.with(|st| {
            shortcuts::record(
                &mut st.current.borrow_mut(),
                n.message,
                n.w_param,
                n.l_param,
                s.as_deref(),
                crlf,
            )
        });
    }

    // Records the menu commands that Notepad++ records as type 2 steps.
    pub(crate) fn macro_menu_did_send(&self, n: &NSNotification) {
        if !recording() {
            return;
        }
        self.macro_arm_tabs();
        let item: Option<Retained<AnyObject>> = n
            .userInfo()
            .and_then(|d| unsafe { msg_send![&d, objectForKey: &*ns("MenuItem")] });
        let Some(item) = item.and_then(|i| i.downcast::<NSMenuItem>().ok()) else {
            return;
        };
        let Some(action) = item.action() else { return };
        let main_key = NSApplication::sharedApplication(self.mtm())
            .keyWindow()
            .as_deref()
            == self.ivars().window.get().map(|w| &**w);
        if item.target().is_none() && !main_key {
            return;
        }
        if let Some(step) = menu_step(action.name().to_str().unwrap_or(""), item.tag()) {
            S.with(|s| s.current.borrow_mut().push(step));
        }
    }

    fn macro_menu_command(&self, id: i32) {
        let Some((name, tag)) = menu_action(id) else {
            return;
        };
        let action = Sel::register(&CString::new(name).unwrap());
        let app = NSApplication::sharedApplication(self.mtm());
        let Some((m, i)) = app.mainMenu().and_then(|m| find_item(&m, action, tag)) else {
            return;
        };
        let it = m.itemAtIndex(i).unwrap();
        if it.target().is_some() {
            m.performActionForItemAtIndex(i);
        } else if let Some(v) = self.editor() {
            unsafe { app.sendAction_to_from(action, Some(&sci::content(&v)), Some(&it)) };
        }
    }

    // Port of FindReplaceDlg::execSavedCommand for Find Next, Replace and Replace All.
    fn macro_search(&self, step: &Step, env: &mut SnR) {
        match step.message {
            IDC_FRCOMMAND_INIT => *env = SnR::default(),
            IDFINDWHAT => env.find = step.s.clone(),
            IDREPLACEWITH => env.replace = step.s.clone(),
            IDNORMAL => env.mode = step.l,
            IDC_FRCOMMAND_BOOLEANS => env.flags = step.l,
            IDC_FRCOMMAND_EXEC => {
                let Some(v) = self.editor() else { return };
                let o = env.opts();
                let up = match step.l {
                    IDC_FINDNEXT => false,
                    IDC_FINDPREV => true,
                    _ => env.flags & IDF_WHICH_DIRECTION == 0,
                };
                if up && o.regex() {
                    return;
                }
                let doc = sci::doc(&v);
                match step.l {
                    IDOK | IDC_FINDNEXT | IDC_FINDPREV => {
                        if let Ok(Some((m, _))) =
                            search::find_next(&doc, &o, sci::selection(&v), up, Next::Find)
                        {
                            sci::select(&v, m);
                        }
                    }
                    IDREPLACE if !o.find.is_empty() => _ = self.replace_once(&v, &o),
                    IDREPLACEALL => {
                        let start = if o.wrap { 0 } else { sci::selection(&v).0 };
                        _ = search::replace_all(&doc, &o, (start, doc.len()));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Port of Notepad_plus::macroPlayback: each touched document gets one undo action, which `views` keeps open.
    fn macro_play(&self, m: &[Step], views: &mut Vec<Retained<NSView>>) {
        let mut env = SnR::default();
        for step in m {
            let Some(v) = self.editor() else { return };
            if !views.contains(&v) {
                sci::send(&v, SCI_BEGINUNDOACTION, 0, 0);
                views.push(v.clone());
            }
            match step.kind {
                TYPE_MENU => self.macro_menu_command(step.w as i32),
                TYPE_SNR => self.macro_search(step, &mut env),
                _ if !step.is_macroable() => {}
                TYPE_S => {
                    let s = CString::new(step.s.replace('\0', "")).unwrap();
                    // ADDTEXT and APPENDTEXT read wParam bytes, so wParam must be the real length.
                    let w = match step.message {
                        2001 | 2282 => s.as_bytes().len(),
                        _ => step.w,
                    };
                    sci::send(&v, step.message as u32, w, s.as_ptr() as isize);
                }
                _ => _ = sci::send(&v, step.message as u32, step.w, step.l),
            }
        }
    }

    fn macro_end_undo(views: Vec<Retained<NSView>>) {
        for v in views.iter().rev() {
            sci::send(v, SCI_ENDUNDOACTION, 0, 0);
        }
    }

    fn macro_run(&self, m: &[Step]) {
        let mut views = vec![];
        self.macro_play(m, &mut views);
        Self::macro_end_undo(views);
    }

    pub(crate) fn macro_playback(&self) {
        if recording() {
            return;
        }
        let m = S.with(|s| s.current.borrow().clone());
        self.macro_run(&m);
    }

    pub(crate) fn macro_run_saved(&self, s: &NSMenuItem) {
        let m = with_store(|st| st.macros.get(s.tag() as usize).map(|m| m.steps.clone()));
        if let Some(m) = m {
            self.macro_run(&m);
        }
    }

    // Name part of the Notepad++ Shortcut dialog; the key is not asked and stays empty.
    pub(crate) fn ask_shortcut_name(&self) -> Option<String> {
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&ns("Shortcut"));
        a.setInformativeText(&ns("Name:"));
        let f = NSTextField::textFieldWithString(&NSString::new(), self.mtm());
        f.setFrame(NSRect::new(NSPoint::new(0., 0.), NSSize::new(260., 24.)));
        a.setAccessoryView(Some(&f));
        a.addButtonWithTitle(&ns("OK"));
        a.addButtonWithTitle(&ns("Cancel"));
        a.window().setInitialFirstResponder(Some(&f));
        let ok = a.runModal() == NSAlertFirstButtonReturn;
        let name = panel::text(&f).trim().to_string();
        (ok && !name.is_empty()).then_some(name)
    }

    pub(crate) fn store_changed(&self) {
        if let Err(e) = save() {
            self.alert("Cannot save shortcuts.xml", &e, &["OK"]);
        }
        let t: &AnyObject = self;
        rebuild(self.mtm(), Some(t));
        check_state();
        self.macro_fill_list();
    }

    pub(crate) fn macro_save(&self) {
        let Some(name) = self.ask_shortcut_name() else {
            return;
        };
        let steps = S.with(|s| s.current.borrow().clone());
        with_store(|s| {
            s.macros.push(Macro {
                name,
                steps,
                ..Macro::default()
            })
        });
        S.with(|s| s.saved.set(true));
        self.store_changed();
    }

    fn with_multi<R>(&self, f: impl FnOnce(&MultiUi) -> R) -> R {
        S.with(|s| {
            f(s.multi.get_or_init(|| {
                let mtm = self.mtm();
                let t: &AnyObject = self;
                let f = Form::new(mtm, "Run a Macro Multiple Times", 340., 170.);
                f.label("Macro to run", 16., 10., 300.);
                let list = NSPopUpButton::new(mtm);
                f.place(&list, 16., 34., 308., 26.);
                let radio = |title: &str, top: f64, w: f64| {
                    let b = unsafe {
                        NSButton::radioButtonWithTitle_target_action(
                            &ns(title),
                            Some(t),
                            Some(sel!(macroRunMode:)),
                            mtm,
                        )
                    };
                    f.place(&b, 24., top, w, 20.);
                    b
                };
                let multi = radio("Run", 74., 60.);
                let times = f.field(86., 72., 50.);
                times.setStringValue(&ns("1"));
                f.label("times", 142., 72., 80.);
                let eof = radio("Run until the end of file", 100., 260.);
                panel::set_on(&multi, true);
                panel::set_on(&eof, false);
                f.button("Run", 76., 130., 90., t, sel!(macroRunMulti:))
                    .setKeyEquivalent(&ns("\r"));
                f.button("Cancel", 174., 130., 90., t, sel!(closePanel:))
                    .setKeyEquivalent(&ns("\u{1b}"));
                MultiUi {
                    form: f,
                    list,
                    times,
                    multi,
                }
            }))
        })
    }

    // Port of RunMacroDlg::initMacroList.
    fn macro_fill_list(&self) {
        if S.with(|s| s.multi.get().is_none()) {
            return;
        }
        let mut names = vec![];
        S.with(|s| {
            if !s.recording.get() && !s.current.borrow().is_empty() {
                names.push("Current recorded macro".to_string());
            }
            names.extend(s.store.borrow().macros.iter().map(|m| m.name.clone()));
        });
        self.with_multi(|u| {
            u.list.removeAllItems();
            for n in &names {
                u.list.addItemWithTitle(&ns(n));
            }
            u.list.selectItemAtIndex(0);
        });
    }

    pub(crate) fn macro_show_multi(&self) {
        if recording() {
            return;
        }
        self.with_multi(|_| ());
        self.macro_fill_list();
        self.with_multi(|u| u.form.panel.makeKeyAndOrderFront(None));
    }

    pub(crate) fn macro_run_mode(&self) {
        self.with_multi(|u| u.times.setEnabled(panel::on(&u.multi)));
    }

    // Port of the WM_MACRODLGRUNMACRO handler: all runs together make one undo action per document.
    pub(crate) fn macro_run_multi(&self) {
        if recording() {
            return;
        }
        let (idx, times) = self.with_multi(|u| {
            let n = panel::text(&u.times)
                .trim()
                .parse::<usize>()
                .unwrap_or(1)
                .max(1);
            u.times.setStringValue(&ns(&n.to_string()));
            (
                u.list.indexOfSelectedItem(),
                panel::on(&u.multi).then_some(n),
            )
        });
        if idx < 0 {
            return;
        }
        let m = S.with(|s| {
            let cur = !s.current.borrow().is_empty();
            match (cur, idx) {
                (true, 0) => Some(s.current.borrow().clone()),
                (true, i) => s
                    .store
                    .borrow()
                    .macros
                    .get(i as usize - 1)
                    .map(|m| m.steps.clone()),
                (false, i) => s
                    .store
                    .borrow()
                    .macros
                    .get(i as usize)
                    .map(|m| m.steps.clone()),
            }
        });
        let (Some(m), Some(v)) = (m, self.editor()) else {
            return;
        };
        let line = |v: &NSView| {
            let caret = sci::send(v, SCI_GETCURRENTPOS, 0, 0);
            (
                sci::send(v, SCI_GETLINECOUNT, 0, 0),
                sci::send(v, sci::SCI_LINEFROMPOSITION, caret as usize, 0),
            )
        };
        let (count, cur) = line(&v);
        let mut eof = UntilEof::new(count, cur);
        let mut views = vec![];
        let mut n = 0;
        loop {
            self.macro_play(&m, &mut views);
            n += 1;
            match times {
                Some(t) if n >= t => break,
                Some(_) => {}
                None => {
                    let Some(v) = self.editor() else { break };
                    let (count, cur) = line(&v);
                    if !eof.again(count, cur) {
                        break;
                    }
                }
            }
        }
        Self::macro_end_undo(views);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id_of(src: &str, n: &str) -> i32 {
        let l = src
            .lines()
            .find(|l| l.split_whitespace().take(2).eq(["#define", n]))
            .unwrap_or_else(|| panic!("{n}"));
        let v: String = l.split_whitespace().skip(2).collect();
        let v = v.trim_matches(|c| c == '(' || c == ')');
        match v.split_once('+') {
            Some((b, o)) => id_of(src, b) + o.parse::<i32>().unwrap(),
            None => v.parse().unwrap(),
        }
    }

    #[test]
    fn menu_steps() {
        assert_eq!(menu_step("newDocument:", 0), Some(Step::menu(41001)));
        assert_eq!(menu_step("eolConvert:", 2), Some(Step::menu(45002)));
        assert_eq!(menu_step("eolConvert:", 1), Some(Step::menu(45003)));
        assert_eq!(menu_step("cut:", 0), None);
        assert_eq!(menu_action(42001), Some(("cut:", -1)));
        assert_eq!(
            menu_step("paste:", 0),
            Some(Step::new(shortcuts::TYPE_L, 2179, 0, 0, ""))
        );
        assert_eq!(menu_step("macroPlayback:", 0), None);
        assert_eq!(menu_action(42007), Some(("selectAll:", -1)));
        assert_eq!(menu_action(42024), None);
        let src = include_str!("../../PowerEditor/src/menuCmdID.h");
        for (name, id, _, tag) in menu_cmds() {
            let name = match name.strip_suffix('_').or(name.strip_suffix("TAB")) {
                Some(_) => format!("{name}{}", tag + 1),
                None => name.to_string(),
            };
            assert_eq!(id_of(src, &name), id, "{name}");
        }
        let menus = [
            include_str!("main.rs"),
            include_str!("view.rs"),
            include_str!("fileops.rs"),
            include_str!("edit.rs"),
        ]
        .concat();
        for (_, _, action, _) in menu_cmds() {
            assert!(menus.contains(&format!("sel!({action})")), "{action}");
        }
        let view = include_str!("view.rs");
        for c in [
            "NEXT: usize = 11",
            "PREV: usize = 12",
            "TO_START: usize = 0",
            "TO_END: usize = 1",
            "FORWARD: usize = 2",
            "BACKWARD: usize = 3",
        ] {
            assert!(view.contains(c), "{c}");
        }
        use crate::fileops::Close;
        assert_eq!(
            [
                Close::All,
                Close::ButActive,
                Close::Left,
                Close::Right,
                Close::Unchanged
            ]
            .map(|c| c as isize),
            [0, 1, 2, 3, 4]
        );
        assert_eq!(menu_step("selectTab:", 3), Some(Step::menu(44089)));
        assert_eq!(menu_step("selectTab:", 9), None);
        assert_eq!(menu_step("unfoldLevel:", 7), Some(Step::menu(44068)));
    }

    #[test]
    fn file_read_and_write() {
        let dir = std::env::temp_dir().join(format!("npp-macros-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("sub/shortcuts.xml");
        assert_eq!(read_file(&p), Ok(None));
        write_file(&p, "one").unwrap();
        write_file(&p, "two").unwrap();
        assert_eq!(read_file(&p), Ok(Some("two".into())));
        assert!(!p.with_extension("xml.tmp").exists());
        std::fs::write(&p, b"\xff\xfe<").unwrap();
        assert!(read_file(&p).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn run(lines: isize, mut f: impl FnMut(&mut isize, &mut isize)) -> usize {
        let (mut count, mut cur) = (lines, 0);
        let mut e = UntilEof::new(count, cur);
        let mut n = 0;
        loop {
            f(&mut count, &mut cur);
            n += 1;
            if n > 1000 || !e.again(count, cur) {
                return n;
            }
        }
    }

    #[test]
    fn until_end_of_file() {
        assert_eq!(run(5, |c, l| *l = (*l + 1).min(*c - 1)), 5);
        assert_eq!(run(5, |_, _| {}), 1);
        assert_eq!(run(5, |c, _| *c = (*c - 1).max(1)), 5);
        assert_eq!(
            run(3, |c, l| {
                *c += 1;
                *l += 2
            }),
            3
        );
    }
}
