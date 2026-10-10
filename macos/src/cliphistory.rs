// SPDX-License-Identifier: GPL-3.0-or-later
use crate::docking::{column, content_box, insert_text, scroll};
use crate::{ns, App};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{sel, MainThreadOnly};
use objc2_app_kit::{
    NSPasteboard, NSPasteboardTypeString, NSTableColumnResizingOptions, NSTableView,
    NSTableViewColumnAutoresizingStyle, NSUserInterfaceItemIdentification, NSView,
};
use std::cell::{Cell, RefCell};

// Port only: Notepad++ keeps every item.
const MAX_ITEMS: usize = 100;
// clipboardHistoryPanel.cpp MAX_DISPLAY_LENGTH, in bytes of UTF-16 with the end null.
const MAX_DISPLAY: usize = 64;
// Port only: a text longer than 1 Mi UTF-16 units is not kept; Notepad++ keeps any size.
const MAX_TEXT: usize = 1 << 20;

// A text of the history and the start of it that the list shows.
pub struct Clip {
    pub text: String,
    pub shown: String,
}

pub struct Clips {
    table: Retained<NSTableView>,
    items: RefCell<Vec<Clip>>,
    seen: Cell<isize>,
}

// ClipboardHistoryPanel::addToClipboadHistory: a new text goes to the top; a text in the list moves to the top.
pub fn add(items: &mut Vec<Clip>, s: String, max: usize) {
    if items.first().is_some_and(|f| f.text == s) {
        return;
    }
    items.retain(|x| x.text != s);
    let shown = display(&s);
    items.insert(0, Clip { text: s, shown });
    items.truncate(max);
}

// StringArray: a long text shows its start and "..."; DT_SINGLELINE draws no line breaks.
pub fn display(s: &str) -> String {
    let units: Vec<u16> = s.encode_utf16().take(MAX_DISPLAY / 2).collect();
    let shown = if (units.len() + 1) * 2 <= MAX_DISPLAY {
        s.to_string()
    } else {
        String::from_utf16_lossy(&units[..MAX_DISPLAY / 2 - 3]) + "..."
    };
    shown.replace(['\r', '\n'], "")
}

impl App {
    pub(crate) fn clips_build(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let b = content_box(mtm);
        let size = b.frame().size;
        let table = NSTableView::initWithFrame(NSTableView::alloc(mtm), b.frame());
        table.setIdentifier(Some(&ns("clips")));
        let c = column(mtm, "text", "", size.width);
        c.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        table.addTableColumn(&c);
        table.setHeaderView(None);
        table.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        let Some(d) = self.dock_ui() else { return b };
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*d.target)));
            table.setTarget(Some(&d.target));
            table.setDoubleAction(Some(sel!(clipInsert:)));
        }
        b.addSubview(&scroll(mtm, &table, size.width, size.height));
        let _ = d.clips.set(Clips {
            table,
            items: RefCell::new(vec![]),
            seen: Cell::new(-1),
        });
        self.clips_poll();
        b
    }

    // WM_DRAWCLIPBOARD: macOS sends no clipboard event, so a timer reads the change count while the app is active.
    pub(crate) fn clips_poll(&self) {
        let Some(c) = self.dock_ui().and_then(|d| d.clips.get()) else {
            return;
        };
        let pb = NSPasteboard::generalPasteboard();
        let n = pb.changeCount();
        if c.seen.replace(n) == n {
            return;
        }
        let text = pb
            .stringForType(unsafe { NSPasteboardTypeString })
            .filter(|s| s.length() <= MAX_TEXT)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        if let Some(s) = text {
            add(&mut c.items.borrow_mut(), s, MAX_ITEMS);
            c.table.reloadData();
        }
    }

    pub(crate) fn clips_count(&self) -> isize {
        self.dock_ui()
            .and_then(|d| d.clips.get())
            .map_or(0, |c| c.items.borrow().len() as isize)
    }

    pub(crate) fn clips_text(&self, row: isize) -> Option<String> {
        let c = self.dock_ui()?.clips.get()?;
        let items = c.items.borrow();
        Some(items.get(row as usize)?.shown.clone())
    }

    // ClipboardHistoryPanel LBN_DBLCLK: the text replaces the selection.
    pub(crate) fn clips_insert(&self) {
        if let Some(c) = self.dock_ui().and_then(|d| d.clips.get()) {
            self.clips_insert_at(c.table.clickedRow());
        }
    }

    pub(crate) fn clips_insert_at(&self, row: isize) {
        let Some(c) = self.dock_ui().and_then(|d| d.clips.get()) else {
            return;
        };
        let text = c.items.borrow().get(row.max(0) as usize).map(|x| x.text.clone());
        let (Some(text), Some(v)) = (text.filter(|_| row >= 0), self.editor()) else {
            return;
        };
        insert_text(&v, &text);
        self.focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_dedupe_and_limit() {
        let mut v = vec![];
        let texts = |v: &Vec<Clip>| v.iter().map(|c| c.text.clone()).collect::<Vec<_>>();
        for x in ["a", "b", "b"] {
            add(&mut v, x.into(), 3);
        }
        assert_eq!(texts(&v), ["b", "a"]);
        add(&mut v, "a".into(), 3);
        assert_eq!(texts(&v), ["a", "b"]);
        add(&mut v, "c".into(), 3);
        add(&mut v, "d\r\n".repeat(20), 3);
        assert_eq!(texts(&v)[1..], ["c", "a"]);
        assert_eq!(v[0].shown, "d".repeat(10) + "...");
    }

    #[test]
    fn display_text() {
        assert_eq!(display("short\r\ntext"), "shorttext");
        let s31 = "x".repeat(31);
        assert_eq!(display(&s31), s31);
        assert_eq!(display(&"y".repeat(32)), "y".repeat(29) + "...");
    }
}
