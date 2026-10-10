// SPDX-License-Identifier: GPL-3.0-or-later
use crate::search_extras::{icon_bytes, BOOKMARK_MARGIN};
use crate::{docking, sci, App, STATUS_H};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, DefinedClass};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSControlStateValueOff, NSControlStateValueOn,
    NSFloatingWindowLevel, NSMenuItem, NSNormalWindowLevel, NSTabViewItem, NSTabViewType, NSView,
    NSWindowStyleMask, NSWindowTitleVisibility,
};
use std::cell::{Cell, RefCell};
use std::time::SystemTime;

// ScintillaEditView.h MARK_HIDELINESBEGIN and MARK_HIDELINESEND.
pub const BEGIN: u32 = 19;
pub const END: u32 = 18;
const B: u32 = 1 << BEGIN;
const E: u32 = 1 << END;

const SCI_MARKERDEFINERGBAIMAGE: u32 = 2626;
const SCI_RGBAIMAGESETWIDTH: u32 = 2624;
const SCI_RGBAIMAGESETHEIGHT: u32 = 2625;
const SCI_SETMARGINMASKN: u32 = 2244;
const SCI_GETMARGINMASKN: u32 = 2245;
const SCI_SETELEMENTCOLOUR: u32 = 2753;
const SC_ELEMENT_HIDDEN_LINE: usize = 81;
const SCI_MARKERGET: u32 = 2046;
const SCI_MARKERADD: u32 = 2043;
const SCI_MARKERDELETE: u32 = 2044;
const SCI_SHOWLINES: u32 = 2226;
const SCI_HIDELINES: u32 = 2227;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_GETSELECTIONSTART: u32 = 2143;
const SCI_GETSELECTIONEND: u32 = 2145;
const SCI_GETFOLDLEVEL: u32 = 2223;
const SCI_GETFOLDEXPANDED: u32 = 2230;
const SCI_SETMARGINLEFT: u32 = 2155;
const SCI_SETMARGINRIGHT: u32 = 2157;
const SCI_DOCUMENTEND: u32 = 2318;
const SC_FOLDLEVELHEADERFLAG: isize = 0x2000;

// The marker and line calls of the hide lines code, so that tests can run it without Scintilla.
pub trait Lines {
    fn count(&self) -> isize;
    fn marks(&self, line: isize) -> u32;
    fn del(&mut self, line: isize, marker: u32);
    fn add(&mut self, line: isize, marker: u32);
    fn hide(&mut self, from: isize, to: isize);
    fn show(&mut self, from: isize, to: isize);
    // A fold header line that is not expanded (ScintillaEditView::isFolded is false).
    fn collapsed_header(&self, line: isize) -> bool;
}

struct Scope {
    n: i32,
    open: bool,
}

fn remove(d: &mut impl Lines, s: &mut Scope, line: isize, mask: u32) {
    let state = d.marks(line) & mask;
    if state & E != 0 {
        d.del(line, END);
        s.open = false;
        s.n -= 1;
    }
    if state & B != 0 {
        d.del(line, BEGIN);
        s.open = true;
        s.n += 1;
    }
}

// Port of ScintillaEditView::hideLines for the selected lines; returns the line of the begin marker.
pub fn hide_lines(d: &mut impl Lines, first: isize, last: isize) -> Option<isize> {
    let n = d.count();
    if n < 3 {
        return None;
    }
    let start = if first == 0 { 1 } else { first };
    let end = if last == n - 1 { last - 1 } else { last };
    if start > end {
        return None;
    }
    let s = &mut Scope { n: 0, open: false };
    let (mut sm, mut em) = (start - 1, end + 1);
    remove(d, s, sm, B);
    for i in start..=end {
        remove(d, s, i, B | E);
    }
    remove(d, s, em, E);
    if s.n == 0 && s.open {
        while s.n == 0 && sm >= 0 {
            sm -= 1;
            remove(d, s, sm, B);
        }
        while s.n != 0 && em < n {
            em += 1;
            remove(d, s, em, E);
        }
    } else {
        while s.n < 0 && sm >= 0 {
            sm -= 1;
            remove(d, s, sm, B);
        }
        while s.n > 0 && em < n {
            em += 1;
            remove(d, s, em, E);
        }
    }
    d.add(sm, BEGIN);
    d.add(em, END);
    hide_marked(d, sm, false);
    Some(sm)
}

// Port of ScintillaEditView::hideMarkedLines.
pub fn hide_marked(d: &mut impl Lines, from: isize, to_end: bool) {
    let mut start = from;
    let mut inside = false;
    for i in from.max(0)..d.count() {
        let state = d.marks(i);
        if state & E != 0 {
            if inside {
                d.hide(start, i - 1);
                if !to_end {
                    return;
                }
            }
            inside = false;
        }
        if state & B != 0 {
            inside = true;
            start = i + 1;
        }
    }
}

// Port of ScintillaEditView::showHiddenLines.
pub fn show_hidden(d: &mut impl Lines, from: isize, to_end: bool, delete: bool) {
    let mut start = from;
    let mut inside = false;
    let mut i = from.max(0);
    while i < d.count() {
        let state = d.marks(i);
        if state & B != 0 && !inside {
            inside = true;
            if delete {
                d.del(i, BEGIN);
            } else {
                start = i + 1;
            }
        } else if state & E != 0 {
            if delete {
                d.del(i, END);
                if !to_end {
                    return;
                }
                inside = false;
            } else if inside {
                if start >= i {
                    if !to_end {
                        return;
                    }
                    inside = false;
                    i += 1;
                    continue;
                }
                d.show(start, i - 1);
                if !to_end {
                    return;
                }
                inside = false;
            }
        }
        if inside && d.collapsed_header(i) {
            d.show(start, i);
        }
        i += 1;
    }
}

// Buffer::setHideLineChanged(false, line): shows the section, then removes its markers.
fn show_section(d: &mut impl Lines, line: isize) {
    show_hidden(d, line, false, false);
    show_hidden(d, line, false, true);
}

// Port of ScintillaEditView::hidelineMarkerClicked; false when the line has no hide lines marker.
pub fn marker_clicked(d: &mut impl Lines, line: isize) -> bool {
    let state = d.marks(line);
    let (open, close) = (state & B != 0, state & E != 0);
    if !open && !close {
        return false;
    }
    if open {
        show_section(d, line);
        return true;
    }
    let mut i = line - 1;
    let mut found = false;
    while i >= 0 && !found {
        found = d.marks(i) & B != 0;
        i -= 1;
    }
    if found {
        show_section(d, i + 1);
    } else {
        d.del(line, END);
    }
    true
}

struct Sci<'a>(&'a NSView);

impl Lines for Sci<'_> {
    fn count(&self) -> isize {
        sci::send(self.0, SCI_GETLINECOUNT, 0, 0)
    }
    fn marks(&self, line: isize) -> u32 {
        if line < 0 {
            return 0;
        }
        sci::send(self.0, SCI_MARKERGET, line as usize, 0) as u32
    }
    fn del(&mut self, line: isize, marker: u32) {
        if line >= 0 {
            sci::send(self.0, SCI_MARKERDELETE, line as usize, marker as isize);
        }
    }
    fn add(&mut self, line: isize, marker: u32) {
        if line >= 0 {
            sci::send(self.0, SCI_MARKERADD, line as usize, marker as isize);
        }
    }
    fn hide(&mut self, from: isize, to: isize) {
        sci::send(self.0, SCI_HIDELINES, from.max(0) as usize, to);
    }
    fn show(&mut self, from: isize, to: isize) {
        sci::send(self.0, SCI_SHOWLINES, from.max(0) as usize, to);
    }
    fn collapsed_header(&self, line: isize) -> bool {
        let level = sci::send(self.0, SCI_GETFOLDLEVEL, line as usize, 0);
        level & SC_FOLDLEVELHEADERFLAG != 0
            && sci::send(self.0, SCI_GETFOLDEXPANDED, line as usize, 0) == 0
    }
}

// The hide lines markers in the bookmark margin, with the Notepad++ icons and the green hidden line colour.
pub fn setup_hide_lines(v: &NSView) {
    const ICONS: &str = include_str!("../../PowerEditor/src/rgba_icons.h");
    let m = BOOKMARK_MARGIN;
    let mask = sci::send(v, SCI_GETMARGINMASKN, m, 0);
    sci::send(v, SCI_SETMARGINMASKN, m, mask | B as isize | E as isize);
    sci::send(v, SCI_RGBAIMAGESETWIDTH, 14, 0);
    sci::send(v, SCI_RGBAIMAGESETHEIGHT, 14, 0);
    for (marker, name) in [(BEGIN, "hidelines_begin14"), (END, "hidelines_end14")] {
        let icon = icon_bytes(ICONS, name);
        if icon.len() == 14 * 14 * 4 {
            sci::send(
                v,
                SCI_MARKERDEFINERGBAIMAGE,
                marker as usize,
                icon.as_ptr() as isize,
            );
        }
    }
    sci::send(v, SCI_SETELEMENTCOLOUR, SC_ELEMENT_HIDDEN_LINE, 0xFF77CC77);
}

// A click on a hide lines marker in the bookmark margin shows the lines again.
pub fn hide_marker_clicked(v: &NSView, line: isize) -> bool {
    marker_clicked(&mut Sci(v), line)
}

struct PostIt {
    on_top: bool,
    tabs: NSTabViewType,
    transparent: bool,
    title: NSWindowTitleVisibility,
}

#[derive(Default)]
struct Monitor {
    item: Option<Retained<NSTabViewItem>>,
    stamp: Option<(SystemTime, u64)>,
    prior_ro: bool,
}

thread_local! {
    static POST_IT: RefCell<Option<PostIt>> = const { RefCell::new(None) };
    static DISTRACTION_FREE: RefCell<Option<Vec<isize>>> = const { RefCell::new(None) };
    static MONITORED: RefCell<Vec<Monitor>> = const { RefCell::new(vec![]) };
    static TICKING: Cell<bool> = const { Cell::new(false) };
}

fn stamp(p: &std::path::Path) -> Option<Option<(SystemTime, u64)>> {
    let m = std::fs::metadata(p).ok()?;
    Some(m.modified().ok().map(|t| (t, m.len())))
}

fn state(on: bool) -> objc2_app_kit::NSControlStateValue {
    if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    }
}

impl App {
    // Called for each new editor.
    pub(crate) fn setup_extras(&self, v: &NSView) {
        setup_hide_lines(v);
        self.context_menu_setup(v);
        if DISTRACTION_FREE.with(|d| d.borrow().is_some()) {
            self.set_padding(v, self.distraction_free_padding());
        }
    }

    // NppCommands.cpp IDM_VIEW_HIDELINES.
    pub(crate) fn hide_lines(&self) {
        let Some(v) = self.editor() else { return };
        let line = |msg| {
            let pos = sci::send(&v, msg, 0, 0);
            sci::send(&v, sci::SCI_LINEFROMPOSITION, pos as usize, 0)
        };
        let (first, last) = (line(SCI_GETSELECTIONSTART), line(SCI_GETSELECTIONEND));
        hide_lines(&mut Sci(&v), first, last);
    }

    pub(crate) fn monitored(&self, item: &NSTabViewItem) -> bool {
        MONITORED.with(|m| {
            m.borrow()
                .iter()
                .any(|x| x.item.as_deref().is_some_and(|i| std::ptr::eq(i, item)))
        })
    }

    fn set_user_read_only(&self, i: usize, on: bool) {
        let view = {
            let mut tabs = self.ivars().tabs.borrow_mut();
            let Some(t) = tabs.get_mut(i) else { return };
            t.ro = on;
            (t.view.clone(), t.read_only())
        };
        let (view, ro) = view;
        sci::set_read_only(&view, ro || self.ivars().replacing.get());
        self.refresh_title(i);
    }

    // NppCommands.cpp IDM_VIEW_MONITORING and Notepad_plus::monitoringStartOrStopAndUpdateUI.
    pub(crate) fn monitoring(&self) {
        let Some(i) = self.current() else { return };
        let Some(t) = self.tab(i) else { return };
        if self.monitored(&t.item) {
            let prior = MONITORED.with(|m| {
                let mut m = m.borrow_mut();
                let k = m.iter().position(|x| {
                    x.item
                        .as_deref()
                        .is_some_and(|it| std::ptr::eq(it, &*t.item))
                })?;
                Some(m.remove(k).prior_ro)
            });
            self.set_user_read_only(i, prior.unwrap_or(false));
            return;
        }
        let Some(st) = t.path.as_deref().and_then(stamp) else {
            self.alert(
                "Monitoring problem",
                "The file should exist to be monitored.",
                &["OK"],
            );
            return;
        };
        if self.dirty(&t) {
            self.alert(
                "Monitoring problem",
                "The document is dirty. Please save the modification before monitoring it.",
                &["OK"],
            );
            return;
        }
        MONITORED.with(|m| {
            m.borrow_mut().push(Monitor {
                item: Some(t.item.clone()),
                stamp: st,
                prior_ro: t.ro,
            })
        });
        self.set_user_read_only(i, true);
        self.schedule_monitor();
    }

    fn schedule_monitor(&self) {
        if TICKING.with(|t| t.replace(true)) {
            return;
        }
        let _: () = unsafe {
            msg_send![self, performSelector: sel!(monitorTick:), withObject: None::<&AnyObject>, afterDelay: 0.25f64]
        };
    }

    // The monitoring thread of NppIO.cpp checks every 250 ms; a changed file reloads and goes to the end.
    pub(crate) fn monitor_tick(&self) {
        TICKING.with(|t| t.set(false));
        let list = MONITORED.with(|m| std::mem::take(&mut *m.borrow_mut()));
        let mut keep = vec![];
        for mut m in list {
            let Some(item) = m.item.clone() else { continue };
            let i = self
                .ivars()
                .tabs
                .borrow()
                .iter()
                .position(|t| std::ptr::eq(&*t.item, &*item));
            let Some(t) = i.and_then(|i| self.tab(i)) else {
                continue;
            };
            let (Some(i), Some(p)) = (i, t.path.clone()) else {
                continue;
            };
            let Some(now) = stamp(&p) else {
                self.set_user_read_only(i, m.prior_ro);
                continue;
            };
            if now != m.stamp {
                if let Ok(b) = std::fs::read(&p) {
                    let e = match t.enc {
                        crate::Enc::Cp(_) => t.enc,
                        _ => crate::encoding::detect(&b),
                    };
                    self.load_into(i, &b, e);
                    sci::send(&t.view, SCI_DOCUMENTEND, 0, 0);
                }
                m.stamp = now;
            }
            keep.push(m);
        }
        let more = MONITORED.with(|m| {
            let mut m = m.borrow_mut();
            keep.extend(m.drain(..));
            *m = keep;
            !m.is_empty()
        });
        if more {
            self.schedule_monitor();
        }
    }

    fn post_it_on(&self) -> bool {
        POST_IT.with(|p| p.borrow().is_some())
    }

    fn distraction_free_on(&self) -> bool {
        DISTRACTION_FREE.with(|d| d.borrow().is_some())
    }

    // The status bar labels hide, and the view above them takes their place.
    fn show_status_bar(&self, show: bool) {
        let Some(content) = self.ivars().window.get().and_then(|w| w.contentView()) else {
            return;
        };
        let labels = self.ivars().status.get().cloned().unwrap_or_default();
        let d = if show { STATUS_H } else { -STATUS_H };
        for v in content.subviews().iter() {
            if labels.iter().any(|l| std::ptr::eq::<NSView>(&****l, &*v)) {
                v.setHidden(!show);
                continue;
            }
            if v.isHidden() {
                continue;
            }
            let mut f = v.frame();
            f.origin.y += d;
            if v.autoresizingMask()
                .contains(NSAutoresizingMaskOptions::ViewHeightSizable)
            {
                f.size.height -= d;
            }
            v.setFrame(f);
        }
    }

    // Notepad_plus::postItToggle: no tab bar, no status bar, no title, and always on top.
    fn post_it_toggle(&self) {
        let Some(w) = self.ivars().window.get() else {
            return;
        };
        let tv = self.doc_tabs(0);
        match POST_IT.with(|p| p.borrow_mut().take()) {
            None => {
                let saved = PostIt {
                    on_top: w.level() == NSFloatingWindowLevel,
                    tabs: tv.tabViewType(),
                    transparent: w.titlebarAppearsTransparent(),
                    title: w.titleVisibility(),
                };
                w.setLevel(NSFloatingWindowLevel);
                self.doc_tabs(1).setTabViewType(NSTabViewType::NoTabsNoBorder);
                tv.setTabViewType(NSTabViewType::NoTabsNoBorder);
                w.setTitlebarAppearsTransparent(true);
                w.setTitleVisibility(NSWindowTitleVisibility::Hidden);
                self.show_status_bar(false);
                crate::toolbar::hide_without_saving(true);
                POST_IT.with(|p| *p.borrow_mut() = Some(saved));
            }
            Some(saved) => {
                w.setLevel(if saved.on_top {
                    NSFloatingWindowLevel
                } else {
                    NSNormalWindowLevel
                });
                self.doc_tabs(1).setTabViewType(saved.tabs);
                tv.setTabViewType(saved.tabs);
                w.setTitlebarAppearsTransparent(saved.transparent);
                w.setTitleVisibility(saved.title);
                self.show_status_bar(true);
                crate::toolbar::hide_without_saving(false);
                self.tab_bar_menu_for(self.doc_tabs(0));
                self.tab_bar_menu_for(self.doc_tabs(1));
            }
        }
        self.focus();
    }

    // NppCommands.cpp IDM_VIEW_POSTIT: not in Distraction Free mode.
    pub(crate) fn post_it(&self) {
        if !self.distraction_free_on() {
            self.post_it_toggle();
        }
    }

    // ScintillaViewParams::getDistractionFreePadding with the default of 4 parts.
    fn distraction_free_padding(&self) -> isize {
        let w = self.ivars().window.get();
        let width = w
            .and_then(|w| w.screen())
            .map(|s| s.frame().size.width)
            .or(w.map(|w| w.frame().size.width))
            .unwrap_or(0.);
        (width / 4.) as isize
    }

    fn set_padding(&self, v: &NSView, px: isize) {
        sci::send(v, SCI_SETMARGINLEFT, 0, px);
        sci::send(v, SCI_SETMARGINRIGHT, 0, px);
    }

    fn full_screen_on(&self) -> bool {
        self.ivars()
            .window
            .get()
            .is_some_and(|w| w.styleMask().contains(NSWindowStyleMask::FullScreen))
    }

    // Notepad_plus::distractionFreeToggle: full screen, Post-It, no panels, and a centred text column.
    pub(crate) fn distraction_free(&self) {
        let Some(w) = self.ivars().window.get() else {
            return;
        };
        let fs = self.full_screen_on();
        let panels = DISTRACTION_FREE.with(|d| d.borrow_mut().take());
        let px = match panels {
            None => {
                if fs || self.post_it_on() {
                    return;
                }
                w.toggleFullScreen(None);
                self.post_it_toggle();
                w.setLevel(NSNormalWindowLevel);
                let shown: Vec<isize> = [docking::LEFT, docking::RIGHT]
                    .into_iter()
                    .filter(|&s| self.panel_visible(s))
                    .collect();
                shown.iter().for_each(|&s| self.toggle_panel(s));
                DISTRACTION_FREE.with(|d| *d.borrow_mut() = Some(shown));
                self.distraction_free_padding()
            }
            Some(shown) => {
                if fs {
                    w.toggleFullScreen(None);
                }
                if self.post_it_on() {
                    self.post_it_toggle();
                }
                shown
                    .into_iter()
                    .filter(|&s| !self.panel_visible(s))
                    .for_each(|s| self.toggle_panel(s));
                0
            }
        };
        let tabs: Vec<Retained<NSView>> = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .map(|t| t.view.clone())
            .collect();
        tabs.iter().for_each(|v| self.set_padding(v, px));
    }

    // Checkmarks and enable state of the items of this module; None for other items.
    pub(crate) fn validate_extras(&self, item: &NSMenuItem) -> Option<bool> {
        let a = item.action()?;
        let tab = self.current().and_then(|i| self.tab(i));
        let watched = tab.as_ref().is_some_and(|t| self.monitored(&t.item));
        let df = self.distraction_free_on();
        let (on, enabled) = if a == sel!(monitoring:) {
            (watched, tab.as_ref().is_some_and(|t| t.path.is_some()))
        } else if a == sel!(toggleReadOnly:) && watched {
            (true, false)
        } else if a == sel!(postIt:) {
            (self.post_it_on(), !df)
        } else if a == sel!(distractionFree:) {
            (df, true)
        } else if a == sel!(fullScreen:) && df {
            (false, false)
        } else if a == sel!(hideLines:) {
            (false, tab.is_some())
        } else {
            return None;
        };
        item.setState(state(on));
        Some(enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Mock {
        marks: Vec<u32>,
        hidden: Vec<bool>,
    }

    impl Mock {
        fn new(n: usize) -> Mock {
            Mock {
                marks: vec![0; n],
                hidden: vec![false; n],
            }
        }
        fn at(&self, m: u32) -> Vec<isize> {
            (0..self.marks.len() as isize)
                .filter(|&i| self.marks[i as usize] & (1 << m) != 0)
                .collect()
        }
        fn hidden(&self) -> Vec<isize> {
            (0..self.hidden.len() as isize)
                .filter(|&i| self.hidden[i as usize])
                .collect()
        }
    }

    impl Lines for Mock {
        fn count(&self) -> isize {
            self.marks.len() as isize
        }
        fn marks(&self, line: isize) -> u32 {
            usize::try_from(line)
                .ok()
                .and_then(|l| self.marks.get(l).copied())
                .unwrap_or(0)
        }
        fn del(&mut self, line: isize, m: u32) {
            if let Some(x) = usize::try_from(line)
                .ok()
                .and_then(|l| self.marks.get_mut(l))
            {
                *x &= !(1 << m);
            }
        }
        fn add(&mut self, line: isize, m: u32) {
            if let Some(x) = usize::try_from(line)
                .ok()
                .and_then(|l| self.marks.get_mut(l))
            {
                *x |= 1 << m;
            }
        }
        fn hide(&mut self, from: isize, to: isize) {
            (from..=to).for_each(|i| self.hidden[i as usize] = true);
        }
        fn show(&mut self, from: isize, to: isize) {
            (from..=to).for_each(|i| self.hidden[i as usize] = false);
        }
        fn collapsed_header(&self, _line: isize) -> bool {
            false
        }
    }

    #[test]
    fn hide_selected_lines() {
        let mut d = Mock::new(10);
        assert_eq!(hide_lines(&mut d, 2, 4), Some(1));
        assert_eq!((d.at(BEGIN), d.at(END)), (vec![1], vec![5]));
        assert_eq!(d.hidden(), vec![2, 3, 4]);
    }

    #[test]
    fn first_and_last_lines_stay() {
        let mut d = Mock::new(10);
        assert_eq!(hide_lines(&mut d, 0, 9), Some(0));
        assert_eq!((d.at(BEGIN), d.at(END)), (vec![0], vec![9]));
        assert_eq!(d.hidden(), (1..=8).collect::<Vec<_>>());
        let mut d = Mock::new(2);
        assert_eq!(hide_lines(&mut d, 0, 1), None);
        let mut d = Mock::new(5);
        assert_eq!(hide_lines(&mut d, 0, 0), None);
        assert!(d.at(BEGIN).is_empty());
    }

    #[test]
    fn next_sections_merge() {
        let mut d = Mock::new(12);
        hide_lines(&mut d, 2, 3);
        assert_eq!(hide_lines(&mut d, 4, 5), Some(1));
        assert_eq!((d.at(BEGIN), d.at(END)), (vec![1], vec![6]));
        assert_eq!(d.hidden(), vec![2, 3, 4, 5]);
        hide_lines(&mut d, 9, 9);
        assert_eq!((d.at(BEGIN), d.at(END)), (vec![1, 8], vec![6, 10]));
        hide_lines(&mut d, 7, 7);
        assert_eq!((d.at(BEGIN), d.at(END)), (vec![1, 6, 8], vec![6, 8, 10]));
        assert_eq!(d.hidden(), vec![2, 3, 4, 5, 7, 9]);
    }

    #[test]
    fn selection_over_two_sections_merges_them() {
        let mut d = Mock::new(12);
        hide_lines(&mut d, 2, 3);
        hide_lines(&mut d, 7, 8);
        assert_eq!((d.at(BEGIN), d.at(END)), (vec![1, 6], vec![4, 9]));
        assert_eq!(hide_lines(&mut d, 3, 7), Some(1));
        assert_eq!((d.at(BEGIN), d.at(END)), (vec![1], vec![9]));
        assert_eq!(d.hidden(), (2..=8).collect::<Vec<_>>());
    }

    #[test]
    fn marker_click_shows_lines() {
        let mut d = Mock::new(10);
        hide_lines(&mut d, 2, 4);
        hide_lines(&mut d, 7, 7);
        assert!(!marker_clicked(&mut d, 3));
        assert!(marker_clicked(&mut d, 1));
        assert_eq!((d.at(BEGIN), d.at(END)), (vec![6], vec![8]));
        assert_eq!(d.hidden(), vec![7]);
        assert!(marker_clicked(&mut d, 8));
        assert!(d.at(BEGIN).is_empty() && d.at(END).is_empty() && d.hidden().is_empty());
        d.add(4, END);
        assert!(marker_clicked(&mut d, 4));
        assert!(d.at(END).is_empty());
    }
}
