// SPDX-License-Identifier: GPL-3.0-or-later
mod config;
mod lang;
mod sci;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSApplicationTerminateReply,
    NSBackingStoreType, NSMenu, NSMenuItem, NSModalResponseOK, NSOpenPanel, NSSavePanel, NSTabView,
    NSTabViewDelegate, NSTabViewItem, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
}

#[repr(C)]
struct NotifyHeader {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
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
            let code = unsafe { (*(scn as *const NotifyHeader)).code };
            if code == sci::SCN_SAVEPOINTREACHED || code == sci::SCN_SAVEPOINTLEFT {
                let n = self.ivars().tabs.borrow().len();
                (0..n).for_each(|i| self.refresh_title(i));
            }
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
        w.setContentView(Some(&tv));
        w.center();
        w.makeKeyAndOrderFront(None);
        let _ = self.ivars().window.set(w);
        let _ = self.ivars().tab_view.set(tv);
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
    app.setMainMenu(Some(&bar));
    app.run();
}
