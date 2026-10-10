// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{ns, prefs};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBezelStyle, NSBitmapFormat, NSBitmapImageRep, NSBox, NSBoxType, NSButton,
    NSButtonType, NSColor, NSColorSpace, NSDeviceRGBColorSpace, NSImage, NSMenuItem,
    NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode, NSToolbarItem, NSWindow,
    NSWindowToolbarStyle,
};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSKeyValueObservingOptions, NSObjectNSKeyValueObserverRegistration,
    NSObjectProtocol, NSSize, NSString,
};
use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::{c_void, CString};

include!(concat!(env!("OUT_DIR"), "/toolbar_icons.rs"));

// A toolbar button: menuCmdID.h name, the action and tag of its menu item (-1 is any tag), which match of them, Fluent icon name, standard bitmap name.
pub struct Button {
    pub id: &'static str,
    pub action: &'static str,
    pub tag: isize,
    pub nth: usize,
    pub icon: &'static str,
    pub bmp: &'static str,
}

const fn b(id: &'static str, action: &'static str, tag: isize, icon: &'static str, bmp: &'static str) -> Button {
    Button {
        id,
        action,
        tag,
        nth: 0,
        icon,
        bmp,
    }
}

const SEP: Button = b("", "", 0, "", "");
const WRAP: isize = crate::view::WRAP as isize;
const ALL: isize = crate::view::ALL as isize;
const GUIDES: isize = crate::view::GUIDES as isize;

// Notepad_plus.cpp toolBarIcons; a button shows only when its menu item is in this app.
pub const BUTTONS: &[Button] = &[
    b("IDM_FILE_NEW", "newDocument:", -1, "new", "newFile"),
    b("IDM_FILE_OPEN", "openDocument:", -1, "open", "openFile"),
    b("IDM_FILE_SAVE", "saveDocument:", -1, "save", "saveFile"),
    b("IDM_FILE_SAVEALL", "saveAll:", -1, "saveall", "saveAll"),
    b("IDM_FILE_CLOSE", "closeTab:", -1, "close", "closeFile"),
    b("IDM_FILE_CLOSEALL", "closeMultiple:", 0, "closeall", "closeAll"),
    b("IDM_FILE_PRINT", "filePrint:", -1, "print", "print"),
    SEP,
    b("IDM_EDIT_CUT", "cut:", -1, "cut", "cut"),
    b("IDM_EDIT_COPY", "copy:", -1, "copy", "copy"),
    b("IDM_EDIT_PASTE", "paste:", -1, "paste", "paste"),
    SEP,
    b("IDM_EDIT_UNDO", "undo:", -1, "undo", "undo"),
    b("IDM_EDIT_REDO", "redo:", -1, "redo", "redo"),
    SEP,
    b("IDM_SEARCH_FIND", "showFind:", -1, "find", "find"),
    b("IDM_SEARCH_REPLACE", "showReplace:", -1, "findrep", "findReplace"),
    SEP,
    b("IDM_VIEW_ZOOMIN", "zoom:", 1, "zoomIn", "zoomIn"),
    Button {
        nth: 1,
        ..b("IDM_VIEW_ZOOMOUT", "zoom:", 1, "zoomOut", "zoomOut")
    },
    SEP,
    b("IDM_VIEW_SYNSCROLLV", "syncScroll:", 0, "syncV", "syncV"),
    b("IDM_VIEW_SYNSCROLLH", "syncScroll:", 1, "syncH", "syncH"),
    SEP,
    b("IDM_VIEW_WRAP", "viewOption:", WRAP, "wrap", "wrap"),
    b("IDM_VIEW_ALL_CHARACTERS", "viewOption:", ALL, "allChars", "allChars"),
    b("IDM_VIEW_INDENT_GUIDE", "viewOption:", GUIDES, "indentGuide", "indentGuide"),
    SEP,
    b("IDM_LANG_USER_DLG", "defineUdl:", -1, "udl", "udl"),
    b("IDM_VIEW_DOC_MAP", "toggleDocMap:", -1, "docMap", "docMap"),
    b("IDM_VIEW_DOCLIST", "toggleDocList:", -1, "docList", "docList"),
    b("IDM_VIEW_FUNC_LIST", "toggleFunctionList:", -1, "funcList", "funcList"),
    b("IDM_VIEW_FILEBROWSER", "toggleFolderAsWorkspace:", -1, "fileBrowser", "fileBrowser"),
    SEP,
    b("IDM_VIEW_MONITORING", "monitoring:", -1, "monitoring", "monitoring"),
    SEP,
    b("IDM_MACRO_STARTRECORDINGMACRO", "macroToggleRecord:", -1, "startrecord", "startRecord"),
    Button {
        nth: 1,
        ..b("IDM_MACRO_STOPRECORDINGMACRO", "macroToggleRecord:", -1, "stoprecord", "stopRecord")
    },
    b("IDM_MACRO_PLAYBACKRECORDEDMACRO", "macroPlayback:", -1, "playrecord", "playRecord"),
    b("IDM_MACRO_RUNMULTIMACRODLG", "macroShowMulti:", -1, "playrecord_m", "playRecord_m"),
    b("IDM_MACRO_SAVECURRENTMACRO", "macroSave:", -1, "saverecord", "saveRecord"),
];

type Rgb = [u8; 3];

// NppConstants.h g_cDefaultMainLight, g_cDefaultSecondaryLight, g_cDefaultMainDark, g_cDefaultSecondaryDark.
const MAIN_LIGHT: Rgb = [0x21, 0x21, 0x21];
const SECOND_LIGHT: Rgb = [0x00, 0x78, 0xD4];
const MAIN_DARK: Rgb = [0xDE, 0xDE, 0xDE];
const SECOND_DARK: Rgb = [0x4C, 0xC2, 0xFF];
// FluentColor red to yellow in IconList::changeFluentIconColor.
const COLORS: [Rgb; 7] = [
    [0xE8, 0x11, 0x23],
    [0x00, 0x8B, 0x00],
    [0x00, 0x78, 0xD4],
    [0xB1, 0x46, 0xC2],
    [0x00, 0xB7, 0xC3],
    [0x49, 0x82, 0x05],
    [0xFF, 0xB9, 0x00],
];
pub const ACCENT: i64 = 8;
pub const CUSTOM: i64 = 9;

// The icon set of the ToolBar text; dark mode has no standard icons (ToolBar::reset).
pub fn icon_set(text: &str, dark: bool) -> &'static str {
    match text.trim() {
        "small" => "small",
        "large" => "large",
        "small2" => "small2",
        "large2" => "large2",
        _ if dark => "small",
        _ => "standard",
    }
}

// The file of a button icon in PowerEditor/src/icons, as Notepad_plus.rc names it.
pub fn icon_path(b: &Button, set: &str, dark: bool) -> String {
    if set == "standard" {
        return format!("standard/toolbar/{}.bmp", b.bmp);
    }
    let mode = if dark { "dark" } else { "light" };
    let kind = if set.ends_with('2') {
        "filled"
    } else {
        "regular"
    };
    format!("{mode}/toolbar/{kind}/{}_off.ico", b.icon)
}

pub fn colorref(c: i64) -> Rgb {
    [c as u8, (c >> 8) as u8, (c >> 16) as u8]
}

// Port of IconList::changeFluentIconColor: the color to change (None changes all colors) and the new color.
pub fn fluent_map(
    color: i64,
    custom: i64,
    mono: bool,
    dark: bool,
    accent: Rgb,
) -> Option<(Option<Rgb>, Rgb)> {
    let old = (!mono).then_some(if dark { SECOND_DARK } else { SECOND_LIGHT });
    let new = match color {
        1..=7 => COLORS[color as usize - 1],
        ACCENT => accent,
        CUSTOM if custom != 0 => colorref(custom),
        _ if mono => {
            if dark {
                MAIN_DARK
            } else {
                MAIN_LIGHT
            }
        }
        _ => return None,
    };
    Some((old, new))
}

// Changes the visible pixels of an RGBA buffer that match `old` within 3 per channel.
pub fn recolor(px: &mut [u8], old: Option<Rgb>, new: Rgb) {
    for p in px.chunks_exact_mut(4) {
        if p[3] != 0 && old.is_none_or(|o| (0..3).all(|i| p[i].abs_diff(o[i]) <= 3)) {
            p[..3].copy_from_slice(&new);
        }
    }
}

fn le(b: &[u8], at: usize, n: usize) -> Option<u32> {
    let s = b.get(at..at + n)?;
    Some(s.iter().rev().fold(0, |v, x| v << 8 | *x as u32))
}

#[cfg(test)]
fn png_width(png: &[u8]) -> u32 {
    le(png, 16, 4).map_or(0, u32::swap_bytes)
}

// The PNG images of an .ico file.
pub fn ico_images(b: &[u8]) -> Vec<&[u8]> {
    let n = le(b, 4, 2).unwrap_or(0) as usize;
    (0..n)
        .filter_map(|i| {
            let e = 6 + 16 * i;
            let (size, at) = (le(b, e + 8, 4)? as usize, le(b, e + 12, 4)? as usize);
            b.get(at..at + size)
        })
        .collect()
}

// An uncompressed .bmp as top-down RGBA; LR_LOADTRANSPARENT and ImageList_AddMasked make the first pixel color and the 3D face gray transparent.
pub fn bmp_rgba(b: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if b.get(..2)? != b"BM" || le(b, 30, 4)? != 0 {
        return None;
    }
    let (off, head) = (le(b, 10, 4)? as usize, le(b, 14, 4)? as usize);
    let (w, h) = (le(b, 18, 4)? as i32, le(b, 22, 4)? as i32);
    let bpp = le(b, 28, 2)? as usize;
    if w <= 0 || h == 0 || ![4, 8, 24, 32].contains(&bpp) {
        return None;
    }
    let (w, rows) = (w as usize, h.unsigned_abs() as usize);
    let stride = (w * bpp).div_ceil(32) * 4;
    let pal = 14 + head;
    let color = |row: usize, x: usize| -> Option<Rgb> {
        let at = match bpp {
            4 => {
                pal + 4 * ((*b.get(row + x / 2)? >> if x.is_multiple_of(2) { 4 } else { 0 }) & 0xF) as usize
            }
            8 => pal + 4 * *b.get(row + x)? as usize,
            _ => row + x * bpp / 8,
        };
        Some([*b.get(at + 2)?, *b.get(at + 1)?, *b.get(at)?])
    };
    let key = color(off, 0)?;
    let mut out = Vec::with_capacity(w * rows * 4);
    for y in 0..rows {
        let row = off + stride * if h < 0 { y } else { rows - 1 - y };
        for x in 0..w {
            let c = color(row, x)?;
            let clear = c == key || c == [0xC0, 0xC0, 0xC0];
            out.extend_from_slice(&c);
            out.push(if clear { 0 } else { 255 });
        }
    }
    Some((w, rows, out))
}

#[derive(Clone, PartialEq)]
struct Look {
    set: &'static str,
    dark: bool,
    map: Option<(Option<Rgb>, Rgb)>,
}

fn accent() -> Rgb {
    let c = NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace());
    c.map_or(SECOND_LIGHT, |c| {
        let v = |x: f64| (x.clamp(0., 1.) * 255.).round() as u8;
        [
            v(c.redComponent()),
            v(c.greenComponent()),
            v(c.blueComponent()),
        ]
    })
}

fn look() -> Look {
    let dark = crate::styler::dark();
    let (text, color, custom, mono) = prefs::with(|p| {
        (
            p.toolbar_icons.clone(),
            p.toolbar_color,
            p.toolbar_custom_color,
            p.toolbar_mono,
        )
    });
    let set = icon_set(&text, dark);
    let acc = if color == ACCENT { accent() } else { [0; 3] };
    let map = if set == "standard" {
        None
    } else {
        fluent_map(color, custom, mono, dark, acc)
    };
    Look { set, dark, map }
}

fn recolor_rep(r: &NSBitmapImageRep, old: Option<Rgb>, new: Rgb) {
    let (w, h, row) = (
        r.pixelsWide() as usize,
        r.pixelsHigh() as usize,
        r.bytesPerRow() as usize,
    );
    let f = r.bitmapFormat();
    let rgba = r.bitsPerSample() == 8 && r.samplesPerPixel() == 4 && !r.isPlanar();
    if !rgba
        || !f.contains(NSBitmapFormat::AlphaNonpremultiplied)
        || f.contains(NSBitmapFormat::AlphaFirst)
        || row < w * 4
    {
        return;
    }
    let p = r.bitmapData();
    if p.is_null() {
        return;
    }
    for y in 0..h {
        recolor(
            unsafe { std::slice::from_raw_parts_mut(p.add(y * row), w * 4) },
            old,
            new,
        );
    }
}

fn image(mtm: MainThreadMarker, b: &Button, l: &Look) -> Option<Retained<NSImage>> {
    let path = icon_path(b, l.set, l.dark);
    let bytes = TOOLBAR_ICONS.iter().find(|(p, _)| *p == path)?.1;
    let pt = if l.set.starts_with("large") { 32. } else { 16. };
    let img = NSImage::initWithSize(mtm.alloc(), NSSize::new(pt, pt));
    if l.set == "standard" {
        let (w, h, px) = bmp_rgba(bytes)?;
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
                mtm.alloc(), std::ptr::null_mut(), w as isize, h as isize, 8, 4, true, false,
                NSDeviceRGBColorSpace, NSBitmapFormat::AlphaNonpremultiplied, (w * 4) as isize, 32,
            )
        }?;
        let p = rep.bitmapData();
        if p.is_null() {
            return None;
        }
        unsafe { std::ptr::copy_nonoverlapping(px.as_ptr(), p, px.len().min(w * h * 4)) };
        rep.setSize(NSSize::new(pt, pt));
        img.addRepresentation(&rep);
        return Some(img);
    }
    for png in ico_images(bytes) {
        let Some(rep) = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(png)) else {
            continue;
        };
        if let Some((old, new)) = l.map {
            recolor_rep(&rep, old, new);
        }
        rep.setSize(NSSize::new(pt, pt));
        img.addRepresentation(&rep);
    }
    Some(img)
}

struct Btn {
    b: &'static Button,
    button: Retained<NSButton>,
    menu: RefCell<Retained<NSMenuItem>>,
}

struct State {
    toolbar: Retained<NSToolbar>,
    _target: Retained<Target>,
    ids: Vec<Retained<NSString>>,
    items: Vec<Retained<NSToolbarItem>>,
    btns: Vec<Btn>,
}

thread_local! {
    static S: OnceCell<State> = const { OnceCell::new() };
    static LOOK: RefCell<Option<Look>> = const { RefCell::new(None) };
    static APPLYING: Cell<bool> = const { Cell::new(false) };
    static HIDDEN: Cell<bool> = const { Cell::new(false) };
}

define_class!(
    // A toolbar button item that takes its enabled and on state from its menu item.
    #[unsafe(super(NSToolbarItem))]
    #[thread_kind = MainThreadOnly]
    #[name = "NppToolbarItem"]
    struct TbItem;

    impl TbItem {
        #[unsafe(method(validate))]
        fn validate(&self) {
            let tag: isize = self.view().map_or(-1, |v| unsafe { msg_send![&v, tag] });
            sync_look();
            if let Ok(i) = usize::try_from(tag) {
                refresh(i);
            }
        }
    }
);

define_class!(
    // The toolbar delegate and the target of the toolbar buttons.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NppToolbarTarget"]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    unsafe impl NSToolbarDelegate for Target {
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn item_for(&self, _t: &NSToolbar, id: &NSString, _insert: bool) -> Option<Retained<NSToolbarItem>> {
            S.with(|s| {
                let s = s.get()?;
                let i = s.ids.iter().position(|x| **x == *id)?;
                s.items.get(i).cloned()
            })
        }

        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn defaults(&self, _t: &NSToolbar) -> Retained<NSArray<NSString>> {
            ids()
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn allowed(&self, _t: &NSToolbar) -> Retained<NSArray<NSString>> {
            ids()
        }
    }

    impl Target {
        #[unsafe(method(tbClick:))]
        fn click(&self, s: &AnyObject) {
            let tag: isize = unsafe { msg_send![s, tag] };
            if let Ok(i) = usize::try_from(tag) {
                click(i);
            }
        }

        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe(&self, _k: Option<&NSString>, _o: Option<&AnyObject>, _c: Option<&NSDictionary>, _x: *mut c_void) {
            visibility_changed();
        }
    }
);

fn ids() -> Retained<NSArray<NSString>> {
    S.with(|s| {
        let v: Vec<&NSString> = s
            .get()
            .map_or(vec![], |s| s.ids.iter().map(|x| &**x).collect());
        NSArray::from_slice(&v)
    })
}

// The menu item by action and tag, not by title, as the menu titles can be translated.
fn lookup(mtm: MainThreadMarker, b: &Button) -> Option<Retained<NSMenuItem>> {
    let bar = NSApplication::sharedApplication(mtm).mainMenu()?;
    let action = Sel::register(&CString::new(b.action).ok()?);
    let (m, mut at) = crate::macros::find_item(&bar, action, b.tag)?;
    for _ in 0..b.nth {
        at = (at + 1..m.numberOfItems()).find(|&k| m.itemAtIndex(k).and_then(|it| it.action()) == Some(action))?;
    }
    m.itemAtIndex(at)
}

// The menu item of button `i`, found again when its menu was built again.
fn menu_item(
    mtm: MainThreadMarker,
    i: usize,
) -> Option<(Retained<NSMenuItem>, Retained<NSButton>)> {
    S.with(|s| {
        let b = s.get()?.btns.get(i)?;
        let cur = b.menu.borrow().clone();
        if unsafe { cur.menu() }.is_some() {
            return Some((cur, b.button.clone()));
        }
        let new = lookup(mtm, b.b)?;
        *b.menu.borrow_mut() = new.clone();
        Some((new, b.button.clone()))
    })
}

// The menu validation of AppKit: the item target, or the responder chain for a nil target.
fn validate_item(mtm: MainThreadMarker, mi: &NSMenuItem) -> bool {
    let Some(menu) = (unsafe { mi.menu() }) else { return false };
    if !menu.autoenablesItems() {
        return mi.isEnabled();
    }
    let Some(action) = mi.action() else {
        return false;
    };
    let target = mi.target().or_else(|| unsafe {
        NSApplication::sharedApplication(mtm).targetForAction_to_from(action, None, Some(mi))
    });
    let Some(t) = target else { return false };
    let ok = unsafe {
        let has = |s| -> bool { msg_send![&t, respondsToSelector: s] };
        if has(sel!(validateMenuItem:)) {
            msg_send![&t, validateMenuItem: mi]
        } else if has(sel!(validateUserInterfaceItem:)) {
            msg_send![&t, validateUserInterfaceItem: mi]
        } else {
            true
        }
    };
    mi.setEnabled(ok);
    ok
}

fn refresh(i: usize) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some((mi, button)) = menu_item(mtm, i) else {
        return;
    };
    button.setEnabled(validate_item(mtm, &mi));
    button.setState(mi.state());
}

fn click(i: usize) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some((mi, _)) = menu_item(mtm, i) else {
        return;
    };
    if validate_item(mtm, &mi) {
        if let Some(m) = unsafe { mi.menu() } {
            let at = m.indexOfItem(&mi);
            if at >= 0 {
                m.performActionForItemAtIndex(at);
            }
        }
    }
    refresh(i);
}

// Loads the icons again when the icon set, the colors or the appearance change.
fn sync_look() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let l = look();
    if LOOK.with(|x| x.borrow().as_ref() == Some(&l)) {
        return;
    }
    LOOK.with(|x| *x.borrow_mut() = Some(l.clone()));
    let btns: Vec<(&'static Button, Retained<NSButton>)> = S.with(|s| {
        s.get().map_or(vec![], |s| {
            s.btns.iter().map(|b| (b.b, b.button.clone())).collect()
        })
    });
    for (b, button) in btns {
        button.setImage(image(mtm, b, &l).as_deref());
    }
}

fn set_visible(tb: &NSToolbar, on: bool) {
    if tb.isVisible() != on {
        APPLYING.with(|a| a.set(true));
        tb.setVisible(on);
        APPLYING.with(|a| a.set(false));
    }
}

// Shows the settings of the Preferences Toolbar page.
pub(crate) fn apply() {
    sync_look();
    if let Some(tb) = S.with(|s| s.get().map(|s| s.toolbar.clone())) {
        set_visible(
            &tb,
            prefs::with(|p| p.toolbar_visible) && !HIDDEN.with(Cell::get),
        );
    }
}

// Hides the toolbar without a change to the setting, for Post-it and Distraction Free modes.
pub(crate) fn hide_without_saving(hide: bool) {
    HIDDEN.with(|h| h.set(hide));
    apply();
}

// View > Show Toolbar (toggleToolbarShown:) changes the ToolBar visible setting too.
fn visibility_changed() {
    if APPLYING.with(Cell::get) || HIDDEN.with(Cell::get) {
        return;
    }
    let Some(on) = S.with(|s| s.get().map(|s| s.toolbar.isVisible())) else {
        return;
    };
    if prefs::with(|p| p.toolbar_visible) == on {
        return;
    }
    prefs::update(|p| p.toolbar_visible = on);
    if let Err(e) = crate::session::save_config() {
        eprintln!("Cannot save the settings: {e}");
    }
}

fn button_item(
    mtm: MainThreadMarker,
    target: &Target,
    b: &'static Button,
    mi: &NSMenuItem,
    i: usize,
) -> Retained<NSToolbarItem> {
    let title = mi.title();
    let id = ns(&format!("npp.{}", b.id));
    let item: Retained<TbItem> =
        unsafe { msg_send![TbItem::alloc(mtm), initWithItemIdentifier: &*id] };
    let button = NSButton::new(mtm);
    button.setButtonType(NSButtonType::PushOnPushOff);
    button.setBezelStyle(NSBezelStyle::Toolbar);
    button.setTitle(&NSString::new());
    button.setTag(i as isize);
    unsafe {
        button.setTarget(Some(target));
        button.setAction(Some(sel!(tbClick:)));
    }
    button.setToolTip(Some(&title));
    let form = crate::item(mtm, &title.to_string(), sel!(tbClick:), "", Some(target));
    form.setTag(i as isize);
    item.setView(Some(&button));
    item.setLabel(&title);
    item.setPaletteLabel(&title);
    item.setToolTip(Some(&title));
    item.setMenuFormRepresentation(Some(&form));
    Retained::into_super(item)
}

fn separator_item(mtm: MainThreadMarker, k: usize) -> Retained<NSToolbarItem> {
    let item = NSToolbarItem::initWithItemIdentifier(
        NSToolbarItem::alloc(mtm),
        &ns(&format!("npp.sep.{k}")),
    );
    let line = NSBox::new(mtm);
    line.setBoxType(NSBoxType::Separator);
    line.widthAnchor()
        .constraintEqualToConstant(1.)
        .setActive(true);
    line.heightAnchor()
        .constraintEqualToConstant(20.)
        .setActive(true);
    item.setView(Some(&line));
    item
}

// Puts the Notepad++ toolbar on the window, below the title bar; call it after the menus are built.
pub(crate) fn attach(w: &NSWindow) {
    let mtm = w.mtm();
    let target: Retained<Target> = unsafe { msg_send![Target::alloc(mtm), init] };
    let (mut ids, mut items, mut btns) = (vec![], vec![], vec![]);
    for b in BUTTONS {
        let last_sep = ids
            .last()
            .is_none_or(|x: &Retained<NSString>| x.to_string().starts_with("npp.sep."));
        if b.id.is_empty() {
            if !last_sep {
                let it = separator_item(mtm, items.len());
                ids.push(it.itemIdentifier());
                items.push(it);
            }
            continue;
        }
        let Some(mi) = lookup(mtm, b) else { continue };
        let it = button_item(mtm, &target, b, &mi, btns.len());
        let Some(button) = it.view().and_then(|v| v.downcast::<NSButton>().ok()) else {
            continue;
        };
        ids.push(it.itemIdentifier());
        items.push(it);
        btns.push(Btn {
            b,
            button,
            menu: RefCell::new(mi),
        });
    }
    if ids
        .last()
        .is_some_and(|x| x.to_string().starts_with("npp.sep."))
    {
        ids.pop();
        items.pop();
    }
    let tb = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &ns("NotepadPlusToolbar"));
    tb.setDisplayMode(NSToolbarDisplayMode::IconOnly);
    tb.setAllowsUserCustomization(false);
    let _ = S.with(|s| {
        s.set(State {
            toolbar: tb.clone(),
            _target: target.clone(),
            ids,
            items,
            btns,
        })
    });
    tb.setDelegate(Some(ProtocolObject::from_ref(&*target)));
    w.setToolbarStyle(NSWindowToolbarStyle::Expanded);
    w.setToolbar(Some(&tb));
    apply();
    unsafe {
        tb.addObserver_forKeyPath_options_context(
            &target,
            &ns("visible"),
            NSKeyValueObservingOptions::New,
            std::ptr::null_mut(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CPP: &str = include_str!("../../PowerEditor/src/Notepad_plus.cpp");
    const RC: &str = include_str!("../../PowerEditor/src/Notepad_plus.rc");

    // The rows of toolBarIcons: each row is its fields; a separator row starts with 0.
    fn rows() -> Vec<Vec<String>> {
        let t = &CPP[CPP.find("toolBarIcons[]{").unwrap()..];
        let t = &t[..t.find("\n};").unwrap()];
        t.lines()
            .map(str::trim)
            .filter(|l| l.starts_with('{'))
            .map(|l| {
                l.trim_start_matches('{')
                    .split('}')
                    .next()
                    .unwrap()
                    .split(',')
                    .map(|f| f.trim().to_string())
                    .collect()
            })
            .collect()
    }

    fn rc_file(id: &str) -> String {
        let l = RC
            .lines()
            .find(|l| l.split_whitespace().next() == Some(id))
            .unwrap_or_else(|| panic!("{id}"));
        l.split('"').nth(1).unwrap().to_string()
    }

    #[test]
    fn buttons_match_notepad_plus_order() {
        let ids: Vec<String> = rows()
            .iter()
            .map(|r| {
                if r[0] == "0" {
                    String::new()
                } else {
                    r[0].clone()
                }
            })
            .collect();
        let ours: Vec<&str> = BUTTONS.iter().map(|b| b.id).collect();
        assert_eq!(ids, ours);
    }

    #[test]
    fn icon_files_match_the_resources() {
        for (b, r) in BUTTONS.iter().zip(rows()).filter(|(b, _)| !b.id.is_empty()) {
            let want = [
                (1, "small", false),
                (3, "small2", false),
                (5, "large", true),
                (7, "large2", true),
                (9, "standard", false),
            ];
            for (col, set, dark) in want {
                assert_eq!(
                    format!("icons/{}", icon_path(b, set, dark)),
                    rc_file(&r[col]),
                    "{}",
                    b.id
                );
            }
        }
    }

    #[test]
    fn every_icon_is_embedded_and_loads() {
        for b in BUTTONS.iter().filter(|b| !b.id.is_empty()) {
            for (set, dark) in [
                ("small", false),
                ("large2", false),
                ("small", true),
                ("small2", true),
                ("standard", false),
            ] {
                let p = icon_path(b, set, dark);
                let bytes = TOOLBAR_ICONS
                    .iter()
                    .find(|(x, _)| *x == p)
                    .unwrap_or_else(|| panic!("{p}"))
                    .1;
                if set == "standard" {
                    let (w, h, px) = bmp_rgba(bytes).unwrap_or_else(|| panic!("{p}"));
                    assert_eq!((w, h, px.len()), (16, 16, 1024), "{p}");
                    assert!(
                        px.chunks(4).any(|c| c[3] == 0) && px.chunks(4).any(|c| c[3] == 255),
                        "{p}"
                    );
                } else {
                    let imgs = ico_images(bytes);
                    assert_eq!(imgs.iter().map(|i| crate::toolbar::png_width(i)).collect::<Vec<_>>(), [16, 32, 64], "{p}");
                    assert!(imgs.iter().all(|i| i.starts_with(b"\x89PNG")), "{p}");
                }
            }
        }
    }

    #[test]
    fn every_action_is_in_the_menus() {
        let menus = [
            include_str!("fileops.rs"),
            include_str!("edit.rs"),
            include_str!("search_extras.rs"),
            include_str!("view.rs"),
            include_str!("views.rs"),
            include_str!("udl/mod.rs"),
            include_str!("macros.rs"),
        ]
        .concat();
        for b in BUTTONS.iter().filter(|b| !b.id.is_empty()) {
            assert!(menus.contains(&format!("sel!({})", b.action)), "{}", b.id);
        }
        assert!(menus.contains("keyed(mtm, \"Zoom In\", sel!(zoom:), 1,"));
        assert!(menus.contains("keyed(mtm, \"Zoom Out\", sel!(zoom:), -1,"));
    }

    #[test]
    fn icon_sets() {
        assert_eq!(icon_set("standard", false), "standard");
        assert_eq!(icon_set("standard", true), "small");
        assert_eq!(icon_set("bogus", false), "standard");
        assert_eq!(icon_set("large2", true), "large2");
        let b = &BUTTONS[0];
        assert_eq!(
            icon_path(b, "large", true),
            "dark/toolbar/regular/new_off.ico"
        );
    }

    #[test]
    fn defaults_match_parameters_h() {
        let p = include_str!("../../PowerEditor/src/Parameters.h");
        assert!(p.contains("TbIconInfo _tbIconInfo{ toolBarStatusType::TB_STANDARD, FluentColor::defaultColor, 0, false };"));
        let d = prefs::Prefs::default();
        assert_eq!(
            (
                d.toolbar_icons.as_str(),
                d.toolbar_color,
                d.toolbar_custom_color,
                d.toolbar_mono,
                d.toolbar_visible
            ),
            ("standard", 0, 0, false, true)
        );
    }

    #[test]
    fn colors_match_image_list_set() {
        let consts = include_str!("../../PowerEditor/src/MISC/Common/NppConstants.h");
        let e = &consts[consts.find("enum class FluentColor").unwrap()..];
        let names: Vec<&str> = e[e.find('{').unwrap() + 1..e.find('}').unwrap()]
            .split(',')
            .map(str::trim)
            .collect();
        assert_eq!(
            names.iter().position(|n| *n == "accent"),
            Some(ACCENT as usize)
        );
        assert_eq!(
            names.iter().position(|n| *n == "custom"),
            Some(CUSTOM as usize)
        );
        let src = include_str!("../../PowerEditor/src/WinControls/ImageListSet/ImageListSet.cpp");
        for (k, n) in ["red", "green", "blue", "purple", "cyan", "olive", "yellow"]
            .iter()
            .enumerate()
        {
            assert_eq!(names[k + 1], *n);
            let t = &src[src.find(&format!("case FluentColor::{n}:")).unwrap()..];
            let t = &t[t.find("RGB(").unwrap() + 4..];
            let rgb: Vec<u8> = t[..t.find(')').unwrap()]
                .split(',')
                .map(|x| u8::from_str_radix(x.trim().trim_start_matches("0x"), 16).unwrap())
                .collect();
            assert_eq!(rgb, COLORS[k], "{n}");
        }
    }

    #[test]
    fn fluent_colors() {
        let acc = [1, 2, 3];
        assert_eq!(fluent_map(0, 0, false, false, acc), None);
        assert_eq!(fluent_map(0, 0, true, false, acc), Some((None, MAIN_LIGHT)));
        assert_eq!(fluent_map(0, 0, true, true, acc), Some((None, MAIN_DARK)));
        assert_eq!(
            fluent_map(1, 0, false, false, acc),
            Some((Some(SECOND_LIGHT), COLORS[0]))
        );
        assert_eq!(
            fluent_map(ACCENT, 0, false, true, acc),
            Some((Some(SECOND_DARK), acc))
        );
        assert_eq!(
            fluent_map(CUSTOM, 0x123456, true, false, acc),
            Some((None, [0x56, 0x34, 0x12]))
        );
        assert_eq!(fluent_map(CUSTOM, 0, false, false, acc), None);
        let mut px = vec![
            0x21, 0x21, 0x21, 255, 0x01, 0x79, 0xD2, 255, 0x00, 0x78, 0xD4, 0,
        ];
        recolor(&mut px, Some(SECOND_LIGHT), [9, 9, 9]);
        assert_eq!(
            px,
            [0x21, 0x21, 0x21, 255, 9, 9, 9, 255, 0x00, 0x78, 0xD4, 0]
        );
        recolor(&mut px, None, [7, 7, 7]);
        assert_eq!(px, [7, 7, 7, 255, 7, 7, 7, 255, 0x00, 0x78, 0xD4, 0]);
    }
}
