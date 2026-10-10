// SPDX-License-Identifier: GPL-3.0-or-later
use crate::funclist::{self, Node};
use crate::{language, ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSButton, NSControlStateValueOff, NSControlStateValueOn, NSImage,
    NSImageCell, NSOutlineView, NSScrollView, NSSearchField, NSSplitView, NSSplitViewDividerStyle,
    NSTableColumn, NSTableColumnResizingOptions, NSTableView, NSTableViewColumnAutoresizingStyle,
    NSTextField, NSView,
};
use objc2_foundation::{NSIndexSet, NSNumber, NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

pub const LEFT: isize = 0;
pub const RIGHT: isize = 1;
const TITLE_H: f64 = 24.;
const TOOLBAR_H: f64 = 28.;
const WIDTH: f64 = 250.;

struct Item {
    label: String,
    pos: isize,
    children: Vec<usize>,
    obj: Retained<NSNumber>,
}

// Two docking areas next to the editor: Document List on the left and Function List on the right, the Notepad++ defaults.
pub struct Dock {
    outer: Retained<NSSplitView>,
    panes: [Retained<NSView>; 2],
    widths: Cell<[f64; 2]>,
    docs: Retained<NSTableView>,
    funcs: Retained<NSOutlineView>,
    search: Retained<NSSearchField>,
    sort: Retained<NSButton>,
    items: RefCell<Vec<Item>>,
    roots: RefCell<Vec<usize>>,
    key: RefCell<String>,
    state: RefCell<HashMap<String, (bool, String)>>,
}

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

fn frame(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
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

fn scroll(mtm: MainThreadMarker, doc: &NSView, w: f64, h: f64) -> Retained<NSScrollView> {
    let s = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame(0., 0., w, h));
    s.setHasVerticalScroller(true);
    s.setAutohidesScrollers(true);
    s.setDocumentView(Some(doc));
    s.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    s
}

fn column(mtm: MainThreadMarker, id: &str, title: &str, w: f64) -> Retained<NSTableColumn> {
    let c = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns(id));
    c.setTitle(&ns(title));
    c.setWidth(w);
    c.setEditable(false);
    c
}

// A docked panel: a caption with the title and a close button above the content.
fn pane(
    mtm: MainThreadMarker,
    title: &str,
    side: isize,
    t: &AnyObject,
    h: f64,
) -> Retained<NSView> {
    let p = NSView::initWithFrame(NSView::alloc(mtm), frame(0., 0., WIDTH, h));
    let label = NSTextField::labelWithString(&ns(title), mtm);
    label.setFrame(frame(6., h - TITLE_H + 4., WIDTH - 32., 16.));
    label.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    p.addSubview(&label);
    let close = symbol_button(mtm, "xmark", "Close", t, sel!(dockClose:));
    close.setTag(side);
    close.setFrame(frame(WIDTH - 24., h - TITLE_H + 2., 20., 20.));
    close.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    p.addSubview(&close);
    p.setHidden(true);
    p
}

impl App {
    fn dock_ui(&self) -> Option<&Dock> {
        self.ivars().dock.get()
    }

    // Puts the editor area between the two docking areas and returns the view that holds all three.
    pub(crate) fn dock(&self, center: &NSView) -> Retained<NSSplitView> {
        let mtm = self.mtm();
        let t: &AnyObject = self;
        let f = center.frame();
        let h = f.size.height;
        let outer = NSSplitView::initWithFrame(NSSplitView::alloc(mtm), f);
        outer.setVertical(true);
        outer.setDividerStyle(NSSplitViewDividerStyle::Thin);
        outer.setAutoresizingMask(center.autoresizingMask());
        let left = pane(mtm, "Document List", LEFT, t, h);
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
        left.addSubview(&scroll(mtm, &docs, WIDTH, h - TITLE_H));
        let right = pane(mtm, "Function List", RIGHT, t, h);
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
        right.addSubview(&scroll(mtm, &funcs, WIDTH, h - TITLE_H - TOOLBAR_H));
        let search = NSSearchField::initWithFrame(
            NSSearchField::alloc(mtm),
            frame(4., h - TITLE_H - TOOLBAR_H + 3., WIDTH - 60., 22.),
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
                h - TITLE_H - TOOLBAR_H + 4.,
                20.,
                20.,
            ));
            b.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin | top);
            right.addSubview(b);
        }
        outer.addSubview(&left);
        outer.addSubview(center);
        outer.addSubview(&right);
        for (i, p) in [260., 250., 260.].iter().enumerate() {
            outer.setHoldingPriority_forSubviewAtIndex(*p, i as isize);
        }
        let _ = self.ivars().dock.set(Dock {
            outer: outer.clone(),
            panes: [left, right],
            widths: Cell::new([WIDTH, WIDTH]),
            docs,
            funcs,
            search,
            sort,
            items: RefCell::new(vec![]),
            roots: RefCell::new(vec![]),
            key: RefCell::new(String::new()),
            state: RefCell::new(HashMap::new()),
        });
        outer
    }

    pub(crate) fn panel_visible(&self, side: isize) -> bool {
        self.dock_ui()
            .and_then(|d| d.panes.get(side as usize))
            .is_some_and(|p| !p.isHidden())
    }

    // NppCommands.cpp IDM_VIEW_DOCLIST and IDM_VIEW_FUNC_LIST: show the panel, or close it when it shows.
    pub(crate) fn toggle_panel(&self, side: isize) {
        let Some(d) = self.dock_ui() else { return };
        let Some(p) = d.panes.get(side as usize) else {
            return;
        };
        let mut widths = d.widths.get();
        for (w, pane) in widths.iter_mut().zip(&d.panes) {
            if !pane.isHidden() {
                *w = pane.frame().size.width.max(80.);
            }
        }
        d.widths.set(widths);
        let show = p.isHidden();
        p.setHidden(!show);
        d.outer.adjustSubviews();
        let total = d.outer.frame().size.width;
        let gap = d.outer.dividerThickness();
        if !d.panes[0].isHidden() {
            d.outer.setPosition_ofDividerAtIndex(widths[0], 0);
        }
        if !d.panes[1].isHidden() {
            d.outer.setPosition_ofDividerAtIndex(total - widths[1] - gap, 1);
        }
        if show {
            self.doc_list_reload();
            self.function_list_reload();
        } else {
            self.focus();
        }
    }

    // VerticalFileSwitcher: the open documents in tab order, with the current one selected.
    pub(crate) fn doc_list_reload(&self) {
        if !self.panel_visible(LEFT) {
            return;
        }
        let d = self.dock_ui().unwrap();
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
        if !self.panel_visible(RIGHT) {
            return;
        }
        let d = self.dock_ui().unwrap();
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
        let d = self.dock_ui().unwrap();
        let tab = self.current().and_then(|i| self.tab(i));
        let parsed = tab.as_ref().and_then(|t| {
            let lang = language::tab_language(t)?;
            let p = funclist::parser_for(&lang.name)?;
            Some((t.name.clone(), p.parse(&sci::doc(&t.view))))
        });
        let mut items = vec![];
        let mut roots = vec![];
        if let Some((name, found)) = parsed {
            let mut nodes = funclist::tree(&found);
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
            let obj = d.items.borrow()[r].obj.clone();
            unsafe { d.funcs.expandItem(Some(&obj)) };
        }
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
            c.get(n as usize)
                .map(|&k| Retained::into_super(Retained::into_super(Retained::into_super(items[k].obj.clone()))))
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
        if !self.panel_visible(RIGHT) {
            return;
        }
        let d = self.dock_ui().unwrap();
        let Some(v) = self.editor() else { return };
        let caret = sci::selection(&v).1;
        let line = sci::send(&v, sci::SCI_LINEFROMPOSITION, caret as usize, 0);
        let (best, parent, root) = {
            let items = d.items.borrow();
            let mut best: Option<(isize, usize, Option<usize>)> = None;
            for (k, it) in items.iter().enumerate() {
                for &c in &it.children {
                    let leaf = &items[c];
                    if !leaf.children.is_empty() || leaf.pos < 0 {
                        continue;
                    }
                    let l = sci::send(&v, sci::SCI_LINEFROMPOSITION, leaf.pos as usize, 0);
                    if l <= line && best.is_none_or(|b| l > b.0) {
                        best = Some((l, c, Some(k)));
                    }
                }
            }
            let obj = |k: usize| items[k].obj.clone();
            (
                best.map(|b| obj(b.1)),
                best.and_then(|b| b.2).map(obj),
                d.roots.borrow().first().map(|&r| obj(r)),
            )
        };
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
