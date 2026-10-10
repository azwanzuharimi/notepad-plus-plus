// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{item, nested, ns, sci, search, tagged, App, Tab};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSEventModifierFlags, NSMenuItem, NSModalResponseOK,
    NSDragOperation, NSPasteboard, NSPasteboardTypeFileURL, NSSavePanel, NSTabViewItem,
    NSTextField, NSWindow, NSWorkspace,
};
use objc2_foundation::{NSArray, NSFileManager, NSPoint, NSRect, NSSize, NSURL};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Close {
    All,
    ButActive,
    Left,
    Right,
    Unchanged,
}

const CLOSE: [Close; 5] = [
    Close::All,
    Close::ButActive,
    Close::Left,
    Close::Right,
    Close::Unchanged,
];

// Canonical form of a path; a missing file uses the canonical form of its folder.
fn key(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| match (p.parent(), p.file_name()) {
        (Some(d), Some(n)) => search::canonical(d).join(n),
        _ => p.to_path_buf(),
    })
}

pub fn same_file(a: &Path, b: &Path) -> bool {
    a == b || key(a) == key(b)
}

// Index of the tab, other than `except`, that holds the file `p`.
pub fn open_index(paths: &[Option<PathBuf>], p: &Path, except: Option<usize>) -> Option<usize> {
    let k = key(p);
    paths
        .iter()
        .enumerate()
        .position(|(i, t)| Some(i) != except && t.as_deref().is_some_and(|t| t == p || key(t) == k))
}

// NppIO.cpp fileCloseAll*: the tabs to close, in the order Notepad++ asks about them.
pub fn to_close(kind: Close, active: usize, unchanged: &[bool]) -> Vec<usize> {
    let n = unchanged.len();
    match kind {
        Close::All => (0..n).collect(),
        Close::ButActive => (0..n).filter(|&i| i != active).collect(),
        Close::Left => (0..active.min(n)).rev().collect(),
        Close::Right => (active + 1..n).rev().collect(),
        Close::Unchanged => (0..n).rev().filter(|&i| unchanged[i]).collect(),
    }
}

fn exists(t: &Tab) -> bool {
    t.path.as_deref().is_some_and(Path::exists)
}

fn file_url(p: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&ns(&p.to_string_lossy()))
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

pub fn file_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let save_all = item(mtm, "Save All", sel!(saveAll:), "s", t);
    save_all
        .setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Option);
    let close = |n, k: Close| tagged(mtm, n, sel!(closeMultiple:), k as isize, t);
    let close_all = close("Close All", Close::All);
    close_all.setKeyEquivalent(&ns("W"));
    vec![
        item(mtm, "New", sel!(newDocument:), "n", t),
        item(mtm, "Open...", sel!(openDocument:), "o", t),
        nested(
            mtm,
            "Open Containing Folder",
            vec![
                item(mtm, "Finder", sel!(openFolderFinder:), "", t),
                item(mtm, "Terminal", sel!(openFolderTerminal:), "", t),
                NSMenuItem::separatorItem(mtm),
                item(mtm, "Folder as Workspace", sel!(containingFolderAsWorkspace:), "", t),
            ],
        ),
        item(
            mtm,
            "Open in Default Viewer",
            sel!(openDefaultViewer:),
            "",
            t,
        ),
        item(mtm, "Open Folder as Workspace...", sel!(openFolderAsWorkspace:), "", t),
        item(mtm, "Reload from Disk", sel!(reloadFromDisk:), "r", t),
        item(mtm, "Save", sel!(saveDocument:), "s", t),
        item(mtm, "Save As...", sel!(saveDocumentAs:), "S", t),
        item(mtm, "Save a Copy As...", sel!(saveCopyAs:), "", t),
        save_all,
        item(mtm, "Rename...", sel!(renameFile:), "", t),
        item(mtm, "Close", sel!(closeTab:), "w", t),
        close_all,
        nested(
            mtm,
            "Close Multiple Documents",
            vec![
                close("Close All but Active Document", Close::ButActive),
                close("Close All to the Left", Close::Left),
                close("Close All to the Right", Close::Right),
                close("Close All Unchanged", Close::Unchanged),
            ],
        ),
        item(mtm, "Move to Trash", sel!(moveToTrash:), "", t),
    ]
    .into_iter()
    .chain(crate::session::session_menu(mtm, t))
    .chain([
        NSMenuItem::separatorItem(mtm),
        item(mtm, "Print...", sel!(filePrint:), "p", t),
        item(mtm, "Print Now", sel!(filePrintNow:), "", t),
    ])
    .collect()
}

impl App {
    fn tab_paths(&self) -> Vec<Option<PathBuf>> {
        self.ivars()
            .tabs
            .borrow()
            .iter()
            .map(|t| t.path.clone())
            .collect()
    }

    fn index_of(&self, item: &NSTabViewItem) -> Option<usize> {
        self.ivars()
            .tabs
            .borrow()
            .iter()
            .position(|t| std::ptr::eq(&*t.item, item))
    }

    pub(crate) fn find_open(&self, p: &Path, except: Option<usize>) -> Option<usize> {
        open_index(&self.tab_paths(), p, except)
    }

    // Enable state for the File items of this module; None for other items.
    pub(crate) fn validate_file(&self, action: Sel) -> Option<bool> {
        let tab = self.current().and_then(|i| self.tab(i));
        let busy = self.ivars().replacing.get();
        let on_disk = tab.as_ref().is_some_and(exists);
        Some(
            if action == sel!(reloadFromDisk:) || action == sel!(moveToTrash:) {
                on_disk && !busy
            } else if action == sel!(openFolderFinder:) || action == sel!(openFolderTerminal:) {
                on_disk
            } else if action == sel!(openDefaultViewer:) {
                on_disk
                    && tab.and_then(|t| t.path).is_some_and(|p| {
                        NSWorkspace::sharedWorkspace()
                            .URLForApplicationToOpenURL(&file_url(&p))
                            .is_some()
                    })
            } else if action == sel!(renameFile:) {
                !busy && tab.is_some_and(|t| t.path.is_none() || exists(&t))
            } else if action == sel!(saveAll:) {
                let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
                !busy && tabs.iter().any(|t| self.dirty(t))
            } else if action == sel!(saveCopyAs:) {
                !busy && tab.is_some()
            } else {
                return None;
            },
        )
    }

    // Save panel for Save As, Save a Copy As and Rename; refuses a file open in another tab.
    pub(crate) fn ask_save_path(&self, i: usize, title: &str) -> Option<PathBuf> {
        let tab = self.tab(i)?;
        let p = NSSavePanel::savePanel(self.mtm());
        p.setTitle(Some(&ns(title)));
        p.setNameFieldStringValue(&ns(&tab.name));
        let dir = tab.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        if let Some(dir) = dir.or_else(|| crate::prefs::dialog_dir(None)) {
            p.setDirectoryURL(Some(&file_url(&dir)));
        }
        if title == "Rename" {
            p.setPrompt(Some(&ns("Rename")));
        }
        if p.runModal() != NSModalResponseOK {
            return None;
        }
        let path = PathBuf::from(p.URL()?.path()?.to_string());
        crate::prefs::used_file(&path);
        let other = self.find_open(&path, Some(i)).filter(|o| !self.clones_of(i).contains(o));
        if let Some(other) = other.and_then(|o| self.tab(o)) {
            self.alert("The file is already opened in Notepad++.", "", &["OK"]);
            self.tab_view().selectTabViewItem(Some(&other.item));
            return None;
        }
        Some(path)
    }

    // Removes the tabs without a question; an empty window gets a new tab.
    pub(crate) fn drop_tabs(&self, items: &[Retained<NSTabViewItem>]) {
        let active = self.active_view();
        for item in items {
            match self.clone_view(item) {
                Some((from, to)) => self.backup_to_clone(&from, &to),
                None => {
                    self.recent_closed(item);
                    self.backup_closed(item);
                }
            }
            self.ivars()
                .tabs
                .borrow_mut()
                .retain(|t| !std::ptr::eq(&*t.item, &**item));
            self.tab_view().removeTabViewItem(item);
        }
        self.keep_active(active);
        self.layout_views();
        if self.ivars().tabs.borrow().is_empty() {
            self.ivars().untitled.set(0);
            self.add_tab(None, crate::Enc::Utf8, b"", false);
        }
        self.focus();
    }

    // NppIO.cpp doReload: a modified tab asks first; a character set stays, else detection runs again.
    pub(crate) fn reload_from_disk(&self) {
        let Some(i) = self.current() else { return };
        let Some(t) = self.tab(i) else { return };
        let Some(path) = t.path.clone() else { return };
        if self.dirty(&t)
            && self.alert(
                "Are you sure you want to reload the current file and lose the changes made in Notepad++?",
                "",
                &["Yes", "No"],
            ) != NSAlertFirstButtonReturn
        {
            return;
        }
        match std::fs::read(&path) {
            Ok(b) => {
                let e = match t.enc {
                    crate::Enc::Cp(_) => t.enc,
                    _ => crate::encoding::detect(&b),
                };
                let sel = sci::selection(&t.view);
                self.load_into(i, &b, e);
                sci::select(&t.view, sel);
            }
            Err(err) => {
                self.alert(
                    &format!("Cannot open {}", path.display()),
                    &err.to_string(),
                    &["OK"],
                );
            }
        }
    }

    pub(crate) fn save_copy_as(&self) {
        let Some(i) = self.current() else { return };
        let Some(t) = self.tab(i) else { return };
        if let Some(path) = self.ask_save_path(i, "Save a Copy As") {
            self.write_tab(&t, &path);
        }
    }

    // NppIO.cpp fileSaveAll: one modified current tab saves at once, else a confirmation shows first.
    pub(crate) fn save_all(&self) {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        let dirty: Vec<usize> = (0..tabs.len())
            .filter(|&i| self.dirty(&tabs[i]) && self.first_copy(i) == i)
            .collect();
        let cur = self.current().map(|c| self.first_copy(c));
        if dirty.is_empty() {
            return;
        }
        if dirty.len() == 1 && cur == Some(dirty[0]) {
            self.save(dirty[0], false);
            return;
        }
        if self.alert(
            "Are you sure you want to save all modified documents?",
            "",
            &["Yes", "No"],
        ) != NSAlertFirstButtonReturn
        {
            return;
        }
        for t in tabs.iter().filter(|t| self.dirty(t)) {
            let Some(i) = self.index_of(&t.item) else {
                continue;
            };
            if !self.tab(i).is_some_and(|t| self.dirty(&t)) {
                continue;
            }
            if t.path.is_none() {
                self.tab_view().selectTabViewItem(Some(&t.item));
            }
            self.save(i, false);
        }
    }

    // NppIO.cpp fileRename: a file on disk moves; a tab with no file gets a new tab name.
    pub(crate) fn rename_file(&self) {
        let Some(i) = self.current() else { return };
        let Some(t) = self.tab(i) else { return };
        let Some(old) = t.path.clone() else {
            self.rename_tab(i);
            return;
        };
        let Some(new) = self.ask_save_path(i, "Rename") else {
            return;
        };
        if new == old {
            return;
        }
        let moved = std::fs::rename(&old, &new)
            .or_else(|_| std::fs::copy(&old, &new).and_then(|_| std::fs::remove_file(&old)));
        if let Err(e) = moved {
            self.alert(
                &format!("Cannot rename {}", old.display()),
                &e.to_string(),
                &["OK"],
            );
            return;
        }
        if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
            t.name = file_name(&new);
            t.path = Some(new.clone());
        }
        self.apply_tab_language(i);
        self.refresh_title(i);
        self.update_status();
    }

    fn rename_tab(&self, i: usize) {
        let Some(t) = self.tab(i) else { return };
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&ns("Rename Current Tab"));
        a.setInformativeText(&ns("New name"));
        let f = NSTextField::textFieldWithString(&ns(&t.name), self.mtm());
        f.setFrame(NSRect::new(NSPoint::new(0., 0.), NSSize::new(260., 24.)));
        a.setAccessoryView(Some(&f));
        a.addButtonWithTitle(&ns("OK"));
        a.addButtonWithTitle(&ns("Cancel"));
        a.window().setInitialFirstResponder(Some(&f));
        if a.runModal() != NSAlertFirstButtonReturn {
            return;
        }
        let name = f.stringValue().to_string().trim().to_string();
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        if name == t.name {
            return;
        }
        let clones = self.clones_of(i);
        if tabs
            .iter()
            .enumerate()
            .any(|(k, o)| k != i && !clones.contains(&k) && o.name == name)
        {
            self.alert(
                "Rename failed",
                "The specified name is already in use on another tab.",
                &["OK"],
            );
        } else if name.is_empty() {
            self.alert(
                "Rename failed",
                "The specified name cannot be empty, or it cannot contain only space(s) or TAB(s).",
                &["OK"],
            );
        } else {
            if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
                t.name = name;
            }
            self.apply_tab_language(i);
            self.refresh_title(i);
            self.update_status();
        }
    }

    // Asks about each modified tab like Close; Cancel closes nothing.
    pub(crate) fn close_multiple(&self, tag: isize) {
        let Some(&kind) = CLOSE.get(tag as usize) else {
            return;
        };
        let Some(active) = self.current() else { return };
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        let unchanged: Vec<bool> = tabs
            .iter()
            .map(|t| !self.dirty(t) || (t.path.is_none() && sci::length(&t.view) == 0))
            .collect();
        let r = match kind {
            Close::Left | Close::Right => self.view_range(self.active_view()),
            _ => 0..tabs.len(),
        };
        let items: Vec<_> = to_close(kind, active - r.start, &unchanged[r.clone()])
            .into_iter()
            .map(|k| tabs[k + r.start].item.clone())
            .collect();
        if !self.confirm_close_all(&items) {
            return;
        }
        self.drop_tabs(&items);
        if let Some(t) = tabs
            .get(active)
            .filter(|t| self.index_of(&t.item).is_some())
        {
            self.tab_view().selectTabViewItem(Some(&t.item));
        }
    }

    // NppIO.cpp fileDelete: confirm, move the file to the Trash, then close the tab without a question.
    pub(crate) fn move_to_trash(&self) {
        let Some((i, t)) = self.current().and_then(|i| Some((i, self.tab(i)?))) else {
            return;
        };
        let Some(path) = t.path.clone() else { return };
        let msg = format!(
            "The file \"{}\"\nwill be moved to the Trash and this document will be closed.\nContinue?",
            path.display()
        );
        if self.alert("Delete file", &msg, &["OK", "Cancel"]) != NSAlertFirstButtonReturn {
            return;
        }
        let r = NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&file_url(&path), None);
        if let Err(e) = r {
            self.alert(
                "Delete File failed",
                &e.localizedDescription().to_string(),
                &["OK"],
            );
            return;
        }
        self.drop_tabs(&self.with_clones(i));
    }

    pub(crate) fn open_folder(&self, terminal: bool) {
        let Some(path) = self.current().and_then(|i| self.tab(i)?.path) else {
            return;
        };
        if terminal {
            let dir = path.parent().unwrap_or(&path);
            if let Err(e) = std::process::Command::new("open")
                .arg("-a")
                .arg("Terminal")
                .arg(dir)
                .spawn()
            {
                self.alert("Cannot open Terminal", &e.to_string(), &["OK"]);
            }
        } else {
            NSWorkspace::sharedWorkspace()
                .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[file_url(&path)]));
        }
    }

    pub(crate) fn open_default_viewer(&self) {
        let Some(path) = self.current().and_then(|i| self.tab(i)?.path) else {
            return;
        };
        if !NSWorkspace::sharedWorkspace().openURL(&file_url(&path)) {
            self.alert(
                "Open in Default Viewer - ERROR",
                &format!(
                    "An attempt was made to open the below file.\n{}",
                    path.display()
                ),
                &["OK"],
            );
        }
    }
}

pub const SCN_URIDROPPED: u32 = 2015;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Drop {
    Nothing,
    Open,
    Workspace,
    Mixed,
}

// Notepad_plus::dropFiles: open the paths, launch Folder as Workspace on folders, or refuse a mix in that mode.
pub fn drop_action(folders: usize, files: usize, open_all: bool) -> Drop {
    match (folders, files) {
        (0, 0) => Drop::Nothing,
        _ if open_all || folders == 0 => Drop::Open,
        (_, 0) => Drop::Workspace,
        _ => Drop::Mixed,
    }
}

// Notepad_plus::doOpen on a folder: all files, also in sub-folders, but not in hidden folders.
pub fn folder_files(dir: &Path) -> Vec<PathBuf> {
    search::walk(dir, &["*".to_string()], true, false)
}

// The window takes file drops outside the editors; the editors send SCN_URIDROPPED.
pub(crate) fn accept_file_drops(w: &NSWindow) {
    w.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
}

pub(crate) fn pasteboard_files(pb: &NSPasteboard) -> Vec<PathBuf> {
    let Some(items) = pb.pasteboardItems() else {
        return vec![];
    };
    items
        .iter()
        .filter_map(|i| {
            let s = i.stringForType(unsafe { NSPasteboardTypeFileURL })?;
            Some(PathBuf::from(NSURL::URLWithString(&s)?.path()?.to_string()))
        })
        .collect()
}

pub(crate) fn drag_operation(pb: &NSPasteboard) -> NSDragOperation {
    if pasteboard_files(pb).is_empty() {
        NSDragOperation::None
    } else {
        NSDragOperation::Copy
    }
}

thread_local! {
    static DROPPED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[repr(C)]
struct UriNotify {
    hwnd_from: *mut std::ffi::c_void,
    id_from: usize,
    code: u32,
    position: isize,
    ch: i32,
    modifiers: i32,
    modification_type: i32,
    text: *const std::ffi::c_char,
}

impl App {
    // Scintilla sends SCN_URIDROPPED one time for each file; the paths are collected and opened together.
    pub(crate) fn uri_dropped(&self, scn: *const std::ffi::c_void) {
        let n = unsafe { &*(scn as *const UriNotify) };
        if n.code != SCN_URIDROPPED || n.text.is_null() {
            return;
        }
        let path = unsafe { std::ffi::CStr::from_ptr(n.text) };
        let path = PathBuf::from(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(path.to_bytes()));
        let first = DROPPED.with(|d| {
            let mut d = d.borrow_mut();
            d.push(path);
            d.len() == 1
        });
        if first {
            let _: () = unsafe {
                objc2::msg_send![self, performSelector: sel!(dropPending:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]
            };
        }
    }

    pub(crate) fn drop_pending(&self) {
        let paths = DROPPED.with(|d| std::mem::take(&mut *d.borrow_mut()));
        self.drop_paths(&paths);
    }

    // Notepad_plus::dropFiles.
    pub(crate) fn drop_paths(&self, paths: &[PathBuf]) {
        let folders: Vec<&PathBuf> = paths.iter().filter(|p| p.is_dir()).collect();
        let open_all = crate::prefs::get().drop_folder_open_files;
        match drop_action(folders.len(), paths.len() - folders.len(), open_all) {
            Drop::Nothing => {}
            Drop::Open => {
                for p in paths {
                    if p.is_dir() {
                        self.open_folder_files(p);
                    } else {
                        self.open_path(p);
                    }
                }
            }
            Drop::Workspace => {
                if !self.panel_visible(crate::docking::FOLDERS) {
                    self.dock_open_panel(crate::docking::FOLDERS);
                }
                for f in folders {
                    self.folders_add_root(f);
                }
            }
            Drop::Mixed => {
                self.alert(
                    "Invalid action",
                    "You can only drop files or folders but not both, because you're in dropping Folder as Project mode.\nYou have to enable \"Open all files of folder instead of launching Folder as Workspace on folder dropping\" in \"Default Directory\" section of Preferences dialog to make this operation work.",
                    &["OK"],
                );
            }
        }
    }

    fn open_folder_files(&self, dir: &Path) {
        let files = folder_files(dir);
        if files.len() > 200
            && self.alert(
                "Amount of files to open is too large",
                &format!("{} files are about to be opened.\nAre you sure to open them?", files.len()),
                &["Yes", "No"],
            ) != NSAlertFirstButtonReturn
        {
            return;
        }
        for f in &files {
            self.open_path(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/a.txt"), "a").unwrap();
        d
    }

    #[test]
    fn same_file_after_canonical() {
        let d = tmp("fileops-same");
        let a = d.join("sub/a.txt");
        std::os::unix::fs::symlink(d.join("sub"), d.join("link")).unwrap();
        assert!(same_file(&a, &d.join("sub/../sub/a.txt")));
        assert!(same_file(&a, &d.join("link/a.txt")));
        assert!(same_file(&d.join("sub/new.txt"), &d.join("link/new.txt")));
        assert!(!same_file(&a, &d.join("sub/b.txt")));
        if d.join("SUB/A.TXT").exists() {
            assert!(same_file(&a, &d.join("SUB/A.TXT")));
        }
    }

    #[test]
    fn open_index_skips_own_tab() {
        let d = tmp("fileops-open");
        let a = d.join("sub/a.txt");
        let paths = vec![None, Some(d.join("sub/x.txt")), Some(a.clone())];
        assert_eq!(open_index(&paths, &d.join("sub/./a.txt"), None), Some(2));
        assert_eq!(open_index(&paths, &a, Some(2)), None);
        assert_eq!(open_index(&paths, &d.join("sub/y.txt"), None), None);
    }

    #[test]
    fn close_sets() {
        let u = [true, false, true, true, false];
        assert_eq!(to_close(Close::All, 2, &u), [0, 1, 2, 3, 4]);
        assert_eq!(to_close(Close::ButActive, 2, &u), [0, 1, 3, 4]);
        assert_eq!(to_close(Close::Left, 2, &u), [1, 0]);
        assert_eq!(to_close(Close::Right, 2, &u), [4, 3]);
        assert_eq!(to_close(Close::Unchanged, 2, &u), [3, 2, 0]);
        assert!(to_close(Close::Left, 0, &u).is_empty());
        assert!(to_close(Close::Right, 4, &u).is_empty());
    }

    #[test]
    fn drop_actions() {
        assert_eq!(drop_action(0, 0, false), Drop::Nothing);
        assert_eq!(drop_action(0, 2, false), Drop::Open);
        assert_eq!(drop_action(2, 0, false), Drop::Workspace);
        assert_eq!(drop_action(1, 1, false), Drop::Mixed);
        assert_eq!(drop_action(1, 1, true), Drop::Open);
        assert_eq!(drop_action(2, 0, true), Drop::Open);
    }

    #[test]
    fn folder_files_skip_hidden_folders() {
        let d = tmp("drop_folder");
        for f in ["a.txt", ".hidden.txt", "sub/b.c", "sub/deep/c.md", ".git/config"] {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        let got: Vec<String> = folder_files(&d)
            .iter()
            .map(|p| p.strip_prefix(&d).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(got, [".hidden.txt", "a.txt", "sub/a.txt", "sub/b.c", "sub/deep/c.md"]);
    }
}
