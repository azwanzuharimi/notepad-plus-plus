// SPDX-License-Identifier: GPL-3.0-or-later
use crate::encoding::{self, Enc};
use crate::prefs::{self, AutoDetect as Detect};
use crate::{ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSApplication, NSTabViewItem};
use std::cell::Cell;
use std::path::Path;
use std::time::SystemTime;

const SCI_GETFIRSTVISIBLELINE: u32 = 2152;
const SCI_SETFIRSTVISIBLELINE: u32 = 2613;
const SCI_DOCUMENTEND: u32 = 2318;

pub fn stamp(p: &Path) -> Option<SystemTime> {
    let m = std::fs::metadata(p).ok().filter(|m| m.is_file())?;
    m.modified().ok()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Unchanged,
    Reload,
    AskReload,
    AskKeep,
}

// Buffer::checkFileState and Notepad_plus::notifyBufferChanged: the step for one tab of a file on disk.
pub fn decide(
    d: &Detect,
    known: Option<SystemTime>,
    now: Option<SystemTime>,
    dirty: bool,
) -> Action {
    if !d.enabled {
        return Action::Unchanged;
    }
    match (known, now) {
        (Some(_), None) => Action::AskKeep,
        (k, Some(n)) if k != Some(n) => {
            if d.silent && !dirty {
                Action::Reload
            } else {
                Action::AskReload
            }
        }
        _ => Action::Unchanged,
    }
}

thread_local! {
    static CHECKING: Cell<bool> = const { Cell::new(false) };
}

impl App {
    // WM_ACTIVATEAPP: NPPM_INTERNAL_CHECKDOCSTATUS checks the current tab, or all tabs for "Enable for all opened files".
    pub(crate) fn schedule_file_status_activated(&self) {
        let _: () = unsafe {
            msg_send![self, performSelector: sel!(fileStatusCheckActivated:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]
        };
    }

    pub(crate) fn file_status_activated(&self) {
        let d = prefs::with(|p| p.auto_detect());
        if d.enabled {
            self.check_files(&d, d.all_files);
        }
    }

    // Notepad_plus::notifyBufferActivated checks the new current tab for cdEnabledNew.
    pub(crate) fn file_status_tab_switched(&self) {
        let d = prefs::with(|p| p.auto_detect());
        if d.enabled && !d.all_files {
            self.check_files(&d, false);
        }
    }

    pub(crate) fn schedule_file_status_check(&self) {
        let _: () = unsafe {
            msg_send![self, performSelector: sel!(fileStatusCheckCurrent:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]
        };
    }

    fn check_files(&self, d: &Detect, all: bool) {
        if self.ivars().tab_view.get().is_none()
            || CHECKING.with(Cell::get)
            || self.ivars().replacing.get()
            || NSApplication::sharedApplication(self.mtm())
                .modalWindow()
                .is_some()
        {
            return;
        }
        CHECKING.with(|c| c.set(true));
        let items: Vec<Retained<NSTabViewItem>> = if all {
            self.ivars()
                .tabs
                .borrow()
                .iter()
                .enumerate()
                .rev()
                .filter(|&(i, _)| self.first_copy(i) == i)
                .map(|(_, t)| t.item.clone())
                .collect()
        } else {
            self.tab_view().selectedTabViewItem().into_iter().collect()
        };
        for item in &items {
            self.check_file(d, item);
        }
        CHECKING.with(|c| c.set(false));
    }

    fn tab_at(&self, item: &NSTabViewItem) -> Option<usize> {
        self.ivars()
            .tabs
            .borrow()
            .iter()
            .position(|t| std::ptr::eq(&*t.item, item))
    }

    fn check_file(&self, d: &Detect, item: &Retained<NSTabViewItem>) {
        if self.monitored(item) {
            return;
        }
        let Some(i) = self.tab_at(item) else { return };
        let Some(t) = self.tab(i) else { return };
        let Some(path) = t.path.clone() else { return };
        let now = stamp(&path);
        let dirty = self.dirty(&t);
        let action = decide(d, t.mtime, now, dirty);
        if action == Action::Unchanged {
            return;
        }
        if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
            t.mtime = now;
        }
        self.sync_clones(i);
        let shown = format!("\"{}\"", path.display());
        match action {
            Action::Unchanged => {}
            Action::Reload => self.reload_tab(item, d.go_to_end),
            Action::AskReload => {
                self.show_changed_tab(item);
                let msg = if dirty {
                    format!("{shown}\n\nThis file has been modified by another program.\nDo you want to reload it and lose the changes made in Notepad++?")
                } else {
                    format!("{shown}\n\nThis file has been modified by another program.\nDo you want to reload it?")
                };
                if self.ask_yes_no("Reload", &msg, dirty) {
                    self.reload_tab(item, d.go_to_end);
                } else if let Some(i) = self.tab_at(item) {
                    self.set_enc(i, t.enc, true);
                }
            }
            Action::AskKeep => {
                self.set_enc(i, t.enc, true);
                self.show_changed_tab(item);
                let msg =
                    format!("The file {shown} doesn't exist anymore.\nKeep this file in editor?");
                if !self.ask_yes_no("Keep non existing file", &msg, false) {
                    if let Some(i) = self.tab_at(item) {
                        self.drop_tabs(&self.with_clones(i));
                    }
                }
            }
        }
    }

    // Notepad_plus::prepareBufferChangedDialog: restore the window and show the tab.
    fn show_changed_tab(&self, item: &NSTabViewItem) {
        if let Some(w) = self.ivars().window.get() {
            if w.isMiniaturized() {
                w.deminiaturize(None);
            }
        }
        self.tab_view().selectTabViewItem(Some(item));
    }

    // MB_DEFBUTTON2 makes No the default button.
    fn ask_yes_no(&self, title: &str, msg: &str, no_default: bool) -> bool {
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&ns(title));
        a.setInformativeText(&ns(msg));
        let yes = a.addButtonWithTitle(&ns("Yes"));
        let no = a.addButtonWithTitle(&ns("No"));
        if no_default {
            yes.setKeyEquivalent(&ns(""));
            no.setKeyEquivalent(&ns("\r"));
        }
        a.runModal() == NSAlertFirstButtonReturn
    }

    // NppIO.cpp doReload and performPostReload: caret and scroll stay, or the current tab goes to the end.
    fn reload_tab(&self, item: &NSTabViewItem, to_end: bool) {
        let Some(i) = self.tab_at(item) else { return };
        let Some(t) = self.tab(i) else { return };
        let Some(path) = t.path.clone() else { return };
        match std::fs::read(&path) {
            Ok(b) => {
                let e = match t.enc {
                    Enc::Cp(_) => t.enc,
                    _ => encoding::detect(&b),
                };
                let sel = sci::selection(&t.view);
                let first = sci::send(&t.view, SCI_GETFIRSTVISIBLELINE, 0, 0);
                self.load_into(i, &b, e);
                if to_end && self.current() == Some(i) {
                    sci::send(&t.view, SCI_DOCUMENTEND, 0, 0);
                } else {
                    sci::select(&t.view, sel);
                    sci::send(&t.view, SCI_SETFIRSTVISIBLELINE, first as usize, 0);
                }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn d(text: &str) -> Detect {
        let p = prefs::Prefs {
            auto_detect: text.into(),
            ..Default::default()
        };
        p.auto_detect()
    }

    #[test]
    fn decide_each_case() {
        use Action::*;
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let t1 = t0 + Duration::from_secs(1);
        let older = t0 - Duration::from_secs(1);
        let ask = d("yes");
        let silent = d("auto");
        let off = d("no");
        for s in [&ask, &silent] {
            for dirty in [false, true] {
                assert_eq!(decide(s, Some(t0), Some(t0), dirty), Unchanged);
                assert_eq!(decide(s, None, None, dirty), Unchanged);
                assert_eq!(decide(s, Some(t0), None, dirty), AskKeep);
            }
        }
        assert_eq!(decide(&ask, Some(t0), Some(t1), false), AskReload);
        assert_eq!(decide(&ask, Some(t0), Some(t1), true), AskReload);
        assert_eq!(decide(&ask, Some(t0), Some(older), false), AskReload);
        assert_eq!(decide(&silent, Some(t0), Some(t1), false), Reload);
        assert_eq!(decide(&silent, Some(t0), Some(t1), true), AskReload);
        assert_eq!(decide(&silent, None, Some(t1), false), Reload);
        assert_eq!(decide(&ask, None, Some(t1), true), AskReload);
        assert_eq!(
            decide(&d("autoUpdate2EndOld"), Some(t0), Some(t1), false),
            Reload
        );
        for (k, n) in [(Some(t0), Some(t1)), (Some(t0), None), (None, Some(t1))] {
            for dirty in [false, true] {
                assert_eq!(decide(&off, k, n, dirty), Unchanged);
            }
        }
    }

    #[test]
    fn stamp_of_missing_file_or_folder_is_none() {
        let dir = std::env::temp_dir().join(format!("npp-filestatus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.txt");
        std::fs::write(&f, "x").unwrap();
        assert!(stamp(&f).is_some());
        assert_eq!(stamp(&dir), None);
        std::fs::remove_file(&f).unwrap();
        assert_eq!(stamp(&f), None);
        std::fs::remove_dir(&dir).unwrap();
    }
}
