// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{config, macros, ns};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, sel, ClassType, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSApplication, NSButton, NSMenu, NSMenuDidAddItemNotification, NSMenuItem, NSPopUpButton,
    NSTableView, NSTextField, NSUserInterfaceLayoutDirection, NSView, NSWindow,
    NSWindowDidBecomeKeyNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;

include!(concat!(env!("OUT_DIR"), "/native_langs.rs"));
const DEFS: &str = include_str!("../../PowerEditor/src/localizationString.h");
const ENGLISH: &str = "english.xml";

#[derive(Default, Debug)]
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    kids: Vec<El>,
}

impl El {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    fn path(&self, p: &[&str]) -> Option<&El> {
        p.iter()
            .try_fold(self, |e, n| e.kids.iter().find(|k| k.name == *n))
    }

    // (key attribute, name attribute) of the Item children.
    fn items<'a>(&'a self, key: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> {
        self.kids
            .iter()
            .filter(|k| k.name == "Item")
            .filter_map(move |k| Some((k.attr(key)?, k.attr("name")?)))
    }

    fn item<'a>(&'a self, key: &'a str, v: &str) -> Option<&'a str> {
        self.items(key).find(|(k, _)| *k == v).map(|(_, n)| n)
    }

    // Item descendants with an id and a name, as (path of element names, id, name).
    fn deep_items(&self, at: &str, out: &mut Vec<(String, String, String)>) {
        for k in &self.kids {
            if let (Some(id), Some(n)) = (k.attr("id"), k.attr("name")) {
                out.push((at.to_string(), id.to_string(), n.to_string()));
            }
            k.deep_items(&format!("{at}/{}", k.name), out);
        }
    }
}

fn element(e: &BytesStart) -> El {
    El {
        name: e.name().as_ref().to_string(),
        attrs: e
            .attributes()
            .flatten()
            .map(|a| {
                let raw = a.value.to_string();
                let v = quick_xml::escape::unescape(&raw).map_or(raw.clone(), |v| v.into_owned());
                (a.key.as_ref().to_string(), v)
            })
            .collect(),
        kids: vec![],
    }
}

// The <Native-Langue> element; attribute values keep their line breaks, as pugixml loads them for Notepad++.
fn parse(xml: &str) -> Option<El> {
    let mut r = Reader::from_str(xml);
    let mut stack = vec![El::default()];
    loop {
        match r.read_event().ok()? {
            Event::Start(e) => stack.push(element(&e)),
            Event::Empty(e) => stack.last_mut()?.kids.push(element(&e)),
            Event::End(_) => {
                let e = stack.pop()?;
                stack.last_mut()?.kids.push(e);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut root = stack.pop()?;
    let at = root.kids.iter().position(|k| k.name == "NotepadPlus")?;
    let mut np = root.kids.swap_remove(at);
    let at = np.kids.iter().position(|k| k.name == "Native-Langue")?;
    Some(np.kids.swap_remove(at))
}

// purifyMenuString of localization.cpp without the ellipsis step, and the text after a TAB removed.
fn clean(s: &str) -> String {
    let mut s = s.split('\t').next().unwrap_or("").to_string();
    if let Some(p) = s.find("(&") {
        if s[p..].chars().nth(3) == Some(')') {
            let end = p + s[p..].char_indices().nth(4).map_or(s.len() - p, |(i, _)| i);
            s.replace_range(p..end, "");
        }
    }
    let mut out = String::new();
    let mut amp = false;
    for c in s.chars() {
        if c == '&' && !amp {
            amp = true;
            continue;
        }
        amp = false;
        out.push(c);
    }
    out.trim().to_string()
}

// The key that matches an English text to a nativeLang name.
fn norm(s: &str) -> String {
    let c = clean(s);
    let c = c.trim_end_matches("...").trim_end_matches('…');
    c.trim().to_lowercase()
}

fn menu<'a>(l: &'a El, section: &str) -> Option<&'a El> {
    l.path(&["Menu", "Main", section])
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Top,
    Sub,
    Cmd,
    Tab,
}

// changeMenuLang: the native name of a menu bar entry, a submenu, or a command by id or by its English name.
fn menu_text(en: &El, tr: &El, orig: &str, kind: Kind, id: Option<i32>) -> Option<String> {
    let key = norm(orig);
    let by_name = |sec: &El, attr: &str| {
        sec.items(attr)
            .find(|(_, n)| norm(n) == key)
            .map(|(k, _)| k.to_string())
    };
    let named = |path: &[&str], attr: &str| {
        let k = by_name(en.path(path)?, attr)?;
        tr.path(path)?.item(attr, &k).map(clean)
    };
    let cmd = |id: &str| menu(tr, "Commands")?.item("id", id).map(clean);
    let found = match kind {
        Kind::Top => named(&["Menu", "Main", "Entries"], "menuId"),
        Kind::Sub => named(&["Menu", "Main", "SubEntries"], "subMenuId"),
        Kind::Cmd => id
            .and_then(|i| cmd(&i.to_string()))
            .or_else(|| cmd(&by_name(menu(en, "Commands")?, "id")?)),
        Kind::Tab => {
            named(&["Menu", "TabBar"], "CMDID").or_else(|| menu_text(en, tr, orig, Kind::Cmd, id))
        }
    };
    found.filter(|s| !s.is_empty())
}

fn placeholder(s: &str) -> Option<usize> {
    let rest = s.strip_prefix('$')?;
    let end = rest.find('$')?;
    rest[..end]
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        .then_some(end + 2)
        .filter(|_| end > 0)
}

// A template as literal text and $..._REPLACE$ names, in order.
fn split(t: &str) -> Vec<(bool, &str)> {
    let mut out = vec![];
    let (mut i, mut lit) = (0, 0);
    while i < t.len() {
        if let Some(n) = placeholder(&t[i..]) {
            out.push((false, &t[lit..i]));
            out.push((true, &t[i..i + n]));
            i += n;
            lit = i;
        } else {
            i += t[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push((false, &t[lit..]));
    out
}

// The values of the placeholders when `s` is the English template `t` filled in.
fn capture<'a>(t: &str, s: &'a str) -> Option<Vec<(String, &'a str)>> {
    let parts = split(t);
    if !parts
        .iter()
        .any(|(ph, x)| !ph && x.chars().any(char::is_alphabetic))
    {
        return None;
    }
    let mut out = vec![];
    let mut pos = 0;
    let mut open: Option<&str> = None;
    for (k, (ph, x)) in parts.iter().enumerate() {
        if *ph {
            open = Some(x);
            continue;
        }
        let last = k + 1 == parts.len();
        let at = match (open, last) {
            (None, _) if s[pos..].starts_with(x) => pos,
            (None, _) => return None,
            (Some(_), true) if x.is_empty() => s.len(),
            (Some(_), true) => s
                .len()
                .checked_sub(x.len())
                .filter(|&a| a >= pos && s.is_char_boundary(a) && s[a..] == **x)?,
            (Some(_), false) => pos + s[pos..].find(x)?,
        };
        if let Some(n) = open.take() {
            out.push((n.to_string(), &s[pos..at]));
        }
        pos = at + x.len();
    }
    (pos == s.len()).then_some(out)
}

// NativeLangSpeaker::messageBox: the native title or message for an English one, placeholders filled in.
fn message_text(en: &El, tr: &El, s: &str) -> Option<String> {
    for m in &en.path(&["MessageBox"])?.kids {
        for a in ["title", "message"] {
            let Some(caps) = m.attr(a).and_then(|t| capture(t, s)) else {
                continue;
            };
            let Some(mut t) = tr
                .path(&["MessageBox", &m.name])
                .and_then(|x| x.attr(a))
                .filter(|t| !t.is_empty())
                .map(String::from)
            else {
                continue;
            };
            for (n, v) in &caps {
                t = t.replacen(n.as_str(), v, 1);
            }
            for (n, v) in caps.iter().rev() {
                t = t.replace(n.as_str(), v);
            }
            return Some(t);
        }
    }
    None
}

// changeDlgLang: the dialog whose English title matches, as (tag, title attribute).
fn dialog<'a>(en: &'a El, title: &str) -> Option<(&'a El, &'a str)> {
    let key = norm(title);
    if key.is_empty() {
        return None;
    }
    en.path(&["Dialog"])?.kids.iter().find_map(|d| {
        d.attrs
            .iter()
            .find(|(k, v)| k.starts_with("title") && norm(v) == key)
            .map(|(k, _)| (d, k.as_str()))
    })
}

fn dialog_text(tr: &El, d: &El, orig: &str) -> Option<String> {
    let mut a = vec![];
    d.deep_items("", &mut a);
    let key = norm(orig);
    let (path, id, _) = a.into_iter().find(|(_, _, n)| norm(n) == key)?;
    let t = tr.path(&["Dialog", &d.name])?;
    let mut b = vec![];
    t.deep_items("", &mut b);
    b.into_iter()
        .find(|(p, i, _)| *p == path && *i == id)
        .map(|(_, _, n)| clean(&n))
        .filter(|s| !s.is_empty())
}

// LocalizationSwitcher: (native name, file) of each nativeLang file, in file name order.
fn languages() -> Vec<(String, &'static str)> {
    let defs: Vec<(&str, &str)> = DEFS
        .lines()
        .filter_map(|l| {
            let mut p = l.split("L\"").skip(1).filter_map(|s| s.split('"').next());
            Some((p.next()?, p.next()?))
        })
        .collect();
    let mut v: Vec<_> = NATIVE_LANGS
        .iter()
        .filter_map(|(f, _)| {
            let (n, _) = defs.iter().find(|(_, d)| d.eq_ignore_ascii_case(f))?;
            Some((n.to_string(), *f))
        })
        .collect();
    v.sort_by_key(|(_, f)| f.to_lowercase());
    v
}

fn lang_name(file: &str) -> Option<String> {
    languages()
        .into_iter()
        .find(|(_, f)| f.eq_ignore_ascii_case(file))
        .map(|(n, _)| n)
}

fn native_lang_path() -> Option<PathBuf> {
    Some(config::app_support_dir()?.join("nativeLang.xml"))
}

struct State {
    en: El,
    tr: Option<El>,
    rtl: bool,
    ids: Vec<(&'static str, isize, i32)>,
    // ponytail: entries for released items stay, a few per menu rebuild; prune by liveness if it grows.
    shown: HashMap<usize, (String, String)>,
}

thread_local! {
    static S: RefCell<Option<State>> = const { RefCell::new(None) };
    static OBSERVER: OnceCell<Retained<Observer>> = const { OnceCell::new() };
}

// NativeLangSpeaker::init: no translation for english.xml at launch; `load_english` is the live switch.
fn load(xml: Option<&str>, load_english: bool) -> State {
    let tr = xml.and_then(parse).filter(|l| {
        load_english
            || !l
                .attr("filename")
                .is_some_and(|f| f.eq_ignore_ascii_case(ENGLISH))
    });
    let english = NATIVE_LANGS.iter().find(|(f, _)| *f == ENGLISH);
    State {
        rtl: tr.as_ref().and_then(|l| l.attr("RTL")) == Some("yes"),
        en: english.and_then(|(_, x)| parse(x)).unwrap_or_default(),
        tr,
        ids: macros::menu_ids(),
        shown: HashMap::new(),
    }
}

impl State {
    // The text to show for an object: the translation of its original text, or the original.
    fn retitle(
        &mut self,
        p: usize,
        cur: &str,
        f: impl FnOnce(&El, &El, &str) -> Option<String>,
    ) -> Option<String> {
        let orig = match self.shown.get(&p) {
            Some((o, s)) if s == cur => o.clone(),
            _ => cur.to_string(),
        };
        let new = self
            .tr
            .as_ref()
            .and_then(|tr| f(&self.en, tr, &orig))
            .unwrap_or_else(|| orig.clone());
        if new == orig {
            self.shown.remove(&p);
        } else {
            self.shown.insert(p, (orig, new.clone()));
        }
        (new != cur).then_some(new)
    }

    fn id(&self, it: &NSMenuItem) -> Option<i32> {
        let a = it.action()?;
        let a = a.name().to_str().ok()?;
        self.ids
            .iter()
            .find(|(x, t, _)| *x == a && (*t == -1 || *t == it.tag()))
            .map(|(_, _, i)| *i)
    }

    fn item(&mut self, it: &NSMenuItem, top: bool, kind: Kind) {
        if it.isSeparatorItem() {
            return;
        }
        let sub = it.submenu();
        let id = self.id(it);
        let k = match (top, &sub) {
            (true, _) => Kind::Top,
            (_, Some(_)) if kind != Kind::Tab => Kind::Sub,
            _ => kind,
        };
        let p = it as *const NSMenuItem as usize;
        if let Some(t) = self.retitle(p, &it.title().to_string(), |en, tr, o| {
            menu_text(en, tr, o, k, id)
        }) {
            it.setTitle(&ns(&t));
            if let Some(s) = &sub {
                s.setTitle(&ns(&t));
            }
        }
        if let Some(s) = sub {
            self.menu(&s, false, kind);
        }
    }

    fn menu(&mut self, m: &NSMenu, top: bool, kind: Kind) {
        m.setUserInterfaceLayoutDirection(if self.rtl {
            NSUserInterfaceLayoutDirection::RightToLeft
        } else {
            NSUserInterfaceLayoutDirection::LeftToRight
        });
        for it in m.itemArray().iter() {
            self.item(&it, top, kind);
        }
    }

    fn window(&mut self, w: &NSWindow) {
        let p = w as *const NSWindow as usize;
        let cur = w.title().to_string();
        let orig = match self.shown.get(&p) {
            Some((o, s)) if *s == cur => o.clone(),
            _ => cur.clone(),
        };
        let Some((tag, a)) = dialog(&self.en, &orig).map(|(d, a)| (d.name.clone(), a.to_string()))
        else {
            return;
        };
        if let Some(t) = self.retitle(p, &cur, |_, tr, _| {
            tr.path(&["Dialog", &tag])?
                .attr(&a)
                .map(clean)
                .filter(|s| !s.is_empty())
        }) {
            w.setTitle(&ns(&t));
        }
        if let Some(v) = w.contentView() {
            self.view(&v, &tag);
        }
    }

    fn view(&mut self, v: &NSView, tag: &str) {
        if v.isKindOfClass(NSTableView::class()) {
            return;
        }
        let text = |en: &El, tr: &El, o: &str| {
            let d = en.path(&["Dialog", tag])?;
            dialog_text(tr, d, o)
        };
        let p = v as *const NSView as usize;
        if v.isKindOfClass(NSPopUpButton::class()) {
        } else if let Some(b) = v.downcast_ref::<NSButton>() {
            if let Some(t) = self.retitle(p, &b.title().to_string(), text) {
                b.setTitle(&ns(&t));
            }
        } else if let Some(f) = v.downcast_ref::<NSTextField>() {
            if !f.isEditable() {
                if let Some(t) = self.retitle(p, &f.stringValue().to_string(), text) {
                    f.setStringValue(&ns(&t));
                }
            }
        }
        for s in v.subviews().iter() {
            self.view(&s, tag);
        }
    }
}

fn with(f: impl FnOnce(&mut State)) {
    S.with(|s| {
        if let Ok(mut s) = s.try_borrow_mut() {
            if let Some(s) = s.as_mut() {
                f(s);
            }
        }
    });
}

fn translating() -> bool {
    S.with(|s| {
        s.try_borrow()
            .is_ok_and(|s| s.as_ref().is_some_and(|s| s.tr.is_some()))
    })
}

// Root menu of `m` and whether it is the editor or tab context menu.
fn root(m: &NSMenu) -> (Retained<NSMenu>, bool) {
    let mut r = m.retain();
    while let Some(s) = unsafe { r.supermenu() } {
        r = s;
    }
    let ctx = r.delegate().is_some_and(|d| {
        let o: &AnyObject = (*d).as_ref();
        o.class().name().to_str() == Ok("NppContextMenus")
    });
    (r, ctx)
}

fn apply_all(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    with(|s| {
        if let Some(bar) = app.mainMenu() {
            s.menu(&bar, true, Kind::Cmd);
        }
        for w in app.windows().iter() {
            s.window(&w);
        }
    });
}

define_class!(
    // Translates menu items as the app adds them, dialogs as they become key, and the Localization popup.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NppL10n"]
    struct Observer;

    unsafe impl NSObjectProtocol for Observer {}

    impl Observer {
        #[unsafe(method(menuDidAddItem:))]
        fn menu_did_add_item(&self, n: &NSNotification) {
            if !translating() {
                return;
            }
            let Some(m) = n.object().and_then(|o| o.downcast::<NSMenu>().ok()) else {
                return;
            };
            let (r, ctx) = root(&m);
            let bar = NSApplication::sharedApplication(self.mtm()).mainMenu();
            let main = bar.as_ref().is_some_and(|b| std::ptr::eq(&**b, &*r));
            if !main && !ctx {
                return;
            }
            let top = main && std::ptr::eq(&*m, &*r);
            let kind = if ctx { Kind::Tab } else { Kind::Cmd };
            let at = n
                .userInfo()
                .and_then(|u| u.objectForKey(&ns("NSMenuItemIndex")))
                .and_then(|o| o.downcast::<NSNumber>().ok())
                .map(|x| x.integerValue());
            with(|s| match at.and_then(|i| m.itemAtIndex(i)) {
                Some(it) => s.item(&it, top, kind),
                None => s.menu(&m, top, kind),
            });
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, n: &NSNotification) {
            if !translating() {
                return;
            }
            if let Some(w) = n.object().and_then(|o| o.downcast::<NSWindow>().ok()) {
                with(|s| s.window(&w));
            }
        }

        #[unsafe(method(localizationChanged:))]
        fn localization_changed(&self, p: &NSPopUpButton) {
            let Some((_, file)) = usize::try_from(p.indexOfSelectedItem())
                .ok()
                .and_then(|i| languages().into_iter().nth(i))
            else {
                return;
            };
            if let Err(e) = switch_to(file) {
                if let Some(d) = NSApplication::sharedApplication(self.mtm()).delegate() {
                    let app: Retained<crate::App> = unsafe { Retained::cast_unchecked(d) };
                    app.alert("Cannot change the localization", &e, &["OK"]);
                }
                return;
            }
            apply_all(self.mtm());
        }
    }
);

// LocalizationSwitcher::switchToLang: the chosen file becomes nativeLang.xml in the settings folder.
fn switch_to(file: &str) -> Result<(), String> {
    let xml = NATIVE_LANGS
        .iter()
        .find(|(f, _)| *f == file)
        .map(|(_, x)| *x)
        .ok_or_else(|| format!("{file} is missing"))?;
    let path = native_lang_path().ok_or("HOME is not set")?;
    crate::session::read_file(&path)?;
    crate::session::write_file(&path, xml, false)?;
    let st = load(Some(xml), true);
    S.with(|s| {
        if let Ok(mut s) = s.try_borrow_mut() {
            let shown = s.take().map(|o| o.shown).unwrap_or_default();
            *s = Some(State { shown, ..st });
        }
    });
    Ok(())
}

// The text of a menu item before translation.
pub fn english_title(i: &NSMenuItem) -> String {
    let cur = i.title().to_string();
    S.with(|s| {
        let s = s.try_borrow().ok()?;
        let (o, shown) = s.as_ref()?.shown.get(&(i as *const NSMenuItem as usize))?;
        (*shown == cur).then(|| o.clone())
    })
    .unwrap_or(cur)
}

// The menu bar menu with the English title `title`, also when the bar shows a translation.
pub fn bar_menu(bar: &NSMenu, title: &str) -> Option<Retained<NSMenu>> {
    bar.itemArray()
        .iter()
        .find(|i| english_title(i) == title)?
        .submenu()
}

// Loads nativeLang.xml, translates the menu bar, and follows later menu items and dialogs.
pub fn install(mtm: MainThreadMarker) {
    let xml = native_lang_path().and_then(|p| std::fs::read_to_string(p).ok());
    let st = load(xml.as_deref(), false);
    S.with(|s| *s.borrow_mut() = Some(st));
    let o: Retained<Observer> = unsafe { msg_send![Observer::alloc(mtm), init] };
    let c = NSNotificationCenter::defaultCenter();
    unsafe {
        c.addObserver_selector_name_object(
            &o,
            sel!(menuDidAddItem:),
            Some(NSMenuDidAddItemNotification),
            None,
        );
        c.addObserver_selector_name_object(
            &o,
            sel!(windowDidBecomeKey:),
            Some(NSWindowDidBecomeKeyNotification),
            None,
        );
    }
    OBSERVER.with(|x| {
        let _ = x.set(o);
    });
    if translating() {
        apply_all(mtm);
    }
}

// The Localization list of Preferences > General, with the language of nativeLang.xml selected.
pub fn popup(mtm: MainThreadMarker) -> Retained<NSPopUpButton> {
    let p = NSPopUpButton::new(mtm);
    let list = languages();
    for (n, _) in &list {
        p.addItemWithTitle(&ns(n));
    }
    let cur = native_lang_path()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map_or(Some("English".to_string()), |x| {
            parse(&x).and_then(|l| lang_name(l.attr("filename")?))
        });
    p.selectItemAtIndex(
        cur.and_then(|c| list.iter().position(|(n, _)| *n == c))
            .map_or(-1, |i| i as isize),
    );
    OBSERVER.with(|o| {
        if let Some(o) = o.get() {
            let t: &AnyObject = o;
            unsafe {
                p.setTarget(Some(t));
                p.setAction(Some(sel!(localizationChanged:)));
            }
        }
    });
    p
}

// NativeLangSpeaker::getAttrNameStr and getProjectPanelLangMenuStr: the name at `path` (of the Item `id` when given), or `default`.
pub fn native_name(path: &[&str], id: Option<&str>, default: &str) -> String {
    S.with(|st| {
        let st = st.try_borrow().ok()?;
        let e = st.as_ref()?.tr.as_ref()?.path(path)?;
        let n = match id {
            Some(i) => e.item("id", i)?,
            None => e.attr("name")?,
        };
        (!n.is_empty()).then(|| n.to_string())
    })
    .unwrap_or_else(|| default.to_string())
}

// NativeLangSpeaker::messageBox: the native text of an English MessageBox title or message.
pub fn message(s: &str) -> String {
    S.with(|st| {
        let st = st.try_borrow().ok()?;
        let st = st.as_ref()?;
        message_text(&st.en, st.tr.as_ref()?, s)
    })
    .unwrap_or_else(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(f: &str) -> El {
        let x = NATIVE_LANGS.iter().find(|(n, _)| *n == f).unwrap().1;
        parse(x).unwrap()
    }

    #[test]
    fn project_manager_names() {
        let title = |k| crate::project::panel_title(k);
        assert_eq!(title(0), "Project Panel 1");
        let x = NATIVE_LANGS.iter().find(|(n, _)| *n == "french.xml").unwrap().1;
        S.with(|s| *s.borrow_mut() = Some(load(Some(x), true)));
        assert_eq!(title(2), "Projet 3");
        let file = ["ProjectManager", "Menus", "FileMenu"];
        assert_eq!(native_name(&file, Some("3111"), "Rename"), "Renommer");
        assert_eq!(native_name(&file, Some("9999"), "Other"), "Other");
        let root = ["ProjectManager", "WorkspaceRootName"];
        assert_eq!(native_name(&root, None, "Workspace"), "Espace de travail");
        S.with(|s| *s.borrow_mut() = None);
    }

    #[test]
    fn parses_english_and_french() {
        let en = lang("english.xml");
        assert_eq!(en.attr("filename"), Some("english.xml"));
        assert_eq!(
            menu(&en, "Entries").unwrap().item("menuId", "file"),
            Some("&File")
        );
        assert_eq!(
            menu(&en, "Commands").unwrap().item("id", "41001"),
            Some("&New")
        );
        let fr = lang("french.xml");
        assert_eq!(fr.attr("name"), Some("Français"));
        assert_eq!(
            menu(&fr, "Entries").unwrap().item("menuId", "file"),
            Some("&Fichier")
        );
        assert_eq!(
            menu(&fr, "Commands").unwrap().item("id", "42001"),
            Some("Cou&per")
        );
        assert_eq!(lang("arabic.xml").attr("RTL"), Some("yes"));
        let m = en
            .path(&["MessageBox", "DoCloseOrNot"])
            .unwrap()
            .attr("message")
            .unwrap();
        assert!(m.contains("anymore.\n"));
    }

    #[test]
    fn accelerators_are_removed() {
        assert_eq!(clean("&Nouveau"), "Nouveau");
        assert_eq!(clean("Cou&per\tCtrl+X"), "Couper");
        assert_eq!(clean("文件(&F)"), "文件");
        assert_eq!(clean("新建(&N)..."), "新建...");
        assert_eq!(clean("Copy && Paste"), "Copy & Paste");
        assert_eq!(norm("&Find..."), "find");
        assert_eq!(norm("Find…"), "find");
    }

    #[test]
    fn menu_names_by_id_and_by_english_text() {
        let (en, fr) = (lang("english.xml"), lang("french.xml"));
        let t = |o: &str, k: Kind, id: Option<i32>| menu_text(&en, &fr, o, k, id);
        assert_eq!(t("File", Kind::Top, None).as_deref(), Some("Fichier"));
        assert_eq!(t("?", Kind::Top, None), None);
        assert_eq!(
            t("Line Operations", Kind::Sub, None).as_deref(),
            Some("Ligne")
        );
        assert_eq!(t("New", Kind::Cmd, Some(41001)).as_deref(), Some("Nouveau"));
        assert_eq!(
            t("Something else", Kind::Cmd, Some(42001)).as_deref(),
            Some("Couper")
        );
        assert_eq!(t("Cut", Kind::Cmd, None).as_deref(), Some("Couper"));
        assert_eq!(t("no such item", Kind::Cmd, None), None);
        assert_eq!(
            t("Close Multiple Tabs", Kind::Tab, None).as_deref(),
            fr.path(&["Menu", "TabBar"]).unwrap().item("CMDID", "0")
        );
        assert_eq!(t("Cut", Kind::Tab, Some(42001)).as_deref(), Some("Couper"));
        let zh = lang("chineseSimplified.xml");
        assert_eq!(
            menu_text(&en, &zh, "File", Kind::Top, None).as_deref(),
            Some("文件")
        );
    }

    #[test]
    fn every_port_command_in_english_xml_has_a_name() {
        let (en, fr) = (lang("english.xml"), lang("french.xml"));
        let cmds = menu(&en, "Commands").unwrap();
        let mut n = 0;
        for (_, _, id) in macros::menu_ids() {
            if cmds.item("id", &id.to_string()).is_none() {
                continue;
            }
            n += 1;
            for l in [&en, &fr] {
                assert!(menu_text(&en, l, "", Kind::Cmd, Some(id)).is_some(), "{id}");
            }
        }
        assert!(n > 150, "{n}");
    }

    #[test]
    fn message_box_placeholders() {
        let (en, fr) = (lang("english.xml"), lang("french.xml"));
        assert_eq!(
            capture(
                "Cannot open file \"$STR_REPLACE$\".",
                "Cannot open file \"/a b\"."
            )
            .unwrap(),
            [("$STR_REPLACE$".to_string(), "/a b")]
        );
        assert_eq!(capture("\"$STR_REPLACE$\"", "\"x\""), None);
        assert_eq!(
            capture(
                "Cannot open file \"$STR_REPLACE$\".",
                "Cannot open file \"/a/b€"
            ),
            None
        );
        assert_eq!(capture("x $STR_REPLACE$ €.", "x a€."), None);
        assert_eq!(
            capture("a$STR_REPLACE$é", "aé€é").unwrap(),
            [("$STR_REPLACE$".to_string(), "é€")]
        );
        assert_eq!(capture("Sort Failed", "Sort Failed!"), None);
        let fr_open = fr
            .path(&["MessageBox", "OpenFileError"])
            .unwrap()
            .attr("message")
            .unwrap();
        assert_eq!(
            message_text(&en, &fr, "Cannot open file \"/tmp/x\"."),
            Some(fr_open.replace("$STR_REPLACE$", "/tmp/x"))
        );
        assert_eq!(message_text(&en, &fr, "Not a Notepad++ message"), None);
    }

    #[test]
    fn repeated_placeholders_fill_in_order() {
        let doc = |m: &str| {
            parse(&format!("<NotepadPlus><Native-Langue><MessageBox><X title=\"T\" message=\"{m}\"/></MessageBox></Native-Langue></NotepadPlus>")).unwrap()
        };
        let en = doc("Copy $STR_REPLACE$ to $STR_REPLACE$ now");
        let tr = doc("Copier $STR_REPLACE$ vers $STR_REPLACE$");
        assert_eq!(
            message_text(&en, &tr, "Copy a to b now").as_deref(),
            Some("Copier a vers b")
        );
        let tr = doc("$STR_REPLACE$ / $STR_REPLACE$ / $STR_REPLACE$");
        assert_eq!(
            message_text(&en, &tr, "Copy a to b now").as_deref(),
            Some("a / b / b")
        );
    }

    #[test]
    fn find_dialog_texts() {
        let (en, fr) = (lang("english.xml"), lang("french.xml"));
        let (d, a) = dialog(&en, "Find").unwrap();
        assert_eq!((d.name.as_str(), a), ("Find", "titleFind"));
        assert_eq!(
            dialog_text(&fr, d, "Match case").as_deref(),
            Some("Respecter la casse")
        );
        assert_eq!(dialog(&en, "Mark").unwrap().1, "titleMark");
        assert!(dialog(&en, "").is_none());
    }

    #[test]
    fn localization_list() {
        let v = languages();
        assert_eq!(v.len(), NATIVE_LANGS.len());
        assert_eq!(v.len(), 94);
        assert!(v.iter().any(|(n, f)| n == "Français" && *f == "french.xml"));
        assert_eq!(lang_name("ENGLISH.xml").as_deref(), Some("English"));
        assert_eq!(lang_name("azerbaijan.xml"), None);
        assert!(v
            .windows(2)
            .all(|w| w[0].1.to_lowercase() <= w[1].1.to_lowercase()));
    }

    #[test]
    fn english_is_not_applied_at_launch() {
        let en = NATIVE_LANGS.iter().find(|(f, _)| *f == ENGLISH).unwrap().1;
        assert!(load(Some(en), false).tr.is_none());
        assert!(load(Some(en), true).tr.is_some());
        assert!(load(None, false).tr.is_none());
        let ar = NATIVE_LANGS
            .iter()
            .find(|(f, _)| *f == "arabic.xml")
            .unwrap()
            .1;
        assert!(load(Some(ar), false).rtl);
    }
}
