// SPDX-License-Identifier: GPL-3.0-or-later
use crate::funclist::{self, Node};
use crate::{language, ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSApplication, NSApplicationDidBecomeActiveNotification,
    NSApplicationWillTerminateNotification, NSAutoresizingMaskOptions, NSButton,
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSImageCell, NSMenu, NSMenuDelegate,
    NSMenuItem, NSOutlineView, NSOutlineViewDataSource, NSScrollView, NSSearchField,
    NSSegmentDistribution, NSSegmentedControl, NSSplitView, NSSplitViewDividerStyle,
    NSTableColumn, NSTableColumnResizingOptions, NSTableView, NSTableViewColumnAutoresizingStyle,
    NSTableViewDataSource, NSTextField, NSUserInterfaceItemIdentification, NSView,
};
use objc2_foundation::{
    NSIndexSet, NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSTimer,
};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;

// Panel ids; Document List and Function List keep the ids that the View menu gives them.
pub const LEFT: isize = 0;
pub const RIGHT: isize = 1;
pub const DOC_MAP: isize = 2;
pub const FOLDERS: isize = 3;
pub const CLIPBOARD: isize = 4;
pub const CHARS: isize = 5;
const TITLE_H: f64 = 24.;
const TOOLBAR_H: f64 = 28.;
const TAB_H: f64 = 26.;
const WIDTH: f64 = 250.;
// Port only: Notepad++ parses any size on the main thread.
const PARSE_LIMIT: isize = 10 * 1024 * 1024;

// Title and default side (0 left, 1 right) of each panel, as Notepad_plus.cpp launch* sets DWS_DF_CONT_LEFT or DWS_DF_CONT_RIGHT.
const PANELS: [(&str, usize); 6] = [
    ("Document List", 0),
    ("Function List", 1),
    ("Document Map", 1),
    ("Folder as Workspace", 0),
    ("Clipboard History", 1),
    ("ASCII Codes Insertion Panel", 1),
];

struct Item {
    label: String,
    pos: isize,
    children: Vec<usize>,
    obj: Retained<NSNumber>,
}

// A docking area: a caption, the open panels, and a tab strip when two or more panels are open, as a Notepad++ DockingCont.
struct Side {
    view: Retained<NSView>,
    title: Retained<NSTextField>,
    close: Retained<NSButton>,
    tabs: Retained<NSSegmentedControl>,
    open: RefCell<Vec<isize>>,
    active: Cell<isize>,
}

// Two docking areas next to the editor, left and right, as the Notepad++ defaults.
pub struct Dock {
    outer: Retained<NSSplitView>,
    sides: [Side; 2],
    widths: Cell<[f64; 2]>,
    contents: RefCell<[Option<Retained<NSView>>; 6]>,
    pub(crate) target: Retained<PanelTarget>,
    ticking: Cell<bool>,
    docs: Retained<NSTableView>,
    funcs: Retained<NSOutlineView>,
    search: Retained<NSSearchField>,
    sort: Retained<NSButton>,
    items: RefCell<Vec<Item>>,
    roots: RefCell<Vec<usize>>,
    key: RefCell<String>,
    mark: Cell<(isize, isize)>,
    state: RefCell<HashMap<String, (bool, String)>>,
    pub(crate) map: OnceCell<crate::docmap::Map>,
    pub(crate) folders: OnceCell<crate::filebrowser::Folders>,
    pub(crate) clips: OnceCell<crate::cliphistory::Clips>,
    pub(crate) chars: OnceCell<crate::charpanel::Chars>,
}

define_class!(
    // Receives the actions, data requests and notifications of the panels.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Retained<App>]
    pub(crate) struct PanelTarget;

    impl PanelTarget {
        #[unsafe(method(dockTab:))]
        fn dock_tab(&self, s: &NSSegmentedControl) {
            self.ivars().dock_tab(s);
        }

        #[unsafe(method(panelTick:))]
        fn tick(&self, _t: Option<&AnyObject>) {
            self.ivars().panels_tick();
        }

        #[unsafe(method(appActive:))]
        fn app_active(&self, _n: &NSNotification) {
            self.ivars().folders_refresh();
        }

        #[unsafe(method(appWillQuit:))]
        fn app_will_quit(&self, _n: &NSNotification) {
            self.ivars().folders_save();
        }

        #[unsafe(method(clipInsert:))]
        fn clip_insert(&self, _s: Option<&AnyObject>) {
            self.ivars().clips_insert();
        }

        #[unsafe(method(charInsert:))]
        fn char_insert(&self, _s: Option<&AnyObject>) {
            self.ivars().chars_insert();
        }

        #[unsafe(method(folderOpen:))]
        fn folder_open(&self, _s: Option<&AnyObject>) {
            self.ivars().folders_open();
        }

        #[unsafe(method(folderCmd:))]
        fn folder_cmd(&self, s: &NSMenuItem) {
            self.ivars().folders_cmd(s.tag());
        }
    }

    unsafe impl NSObjectProtocol for PanelTarget {}

    unsafe impl NSTableViewDataSource for PanelTarget {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, t: &NSTableView) -> isize {
            match t.identifier().map(|i| i.to_string()).as_deref() {
                Some("clips") => self.ivars().clips_count(),
                Some("chars") => 256,
                _ => 0,
            }
        }

        #[unsafe(method_id(tableView:objectValueForTableColumn:row:))]
        fn value(&self, t: &NSTableView, c: Option<&NSTableColumn>, row: isize) -> Option<Retained<AnyObject>> {
            self.ivars().panel_cell(t, c, row)
        }
    }

    unsafe impl NSOutlineViewDataSource for PanelTarget {
        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        fn count(&self, _o: &NSOutlineView, item: Option<&AnyObject>) -> isize {
            self.ivars().folders_count(item)
        }

        #[unsafe(method_id(outlineView:child:ofItem:))]
        fn child(&self, _o: &NSOutlineView, n: isize, item: Option<&AnyObject>) -> Option<Retained<AnyObject>> {
            self.ivars().folders_child(n, item)
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        fn expandable(&self, _o: &NSOutlineView, item: &AnyObject) -> bool {
            self.ivars().folders_expandable(item)
        }

        #[unsafe(method_id(outlineView:objectValueForTableColumn:byItem:))]
        fn object(&self, _o: &NSOutlineView, _c: Option<&NSTableColumn>, item: Option<&AnyObject>) -> Option<Retained<AnyObject>> {
            self.ivars().folders_value(item)
        }
    }

    unsafe impl NSMenuDelegate for PanelTarget {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, m: &NSMenu) {
            self.ivars().folders_menu(m);
        }
    }
);

// PathFindExtension: the text from the last dot, if no space follows it.
pub fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if !name[i..].contains(' ') => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

// ScintillaEditView::scrollPosToCenter: lines to scroll to put `line` in the middle; it compares as size_t does.
pub fn center_scroll(line: isize, first: isize, last: isize, on_screen: isize) -> isize {
    let middle = if ((line - first) as usize) < ((last - line) as usize) {
        first + on_screen / 2
    } else {
        last - on_screen / 2
    };
    line - middle
}

pub(crate) fn frame(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

pub(crate) fn fill() -> NSAutoresizingMaskOptions {
    NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable
}

fn symbol_button(
    mtm: MainThreadMarker,
    symbol: &str,
    tip: &str,
    t: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    let img =
        NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(symbol), Some(&ns(tip)));
    let b = match img {
        Some(i) => unsafe {
            NSButton::buttonWithImage_target_action(&i, Some(t), Some(action), mtm)
        },
        None => unsafe {
            NSButton::buttonWithTitle_target_action(&ns(tip), Some(t), Some(action), mtm)
        },
    };
    b.setBordered(false);
    b.setToolTip(Some(&ns(tip)));
    b
}

pub(crate) fn scroll(mtm: MainThreadMarker, doc: &NSView, w: f64, h: f64) -> Retained<NSScrollView> {
    let s = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame(0., 0., w, h));
    s.setHasVerticalScroller(true);
    s.setAutohidesScrollers(true);
    s.setDocumentView(Some(doc));
    s.setAutoresizingMask(fill());
    s
}

pub(crate) fn column(mtm: MainThreadMarker, id: &str, title: &str, w: f64) -> Retained<NSTableColumn> {
    let c = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns(id));
    c.setTitle(&ns(title));
    c.setWidth(w);
    c.setEditable(false);
    c
}

// The view of one panel, which the docking area sizes.
pub(crate) fn content_box(mtm: MainThreadMarker) -> Retained<NSView> {
    let b = NSView::initWithFrame(NSView::alloc(mtm), frame(0., 0., WIDTH, 400.));
    b.setAutoresizingMask(fill());
    b
}

// A docking area: a caption with the title and a close button above the panels, and the tab strip below them.
fn side(mtm: MainThreadMarker, t: &AnyObject, target: &PanelTarget, h: f64) -> Side {
    let p = NSView::initWithFrame(NSView::alloc(mtm), frame(0., 0., WIDTH, h));
    let title = NSTextField::labelWithString(&ns(""), mtm);
    title.setFrame(frame(6., h - TITLE_H + 4., WIDTH - 32., 16.));
    title.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    p.addSubview(&title);
    let close = symbol_button(mtm, "xmark", "Close", t, sel!(dockClose:));
    close.setFrame(frame(WIDTH - 24., h - TITLE_H + 2., 20., 20.));
    close.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    p.addSubview(&close);
    let tabs = NSSegmentedControl::initWithFrame(
        NSSegmentedControl::alloc(mtm),
        frame(2., 2., WIDTH - 4., TAB_H - 4.),
    );
    tabs.setSegmentDistribution(NSSegmentDistribution::Fill);
    tabs.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMaxYMargin,
    );
    unsafe {
        tabs.setTarget(Some(target));
        tabs.setAction(Some(sel!(dockTab:)));
    }
    tabs.setHidden(true);
    p.addSubview(&tabs);
    p.setHidden(true);
    Side {
        view: p,
        title,
        close,
        tabs,
        open: RefCell::new(vec![]),
        active: Cell::new(-1),
    }
}

impl App {
    pub(crate) fn dock_ui(&self) -> Option<&Dock> {
        self.ivars().dock.get()
    }

    // Puts the editor area between the two docking areas and returns the view that holds all three.
    pub(crate) fn dock(&self, center: &NSView) -> Retained<NSSplitView> {
        let mtm = self.mtm();
        let t: &AnyObject = self;
        let f = center.frame();
        let h = f.size.height;
        let ch = h - TITLE_H;
        let target: Retained<PanelTarget> = {
            let this = PanelTarget::alloc(mtm).set_ivars(self.retain());
            unsafe { msg_send![super(this), init] }
        };
        let outer = NSSplitView::initWithFrame(NSSplitView::alloc(mtm), f);
        outer.setVertical(true);
        outer.setDividerStyle(NSSplitViewDividerStyle::Thin);
        outer.setAutoresizingMask(center.autoresizingMask());
        let sides = [side(mtm, t, &target, h), side(mtm, t, &target, h)];
        let left = content_box(mtm);
        left.setFrame(frame(0., 0., WIDTH, ch));
        let docs = NSTableView::initWithFrame(NSTableView::alloc(mtm), frame(0., 0., WIDTH, 100.));
        let status = column(mtm, "status", "", 18.);
        let name = column(mtm, "name", "Name", WIDTH - 90.);
        let ext = column(mtm, "ext", "Ext.", 60.);
        status.setResizingMask(NSTableColumnResizingOptions::NoResizing);
        ext.setResizingMask(NSTableColumnResizingOptions::UserResizingMask);
        name.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        docs.setColumnAutoresizingStyle(NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle);
        let cell = NSImageCell::new(mtm);
        unsafe { status.setDataCell(&cell) };
        docs.addTableColumn(&status);
        docs.addTableColumn(&name);
        docs.addTableColumn(&ext);
        unsafe {
            docs.setDataSource(Some(ProtocolObject::from_ref(self)));
            docs.setTarget(Some(t));
            docs.setAction(Some(sel!(docListClick:)));
        }
        left.addSubview(&scroll(mtm, &docs, WIDTH, ch));
        let right = content_box(mtm);
        right.setFrame(frame(0., 0., WIDTH, ch));
        let funcs =
            NSOutlineView::initWithFrame(NSOutlineView::alloc(mtm), frame(0., 0., WIDTH, 100.));
        let c = column(mtm, "name", "", WIDTH);
        c.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        funcs.addTableColumn(&c);
        unsafe {
            funcs.setOutlineTableColumn(Some(&c));
            funcs.setDataSource(Some(ProtocolObject::from_ref(self)));
            funcs.setTarget(Some(t));
            funcs.setDoubleAction(Some(sel!(functionListOpen:)));
        }
        funcs.setHeaderView(None);
        funcs.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        right.addSubview(&scroll(mtm, &funcs, WIDTH, ch - TOOLBAR_H));
        let search = NSSearchField::initWithFrame(
            NSSearchField::alloc(mtm),
            frame(4., ch - TOOLBAR_H + 3., WIDTH - 60., 22.),
        );
        unsafe {
            search.setTarget(Some(t));
            search.setAction(Some(sel!(functionListSearch:)));
        }
        let top = NSAutoresizingMaskOptions::ViewMinYMargin;
        search.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | top);
        right.addSubview(&search);
        let sort = symbol_button(
            mtm,
            "arrow.up.arrow.down",
            "Sort",
            t,
            sel!(functionListSort:),
        );
        sort.setButtonType(objc2_app_kit::NSButtonType::PushOnPushOff);
        let reload = symbol_button(
            mtm,
            "arrow.clockwise",
            "Reload",
            t,
            sel!(functionListReload:),
        );
        for (i, b) in [&sort, &reload].iter().enumerate() {
            b.setFrame(frame(
                WIDTH - 52. + 24. * i as f64,
                ch - TOOLBAR_H + 4.,
                20.,
                20.,
            ));
            b.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin | top);
            right.addSubview(b);
        }
        sides[0].view.addSubview(&left);
        sides[1].view.addSubview(&right);
        outer.addSubview(&sides[0].view);
        outer.addSubview(center);
        outer.addSubview(&sides[1].view);
        for (i, p) in [260., 250., 260.].iter().enumerate() {
            outer.setHoldingPriority_forSubviewAtIndex(*p, i as isize);
        }
        let nc = NSNotificationCenter::defaultCenter();
        unsafe {
            nc.addObserver_selector_name_object(
                &target,
                sel!(appActive:),
                Some(NSApplicationDidBecomeActiveNotification),
                None,
            );
            nc.addObserver_selector_name_object(
                &target,
                sel!(appWillQuit:),
                Some(NSApplicationWillTerminateNotification),
                None,
            );
        }
        let _ = self.ivars().dock.set(Dock {
            outer: outer.clone(),
            sides,
            widths: Cell::new([WIDTH, WIDTH]),
            contents: RefCell::new([Some(left), Some(right), None, None, None, None]),
            target,
            ticking: Cell::new(false),
            docs,
            funcs,
            search,
            sort,
            items: RefCell::new(vec![]),
            roots: RefCell::new(vec![]),
            key: RefCell::new(String::new()),
            mark: Cell::new((1, 0)),
            state: RefCell::new(HashMap::new()),
            map: OnceCell::new(),
            folders: OnceCell::new(),
            clips: OnceCell::new(),
            chars: OnceCell::new(),
        });
        outer
    }

    // The view that holds the editor and the docking areas, so a bar below it can shrink it.
    pub(crate) fn editor_area(&self) -> Option<Retained<NSView>> {
        match self.dock_ui() {
            Some(d) => Some(Retained::into_super(d.outer.clone())),
            None => self.ivars().split.get().map(|s| Retained::into_super(s.clone())),
        }
    }

    // True when the panel is open, also when an other tab of its docking area shows; the menu checkmark.
    pub(crate) fn panel_visible(&self, id: isize) -> bool {
        let side = PANELS.get(id as usize).map(|p| p.1);
        self.dock_ui()
            .zip(side)
            .is_some_and(|(d, s)| d.sides[s].open.borrow().contains(&id))
    }

    // True when the panel is open and its tab is the active one.
    pub(crate) fn panel_shown(&self, id: isize) -> bool {
        let side = PANELS.get(id as usize).map(|p| p.1);
        self.dock_ui()
            .zip(side)
            .is_some_and(|(d, s)| self.panel_visible(id) && d.sides[s].active.get() == id)
    }

    // The checkmark of a panel menu item; None for other actions.
    pub(crate) fn panel_checked(&self, action: Sel) -> Option<bool> {
        let id = [
            (sel!(toggleDocMap:), DOC_MAP),
            (sel!(toggleFolderAsWorkspace:), FOLDERS),
            (sel!(toggleClipboardHistory:), CLIPBOARD),
            (sel!(toggleCharPanel:), CHARS),
        ]
        .into_iter()
        .find(|(a, _)| *a == action)?
        .1;
        Some(self.panel_visible(id))
    }

    // NppCommands.cpp IDM_VIEW_DOCLIST, IDM_VIEW_FUNC_LIST, IDM_VIEW_DOC_MAP and the other panel commands: close an open panel, or open it.
    pub(crate) fn toggle_panel(&self, id: isize) {
        if self.panel_visible(id) {
            self.dock_close_panel(id);
        } else {
            self.dock_open_panel(id);
        }
    }

    // Opens the panel as the active tab of its docking area.
    pub(crate) fn dock_open_panel(&self, id: isize) {
        let Some(d) = self.dock_ui() else { return };
        let Some(&(_, side)) = PANELS.get(id as usize) else {
            return;
        };
        let Some(c) = self.panel_content(id) else {
            return;
        };
        let s = &d.sides[side];
        if unsafe { c.superview() }.is_none() {
            s.view.addSubview(&c);
        }
        if !s.open.borrow().contains(&id) {
            s.open.borrow_mut().push(id);
        }
        s.active.set(id);
        if s.view.isHidden() {
            self.show_side(side, true);
        }
        self.layout_side(side);
        self.panel_refresh(id);
    }

    // DockingCont::hideToolbar: the tab before the closed one becomes active; the area hides when no panel is left.
    pub(crate) fn dock_close_panel(&self, id: isize) {
        let Some(d) = self.dock_ui() else { return };
        let Some(&(_, side)) = PANELS.get(id as usize) else {
            return;
        };
        let s = &d.sides[side];
        let Some(k) = s.open.borrow().iter().position(|&x| x == id) else {
            return;
        };
        s.open.borrow_mut().remove(k);
        if let Some(c) = &d.contents.borrow()[id as usize] {
            c.setHidden(true);
        }
        let next = s.open.borrow().get(k.saturating_sub(1)).copied();
        let Some(next) = next else {
            self.show_side(side, false);
            self.focus();
            return;
        };
        if s.active.get() == id {
            s.active.set(next);
        }
        self.layout_side(side);
        self.panel_refresh(s.active.get());
    }

    fn panel_content(&self, id: isize) -> Option<Retained<NSView>> {
        let d = self.dock_ui()?;
        if let Some(c) = d.contents.borrow().get(id as usize)?.clone() {
            return Some(c);
        }
        let c = match id {
            DOC_MAP => self.doc_map_build(),
            FOLDERS => self.folders_build(),
            CLIPBOARD => self.clips_build(),
            CHARS => self.chars_build(),
            _ => return None,
        };
        if matches!(id, DOC_MAP | CLIPBOARD) && !d.ticking.replace(true) {
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    0.5,
                    &d.target,
                    sel!(panelTick:),
                    None,
                    true,
                )
            };
        }
        d.contents.borrow_mut()[id as usize] = Some(c.clone());
        Some(c)
    }

    fn panel_refresh(&self, id: isize) {
        match id {
            LEFT => self.doc_list_reload(),
            RIGHT => self.function_list_reload(),
            DOC_MAP => self.doc_map_reload(),
            FOLDERS => self.folders_refresh(),
            CHARS => self.chars_sync(),
            _ => {}
        }
    }

    // Shows only the active panel, sizes it above the tab strip, and sets the caption, the close button and the tabs.
    fn layout_side(&self, side: usize) {
        let Some(d) = self.dock_ui() else { return };
        let s = &d.sides[side];
        let open = s.open.borrow().clone();
        let active = s.active.get();
        let size = s.view.bounds().size;
        let tab_h = if open.len() > 1 { TAB_H } else { 0. };
        for (id, c) in d.contents.borrow().iter().enumerate() {
            let Some(c) = c.as_ref().filter(|_| PANELS[id].1 == side) else {
                continue;
            };
            let id = id as isize;
            c.setHidden(id != active || !open.contains(&id));
            c.setFrame(frame(0., tab_h, size.width, size.height - TITLE_H - tab_h));
        }
        s.tabs.setHidden(open.len() < 2);
        s.tabs.setSegmentCount(open.len() as isize);
        for (k, id) in open.iter().enumerate() {
            s.tabs.setLabel_forSegment(&ns(PANELS[*id as usize].0), k as isize);
        }
        if let Some(k) = open.iter().position(|&x| x == active) {
            s.tabs.setSelectedSegment(k as isize);
        }
        s.title.setStringValue(&ns(PANELS.get(active as usize).map_or("", |p| p.0)));
        s.close.setTag(active);
    }

    // A click on a tab of the tab strip makes its panel the active one.
    fn dock_tab(&self, tabs: &NSSegmentedControl) {
        let Some(d) = self.dock_ui() else { return };
        let Some(side) = d.sides.iter().position(|s| std::ptr::eq(&*s.tabs, tabs)) else {
            return;
        };
        let id = d.sides[side]
            .open
            .borrow()
            .get(tabs.selectedSegment() as usize)
            .copied();
        if let Some(id) = id {
            d.sides[side].active.set(id);
            self.layout_side(side);
            self.panel_refresh(id);
        }
    }

    // Shows or hides a docking area and keeps the widths that the user gave the areas.
    fn show_side(&self, side: usize, show: bool) {
        let Some(d) = self.dock_ui() else { return };
        let mut widths = d.widths.get();
        for (w, s) in widths.iter_mut().zip(&d.sides) {
            if !s.view.isHidden() {
                *w = s.view.frame().size.width.max(80.);
            }
        }
        d.widths.set(widths);
        d.sides[side].view.setHidden(!show);
        d.outer.adjustSubviews();
        let total = d.outer.frame().size.width;
        let gap = d.outer.dividerThickness();
        if !d.sides[0].view.isHidden() {
            d.outer.setPosition_ofDividerAtIndex(widths[0], 0);
        }
        if !d.sides[1].view.isHidden() {
            d.outer.setPosition_ofDividerAtIndex(total - widths[1] - gap, 1);
        }
    }

    fn panel_cell(&self, t: &NSTableView, c: Option<&NSTableColumn>, row: isize) -> Option<Retained<AnyObject>> {
        let s = match t.identifier()?.to_string().as_str() {
            "clips" => self.clips_text(row)?,
            "chars" => self.chars_text(row, c?.identifier().to_string().parse().ok()?)?,
            _ => return None,
        };
        Some(Retained::into_super(Retained::into_super(ns(&s))))
    }

    fn panels_tick(&self) {
        if !NSApplication::sharedApplication(self.mtm()).isActive() {
            return;
        }
        self.clips_poll();
        if self.panel_shown(DOC_MAP) {
            self.doc_map_scroll();
        }
    }

    // VerticalFileSwitcher: the open documents in tab order, with the current one selected.
    pub(crate) fn doc_list_reload(&self) {
        self.chars_sync();
        if !self.panel_visible(LEFT) {
            return;
        }
        let Some(d) = self.dock_ui() else { return };
        d.docs.reloadData();
        if let Some(i) = self.current() {
            d.docs
                .selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(i), false);
            d.docs.scrollRowToVisible(i as isize);
        }
    }

    pub(crate) fn doc_list_rows(&self) -> isize {
        self.ivars().tabs.borrow().len() as isize
    }

    pub(crate) fn doc_list_value(
        &self,
        col: &NSTableColumn,
        row: isize,
    ) -> Option<Retained<AnyObject>> {
        let t = self.tab(row as usize)?;
        let (name, ext) = split_ext(&t.name);
        let s = |s: &str| Some(Retained::into_super(Retained::into_super(ns(s))));
        match col.identifier().to_string().as_str() {
            "name" => s(name),
            "ext" => s(ext),
            _ => {
                let symbol = if t.ro {
                    "lock.fill"
                } else if self.dirty(&t) {
                    "pencil.circle.fill"
                } else {
                    return None;
                };
                let img =
                    NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(symbol), None)?;
                Some(Retained::into_super(Retained::into_super(img)))
            }
        }
    }

    // VerticalFileSwitcher NM_CLICK: a click on a row switches to its tab.
    pub(crate) fn doc_list_click(&self) {
        let Some(d) = self.dock_ui() else { return };
        let row = d.docs.clickedRow();
        if let Some(t) = (row >= 0).then(|| self.tab(row as usize)).flatten() {
            self.tab_view().selectTabViewItem(Some(&t.item));
        }
    }

    fn file_key(&self) -> String {
        self.current()
            .and_then(|i| self.tab(i))
            .map(|t| t.path.map_or(t.name, |p| p.display().to_string()))
            .unwrap_or_default()
    }

    // FunctionListPanel::reload: parse the current tab with the parser of its language; keep sort and search per file.
    pub(crate) fn function_list_reload(&self) {
        self.doc_map_reload();
        if !self.panel_visible(RIGHT) {
            return;
        }
        let Some(d) = self.dock_ui() else { return };
        let old = d.key.replace(self.file_key());
        let mine = (
            d.sort.state() == NSControlStateValueOn,
            d.search.stringValue().to_string(),
        );
        d.state.borrow_mut().insert(old, mine);
        let (sorted, text) = d
            .state
            .borrow()
            .get(&*d.key.borrow())
            .cloned()
            .unwrap_or_default();
        d.sort.setState(if sorted {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        d.search.setStringValue(&ns(&text));
        self.function_list_build();
    }

    fn function_list_build(&self) {
        let Some(d) = self.dock_ui() else { return };
        let tab = self.current().and_then(|i| self.tab(i));
        let parsed = tab.as_ref().and_then(|t| {
            let lang = language::tab_language(t)?;
            let p = funclist::parser_for(&lang.name)?;
            let found = (sci::length(&t.view) <= PARSE_LIMIT).then(|| p.parse(&sci::doc(&t.view)));
            Some((t.name.clone(), found))
        });
        let mut items = vec![];
        let mut roots = vec![];
        if let Some((name, found)) = parsed {
            let mut nodes = match found {
                Some(found) => funclist::tree(&found),
                None => vec![Node {
                    label: "File too large for Function List".into(),
                    pos: -1,
                    children: vec![],
                }],
            };
            let text = d.search.stringValue().to_string();
            if !text.is_empty() {
                nodes = funclist::filter(&nodes, &text);
            }
            funclist::sort(&mut nodes, d.sort.state() == NSControlStateValueOn);
            let root = Node {
                label: name,
                pos: -1,
                children: nodes,
            };
            roots.push(flatten(&root, &mut items));
        }
        *d.items.borrow_mut() = items;
        *d.roots.borrow_mut() = roots.clone();
        d.funcs.reloadData();
        for r in roots {
            let obj = d.items.borrow().get(r).map(|it| it.obj.clone());
            unsafe { d.funcs.expandItem(obj.as_deref().map(|o| o as &AnyObject)) };
        }
        d.mark.set((1, 0));
        self.function_list_mark();
    }

    pub(crate) fn function_list_search(&self) {
        if self.panel_visible(RIGHT) {
            self.function_list_build();
        }
    }

    fn item_index(&self, item: Option<&AnyObject>) -> Option<usize> {
        item?
            .downcast_ref::<NSNumber>()
            .map(|n| n.integerValue() as usize)
    }

    fn with_children<R>(
        &self,
        item: Option<&AnyObject>,
        f: impl FnOnce(&[usize], &[Item]) -> R,
    ) -> Option<R> {
        let d = self.dock_ui()?;
        let items = d.items.borrow();
        match self.item_index(item) {
            Some(i) => items.get(i).map(|it| f(&it.children, &items)),
            None => Some(f(&d.roots.borrow(), &items)),
        }
    }

    pub(crate) fn function_list_count(&self, item: Option<&AnyObject>) -> isize {
        self.with_children(item, |c, _| c.len() as isize)
            .unwrap_or(0)
    }

    pub(crate) fn function_list_child(
        &self,
        n: isize,
        item: Option<&AnyObject>,
    ) -> Option<Retained<AnyObject>> {
        self.with_children(item, |c, items| {
            let it = items.get(*c.get(n as usize)?)?;
            Some(Retained::into_super(Retained::into_super(Retained::into_super(it.obj.clone()))))
        })
        .flatten()
    }

    pub(crate) fn function_list_expandable(&self, item: Option<&AnyObject>) -> bool {
        self.function_list_count(item) > 0
    }

    pub(crate) fn function_list_value(
        &self,
        item: Option<&AnyObject>,
    ) -> Option<Retained<AnyObject>> {
        let i = self.item_index(item)?;
        let label = self.dock_ui()?.items.borrow().get(i)?.label.clone();
        Some(Retained::into_super(Retained::into_super(ns(&label))))
    }

    // FunctionListPanel::openSelection: a double click on a function shows its position in the middle of the editor.
    pub(crate) fn function_list_open(&self) {
        let Some(d) = self.dock_ui() else { return };
        let row = d.funcs.clickedRow();
        let item = (row >= 0).then(|| d.funcs.itemAtRow(row)).flatten();
        let Some(i) = self.item_index(item.as_deref()) else {
            return;
        };
        let pos = match d.items.borrow().get(i) {
            Some(it) if it.children.is_empty() && it.pos >= 0 => it.pos,
            _ => return,
        };
        let Some(v) = self.editor() else { return };
        let line = sci::send(&v, sci::SCI_LINEFROMPOSITION, pos as usize, 0);
        sci::send(&v, SCI_ENSUREVISIBLE, line as usize, 0);
        sci::send(&v, SCI_GOTOPOS, pos as usize, 0);
        let first_display = sci::send(&v, SCI_GETFIRSTVISIBLELINE, 0, 0);
        let first = sci::send(&v, SCI_DOCLINEFROMVISIBLE, first_display as usize, 0);
        let n = sci::send(&v, SCI_LINESONSCREEN, 0, 0);
        let last = sci::send(&v, SCI_DOCLINEFROMVISIBLE, (first_display + n) as usize, 0);
        sci::send(&v, SCI_LINESCROLL, 0, center_scroll(line, first, last, n));
        sci::send(&v, SCI_ENSUREVISIBLEENFORCEPOLICY, line as usize, 0);
        self.focus();
    }

    // FunctionListPanel::markEntry: select the last function that starts on or before the caret line.
    pub(crate) fn function_list_mark(&self) {
        self.doc_map_scroll();
        if !self.panel_visible(RIGHT) {
            return;
        }
        let Some(d) = self.dock_ui() else { return };
        let Some(v) = self.editor() else { return };
        let caret = sci::selection(&v).1;
        let line = sci::send(&v, sci::SCI_LINEFROMPOSITION, caret as usize, 0);
        let (from, to) = d.mark.get();
        if from <= line && line < to {
            return;
        }
        let (best, parent, root, range) = {
            let items = d.items.borrow();
            let mut best: Option<(isize, usize, usize)> = None;
            let mut next = isize::MAX;
            for (k, it) in items.iter().enumerate() {
                for &c in &it.children {
                    let Some(leaf) = items.get(c) else { continue };
                    if !leaf.children.is_empty() || leaf.pos < 0 {
                        continue;
                    }
                    let l = sci::send(&v, sci::SCI_LINEFROMPOSITION, leaf.pos as usize, 0);
                    if l > line {
                        next = next.min(l);
                    } else if best.is_none_or(|b| l > b.0) {
                        best = Some((l, c, k));
                    }
                }
            }
            let obj = |k: usize| items.get(k).map(|it| it.obj.clone());
            (
                best.and_then(|b| obj(b.1)),
                best.and_then(|b| obj(b.2)),
                d.roots.borrow().first().and_then(|&r| obj(r)),
                (best.map_or(isize::MIN, |b| b.0), next),
            )
        };
        d.mark.set(range);
        if let Some(p) = parent {
            unsafe { d.funcs.expandItem(Some(&p)) };
        }
        let Some(target) = best.or(root) else { return };
        let row = unsafe { d.funcs.rowForItem(Some(&target)) };
        if row >= 0 && d.funcs.selectedRow() != row {
            d.funcs.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(row as usize),
                false,
            );
            d.funcs.scrollRowToVisible(row);
        }
    }
}

const SCI_GOTOPOS: u32 = 2025;
const SCI_GETFIRSTVISIBLELINE: u32 = 2152;
const SCI_LINESCROLL: u32 = 2168;
const SCI_DOCLINEFROMVISIBLE: u32 = 2221;
const SCI_ENSUREVISIBLE: u32 = 2232;
const SCI_ENSUREVISIBLEENFORCEPOLICY: u32 = 2234;
const SCI_LINESONSCREEN: u32 = 2370;

fn flatten(n: &Node, items: &mut Vec<Item>) -> usize {
    let k = items.len();
    items.push(Item {
        label: n.label.clone(),
        pos: n.pos,
        children: vec![],
        obj: NSNumber::new_isize(k as isize),
    });
    let children = n.children.iter().map(|c| flatten(c, items)).collect();
    items[k].children = children;
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions() {
        assert_eq!(split_ext("main.rs"), ("main", ".rs"));
        assert_eq!(split_ext("a.tar.gz"), ("a.tar", ".gz"));
        assert_eq!(split_ext("new 1"), ("new 1", ""));
        assert_eq!(split_ext("v1.0 notes"), ("v1.0 notes", ""));
        assert_eq!(split_ext(".bashrc"), ("", ".bashrc"));
    }

    #[test]
    fn centering() {
        assert_eq!(center_scroll(100, 0, 40, 40), 80);
        assert_eq!(center_scroll(10, 0, 40, 40), -10);
        assert_eq!(center_scroll(30, 20, 60, 40), -10);
        assert_eq!(center_scroll(5, 50, 80, 40), -55);
    }
}
