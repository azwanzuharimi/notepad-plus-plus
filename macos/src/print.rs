// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{ns, prefs, run, sci, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBezierPath, NSButton, NSColor, NSFont, NSFontAttributeName, NSFontManager, NSFontTraitMask,
    NSForegroundColorAttributeName, NSPopUpButton, NSPrintAllPages, NSPrintCopies, NSPrintInfo,
    NSPrintJobSavingURL, NSPrintOperation, NSPrintSpoolJob,
    NSPrintPanelOptions, NSPrintingPaginationMode, NSStringDrawing, NSTextField, NSView,
};
use objc2_foundation::{
    NSDate, NSDateFormatter, NSDateFormatterStyle, NSDictionary, NSNumber, NSPoint, NSRange, NSRect, NSSize,
    NSString,
};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;

const SCI_SETPRINTCOLOURMODE: u32 = 2148;
const SCI_SETMARGINWIDTHN: u32 = 2242;
const SCI_GETMARGINWIDTHN: u32 = 2243;
const SCI_FORMATRANGEFULL: u32 = 2777;
const PAGE_VAR: &str = "$(CURRENT_PRINTING_PAGE)";

// The variable list of the Print page (PrintSubDlg): (label, variable).
pub const VARS: [(&str, &str); 7] = [
    ("Full file name path", "$(FULL_CURRENT_PATH)"),
    ("File name", "$(FILE_NAME)"),
    ("File directory", "$(CURRENT_DIRECTORY)"),
    ("Page", PAGE_VAR),
    ("Short date format", "$(SHORT_DATE)"),
    ("Long date format", "$(LONG_DATE)"),
    ("Time", "$(TIME)"),
];
pub const VAR_TAG: isize = 2900;

// Sci_RangeToFormatFull.
#[repr(C)]
struct RangeToFormat {
    hdc: *mut c_void,
    hdc_target: *mut c_void,
    rc: [i32; 4],
    rc_page: [i32; 4],
    chrg: [isize; 2],
}

extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGColorSpaceRelease(s: *mut c_void);
    fn CGBitmapContextCreate(
        data: *mut c_void,
        w: usize,
        h: usize,
        bits: usize,
        row: usize,
        space: *mut c_void,
        info: u32,
    ) -> *mut c_void;
    fn CGContextRelease(c: *mut c_void);
    fn CGContextSaveGState(c: *mut c_void);
    fn CGContextRestoreGState(c: *mut c_void);
    fn CGContextTranslateCTM(c: *mut c_void, x: f64, y: f64);
}

// The text rectangle of a page in points: left, top, right, bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Area {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

// Printer.cpp doPrint, from the printable origin: user margins (mm) apply when one is set, but not less than the unprintable border; then the header and footer space.
pub fn text_area(
    paper: (f64, f64),
    border: [f64; 4],
    mm: [i64; 4],
    header: Option<f64>,
    footer: Option<f64>,
    footer_line: f64,
) -> Area {
    let mut m = border;
    if mm.iter().any(|&x| x != 0) {
        for (b, u) in m.iter_mut().zip(mm) {
            *b = b.max(u as f64 * 72. / 25.4);
        }
    }
    let marge = footer_line * 1.5;
    let mut a = Area {
        left: m[0] - border[0] + marge,
        top: m[1] - border[1] + marge,
        right: paper.0 - m[2] - border[0] - marge,
        bottom: paper.1 - m[3] - border[1] - marge,
    };
    if let Some(h) = header {
        a.top += h * 1.5;
    }
    if let Some(h) = footer {
        a.bottom -= h * 1.5;
    }
    a
}

// Printer.cpp: the selection is printed only when the print dialog asks for it; else the whole document.
pub fn print_range(sel: (usize, usize), len: usize, selection_only: bool) -> (usize, usize) {
    if selection_only && sel.0 != sel.1 {
        (sel.0.min(sel.1), sel.0.max(sel.1).min(len))
    } else {
        (0, len)
    }
}

// The page loop of Printer.cpp doPrint: the range given to SCI_FORMATRANGEFULL for each page; with form feeds, a page stops before the next one.
pub fn paginate(
    range: (usize, usize),
    form_feeds: Option<&[usize]>,
    mut format: impl FnMut(usize, usize) -> usize,
) -> Vec<(usize, usize)> {
    let (mut done, end) = range;
    let mut pages = vec![];
    while done < end {
        let ff = form_feeds.and_then(|f| f.iter().copied().find(|&p| p >= done && p < end));
        let max = match ff {
            Some(p) => p.saturating_sub(1),
            None => end,
        };
        let mut next = format(done, max);
        if let Some(p) = ff.filter(|&p| next <= p) {
            next = p + 1;
        }
        pages.push((done, max));
        if next <= done {
            break;
        }
        done = next;
    }
    pages
}

// Printer.cpp: $(SHORT_DATE), $(LONG_DATE) and $(TIME) first (first match only), then the Run variables; an unknown $(NAME) stays.
pub fn expand_part(s: &str, dates: &[String; 3], var: &dyn Fn(&str) -> Option<String>) -> String {
    let s = s
        .replacen("$(SHORT_DATE)", &dates[0], 1)
        .replacen("$(LONG_DATE)", &dates[1], 1)
        .replacen("$(TIME)", &dates[2], 1);
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(i) = rest.find("$(") {
        out += &rest[..i];
        let after = &rest[i + 2..];
        match after.find(')').and_then(|e| Some((var(&after[..e])?, e))) {
            Some((v, e)) => {
                out += &v;
                rest = &after[e + 1..];
            }
            None => {
                out.push('$');
                rest = &rest[i + 1..];
            }
        }
    }
    out + rest
}

pub fn page_text(s: &str, page: usize) -> String {
    s.replacen(PAGE_VAR, &page.to_string(), 1)
}

// GetDateFormat DATE_SHORTDATE and DATE_LONGDATE, GetTimeFormat TIME_NOSECONDS.
fn dates() -> [String; 3] {
    let now = NSDate::now();
    let f =
        |d, t| NSDateFormatter::localizedStringFromDate_dateStyle_timeStyle(&now, d, t).to_string();
    [
        f(
            NSDateFormatterStyle::ShortStyle,
            NSDateFormatterStyle::NoStyle,
        ),
        f(
            NSDateFormatterStyle::FullStyle,
            NSDateFormatterStyle::NoStyle,
        ),
        f(
            NSDateFormatterStyle::NoStyle,
            NSDateFormatterStyle::ShortStyle,
        ),
    ]
}

// The header or footer font: Arial 9 when not set.
fn font(mtm: MainThreadMarker, name: &str, style: i64, size: i64) -> Retained<NSFont> {
    let size = if size > 0 { size as f64 } else { 9. };
    let name = if name.is_empty() { "Arial" } else { name };
    let mut traits = NSFontTraitMask::empty();
    if style & 1 != 0 {
        traits |= NSFontTraitMask::BoldFontMask;
    }
    if style & 2 != 0 {
        traits |= NSFontTraitMask::ItalicFontMask;
    }
    let weight = if style & 1 != 0 { 9 } else { 5 };
    NSFontManager::sharedFontManager(mtm)
        .fontWithFamily_traits_weight_size(&ns(name), traits, weight, size)
        .unwrap_or_else(|| NSFont::systemFontOfSize(size))
}

fn line_height(f: &NSFont) -> f64 {
    f.ascender() - f.descender() + f.leading()
}

struct Part {
    text: [String; 3],
    font: Retained<NSFont>,
    height: f64,
}

pub struct Job {
    view: Retained<NSView>,
    sel: (usize, usize),
    len: usize,
    form_feeds: Option<Vec<usize>>,
    header: Option<Part>,
    footer: Option<Part>,
    footer_line: f64,
    mm: [i64; 4],
    pages: RefCell<Vec<(usize, usize)>>,
    area: Cell<Area>,
    page: Cell<NSSize>,
}

define_class!(
    // The pages of one print job; each page is SCI_FORMATRANGEFULL with the header and footer of Printer.cpp.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Job]
    struct PrintView;

    impl PrintView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(knowsPageRange:))]
        fn knows_page_range(&self, range: *mut NSRange) -> bool {
            let n = self.paginate();
            if !range.is_null() {
                unsafe { *range = NSRange::new(1, n.max(1)) };
            }
            true
        }

        #[unsafe(method(rectForPage:))]
        fn rect_for_page(&self, page: isize) -> NSRect {
            let p = self.ivars().page.get();
            NSRect::new(
                NSPoint::new(0., (page.max(1) - 1) as f64 * p.height),
                p,
            )
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _r: NSRect) {
            let page = NSPrintOperation::currentOperation(self.mtm()).map(|o| o.currentPage());
            if let Some(n) = page.and_then(|n| usize::try_from(n).ok()).filter(|&n| n > 0) {
                self.draw_page(n);
            }
        }
    }
);

fn format(
    v: &NSView,
    gc: *mut c_void,
    draw: bool,
    a: Area,
    page: NSSize,
    r: (usize, usize),
) -> usize {
    let mut fr = RangeToFormat {
        hdc: gc,
        hdc_target: gc,
        rc: [a.left as i32, a.top as i32, a.right as i32, a.bottom as i32],
        rc_page: [0, 0, page.width as i32, page.height as i32],
        chrg: [r.0 as isize, r.1 as isize],
    };
    sci::send(
        v,
        SCI_FORMATRANGEFULL,
        draw as usize,
        &mut fr as *mut _ as isize,
    )
    .max(0) as usize
}

fn attrs(f: &NSFont) -> Retained<NSDictionary<NSString, AnyObject>> {
    let black = NSColor::blackColor();
    let keys = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let vals: [&AnyObject; 2] = [f, &black];
    NSDictionary::from_slices(&keys, &vals)
}

impl PrintView {
    fn new(mtm: MainThreadMarker, job: Job) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(job);
        unsafe { msg_send![super(this), init] }
    }

    // Lays out the pages for the current paper and selection setting; returns the page count.
    fn paginate(&self) -> usize {
        let j = self.ivars();
        let Some(info) = NSPrintOperation::currentOperation(self.mtm()).map(|o| o.printInfo())
        else {
            return 0;
        };
        let paper = info.paperSize();
        let ib = info.imageablePageBounds();
        let border = [
            ib.origin.x,
            paper.height - ib.origin.y - ib.size.height,
            paper.width - ib.origin.x - ib.size.width,
            ib.origin.y,
        ]
        .map(|x| x.max(0.));
        // AppKit puts each page rectangle at the printable origin, as a Windows printer DC does.
        let page = ib.size;
        let area = text_area(
            (paper.width, paper.height),
            border,
            j.mm,
            j.header.as_ref().map(|h| h.height),
            j.footer.as_ref().map(|h| h.height),
            j.footer_line,
        );
        let range = print_range(j.sel, j.len, info.isSelectionOnly());
        let space = unsafe { CGColorSpaceCreateDeviceRGB() };
        let gc = unsafe { CGBitmapContextCreate(std::ptr::null_mut(), 1, 1, 8, 0, space, 1) };
        unsafe { CGColorSpaceRelease(space) };
        let pages = if gc.is_null() {
            vec![]
        } else {
            let p = paginate(range, j.form_feeds.as_deref(), |a, b| {
                format(&j.view, gc, false, area, page, (a, b))
            });
            unsafe { CGContextRelease(gc) };
            p
        };
        let n = pages.len();
        *j.pages.borrow_mut() = pages;
        j.area.set(area);
        j.page.set(page);
        self.setFrameSize(NSSize::new(page.width, page.height * n.max(1) as f64));
        n
    }

    fn draw_page(&self, n: usize) {
        let j = self.ivars();
        let Some(ctx) = objc2_app_kit::NSGraphicsContext::currentContext() else {
            return;
        };
        let cg = ctx.CGContext();
        let gc = Retained::as_ptr(&cg) as *mut c_void;
        if gc.is_null() {
            return;
        }
        let (a, page) = (j.area.get(), j.page.get());
        unsafe {
            CGContextSaveGState(gc);
            CGContextTranslateCTM(gc, 0., (n - 1) as f64 * page.height);
        }
        // Text tops: the header ends half a line above the text, the footer starts half a line below it.
        if let Some(h) = &j.header {
            self.draw_part(h, n, a, a.top - h.height * 1.5, a.top - h.height / 4.);
        }
        if let Some(&r) = j.pages.borrow().get(n - 1) {
            format(&j.view, gc, true, a, page, r);
        }
        if let Some(f) = &j.footer {
            self.draw_part(f, n, a, a.bottom + f.height / 2., a.bottom + f.height / 4.);
        }
        unsafe { CGContextRestoreGState(gc) };
    }

    fn draw_part(&self, p: &Part, page: usize, a: Area, y: f64, line: f64) {
        let at = attrs(&p.font);
        for (i, t) in p.text.iter().enumerate() {
            if t.is_empty() {
                continue;
            }
            let s = ns(&page_text(t, page));
            let w = unsafe { s.sizeWithAttributes(Some(&at)) }.width;
            let x = match i {
                0 => a.left + 5.,
                1 => (a.right - a.left) / 2. + a.left - w / 2.,
                _ => a.right - w,
            };
            unsafe { s.drawAtPoint_withAttributes(NSPoint::new(x, y), Some(&at)) };
        }
        NSColor::blackColor().setStroke();
        NSBezierPath::strokeLineFromPoint_toPoint(
            NSPoint::new(a.left, line),
            NSPoint::new(a.right, line),
        );
    }
}

impl App {
    // Notepad_plus::filePrint and Printer: Print... shows the print panel, Print Now uses the last printer settings.
    pub(crate) fn file_print(&self, show_dialog: bool) {
        let Some(tab) = self.current().and_then(|i| self.tab(i)) else {
            return;
        };
        let mtm = self.mtm();
        let v = tab.view.clone();
        // Printer.cpp prints no page for an empty document.
        if sci::length(&v) <= 0 {
            return;
        }
        let p = prefs::get();
        let (s, e) = sci::selection(&v);
        let sel = (s.max(0) as usize, e.max(0) as usize);
        let dates = dates();
        let var = |n: &str| {
            run::VARS
                .iter()
                .position(|x| x.0 == n)
                .map(|i| self.run_value(i))
        };
        let part = |text: [&String; 3], name: &str, style, size| {
            text.iter().any(|t| !t.is_empty()).then(|| {
                let font = font(mtm, name, style, size);
                Part {
                    text: text.map(|t| expand_part(t, &dates, &var)),
                    height: line_height(&font),
                    font,
                }
            })
        };
        let header = part(
            [&p.header_left, &p.header_middle, &p.header_right],
            &p.header_font_name,
            p.header_font_style,
            p.header_font_size,
        );
        let footer = part(
            [&p.footer_left, &p.footer_middle, &p.footer_right],
            &p.footer_font_name,
            p.footer_font_style,
            p.footer_font_size,
        );
        let footer_line = line_height(&font(
            mtm,
            &p.footer_font_name,
            p.footer_font_style,
            p.footer_font_size,
        ));
        let form_feeds = p.print_form_feed.then(|| {
            sci::bytes(&v)
                .iter()
                .enumerate()
                .filter(|(_, b)| **b == b'\x0c')
                .map(|(i, _)| i)
                .collect()
        });
        let job = Job {
            view: v.clone(),
            sel,
            len: sci::length(&v).max(0) as usize,
            form_feeds,
            header,
            footer,
            footer_line,
            mm: [p.marge_left, p.marge_top, p.marge_right, p.marge_bottom],
            pages: RefCell::default(),
            area: Cell::default(),
            page: Cell::new(NSSize::new(0., 0.)),
        };
        let info: Retained<NSPrintInfo> =
            unsafe { msg_send![&*NSPrintInfo::sharedPrintInfo(), copy] };
        for m in [
            NSPrintInfo::setLeftMargin,
            NSPrintInfo::setRightMargin,
            NSPrintInfo::setTopMargin,
            NSPrintInfo::setBottomMargin,
        ] {
            m(&info, 0.);
        }
        info.setHorizontalPagination(NSPrintingPaginationMode::Clip);
        info.setVerticalPagination(NSPrintingPaginationMode::Clip);
        info.setHorizontallyCentered(false);
        info.setVerticallyCentered(false);
        info.setSelectionOnly(show_dialog && sel.0 != sel.1);
        let pv = PrintView::new(mtm, job);
        let op = NSPrintOperation::printOperationWithView_printInfo(&pv, &info);
        let title = tab
            .path
            .as_ref()
            .map_or(tab.name.clone(), |p| p.to_string_lossy().into_owned());
        op.setJobTitle(Some(&ns(&title)));
        op.setShowsPrintPanel(show_dialog);
        if show_dialog {
            let panel = op.printPanel();
            let mut o = panel.options()
                | NSPrintPanelOptions::ShowsPaperSize
                | NSPrintPanelOptions::ShowsOrientation
                | NSPrintPanelOptions::ShowsPreview;
            if sel.0 != sel.1 {
                o |= NSPrintPanelOptions::ShowsPrintSelection;
            }
            panel.setOptions(o);
        }
        let mode = if (0..=3).contains(&p.print_option) {
            p.print_option
        } else {
            3
        };
        sci::send(&v, SCI_SETPRINTCOLOURMODE, mode as usize, 0);
        let margin = prefs::MARGIN_LINE_NUMBER;
        let width = sci::send(&v, SCI_GETMARGINWIDTHN, margin, 0);
        if !p.print_line_number {
            sci::send(&v, SCI_SETMARGINWIDTHN, margin, 0);
        }
        let ok = op.runOperation();
        if !p.print_line_number {
            sci::send(&v, SCI_SETMARGINWIDTHN, margin, width);
        }
        // Print Now keeps the printer and paper, but not the job choices of the last print.
        if ok && show_dialog {
            let used = op.printInfo();
            used.setSelectionOnly(false);
            used.setJobDisposition(unsafe { NSPrintSpoolJob });
            unsafe {
                let d = used.dictionary();
                d.removeObjectForKey(NSPrintJobSavingURL);
                d.setObject_forKey(&NSNumber::new_bool(true), ProtocolObject::from_ref(NSPrintAllPages));
                d.setObject_forKey(&NSNumber::new_isize(1), ProtocolObject::from_ref(NSPrintCopies));
            }
            NSPrintInfo::setSharedPrintInfo(&used);
        }
    }

    // PrintSubDlg IDC_BUTTON_ADDVAR: the variable replaces the selection of the focused header or footer field.
    pub(crate) fn print_add_var(&self, b: &NSButton) {
        let Some(group) = (unsafe { b.superview() }) else {
            return;
        };
        let var = group
            .viewWithTag(VAR_TAG)
            .and_then(|v| v.downcast::<NSPopUpButton>().ok())
            .and_then(|p| usize::try_from(p.indexOfSelectedItem()).ok())
            .and_then(|i| VARS.get(i));
        let editor = b.window().and_then(|w| w.firstResponder());
        let field = editor.as_ref().and_then(|e| {
            let d: Option<Retained<AnyObject>> = unsafe { msg_send![&**e, delegate] };
            d?.downcast::<NSTextField>().ok()
        });
        let (Some((_, var)), Some(editor), Some(field)) = (var, editor, field) else {
            return;
        };
        if !field.isDescendantOf(&group) {
            return;
        }
        let _: () = unsafe { msg_send![&*editor, insertText: &*ns(var)] };
        self.pref_changed(&field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_header_variables() {
        let dates = [
            "1/2/26".to_string(),
            "Friday, January 2, 2026".into(),
            "9:05".into(),
        ];
        let var = |n: &str| match n {
            "FULL_CURRENT_PATH" => Some("/tmp/a b.txt".to_string()),
            "FILE_NAME" => Some("a b.txt".to_string()),
            _ => None,
        };
        assert_eq!(
            expand_part("$(FULL_CURRENT_PATH) - $(SHORT_DATE)", &dates, &var),
            "/tmp/a b.txt - 1/2/26"
        );
        assert_eq!(
            expand_part("$(LONG_DATE) $(TIME) $(TIME)", &dates, &var),
            "Friday, January 2, 2026 9:05 $(TIME)"
        );
        assert_eq!(
            expand_part(
                "Page $(CURRENT_PRINTING_PAGE) of $(FILE_NAME)",
                &dates,
                &var
            ),
            "Page $(CURRENT_PRINTING_PAGE) of a b.txt"
        );
        assert_eq!(
            expand_part("$(NOPE) $(FILE_NAME", &dates, &var),
            "$(NOPE) $(FILE_NAME"
        );
        assert_eq!(expand_part("$$(FILE_NAME)$", &dates, &var), "$a b.txt$");
        assert_eq!(expand_part("", &dates, &var), "");
        assert_eq!(
            page_text("p$(CURRENT_PRINTING_PAGE)/$(CURRENT_PRINTING_PAGE)", 3),
            "p3/$(CURRENT_PRINTING_PAGE)"
        );
    }

    #[test]
    fn selection_range() {
        assert_eq!(print_range((5, 9), 100, true), (5, 9));
        assert_eq!(print_range((9, 5), 100, true), (5, 9));
        assert_eq!(print_range((5, 200), 100, true), (5, 100));
        assert_eq!(print_range((5, 5), 100, true), (0, 100));
        assert_eq!(print_range((5, 9), 100, false), (0, 100));
    }

    #[test]
    fn pages_and_form_feeds() {
        let ten = |a: usize, b: usize| (a + 10).min(b.max(a));
        assert_eq!(
            paginate((0, 25), None, ten),
            vec![(0, 25), (10, 25), (20, 25)]
        );
        assert_eq!(paginate((0, 0), None, ten), vec![]);
        assert_eq!(paginate((3, 8), None, ten), vec![(3, 8)]);
        let ff = [4, 6];
        assert_eq!(
            paginate((0, 25), Some(&ff), ten),
            vec![(0, 3), (5, 5), (7, 25), (17, 25)]
        );
        assert_eq!(paginate((0, 5), Some(&[0]), ten), vec![(0, 0), (1, 5)]);
        assert_eq!(paginate((0, 9), None, |a, _| a), vec![(0, 9)]);
    }

    #[test]
    fn page_area() {
        let a4 = (595., 842.);
        // imageablePageBounds (18, 41, 559, 783) of A4: left, top, right, bottom border.
        let border = [18., 18., 18., 41.];
        let a = text_area(a4, border, [0; 4], None, None, 10.);
        assert_eq!(
            a,
            Area {
                left: 15.,
                top: 15.,
                right: 544.,
                bottom: 768.
            }
        );
        let a = text_area(a4, border, [20, 5, 0, 10], Some(12.), Some(10.), 10.);
        assert_eq!(a.left, 20. * 72. / 25.4 - 18. + 15.);
        assert_eq!(a.top, 15. + 18.);
        assert_eq!(a.right, 595. - 18. - 18. - 15.);
        assert_eq!(a.bottom, 842. - 41. - 18. - 15. - 15.);
    }
}
