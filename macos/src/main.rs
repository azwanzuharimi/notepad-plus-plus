// SPDX-License-Identifier: GPL-3.0-or-later
mod config;
mod lang;
mod panel;
mod sci;
mod search;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSApplicationTerminateReply,
    NSBackingStoreType, NSButton, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem,
    NSModalResponseOK, NSOpenPanel, NSSavePanel, NSSplitView, NSSplitViewDividerStyle, NSTabView,
    NSTabViewDelegate, NSTabViewItem, NSTextField, NSView, NSWindow, NSWindowDelegate,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use panel::{Controls, Form};
use search::{FifArgs, FifOut, Line, Next, Wrap};
use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static FIF_DONE: Mutex<Option<(bool, Result<FifOut, String>)>> = Mutex::new(None);

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
    static C: OnceLock<config::Config> = OnceLock::new();
    C.get_or_init(config::load)
}

#[derive(Clone)]
struct Tab {
    view: Retained<NSView>,
    item: Retained<NSTabViewItem>,
    path: Option<PathBuf>,
    name: String,
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
    find_ui: OnceCell<FindUi>,
    fif_ui: OnceCell<FifUi>,
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
            self.add_tab(None, b"");
        }

        #[unsafe(method(openDocument:))]
        fn open_document(&self, _s: Option<&AnyObject>) {
            let p = NSOpenPanel::openPanel(self.mtm());
            p.setAllowsMultipleSelection(true);
            if p.runModal() == NSModalResponseOK {
                for url in p.URLs() {
                    if let Some(path) = url.path() {
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
            let Some(tab) = self.tab(i) else { return };
            self.ivars()
                .tabs
                .borrow_mut()
                .retain(|t| !std::ptr::eq(&*t.item, &*tab.item));
            self.tab_view().removeTabViewItem(&tab.item);
            if self.ivars().tabs.borrow().is_empty() {
                self.ivars().untitled.set(0);
                self.add_tab(None, b"");
            }
            self.focus();
        }

        #[unsafe(method(notification:))]
        fn notification(&self, scn: *const c_void) {
            let h = unsafe { &*(scn as *const NotifyHeader) };
            if h.code == sci::SCN_SAVEPOINTREACHED || h.code == sci::SCN_SAVEPOINTLEFT {
                let n = self.ivars().tabs.borrow().len();
                (0..n).for_each(|i| self.refresh_title(i));
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
            let hit = self.ivars().result_lines.borrow().get(line as usize).and_then(|l| Some((l.hit.clone()?, l.marks.clone())));
            let Some(((path, ranges), marks)) = hit else { return };
            let k = marks.iter().position(|&(s, e)| s <= at && at <= e).unwrap_or(0);
            self.open_path(&path);
            let tab = self.current().and_then(|i| self.tab(i));
            if let Some(t) = tab.filter(|t| t.path.as_deref() == Some(&path)) {
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
                Err(e) => search::regex_error_status(&e),
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
                Err(e) => search::regex_error_status(&e),
            });
        }

        #[unsafe(method(replace:))]
        fn replace(&self, _s: Option<&AnyObject>) {
            let (c, o) = (&self.find_ui().c, self.find_ui().c.opts());
            let Some(v) = self.editor() else { return };
            if o.find.is_empty() {
                return;
            }
            c.set_status(&self.replace_once(&v, &o).unwrap_or_else(|e| search::regex_error_status(&e)));
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
            let Some((replace, r)) = FIF_DONE.lock().unwrap().take() else { return };
            self.ivars().fif_running.set(false);
            let c = &self.fif_ui().c;
            match r {
                Err(e) => c.set_status(&e),
                Ok(out) if replace => {
                    c.set_status(&search::replace_in_files_status(out.count, &out.skipped));
                    self.reload_changed(&out.changed);
                }
                Ok(out) => {
                    c.set_status("");
                    self.show_results(out.lines);
                }
            }
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

    unsafe impl NSObjectProtocol for App {}

    unsafe impl NSApplicationDelegate for App {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            self.build_window();
            for a in std::env::args_os()
                .skip(1)
                .filter(|a| !a.to_string_lossy().starts_with('-'))
            {
                self.open_path(&std::path::absolute(&a).unwrap_or(a.into()));
            }
            if self.ivars().tabs.borrow().is_empty() {
                self.add_tab(None, b"");
            }
            NSApplication::sharedApplication(self.mtm()).activate();
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _a: &NSApplication) -> NSApplicationTerminateReply {
            if self.ivars().fif_running.get() {
                self.alert("Find in Files is still running.", "Quit again when it is done.", &["OK"]);
                return NSApplicationTerminateReply::TerminateCancel;
            }
            let n = self.ivars().tabs.borrow().len();
            for i in 0..n {
                self.tab_view().selectTabViewItemAtIndex(i as isize);
                if !self.confirm_close(i) {
                    return NSApplicationTerminateReply::TerminateCancel;
                }
            }
            NSApplicationTerminateReply::TerminateNow
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
        let split = NSSplitView::new(mtm);
        split.setVertical(false);
        split.setDividerStyle(NSSplitViewDividerStyle::Thin);
        split.addSubview(&tv);
        split.addSubview(&results);
        w.setContentView(Some(&split));
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
        let v = self.editor()?;
        let (s, e) = sci::selection(&v);
        let b = sci::doc(&v).range(s, e);
        (s < e && b.len() <= 1024 && !b.contains(&b'\n') && !b.contains(&b'\r'))
            .then(|| String::from_utf8_lossy(&b).into_owned())
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
        let c = &self.find_ui().c;
        let o = c.opts();
        let Some(v) = self.editor() else { return };
        if up && o.regex() {
            return;
        }
        let doc = sci::doc(&v);
        c.set_status(
            &match search::find_next(&doc, &o, sci::selection(&v), up, Next::Find) {
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
                Ok(None) => search::not_found_status(&o),
                Err(e) => search::regex_error_status(&e),
            },
        );
    }

    // Port of FindReplaceDlg::processReplace.
    fn replace_once(&self, v: &NSView, o: &search::Opts) -> Result<String, String> {
        let doc = sci::doc(v);
        let cur = sci::selection(v);
        let Some((m, _)) = search::find_next(&doc, o, cur, false, Next::ForReplace)? else {
            return Ok(search::replace_not_found_status(o));
        };
        if m != cur {
            sci::select(v, m);
            return Ok(String::new());
        }
        let p = m.0 + doc.replace(m.0, m.1 - m.0, &o.replace_bytes(), o.regex());
        sci::select(v, (p, p));
        Ok(
            match search::find_next(&doc, o, (p, p), false, Next::AfterReplace)? {
                Some((n, w)) => {
                    sci::select(v, n);
                    match w {
                        Wrap::End => search::REPLACE_END_REACHED,
                        Wrap::Top => search::REPLACE_TOP_REACHED,
                        Wrap::No => {
                            "Replace: 1 occurrence was replaced. The next occurrence found."
                        }
                    }
                }
                None => "Replace: 1 occurrence was replaced. No more occurrences were found.",
            }
            .to_string(),
        )
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
            skip: self.unsaved_paths(),
        };
        u.c.set_status(if replace {
            "Replace In Files progress..."
        } else {
            "Find In Files progress..."
        });
        self.ivars().fif_running.set(true);
        let app = self as *const Self as usize;
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                search::find_in_files(&args)
            }))
            .unwrap_or_else(|_| Err("Find in Files stopped because of an internal error.".into()));
            *FIF_DONE.lock().unwrap() = Some((args.replace, r));
            let app = unsafe { &*(app as *const AnyObject) };
            let _: () = unsafe {
                msg_send![app, performSelectorOnMainThread: sel!(fifDone:), withObject: None::<&AnyObject>, waitUntilDone: false]
            };
        });
    }

    fn unsaved_paths(&self) -> Vec<PathBuf> {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        tabs.iter()
            .filter(|t| sci::is_modified(&t.view))
            .filter_map(|t| t.path.as_deref().map(search::canonical))
            .collect()
    }

    fn reload_changed(&self, changed: &[PathBuf]) {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        for t in tabs {
            let Some(p) = &t.path else { continue };
            if sci::is_modified(&t.view) || !changed.contains(&search::canonical(p)) {
                continue;
            }
            if let Ok(b) = std::fs::read(p) {
                sci::reload(&t.view, &b);
            }
        }
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
        let existing = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .position(|t| t.path.as_deref() == Some(path));
        if let Some(t) = existing.and_then(|i| self.tab(i)) {
            self.tab_view().selectTabViewItem(Some(&t.item));
            return;
        }
        match std::fs::read(path) {
            Ok(b) => self.add_tab(Some(path.to_path_buf()), &b),
            Err(e) => {
                self.alert(
                    &format!("Cannot open {}", path.display()),
                    &e.to_string(),
                    &["OK"],
                );
            }
        }
    }

    fn add_tab(&self, path: Option<PathBuf>, bytes: &[u8]) {
        let view = sci::new_view();
        sci::set_delegate(&view, self);
        sci::set_bytes(&view, bytes);
        let lang = path
            .as_deref()
            .and_then(|p| lang::language_for_path(cfg(), p));
        sci::apply_language(&view, cfg(), lang);
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
        let item = NSTabViewItem::new();
        item.setView(Some(&view));
        self.ivars().tabs.borrow_mut().push(Tab {
            view,
            item: item.clone(),
            path,
            name,
        });
        let last = self.ivars().tabs.borrow().len() - 1;
        self.refresh_title(last);
        self.tab_view().addTabViewItem(&item);
        self.tab_view().selectTabViewItem(Some(&item));
    }

    fn refresh_title(&self, i: usize) {
        let Some(t) = self.tab(i) else { return };
        let mark = if sci::is_modified(&t.view) { "*" } else { "" };
        t.item
            .setLabel(&NSString::from_str(&format!("{mark}{}", t.name)));
    }

    fn save(&self, i: usize, ask: bool) -> bool {
        let Some(tab) = self.tab(i) else { return false };
        let path = match tab.path.clone().filter(|_| !ask) {
            Some(p) => p,
            None => {
                let p = NSSavePanel::savePanel(self.mtm());
                p.setNameFieldStringValue(&NSString::from_str(&tab.name));
                if p.runModal() != NSModalResponseOK {
                    return false;
                }
                match p.URL().and_then(|u: Retained<NSURL>| u.path()) {
                    Some(s) => PathBuf::from(s.to_string()),
                    None => return false,
                }
            }
        };
        if let Err(e) = std::fs::write(&path, sci::bytes(&tab.view)) {
            self.alert(
                &format!("Cannot save {}", path.display()),
                &e.to_string(),
                &["OK"],
            );
            return false;
        }
        let renamed = tab.path.as_deref() != Some(&path);
        if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
            t.name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            t.path = Some(path.clone());
        }
        sci::set_save_point(&tab.view);
        if renamed {
            sci::apply_language(&tab.view, cfg(), lang::language_for_path(cfg(), &path));
        }
        self.refresh_title(i);
        true
    }

    fn confirm_close(&self, i: usize) -> bool {
        let Some(tab) = self.tab(i) else { return true };
        if !sci::is_modified(&tab.view) {
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
    let m = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    items.iter().for_each(|i| m.addItem(i));
    let top = NSMenuItem::new(mtm);
    top.setSubmenu(Some(&m));
    bar.addItem(&top);
}

fn main() {
    let mtm = MainThreadMarker::new().unwrap();
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
    submenu(
        mtm,
        &bar,
        "File",
        vec![
            item(mtm, "New", sel!(newDocument:), "n", t),
            item(mtm, "Open...", sel!(openDocument:), "o", t),
            item(mtm, "Save", sel!(saveDocument:), "s", t),
            item(mtm, "Save As...", sel!(saveDocumentAs:), "S", t),
            item(mtm, "Close", sel!(closeTab:), "w", t),
        ],
    );
    submenu(
        mtm,
        &bar,
        "Edit",
        vec![
            item(mtm, "Undo", sel!(undo:), "z", None),
            item(mtm, "Redo", sel!(redo:), "Z", None),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Cut", sel!(cut:), "x", None),
            item(mtm, "Copy", sel!(copy:), "c", None),
            item(mtm, "Paste", sel!(paste:), "v", None),
            item(mtm, "Select All", sel!(selectAll:), "a", None),
        ],
    );
    let replace = item(mtm, "Replace...", sel!(showReplace:), "f", t);
    replace
        .setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Option);
    submenu(
        mtm,
        &bar,
        "Search",
        vec![
            item(mtm, "Find...", sel!(showFind:), "f", t),
            item(mtm, "Find in Files...", sel!(showFindInFiles:), "F", t),
            item(mtm, "Find Next", sel!(findNext:), "g", t),
            item(mtm, "Find Previous", sel!(findPrevious:), "G", t),
            replace,
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Go to...", sel!(goToLine:), "l", t),
        ],
    );
    app.setMainMenu(Some(&bar));
    app.run();
}
