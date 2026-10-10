// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{app_support_dir, Config};
use crate::panel::{on, set_on, Form};
use crate::styler::{self, Doc, El, Override, Src};
use crate::{item, lang, nested, ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlertFirstButtonReturn, NSButton, NSColor, NSColorPanel, NSColorSpace, NSColorWell,
    NSControlTextEditingDelegate, NSFontManager, NSMenuItem, NSModalResponseOK, NSOpenPanel,
    NSPopUpButton, NSScrollView, NSTableColumn, NSTableView, NSTableViewDataSource,
    NSTableViewDelegate, NSTextDelegate, NSTextField, NSTextFieldDelegate, NSTextView,
    NSTextViewDelegate, NSView, NSWindowDelegate,
};
use objc2_foundation::{
    NSIndexSet, NSInteger, NSNotification, NSObject, NSObjectProtocol,
};
use std::cell::{Cell, OnceCell, RefCell};
use std::path::Path;

const SCI_SETWHITESPACEFORE: u32 = 2084;
// NppConstants.h fontSizeStrs.
const FONT_SIZES: [&str; 17] = [
    "", "5", "6", "7", "8", "9", "10", "11", "12", "14", "16", "18", "20", "22", "24", "26", "28",
];
// WordStyleDlg.rc IDC_GLOBAL_*_CHECK, in the order of styler::OVERRIDE_KEYS.
const OVERRIDE_LABELS: [&str; 7] = [
    "Force foreground color for all styles",
    "Force background color for all styles",
    "Force font choice for all styles",
    "Force font size choice for all styles",
    "Force bold choice for all styles",
    "Force italic choice for all styles",
    "Force underline choice for all styles",
];
const GLOBAL_TIP: &str = "Enabling \"Global override\" here will override that parameter in all language styles. What you probably really want is to use the \"Default Style\" settings instead";

// Notepad_plus.rc Settings menu items of this slice.
pub fn configurator_item(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    item(
        mtm,
        "Style Configurator...",
        sel!(styleConfigurator:),
        "",
        t,
    )
}

pub fn import_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    nested(
        mtm,
        "Import",
        vec![item(
            mtm,
            "Import style theme(s)...",
            sel!(importStyleThemes:),
            "",
            t,
        )],
    )
}

// The Settings menu until the Preferences slice adds its own.
pub fn settings_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    vec![
        configurator_item(mtm, t),
        NSMenuItem::separatorItem(mtm),
        import_menu(mtm, t),
    ]
}

struct St {
    themes: Vec<(String, Src)>,
    theme: usize,
    src: Src,
    doc: Doc,
    go: Override,
    lexers: Vec<usize>,
    lexer: usize,
    style: usize,
    theme_dirty: bool,
    changed: bool,
}

fn has_id(w: &&El) -> bool {
    w.get("styleID").is_some()
}

impl St {
    fn lexer_el(&self) -> Option<&El> {
        let i = *self.lexers.get(self.lexer.checked_sub(1)?)?;
        self.doc.lexers().get(i).copied()
    }

    fn styles(&self) -> Vec<&El> {
        match self.lexer {
            0 => self.doc.widgets().into_iter().filter(has_id).collect(),
            _ => self
                .lexer_el()
                .map_or(vec![], |l| l.els("WordsStyle").collect()),
        }
    }

    fn style_el(&self) -> Option<&El> {
        self.styles().get(self.style).copied()
    }

    fn lexer_mut(&mut self) -> Option<&mut El> {
        let i = *self.lexers.get(self.lexer.checked_sub(1)?)?;
        self.doc
            .root_mut()?
            .first_mut("LexerStyles")?
            .els_mut("LexerType")
            .nth(i)
    }

    fn style_mut(&mut self) -> Option<&mut El> {
        let n = self.style;
        if self.lexer == 0 {
            return self
                .doc
                .root_mut()?
                .first_mut("GlobalStyles")?
                .els_mut("WidgetStyle")
                .filter(|w| w.get("styleID").is_some())
                .nth(n);
        }
        self.lexer_mut()?.els_mut("WordsStyle").nth(n)
    }

    fn lang_titles(&self) -> Vec<String> {
        let lexers = self.doc.lexers();
        std::iter::once("Global Styles".to_string())
            .chain(
                self.lexers
                    .iter()
                    .map(|&i| lexers[i].get("desc").unwrap_or("").to_string()),
            )
            .collect()
    }
}

struct Ui {
    form: Form,
    theme: Retained<NSPopUpButton>,
    lang: Retained<NSPopUpButton>,
    table: Retained<NSTableView>,
    desc: Retained<NSTextField>,
    fg: Retained<NSColorWell>,
    bg: Retained<NSColorWell>,
    font: Retained<NSPopUpButton>,
    size: Retained<NSPopUpButton>,
    font_style: [Retained<NSButton>; 3],
    def_ext: Retained<NSTextField>,
    user_ext: Retained<NSTextField>,
    ext_views: Vec<Retained<NSView>>,
    def_kw: Retained<NSTextView>,
    user_kw: Retained<NSTextView>,
    kw_views: Vec<Retained<NSView>>,
    go: Vec<Retained<NSButton>>,
    go_tip: Retained<NSTextField>,
    save: Retained<NSButton>,
}

#[derive(Default)]
struct Ivars {
    app: OnceCell<Retained<App>>,
    ui: OnceCell<Ui>,
    st: RefCell<Option<St>>,
    rows: RefCell<Vec<String>>,
    filling: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct StyleDlg;

    impl StyleDlg {
        #[unsafe(method(themeChanged:))]
        fn theme_changed(&self, _s: Option<&AnyObject>) {
            self.switch_theme();
        }

        #[unsafe(method(langChanged:))]
        fn lang_changed(&self, _s: Option<&AnyObject>) {
            let i = self.ui().lang.indexOfSelectedItem().max(0) as usize;
            self.with(|s| {
                s.lexer = i;
                s.style = 0;
            });
            self.fill_lexer();
        }

        #[unsafe(method(colourChanged:))]
        fn colour_changed(&self, w: &NSColorWell) {
            let key = if std::ptr::eq(w, &*self.ui().fg) { "fgColor" } else { "bgColor" };
            let hex = to_hex(&w.color());
            self.edit(|e| e.set(key, &hex));
        }

        #[unsafe(method(fontChanged:))]
        fn font_changed(&self, p: &NSPopUpButton) {
            let name = if p.indexOfSelectedItem() <= 0 {
                String::new()
            } else {
                p.titleOfSelectedItem().map_or(String::new(), |t| t.to_string())
            };
            self.edit(|e| e.set("fontName", &name));
        }

        #[unsafe(method(sizeChanged:))]
        fn size_changed(&self, p: &NSPopUpButton) {
            let i = p.indexOfSelectedItem().max(0) as usize;
            self.edit(|e| e.set("fontSize", FONT_SIZES[i.min(FONT_SIZES.len() - 1)]));
        }

        #[unsafe(method(fontStyleChanged:))]
        fn font_style_changed(&self, b: &NSButton) {
            let bit = b.tag() as u32;
            let set = on(b);
            self.edit(|e| {
                let f: u32 = e.get("fontStyle").and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                e.set("fontStyle", &(if set { f | bit } else { f & !bit }).to_string());
            });
        }

        #[unsafe(method(overrideChanged:))]
        fn override_changed(&self, b: &NSButton) {
            let i = b.tag() as usize;
            let set = on(b);
            self.with(|s| {
                s.go[i] = set;
                s.changed = true;
            });
            self.apply();
        }

        #[unsafe(method(saveClose:))]
        fn save_close(&self, _s: Option<&AnyObject>) {
            self.save();
        }

        #[unsafe(method(cancel:))]
        fn cancel_action(&self, _s: Option<&AnyObject>) {
            self.cancel();
        }
    }

    unsafe impl NSObjectProtocol for StyleDlg {}

    unsafe impl NSWindowDelegate for StyleDlg {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _w: &AnyObject) -> bool {
            self.cancel();
            false
        }
    }

    unsafe impl NSTableViewDataSource for StyleDlg {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _t: &NSTableView) -> NSInteger {
            self.ivars().rows.borrow().len() as NSInteger
        }

        #[unsafe(method_id(tableView:objectValueForTableColumn:row:))]
        fn value(
            &self,
            _t: &NSTableView,
            _c: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<AnyObject>> {
            let rows = self.ivars().rows.borrow();
            rows.get(row as usize)
                .map(|s| Retained::into_super(Retained::into_super(ns(s))))
        }
    }

    unsafe impl NSTableViewDelegate for StyleDlg {
        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, _n: &NSNotification) {
            if self.ivars().filling.get() {
                return;
            }
            let r = self.ui().table.selectedRow();
            if r >= 0 {
                self.with(|s| s.style = r as usize);
                self.fill_style();
            }
        }
    }

    unsafe impl NSControlTextEditingDelegate for StyleDlg {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _n: &NSNotification) {
            let ext = self.ui().user_ext.stringValue().to_string();
            self.mark(|s| {
                if let Some(l) = s.lexer_mut() {
                    l.set("ext", &ext);
                }
            });
        }
    }

    unsafe impl NSTextFieldDelegate for StyleDlg {}

    unsafe impl NSTextDelegate for StyleDlg {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, _n: &NSNotification) {
            let words = self.ui().user_kw.string().to_string();
            self.edit(|e| e.set_text(&words));
        }
    }

    unsafe impl NSTextViewDelegate for StyleDlg {}
);

thread_local! {
    static DLG: OnceCell<Retained<StyleDlg>> = const { OnceCell::new() };
}

fn to_ns(hex: &str) -> Retained<NSColor> {
    let v = u32::from_str_radix(hex.trim(), 16).unwrap_or(0);
    let c = |s: u32| ((v >> s) & 0xFF) as f64 / 255.;
    NSColor::colorWithSRGBRed_green_blue_alpha(c(16), c(8), c(0), 1.)
}

fn to_hex(c: &NSColor) -> String {
    let c = c
        .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
        .unwrap_or_else(|| c.retain());
    let b = |x: f64| (x.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "{:02X}{:02X}{:02X}",
        b(c.redComponent()),
        b(c.greenComponent()),
        b(c.blueComponent())
    )
}

fn fill_popup(p: &NSPopUpButton, titles: &[String]) {
    p.removeAllItems();
    let Some(m) = p.menu() else { return };
    for t in titles {
        unsafe { m.addItemWithTitle_action_keyEquivalent(&ns(t), None, &ns("")) };
    }
}

fn text_box(
    mtm: MainThreadMarker,
    f: &Form,
    x: f64,
    top: f64,
    w: f64,
    h: f64,
    editable: bool,
) -> (Retained<NSScrollView>, Retained<NSTextView>) {
    let sv = NSTextView::scrollableTextView(mtm);
    let tv: Retained<NSTextView> = unsafe { msg_send![&sv, documentView] };
    tv.setEditable(editable);
    tv.setRichText(false);
    f.place(&sv, x, top, w, h);
    (sv, tv)
}

impl StyleDlg {
    fn create(mtm: MainThreadMarker, app: &App) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars::default());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let _ = this.ivars().app.set(app.retain());
        let _ = this.ivars().ui.set(this.build(mtm));
        this
    }

    fn build(&self, mtm: MainThreadMarker) -> Ui {
        let t: &AnyObject = self.as_ref();
        let f = Form::new(mtm, "Style Configurator", 770., 430.);
        f.panel.setDelegate(Some(ProtocolObject::from_ref(self)));
        f.panel.setFloatingPanel(false);
        let popup = |x, top, w, action| {
            let p = NSPopUpButton::new(mtm);
            unsafe {
                p.setTarget(Some(t));
                p.setAction(Some(action));
            }
            f.place(&p, x, top, w, 26.);
            p
        };
        f.label("Select theme:", 16., 12., 90.);
        let theme = popup(110., 10., 240., sel!(themeChanged:));
        f.label("Language:", 16., 46., 200.);
        let lang = popup(16., 66., 250., sel!(langChanged:));
        f.label("Style:", 16., 98., 200.);
        let table = NSTableView::new(mtm);
        let col = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns("style"));
        col.setWidth(230.);
        table.addTableColumn(&col);
        table.setHeaderView(None);
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
        }
        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setHasVerticalScroller(true);
        f.place(&scroll, 16., 118., 250., 226.);
        let ext_l1 = NSTextField::labelWithString(&ns("Default ext.:"), mtm);
        f.place(&ext_l1, 16., 352., 110., 18.);
        let ext_l2 = NSTextField::labelWithString(&ns("User ext.:"), mtm);
        f.place(&ext_l2, 140., 352., 120., 18.);
        let def_ext = f.field(16., 372., 110.);
        def_ext.setEditable(false);
        let user_ext = f.field(140., 372., 126.);
        unsafe { user_ext.setDelegate(Some(ProtocolObject::from_ref(self))) };
        let desc = f.status(290., 46., 460., 20.);
        f.label("Colour Style", 290., 74., 200.);
        f.label("Foreground colour", 290., 98., 130.);
        f.label("Background colour", 290., 130., 130.);
        let well = |top| {
            let w = NSColorWell::new(mtm);
            w.setSupportsAlpha(false);
            unsafe {
                w.setTarget(Some(t));
                w.setAction(Some(sel!(colourChanged:)));
            }
            f.place(&w, 425., top, 44., 26.);
            w
        };
        let (fg, bg) = (well(96.), well(128.));
        f.label("Font Style", 500., 74., 200.);
        f.label("Font name:", 500., 98., 75.);
        let font = popup(578., 96., 172., sel!(fontChanged:));
        let mut fonts = vec![String::new()];
        fonts.extend(
            NSFontManager::sharedFontManager(mtm)
                .availableFontFamilies()
                .iter()
                .map(|s| s.to_string()),
        );
        fill_popup(&font, &fonts);
        f.label("Font size:", 620., 130., 70.);
        let size = popup(690., 128., 60., sel!(sizeChanged:));
        fill_popup(&size, &FONT_SIZES.map(String::from));
        let font_style = [("Bold", 1), ("Italic", 2), ("Underline", 4)]
            .iter()
            .enumerate()
            .map(|(i, (n, bit))| {
                let b = f.check(
                    n,
                    500.,
                    128. + 22. * i as f64,
                    110.,
                    t,
                    sel!(fontStyleChanged:),
                );
                b.setTag(*bit);
                b
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let kw_l1 = NSTextField::labelWithString(&ns("Default keywords"), mtm);
        f.place(&kw_l1, 290., 204., 200., 18.);
        let kw_l2 = NSTextField::labelWithString(&ns("User-defined keywords"), mtm);
        f.place(&kw_l2, 525., 204., 220., 18.);
        let (def_sv, def_kw) = text_box(mtm, &f, 290., 224., 220., 140., false);
        let (user_sv, user_kw) = text_box(mtm, &f, 525., 224., 225., 140., true);
        user_kw.setDelegate(Some(ProtocolObject::from_ref(self)));
        let go = OVERRIDE_LABELS
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let (x, top) = if i < 2 {
                    (290., 204. + 24. * i as f64)
                } else {
                    (525., 204. + 24. * (i - 2) as f64)
                };
                let b = f.check(l, x, top, 235., t, sel!(overrideChanged:));
                b.setTag(i as isize);
                b
            })
            .collect();
        let go_tip = f.status(290., 260., 225., 90.);
        go_tip.setStringValue(&ns(GLOBAL_TIP));
        let save = f.button("Save & Close", 520., 390., 120., t, sel!(saveClose:));
        let cancel = f.button("Cancel", 645., 390., 105., t, sel!(cancel:));
        cancel.setKeyEquivalent(&ns("\u{1b}"));
        Ui {
            form: f,
            theme,
            lang,
            table,
            desc,
            fg,
            bg,
            font,
            size,
            font_style,
            def_ext,
            user_ext,
            ext_views: vec![
                ext_l1.into_super().into_super(),
                ext_l2.into_super().into_super(),
            ],
            def_kw,
            user_kw,
            kw_views: vec![
                kw_l1.into_super().into_super(),
                kw_l2.into_super().into_super(),
                def_sv.into_super(),
                user_sv.into_super(),
            ],
            go,
            go_tip,
            save,
        }
    }

    fn ui(&self) -> &Ui {
        self.ivars().ui.get().unwrap()
    }

    fn app(&self) -> &App {
        self.ivars().app.get().unwrap()
    }

    fn with<R>(&self, f: impl FnOnce(&mut St) -> R) -> R {
        f(self.ivars().st.borrow_mut().as_mut().unwrap())
    }

    // Starts a session of the dialog from the styles in use.
    fn open(&self) {
        let dir = app_support_dir();
        let themes = styler::theme_list(&styler::user_themes(dir.as_deref()));
        let st = styler::with_current(|c| {
            let theme = themes.iter().position(|(_, s)| *s == c.src).unwrap_or(0);
            St {
                lexers: styler::sorted_lexers(&c.doc),
                themes,
                theme,
                src: c.src.clone(),
                doc: c.doc.clone(),
                go: c.go,
                lexer: 0,
                style: 0,
                theme_dirty: false,
                changed: false,
            }
        });
        *self.ivars().st.borrow_mut() = Some(st);
        self.fill_themes();
        self.fill_langs();
        self.ui().save.setEnabled(false);
        self.ui().form.panel.makeKeyAndOrderFront(None);
    }

    fn fill_themes(&self) {
        let (names, i) = self.with(|s| {
            (
                s.themes.iter().map(|t| t.0.clone()).collect::<Vec<_>>(),
                s.theme,
            )
        });
        fill_popup(&self.ui().theme, &names);
        self.ui().theme.selectItemAtIndex(i as isize);
    }

    fn fill_langs(&self) {
        let (titles, i) = self.with(|s| (s.lang_titles(), s.lexer));
        fill_popup(&self.ui().lang, &titles);
        self.ui().lang.selectItemAtIndex(i as isize);
        self.fill_lexer();
    }

    // WordStyleDlg::setStyleListFromLexer.
    fn fill_lexer(&self) {
        let ui = self.ui();
        let (rows, exts, style) = self.with(|s| {
            let rows: Vec<String> = s
                .styles()
                .iter()
                .map(|e| e.get("name").unwrap_or("").to_string())
                .collect();
            let exts = s.lexer_el().map(|l| {
                let name = l.get("name").unwrap_or("");
                let def = styler::base_language(name).map_or(String::new(), |l| l.exts.join(" "));
                (def, l.get("ext").unwrap_or("").to_string())
            });
            (rows, exts, s.style)
        });
        *self.ivars().rows.borrow_mut() = rows;
        self.ivars().filling.set(true);
        ui.table.reloadData();
        ui.table
            .selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(style), false);
        self.ivars().filling.set(false);
        let show = exts.is_some();
        ui.ext_views.iter().for_each(|v| v.setHidden(!show));
        ui.def_ext.setHidden(!show);
        ui.user_ext.setHidden(!show);
        if let Some((def, user)) = exts {
            ui.def_ext.setStringValue(&ns(&def));
            ui.user_ext.setStringValue(&ns(&user));
        }
        self.fill_style();
    }

    // WordStyleDlg::setVisualFromStyleList.
    fn fill_style(&self) {
        let ui = self.ui();
        let Some((el, lexer_name, lang_title)) = self.with(|s| {
            let lexer = s
                .lexer_el()
                .and_then(|l| l.get("name"))
                .unwrap_or("")
                .to_string();
            let title = s.lang_titles().get(s.lexer).cloned().unwrap_or_default();
            s.style_el().map(|e| (e.clone(), lexer, title))
        }) else {
            return;
        };
        let name = el.get("name").unwrap_or("");
        ui.desc
            .setStringValue(&ns(&format!("{lang_title}: {name}")));
        for (w, key) in [(&ui.fg, "fgColor"), (&ui.bg, "bgColor")] {
            let v = el.get(key);
            w.deactivate();
            if let Some(v) = v {
                w.setColor(&to_ns(v));
            }
            w.setEnabled(v.is_some() && !(key == "fgColor" && name == "Selected text colour"));
        }
        let font_on = el.get("fontName").is_some();
        let font = el.get("fontName").unwrap_or("");
        let fi = if font.is_empty() {
            0
        } else {
            ui.font.indexOfItemWithTitle(&ns(font)).max(0)
        };
        ui.font.selectItemAtIndex(fi);
        ui.font.setEnabled(font_on);
        let size = el.get("fontSize").unwrap_or("").trim();
        let si = FONT_SIZES
            .iter()
            .skip(1)
            .position(|s| *s == size)
            .map_or(0, |p| p + 1);
        ui.size.selectItemAtIndex(si as isize);
        ui.size.setEnabled(font_on);
        let fs: u32 = el
            .get("fontStyle")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        for b in &ui.font_style {
            set_on(b, fs & b.tag() as u32 != 0);
            b.setEnabled(font_on);
        }
        let class = el.get("keywordClass").filter(|c| !c.is_empty());
        ui.kw_views
            .iter()
            .for_each(|v| v.setHidden(class.is_none()));
        if let Some(c) = class {
            let def = styler::base_language(&lexer_name)
                .and_then(|l| l.keywords.iter().find(|k| k.0 == c))
                .map_or("", |k| k.1.as_str());
            ui.def_kw.setString(&ns(def.trim()));
            ui.user_kw.setString(&ns(&el.text()));
        }
        let global = name == "Global override";
        let go = self.with(|s| s.go);
        for (b, v) in ui.go.iter().zip(go) {
            set_on(b, v);
            b.setHidden(!global);
        }
        ui.go_tip.setHidden(!global);
    }

    fn mark(&self, f: impl FnOnce(&mut St)) {
        self.with(|s| {
            f(s);
            s.theme_dirty = true;
            s.changed = true;
        });
        self.ui().save.setEnabled(true);
        self.apply();
    }

    fn edit(&self, f: impl FnOnce(&mut El)) {
        self.mark(|s| {
            if let Some(e) = s.style_mut() {
                f(e)
            }
        });
    }

    // Live preview in all editors.
    fn apply(&self) {
        let c = self.with(|s| styler::build(&s.doc, &s.go));
        self.app().restyle(&c);
    }

    // WordStyleDlg::switchToTheme.
    fn switch_theme(&self) {
        let i = self.ui().theme.indexOfSelectedItem().max(0) as usize;
        let (dirty, old, new) =
            self.with(|s| (s.theme_dirty, s.src.clone(), s.themes[i].1.clone()));
        if dirty {
            let msg = "Unsaved changes are about to be discarded!\nDo you want to save your changes before switching themes?";
            let title = Path::new(&old.file_name())
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if self.app().alert(&title, msg, &["Yes", "No"]) == NSAlertFirstButtonReturn {
                if let Err(e) = self.write_theme() {
                    self.app().alert("Cannot save the style theme", &e, &["OK"]);
                }
            }
        }
        let dir = app_support_dir();
        match styler::load(&new, dir.as_deref()) {
            Ok(doc) => {
                self.with(|s| {
                    s.lexers = styler::sorted_lexers(&doc);
                    s.doc = doc;
                    s.src = new;
                    s.theme = i;
                    s.lexer = 0;
                    s.style = 0;
                    s.theme_dirty = false;
                    s.changed = true;
                });
                self.ui().save.setEnabled(true);
                self.fill_langs();
                self.apply();
            }
            Err(e) => {
                self.app().alert(
                    "Load stylers.xml failed",
                    &format!("Load \"{e}\" failed!"),
                    &["OK"],
                );
                let cur = self.with(|s| s.theme);
                self.ui().theme.selectItemAtIndex(cur as isize);
            }
        }
    }

    // NppParameters::writeStyles: the styles go to the theme file, or to the user copy of an installed theme.
    fn write_theme(&self) -> Result<(), String> {
        let dir = app_support_dir().ok_or("no HOME folder")?;
        self.with(|s| {
            let path = s.src.save_path(&dir);
            styler::save_doc(&path, &s.doc)?;
            if let Src::Builtin(_) = s.src {
                s.src = Src::File(path);
            }
            s.theme_dirty = false;
            Ok(())
        })
    }

    fn save(&self) {
        let ui = self.ui();
        ui.form.panel.makeFirstResponder(None);
        if self.with(|s| s.theme_dirty) {
            if let Err(e) = self.write_theme() {
                self.app().alert("Cannot save the style theme", &e, &["OK"]);
                return;
            }
        }
        let (src, doc, go, changed) =
            self.with(|s| (s.src.clone(), s.doc.clone(), s.go, s.changed));
        if changed {
            let theme = styler::theme_attr(styler::dark(), &src);
            let r = styler::update_config(&[
                ("DarkMode", &[theme]),
                ("globalOverride", &styler::override_attrs(&go)),
            ]);
            if let Err(e) = r {
                self.app()
                    .alert("Cannot save the theme choice to config.xml", &e, &["OK"]);
            }
            let c = styler::build(&doc, &go);
            styler::set_current(src, doc, go, c);
            self.app().restyle(styler::cfg());
        }
        self.close();
    }

    fn cancel(&self) {
        if self.with(|s| s.changed) {
            self.app().restyle(styler::cfg());
        }
        self.close();
    }

    fn close(&self) {
        let ui = self.ui();
        ui.fg.deactivate();
        ui.bg.deactivate();
        if NSColorPanel::sharedColorPanelExists(self.mtm()) {
            NSColorPanel::sharedColorPanel(self.mtm()).orderOut(None);
        }
        ui.form.panel.orderOut(None);
    }

    fn refresh_themes(&self) {
        if !self.ui().form.panel.isVisible() {
            return;
        }
        let themes = styler::theme_list(&styler::user_themes(app_support_dir().as_deref()));
        self.with(|s| {
            s.theme = themes.iter().position(|(_, t)| *t == s.src).unwrap_or(0);
            s.themes = themes;
        });
        self.fill_themes();
    }
}

impl App {
    // Applies `c` to the open editors: Notepad++ WM_UPDATESCINTILLAS.
    pub(crate) fn restyle(&self, c: &Config) {
        let tabs = self.ivars().tabs.borrow().clone();
        let ws = c
            .global_styles
            .iter()
            .find(|s| s.name == "White space symbol")
            .and_then(|s| s.fg);
        for t in &tabs {
            let l = match &t.lang {
                Some(n) => c.languages.iter().find(|l| &l.name == n),
                None => lang::language_for_path(c, t.path.as_deref().unwrap_or(Path::new(&t.name))),
            };
            sci::apply_language(&t.view, c, l);
            self.apply_view(&t.view, l.map_or("normal", |l| l.name.as_str()));
            sci::setup_bookmark_margin(&t.view, c);
            sci::setup_change_history(&t.view, c);
            if let Some(ws) = ws {
                sci::send(&t.view, SCI_SETWHITESPACEFORE, 1, ws);
            }
        }
    }

    // IDM_LANGSTYLE_CONFIG_DLG.
    pub(crate) fn open_style_configurator(&self) {
        let d = DLG.with(|d| d.get_or_init(|| StyleDlg::create(self.mtm(), self)).clone());
        if !d.ui().form.panel.isVisible() {
            d.open();
        }
        d.ui().form.panel.makeKeyAndOrderFront(None);
    }

    // IDM_SETTING_IMPORTSTYLETHEMES: Notepad_plus::addNppComponents copies the files into the themes folder.
    pub(crate) fn import_style_themes(&self) {
        let Some(dir) = app_support_dir().map(|d| d.join("themes")) else {
            return;
        };
        let p = NSOpenPanel::openPanel(self.mtm());
        p.setAllowsMultipleSelection(true);
        p.setMessage(Some(&ns("Notepad++ style theme (*.xml)")));
        if p.runModal() != NSModalResponseOK {
            return;
        }
        let mut errors = vec![];
        for url in p.URLs().iter() {
            let Some(src) = url.path().map(|s| s.to_string()) else {
                continue;
            };
            let src = Path::new(&src);
            if !src
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("xml"))
            {
                continue;
            }
            let r = std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::copy(src, dir.join(src.file_name().unwrap_or_default())));
            if let Err(e) = r {
                errors.push(format!("{}: {e}", src.display()));
            }
        }
        if !errors.is_empty() {
            self.alert(
                "Cannot import the style themes",
                &errors.join("\n"),
                &["OK"],
            );
        }
        DLG.with(|d| {
            if let Some(d) = d.get() {
                d.refresh_themes();
            }
        });
    }

    // Parameters.cpp load: Notepad++ shows this message when stylers.xml or the theme does not load.
    pub(crate) fn style_load_alert(&self) {
        if let Some(e) = styler::with_current(|c| c.error.take()) {
            self.alert(
                "Load stylers.xml failed",
                &format!("Load \"{e}\" failed!"),
                &["OK"],
            );
        }
    }
}
