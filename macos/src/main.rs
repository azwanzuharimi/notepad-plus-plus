// SPDX-License-Identifier: GPL-3.0-or-later
mod autoc;
mod backup;
mod binary;
mod comment;
mod column;
mod config;
mod docking;
mod edit;
mod encoding;
mod fileops;
mod finder;
mod funclist;
mod incsearch;
mod lang;
mod language;
mod macros;
mod mark;
mod panel;
mod prefs;
mod run;
mod sci;
mod search;
mod search_extras;
mod shortcuts;
mod session;
mod style_dlg;
mod styler;
mod tools;
mod udl;
mod view;
mod window;

use encoding::Enc;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSApplicationTerminateReply,
    NSAutoresizingMaskOptions, NSBackingStoreType, NSButton, NSControl, NSControlStateValueOff,
    NSControlStateValueOn, NSEvent, NSMenu, NSMenuDelegate, NSMenuItem, NSModalResponseOK,
    NSOpenPanel, NSOutlineView, NSOutlineViewDataSource, NSSplitView, NSTableColumn,
    NSTableView, NSTableViewDataSource, NSSplitViewDividerStyle, NSTabView, NSTabViewDelegate, NSTabViewItem,
    NSTextField, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use panel::{Controls, Form};
use search::{FifArgs, FifOut, Line, Next, Wrap};
use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const STATUS_H: f64 = 22.;
// Notepad++ status bar part widths; 0 takes the rest.
const STATUS_WIDTHS: [f64; 6] = [0., 220., 260., 110., 120., 40.];

static FIF_DONE: Mutex<Option<(FifArgs, Result<FifOut, String>)>> = Mutex::new(None);

fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

struct FindUi {
    form: Form,
    c: Controls,
    prev: Retained<NSButton>,
}

struct FifUi {
    form: Form,
    c: Controls,
    filters: Retained<NSTextField>,
    dir: Retained<NSTextField>,
    sub: Retained<NSButton>,
    hidden: Retained<NSButton>,
}

fn cfg() -> &'static config::Config {
    styler::cfg()
}

#[derive(Clone)]
struct Tab {
    view: Retained<NSView>,
    item: Retained<NSTabViewItem>,
    path: Option<PathBuf>,
    name: String,
    enc: Enc,
    enc_dirty: bool,
    lost: bool,
    ro: bool,
    lang: Option<String>,
}

#[derive(Default)]
struct Ivars {
    window: OnceCell<Retained<NSWindow>>,
    tab_view: OnceCell<Retained<NSTabView>>,
    tabs: RefCell<Vec<Tab>>,
    untitled: Cell<u32>,
    split: OnceCell<Retained<NSSplitView>>,
    results: OnceCell<(Retained<NSView>, usize)>,
    result_lines: RefCell<Vec<Line>>,
    pending_hit: Cell<(isize, isize)>,
    fif_running: Cell<bool>,
    replacing: Cell<bool>,
    find_ui: OnceCell<FindUi>,
    fif_ui: OnceCell<FifUi>,
    status: OnceCell<Vec<Retained<NSTextField>>>,
    view: Cell<view::Opts>,
    begin_select: Cell<Option<(isize, bool)>>,
    dock: OnceCell<docking::Dock>,
}

#[repr(C)]
struct NotifyHeader {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
    position: isize,
}

define_class!(
    // Makes Escape cancel a modal alert whose Cancel button keeps the Return key.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct EscapeCancels;

    impl EscapeCancels {
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, e: &NSEvent) -> bool {
            let esc = e.keyCode() == 53;
            if esc {
                NSApplication::sharedApplication(self.mtm()).stopModalWithCode(NSAlertSecondButtonReturn);
            }
            esc
        }
    }
);

impl EscapeCancels {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct App;

    impl App {
        #[unsafe(method(newDocument:))]
        fn new_document(&self, _s: Option<&AnyObject>) {
            self.add_tab(None, Enc::Utf8, b"", false);
        }

        #[unsafe(method(openDocument:))]
        fn open_document(&self, _s: Option<&AnyObject>) {
            let p = NSOpenPanel::openPanel(self.mtm());
            p.setAllowsMultipleSelection(true);
            let cur = self.current().and_then(|i| self.tab(i)?.path);
            if let Some(d) = prefs::dialog_dir(cur.as_deref()) {
                p.setDirectoryURL(Some(&objc2_foundation::NSURL::fileURLWithPath(&ns(&d.to_string_lossy()))));
            }
            if p.runModal() == NSModalResponseOK {
                for url in p.URLs() {
                    if let Some(path) = url.path() {
                        prefs::used_file(Path::new(&path.to_string()));
                        self.open_path(Path::new(&path.to_string()));
                    }
                }
            }
        }

        #[unsafe(method(saveDocument:))]
        fn save_document(&self, _s: Option<&AnyObject>) {
            if let Some(i) = self.current() {
                self.save(i, false);
            }
        }

        #[unsafe(method(saveDocumentAs:))]
        fn save_document_as(&self, _s: Option<&AnyObject>) {
            if let Some(i) = self.current() {
                self.save(i, true);
            }
        }

        #[unsafe(method(closeTab:))]
        fn close_tab(&self, _s: Option<&AnyObject>) {
            let Some(i) = self.current() else { return };
            if !self.confirm_close(i) {
                return;
            }
            if let Some(tab) = self.tab(i) {
                self.drop_tabs(&[tab.item]);
            }
        }

        #[unsafe(method(notification:))]
        fn notification(&self, scn: *const c_void) {
            let h = unsafe { &*(scn as *const NotifyHeader) };
            self.margin_click(scn);
            if h.code == macros::SCN_MACRORECORD {
                self.macro_record(scn);
            }
            self.autoc_notify(scn);
            backup::notify(scn);
            if h.code == sci::SCN_SAVEPOINTREACHED || h.code == sci::SCN_SAVEPOINTLEFT {
                let n = self.ivars().tabs.borrow().len();
                (0..n).for_each(|i| self.refresh_title(i));
            }
            if h.code == sci::SCN_UPDATEUI && h.id_from != sci::RESULTS_ID {
                self.update_status();
                self.schedule_smart_highlight();
                self.function_list_mark();
            }
            if h.code == sci::SCN_DOUBLECLICK && h.id_from == sci::RESULTS_ID {
                let v = &self.ivars().results.get().unwrap().0;
                let pos = if h.position < 0 { sci::selection(v).1 } else { h.position };
                let line = sci::send(v, sci::SCI_LINEFROMPOSITION, pos as usize, 0);
                let start = sci::send(v, sci::SCI_POSITIONFROMLINE, line as usize, 0);
                self.ivars().pending_hit.set((line, pos - start));
                let _: () = unsafe {
                    msg_send![self, performSelector: sel!(openResult:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]
                };
            }
        }

        #[unsafe(method(openResult:))]
        fn open_result(&self, _s: Option<&AnyObject>) {
            let (line, at) = self.ivars().pending_hit.get();
            if self.toggle_result_header(line) {
                return;
            }
            let hit = self.ivars().result_lines.borrow().get(line as usize).and_then(|l| Some((l.hit.clone()?, l.marks.clone())));
            let Some(((path, ranges), marks)) = hit else { return };
            let k = marks.iter().position(|&(s, e)| s <= at && at <= e).unwrap_or(0);
            self.open_path(&path);
            let tab = self.current().and_then(|i| self.tab(i));
            if let Some(t) = tab.filter(|t| t.path.as_deref().is_some_and(|p| fileops::same_file(p, &path))) {
                sci::select(&t.view, ranges[k]);
                self.ivars().window.get().unwrap().makeKeyAndOrderFront(None);
                self.focus();
            }
        }

        #[unsafe(method(showFind:))]
        fn show_find(&self, _s: Option<&AnyObject>) {
            self.open_find(false);
        }

        #[unsafe(method(showReplace:))]
        fn show_replace(&self, _s: Option<&AnyObject>) {
            self.open_find(true);
        }

        #[unsafe(method(findNext:))]
        fn find_next(&self, _s: Option<&AnyObject>) {
            self.find(false);
        }

        #[unsafe(method(findPrevious:))]
        fn find_previous(&self, _s: Option<&AnyObject>) {
            self.find(true);
        }

        #[unsafe(method(count:))]
        fn count(&self, _s: Option<&AnyObject>) {
            let (c, o) = (&self.find_ui().c, self.find_ui().c.opts());
            let Some(v) = self.editor() else { return };
            let doc = sci::doc(&v);
            let start = if o.wrap { 0 } else { sci::selection(&v).0 };
            c.set_status(&match search::process(&doc, &o, false, false, (start, doc.len())) {
                Ok(m) => search::count_status(m.len(), &o),
                Err(e) => e,
            });
        }

        #[unsafe(method(replaceAll:))]
        fn replace_all(&self, _s: Option<&AnyObject>) {
            let (c, o) = (&self.find_ui().c, self.find_ui().c.opts());
            let Some(v) = self.editor() else { return };
            let doc = sci::doc(&v);
            let start = if o.wrap { 0 } else { sci::selection(&v).0 };
            c.set_status(&match search::replace_all(&doc, &o, (start, doc.len())) {
                Ok(n) => search::replace_all_status(n, &o),
                Err(e) => e,
            });
        }

        #[unsafe(method(replace:))]
        fn replace(&self, _s: Option<&AnyObject>) {
            let (c, o) = (&self.find_ui().c, self.find_ui().c.opts());
            let Some(v) = self.editor() else { return };
            if o.find.is_empty() {
                return;
            }
            c.set_status(&self.replace_once(&v, &o).unwrap_or_else(|e| e));
        }

        #[unsafe(method(closePanel:))]
        fn close_panel(&self, s: Option<&AnyObject>) {
            if let Some(s) = s {
                let w: Option<Retained<NSWindow>> = unsafe { msg_send![s, window] };
                if let Some(w) = w {
                    w.orderOut(None);
                }
            }
        }

        #[unsafe(method(searchModeChanged:))]
        fn search_mode_changed(&self, _s: Option<&AnyObject>) {
            if let Some(u) = self.ivars().find_ui.get() {
                u.c.mode_changed();
                u.prev.setEnabled(!panel::on(&u.c.modes[2]));
            }
            if let Some(u) = self.ivars().fif_ui.get() {
                u.c.mode_changed();
            }
        }

        #[unsafe(method(showFindInFiles:))]
        fn show_find_in_files(&self, _s: Option<&AnyObject>) {
            let u = self.fif_ui();
            if panel::text(&u.dir).is_empty() {
                let dir = self.current().and_then(|i| self.tab(i)?.path?.parent().map(Path::to_path_buf));
                if let Some(d) = dir {
                    u.dir.setStringValue(&ns(&d.to_string_lossy()));
                }
            }
            self.show_panel(&u.form, &u.c.find, &u.c.find);
        }

        #[unsafe(method(browseDir:))]
        fn browse_dir(&self, _s: Option<&AnyObject>) {
            let p = NSOpenPanel::openPanel(self.mtm());
            p.setCanChooseDirectories(true);
            p.setCanChooseFiles(false);
            if p.runModal() == NSModalResponseOK {
                if let Some(path) = p.URL().and_then(|u| u.path()) {
                    self.fif_ui().dir.setStringValue(&path);
                }
            }
        }

        #[unsafe(method(findAll:))]
        fn find_all(&self, _s: Option<&AnyObject>) {
            self.start_fif(false);
        }

        #[unsafe(method(replaceInFiles:))]
        fn replace_in_files(&self, _s: Option<&AnyObject>) {
            self.start_fif(true);
        }

        #[unsafe(method(fifDone:))]
        fn fif_done(&self, _s: Option<&AnyObject>) {
            let Some((args, r)) = FIF_DONE.lock().unwrap().take() else { return };
            let replace = args.replace;
            self.ivars().fif_running.set(false);
            if replace {
                self.ivars().replacing.set(false);
                self.set_tabs_read_only(false);
            }
            let c = &self.fif_ui().c;
            match r {
                Err(e) => c.set_status(&e),
                Ok(mut out) if replace => {
                    let not_reloaded = self.reload_changed(&out.changed);
                    c.set_status(&match self.replace_in_tabs(&args.opts, &mut out) {
                        Ok(()) => search::replace_in_files_status(&out, &not_reloaded),
                        Err(e) => e,
                    });
                }
                Ok(out) => {
                    c.set_status("");
                    self.show_results(out.lines);
                }
            }
        }

        #[unsafe(method(encodeIn:))]
        fn encode_in(&self, s: &NSMenuItem) {
            let Some(i) = self.current() else { return };
            let Some(t) = self.tab(i) else { return };
            let e = tag_enc(s.tag());
            if !matches!((t.enc, e), (Enc::Cp(_), _) | (_, Enc::Cp(_))) {
                if t.enc != e && (t.enc == Enc::Ansi || e == Enc::Ansi) {
                    let (text, lost) = encoding::reinterpret(&sci::bytes(&t.view), t.enc, e);
                    sci::replace_text(&t.view, &text);
                    if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
                        t.lost |= lost;
                    }
                }
                if t.enc != e {
                    self.set_enc(i, e, t.enc_dirty || should_be_dirty(t.enc, e));
                }
                return;
            }
            if self.dirty(&t) {
                let r = self.alert(
                    "Save Current Modification",
                    "You should save the current modification.\nAll the saved modifications cannot be undone.\n\nContinue?",
                    &["Yes", "No"],
                );
                if r != NSAlertFirstButtonReturn || !self.save(i, false) {
                    return;
                }
            }
            let Some(path) = self.tab(i).and_then(|t| t.path) else {
                self.set_enc(i, e, false);
                return;
            };
            match std::fs::read(&path) {
                Ok(b) => self.load_into(i, &b, e),
                Err(err) => {
                    self.alert(&format!("Cannot open {}", path.display()), &err.to_string(), &["OK"]);
                }
            }
        }

        #[unsafe(method(convertTo:))]
        fn convert_to(&self, s: &NSMenuItem) {
            let Some(i) = self.current() else { return };
            let Some(t) = self.tab(i) else { return };
            let e = tag_enc(s.tag());
            if t.enc != e && !(e == Enc::Ansi && matches!(t.enc, Enc::Cp(_))) {
                self.set_enc(i, e, true);
            }
        }

        #[unsafe(method(eolConvert:))]
        fn eol_convert(&self, s: &NSMenuItem) {
            if let Some(v) = self.editor() {
                sci::convert_eols(&v, s.tag() as usize);
                self.update_status();
            }
        }

        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            self.validate(item)
        }

        #[unsafe(method(goToLine:))]
        fn go_to_line(&self, _s: Option<&AnyObject>) {
            let Some(v) = self.editor() else { return };
            let (cur, max) = sci::line_info(&v);
            let a = NSAlert::new(self.mtm());
            a.setMessageText(&ns("Go To..."));
            a.setInformativeText(&ns(&format!("You are here: {cur}\nYou can't go further than: {max}")));
            let f = NSTextField::textFieldWithString(&NSString::new(), self.mtm());
            f.setFrame(NSRect::new(NSPoint::new(0., 0.), NSSize::new(200., 24.)));
            f.setPlaceholderString(Some(&ns("You want to go to")));
            a.setAccessoryView(Some(&f));
            a.addButtonWithTitle(&ns("Go"));
            a.addButtonWithTitle(&ns("Cancel"));
            a.window().setInitialFirstResponder(Some(&f));
            if a.runModal() == NSAlertFirstButtonReturn {
                if let Ok(n) = panel::text(&f).trim().parse::<isize>() {
                    sci::goto_line(&v, n.min(max));
                }
            }
            self.focus();
        }
    }

    impl App {
        #[unsafe(method(hashToClipboard:))]
        fn hash_to_clipboard_action(&self, s: &NSMenuItem) {
            self.hash_to_clipboard(s);
        }

        #[unsafe(method(hashGenerate:))]
        fn hash_generate(&self, s: &NSMenuItem) {
            self.hash_show(s, false);
        }

        #[unsafe(method(hashFromFiles:))]
        fn hash_from_files(&self, s: &NSMenuItem) {
            self.hash_show(s, true);
        }

        #[unsafe(method(hashChooseFiles:))]
        fn hash_choose_files_action(&self, _s: Option<&AnyObject>) {
            self.hash_choose_files();
        }

        #[unsafe(method(hashEachLine:))]
        fn hash_each_line(&self, _s: Option<&AnyObject>) {
            self.hash_text_changed();
        }

        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, _n: &NSNotification) {
            self.hash_text_changed();
        }

        #[unsafe(method(hashCopy:))]
        fn hash_copy_action(&self, s: &NSButton) {
            self.hash_copy(s.tag() == 1);
        }

        #[unsafe(method(showAbout:))]
        fn show_about_action(&self, _s: Option<&AnyObject>) {
            self.show_about();
        }

        #[unsafe(method(showCmdLineArgs:))]
        fn show_cmd_line_args_action(&self, _s: Option<&AnyObject>) {
            self.show_cmd_line_args();
        }

        #[unsafe(method(openLink:))]
        fn open_link_action(&self, s: &NSMenuItem) {
            self.open_link(s);
        }

        #[unsafe(method(showDebugInfo:))]
        fn show_debug_info_action(&self, _s: Option<&AnyObject>) {
            self.show_debug_info();
        }
    }

    impl App {
        #[unsafe(method(reloadFromDisk:))]
        fn reload_from_disk_action(&self, _s: Option<&AnyObject>) {
            self.reload_from_disk();
        }

        #[unsafe(method(saveCopyAs:))]
        fn save_copy_as_action(&self, _s: Option<&AnyObject>) {
            self.save_copy_as();
        }

        #[unsafe(method(saveAll:))]
        fn save_all_action(&self, _s: Option<&AnyObject>) {
            self.save_all();
        }

        #[unsafe(method(renameFile:))]
        fn rename_file_action(&self, _s: Option<&AnyObject>) {
            self.rename_file();
        }

        #[unsafe(method(closeMultiple:))]
        fn close_multiple_action(&self, s: Option<&AnyObject>) {
            let tag: isize = s.map_or(0, |s| unsafe { msg_send![s, tag] });
            self.close_multiple(tag);
        }

        #[unsafe(method(moveToTrash:))]
        fn move_to_trash_action(&self, _s: Option<&AnyObject>) {
            self.move_to_trash();
        }

        #[unsafe(method(openFolderFinder:))]
        fn open_folder_finder(&self, _s: Option<&AnyObject>) {
            self.open_folder(false);
        }

        #[unsafe(method(openFolderTerminal:))]
        fn open_folder_terminal(&self, _s: Option<&AnyObject>) {
            self.open_folder(true);
        }

        #[unsafe(method(openDefaultViewer:))]
        fn open_default_viewer_action(&self, _s: Option<&AnyObject>) {
            self.open_default_viewer();
        }
    }

    impl App {
        #[unsafe(method(alwaysOnTop:))]
        fn always_on_top_action(&self, _s: Option<&AnyObject>) {
            self.always_on_top();
        }

        #[unsafe(method(fullScreen:))]
        fn full_screen_action(&self, _s: Option<&AnyObject>) {
            self.full_screen();
        }

        #[unsafe(method(viewOption:))]
        fn view_option_action(&self, s: &NSMenuItem) {
            self.view_option(s.tag() as usize);
        }

        #[unsafe(method(zoom:))]
        fn zoom_action(&self, s: &NSMenuItem) {
            self.zoom(s.tag());
        }

        #[unsafe(method(selectTab:))]
        fn select_tab_action(&self, s: &NSMenuItem) {
            self.select_tab(s.tag() as usize);
        }

        #[unsafe(method(moveTab:))]
        fn move_tab_action(&self, s: &NSMenuItem) {
            self.move_tab(s.tag() as usize);
        }

        #[unsafe(method(foldAll:))]
        fn fold_all_action(&self, s: &NSMenuItem) {
            self.fold_all(s.tag() == 1);
        }

        #[unsafe(method(foldCurrent:))]
        fn fold_current_action(&self, s: &NSMenuItem) {
            self.fold_current(s.tag() == 1);
        }

        #[unsafe(method(foldLevel:))]
        fn fold_level_action(&self, s: &NSMenuItem) {
            self.fold_level(s.tag() as usize, false);
        }

        #[unsafe(method(unfoldLevel:))]
        fn unfold_level_action(&self, s: &NSMenuItem) {
            self.fold_level(s.tag() as usize, true);
        }

        #[unsafe(method(summary:))]
        fn summary_action(&self, _s: Option<&AnyObject>) {
            self.summary();
        }
    }

    impl App {
        #[unsafe(method(autoComplete:))]
        fn auto_complete_action(&self, s: &NSMenuItem) {
            self.autoc_cmd(s.tag());
        }
    }

    impl App {
        #[unsafe(method(searchCmd:))]
        fn search_cmd_action(&self, s: &NSMenuItem) {
            self.search_cmd(s.tag());
        }
    }

    impl App {
        #[unsafe(method(sciCommand:))]
        fn sci_command(&self, s: &NSMenuItem) {
            if let Some(v) = self.editor() {
                sci::send(&v, s.tag() as u32, 0, 0);
            }
        }

        #[unsafe(method(editOp:))]
        fn edit_op(&self, s: &NSMenuItem) {
            let Some(v) = self.editor() else { return };
            if let Err(i) = edit::run(&v, s.tag()) {
                let msg = format!("Unable to perform numeric sorting due to line {}.", i + 1);
                self.alert("Sorting Error", &msg, &["OK"]);
            }
        }

        #[unsafe(method(beginEndSelect:))]
        fn begin_end_select(&self, s: &NSMenuItem) {
            if let Some(v) = self.editor() {
                let b = &self.ivars().begin_select;
                b.set(edit::begin_end_select(&v, b.get(), s.tag() == 1));
            }
        }

        #[unsafe(method(insertDateTime:))]
        fn insert_date_time(&self, s: &NSMenuItem) {
            if let Some(v) = self.editor() {
                edit::insert_date_time(&v, s.tag() == 1);
            }
        }

        #[unsafe(method(copyPathInfo:))]
        fn copy_path_info(&self, s: &NSMenuItem) {
            let Some(t) = self.current().and_then(|i| self.tab(i)) else { return };
            let full = t.path.as_deref().map_or(t.name.clone(), |p| p.display().to_string());
            let dir = t.path.as_deref().and_then(Path::parent).map_or(String::new(), |p| p.display().to_string());
            tools::to_clipboard(match s.tag() {
                0 => &full,
                1 => &t.name,
                _ => &dir,
            });
        }

        #[unsafe(method(toggleReadOnly:))]
        fn toggle_read_only(&self, _s: Option<&AnyObject>) {
            let Some(i) = self.current() else { return };
            let ro = {
                let mut tabs = self.ivars().tabs.borrow_mut();
                let Some(t) = tabs.get_mut(i) else { return };
                t.ro = !t.ro;
                t.ro
            };
            if let Some(t) = self.tab(i) {
                sci::set_read_only(&t.view, ro || self.ivars().replacing.get());
            }
            self.doc_list_reload();
        }

        #[unsafe(method(cut:))]
        fn cut(&self, s: Option<&AnyObject>) {
            self.cut_or_copy(sel!(cut:), s);
        }

        #[unsafe(method(copy:))]
        fn copy(&self, s: Option<&AnyObject>) {
            self.cut_or_copy(sel!(copy:), s);
        }
    }

    impl App {
        #[unsafe(method(setLanguage:))]
        fn set_language_action(&self, s: &NSMenuItem) {
            self.set_language(s.tag());
        }

        #[unsafe(method(comment:))]
        fn comment_action(&self, s: &NSMenuItem) {
            self.comment(s.tag());
        }
    }

    impl App {
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _a: &NSApplication, urls: &objc2_foundation::NSArray<objc2_foundation::NSURL>) {
            if self.ivars().window.get().is_none() {
                self.build_window();
            }
            for url in urls {
                if !url.isFileURL() {
                    continue;
                }
                if let Some(path) = url.path() {
                    self.open_path(Path::new(&path.to_string()));
                }
            }
        }
    }

    impl App {
        #[unsafe(method(macroToggleRecord:))]
        fn macro_toggle_record_action(&self, _s: Option<&AnyObject>) {
            self.macro_toggle_record();
        }

        #[unsafe(method(macroPlayback:))]
        fn macro_playback_action(&self, _s: Option<&AnyObject>) {
            self.macro_playback();
        }

        #[unsafe(method(macroSave:))]
        fn macro_save_action(&self, _s: Option<&AnyObject>) {
            self.macro_save();
        }

        #[unsafe(method(macroShowMulti:))]
        fn macro_show_multi_action(&self, _s: Option<&AnyObject>) {
            self.macro_show_multi();
        }

        #[unsafe(method(macroRunMode:))]
        fn macro_run_mode_action(&self, _s: Option<&AnyObject>) {
            self.macro_run_mode();
        }

        #[unsafe(method(macroRunMulti:))]
        fn macro_run_multi_action(&self, _s: Option<&AnyObject>) {
            self.macro_run_multi();
        }

        #[unsafe(method(macroRunSaved:))]
        fn macro_run_saved_action(&self, s: &NSMenuItem) {
            self.macro_run_saved(s);
        }

        #[unsafe(method(macroLoadError:))]
        fn macro_load_error_action(&self, _s: Option<&AnyObject>) {
            self.macro_show_load_error();
        }

        #[unsafe(method(macroMenuWillSend:))]
        fn macro_menu_will_send_action(&self, _n: &NSNotification) {
            self.macro_menu_will_send();
        }

        #[unsafe(method(macroMenuDidSend:))]
        fn macro_menu_did_send_action(&self, n: &NSNotification) {
            self.macro_menu_did_send(n);
        }

        #[unsafe(method(runShow:))]
        fn run_show_action(&self, _s: Option<&AnyObject>) {
            self.run_show();
        }

        #[unsafe(method(runExecute:))]
        fn run_execute_action(&self, _s: Option<&AnyObject>) {
            self.run_execute();
        }

        #[unsafe(method(runSave:))]
        fn run_save_action(&self, _s: Option<&AnyObject>) {
            self.run_save();
        }

        #[unsafe(method(runBrowse:))]
        fn run_browse_action(&self, _s: Option<&AnyObject>) {
            self.run_browse();
        }

        #[unsafe(method(runVariables:))]
        fn run_variables_action(&self, _s: Option<&AnyObject>) {
            self.run_variables();
        }

        #[unsafe(method(runInsertVariable:))]
        fn run_insert_variable_action(&self, s: &NSMenuItem) {
            self.run_insert_variable(s);
        }

        #[unsafe(method(runUserCommand:))]
        fn run_user_command_action(&self, s: &NSMenuItem) {
            self.run_user_command(s);
        }
    }

    impl App {
        #[unsafe(method(openRecentFile:))]
        fn open_recent_file_action(&self, s: &NSMenuItem) {
            self.open_recent_index(s.tag());
        }

        #[unsafe(method(restoreRecentClosed:))]
        fn restore_recent_closed_action(&self, _s: Option<&AnyObject>) {
            self.restore_recent_closed();
        }

        #[unsafe(method(openAllRecent:))]
        fn open_all_recent_action(&self, _s: Option<&AnyObject>) {
            self.open_all_recent();
        }

        #[unsafe(method(emptyRecent:))]
        fn empty_recent_action(&self, _s: Option<&AnyObject>) {
            self.empty_recent();
        }

        #[unsafe(method(loadSession:))]
        fn load_session_action(&self, _s: Option<&AnyObject>) {
            self.load_session_file();
        }

        #[unsafe(method(saveSession:))]
        fn save_session_action(&self, _s: Option<&AnyObject>) {
            self.save_session_file();
        }
    }

    impl App {
        #[unsafe(method(styleConfigurator:))]
        fn style_configurator_action(&self, _s: Option<&AnyObject>) {
            self.open_style_configurator();
        }

        #[unsafe(method(importStyleThemes:))]
        fn import_style_themes_action(&self, _s: Option<&AnyObject>) {
            self.import_style_themes();
        }
    }

    impl App {
        #[unsafe(method(multiSelect:))]
        fn multi_select_action(&self, s: Option<&AnyObject>) {
            self.multi_select(s.map_or(0, |s| unsafe { msg_send![s, tag] }));
        }

        #[unsafe(method(columnModeTip:))]
        fn column_mode_tip_action(&self, _s: Option<&AnyObject>) {
            self.column_mode_tip();
        }

        #[unsafe(method(columnEditor:))]
        fn column_editor_action(&self, _s: Option<&AnyObject>) {
            self.show_column_editor();
        }

        #[unsafe(method(columnChoice:))]
        fn column_choice_action(&self, _s: Option<&AnyObject>) {
            self.column_choice();
        }

        #[unsafe(method(columnFormat:))]
        fn column_format_action(&self, _s: Option<&AnyObject>) {
            self.column_format();
        }

        #[unsafe(method(columnOk:))]
        fn column_ok_action(&self, _s: Option<&AnyObject>) {
            self.column_ok();
        }
    }

    impl App {
        #[unsafe(method(showMark:))]
        fn show_mark_action(&self, _s: Option<&AnyObject>) {
            self.show_mark();
        }

        #[unsafe(method(markAll:))]
        fn mark_all_action(&self, _s: Option<&AnyObject>) {
            self.mark_all();
        }

        #[unsafe(method(clearAllMarks:))]
        fn clear_all_marks_action(&self, _s: Option<&AnyObject>) {
            self.clear_all_marks();
        }

        #[unsafe(method(copyMarkedText:))]
        fn copy_marked_text(&self, _s: Option<&AnyObject>) {
            self.mark_cmd(mark::COPY_FIND_MARK);
        }

        #[unsafe(method(markModeChanged:))]
        fn mark_mode_changed_action(&self, _s: Option<&AnyObject>) {
            self.mark_mode_changed();
        }

        #[unsafe(method(markCmd:))]
        fn mark_cmd_action(&self, s: &NSMenuItem) {
            self.mark_cmd(s.tag());
        }

        #[unsafe(method(smartHighlight:))]
        fn smart_highlight_action(&self, _s: Option<&AnyObject>) {
            self.smart_highlight();
        }

        #[unsafe(method(sortTabs:))]
        fn sort_tabs_action(&self, s: &NSMenuItem) {
            self.sort_tabs(s.tag() as usize);
        }

        #[unsafe(method(selectWindow:))]
        fn select_window_action(&self, s: &NSMenuItem) {
            self.select_window(s.tag() as usize);
        }

        #[unsafe(method(showWindows:))]
        fn show_windows_action(&self, _s: Option<&AnyObject>) {
            self.show_windows();
        }

        #[unsafe(method(windowsActivate:))]
        fn windows_activate(&self, _s: Option<&AnyObject>) {
            self.windows_cmd("activate");
        }

        #[unsafe(method(windowsSave:))]
        fn windows_save(&self, _s: Option<&AnyObject>) {
            self.windows_cmd("save");
        }

        #[unsafe(method(windowsClose:))]
        fn windows_close(&self, _s: Option<&AnyObject>) {
            self.windows_cmd("close");
        }

        #[unsafe(method(windowsSortTabs:))]
        fn windows_sort_tabs(&self, _s: Option<&AnyObject>) {
            self.windows_cmd("sort");
        }

        #[unsafe(method(windowsOk:))]
        fn windows_ok(&self, _s: Option<&AnyObject>) {
            self.windows_cmd("ok");
        }

        // The Window menu has no key equivalents, so AppKit does not fill it on each key press.
        #[unsafe(method(menuHasKeyEquivalent:forEvent:target:action:))]
        fn menu_has_key_equivalent(
            &self,
            _m: &NSMenu,
            _e: &NSEvent,
            _t: *mut *mut AnyObject,
            _a: *mut c_void,
        ) -> bool {
            false
        }
    }

    unsafe impl NSMenuDelegate for App {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, m: &NSMenu) {
            self.update_window_menu(m);
        }
    }

    impl App {
        #[unsafe(method(copyBinary:))]
        fn copy_binary_action(&self, s: &NSMenuItem) {
            self.copy_binary(s.tag() == 1);
        }

        #[unsafe(method(pasteBinary:))]
        fn paste_binary_action(&self, _s: Option<&AnyObject>) {
            self.paste_binary();
        }
    }

    impl App {
        #[unsafe(method(backupTick:))]
        fn backup_tick_action(&self, _s: Option<&AnyObject>) {
            self.backup_tick();
        }
    }

    impl App {
        #[unsafe(method(showIncrementalSearch:))]
        fn show_incremental_search_action(&self, _s: Option<&AnyObject>) {
            self.show_incremental_search();
        }
    }

    impl App {
        #[unsafe(method(toggleDocList:))]
        fn toggle_doc_list(&self, _s: Option<&AnyObject>) {
            self.toggle_panel(docking::LEFT);
        }

        #[unsafe(method(toggleFunctionList:))]
        fn toggle_function_list(&self, _s: Option<&AnyObject>) {
            self.toggle_panel(docking::RIGHT);
        }

        #[unsafe(method(dockClose:))]
        fn dock_close(&self, s: &NSButton) {
            self.toggle_panel(s.tag());
        }

        #[unsafe(method(docListClick:))]
        fn doc_list_click_action(&self, _s: Option<&AnyObject>) {
            self.doc_list_click();
        }

        #[unsafe(method(functionListOpen:))]
        fn function_list_open_action(&self, _s: Option<&AnyObject>) {
            self.function_list_open();
        }

        #[unsafe(method(functionListSearch:))]
        fn function_list_search_action(&self, _s: Option<&AnyObject>) {
            self.function_list_search();
        }

        #[unsafe(method(functionListSort:))]
        fn function_list_sort_action(&self, _s: Option<&AnyObject>) {
            self.function_list_search();
        }

        #[unsafe(method(functionListReload:))]
        fn function_list_reload_action(&self, _s: Option<&AnyObject>) {
            self.function_list_search();
        }

        #[unsafe(method(tabViewDidChangeNumberOfTabViewItems:))]
        fn tab_count_changed(&self, _t: &NSTabView) {
            self.doc_list_reload();
        }

        #[unsafe(method(numberOfRowsInTableView:))]
        fn doc_list_count(&self, _t: &NSTableView) -> isize {
            self.doc_list_rows()
        }

        #[unsafe(method_id(tableView:objectValueForTableColumn:row:))]
        fn doc_list_object(&self, _t: &NSTableView, c: &NSTableColumn, row: isize) -> Option<Retained<AnyObject>> {
            self.doc_list_value(c, row)
        }

        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        fn fl_count(&self, _o: &NSOutlineView, item: Option<&AnyObject>) -> isize {
            self.function_list_count(item)
        }

        #[unsafe(method_id(outlineView:child:ofItem:))]
        fn fl_child(&self, _o: &NSOutlineView, n: isize, item: Option<&AnyObject>) -> Option<Retained<AnyObject>> {
            self.function_list_child(n, item)
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        fn fl_expandable(&self, _o: &NSOutlineView, item: &AnyObject) -> bool {
            self.function_list_expandable(Some(item))
        }

        #[unsafe(method_id(outlineView:objectValueForTableColumn:byItem:))]
        fn fl_object(&self, _o: &NSOutlineView, _c: Option<&NSTableColumn>, item: Option<&AnyObject>) -> Option<Retained<AnyObject>> {
            self.function_list_value(item)
        }
    }

    unsafe impl NSTableViewDataSource for App {}

    unsafe impl NSOutlineViewDataSource for App {}

    impl App {
        #[unsafe(method(showPreferences:))]
        fn show_preferences_action(&self, _s: Option<&AnyObject>) {
            self.show_preferences();
        }

        #[unsafe(method(prefChanged:))]
        fn pref_changed_action(&self, s: &NSControl) {
            self.pref_changed(s);
        }

        #[unsafe(method(prefBrowse:))]
        fn pref_browse_action(&self, _s: Option<&AnyObject>) {
            self.pref_browse();
        }
    }

    unsafe impl NSObjectProtocol for App {}

    unsafe impl NSApplicationDelegate for App {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            if self.ivars().window.get().is_none() {
                self.build_window();
            }
            self.start_session();
            self.style_load_alert();
            for a in std::env::args_os()
                .skip(1)
                .filter(|a| !a.to_string_lossy().starts_with('-'))
            {
                self.open_path(&std::path::absolute(&a).unwrap_or(a.into()));
            }
            if self.ivars().tabs.borrow().is_empty() {
                self.add_tab(None, Enc::Utf8, b"", false);
            }
            NSApplication::sharedApplication(self.mtm()).activate();
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _a: &NSApplication) -> NSApplicationTerminateReply {
            if self.ivars().replacing.get() {
                self.alert("Replace in Files is still running.", "Quit again when it is done.", &["OK"]);
                return NSApplicationTerminateReply::TerminateCancel;
            }
            let Some(session) = self.quit_session() else {
                return NSApplicationTerminateReply::TerminateCancel;
            };
            self.udl_flush();
            self.save_on_quit(&session);
            NSApplicationTerminateReply::TerminateNow
        }
    }

    impl App {
        #[unsafe(method(setUdl:))]
        fn set_udl_action(&self, s: &NSMenuItem) {
            self.set_udl(s.tag());
        }

        #[unsafe(method(userDefined:))]
        fn user_defined_action(&self, _s: Option<&AnyObject>) {
            self.user_defined();
        }

        #[unsafe(method(defineUdl:))]
        fn define_udl_action(&self, _s: Option<&AnyObject>) {
            self.define_udl();
        }

        #[unsafe(method(openUdlFolder:))]
        fn open_udl_folder_action(&self, _s: Option<&AnyObject>) {
            self.open_udl_folder();
        }

        #[unsafe(method(udlCollection:))]
        fn udl_collection_action(&self, _s: Option<&AnyObject>) {
            self.udl_collection();
        }

        #[unsafe(method(udlLang:))]
        fn udl_lang_action(&self, _s: Option<&AnyObject>) {
            self.udl_lang();
        }

        #[unsafe(method(udlCommand:))]
        fn udl_command_action(&self, s: &NSButton) {
            self.udl_command(s.tag());
        }

        #[unsafe(method(udlEdit:))]
        fn udl_edit_action(&self, s: &objc2_app_kit::NSControl) {
            self.udl_edit(s.tag());
        }

        #[unsafe(method(udlSave:))]
        fn udl_save_action(&self, _s: Option<&AnyObject>) {
            self.udl_flush();
        }

        #[unsafe(method(udlStyler:))]
        fn udl_styler_action(&self, s: &NSButton) {
            self.udl_styler(s.tag());
        }
    }

    unsafe impl NSWindowDelegate for App {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _w: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
            false
        }
    }

    unsafe impl NSTabViewDelegate for App {
        #[unsafe(method(tabView:didSelectTabViewItem:))]
        fn did_select(&self, _t: &NSTabView, _i: Option<&NSTabViewItem>) {
            self.focus();
            self.update_status();
            self.doc_list_reload();
            self.function_list_reload();
        }
    }
);

impl App {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn tab_view(&self) -> &NSTabView {
        self.ivars().tab_view.get().unwrap()
    }

    fn build_window(&self) {
        let mtm = self.mtm();
        let rect = NSRect::new(NSPoint::new(0., 0.), NSSize::new(1000., 700.));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let w = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { w.setReleasedWhenClosed(false) };
        w.setTitle(&NSString::from_str("Notepad++"));
        w.setDelegate(Some(ProtocolObject::from_ref(self)));
        let tv = NSTabView::new(mtm);
        tv.setDelegate(Some(ProtocolObject::from_ref(self)));
        let results = sci::new_view();
        sci::set_delegate(&results, self);
        let markings = sci::setup_results(&results, cfg());
        finder::simple_fold_markers(&results);
        let split = NSSplitView::new(mtm);
        split.setVertical(false);
        split.setDividerStyle(NSSplitViewDividerStyle::Thin);
        split.addSubview(&tv);
        split.addSubview(&results);
        let content = NSView::new(mtm);
        w.setContentView(Some(&content));
        let size = content.bounds().size;
        split.setFrame(NSRect::new(
            NSPoint::new(0., STATUS_H),
            NSSize::new(size.width, size.height - STATUS_H),
        ));
        split.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview(&self.dock(&split));
        let mut right = size.width;
        let mut status = vec![];
        for wd in STATUS_WIDTHS.iter().rev() {
            let l = NSTextField::labelWithString(&NSString::new(), mtm);
            let (x, wd) = if *wd == 0. {
                (6., right - 6.)
            } else {
                (right - wd, *wd)
            };
            l.setFrame(NSRect::new(
                NSPoint::new(x, 3.),
                NSSize::new(wd - 6., STATUS_H - 6.),
            ));
            l.setAutoresizingMask(if x == 6. {
                NSAutoresizingMaskOptions::ViewWidthSizable
            } else {
                NSAutoresizingMaskOptions::ViewMinXMargin
            });
            content.addSubview(&l);
            status.insert(0, l);
            right = x;
        }
        let _ = self.ivars().status.set(status);
        split.setPosition_ofDividerAtIndex(split.frame().size.height, 0);
        w.center();
        w.makeKeyAndOrderFront(None);
        let _ = self.ivars().window.set(w);
        let _ = self.ivars().tab_view.set(tv);
        let _ = self.ivars().split.set(split);
        let _ = self.ivars().results.set((results, markings));
    }

    fn editor(&self) -> Option<Retained<NSView>> {
        self.current().and_then(|i| self.tab(i)).map(|t| t.view)
    }

    fn selected_line(&self) -> Option<String> {
        self.find_fill_text(&*self.editor()?)
    }

    fn show_panel(&self, form: &Form, find: &NSTextField, focus: &NSTextField) {
        if let Some(s) = self.selected_line() {
            find.setStringValue(&ns(&s));
        }
        form.panel.makeKeyAndOrderFront(None);
        form.panel.makeFirstResponder(Some(focus));
        unsafe { focus.selectText(None) };
    }

    fn find_ui(&self) -> &FindUi {
        self.ivars().find_ui.get_or_init(|| {
            let t: &AnyObject = self;
            let f = Form::new(self.mtm(), "Find", 560., 310.);
            let (c, _) = panel::controls(&f, t, 0., &[]);
            let (x, w) = (436., 110.);
            let next = f.button("Find Next", x, 14., w, t, sel!(findNext:));
            next.setKeyEquivalent(&ns("\r"));
            let prev = f.button("Find Previous", x, 46., w, t, sel!(findPrevious:));
            f.button("Count", x, 78., w, t, sel!(count:));
            f.button("Replace", x, 110., w, t, sel!(replace:));
            f.button("Replace All", x, 142., w, t, sel!(replaceAll:));
            f.button("Close", x, 174., w, t, sel!(closePanel:))
                .setKeyEquivalent(&ns("\u{1b}"));
            FindUi { form: f, c, prev }
        })
    }

    fn fif_ui(&self) -> &FifUi {
        self.ivars().fif_ui.get_or_init(|| {
            let t: &AnyObject = self;
            let f = Form::new(self.mtm(), "Find in Files", 600., 390.);
            let (c, b) = panel::controls(&f, t, 60., &["In all sub-folders", "In hidden folders"]);
            f.label("Filters:", 16., 76., 100.);
            let filters = f.field(120., 76., 300.);
            filters.setStringValue(&ns("*.*"));
            f.label("Directory:", 16., 106., 100.);
            let dir = f.field(120., 106., 300.);
            f.button("...", 424., 103., 44., t, sel!(browseDir:));
            panel::set_on(&b[0], true);
            let (x, w) = (476., 110.);
            f.button("Find All", x, 14., w, t, sel!(findAll:))
                .setKeyEquivalent(&ns("\r"));
            f.button("Replace in Files", x, 46., w, t, sel!(replaceInFiles:));
            f.button("Close", x, 78., w, t, sel!(closePanel:))
                .setKeyEquivalent(&ns("\u{1b}"));
            let [sub, hidden] = [b[0].clone(), b[1].clone()];
            FifUi {
                form: f,
                c,
                filters,
                dir,
                sub,
                hidden,
            }
        })
    }

    fn open_find(&self, replace: bool) {
        let u = self.find_ui();
        self.show_panel(
            &u.form,
            &u.c.find,
            if replace { &u.c.replace } else { &u.c.find },
        );
    }

    fn find(&self, up: bool) {
        self.find_with(&self.find_ui().c.opts(), up);
    }

    fn find_with(&self, o: &search::Opts, up: bool) {
        let c = &self.find_ui().c;
        let Some(v) = self.editor() else { return };
        if up && o.regex() {
            return;
        }
        let doc = sci::doc(&v);
        c.set_status(
            &match search::find_next(&doc, o, sci::selection(&v), up, Next::Find) {
                Ok(Some((m, w))) => {
                    sci::select(&v, m);
                    match w {
                        Wrap::End => search::END_REACHED,
                        Wrap::Top => search::TOP_REACHED,
                        Wrap::No => "",
                    }
                    .to_string()
                }
                Ok(None) if o.find.is_empty() => String::new(),
                Ok(None) => search::not_found_status(o),
                Err(e) => e,
            },
        );
    }

    fn replace_once(&self, v: &NSView, o: &search::Opts) -> Result<String, String> {
        let (msg, sel) = search::replace_once(&sci::doc(v), o, sci::selection(v))?;
        sci::select(v, sel);
        Ok(msg)
    }

    fn start_fif(&self, replace: bool) {
        let u = self.fif_ui();
        let opts = u.c.opts();
        let dir = panel::text(&u.dir).trim().to_string();
        let filters = panel::text(&u.filters);
        if self.ivars().fif_running.get() || opts.find.is_empty() || dir.is_empty() {
            return;
        }
        if replace {
            let f = if filters.trim().is_empty() {
                "*.*"
            } else {
                filters.as_str()
            };
            let msg = format!("Are you sure you want to replace all occurrences in:\n\n{dir}\n\nFor file type:\n\n{f}");
            let a = NSAlert::new(self.mtm());
            a.setMessageText(&ns("Are you sure?"));
            a.setInformativeText(&ns(&msg));
            a.addButtonWithTitle(&ns("OK")).setKeyEquivalent(&ns(""));
            a.addButtonWithTitle(&ns("Cancel"))
                .setKeyEquivalent(&ns("\r"));
            a.setAccessoryView(Some(&EscapeCancels::new(self.mtm())));
            if a.runModal() != NSAlertFirstButtonReturn {
                return;
            }
        }
        let args = FifArgs {
            dir: PathBuf::from(dir),
            filters,
            sub: panel::on(&u.sub),
            hidden: panel::on(&u.hidden),
            opts,
            replace,
            open: self.open_texts(),
        };
        u.c.set_status(if replace {
            "Replace In Files progress..."
        } else {
            "Find In Files progress..."
        });
        self.ivars().fif_running.set(true);
        if replace {
            self.ivars().replacing.set(true);
            self.set_tabs_read_only(true);
        }
        let app = self as *const Self as usize;
        search::spawn_find_in_files(args, move |args, r| {
            *FIF_DONE.lock().unwrap() = Some((args, r));
            let app = unsafe { &*(app as *const AnyObject) };
            let _: () = unsafe {
                msg_send![app, performSelectorOnMainThread: sel!(fifDone:), withObject: None::<&AnyObject>, waitUntilDone: false]
            };
        });
    }

    fn set_tabs_read_only(&self, on: bool) {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        tabs.iter().for_each(|t| sci::set_read_only(&t.view, on || t.ro));
    }

    // Reloads open tabs of changed files and returns the paths of modified tabs it did not reload.
    fn reload_changed(&self, changed: &[PathBuf]) -> Vec<PathBuf> {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        let mut kept = vec![];
        for t in tabs {
            let Some(p) = &t.path else { continue };
            if !changed.contains(&search::canonical(p)) {
                continue;
            }
            if self.dirty(&t) {
                kept.push(p.clone());
            } else if let Ok(b) = std::fs::read(p) {
                let i = self
                    .ivars()
                    .tabs
                    .borrow()
                    .iter()
                    .position(|x| std::ptr::eq(&*x.item, &*t.item));
                if let Some(i) = i {
                    self.load_into(i, &b, t.enc);
                }
            }
        }
        kept
    }

    fn show_results(&self, lines: Vec<Line>) {
        let (v, m) = self.ivars().results.get().unwrap();
        let mut text = vec![];
        for l in &lines {
            text.extend_from_slice(&l.text);
            text.extend_from_slice(b"\r\n");
        }
        let mut all: Vec<Line> = lines
            .into_iter()
            .map(|l| Line { text: vec![], ..l })
            .collect();
        all.extend(self.ivars().result_lines.take());
        sci::prepend_results(v, *m, &all, &text);
        *self.ivars().result_lines.borrow_mut() = all;
        self.collapse_old_searches();
        self.reveal_results();
    }

    fn reveal_results(&self) {
        let v = &self.ivars().results.get().unwrap().0;
        let split = self.ivars().split.get().unwrap();
        if v.frame().size.height < 40. {
            split.setPosition_ofDividerAtIndex(split.frame().size.height - 220., 0);
        }
    }

    fn tab(&self, i: usize) -> Option<Tab> {
        self.ivars().tabs.borrow().get(i).cloned()
    }

    fn current(&self) -> Option<usize> {
        let item = self.tab_view().selectedTabViewItem()?;
        let tabs = self.ivars().tabs.borrow();
        tabs.iter().position(|t| std::ptr::eq(&*t.item, &*item))
    }

    fn focus(&self) {
        let tab = self.current().and_then(|i| self.tab(i));
        if let (Some(t), Some(w)) = (tab, self.ivars().window.get()) {
            w.makeFirstResponder(Some(&sci::content(&t.view)));
        }
    }

    fn open_path(&self, path: &Path) {
        self.recent_remove(path);
        if let Some(t) = self.find_open(path, None).and_then(|i| self.tab(i)) {
            self.tab_view().selectTabViewItem(Some(&t.item));
            return;
        }
        match std::fs::read(path) {
            Ok(b) => {
                let (enc, text, lost) = encoding::load(&b);
                self.add_tab(Some(path.to_path_buf()), prefs::opened_encoding(enc, &b), &text, lost)
            }
            Err(e) => {
                self.alert(
                    &format!("Cannot open {}", path.display()),
                    &e.to_string(),
                    &["OK"],
                );
            }
        }
    }

    fn add_tab(&self, path: Option<PathBuf>, enc: Enc, text: &[u8], lost: bool) {
        let p = prefs::get();
        let enc = if path.is_none() && text.is_empty() { p.new_doc_enc() } else { enc };
        let view = sci::new_view();
        sci::set_delegate(&view, self);
        sci::set_bytes(&view, text);
        sci::set_eol_mode(
            &view,
            encoding::detect_eol(text).unwrap_or(p.new_doc_eol()),
        );
        sci::set_read_only(&view, self.ivars().replacing.get());
        self.macro_arm(&view);
        let lang = match path.as_deref() {
            Some(p) => lang::language_for_path(cfg(), p),
            None => prefs::new_doc_language(),
        };
        sci::apply_language(&view, cfg(), lang);
        self.apply_view(&view, lang.map_or("normal", |l| l.name.as_str()), cfg());
        sci::setup_bookmark_margin(&view, cfg());
        sci::setup_change_history(&view, cfg());
        sci::setup_multi_selection(&view);
        mark::setup_indicators(&view, cfg());
        let name = match &path {
            Some(p) => p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            None => {
                self.ivars().untitled.set(self.ivars().untitled.get() + 1);
                format!("new {}", self.ivars().untitled.get())
            }
        };
        backup::file_loaded(&view, path.as_deref());
        let item = NSTabViewItem::new();
        item.setView(Some(&view));
        self.ivars().tabs.borrow_mut().push(Tab {
            view,
            item: item.clone(),
            path,
            name,
            enc,
            enc_dirty: false,
            lost,
            ro: false,
            lang: None,
        });
        let last = self.ivars().tabs.borrow().len() - 1;
        self.apply_udl_at(last);
        self.refresh_title(last);
        self.tab_view().addTabViewItem(&item);
        self.tab_view().selectTabViewItem(Some(&item));
        self.update_status();
    }

    // Checkmarks for the current encoding and EOL; format commands are off while Replace in Files runs.
    fn validate(&self, item: &NSMenuItem) -> bool {
        if let Some(r) = self.validate_edit(item) {
            return r;
        }
        if let Some(r) = self.validate_window(item) {
            return r;
        }
        if let Some(r) = self.validate_autoc(item) {
            return r;
        }
        let Some(action) = item.action() else {
            return true;
        };
        if let Some(on) = self.validate_file(action) {
            return on;
        }
        if self.validate_view(item) {
            return true;
        }
        if let Some(on) = self.validate_udl(item) {
            return on;
        }
        if let Some(on) = self.validate_language(item) {
            return on;
        }
        let tab = self.current().and_then(|i| self.tab(i));
        let checked = match &tab {
            Some(t) if action == sel!(encodeIn:) => enc_tag(t.enc) == item.tag(),
            Some(t) if action == sel!(eolConvert:) => sci::eol_mode(&t.view) as isize == item.tag(),
            _ => false,
        };
        item.setState(if checked {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        let format = [sel!(encodeIn:), sel!(convertTo:), sel!(eolConvert:)].contains(&action);
        !(format && (tab.as_ref().is_none_or(|t| t.ro) || self.ivars().replacing.get()))
            && !matches!(tag_enc(item.tag()), Enc::Cp(cp) if action == sel!(encodeIn:) && !encoding::supported(cp))
    }

    fn dirty(&self, t: &Tab) -> bool {
        t.enc_dirty || sci::is_modified(&t.view)
    }

    fn set_enc(&self, i: usize, e: Enc, dirty: bool) {
        if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
            t.enc = e;
            t.enc_dirty = dirty;
        }
        self.refresh_title(i);
        self.update_status();
    }

    fn load_into(&self, i: usize, b: &[u8], e: Enc) {
        let Some(t) = self.tab(i) else { return };
        let (text, lost) = encoding::decode(b, e);
        sci::reload(&t.view, &text);
        backup::file_loaded(&t.view, t.path.as_deref());
        if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
            t.lost = lost;
        }
        sci::set_eol_mode(
            &t.view,
            encoding::detect_eol(&text).unwrap_or(prefs::get().new_doc_eol()),
        );
        self.set_enc(i, e, false);
    }

    fn update_status(&self) {
        let Some(t) = self.current().and_then(|i| self.tab(i)) else {
            return;
        };
        let Some(labels) = self.ivars().status.get() else {
            return;
        };
        let v = &t.view;
        prefs::line_number_width(v);
        let lang = language::tab_language(&t);
        let (ln, col, pos, sel) = sci::position_info(v);
        let c = |n: isize| search::commafy(n as usize);
        let sel = match sel {
            Some((chars, lines)) => format!("Sel: {} | {}", c(chars), c(lines)),
            None => format!("Pos: {}", c(pos)),
        };
        let texts = [
            self.udl_status(&t)
                .unwrap_or_else(|| lang::long_name(lang.map_or("normal", |l| l.name.as_str()))),
            format!(
                "length: {}    lines: {}",
                c(sci::length(v)),
                c(sci::line_info(v).1)
            ),
            format!("Ln: {}    Col: {}    {sel}", c(ln), c(col)),
            encoding::eol_name(sci::eol_mode(v)).to_string(),
            encoding::name(t.enc),
            if sci::overtype(v) { "OVR" } else { "INS" }.to_string(),
        ];
        for (l, s) in labels.iter().zip(texts) {
            l.setStringValue(&ns(&s));
        }
    }

    fn refresh_title(&self, i: usize) {
        let Some(t) = self.tab(i) else { return };
        let mark = if self.dirty(&t) { "*" } else { "" };
        t.item
            .setLabel(&NSString::from_str(&format!("{mark}{}", t.name)));
        self.doc_list_reload();
    }

    fn save(&self, i: usize, ask: bool) -> bool {
        if self.ivars().replacing.get() {
            self.alert(
                "Replace in Files is still running.",
                "Save again when it is done.",
                &["OK"],
            );
            return false;
        }
        let Some(tab) = self.tab(i) else { return false };
        let path = match tab.path.clone().filter(|_| !ask) {
            Some(p) => p,
            None => match self.ask_save_path(i, "Save As") {
                Some(p) => p,
                None => return false,
            },
        };
        if !ask && tab.path.is_some() && !self.backup_on_save(&path) {
            return false;
        }
        let Some(enc) = self.write_tab(&tab, &path) else {
            return false;
        };
        self.drop_backup(&tab.view);
        backup::file_loaded(&tab.view, Some(&path));
        let renamed = tab.path.as_deref() != Some(&path);
        if renamed {
            self.recent_saved_as(tab.path.as_deref(), &path);
        }
        if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
            t.name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            t.path = Some(path.clone());
            t.enc = enc;
            t.enc_dirty = false;
            t.lost = false;
        }
        sci::set_save_point(&tab.view);
        if renamed {
            self.apply_tab_language(i);
        }
        self.refresh_title(i);
        self.update_status();
        self.function_list_reload();
        true
    }

    fn write_tab(&self, tab: &Tab, path: &Path) -> Option<Enc> {
        let text = sci::bytes(&tab.view);
        let mut enc = tab.enc;
        let bytes = match encoding::encode(&text, enc, false) {
            Ok(b) if !tab.lost => b,
            r => {
                let name = encoding::name(enc);
                let msg = match r {
                    Err(n) => format!("{n} characters cannot be saved in {name}."),
                    Ok(_) => format!(
                        "Some bytes of this file could not be read in {name}. Saving changes them."
                    ),
                };
                let r = self.alert(
                    &msg,
                    "",
                    &["Save as UTF-8 instead", "Save anyway", "Cancel"],
                );
                if r == NSAlertFirstButtonReturn {
                    enc = Enc::Utf8;
                } else if r != NSAlertSecondButtonReturn {
                    return None;
                }
                encoding::encode(&text, enc, true).unwrap_or_default()
            }
        };
        if let Err(e) = std::fs::write(path, bytes) {
            self.alert(
                &format!("Cannot save {}", path.display()),
                &e.to_string(),
                &["OK"],
            );
            return None;
        }
        Some(enc)
    }

    fn confirm_close(&self, i: usize) -> bool {
        let Some(tab) = self.tab(i) else { return true };
        if !self.dirty(&tab) {
            return true;
        }
        let r = self.alert(
            &format!("Save file \"{}\"?", tab.name),
            "",
            &["Save", "Don't Save", "Cancel"],
        );
        if r == NSAlertFirstButtonReturn {
            self.save(i, false)
        } else {
            r == NSAlertSecondButtonReturn
        }
    }

    fn alert(&self, msg: &str, info: &str, buttons: &[&str]) -> isize {
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&NSString::from_str(msg));
        a.setInformativeText(&NSString::from_str(info));
        for b in buttons {
            a.addButtonWithTitle(&NSString::from_str(b));
        }
        a.runModal()
    }
}

fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    key: &str,
    target: Option<&AnyObject>,
) -> Retained<NSMenuItem> {
    let i = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    unsafe { i.setTarget(target) };
    i
}

fn submenu(mtm: MainThreadMarker, bar: &NSMenu, title: &str, items: Vec<Retained<NSMenuItem>>) {
    bar.addItem(&nested(mtm, title, items));
}

fn nested(
    mtm: MainThreadMarker,
    title: &str,
    items: Vec<Retained<NSMenuItem>>,
) -> Retained<NSMenuItem> {
    let m = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    items.iter().for_each(|i| m.addItem(i));
    let top = NSMenuItem::new(mtm);
    top.setTitle(&NSString::from_str(title));
    top.setSubmenu(Some(&m));
    top
}

fn tagged(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    tag: isize,
    t: Option<&AnyObject>,
) -> Retained<NSMenuItem> {
    let i = item(mtm, title, action, "", t);
    i.setTag(tag);
    i
}

// NppCommands.cpp IDM_FORMAT_ANSI..IDM_FORMAT_AS_UTF_8: a Unicode mode change only sets the save mode.
fn should_be_dirty(from: Enc, to: Enc) -> bool {
    match to {
        Enc::Utf8 => from != Enc::Ansi,
        Enc::Ansi => from != Enc::Utf8,
        _ => true,
    }
}

const UNICODE: [(&str, Enc); 5] = [
    ("ANSI", Enc::Ansi),
    ("UTF-8", Enc::Utf8),
    ("UTF-8-BOM", Enc::Utf8Bom),
    ("UTF-16 BE BOM", Enc::Utf16Be),
    ("UTF-16 LE BOM", Enc::Utf16Le),
];

// Menu tag of an encoding: 1 to 5 for the Unicode items, the code page for a character set.
fn enc_tag(e: Enc) -> isize {
    match e {
        Enc::Cp(cp) => cp as isize,
        e => UNICODE
            .iter()
            .position(|(_, u)| *u == e)
            .map_or(-1, |p| p as isize + 1),
    }
}

fn tag_enc(t: isize) -> Enc {
    match t {
        1..=5 => UNICODE[t as usize - 1].1,
        cp => Enc::Cp(cp as u32),
    }
}

fn encoding_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let mut v: Vec<_> = UNICODE
        .iter()
        .map(|(n, e)| tagged(mtm, n, sel!(encodeIn:), enc_tag(*e), t))
        .collect();
    let groups = encoding::CHARSETS.iter().map(|(g, list)| {
        nested(
            mtm,
            g,
            list.iter()
                .map(|(n, cp)| tagged(mtm, n, sel!(encodeIn:), *cp as isize, t))
                .collect(),
        )
    });
    v.push(nested(mtm, "Character sets", groups.collect()));
    v.push(NSMenuItem::separatorItem(mtm));
    v.extend(UNICODE.iter().map(|(n, e)| {
        tagged(
            mtm,
            &format!("Convert to {n}"),
            sel!(convertTo:),
            enc_tag(*e),
            t,
        )
    }));
    v
}

fn main() {
    let mtm = MainThreadMarker::new().unwrap();
    backup::install_panic_hook();
    search::warm_up();
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let d = App::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*d)));
    let t: Option<&AnyObject> = Some(&d);
    let bar = NSMenu::new(mtm);
    submenu(
        mtm,
        &bar,
        "Notepad++",
        vec![item(mtm, "Quit Notepad++", sel!(terminate:), "q", None)],
    );
    submenu(mtm, &bar, "File", fileops::file_menu(mtm, t));
    submenu(mtm, &bar, "Edit", edit::edit_menu(mtm, t));
    submenu(mtm, &bar, "Search", search_extras::search_menu(mtm, t));
    submenu(mtm, &bar, "View", view::view_menu(mtm, t));
    submenu(mtm, &bar, "Encoding", encoding_menu(mtm, t));
    submenu(mtm, &bar, "Language", language::language_menu(mtm, t));
    submenu(mtm, &bar, "Settings", prefs::settings_menu(mtm, t));
    submenu(mtm, &bar, "Tools", tools::tools_menu(mtm, t));
    macros::menus(mtm, &bar, t);
    bar.addItem(&window::window_menu(mtm, &d));
    submenu(mtm, &bar, "?", tools::help_menu(mtm, t));
    if let Some(m) = bar.itemAtIndex(0).and_then(|i| i.submenu()) {
        m.insertItem_atIndex(&NSMenuItem::separatorItem(mtm), 0);
        m.insertItem_atIndex(&item(mtm, "About Notepad++", sel!(showAbout:), "", t), 0);
    }
    app.setMainMenu(Some(&bar));
    app.run();
}
