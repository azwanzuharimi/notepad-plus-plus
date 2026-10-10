// SPDX-License-Identifier: GPL-3.0-or-later
use super::model::{self, UStyle, Udl, COLORSTYLE_BG, COLORSTYLE_FG, KW_COMMENTS, KW_DELIMITERS};
use super::{with, PREFIX};
use crate::panel::{on, set_on, text, Form};
use crate::{ns, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSButton, NSColor, NSColorSpace, NSColorWell, NSFontManager,
    NSLineBreakMode, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSSavePanel, NSTabView,
    NSTabViewItem, NSTextField, NSView, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSURL};
use std::cell::{Cell, OnceCell};
use std::path::PathBuf;

const DOC: &str = "https://npp-user-manual.org/docs/user-defined-language-system/";
const W: f64 = 790.;
const H: f64 = 690.;
// Tags of the controls that are not keyword lists (the lists use their SCE_USER_KWLIST_* index).
const T_PREFIX: isize = 100;
const T_CASE: isize = 110;
const T_FOLD_COMPACT: isize = 111;
const T_FOLD_COMMENTS: isize = 112;
const T_PURE_LC: isize = 120;
const T_DECIMAL: isize = 130;
const T_EXT: isize = 140;
// Commands of the buttons at the top, and the documentation link.
const C_NEW: isize = 0;
const C_SAVE_AS: isize = 1;
const C_RENAME: isize = 2;
const C_REMOVE: isize = 3;
const C_IMPORT: isize = 4;
const C_EXPORT: isize = 5;
const C_DOC: isize = 6;
const ORDINALS: [&str; 8] = ["1st", "2nd", "3rd", "4th", "5th", "6th", "7th", "8th"];
const FONT_SIZES: [&str; 17] = [
    "", "5", "6", "7", "8", "9", "10", "11", "12", "14", "16", "18", "20", "22", "24", "26", "28",
];
// GlobalMappers::nestingMapper: the Styler Dialog check boxes and their SCE_USER_MASK_NESTING_* bits, in 3 columns.
const NESTING: [[(&str, i32); 8]; 3] = [
    [
        ("Delimiter 1", 0x1),
        ("Delimiter 2", 0x2),
        ("Delimiter 3", 0x4),
        ("Delimiter 4", 0x8),
        ("Delimiter 5", 0x10),
        ("Delimiter 6", 0x20),
        ("Delimiter 7", 0x40),
        ("Delimiter 8", 0x80),
    ],
    [
        ("Keyword 1", 0x400),
        ("Keyword 2", 0x800),
        ("Keyword 3", 0x1000),
        ("Keyword 4", 0x2000),
        ("Keyword 5", 0x4000),
        ("Keyword 6", 0x8000),
        ("Keyword 7", 0x10000),
        ("Keyword 8", 0x20000),
    ],
    [
        ("Comment", 0x100),
        ("Comment line", 0x200),
        ("Operators 1", 0x1000000),
        ("Operators 2", 0x2000000),
        ("Numbers", 0x4000000),
        ("", 0),
        ("", 0),
        ("", 0),
    ],
];

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

// A view with controls placed from its top left corner, as in the Notepad++ dialog templates.
struct Pane<'a> {
    view: Retained<NSView>,
    h: f64,
    t: &'a AnyObject,
    mtm: MainThreadMarker,
}

impl Pane<'_> {
    fn put(&self, v: &NSView, x: f64, top: f64, w: f64, h: f64) {
        v.setFrame(rect(x, self.h - top - h, w, h));
        self.view.addSubview(v);
    }

    fn label(&self, s: &str, x: f64, top: f64, w: f64) -> Retained<NSTextField> {
        let l = NSTextField::labelWithString(&ns(s), self.mtm);
        self.put(&l, x, top + 3., w, 18.);
        l
    }

    fn field(&self, x: f64, top: f64, w: f64, h: f64, tag: isize) -> Retained<NSTextField> {
        let f = NSTextField::textFieldWithString(&ns(""), self.mtm);
        if h > 24. {
            f.setUsesSingleLineMode(false);
            f.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
            if let Some(c) = f.cell() {
                c.setScrollable(false);
                c.setWraps(true);
            }
        }
        f.setTag(tag);
        f.setContinuous(true);
        unsafe {
            f.setTarget(Some(self.t));
            f.setAction(Some(sel!(udlEdit:)));
        }
        self.put(&f, x, top, w, h);
        f
    }

    fn button(
        &self,
        s: &str,
        x: f64,
        top: f64,
        w: f64,
        action: Sel,
        tag: isize,
    ) -> Retained<NSButton> {
        let b = unsafe {
            NSButton::buttonWithTitle_target_action(&ns(s), Some(self.t), Some(action), self.mtm)
        };
        b.setTag(tag);
        self.put(&b, x, top - 4., w, 28.);
        b
    }

    fn styler(&self, x: f64, top: f64, id: usize) {
        self.button("Styler", x, top, 80., sel!(udlStyler:), id as isize);
    }

    fn check(&self, s: &str, x: f64, top: f64, w: f64, tag: isize) -> Retained<NSButton> {
        let b = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &ns(s),
                Some(self.t),
                Some(sel!(udlEdit:)),
                self.mtm,
            )
        };
        b.setTag(tag);
        self.put(&b, x, top, w, 20.);
        b
    }

    // Radio buttons in their own view, so that each group works alone.
    fn radios(
        &self,
        titles: &[&str],
        x: f64,
        top: f64,
        w: f64,
        tag: isize,
    ) -> Vec<Retained<NSButton>> {
        let g = Pane {
            view: NSView::new(self.mtm),
            h: titles.len() as f64 * 22.,
            t: self.t,
            mtm: self.mtm,
        };
        self.put(&g.view, x, top, w, g.h);
        titles
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let b = unsafe {
                    NSButton::radioButtonWithTitle_target_action(
                        &ns(s),
                        Some(self.t),
                        Some(sel!(udlEdit:)),
                        self.mtm,
                    )
                };
                b.setTag(tag + k as isize);
                g.put(&b, 0., k as f64 * 22., w, 20.);
                b
            })
            .collect()
    }
}

struct Ui {
    form: Form,
    lang: Retained<NSPopUpButton>,
    ext: Retained<NSTextField>,
    ext_label: Retained<NSTextField>,
    rename: Retained<NSButton>,
    remove: Retained<NSButton>,
    case: Retained<NSButton>,
    lists: Vec<(usize, Retained<NSTextField>)>,
    comments: Vec<Retained<NSTextField>>,
    delims: Vec<Retained<NSTextField>>,
    prefix: Vec<Retained<NSButton>>,
    fold_compact: Retained<NSButton>,
    fold_comments: Retained<NSButton>,
    pure_lc: Vec<Retained<NSButton>>,
    decimal: Vec<Retained<NSButton>>,
    warned: Cell<bool>,
}

thread_local! {
    static UI: OnceCell<&'static Ui> = const { OnceCell::new() };
}

fn tab<'a>(tv: &NSTabView, title: &str, t: &'a AnyObject) -> Pane<'a> {
    let mtm = tv.mtm();
    let r = tv.contentRect();
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        rect(0., 0., r.size.width, r.size.height),
    );
    let item = NSTabViewItem::new();
    item.setLabel(&ns(title));
    item.setView(Some(&view));
    tv.addTabViewItem(&item);
    Pane {
        view,
        h: r.size.height,
        t,
        mtm,
    }
}

// Three labelled rows of fields: Open, Middle and Close of a folding group, or Open, Escape and Close of a delimiter.
fn rows(
    p: &Pane,
    labels: &[&str],
    x: f64,
    top: f64,
    lw: f64,
    fw: f64,
    fh: f64,
    tag: isize,
) -> Vec<Retained<NSTextField>> {
    labels
        .iter()
        .enumerate()
        .map(|(k, l)| {
            let y = top + k as f64 * (fh + 6.);
            p.label(l, x, y, lw);
            p.field(x + lw, y, fw, fh, tag)
        })
        .collect()
}

// IDD_GLOBAL_USERDEFINE_DLG with its four tabs (UserDefineDialog.rc).
fn build(app: &App) -> Ui {
    let mtm = app.mtm();
    let t: &AnyObject = app;
    let form = Form::new(mtm, "User Defined Language v2.1", W, H);
    let top = Pane {
        view: form.panel.contentView().unwrap_or_else(|| NSView::new(mtm)),
        h: H,
        t,
        mtm,
    };
    top.label("User language:", 10., 12., 100.);
    let lang = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(0., 0., 220., 26.),
        false,
    );
    unsafe {
        lang.setTarget(Some(t));
        lang.setAction(Some(sel!(udlLang:)));
    }
    top.put(&lang, 110., 10., 220., 26.);
    top.button("Create new...", 340., 12., 110., sel!(udlCommand:), C_NEW);
    top.button("Save as...", 455., 12., 100., sel!(udlCommand:), C_SAVE_AS);
    let rename = top.button("Rename", 560., 12., 100., sel!(udlCommand:), C_RENAME);
    let remove = top.button("Remove", 665., 12., 100., sel!(udlCommand:), C_REMOVE);
    top.button("Import...", 10., 46., 100., sel!(udlCommand:), C_IMPORT);
    top.button("Export...", 115., 46., 100., sel!(udlCommand:), C_EXPORT);
    let case = top.check("Ignore case", 240., 46., 120., T_CASE);
    let ext_label = top.label("Ext.:", 420., 46., 40.);
    let ext = top.field(460., 46., 300., 22., T_EXT);
    let tv = NSTabView::new(mtm);
    top.put(&tv, 10., 80., W - 20., H - 90.);
    let mut lists = vec![];

    let p = tab(&tv, "Folder & Default", t);
    p.label("Documentation", 10., 8., 200.);
    p.button(
        "User Defined Language Documentation",
        10.,
        32.,
        300.,
        sel!(udlCommand:),
        C_DOC,
    );
    p.label("Default style", 10., 74., 120.);
    p.styler(140., 74., 0);
    let fold_compact = p.check(
        "Fold compact (fold empty lines too)",
        10.,
        104.,
        330.,
        T_FOLD_COMPACT,
    );
    let groups = [
        ("Folding in comment style", 380., 8., 15, 16),
        ("Folding in code 1 style", 10., 250., 13, 10),
        (
            "Folding in code 2 style (separators needed)",
            380.,
            250.,
            14,
            13,
        ),
    ];
    for (title, x, y, style, first) in groups {
        p.label(title, x, y, 270.);
        p.styler(x + 280., y, style);
        for (k, f) in rows(
            &p,
            &["Open:", "Middle:", "Close:"],
            x,
            y + 30.,
            60.,
            290.,
            50.,
            first as isize,
        )
        .into_iter()
        .enumerate()
        {
            f.setTag((first + k) as isize);
            lists.push((first + k, f));
        }
    }

    let p = tab(&tv, "Keywords Lists", t);
    let mut prefix = vec![];
    for k in 0..8 {
        let (x, y) = (10. + (k % 2) as f64 * 375., 8. + (k / 2) as f64 * 132.);
        p.label(&format!("{} group", ORDINALS[k]), x, y, 90.);
        p.styler(x + 95., y, 4 + k);
        prefix.push(p.check("Prefix mode", x + 190., y, 150., T_PREFIX + k as isize));
        lists.push((19 + k, p.field(x, y + 28., 355., 96., (19 + k) as isize)));
    }

    let p = tab(&tv, "Comment & Number", t);
    p.label("Line comment position", 10., 8., 250.);
    let pure_lc = p.radios(
        &[
            "Allow anywhere",
            "Force at beginning of line",
            "Allow preceding whitespace",
        ],
        30.,
        32.,
        260.,
        T_PURE_LC,
    );
    let fold_comments = p.check("Allow folding of comments", 380., 8., 300., T_FOLD_COMMENTS);
    p.label("Comment line style", 10., 112., 200.);
    p.styler(260., 112., 2);
    let mut comments = rows(
        &p,
        &["Open:", "Continue character:", "Close:"],
        10.,
        142.,
        140.,
        200.,
        22.,
        KW_COMMENTS as isize,
    );
    p.label("Comment style", 380., 112., 200.);
    p.styler(630., 112., 1);
    comments.extend(rows(
        &p,
        &["Open:", "Close:"],
        380.,
        142.,
        60.,
        290.,
        22.,
        KW_COMMENTS as isize,
    ));
    p.label("Number style", 10., 250., 200.);
    p.styler(260., 250., 3);
    let numbers = [
        ("Prefix 1:", 1, 10., 280.),
        ("Prefix 2:", 2, 380., 280.),
        ("Extras 1:", 3, 10., 330.),
        ("Extras 2:", 4, 380., 330.),
        ("Suffix 1:", 5, 10., 380.),
        ("Suffix 2:", 6, 380., 380.),
        ("Range:", 7, 10., 430.),
    ];
    for (l, id, x, y) in numbers {
        p.label(l, x, y, 70.);
        lists.push((id, p.field(x + 70., y, 280., 40., id as isize)));
    }
    p.label("Decimal separator", 380., 430., 200.);
    let decimal = p.radios(&["Dot", "Comma", "Both"], 400., 452., 200., T_DECIMAL);

    let p = tab(&tv, "Operators & Delimiters", t);
    p.label("Operators style", 10., 8., 200.);
    p.styler(260., 8., 12);
    p.label("Operators 1", 10., 36., 300.);
    lists.push((8, p.field(10., 58., 355., 40., 8)));
    p.label("Operators 2 (separators required)", 380., 36., 300.);
    lists.push((9, p.field(380., 58., 355., 40., 9)));
    let mut delims = vec![];
    for d in 0..8 {
        let (x, y) = (10. + (d % 2) as f64 * 375., 108. + (d / 2) as f64 * 110.);
        p.label(&format!("Delimiter {} style", d + 1), x, y, 200.);
        p.styler(x + 270., y, 16 + d);
        delims.extend(rows(
            &p,
            &["Open:", "Escape:", "Close:"],
            x,
            y + 28.,
            60.,
            295.,
            22.,
            KW_DELIMITERS as isize,
        ));
    }
    Ui {
        form,
        lang,
        ext,
        ext_label,
        rename,
        remove,
        case,
        lists,
        comments,
        delims,
        prefix,
        fold_compact,
        fold_comments,
        pure_lc,
        decimal,
        warned: Cell::new(false),
    }
}

fn ui(app: &App) -> &'static Ui {
    UI.with(|c| *c.get_or_init(|| Box::leak(Box::new(build(app)))))
}

fn ask_name(app: &App, title: &str, value: &str) -> Option<String> {
    let a = NSAlert::new(app.mtm());
    a.setMessageText(&ns(title));
    a.setInformativeText(&ns("Name"));
    let f = NSTextField::textFieldWithString(&ns(value), app.mtm());
    f.setFrame(rect(0., 0., 260., 24.));
    a.setAccessoryView(Some(&f));
    a.addButtonWithTitle(&ns("OK"));
    a.addButtonWithTitle(&ns("Cancel"));
    a.window().setInitialFirstResponder(Some(&f));
    if a.runModal() != NSAlertFirstButtonReturn {
        return None;
    }
    Some(text(&f)).filter(|s| !s.is_empty())
}

impl Ui {
    // UserDefineDialog::reloadLangCombo and enableLangAndControlsBy.
    fn reload(&self) {
        let (names, cur) = with(|s| {
            (
                s.langs
                    .iter()
                    .map(|e| e.udl.name.clone())
                    .collect::<Vec<_>>(),
                s.current,
            )
        });
        self.lang.removeAllItems();
        self.lang.addItemWithTitle(&ns("User Defined Language"));
        for n in names {
            self.lang.addItemWithTitle(&ns(""));
            if let Some(i) = self.lang.lastItem() {
                i.setTitle(&ns(&n));
            }
        }
        self.lang.selectItemAtIndex(cur as isize);
        for v in [
            &*self.ext as &NSView,
            &self.ext_label,
            &self.rename,
            &self.remove,
        ] {
            v.setHidden(cur == 0);
        }
        self.show(&with(|s| s.dialog_lang().0));
    }

    // UserDefineDialog::updateDlg and the updateDlg of the four tabs.
    fn show(&self, u: &Udl) {
        self.ext.setStringValue(&ns(&u.ext));
        set_on(&self.case, u.case_ignored);
        for (id, f) in &self.lists {
            f.setStringValue(&ns(&u.keywords[*id]));
        }
        for (k, f) in self.comments.iter().enumerate() {
            f.setStringValue(&ns(&model::retrieve(
                &u.keywords[KW_COMMENTS],
                &model::prefix(k),
            )));
        }
        for (k, f) in self.delims.iter().enumerate() {
            f.setStringValue(&ns(&model::retrieve(
                &u.keywords[KW_DELIMITERS],
                &model::prefix(k),
            )));
        }
        for (b, v) in self.prefix.iter().zip(u.prefix) {
            set_on(b, v);
        }
        set_on(&self.fold_compact, u.fold_compact);
        set_on(&self.fold_comments, u.fold_comments);
        for (k, b) in self.pure_lc.iter().enumerate() {
            set_on(b, u.pure_lc == k as i32);
        }
        for (k, b) in self.decimal.iter().enumerate() {
            set_on(b, u.decimal == k as i32);
        }
    }

    // The setKeywords2List and setPropertyByCheck handlers: one control changes the language.
    fn read(&self, tag: isize, u: &mut Udl) {
        let joined = |fields: &[Retained<NSTextField>]| {
            let mut d = String::new();
            for (k, f) in fields.iter().enumerate() {
                model::convert_to(&mut d, &text(f), &model::prefix(k));
            }
            d
        };
        match tag {
            0 => u.keywords[KW_COMMENTS] = joined(&self.comments),
            27 => u.keywords[KW_DELIMITERS] = joined(&self.delims),
            1..=26 => {
                if let Some((_, f)) = self.lists.iter().find(|(id, _)| *id as isize == tag) {
                    u.keywords[tag as usize] = text(f);
                }
            }
            T_CASE => u.case_ignored = on(&self.case),
            T_FOLD_COMPACT => u.fold_compact = on(&self.fold_compact),
            T_FOLD_COMMENTS => u.fold_comments = on(&self.fold_comments),
            T_EXT => u.ext = text(&self.ext),
            t if (T_PREFIX..T_PREFIX + 8).contains(&t) => {
                u.prefix[(t - T_PREFIX) as usize] = on(&self.prefix[(t - T_PREFIX) as usize])
            }
            t if (T_PURE_LC..T_PURE_LC + 3).contains(&t) => u.pure_lc = (t - T_PURE_LC) as i32,
            t if (T_DECIMAL..T_DECIMAL + 3).contains(&t) => u.decimal = (t - T_DECIMAL) as i32,
            _ => {}
        }
    }
}

fn sizes_index(size: i32) -> isize {
    FONT_SIZES
        .iter()
        .position(|s| {
            *s == if size == -1 {
                String::new()
            } else {
                size.to_string()
            }
        })
        .map_or(-1, |i| i as isize)
}

fn color(c: Option<u32>, default: u32) -> Retained<NSColor> {
    let c = c.unwrap_or(default);
    let f = |s: u32| ((c >> s) & 0xFF) as f64 / 255.;
    NSColor::colorWithSRGBRed_green_blue_alpha(f(16), f(8), f(0), 1.)
}

fn rgb(w: &NSColorWell) -> u32 {
    let Some(c) = w
        .color()
        .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
    else {
        return 0;
    };
    let v = |x: f64| (x.clamp(0., 1.) * 255.).round() as u32;
    (v(c.redComponent()) << 16) | (v(c.greenComponent()) << 8) | v(c.blueComponent())
}

// StylerDlg (IDD_STYLER_POPUP_DLG) as an alert; the nesting boxes are on only for comment and delimiter styles.
fn styler(app: &App, s: &UStyle) -> Option<UStyle> {
    let mtm = app.mtm();
    let nest_on = matches!(s.id, 1 | 2 | 16..=23);
    let p = Pane {
        view: NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., 460., 330.)),
        h: 330.,
        t: app,
        mtm,
    };
    let plain = |b: &NSButton| unsafe {
        b.setTarget(None);
        b.setAction(None);
    };
    p.label("Font options", 0., 0., 200.);
    p.label("Name:", 10., 26., 50.);
    let names = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(0., 0., 200., 26.),
        false,
    );
    names.addItemWithTitle(&ns(""));
    let mut fonts: Vec<String> = NSFontManager::sharedFontManager(mtm)
        .availableFontFamilies()
        .iter()
        .map(|f| f.to_string())
        .collect();
    fonts.sort();
    for f in &fonts {
        names.addItemWithTitle(&ns(""));
        if let Some(i) = names.lastItem() {
            i.setTitle(&ns(f));
        }
    }
    names.selectItemAtIndex(
        fonts
            .iter()
            .position(|f| *f == s.font_name)
            .map_or(0, |i| i as isize + 1),
    );
    let name0 = names.indexOfSelectedItem();
    p.put(&names, 60., 24., 200., 26.);
    p.label("Size:", 10., 56., 50.);
    let sizes = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(0., 0., 80., 26.),
        false,
    );
    FONT_SIZES
        .iter()
        .for_each(|z| sizes.addItemWithTitle(&ns(z)));
    sizes.selectItemAtIndex(sizes_index(s.font_size).max(0));
    let size0 = sizes.indexOfSelectedItem();
    p.put(&sizes, 60., 54., 80., 26.);
    let fs = s.font_style.max(0);
    let styles: Vec<Retained<NSButton>> = [("Bold", 1), ("Italic", 2), ("Underline", 4)]
        .iter()
        .enumerate()
        .map(|(k, (n, bit))| {
            let b = p.check(n, 300., 26. + k as f64 * 22., 140., 0);
            plain(&b);
            set_on(&b, fs & bit != 0);
            b
        })
        .collect();
    let mut wells = vec![];
    let mut clear = vec![];
    for (k, (title, c, def, bit)) in [
        ("Foreground color:", s.fg, 0, COLORSTYLE_FG),
        ("Background color:", s.bg, 0xFFFFFF, COLORSTYLE_BG),
    ]
    .into_iter()
    .enumerate()
    {
        let x = k as f64 * 230.;
        p.label(title, x, 100., 130.);
        let w = NSColorWell::initWithFrame(NSColorWell::alloc(mtm), rect(0., 0., 44., 24.));
        w.setColor(&color(c, def));
        p.put(&w, x + 135., 98., 44., 24.);
        let t = p.check("Transparent", x + 10., 128., 150., 0);
        plain(&t);
        set_on(&t, s.color_style & bit == 0);
        wells.push(w);
        clear.push(t);
    }
    p.label("Nesting", 0., 160., 200.);
    let mut nest = vec![];
    for (col, items) in NESTING.iter().enumerate() {
        for (row, (n, mask)) in items.iter().enumerate().filter(|(_, (n, _))| !n.is_empty()) {
            let b = p.check(n, 10. + col as f64 * 150., 184. + row as f64 * 18., 140., 0);
            plain(&b);
            set_on(&b, s.nesting & mask != 0);
            b.setEnabled(nest_on);
            nest.push((b, *mask));
        }
    }
    let a = NSAlert::new(mtm);
    a.setMessageText(&ns("Styler Dialog"));
    a.setAccessoryView(Some(&p.view));
    a.addButtonWithTitle(&ns("OK"));
    a.addButtonWithTitle(&ns("Cancel"));
    if a.runModal() != NSAlertFirstButtonReturn {
        return None;
    }
    let mut out = s.clone();
    if names.indexOfSelectedItem() != name0 {
        out.font_name = match names.indexOfSelectedItem() {
            i if i > 0 => fonts.get(i as usize - 1).cloned().unwrap_or_default(),
            _ => String::new(),
        };
    }
    if sizes.indexOfSelectedItem() != size0 {
        out.font_size = FONT_SIZES
            .get(sizes.indexOfSelectedItem().max(0) as usize)
            .and_then(|z| z.parse().ok())
            .unwrap_or(-1);
    }
    out.fg = Some(rgb(&wells[0]));
    out.bg = Some(rgb(&wells[1]));
    out.color_style = (if on(&clear[0]) { 0 } else { COLORSTYLE_FG })
        | (if on(&clear[1]) { 0 } else { COLORSTYLE_BG });
    let font_style = [1, 2, 4]
        .iter()
        .zip(&styles)
        .filter(|(_, b)| on(b))
        .fold(0, |a, (bit, _)| a | bit);
    if font_style != fs {
        out.font_style = font_style;
    }
    if nest.iter().any(|(b, m)| on(b) != (s.nesting & m != 0)) {
        out.nesting = nest
            .iter()
            .filter(|(b, _)| on(b))
            .fold(0, |a, (_, m)| a | m);
    }
    Some(out)
}

impl App {
    pub(crate) fn udl_dialog_visible(&self) -> bool {
        UI.with(|c| c.get().is_some_and(|u| u.form.panel.isVisible()))
    }

    // IDM_LANG_USER_DLG: shows or hides the dialog.
    pub(crate) fn define_udl(&self) {
        let u = ui(self);
        if u.form.panel.isVisible() {
            u.form.panel.orderOut(None);
            self.udl_flush();
            return;
        }
        u.reload();
        u.form.panel.makeKeyAndOrderFront(None);
    }

    pub(crate) fn udl_lang(&self) {
        let u = ui(self);
        let i = u.lang.indexOfSelectedItem().max(0) as usize;
        with(|s| s.current = i.min(s.langs.len()));
        u.reload();
    }

    // Saves the file of the dialog language and colours the UDL tabs again.
    // Colours the UDL tabs again now; the file write waits until the edits stop, as Notepad++ writes only at exit.
    fn udl_changed(&self, all: bool) {
        with(|s| {
            if let Some(f) = s
                .current
                .checked_sub(1)
                .and_then(|i| s.langs.get(i))
                .map(|e| e.file.clone())
            {
                if !s.dirty.contains(&f) {
                    s.dirty.push(f);
                }
            }
        });
        let none = None::<&AnyObject>;
        unsafe {
            let _: () = msg_send![class!(NSObject), cancelPreviousPerformRequestsWithTarget: self, selector: sel!(udlSave:), object: none];
            let _: () = msg_send![self, performSelector: sel!(udlSave:), withObject: none, afterDelay: 1.0f64];
        }
        if all {
            self.reapply_all_tabs();
        } else {
            self.reapply_udl_tabs();
        }
    }

    // Writes the UDL files that have edits; an error shows once.
    pub(crate) fn udl_flush(&self) {
        let errors: Vec<String> = with(|s| {
            let files = std::mem::take(&mut s.dirty);
            files.iter().filter_map(|f| s.save(f).err()).collect()
        });
        if let Some(e) = errors.first() {
            let warned = UI.with(|c| c.get().is_some_and(|u| u.warned.replace(true)));
            if !warned {
                self.alert("Cannot save the user defined language.", e, &["OK"]);
            }
        }
    }

    fn with_dialog_lang(&self, f: impl FnOnce(&mut Udl)) {
        with(|s| match s.current.checked_sub(1) {
            Some(i) => {
                if let Some(e) = s.langs.get_mut(i) {
                    f(&mut e.udl)
                }
            }
            None => f(&mut s.scratch),
        });
    }

    pub(crate) fn udl_edit(&self, tag: isize) {
        let u = ui(self);
        self.with_dialog_lang(|l| u.read(tag, l));
        self.udl_changed(tag == T_EXT);
    }

    pub(crate) fn udl_styler(&self, id: isize) {
        let Some(cur) = with(|s| s.dialog_lang().0.style(id as usize).cloned()) else {
            return;
        };
        let Some(new) = styler(self, &cur) else {
            return;
        };
        self.with_dialog_lang(|l| {
            if let Some(s) = l.styles.iter_mut().find(|s| s.id == new.id) {
                *s = new;
            }
        });
        self.udl_changed(false);
    }

    fn renamed(&self, old: &str, new: Option<&str>) {
        let old = format!("{PREFIX}{old}");
        for t in self.ivars().tabs.borrow_mut().iter_mut() {
            if t.lang.as_deref() == Some(old.as_str()) {
                t.lang = Some(new.map_or("normal".into(), |n| format!("{PREFIX}{n}")));
            }
        }
    }

    fn after_list_change(&self) {
        ui(self).reload();
        self.refresh_udl_menu();
        self.reapply_all_tabs();
    }

    pub(crate) fn udl_command(&self, cmd: isize) {
        let cur = with(|s| s.current);
        let name_error = "This name is used by another language,\nplease give another one.";
        match cmd {
            C_NEW | C_SAVE_AS => {
                let save_as = cmd == C_SAVE_AS && cur > 0;
                let title = if save_as {
                    "Save Current Language Name As..."
                } else {
                    "Create New Language..."
                };
                let Some(name) = ask_name(self, title, "") else {
                    return;
                };
                if with(|s| s.exists(&name)) {
                    self.alert(name_error, "", &["OK"]);
                    return;
                }
                let r = with(|s| {
                    let src = match s.langs.get(cur.wrapping_sub(1)) {
                        Some(e) if save_as => e.udl.clone(),
                        _ => s.scratch.clone(),
                    };
                    let i = s.add(src.copy_as(&name))?;
                    s.current = i + 1;
                    Ok::<_, String>(())
                });
                if let Err(e) = r {
                    self.alert("Cannot save the user defined language.", &e, &["OK"]);
                }
                self.after_list_change();
            }
            C_RENAME if cur > 0 => {
                let Some(old) = with(|s| s.langs.get(cur - 1).map(|e| e.udl.name.clone())) else {
                    return;
                };
                let Some(name) = ask_name(self, "Rename Current Language Name", &old) else {
                    return;
                };
                if name == old {
                    return;
                }
                if with(|s| s.exists(&name)) {
                    self.alert(name_error, "", &["OK"]);
                    return;
                }
                with(|s| {
                    if let Some(e) = s.langs.get_mut(cur - 1) {
                        e.udl.name = name.clone();
                    }
                });
                self.renamed(&old, Some(&name));
                self.udl_changed(false);
                self.udl_flush();
                self.after_list_change();
            }
            C_REMOVE if cur > 0 => {
                if self.alert(
                    "Are you sure?",
                    "Remove the current language",
                    &["Yes", "No"],
                ) != NSAlertFirstButtonReturn
                {
                    return;
                }
                let (old, r) = with(|s| {
                    let e = s.langs.remove(cur - 1);
                    s.current = cur - 1;
                    (e.udl.name, s.save(&e.file))
                });
                if let Err(e) = r {
                    self.alert("Cannot save the user defined language.", &e, &["OK"]);
                }
                self.renamed(&old, None);
                self.after_list_change();
            }
            C_IMPORT => {
                let p = NSOpenPanel::openPanel(self.mtm());
                if p.runModal() != NSModalResponseOK {
                    return;
                }
                let Some(path) = p.URL().and_then(|u| u.path()) else {
                    return;
                };
                match with(|s| s.import(&PathBuf::from(path.to_string()))) {
                    Ok(n) if n > 0 => {
                        self.alert("Import successful.", "", &["OK"]);
                        self.after_list_change();
                    }
                    Ok(_) => {
                        self.alert(
                            "Failed to import.",
                            "You can have 30 languages at most.",
                            &["OK"],
                        );
                    }
                    Err(e) => {
                        self.alert("Failed to import.", &e, &["OK"]);
                    }
                }
            }
            C_EXPORT => {
                if cur == 0 {
                    self.alert(
                        "Before exporting, save your language definition by clicking \"Save As...\" button",
                        "",
                        &["OK"],
                    );
                    return;
                }
                let Some(lang) = with(|s| s.langs.get(cur - 1).map(|e| e.udl.clone())) else {
                    return;
                };
                let p = NSSavePanel::savePanel(self.mtm());
                p.setNameFieldStringValue(&ns(&format!("{}.xml", lang.name)));
                if p.runModal() != NSModalResponseOK {
                    return;
                }
                let Some(path) = p.URL().and_then(|u| u.path()) else {
                    return;
                };
                let xml = model::to_xml(model::DECLARATION, &[&lang]);
                match crate::session::write_file(&PathBuf::from(path.to_string()), &xml, false) {
                    Ok(()) => self.alert("Export successful.", "", &["OK"]),
                    Err(e) => self.alert("Failed to export.", &e, &["OK"]),
                };
            }
            C_DOC => {
                if let Some(url) = NSURL::URLWithString(&ns(DOC)) {
                    NSWorkspace::sharedWorkspace().openURL(&url);
                }
            }
            _ => {}
        }
    }
}
