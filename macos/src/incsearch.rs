// SPDX-License-Identifier: GPL-3.0-or-later
use crate::panel::{on, text};
use crate::search::{self, Doc, Opts};
use crate::{cfg, ns, sci, App, STATUS_H};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSButton, NSColor, NSEventModifierFlags, NSTextField,
    NSView,
};
use objc2_foundation::{NSNotification, NSPoint, NSRect, NSSize};
use std::cell::OnceCell;

const BAR_H: f64 = 32.;
// SCE_UNIVERSAL_FOUND_STYLE_INC, the "Incremental highlight all" style.
const INDIC_INC: usize = 28;
const INDIC_ROUNDBOX: isize = 7;
const SCI_SETSEL: u32 = 2160;
const SCI_INDICSETSTYLE: u32 = 2080;
const SCI_INDICSETFORE: u32 = 2082;
const SCI_SETINDICATORCURRENT: u32 = 2500;
const SCI_INDICATORFILLRANGE: u32 = 2504;
const SCI_INDICATORCLEARRANGE: u32 = 2505;
const SCI_INDICSETUNDER: u32 = 2510;
const SCI_INDICSETALPHA: u32 = 2523;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Found,
    NotFound,
    TopReached,
    EndReached,
}

// Port of FindReplaceDlg::processFindNext for FirstIncremental (typing) and NextIncremental (advance).
pub fn inc_find(
    doc: &Doc,
    find: &str,
    match_case: bool,
    sel: (isize, isize),
    forward: bool,
    advance: bool,
) -> (Option<(isize, isize)>, Status) {
    if find.is_empty() {
        return (None, Status::Found);
    }
    let o = Opts {
        find: find.into(),
        match_case,
        wrap: true,
        ..Default::default()
    };
    let len = doc.len();
    let (s, e) = match (advance, forward) {
        (false, true) => (sel.0, len),
        (false, false) => (sel.1, 0),
        (true, true) => ((sel.0 + 1).min(len), len),
        (true, false) => ((sel.1 - 1).max(0), 0),
    };
    if let Ok(Some(m)) = o.find_in(doc, s, e) {
        return (Some(m), Status::Found);
    }
    let (s, e, st) = if forward {
        (0, len, Status::EndReached)
    } else {
        (len, 0, Status::TopReached)
    };
    match o.find_in(doc, s, e) {
        Ok(Some(m)) => (Some(m), st),
        _ => (None, Status::NotFound),
    }
}

// Port of FindIncrementDlg::setFindStatus.
pub fn inc_status(st: Status, count: usize, nth: usize) -> String {
    let c = if count > 0 {
        format!("{}/{}", search::commafy(nth), search::commafy(count))
    } else {
        String::new()
    };
    let with = |m: &str| {
        if count > 0 {
            format!("{c} - {m}")
        } else {
            m.to_string()
        }
    };
    match st {
        Status::Found => c,
        Status::NotFound => "Phrase not found".into(),
        Status::TopReached => with("Reached top of page, continued from bottom"),
        Status::EndReached => with("Reached end of page, continued from top"),
    }
}

fn all(doc: &Doc, find: &str, match_case: bool) -> Vec<(isize, isize)> {
    let o = Opts {
        find: find.into(),
        match_case,
        wrap: true,
        ..Default::default()
    };
    search::process(doc, &o, false, false, (0, doc.len())).unwrap_or_default()
}

#[derive(Clone, Copy)]
enum Change {
    Text,
    Advance(bool),
    Case,
    Highlight,
}

struct Bar {
    view: Retained<NSView>,
    field: Retained<NSTextField>,
    case: Retained<NSButton>,
    highlight: Retained<NSButton>,
    count: Retained<NSButton>,
    status: Retained<NSTextField>,
    _target: Retained<IncTarget>,
}

thread_local! {
    static BAR: OnceCell<Bar> = const { OnceCell::new() };
}

define_class!(
    // Receives the actions of the Incremental Search bar and the key commands of its text field.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Retained<App>]
    struct IncTarget;

    impl IncTarget {
        #[unsafe(method(controlTextDidChange:))]
        fn text_changed(&self, _n: &NSNotification) {
            self.ivars().inc_update(Change::Text);
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn command(&self, _c: &AnyObject, _t: &AnyObject, cmd: Sel) -> bool {
            if cmd == sel!(cancelOperation:) {
                self.ivars().close_incremental_search();
            } else if cmd == sel!(insertNewline:) {
                let shift = NSApplication::sharedApplication(self.mtm())
                    .currentEvent()
                    .is_some_and(|e| e.modifierFlags().contains(NSEventModifierFlags::Shift));
                self.ivars().inc_update(Change::Advance(!shift));
            }
            cmd == sel!(cancelOperation:) || cmd == sel!(insertNewline:)
        }

        #[unsafe(method(incPrevious:))]
        fn previous(&self, _s: Option<&AnyObject>) {
            self.ivars().inc_update(Change::Advance(false));
        }

        #[unsafe(method(incNext:))]
        fn next(&self, _s: Option<&AnyObject>) {
            self.ivars().inc_update(Change::Advance(true));
        }

        #[unsafe(method(incClose:))]
        fn close(&self, _s: Option<&AnyObject>) {
            self.ivars().close_incremental_search();
        }

        #[unsafe(method(incMatchCase:))]
        fn match_case(&self, _s: Option<&AnyObject>) {
            self.ivars().inc_update(Change::Case);
        }

        #[unsafe(method(incHighlight:))]
        fn highlight(&self, _s: Option<&AnyObject>) {
            self.ivars().inc_update(Change::Highlight);
        }
    }
);

fn place(bar: &NSView, v: &NSView, x: f64, w: f64, h: f64) {
    v.setFrame(NSRect::new(
        NSPoint::new(x, (BAR_H - h) / 2.),
        NSSize::new(w, h),
    ));
    bar.addSubview(v);
}

// IDD_INCREMENT_FIND: close, Find:, the text, <, >, Match case, Highlight all, Count and the status.
fn build(app: &App) -> Bar {
    let mtm = app.mtm();
    let target: Retained<IncTarget> = {
        let this = IncTarget::alloc(mtm).set_ivars(app.retain());
        unsafe { msg_send![super(this), init] }
    };
    let t: &AnyObject = &target;
    let content = app.ivars().window.get().unwrap().contentView().unwrap();
    let view = NSView::new(mtm);
    view.setFrame(NSRect::new(
        NSPoint::new(0., STATUS_H),
        NSSize::new(content.bounds().size.width, BAR_H),
    ));
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMaxYMargin,
    );
    view.setHidden(true);
    content.addSubview(&view);
    let button = |title: &str, action: Sel, x: f64, w: f64| {
        let b = unsafe {
            NSButton::buttonWithTitle_target_action(&ns(title), Some(t), Some(action), mtm)
        };
        place(&view, &b, x, w, 28.);
        b
    };
    let check = |title: &str, action: Option<Sel>, x: f64, w: f64| {
        let b =
            unsafe { NSButton::checkboxWithTitle_target_action(&ns(title), Some(t), action, mtm) };
        place(&view, &b, x, w, 20.);
        b
    };
    button("\u{2715}", sel!(incClose:), 4., 36.);
    let label = NSTextField::labelWithString(&ns("Find:"), mtm);
    place(&view, &label, 44., 36., 18.);
    let field = NSTextField::textFieldWithString(&ns(""), mtm);
    place(&view, &field, 82., 220., 22.);
    let _: () = unsafe { msg_send![&field, setDelegate: t] };
    button("<", sel!(incPrevious:), 306., 36.);
    button(">", sel!(incNext:), 344., 36.);
    let case = check("Match case", Some(sel!(incMatchCase:)), 390., 100.);
    let highlight = check("Highlight all", Some(sel!(incHighlight:)), 494., 104.);
    let count = check("Count", None, 602., 64.);
    crate::panel::set_on(&count, true);
    let status = NSTextField::labelWithString(&ns(""), mtm);
    place(
        &view,
        &status,
        670.,
        (view.frame().size.width - 676.).max(80.),
        18.,
    );
    status.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    Bar {
        view,
        field,
        case,
        highlight,
        count,
        status,
        _target: target,
    }
}

impl App {
    fn with_bar<R>(&self, f: impl FnOnce(&Bar) -> R) -> R {
        BAR.with(|c| f(c.get_or_init(|| build(self))))
    }

    fn set_bar_visible(&self, b: &Bar, show: bool) {
        if b.view.isHidden() != show {
            return;
        }
        b.view.setHidden(!show);
        let split = self.ivars().split.get().unwrap();
        let mut f = split.frame();
        let d = if show { BAR_H } else { -BAR_H };
        f.origin.y += d;
        f.size.height -= d;
        split.setFrame(f);
    }

    // IDM_SEARCH_FINDINCREMENT: show the bar, or search again when its text field has the focus.
    pub(crate) fn show_incremental_search(&self) {
        let again = self.with_bar(|b| !b.view.isHidden() && b.field.currentEditor().is_some());
        if again {
            return self.inc_update(Change::Advance(true));
        }
        self.with_bar(|b| {
            self.set_bar_visible(b, true);
            let w = self.ivars().window.get().unwrap();
            w.makeFirstResponder(Some(&b.field));
            unsafe { b.field.selectText(None) };
        });
    }

    pub(crate) fn close_incremental_search(&self) {
        if let Some(v) = self.editor() {
            clear_highlight(&v);
        }
        self.with_bar(|b| self.set_bar_visible(b, false));
        self.focus();
    }

    // Port of FindIncrementDlg::run_dlgProc WM_COMMAND.
    fn inc_update(&self, ch: Change) {
        let Some(v) = self.editor() else { return };
        self.with_bar(|b| {
            let find = text(&b.field);
            let case = on(&b.case);
            let (search, hilite, update_case, forward, advance) = match ch {
                Change::Text => (true, on(&b.highlight), case, true, false),
                Change::Advance(f) => (true, false, false, f, true),
                Change::Case => (true, true, true, true, false),
                Change::Highlight => (false, true, false, true, false),
            };
            let doc = sci::doc(&v);
            if search {
                let sel = sci::selection(&v);
                let (m, st) = inc_find(&doc, &find, case, sel, forward, advance);
                if let Some(m) = m {
                    sci::select(&v, m);
                }
                let matches = if on(&b.count) && !find.is_empty() {
                    all(&doc, &find, case)
                } else {
                    vec![]
                };
                let nth = m
                    .and_then(|m| matches.iter().position(|&x| x == m))
                    .map_or(0, |i| i + 1);
                b.status
                    .setStringValue(&ns(&inc_status(st, matches.len(), nth)));
                let missing = st == Status::NotFound;
                b.field.setDrawsBackground(true);
                b.field.setBackgroundColor(Some(&*if missing {
                    NSColor::colorWithSRGBRed_green_blue_alpha(1., 0.4, 0.4, 1.)
                } else {
                    NSColor::textBackgroundColor()
                }));
                if update_case && m.is_none() {
                    sci::send(&v, SCI_SETSEL, usize::MAX, sel.0);
                }
            }
            if hilite {
                highlight(&v, &doc, !find.is_empty() && on(&b.highlight), case);
            }
        });
    }
}

fn clear_highlight(v: &NSView) {
    sci::send(v, SCI_SETINDICATORCURRENT, INDIC_INC, 0);
    sci::send(v, SCI_INDICATORCLEARRANGE, 0, sci::length(v));
}

// Port of FindIncrementDlg::markSelectedTextInc: marks each occurrence of the selected text.
fn highlight(v: &NSView, doc: &Doc, enable: bool, match_case: bool) {
    clear_highlight(v);
    let (s, e) = sci::selection(v);
    if !enable || s == e {
        return;
    }
    let find = String::from_utf8_lossy(&doc.range(s, e)).into_owned();
    sci::send(v, SCI_INDICSETSTYLE, INDIC_INC, INDIC_ROUNDBOX);
    sci::send(v, SCI_INDICSETALPHA, INDIC_INC, 100);
    sci::send(v, SCI_INDICSETUNDER, INDIC_INC, 1);
    let colour = cfg()
        .global_styles
        .iter()
        .find(|s| s.id == INDIC_INC)
        .and_then(|s| s.bg);
    sci::send(v, SCI_INDICSETFORE, INDIC_INC, colour.unwrap_or(0xFF0000));
    sci::send(v, SCI_SETINDICATORCURRENT, INDIC_INC, 0);
    for (s, e) in all(doc, &find, match_case) {
        if e > s {
            sci::send(v, SCI_INDICATORFILLRANGE, s as usize, e - s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_advancing() {
        let d = Doc::new(b"Foo foo xfoo").unwrap();
        assert_eq!(
            inc_find(&d, "fo", false, (0, 0), true, false),
            (Some((0, 2)), Status::Found)
        );
        assert_eq!(
            inc_find(&d, "foo", false, (0, 2), true, false),
            (Some((0, 3)), Status::Found)
        );
        assert_eq!(
            inc_find(&d, "foo", true, (0, 3), true, false),
            (Some((4, 7)), Status::Found)
        );
        assert_eq!(
            inc_find(&d, "foo", false, (0, 3), true, true),
            (Some((4, 7)), Status::Found)
        );
        assert_eq!(
            inc_find(&d, "foo", false, (9, 12), true, true),
            (Some((0, 3)), Status::EndReached)
        );
        assert_eq!(
            inc_find(&d, "foo", false, (4, 7), false, true),
            (Some((0, 3)), Status::Found)
        );
        assert_eq!(
            inc_find(&d, "foo", false, (0, 3), false, true),
            (Some((9, 12)), Status::TopReached)
        );
        assert_eq!(
            inc_find(&d, "bar", false, (0, 0), true, false),
            (None, Status::NotFound)
        );
        assert_eq!(
            inc_find(&d, "", false, (0, 0), true, false),
            (None, Status::Found)
        );
        assert_eq!(all(&d, "foo", false).len(), 3);
        assert_eq!(all(&d, "foo", true).len(), 2);
    }

    #[test]
    fn status_text() {
        assert_eq!(inc_status(Status::Found, 3, 2), "2/3");
        assert_eq!(inc_status(Status::Found, 0, 0), "");
        assert_eq!(inc_status(Status::NotFound, 0, 0), "Phrase not found");
        assert_eq!(
            inc_status(Status::EndReached, 1234, 1),
            "1/1,234 - Reached end of page, continued from top"
        );
        assert_eq!(
            inc_status(Status::TopReached, 0, 0),
            "Reached top of page, continued from bottom"
        );
    }
}
