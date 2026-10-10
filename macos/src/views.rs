// SPDX-License-Identifier: GPL-3.0-or-later
use crate::session::{restore_position, Session};
use crate::tabbar::{TabBar, BAR_H};
use crate::{fileops, item, language, nested, prefs, sci, tagged, App, Tab};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAccessibilityTabGroupRole, NSControlStateValueOff, NSControlStateValueOn,
    NSEventModifierFlags, NSMenuItem, NSSplitView, NSSplitViewDividerStyle, NSTabView,
    NSTabViewItem, NSTabViewType, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSArray, NSRect, NSString};
use std::cell::{Cell, OnceCell};
use std::ffi::c_void;
use std::ops::Range;
use std::path::Path;

pub const MAIN: usize = 0;
pub const SUB: usize = 1;

const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GETANCHOR: u32 = 2009;
const SCI_GETFIRSTVISIBLELINE: u32 = 2152;
const SCI_SETSEL: u32 = 2160;
const SCI_LINESCROLL: u32 = 2168;
const SCI_GETDIRECTPOINTER: u32 = 2185;
const SCI_TEXTWIDTH: u32 = 2276;
const SCI_GETDOCPOINTER: u32 = 2357;
const SCI_SETDOCPOINTER: u32 = 2358;
const SCI_SETXOFFSET: u32 = 2397;
const SCI_GETXOFFSET: u32 = 2398;
const SCI_SETFIRSTVISIBLELINE: u32 = 2613;
const STYLE_DEFAULT: usize = 32;
const SCN_FOCUSIN: u32 = 2028;
const F8: &str = "\u{F70B}";

// Notepad++ _mainDocTab/_subDocTab state: the sub tab view, the split that holds both, and the active view.
#[derive(Default)]
pub struct Views {
    sub: OnceCell<Retained<NSTabView>>,
    split: OnceCell<Retained<NSSplitView>>,
    active: Cell<usize>,
    swapped: Cell<bool>,
    sync: Cell<[bool; 2]>,
    offset: Cell<(isize, isize)>,
}

// SCNotification up to the nmhdr fields.
#[repr(C)]
struct Header {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
}

#[derive(Default)]
pub struct DocIvars {
    bar: OnceCell<Retained<TabBar>>,
}

define_class!(
    // A tab view that sends a select or remove call to the tab view that holds the item; its own tab bar replaces the AppKit tabs.
    #[unsafe(super(NSTabView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DocIvars]
    pub struct DocTabs;

    impl DocTabs {
        #[unsafe(method(selectTabViewItem:))]
        fn select_item(&self, item: Option<&NSTabViewItem>) {
            let me: &NSTabView = self;
            match item.and_then(|i| i.tabView(self.mtm())) {
                Some(o) if !std::ptr::eq(&*o, me) => o.selectTabViewItem(item),
                _ => {
                    let same = item.is_some_and(|i| {
                        me.selectedTabViewItem().is_some_and(|s| std::ptr::eq(&*s, i))
                    });
                    let _: () = unsafe { msg_send![super(self), selectTabViewItem: item] };
                    if let (true, Some(d)) = (same, me.delegate()) {
                        let _: () = unsafe { msg_send![&*d, tabView: me, didSelectTabViewItem: item] };
                    }
                    self.bar_changed();
                }
            }
        }

        #[unsafe(method(insertTabViewItem:atIndex:))]
        fn insert_item(&self, item: &NSTabViewItem, at: isize) {
            let _: () = unsafe { msg_send![super(self), insertTabViewItem: item, atIndex: at] };
            self.bar_changed();
        }

        #[unsafe(method(contentRect))]
        fn content_rect(&self) -> NSRect {
            let mut r = self.bounds();
            if self.ivars().bar.get().is_some_and(|b| !b.isHidden()) {
                r.size.height = (r.size.height - BAR_H).max(0.);
                if self.isFlipped() {
                    r.origin.y += BAR_H;
                }
            }
            r
        }

        // Post-It sets NoTabsNoBorder to hide the tabs: that hides the bar, and any other type shows it.
        #[unsafe(method(setTabViewType:))]
        fn set_tab_view_type(&self, t: NSTabViewType) {
            let hide = t == NSTabViewType::NoTabsNoBorder;
            POST_IT.with(|p| p.set(hide));
            self.show_bar();
        }

        #[unsafe(method_id(accessibilityRole))]
        fn ax_role(&self) -> Option<Retained<NSString>> {
            Some(unsafe { NSAccessibilityTabGroupRole }.retain())
        }

        #[unsafe(method_id(accessibilityChildren))]
        fn ax_children(&self) -> Option<Retained<NSArray>> {
            let sup: Option<Retained<NSArray>> = unsafe { msg_send![super(self), accessibilityChildren] };
            let mut v = self.ivars().bar.get().map(|b| b.ax_tabs(self)).unwrap_or_default();
            if let Some(sup) = sup {
                v.extend(sup.iter());
            }
            Some(NSArray::from_retained_slice(&v))
        }

        #[unsafe(method(removeTabViewItem:))]
        fn remove_item(&self, item: &NSTabViewItem) {
            let me: &NSTabView = self;
            match item.tabView(self.mtm()) {
                Some(o) if !std::ptr::eq(&*o, me) => o.removeTabViewItem(item),
                Some(_) => {
                    let _: () = unsafe { msg_send![super(self), removeTabViewItem: item] };
                    self.bar_changed();
                }
                None => {}
            }
        }
    }
);

thread_local! {
    static POST_IT: Cell<bool> = const { Cell::new(false) };
}

impl DocTabs {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DocIvars::default());
        let tv: Retained<Self> = unsafe { msg_send![super(this), init] };
        let _: () =
            unsafe { msg_send![super(&*tv), setTabViewType: NSTabViewType::NoTabsNoBorder] };
        let bar = TabBar::new(mtm);
        bar.place(&tv);
        tv.addSubview(&bar);
        let _ = tv.ivars().bar.set(bar);
        tv.show_bar();
        tv
    }

    fn bar_changed(&self) {
        if let Some(b) = self.ivars().bar.get() {
            b.setNeedsDisplay(true);
        }
    }

    // Shows the bar unless Post-It or the "hide" option is on, and fits the selected editor below it.
    pub fn show_bar(&self) {
        let Some(b) = self.ivars().bar.get() else {
            return;
        };
        b.setHidden(POST_IT.with(Cell::get) || prefs::get().tab_hide);
        b.place(self);
        if let Some(v) = self.selectedTabViewItem().and_then(|i| i.view(self.mtm())) {
            v.setFrame(self.contentRect());
        }
        b.setNeedsDisplay(true);
    }
}

pub fn bar_of(tv: &NSTabView) -> Option<&TabBar> {
    tv.downcast_ref::<DocTabs>()?
        .ivars()
        .bar
        .get()
        .map(|b| &**b)
}

// The tab indices of view `p` when the main view tabs come first.
pub fn range(p: usize, n_main: usize, n: usize) -> Range<usize> {
    if p == MAIN {
        0..n_main.min(n)
    } else {
        n_main.min(n)..n
    }
}

pub fn pane_at(i: usize, n_main: usize) -> usize {
    usize::from(i >= n_main)
}

// The other tabs that show the same document.
pub fn peers(docs: &[isize], i: usize) -> Vec<usize> {
    let Some(&d) = docs.get(i) else { return vec![] };
    (0..docs.len())
        .filter(|&k| k != i && docs[k] == d)
        .collect()
}

// NppIO.cpp fileClose and fileCloseAll: one question per document, none when a clone stays open.
pub fn to_ask(docs: &[isize], closing: &[usize]) -> Vec<usize> {
    let kept = |d: isize| (0..docs.len()).any(|k| docs[k] == d && !closing.contains(&k));
    let mut asked = vec![];
    let mut out = vec![];
    for &i in closing {
        let Some(&d) = docs.get(i) else { continue };
        if !kept(d) && !asked.contains(&d) {
            asked.push(d);
            out.push(i);
        }
    }
    out
}

// A new tab order that keeps the main view tabs first, so a sort stays inside each view.
pub fn keep_views(order: &[usize], n_main: usize) -> Vec<usize> {
    let mut v = order.to_vec();
    v.sort_by_key(|&k| pane_at(k, n_main));
    v
}

// Notepad_plus.cpp doSynScroll: (columns, lines) to scroll the other view.
pub fn sync_scroll(
    from_main: bool,
    main: (isize, isize),
    sub: (isize, isize),
    off: (isize, isize),
    on: [bool; 2],
) -> (isize, isize) {
    let (l, c) = if from_main {
        (main.0 - off.0 - sub.0, main.1 - off.1 - sub.1)
    } else {
        (sub.0 + off.0 - main.0, sub.1 + off.1 - main.1)
    };
    (if on[1] { c } else { 0 }, if on[0] { l } else { 0 })
}

// SplitterContainer::rotateTo: the new orientation, and if the sub view comes first.
pub fn rotate(vertical: bool, swapped: bool, right: bool) -> (bool, bool) {
    let switch = if vertical { !right } else { right };
    (!vertical, swapped ^ switch)
}

fn doc_of(v: &NSView) -> isize {
    sci::send(v, SCI_GETDOCPOINTER, 0, 0)
}

fn column(v: &NSView) -> isize {
    let px = sci::send(v, SCI_TEXTWIDTH, STYLE_DEFAULT, c"P".as_ptr() as isize);
    sci::send(v, SCI_GETXOFFSET, 0, 0) / px.max(1)
}

impl App {
    fn vw(&self) -> &Views {
        &self.ivars().views
    }

    // Puts the main tab view in a split; the sub tab view joins it when a document goes there.
    pub(crate) fn edit_split(&self, main: &NSTabView) -> Retained<NSSplitView> {
        let mtm = self.mtm();
        let sub = Retained::into_super(DocTabs::new(mtm));
        sub.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(self)));
        let split = NSSplitView::new(mtm);
        split.setVertical(true);
        split.setDividerStyle(NSSplitViewDividerStyle::Thin);
        split.addSubview(main);
        self.tab_bar_menu_for(&sub);
        let _ = self.vw().sub.set(sub);
        let _ = self.vw().split.set(split.clone());
        split
    }

    pub(crate) fn doc_tabs(&self, p: usize) -> &NSTabView {
        match (p, self.vw().sub.get()) {
            (SUB, Some(s)) => s,
            _ => self.ivars().tab_view.get().unwrap(),
        }
    }

    pub(crate) fn active_view(&self) -> usize {
        self.vw().active.get()
    }

    fn view_shown(&self, p: usize) -> bool {
        unsafe { self.doc_tabs(p).superview() }.is_some()
    }

    fn n_main(&self) -> usize {
        self.doc_tabs(MAIN).numberOfTabViewItems() as usize
    }

    pub(crate) fn view_range(&self, p: usize) -> Range<usize> {
        range(p, self.n_main(), self.ivars().tabs.borrow().len())
    }

    pub(crate) fn pane_of(&self, i: usize) -> usize {
        pane_at(i, self.n_main())
    }

    pub(crate) fn clones_of(&self, i: usize) -> Vec<usize> {
        let docs: Vec<isize> = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .map(|t| doc_of(&t.view))
            .collect();
        peers(&docs, i)
    }

    // The lowest index among a tab and its clones, to count a document once.
    pub(crate) fn first_copy(&self, i: usize) -> usize {
        self.clones_of(i).into_iter().fold(i, usize::min)
    }

    // A tab for the same document in the active view, else `i`.
    pub(crate) fn in_active_view(&self, i: usize) -> usize {
        let a = self.active_view();
        if self.pane_of(i) == a {
            return i;
        }
        self.clones_of(i)
            .into_iter()
            .find(|&k| self.pane_of(k) == a)
            .unwrap_or(i)
    }

    // Copies the document fields of tab `i`, the tab that changed, to its clones.
    pub(crate) fn sync_clones(&self, i: usize) {
        let Some(src) = self.tab(i) else {
            return;
        };
        let mut restyle = vec![];
        for k in self.clones_of(i) {
            if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(k) {
                if t.lang != src.lang || t.path != src.path || t.name != src.name {
                    restyle.push(k);
                }
                *t = Tab {
                    view: t.view.clone(),
                    item: t.item.clone(),
                    ..src.clone()
                };
            }
        }
        restyle.iter().for_each(|&k| self.apply_tab_language(k));
    }

    // One tab for each document, for work that must not run twice on a clone.
    pub(crate) fn doc_tabs_once(&self) -> Vec<Tab> {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        let firsts: Vec<bool> = (0..tabs.len()).map(|i| self.first_copy(i) == i).collect();
        tabs.into_iter()
            .zip(firsts)
            .filter(|(_, f)| *f)
            .map(|(t, _)| t)
            .collect()
    }

    // The editor of a closing tab and of a clone that stays open.
    pub(crate) fn clone_view(
        &self,
        item: &NSTabViewItem,
    ) -> Option<(Retained<NSView>, Retained<NSView>)> {
        let i = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .position(|t| std::ptr::eq(&*t.item, item))?;
        let k = *self.clones_of(i).first()?;
        Some((self.tab(i)?.view, self.tab(k)?.view))
    }

    // NppIO.cpp fileCloseAll: asks once for each modified document that no other open tab keeps.
    pub(crate) fn confirm_close_all(&self, items: &[Retained<NSTabViewItem>]) -> bool {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        let docs: Vec<isize> = tabs.iter().map(|t| doc_of(&t.view)).collect();
        let closing: Vec<usize> = items
            .iter()
            .filter_map(|it| tabs.iter().position(|t| std::ptr::eq(&*t.item, &**it)))
            .collect();
        for i in to_ask(&docs, &closing) {
            if self.dirty(&tabs[i]) {
                self.tab_view().selectTabViewItem(Some(&tabs[i].item));
            }
            if !self.ask_save(i) {
                return false;
            }
        }
        true
    }

    pub(crate) fn with_clones(&self, i: usize) -> Vec<Retained<NSTabViewItem>> {
        let tabs = self.ivars().tabs.borrow();
        std::iter::once(i)
            .chain(self.clones_of(i))
            .filter_map(|k| tabs.get(k).map(|t| t.item.clone()))
            .collect()
    }

    // Notepad_plus::showView and hideView: an empty view hides; the main view stays when both are empty.
    pub(crate) fn layout_views(&self) {
        let Some(split) = self.vw().split.get() else {
            return;
        };
        let n = [MAIN, SUB].map(|p| self.doc_tabs(p).numberOfTabViewItems());
        let show = [n[MAIN] > 0 || n[SUB] == 0, n[SUB] > 0];
        let mut changed = false;
        for p in [MAIN, SUB] {
            let tv = self.doc_tabs(p);
            if show[p] && !self.view_shown(p) {
                let other = self.doc_tabs(1 - p);
                let first = (p == SUB) == self.vw().swapped.get();
                if first && self.view_shown(1 - p) {
                    split.addSubview_positioned_relativeTo(
                        tv,
                        NSWindowOrderingMode::Below,
                        Some(other),
                    );
                } else {
                    split.addSubview(tv);
                }
                changed = true;
            } else if !show[p] && self.view_shown(p) {
                tv.removeFromSuperview();
                changed = true;
            }
        }
        if changed {
            split.adjustSubviews();
            self.center_divider();
        }
        if !show[self.active_view()] {
            self.vw().active.set(1 - self.active_view());
        }
        if !(show[MAIN] && show[SUB]) {
            self.vw().sync.set([false; 2]);
        }
    }

    fn center_divider(&self) {
        let Some(split) = self.vw().split.get() else {
            return;
        };
        if split.subviews().count() == 2 {
            let s = split.frame().size;
            let at = if split.isVertical() {
                s.width
            } else {
                s.height
            } / 2.;
            split.setPosition_ofDividerAtIndex(at, 0);
        }
    }

    // Drop calls can select a tab in the other view; the view that was active stays active if it shows.
    pub(crate) fn keep_active(&self, p: usize) {
        if self.view_shown(p) {
            self.vw().active.set(p);
        }
    }

    // A didSelect from tab view `tv` makes its view the active one.
    pub(crate) fn view_selected(&self, tv: &NSTabView) {
        let sub = self.vw().sub.get().is_some_and(|s| std::ptr::eq(&**s, tv));
        self.vw().active.set(usize::from(sub));
    }

    // Moves tab `i` to the end of view `to`; the caller lays out the views.
    fn transfer(&self, i: usize, to: usize) {
        if self.pane_of(i) == to {
            return;
        }
        let n_main = self.n_main();
        let Some(t) = self.tab(i) else { return };
        {
            let mut tabs = self.ivars().tabs.borrow_mut();
            let t = tabs.remove(i);
            if to == SUB {
                tabs.push(t);
            } else {
                tabs.insert(n_main, t);
            }
        }
        self.doc_tabs(1 - to).removeTabViewItem(&t.item);
        self.doc_tabs(to).addTabViewItem(&t.item);
    }

    // A new tab in view `to` that shows the document of tab `i`, at the same place.
    fn add_clone(&self, i: usize, to: usize) -> Option<usize> {
        let src = self.tab(i)?;
        let view = sci::new_view();
        sci::set_delegate(&view, self);
        sci::send(&view, SCI_SETDOCPOINTER, 0, doc_of(&src.view));
        self.setup_editor(&view, language::tab_language(&src));
        let get = |m| sci::send(&src.view, m, 0, 0);
        sci::send(
            &view,
            SCI_SETSEL,
            get(SCI_GETANCHOR) as usize,
            get(SCI_GETCURRENTPOS),
        );
        sci::send(
            &view,
            SCI_SETFIRSTVISIBLELINE,
            get(SCI_GETFIRSTVISIBLELINE) as usize,
            0,
        );
        sci::send(&view, SCI_SETXOFFSET, get(SCI_GETXOFFSET) as usize, 0);
        let item = NSTabViewItem::new();
        item.setView(Some(&view));
        let at = self.view_range(to).end;
        self.ivars().tabs.borrow_mut().insert(
            at,
            Tab {
                view,
                item: item.clone(),
                ..src
            },
        );
        self.doc_tabs(to).addTabViewItem(&item);
        self.apply_udl_at(at);
        self.refresh_title(at);
        Some(at)
    }

    pub(crate) fn move_to_other_view(&self) {
        self.go_to_other_view(false);
    }

    pub(crate) fn clone_to_other_view(&self) {
        self.go_to_other_view(true);
    }

    fn can_move(&self) -> bool {
        let a = self.active_view();
        self.current().is_some() && (self.view_range(a).len() > 1 || self.view_shown(1 - a))
    }

    // Notepad_plus.cpp docGotoAnotherEditView and loadBufferIntoView.
    fn go_to_other_view(&self, clone: bool) {
        let Some(i) = self.current() else { return };
        if !clone && !self.can_move() {
            return;
        }
        let to = 1 - self.active_view();
        let Some(t) = self.tab(i) else { return };
        let there = self
            .clones_of(i)
            .into_iter()
            .find(|&k| self.pane_of(k) == to);
        let lone = Some(self.view_range(to))
            .filter(|r| there.is_none() && r.len() == 1)
            .and_then(|r| self.tab(r.start))
            .filter(|l| l.path.is_none() && !self.dirty(l));
        let item = match there {
            Some(k) => self.tab(k).map(|k| k.item),
            None if clone => self
                .add_clone(i, to)
                .and_then(|k| self.tab(k))
                .map(|k| k.item),
            None => {
                self.transfer(i, to);
                Some(t.item.clone())
            }
        };
        if let Some(l) = lone {
            self.drop_tabs(&[l.item]);
        }
        self.layout_views();
        if let Some(item) = &item {
            self.tab_view().selectTabViewItem(Some(item));
        }
        if !clone && there.is_some() {
            self.drop_tabs(&[t.item]);
        }
    }

    // IDM_VIEW_SWITCHTO_OTHER_VIEW: from the editor to the other view, else back to the editor.
    pub(crate) fn focus_other_view(&self) {
        let a = self.active_view();
        let in_editor = match (self.ivars().window.get(), self.editor()) {
            (Some(w), Some(v)) => w.firstResponder().is_some_and(|r| {
                Retained::as_ptr(&r).cast::<c_void>() == Retained::as_ptr(&sci::content(&v)).cast()
            }),
            _ => false,
        };
        let to = if in_editor && self.view_shown(1 - a) {
            1 - a
        } else {
            a
        };
        let tv = self.doc_tabs(to);
        if let Some(item) = tv.selectedTabViewItem() {
            tv.selectTabViewItem(Some(&item));
        }
    }

    // IDM_VIEW_SYNSCROLLV (tag 0) and IDM_VIEW_SYNSCROLLH (tag 1): the offset between the views stays.
    pub(crate) fn toggle_sync(&self, tag: usize) {
        let mut on = self.vw().sync.get();
        let Some(k) = on.get_mut(tag) else { return };
        *k = !*k;
        let (mut line, mut col) = self.vw().offset.get();
        if let (true, Some(m), Some(s)) = (*k, self.visible_editor(MAIN), self.visible_editor(SUB))
        {
            if tag == 0 {
                line = sci::send(&m, SCI_GETFIRSTVISIBLELINE, 0, 0)
                    - sci::send(&s, SCI_GETFIRSTVISIBLELINE, 0, 0);
            } else {
                col = column(&m) - column(&s);
            }
        }
        self.vw().sync.set(on);
        self.vw().offset.set((line, col));
    }

    fn visible_editor(&self, p: usize) -> Option<Retained<NSView>> {
        self.doc_tabs(p)
            .selectedTabViewItem()
            .and_then(|i| i.view(self.mtm()))
    }

    pub(crate) fn rotate_views(&self, right: bool) {
        let Some(split) = self.vw().split.get() else {
            return;
        };
        let (v, swapped) = rotate(split.isVertical(), self.vw().swapped.get(), right);
        split.setVertical(v);
        if swapped != self.vw().swapped.get() {
            self.vw().swapped.set(swapped);
            if let Some(first) = split
                .subviews()
                .firstObject()
                .filter(|_| split.subviews().count() == 2)
            {
                first.removeFromSuperview();
                split.addSubview(&first);
            }
        }
        split.adjustSubviews();
        self.center_divider();
    }

    // Focus in an editor makes its view active; a scroll moves the other view when sync is on.
    pub(crate) fn views_notify(&self, scn: *const c_void) {
        let h = unsafe { &*(scn as *const Header) };
        if h.code != SCN_FOCUSIN && h.code != sci::SCN_UPDATEUI {
            return;
        }
        let from = h.hwnd_from as isize;
        let eds = [MAIN, SUB].map(|p| self.visible_editor(p).filter(|_| self.view_shown(p)));
        let Some(p) = (0..2).find(|&p| {
            eds[p]
                .as_ref()
                .is_some_and(|v| sci::send(v, SCI_GETDIRECTPOINTER, 0, 0) == from)
        }) else {
            return;
        };
        if h.code == SCN_FOCUSIN {
            if self.vw().active.replace(p) != p {
                self.update_status();
                self.doc_list_reload();
                self.function_list_reload();
            }
            return;
        }
        let on = self.vw().sync.get();
        let [Some(m), Some(s)] = &eds else { return };
        if on == [false; 2] {
            return;
        }
        let first = |v: &NSView| sci::send(v, SCI_GETFIRSTVISIBLELINE, 0, 0);
        let (cols, lines) = sync_scroll(
            p == MAIN,
            (first(m), column(m)),
            (first(s), column(s)),
            self.vw().offset.get(),
            on,
        );
        if cols != 0 || lines != 0 {
            sci::send(
                if p == MAIN { s } else { m },
                SCI_LINESCROLL,
                cols as usize,
                lines,
            );
        }
    }

    // NppIO.cpp loadSession: subView files go to the sub view; a file in both lists becomes a clone.
    pub(crate) fn restore_views(&self, s: &Session) {
        let main = s.files.len().saturating_sub(s.in_sub_view);
        let lists = s.files.split_at(main);
        let tabs_of = |name: &str| -> Vec<usize> {
            let tabs = self.ivars().tabs.borrow();
            (0..tabs.len())
                .filter(|&i| {
                    tabs[i]
                        .path
                        .as_deref()
                        .is_some_and(|q| fileops::same_file(q, Path::new(name)))
                })
                .collect()
        };
        for (k, f) in s.files.iter().enumerate() {
            let to = usize::from(k >= main);
            let found = tabs_of(&f.filename);
            if found.iter().any(|&i| self.pane_of(i) == to) {
                continue;
            }
            let Some(&i) = found.first() else { continue };
            let other = [lists.0, lists.1][1 - to]
                .iter()
                .find(|o| o.filename == f.filename);
            match (other, self.tab(i)) {
                (Some(o), Some(orig)) => {
                    if let Some(c) = self.add_clone(i, to).and_then(|c| self.tab(c)) {
                        restore_position(&c.view, f);
                    }
                    restore_position(&orig.view, o);
                }
                _ => self.transfer(i, to),
            }
        }
        self.layout_views();
        let active = s.files.get(s.active).and_then(|f| {
            let to = usize::from(s.active >= main);
            let found = tabs_of(&f.filename);
            found.into_iter().find(|&i| self.pane_of(i) == to)
        });
        if let Some(t) = active.and_then(|i| self.tab(i)) {
            self.tab_view().selectTabViewItem(Some(&t.item));
        }
    }

    pub(crate) fn validate_views(&self, item: &NSMenuItem) -> Option<bool> {
        let a = item.action()?;
        let both = self.view_shown(MAIN) && self.view_shown(SUB);
        let on = if a == sel!(syncScroll:) {
            let on = self
                .vw()
                .sync
                .get()
                .get(item.tag() as usize)
                .copied()
                .unwrap_or(false);
            item.setState(if on {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            both
        } else if a == sel!(moveToOtherView:) {
            self.can_move()
        } else if a == sel!(cloneToOtherView:) {
            self.current().is_some()
        } else if a == sel!(focusOtherView:) || a == sel!(rotateViews:) {
            true
        } else {
            return None;
        };
        Some(on)
    }
}

// Adds the Notepad++ two view items to the View menu items of view::view_menu.
pub fn with_view_items(
    mtm: MainThreadMarker,
    mut v: Vec<Retained<NSMenuItem>>,
    t: Option<&AnyObject>,
) -> Vec<Retained<NSMenuItem>> {
    let at = |v: &[Retained<NSMenuItem>], title: &str| {
        v.iter()
            .position(|i| i.title().to_string() == title)
            .map_or(v.len(), |p| p + 1)
    };
    let mv = nested(
        mtm,
        "Move/Clone Current Document",
        vec![
            item(mtm, "Move to Other View", sel!(moveToOtherView:), "", t),
            item(mtm, "Clone to Other View", sel!(cloneToOtherView:), "", t),
        ],
    );
    let p = at(&v, "Zoom");
    v.insert(p, mv);
    let focus = item(mtm, "Focus on Another View", sel!(focusOtherView:), F8, t);
    focus.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());
    let p = at(&v, "Word wrap");
    v.insert(p, focus);
    let p = at(&v, "Function List");
    let tail = [
        NSMenuItem::separatorItem(mtm),
        tagged(
            mtm,
            "Synchronize Vertical Scrolling",
            sel!(syncScroll:),
            0,
            t,
        ),
        tagged(
            mtm,
            "Synchronize Horizontal Scrolling",
            sel!(syncScroll:),
            1,
            t,
        ),
        NSMenuItem::separatorItem(mtm),
        tagged(mtm, "Rotate to Right", sel!(rotateViews:), 1, t),
        tagged(mtm, "Rotate to Left", sel!(rotateViews:), 0, t),
    ];
    v.splice(p..p, tail);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_ranges() {
        assert_eq!(range(MAIN, 3, 5), 0..3);
        assert_eq!(range(SUB, 3, 5), 3..5);
        assert_eq!(range(SUB, 3, 3), 3..3);
        assert_eq!(range(MAIN, 0, 2), 0..0);
        assert_eq!((pane_at(2, 3), pane_at(3, 3)), (MAIN, SUB));
        assert_eq!(keep_views(&[4, 1, 3, 0, 2], 3), [1, 0, 2, 4, 3]);
    }

    #[test]
    fn clone_rules() {
        let docs = [10, 20, 10, 30];
        assert_eq!(peers(&docs, 0), [2]);
        assert_eq!(peers(&docs, 1), Vec::<usize>::new());
        assert_eq!(peers(&docs, 9), Vec::<usize>::new());
        assert_eq!(to_ask(&docs, &[0]), Vec::<usize>::new());
        assert_eq!(to_ask(&docs, &[2, 0]), [2]);
        assert_eq!(to_ask(&docs, &[0, 1, 2, 3]), [0, 1, 3]);
        assert_eq!(to_ask(&docs, &[3, 1]), [3, 1]);
    }

    #[test]
    fn scroll_sync() {
        let on = [true, true];
        assert_eq!(sync_scroll(true, (40, 3), (10, 0), (5, 1), on), (2, 25));
        assert_eq!(sync_scroll(false, (40, 3), (10, 0), (5, 1), on), (-2, -25));
        assert_eq!(sync_scroll(true, (15, 0), (10, 0), (5, 0), on), (0, 0));
        assert_eq!(
            sync_scroll(true, (40, 3), (10, 0), (0, 0), [true, false]),
            (0, 30)
        );
    }

    #[test]
    fn rotation() {
        assert_eq!(rotate(true, false, true), (false, false));
        assert_eq!(rotate(true, false, false), (false, true));
        assert_eq!(rotate(false, false, true), (true, true));
        assert_eq!(rotate(false, true, false), (true, true));
        let (v, s) = rotate(true, false, true);
        assert_eq!(rotate(v, s, true), (true, true));
    }
}
