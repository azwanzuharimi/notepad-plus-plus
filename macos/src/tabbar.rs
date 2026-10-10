// SPDX-License-Identifier: GPL-3.0-or-later
use crate::views::{MAIN, SUB};
use crate::{prefs, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityElement, NSAccessibilityRadioButtonRole, NSApplication,
    NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSEvent, NSEventMask, NSEventModifierFlags,
    NSEventType, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSGraphicsContext, NSImage, NSImageSymbolConfiguration,
    NSMenu, NSMenuItem, NSStringDrawing, NSTabView, NSTabViewItem, NSView,
};
use objc2_foundation::{NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};

pub const BAR_H: f64 = 24.;
const PAD: f64 = 8.;
const ICON: f64 = 8.;
const GAP: f64 = 5.;
const CLOSE: f64 = 14.;
const PIN: f64 = 12.;
const MIN_W: f64 = 48.;
const MAX_W: f64 = 240.;
const ARROW_W: f64 = 18.;
const TOP_BAR: f64 = 3.;
const FONT: f64 = 11.;

// DocTabView.cpp tab images: saved, unsaved, read-only, monitoring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Saved,
    Unsaved,
    ReadOnly,
    Monitoring,
}

// The "TabBar" GUIConfig options that the bar uses.
#[derive(Clone, Copy)]
pub struct Opts {
    pub close: bool,
    pub dbl_close: bool,
    pub top_bar: bool,
    pub inactive: bool,
    pub drag: bool,
    pub pin: bool,
}

pub fn opts() -> Opts {
    let p = prefs::get();
    Opts {
        close: p.tab_close,
        dbl_close: p.tab_dbl_close,
        top_bar: p.tab_top_bar,
        inactive: p.tab_inactive,
        drag: p.tab_drag,
        pin: p.tab_pin,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub i: usize,
    pub x: f64,
    pub w: f64,
}

pub fn tab_width(text: f64, close: bool, pin: bool) -> f64 {
    let close = if close { GAP + CLOSE } else { 0. };
    let pin = if pin { GAP + PIN } else { 0. };
    (PAD + ICON + GAP + text + pin + close + PAD).clamp(MIN_W, MAX_W)
}

// The tabs from `first` that fit in `width`; when not all tabs fit, two arrows take the right end.
pub fn layout(widths: &[f64], first: usize, width: f64) -> (Vec<Slot>, bool) {
    let arrows = widths.iter().sum::<f64>() > width;
    let (first, avail) = if arrows {
        (
            first.min(widths.len().saturating_sub(1)),
            width - 2. * ARROW_W,
        )
    } else {
        (0, width)
    };
    let mut x = 0.;
    let mut out = vec![];
    for (i, &w) in widths.iter().enumerate().skip(first) {
        if x + w > avail && !out.is_empty() {
            break;
        }
        out.push(Slot { i, x, w });
        x += w;
    }
    (out, arrows)
}

pub fn hit(slots: &[Slot], x: f64) -> Option<Slot> {
    slots.iter().copied().find(|s| x >= s.x && x < s.x + s.w)
}

pub fn close_x(s: &Slot) -> f64 {
    s.x + s.w - PAD - CLOSE
}

pub fn on_close(s: &Slot, x: f64) -> bool {
    let c = close_x(s);
    x >= c && x < c + CLOSE
}

pub fn pin_x(s: &Slot, close: bool) -> f64 {
    if close {
        close_x(s) - GAP - PIN
    } else {
        s.x + s.w - PAD - PIN
    }
}

pub fn on_pin(s: &Slot, close: bool, x: f64) -> bool {
    let p = pin_x(s, close);
    x >= p && x < p + PIN
}

// TabBarPlus: a pinned tab stays among the pinned tabs at the start, an unpinned tab stays after them.
pub fn pin_drop(pinned: &[bool], from: usize, slot: usize) -> usize {
    let n = pinned.iter().filter(|&&p| p).count();
    if pinned.get(from).copied().unwrap_or(false) {
        slot.min(n)
    } else {
        slot.max(n)
    }
}

// The first visible tab after a scroll that shows tab `sel`.
pub fn scroll_to(widths: &[f64], first: usize, sel: usize, width: f64) -> usize {
    let mut f = first.min(sel);
    loop {
        let (s, _) = layout(widths, f, width);
        if f >= sel || s.iter().any(|s| s.i == sel) {
            return f;
        }
        f += 1;
    }
}

// The insert position (0 to n) for a drop at `x`: before the first tab whose middle is right of `x`.
pub fn drop_slot(slots: &[Slot], n: usize, x: f64) -> usize {
    slots
        .iter()
        .find(|s| x < s.x + s.w / 2.)
        .map(|s| s.i)
        .unwrap_or_else(|| slots.last().map_or(n, |s| s.i + 1))
}

// The new index of a tab that moves from `from` to insert position `slot` in the same list.
pub fn moved_to(from: usize, slot: usize) -> usize {
    if slot > from {
        slot - 1
    } else {
        slot
    }
}

thread_local! {
    static PINNED: RefCell<Vec<Retained<NSTabViewItem>>> = const { RefCell::new(vec![]) };
}

#[derive(Default)]
pub struct BarIvars {
    first: Cell<usize>,
    slots: RefCell<Vec<Slot>>,
    arrows: Cell<bool>,
    shown_sel: Cell<Option<usize>>,
    ax: RefCell<Vec<Retained<NSAccessibilityElement>>>,
}

define_class!(
    // TabBarPlus: draws the tabs of the tab view that holds it, and sends clicks, drags and closes to the app.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = BarIvars]
    #[name = "NppTabBar"]
    pub struct TabBar;

    impl TabBar {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _e: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _r: NSRect) {
            self.draw();
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, e: &NSEvent) {
            self.clicked(e);
        }

        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, e: &NSEvent) {
            if e.buttonNumber() == 2 {
                if let Some(s) = self.slot_at(e) {
                    self.close(s.i);
                }
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, e: &NSEvent) {
            let d = if e.scrollingDeltaX().abs() > e.scrollingDeltaY().abs() {
                -e.scrollingDeltaX()
            } else {
                -e.scrollingDeltaY()
            };
            if d != 0. {
                self.scroll(if d > 0. { 1 } else { -1 });
            }
        }

        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, e: &NSEvent) -> Option<Retained<NSMenu>> {
            self.menu_at(e)
        }
    }
);

fn app(mtm: MainThreadMarker) -> Option<Retained<App>> {
    let d = NSApplication::sharedApplication(mtm).delegate()?;
    Some(unsafe { Retained::cast_unchecked(d) })
}

fn font() -> Retained<NSFont> {
    NSFont::systemFontOfSize(FONT)
}

fn attrs(color: &NSColor) -> Retained<NSDictionary<NSString, AnyObject>> {
    let f = font();
    let keys = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let vals: [&AnyObject; 2] = [f.as_ref(), color.as_ref()];
    NSDictionary::from_slices(&keys, &vals)
}

fn rgb(r: f64, g: f64, b: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.)
}

fn draw_pin(pinned: bool, x: f64) {
    let name = NSString::from_str(if pinned { "pin.fill" } else { "pin" });
    let Some(img) = NSImage::imageWithSystemSymbolName_accessibilityDescription(&name, None) else {
        return;
    };
    let color = if pinned {
        rgb(250. / 255., 170. / 255., 60. / 255.)
    } else {
        NSColor::tertiaryLabelColor()
    };
    let cfg = NSImageSymbolConfiguration::configurationWithHierarchicalColor(&color);
    if let Some(img) = img.imageWithSymbolConfiguration(&cfg) {
        img.drawInRect(NSRect::new(
            NSPoint::new(x, (BAR_H - PIN) / 2.),
            NSSize::new(PIN, PIN),
        ));
    }
}

fn state_color(s: State) -> Retained<NSColor> {
    match s {
        State::Saved => rgb(0.2, 0.45, 0.85),
        State::Unsaved => rgb(0.85, 0.2, 0.2),
        State::ReadOnly => rgb(0.55, 0.55, 0.55),
        State::Monitoring => rgb(0.2, 0.65, 0.3),
    }
}

impl TabBar {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BarIvars::default());
        let r = NSRect::new(NSPoint::new(0., 0.), NSSize::new(100., BAR_H));
        let bar: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: r] };
        bar.setAccessibilityElement(false);
        bar
    }

    // Puts the bar at the top edge of the tab view.
    pub fn place(&self, tv: &NSTabView) {
        let b = tv.bounds();
        let flipped = tv.isFlipped();
        let y = if flipped { 0. } else { b.size.height - BAR_H };
        self.setFrame(NSRect::new(
            NSPoint::new(0., y),
            NSSize::new(b.size.width, BAR_H),
        ));
        self.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | if flipped {
                    NSAutoresizingMaskOptions::ViewMaxYMargin
                } else {
                    NSAutoresizingMaskOptions::ViewMinYMargin
                },
        );
    }

    fn tab_view(&self) -> Option<Retained<NSTabView>> {
        unsafe { self.superview() }?.downcast::<NSTabView>().ok()
    }

    fn items(&self) -> Vec<Retained<NSTabViewItem>> {
        self.tab_view()
            .map(|tv| tv.tabViewItems().to_vec())
            .unwrap_or_default()
    }

    fn selected(&self) -> Option<usize> {
        let tv = self.tab_view()?;
        let s = tv.selectedTabViewItem()?;
        usize::try_from(tv.indexOfTabViewItem(&s)).ok()
    }

    fn labels(&self) -> Vec<(String, State, bool)> {
        let items = self.items();
        let app = app(self.mtm());
        items
            .iter()
            .map(|it| match &app {
                Some(a) => a.tab_bar_info(it),
                None => (it.label().to_string(), State::Saved, false),
            })
            .collect()
    }

    fn widths(&self, labels: &[(String, State, bool)]) -> Vec<f64> {
        let a = attrs(&NSColor::labelColor());
        let o = opts();
        labels
            .iter()
            .map(|(l, _, _)| {
                let w = unsafe { NSString::from_str(l).sizeWithAttributes(Some(&a)) }.width;
                tab_width(w.ceil(), o.close, o.pin)
            })
            .collect()
    }

    fn relayout(&self, labels: &[(String, State, bool)]) -> Vec<Slot> {
        let ws = self.widths(labels);
        let width = self.bounds().size.width;
        let mut first = self.ivars().first.get();
        let sel = self
            .selected()
            .filter(|&s| self.ivars().shown_sel.replace(Some(s)) != Some(s));
        if let Some(sel) = sel {
            let (s, _) = layout(&ws, first, width);
            if !s.iter().any(|s| s.i == sel) {
                first = if sel < first {
                    sel
                } else {
                    scroll_to(&ws, first, sel, width)
                };
            }
        }
        let (slots, arrows) = layout(&ws, first, width);
        self.ivars().first.set(slots.first().map_or(0, |s| s.i));
        self.ivars().arrows.set(arrows);
        *self.ivars().slots.borrow_mut() = slots.clone();
        slots
    }

    fn draw(&self) {
        let labels = self.labels();
        let slots = self.relayout(&labels);
        let o = opts();
        let b = self.bounds();
        NSColor::windowBackgroundColor().set();
        NSBezierPath::fillRect(b);
        let sel = self.selected();
        let focused = self.is_active_view();
        for s in &slots {
            let Some((label, state, pinned)) = labels.get(s.i) else {
                continue;
            };
            let r = NSRect::new(NSPoint::new(s.x, 0.), NSSize::new(s.w, BAR_H));
            let active = sel == Some(s.i);
            if active {
                NSColor::controlBackgroundColor().set();
                NSBezierPath::fillRect(r);
                if o.top_bar {
                    if focused {
                        rgb(250. / 255., 170. / 255., 60. / 255.).set();
                    } else {
                        rgb(250. / 255., 210. / 255., 150. / 255.).set();
                    }
                    NSBezierPath::fillRect(NSRect::new(r.origin, NSSize::new(s.w, TOP_BAR)));
                }
            } else if o.inactive {
                NSColor::systemGrayColor()
                    .colorWithAlphaComponent(0.15)
                    .set();
                NSBezierPath::fillRect(r);
            }
            NSColor::separatorColor().set();
            NSBezierPath::fillRect(NSRect::new(
                NSPoint::new(s.x + s.w - 1., 0.),
                NSSize::new(1., BAR_H),
            ));
            state_color(*state).set();
            NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                NSPoint::new(s.x + PAD, (BAR_H - ICON) / 2.),
                NSSize::new(ICON, ICON),
            ))
            .fill();
            let fg = if active || !o.inactive {
                NSColor::labelColor()
            } else {
                NSColor::secondaryLabelColor()
            };
            let a = attrs(&fg);
            let tx = s.x + PAD + ICON + GAP;
            let tw = if o.pin {
                pin_x(s, o.close) - GAP
            } else if o.close {
                close_x(s) - GAP
            } else {
                s.x + s.w - PAD
            } - tx;
            if o.pin {
                draw_pin(*pinned, pin_x(s, o.close));
            }
            NSGraphicsContext::saveGraphicsState_class();
            NSBezierPath::clipRect(NSRect::new(
                NSPoint::new(tx, 0.),
                NSSize::new(tw.max(0.), BAR_H),
            ));
            unsafe {
                NSString::from_str(label)
                    .drawAtPoint_withAttributes(NSPoint::new(tx, (BAR_H - 14.) / 2.), Some(&a))
            };
            NSGraphicsContext::restoreGraphicsState_class();
            if o.close {
                let c = attrs(&NSColor::secondaryLabelColor());
                unsafe {
                    NSString::from_str("\u{2715}").drawAtPoint_withAttributes(
                        NSPoint::new(close_x(s) + 2., (BAR_H - 14.) / 2.),
                        Some(&c),
                    )
                };
            }
        }
        if self.ivars().arrows.get() {
            let a = attrs(&NSColor::labelColor());
            for (k, t) in ["\u{25C0}", "\u{25B6}"].iter().enumerate() {
                let x = b.size.width - (2 - k) as f64 * ARROW_W + 4.;
                unsafe {
                    NSString::from_str(t)
                        .drawAtPoint_withAttributes(NSPoint::new(x, (BAR_H - 14.) / 2.), Some(&a))
                };
            }
        }
    }

    fn is_active_view(&self) -> bool {
        let (Some(a), Some(tv)) = (app(self.mtm()), self.tab_view()) else {
            return false;
        };
        std::ptr::eq(a.doc_tabs(a.active_view()), &*tv)
    }

    // The tab under the cursor is selected before the tab menu opens.
    fn menu_at(&self, e: &NSEvent) -> Option<Retained<NSMenu>> {
        let tv = self.tab_view()?;
        if let Some(s) = self.slot_at(e) {
            self.select(&tv, s.i);
        }
        tv.menu()
    }

    fn local(&self, e: &NSEvent) -> NSPoint {
        self.convertPoint_fromView(e.locationInWindow(), None)
    }

    fn slot_at(&self, e: &NSEvent) -> Option<Slot> {
        let labels = self.labels();
        let slots = self.relayout(&labels);
        hit(&slots, self.local(e).x)
    }

    fn scroll(&self, by: isize) {
        if !self.ivars().arrows.get() {
            return;
        }
        let n = self.items().len().saturating_sub(1) as isize;
        let f = (self.ivars().first.get() as isize + by).clamp(0, n);
        self.ivars().first.set(f as usize);
        self.setNeedsDisplay(true);
    }

    fn select(&self, tv: &NSTabView, i: usize) {
        if let Some(it) = self.items().get(i) {
            tv.selectTabViewItem(Some(it));
        }
    }

    fn close(&self, i: usize) {
        let Some(tv) = self.tab_view() else { return };
        self.select(&tv, i);
        let app = NSApplication::sharedApplication(self.mtm());
        unsafe { app.sendAction_to_from(sel!(closeTab:), None, Some(self)) };
    }

    fn clicked(&self, e: &NSEvent) {
        let Some(tv) = self.tab_view() else { return };
        let p = self.local(e);
        let labels = self.labels();
        let slots = self.relayout(&labels);
        let width = self.bounds().size.width;
        if self.ivars().arrows.get() && p.x >= width - 2. * ARROW_W {
            self.scroll(if p.x < width - ARROW_W { -1 } else { 1 });
            return;
        }
        let o = opts();
        let n = e.clickCount();
        let Some(s) = hit(&slots, p.x) else {
            if n == 2 {
                let app = NSApplication::sharedApplication(self.mtm());
                unsafe { app.sendAction_to_from(sel!(newDocument:), None, Some(self)) };
            }
            return;
        };
        if (o.close && on_close(&s, p.x)) || (n == 2 && o.dbl_close) {
            self.close(s.i);
            return;
        }
        if o.pin && on_pin(&s, o.close, p.x) {
            self.select(&tv, s.i);
            let app = NSApplication::sharedApplication(self.mtm());
            unsafe { app.sendAction_to_from(sel!(pinTab:), None, Some(self)) };
            return;
        }
        self.select(&tv, s.i);
        if o.drag && n == 1 {
            self.track_drag(e, s.i);
        }
    }

    fn track_drag(&self, e: &NSEvent, i: usize) {
        let Some(w) = self.window() else { return };
        let start = e.locationInWindow();
        let mut dragging = false;
        loop {
            let Some(ev) =
                w.nextEventMatchingMask(NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp)
            else {
                return;
            };
            let at = ev.locationInWindow();
            if ev.r#type() == NSEventType::LeftMouseUp {
                if dragging {
                    let clone = ev.modifierFlags().contains(NSEventModifierFlags::Option);
                    if let (Some(a), Some(tv)) = (app(self.mtm()), self.tab_view()) {
                        a.tab_bar_drop(&tv, i, at, clone);
                    }
                }
                return;
            }
            if (at.x - start.x).abs() > 4. || (at.y - start.y).abs() > 4. {
                dragging = true;
            }
        }
    }

    // The insert position in this bar for a drop at window point `at`.
    pub fn drop_index(&self, at: NSPoint) -> usize {
        let labels = self.labels();
        let slots = self.relayout(&labels);
        drop_slot(&slots, labels.len(), self.convertPoint_fromView(at, None).x)
    }

    // AXRadioButton elements for the tabs, with `parent` (the tab view) as their parent.
    pub fn ax_tabs(&self, parent: &NSView) -> Vec<Retained<AnyObject>> {
        if self.isHidden() {
            return vec![];
        }
        let labels = self.labels();
        let slots = self.relayout(&labels);
        let sel = self.selected();
        let mut ax = self.ivars().ax.borrow_mut();
        while ax.len() < slots.len() {
            let el = NSAccessibilityElement::new();
            el.setAccessibilityElement(true);
            el.setAccessibilityRole(Some(unsafe { NSAccessibilityRadioButtonRole }));
            ax.push(el);
        }
        slots
            .iter()
            .zip(ax.iter())
            .filter_map(|(s, el)| {
                let (l, _, _) = labels.get(s.i)?;
                let r = NSRect::new(NSPoint::new(s.x, 0.), NSSize::new(s.w, BAR_H));
                el.setAccessibilityTitle(Some(&NSString::from_str(l)));
                unsafe {
                    el.setAccessibilityParent(Some(parent));
                    el.setAccessibilityValue(Some(&NSNumber::new_bool(sel == Some(s.i))));
                }
                let mut r = self.convertRect_toView(r, Some(parent));
                r.origin.y = parent.bounds().size.height - r.origin.y - r.size.height;
                el.setAccessibilityFrameInParentSpace(r);
                Some(Retained::into_super(Retained::into_super(el.clone())))
            })
            .collect()
    }
}

impl App {
    // The tab label without the modified mark, and the DocTabView.cpp image state.
    pub(crate) fn tab_bar_info(&self, item: &NSTabViewItem) -> (String, State, bool) {
        let pinned = self.item_pinned(item);
        let t = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .find(|t| std::ptr::eq(&*t.item, item))
            .cloned();
        let Some(t) = t else {
            return (item.label().to_string(), State::Saved, pinned);
        };
        let state = if self.monitored(item) {
            State::Monitoring
        } else if t.ro {
            State::ReadOnly
        } else if self.dirty(&t) {
            State::Unsaved
        } else {
            State::Saved
        };
        (item.label().to_string(), state, pinned)
    }

    // Buffer::isPinned: a tab is pinned when it or a clone of it is pinned.
    fn item_pinned(&self, item: &NSTabViewItem) -> bool {
        let i = self
            .ivars()
            .tabs
            .borrow()
            .iter()
            .position(|t| std::ptr::eq(&*t.item, item));
        i.is_some_and(|i| self.pinned_at(i))
    }

    pub(crate) fn pinned_at(&self, i: usize) -> bool {
        let items = self.with_clones(i);
        PINNED.with(|p| {
            p.borrow()
                .iter()
                .any(|x| items.iter().any(|it| std::ptr::eq(&**x, &**it)))
        })
    }

    // NppNotification.cpp TCN_TABPINNED: a pinned tab goes to the start of its view, an unpinned tab to the end.
    pub(crate) fn toggle_pin(&self) {
        let Some(i) = self.current() else { return };
        let items = self.with_clones(i);
        let was = self.pinned_at(i);
        let tabs: Vec<Retained<NSTabViewItem>> =
            self.ivars().tabs.borrow().iter().map(|t| t.item.clone()).collect();
        PINNED.with(|p| {
            let mut p = p.borrow_mut();
            p.retain(|x| tabs.iter().any(|t| std::ptr::eq(&**t, &**x)));
            if was {
                p.retain(|x| !items.iter().any(|it| std::ptr::eq(&**x, &**it)));
            } else if let Some(it) = items.first() {
                p.push(it.clone());
            }
        });
        let r = self.view_range(self.pane_of(i));
        let to = if was { r.end - 1 } else { r.start };
        if to != i {
            self.move_tab_to(i, to);
        }
        self.tab_bars_redraw();
    }

    // NppIO.cpp fileCloseAllButPinned.
    pub(crate) fn close_all_but_pinned(&self) {
        let n = self.ivars().tabs.borrow().len();
        let items: Vec<Retained<NSTabViewItem>> = (0..n)
            .filter(|&i| !self.pinned_at(i))
            .filter_map(|i| self.tab(i).map(|t| t.item))
            .collect();
        if items.is_empty() || !self.confirm_close_all(&items) {
            return;
        }
        self.drop_tabs(&items);
    }

    pub(crate) fn validate_tabbar(&self, item: &NSMenuItem) -> Option<bool> {
        let a = item.action()?;
        if a == sel!(pinTab:) {
            let pinned = self.current().is_some_and(|i| self.pinned_at(i));
            item.setTitle(&NSString::from_str(if pinned { "Unpin Tab" } else { "Pin Tab" }));
            Some(opts().pin && self.current().is_some())
        } else if a == sel!(closeAllButPinned:) {
            Some(self.current().is_some())
        } else {
            None
        }
    }

    pub(crate) fn tab_bars_redraw(&self) {
        for p in [MAIN, SUB] {
            if let Some(b) = crate::views::bar_of(self.doc_tabs(p)) {
                b.setNeedsDisplay(true);
            }
        }
    }

    pub(crate) fn tab_bars_apply(&self) {
        for p in [MAIN, SUB] {
            if let Some(d) = self.doc_tabs(p).downcast_ref::<crate::views::DocTabs>() {
                d.show_bar();
            }
        }
    }

    // TabBarPlus drag and drop: inside a view the tab moves; to the other view it moves, or with Option it clones.
    pub(crate) fn tab_bar_drop(&self, src: &NSTabView, i: usize, at: NSPoint, clone: bool) {
        let from_p = if std::ptr::eq(self.doc_tabs(SUB), src) {
            SUB
        } else {
            MAIN
        };
        if i >= self.view_range(from_p).len() {
            return;
        }
        let target = [MAIN, SUB].into_iter().find(|&p| {
            let tv = self.doc_tabs(p);
            unsafe { tv.superview() }.is_some() && {
                let lp = tv.convertPoint_fromView(at, None);
                let b = tv.bounds();
                lp.x >= 0. && lp.y >= 0. && lp.x < b.size.width && lp.y < b.size.height
            }
        });
        let Some(to_p) = target else { return };
        let Some(bar) = crate::views::bar_of(self.doc_tabs(to_p)) else {
            return;
        };
        let slot = bar.drop_index(at);
        let from = self.view_range(from_p).start + i;
        if to_p == from_p {
            let pinned: Vec<bool> = self.view_range(to_p).map(|k| self.pinned_at(k)).collect();
            let to = moved_to(i, pin_drop(&pinned, i, slot));
            if to != i {
                self.move_tab_to(from, self.view_range(to_p).start + to);
            }
            return;
        }
        if let Some(t) = self.tab(from) {
            src.selectTabViewItem(Some(&t.item));
        }
        if clone {
            self.clone_to_other_view();
        } else {
            self.move_to_other_view();
        }
        let r = self.view_range(to_p);
        if let Some(cur) = self.current().filter(|c| r.contains(c)) {
            let to = r.start + slot.min(r.len() - 1);
            if to != cur {
                self.move_tab_to(cur, to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_and_layout() {
        assert_eq!(tab_width(0., false, false), MIN_W);
        assert_eq!(tab_width(1000., true, true), MAX_W);
        assert_eq!(
            tab_width(50., true, true) - tab_width(50., true, false),
            GAP + PIN
        );
        assert_eq!(
            tab_width(50., true, false),
            PAD + ICON + GAP + 50. + GAP + CLOSE + PAD
        );
        let ws = [100., 100., 100.];
        let (s, arrows) = layout(&ws, 2, 400.);
        assert!(!arrows);
        assert_eq!(
            s.iter().map(|s| (s.i, s.x)).collect::<Vec<_>>(),
            [(0, 0.), (1, 100.), (2, 200.)]
        );
        let (s, arrows) = layout(&ws, 0, 250.);
        assert!(arrows);
        assert_eq!(s.iter().map(|s| s.i).collect::<Vec<_>>(), [0, 1]);
        let (s, _) = layout(&ws, 1, 250.);
        assert_eq!(
            s.iter().map(|s| (s.i, s.x)).collect::<Vec<_>>(),
            [(1, 0.), (2, 100.)]
        );
        let (s, _) = layout(&ws, 9, 250.);
        assert_eq!(s.iter().map(|s| s.i).collect::<Vec<_>>(), [2]);
        let (s, _) = layout(&[300.], 0, 100.);
        assert_eq!(s.len(), 1);
        assert_eq!(layout(&[], 0, 100.), (vec![], false));
    }

    #[test]
    fn hit_testing() {
        let (s, _) = layout(&[100., 60.], 0, 400.);
        assert_eq!(hit(&s, 0.).map(|s| s.i), Some(0));
        assert_eq!(hit(&s, 99.9).map(|s| s.i), Some(0));
        assert_eq!(hit(&s, 100.).map(|s| s.i), Some(1));
        assert_eq!(hit(&s, 160.), None);
        let t = s[0];
        assert!(on_close(&t, 100. - PAD - 1.));
        assert!(!on_close(&t, 100. - PAD));
        assert!(!on_close(&t, 50.));
        assert!(on_pin(&t, true, close_x(&t) - GAP - 1.));
        assert!(!on_pin(&t, true, close_x(&t)));
        assert!(on_pin(&t, false, 100. - PAD - 1.));
    }

    #[test]
    fn overflow_scroll() {
        let ws = [100.; 6];
        assert_eq!(scroll_to(&ws, 0, 1, 250.), 0);
        assert_eq!(scroll_to(&ws, 0, 4, 250.), 3);
        assert_eq!(scroll_to(&ws, 4, 2, 250.), 2);
        assert_eq!(scroll_to(&ws, 0, 5, 1000.), 0);
    }

    #[test]
    fn drop_positions() {
        let (s, _) = layout(&[100., 100., 100.], 0, 400.);
        assert_eq!(drop_slot(&s, 3, 10.), 0);
        assert_eq!(drop_slot(&s, 3, 60.), 1);
        assert_eq!(drop_slot(&s, 3, 249.), 2);
        assert_eq!(drop_slot(&s, 3, 390.), 3);
        assert_eq!(drop_slot(&[], 0, 5.), 0);
        let (s, _) = layout(&[100.; 6], 2, 250.);
        assert_eq!(drop_slot(&s, 6, 240.), 4);
        assert_eq!(moved_to(0, 3), 2);
        assert_eq!(moved_to(2, 0), 0);
        assert_eq!(moved_to(1, 1), 1);
        assert_eq!(moved_to(1, 2), 1);
    }

    #[test]
    fn pinned_tabs_stay_first() {
        let pinned = [true, true, false, false];
        assert_eq!(pin_drop(&pinned, 0, 4), 2);
        assert_eq!(pin_drop(&pinned, 1, 0), 0);
        assert_eq!(pin_drop(&pinned, 3, 0), 2);
        assert_eq!(pin_drop(&pinned, 2, 4), 4);
        assert_eq!(pin_drop(&[false, false], 0, 0), 0);
        assert_eq!(moved_to(0, pin_drop(&pinned, 0, 4)), 1);
    }
}
