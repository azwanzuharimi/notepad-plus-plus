// SPDX-License-Identifier: GPL-3.0-or-later
use crate::search::commafy;
use crate::{ns, prefs, sci, App, STATUS_H};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{sel, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSButton, NSClickGestureRecognizer,
    NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSTextField, NSView,
};
use objc2_foundation::{NSCopying, NSPoint, NSRect, NSSize, NSString};

const SCI_GETLENGTH: u32 = 2006;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GOTOLINE: u32 = 2024;
const SCI_GOTOPOS: u32 = 2025;
const SCI_GETSELECTIONSTART: u32 = 2143;
const SCI_GETSELECTIONEND: u32 = 2145;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_LINEFROMPOSITION: u32 = 2166;
const SCI_POSITIONFROMLINE: u32 = 2167;
const SCI_SETOVERTYPE: u32 = 2186;
const SCI_GETOVERTYPE: u32 = 2187;
const SCI_ENSUREVISIBLE: u32 = 2232;
const SCI_SELECTIONISRECTANGLE: u32 = 2372;
const SCI_POSITIONBEFORE: u32 = 2417;
const SCI_POSITIONAFTER: u32 = 2418;
const SCI_GETSELECTIONS: u32 = 2570;
const SCI_GETSELECTIONNSTART: u32 = 2585;
const SCI_GETSELECTIONNEND: u32 = 2587;
const SCI_COUNTCHARACTERS: u32 = 2633;

// Status bar parts, as STATUSBAR_* in Notepad_plus.h.
const DOC_TYPE: isize = 0;
const DOC_SIZE: isize = 1;
const CUR_POS: isize = 2;
const EOF_FORMAT: isize = 3;
const UNICODE_TYPE: isize = 4;
const TYPING_MODE: isize = 5;

// Go to dialog control tags.
const GO_LINE: isize = 1;
const GO_OFFSET: isize = 2;
const GO_CURRENT: isize = 3;
const GO_LIMIT: isize = 4;

// Notepad_plus::setTitle.
pub fn title(full: &str, name: &str, dirty: bool, short: bool) -> String {
    let mark = if dirty { "*" } else { "" };
    let shown = if short { name } else { full };
    format!("{mark}{shown} - Notepad++")
}

// One selection: its character count and its line range (ScintillaEditView::getSelectionLinesRange).
#[derive(Clone, Copy)]
pub struct Sel1 {
    pub chars: usize,
    pub lines: (usize, usize),
}

// The Pos/Sel part of Notepad_plus::updateStatusBar; pos is 1-based.
pub fn sel_status(sels: &[Sel1], rect: bool, pos: usize) -> String {
    let c = commafy;
    let total: usize = sels.iter().map(|s| s.chars).sum();
    match sels {
        [] => format!("Pos: {}", c(pos)),
        [s] if s.chars == 0 => format!("Pos: {}", c(pos)),
        [s] => format!("Sel: {} | {}", c(s.chars), c(s.lines.1 - s.lines.0 + 1)),
        _ if rect => {
            let max = sels.iter().map(|s| s.chars).max().unwrap_or(0);
            let same = sels.iter().all(|s| s.chars == sels[0].chars);
            let op = if same { " = " } else { " -> " };
            format!("Sel: {}x{}{op}{}", c(sels.len()), c(max), c(total))
        }
        _ => {
            let lines = if sels.len() <= 99 {
                let mut v: Vec<(usize, usize)> = sels.iter().map(|s| s.lines).collect();
                v.sort();
                let mut n = 0;
                let mut prev: Option<usize> = None;
                for (a, b) in v {
                    n += b - a;
                    if prev != Some(a) {
                        n += 1;
                    }
                    prev = Some(b);
                }
                c(n)
            } else {
                "...".into()
            };
            format!("Sel {} : {} | {lines}", c(sels.len()), c(total))
        }
    }
}

// GoToLineDlg::getLine: strtoll of the field; an empty field is -1.
pub fn goto_value(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    let t = s.trim_start();
    let (neg, d) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let n = d.bytes().take_while(u8::is_ascii_digit).fold(0i64, |n, b| {
        n.saturating_mul(10).saturating_add((b - b'0') as i64)
    });
    Some(if neg { -n } else { n })
}

// GoToLineDlg::updateLinesNumbers offset mode: the last offset.
pub fn offset_limit(len: usize) -> usize {
    len.saturating_sub(1)
}

fn s(v: &NSView, m: u32, w: usize, l: isize) -> isize {
    sci::send(v, m, w, l)
}

fn line_range(v: &NSView, a: isize, b: isize) -> (usize, usize) {
    let l1 = s(v, SCI_LINEFROMPOSITION, a as usize, 0);
    let mut l2 = s(v, SCI_LINEFROMPOSITION, b as usize, 0);
    if l1 != l2 && s(v, SCI_POSITIONFROMLINE, l2 as usize, 0) == b {
        l2 -= 1;
    }
    (l1.max(0) as usize, l2.max(l1).max(0) as usize)
}

pub fn selection_status(v: &NSView) -> String {
    let n = s(v, SCI_GETSELECTIONS, 0, 0).max(1) as usize;
    let rect = s(v, SCI_SELECTIONISRECTANGLE, 0, 0) != 0;
    let mut sels = vec![];
    for i in 0..n {
        let (a, b) = if n == 1 {
            (
                s(v, SCI_GETSELECTIONSTART, 0, 0),
                s(v, SCI_GETSELECTIONEND, 0, 0),
            )
        } else {
            (
                s(v, SCI_GETSELECTIONNSTART, i, 0),
                s(v, SCI_GETSELECTIONNEND, i, 0),
            )
        };
        let chars = s(v, SCI_COUNTCHARACTERS, a as usize, b).max(0) as usize;
        let lines = if n == 1 || !rect {
            line_range(v, a, b)
        } else {
            (0, 0)
        };
        sels.push(Sel1 { chars, lines });
    }
    let pos = s(v, SCI_GETCURRENTPOS, 0, 0).max(0) as usize + 1;
    sel_status(&sels, rect, pos)
}

// The first menu, breadth first, with an item that sends this action.
fn menu_with(root: &NSMenu, action: Sel) -> Option<Retained<NSMenu>> {
    let mut queue = vec![root.retain()];
    let mut i = 0;
    while i < queue.len() {
        let m = queue[i].clone();
        i += 1;
        for it in m.itemArray().iter() {
            if it.action() == Some(action) && i > 1 {
                return Some(m);
            }
            if let Some(sub) = it.submenu() {
                queue.push(sub);
            }
        }
    }
    None
}

impl App {
    pub(crate) fn update_title(&self) {
        let Some(w) = self.ivars().window.get() else {
            return;
        };
        let Some(t) = self.current().and_then(|i| self.tab(i)) else {
            return;
        };
        let full = t
            .path
            .as_deref()
            .map_or(t.name.clone(), |p| p.to_string_lossy().into_owned());
        let dirty = self.dirty(&t);
        let short = prefs::with(|p| p.short_title);
        w.setTitle(&ns(&title(&full, &t.name, dirty, short)));
        w.setDocumentEdited(dirty);
        let rep = t.path.as_deref().map(|p| p.to_string_lossy().into_owned());
        w.setRepresentedFilename(&ns(&rep.unwrap_or_default()));
    }

    pub(crate) fn apply_status_bar(&self) {
        self.set_status_bar(prefs::with(|p| p.status_bar.0));
    }

    pub(crate) fn set_status_bar(&self, show: bool) {
        let Some(labels) = self.ivars().status.get() else {
            return;
        };
        let Some(content) = self.ivars().window.get().and_then(|w| w.contentView()) else {
            return;
        };
        let h = if show { STATUS_H } else { 0. };
        for l in labels {
            l.setHidden(!show);
        }
        let size = content.bounds().size;
        for v in content.subviews().iter() {
            if labels.iter().any(|l| {
                let lv: &NSView = l;
                std::ptr::eq(lv, &*v)
            }) {
                continue;
            }
            v.setFrame(NSRect::new(
                NSPoint::new(0., h),
                NSSize::new(size.width, size.height - h),
            ));
        }
    }

    pub(crate) fn add_status_clicks(&self) {
        let Some(labels) = self.ivars().status.get() else {
            return;
        };
        let mtm = self.mtm();
        let t: &AnyObject = self;
        let add = |l: &NSTextField, action: Sel, clicks: isize, button: usize| {
            let g = unsafe {
                NSClickGestureRecognizer::initWithTarget_action(
                    NSClickGestureRecognizer::alloc(mtm),
                    Some(t),
                    Some(action),
                )
            };
            g.setNumberOfClicksRequired(clicks);
            g.setButtonMask(button);
            l.addGestureRecognizer(&g);
        };
        for (i, l) in labels.iter().enumerate() {
            l.setTag(i as isize);
            match i as isize {
                TYPING_MODE => add(l, sel!(statusClick:), 1, 1),
                DOC_TYPE | EOF_FORMAT | UNICODE_TYPE => {
                    add(l, sel!(statusDoubleClick:), 2, 1);
                    add(l, sel!(statusRightClick:), 1, 2);
                }
                _ => add(l, sel!(statusDoubleClick:), 2, 1),
            }
        }
    }

    pub(crate) fn status_click(&self, g: &NSClickGestureRecognizer) {
        let Some(l) = g.view() else { return };
        if l.tag() != TYPING_MODE {
            return;
        }
        let Some(v) = self.editor() else { return };
        let ovr = s(&v, SCI_GETOVERTYPE, 0, 0) != 0;
        s(&v, SCI_SETOVERTYPE, !ovr as usize, 0);
        self.update_status();
    }

    pub(crate) fn status_double_click(&self, g: &NSClickGestureRecognizer) {
        let Some(l) = g.view() else { return };
        match l.tag() {
            CUR_POS => self.go_to(),
            DOC_SIZE => self.summary(),
            _ => self.status_menu(g),
        }
    }

    pub(crate) fn status_menu(&self, g: &NSClickGestureRecognizer) {
        let Some(l) = g.view() else { return };
        let action = match l.tag() {
            DOC_TYPE => sel!(setLanguage:),
            EOF_FORMAT => sel!(eolConvert:),
            UNICODE_TYPE => sel!(encodeIn:),
            _ => return,
        };
        let Some(bar) = NSApplication::sharedApplication(self.mtm()).mainMenu() else {
            return;
        };
        let Some(m) = menu_with(&bar, action) else {
            return;
        };
        let m = m.copy();
        m.popUpMenuPositioningItem_atLocation_inView(None, g.locationInView(Some(&l)), Some(&l));
    }

    fn go_to_numbers(&self, a: &NSView) {
        let Some(v) = self.editor() else { return };
        let line = a
            .viewWithTag(GO_LINE)
            .and_then(|b| b.downcast::<NSButton>().ok())
            .is_some_and(|b| b.state() == NSControlStateValueOn);
        let (cur, max) = if line {
            let p = s(&v, SCI_GETCURRENTPOS, 0, 0);
            (
                s(&v, SCI_LINEFROMPOSITION, p as usize, 0) as usize + 1,
                s(&v, SCI_GETLINECOUNT, 0, 0) as usize,
            )
        } else {
            (
                s(&v, SCI_GETCURRENTPOS, 0, 0) as usize,
                offset_limit(s(&v, SCI_GETLENGTH, 0, 0).max(0) as usize),
            )
        };
        for (tag, n) in [(GO_CURRENT, cur), (GO_LIMIT, max)] {
            if let Some(f) = a
                .viewWithTag(tag)
                .and_then(|f| f.downcast::<NSTextField>().ok())
            {
                f.setStringValue(&ns(&n.to_string()));
            }
        }
    }

    pub(crate) fn go_to_mode(&self, b: &NSButton) {
        let Some(a) = (unsafe { b.superview() }) else { return };
        for tag in [GO_LINE, GO_OFFSET] {
            if let Some(r) = a
                .viewWithTag(tag)
                .and_then(|r| r.downcast::<NSButton>().ok())
            {
                r.setState(if tag == b.tag() {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
            }
        }
        self.go_to_numbers(&a);
    }

    // GoToLineDlg as a modal sheet of fields.
    pub(crate) fn go_to(&self) {
        let Some(v) = self.editor() else { return };
        let mtm = self.mtm();
        let t: &AnyObject = self;
        let a = NSAlert::new(mtm);
        a.setMessageText(&ns("Go To..."));
        let acc = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(300., 92.)),
        );
        let at = |c: &NSView, x: f64, y: f64, w: f64| {
            c.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 22.)));
            acc.addSubview(c);
        };
        for (tag, title, x) in [(GO_LINE, "Line", 0.), (GO_OFFSET, "Offset", 120.)] {
            let r = unsafe {
                NSButton::radioButtonWithTitle_target_action(
                    &ns(title),
                    Some(t),
                    Some(sel!(goToMode:)),
                    mtm,
                )
            };
            r.setTag(tag);
            r.setState(if tag == GO_LINE {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            at(&r, x, 70., 110.);
        }
        let label = |text: &str, y: f64| {
            at(&NSTextField::labelWithString(&ns(text), mtm), 0., y, 170.);
        };
        label("You are here:", 46.);
        label("You want to go to:", 22.);
        label("You can't go further than:", 0.);
        let cur = NSTextField::labelWithString(&NSString::new(), mtm);
        cur.setTag(GO_CURRENT);
        cur.setSelectable(true);
        at(&cur, 175., 46., 120.);
        let field = NSTextField::textFieldWithString(&NSString::new(), mtm);
        at(&field, 175., 22., 120.);
        let lim = NSTextField::labelWithString(&NSString::new(), mtm);
        lim.setTag(GO_LIMIT);
        at(&lim, 175., 0., 120.);
        self.go_to_numbers(&acc);
        a.setAccessoryView(Some(&acc));
        a.addButtonWithTitle(&ns("Go"));
        a.addButtonWithTitle(&ns("I'm going nowhere"));
        a.window().setInitialFirstResponder(Some(&field));
        if a.runModal() == NSAlertFirstButtonReturn {
            let line = acc
                .viewWithTag(GO_LINE)
                .and_then(|b| b.downcast::<NSButton>().ok())
                .is_some_and(|b| b.state() == NSControlStateValueOn);
            if let Some(n) = goto_value(&crate::panel::text(&field)) {
                if line {
                    let l = (n - 1).clamp(isize::MIN as i64, isize::MAX as i64) as isize;
                    s(&v, SCI_ENSUREVISIBLE, l as usize, 0);
                    s(&v, SCI_GOTOLINE, l as usize, 0);
                } else {
                    let mut p = 0;
                    if n > 0 {
                        let before =
                            s(&v, SCI_POSITIONBEFORE, n.min(isize::MAX as i64) as usize, 0);
                        p = s(&v, SCI_POSITIONAFTER, before as usize, 0);
                    }
                    let l = s(&v, SCI_LINEFROMPOSITION, p as usize, 0);
                    s(&v, SCI_ENSUREVISIBLE, l as usize, 0);
                    s(&v, SCI_GOTOPOS, p as usize, 0);
                }
            }
        }
        self.focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_format() {
        assert_eq!(
            title("/a/b.txt", "b.txt", false, false),
            "/a/b.txt - Notepad++"
        );
        assert_eq!(
            title("/a/b.txt", "b.txt", true, false),
            "*/a/b.txt - Notepad++"
        );
        assert_eq!(title("/a/b.txt", "b.txt", true, true), "*b.txt - Notepad++");
        assert_eq!(title("new 1", "new 1", false, true), "new 1 - Notepad++");
    }

    #[test]
    fn selection_formats() {
        let s = |chars, a, b| Sel1 {
            chars,
            lines: (a, b),
        };
        assert_eq!(sel_status(&[s(0, 3, 3)], false, 1235), "Pos: 1,235");
        assert_eq!(sel_status(&[s(1500, 0, 9)], false, 1), "Sel: 1,500 | 10");
        assert_eq!(
            sel_status(&[s(3, 0, 0), s(3, 0, 0), s(3, 0, 0)], true, 1),
            "Sel: 3x3 = 9"
        );
        assert_eq!(
            sel_status(&[s(2, 0, 0), s(5, 0, 0)], true, 1),
            "Sel: 2x5 -> 7"
        );
        assert_eq!(
            sel_status(&[s(2, 4, 4), s(3, 1, 2), s(1, 2, 2)], false, 1),
            "Sel 3 : 6 | 3"
        );
        let many = vec![s(1, 0, 0); 100];
        assert_eq!(sel_status(&many, false, 1), "Sel 100 : 100 | ...");
    }

    #[test]
    fn go_to_values() {
        assert_eq!(goto_value(""), None);
        assert_eq!(goto_value("42"), Some(42));
        assert_eq!(goto_value("12ab"), Some(12));
        assert_eq!(goto_value("ab"), Some(0));
        assert_eq!(goto_value("99999999999999999999999"), Some(i64::MAX));
        assert_eq!(offset_limit(0), 0);
        assert_eq!(offset_limit(10), 9);
    }
}
