// SPDX-License-Identifier: GPL-3.0-or-later
use crate::docking::{content_box, fill, frame, DOC_MAP};
use crate::{sci, App};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSEvent, NSView};
use objc2_foundation::NSRect;
use std::cell::Cell;

const SCI_GOTOPOS: u32 = 2025;
const SCI_POSITIONFROMPOINT: u32 = 2022;
const SCI_POINTYFROMPOSITION: u32 = 2165;
const SCI_LINESCROLL: u32 = 2168;
const SCI_FOLDLINE: u32 = 2237;
const SCI_SETMARGINWIDTHN: u32 = 2242;
const SCI_SETWRAPMODE: u32 = 2268;
const SCI_GETWRAPMODE: u32 = 2269;
const SCI_TEXTHEIGHT: u32 = 2279;
const SCI_SETVSCROLLBAR: u32 = 2280;
const SCI_SETHSCROLLBAR: u32 = 2130;
const SCI_GETDOCPOINTER: u32 = 2357;
const SCI_SETDOCPOINTER: u32 = 2358;
const SCI_SETMODEVENTMASK: u32 = 2359;
const SCI_SETZOOM: u32 = 2373;
const SCI_GETZOOM: u32 = 2374;
const SCI_SETWRAPINDENTMODE: u32 = 2472;
const SCI_GETWRAPINDENTMODE: u32 = 2473;
const SCI_SETCARETSTYLE: u32 = 2512;
const SCI_CONTRACTEDFOLDNEXT: u32 = 2618;
const SCI_FOLDALL: u32 = 2662;
const SCI_STYLEGETFONT: u32 = 2486;
const SCI_STYLESETFONT: u32 = 2056;
const SC_FOLDACTION_CONTRACT: isize = 0;
const SC_FOLDACTION_EXPAND: usize = 1;
// Style properties the map copies from the editor: (get, set).
const STYLE_PROPS: [(u32, u32); 6] = [
    (2481, 2051),
    (2482, 2052),
    (2484, 2054),
    (2487, 2057),
    (2062, 2061),
    (2064, 2063),
];
// documentMap.cpp zoomRatio: editor text width / map width for the editor zooms -10 to 20.
const ZOOM_RATIO: [f64; 31] = [
    1., 1., 1., 1., 1.5, 2., 2.5, 2.5, 3.5, 3.5, 4., 4.5, 5., 5., 5.5, 6., 6.5, 7., 7., 7.5, 8.,
    8.5, 8.5, 9.5, 9.5, 10., 10.5, 11., 11., 11.5, 12.,
];
// ViewZoneDlg: orange zone on white, with SetLayeredWindowAttributes alpha 50 of 255.
const ALPHA: f64 = 50. / 255.;

pub struct Map {
    view: Retained<NSView>,
    zone: Retained<ViewZone>,
    wrap: Cell<Option<(bool, isize, i64, i64)>>,
}

struct ZoneIvars {
    app: Retained<App>,
    zone: Cell<(f64, f64)>,
}

define_class!(
    // ViewZoneDlg: the translucent layer over the map that marks what the editor shows; a click or a drag scrolls the editor.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ZoneIvars]
    struct ViewZone;

    impl ViewZone {
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
            let b = self.bounds();
            NSColor::colorWithSRGBRed_green_blue_alpha(1., 1., 1., ALPHA).set();
            NSBezierPath::fillRect(b);
            let (top, bottom) = self.ivars().zone.get();
            let mut z = b;
            z.origin.y = top;
            z.size.height = (bottom - top).max(0.);
            NSColor::colorWithSRGBRed_green_blue_alpha(1., 0.5, 0., ALPHA).set();
            NSBezierPath::fillRect(z);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, e: &NSEvent) {
            self.clicked(e);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, e: &NSEvent) {
            self.clicked(e);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, e: &NSEvent) {
            let scroller = self.ivars().app.editor().and_then(|v| sci::content(&v).enclosingScrollView());
            if let Some(s) = scroller {
                s.scrollWheel(e);
            }
        }
    }
);

impl ViewZone {
    fn new(mtm: MainThreadMarker, app: Retained<App>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ZoneIvars {
            app,
            zone: Cell::new((0., 0.)),
        });
        unsafe { msg_send![super(this), initWithFrame: frame(0., 0., 100., 100.)] }
    }

    fn clicked(&self, e: &NSEvent) {
        let p = self.convertPoint_fromView(e.locationInWindow(), None);
        self.ivars().app.doc_map_click(p.y);
    }
}

fn contracted(v: &NSView) -> Vec<isize> {
    let mut out = vec![];
    let mut line = 0;
    loop {
        let next = sci::send(v, SCI_CONTRACTEDFOLDNEXT, line, 0);
        if next < 0 {
            return out;
        }
        out.push(next);
        line = next as usize + 1;
    }
}

fn copy_styles(from: &NSView, to: &NSView) {
    for s in 0..=255 {
        for (get, set) in STYLE_PROPS {
            sci::send(to, set, s, sci::send(from, get, s, 0));
        }
        let mut font = [0u8; 256];
        let n = sci::send(from, SCI_STYLEGETFONT, s, font.as_mut_ptr() as isize);
        if n > 0 && (n as usize) < font.len() {
            sci::send(to, SCI_STYLESETFONT, s, font.as_ptr() as isize);
        }
    }
}

impl App {
    fn doc_map(&self) -> Option<&Map> {
        self.dock_ui()?.map.get()
    }

    // DocumentMap WM_INITDIALOG: a Scintilla view at the smallest zoom, with no scroll bars and no margins.
    pub(crate) fn doc_map_build(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let b = content_box(mtm);
        let view = sci::new_view();
        view.setFrame(b.bounds());
        view.setAutoresizingMask(fill());
        sci::send(&view, SCI_SETZOOM, -10isize as usize, 0);
        sci::send(&view, SCI_SETVSCROLLBAR, 0, 0);
        sci::send(&view, SCI_SETHSCROLLBAR, 0, 0);
        sci::send(&view, SCI_SETMODEVENTMASK, 0, 0);
        sci::send(&view, SCI_SETCARETSTYLE, 0, 0);
        for m in 0..5 {
            sci::send(&view, SCI_SETMARGINWIDTHN, m, 0);
        }
        let zone = ViewZone::new(mtm, self.retain());
        zone.setFrame(b.bounds());
        zone.setAutoresizingMask(fill());
        b.addSubview(&view);
        b.addSubview(&zone);
        if let Some(d) = self.dock_ui() {
            let _ = d.map.set(Map {
                view,
                zone,
                wrap: Cell::new(None),
            });
        }
        b
    }

    // DocumentMap::reloadMap: the map shows the document of the editor (SCI_SETDOCPOINTER) with its styles and folds.
    pub(crate) fn doc_map_reload(&self) {
        let (Some(m), Some(v)) = (self.doc_map(), self.editor()) else {
            return;
        };
        if !self.panel_visible(DOC_MAP) {
            return;
        }
        let doc = sci::send(&v, SCI_GETDOCPOINTER, 0, 0);
        sci::send(&m.view, SCI_SETDOCPOINTER, 0, doc);
        copy_styles(&v, &m.view);
        m.wrap.set(None);
        self.doc_map_scroll();
    }

    // DocumentMap::wrapMap: a wrapped editor gives a map that is narrower by the zoom ratio, so that the lines wrap the same.
    fn doc_map_wrap(&self, v: &NSView, m: &Map) {
        let mode = sci::send(v, SCI_GETWRAPMODE, 0, 0);
        let zoom = sci::send(v, SCI_GETZOOM, 0, 0);
        let width = sci::content(v).visibleRect().size.width as i64;
        let area = unsafe { m.view.superview() }.map_or(0., |s| s.bounds().size.width);
        let key = (mode != 0, zoom, width, area as i64);
        if m.wrap.replace(Some(key)) == Some(key) {
            return;
        }
        let Some(s) = (unsafe { m.view.superview() }) else { return };
        let h = s.bounds().size.height;
        if mode != 0 {
            let ratio = ZOOM_RATIO[(zoom + 10).clamp(0, 30) as usize];
            m.view.setFrame(frame(0., 0., width as f64 / ratio, h));
            m.view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewHeightSizable
                    | NSAutoresizingMaskOptions::ViewMaxXMargin,
            );
            let indent = sci::send(v, SCI_GETWRAPINDENTMODE, 0, 0);
            sci::send(&m.view, SCI_SETWRAPINDENTMODE, indent as usize, 0);
        } else {
            m.view.setFrame(frame(0., 0., area, h));
            m.view.setAutoresizingMask(fill());
        }
        sci::send(&m.view, SCI_SETWRAPMODE, mode as usize, 0);
        m.zone.setFrame(m.view.frame());
        m.zone.setAutoresizingMask(m.view.autoresizingMask());
    }

    // DocumentMap::scrollMap: the map scrolls to show the lines of the editor, and the zone marks them.
    pub(crate) fn doc_map_scroll(&self) {
        let (Some(m), Some(v)) = (self.doc_map(), self.editor()) else {
            return;
        };
        if !self.panel_shown(DOC_MAP) {
            return;
        }
        if sci::send(&m.view, SCI_GETDOCPOINTER, 0, 0) != sci::send(&v, SCI_GETDOCPOINTER, 0, 0) {
            return self.doc_map_reload();
        }
        let folds = contracted(&v);
        if contracted(&m.view) != folds {
            sci::send(&m.view, SCI_FOLDALL, SC_FOLDACTION_EXPAND, 0);
            for line in folds {
                sci::send(&m.view, SCI_FOLDLINE, line as usize, SC_FOLDACTION_CONTRACT);
            }
        }
        self.doc_map_wrap(&v, m);
        let size = sci::content(&v).visibleRect().size;
        let high = sci::send(&v, SCI_POSITIONFROMPOINT, 0, 0);
        let low = sci::send(
            &v,
            SCI_POSITIONFROMPOINT,
            size.width as usize,
            size.height as isize,
        );
        sci::send(&m.view, SCI_GOTOPOS, high as usize, 0);
        sci::send(&m.view, SCI_GOTOPOS, low as usize, 0);
        let top = sci::send(&m.view, SCI_POINTYFROMPOSITION, 0, high);
        let line_h = sci::send(&m.view, SCI_TEXTHEIGHT, 0, 0);
        let bottom = if sci::send(&v, SCI_GETWRAPMODE, 0, 0) == 0 {
            let edit_h = sci::send(&v, SCI_TEXTHEIGHT, 0, 0).max(1);
            top + line_h * size.height as isize / edit_h
        } else {
            sci::send(&m.view, SCI_POINTYFROMPOSITION, 0, low) + line_h
        };
        m.zone.ivars().zone.set((top as f64, bottom as f64));
        m.zone.setNeedsDisplay(true);
    }

    // DOCUMENTMAP_MOUSECLICKED: the editor scrolls by the map lines between the click and the middle of the zone.
    fn doc_map_click(&self, y: f64) {
        let (Some(m), Some(v)) = (self.doc_map(), self.editor()) else {
            return;
        };
        let (top, bottom) = m.zone.ivars().zone.get();
        let line_h = sci::send(&m.view, SCI_TEXTHEIGHT, 0, 0).max(1);
        let lines = (y - (top + bottom) / 2.) as isize / line_h;
        sci::send(&v, SCI_LINESCROLL, 0, lines);
        self.doc_map_scroll();
    }
}
