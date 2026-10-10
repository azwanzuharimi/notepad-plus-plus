// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{item, macros, nested, ns, sci, tagged, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject, Sel};
use objc2::{
    define_class, msg_send, sel, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSEventType, NSMenu, NSMenuDelegate, NSMenuItem, NSSegmentedControl, NSView,
};
use objc2_foundation::NSObjectProtocol;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::cell::OnceCell;
use std::path::{Path, PathBuf};

const SCI_GETSELECTIONS: u32 = 2570;
const SCI_GETSELECTIONNSTART: u32 = 2585;
const SCI_GETSELECTIONNEND: u32 = 2587;
const SCI_POSITIONFROMPOINT: u32 = 2022;
const SCI_CHARPOSITIONFROMPOINT: u32 = 2561;
const SCI_SETEMPTYSELECTION: u32 = 2556;
const SCI_AUTOCCANCEL: u32 = 2101;
const SCI_CALLTIPCANCEL: u32 = 2201;
const SCI_USEPOPUP: u32 = 2371;

const CONSTANTS: &str = include_str!("../../PowerEditor/src/MISC/Common/NppConstants.h");

// The C string CONTEXTMENU_XML_CONTENT of NppConstants.h: the contextMenu.xml that Notepad++ writes when the file is missing.
pub fn default_xml() -> String {
    let body = CONSTANTS
        .split("CONTEXTMENU_XML_CONTENT[] = \"\\")
        .nth(1)
        .and_then(|s| s.split("\n\";").next())
        .unwrap_or("");
    body.lines()
        .map(|l| l.trim_end_matches('\r').strip_suffix('\\').unwrap_or(l))
        .collect::<String>()
        .replace("\\r", "\r")
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Sep,
    Id(i32),
    Path(String, String),
}

// One Item of contextMenu.xml: the command, ItemNameAs and FolderName.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub cmd: Cmd,
    pub label: String,
    pub folder: String,
}

fn attr(e: &BytesStart, key: &str) -> String {
    let Some(a) = e.try_get_attribute(key).ok().flatten() else {
        return String::new();
    };
    let raw = a.value.into_owned();
    quick_xml::escape::unescape(&raw).map_or(raw.clone(), |v| v.into_owned())
}

// Port of NppParameters::getContextMenuFromXmlTree; plugin items are not loaded, because the app has no plugins.
pub fn parse(xml: &str, section: &str) -> Result<Vec<Entry>, String> {
    let mut r = Reader::from_str(xml);
    let mut path: Vec<String> = vec![];
    let mut out = vec![];
    loop {
        let (e, empty) = match r.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(_) => {
                path.pop();
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let name = e.name().as_ref().to_string();
        if name == "Item" && path == ["NotepadPlus", section] {
            let folder = attr(&e, "FolderName");
            let label = attr(&e, "ItemNameAs");
            let cmd = match attr(&e, "id").trim().parse::<i32>() {
                Ok(0) => Some(Cmd::Sep),
                Ok(id) if id > 0 => Some(Cmd::Id(id)),
                _ => {
                    let (entry, item) = (attr(&e, "MenuEntryName"), attr(&e, "MenuItemName"));
                    (!entry.is_empty() && !item.is_empty()).then_some(Cmd::Path(entry, item))
                }
            };
            if let Some(cmd) = cmd {
                out.push(Entry { cmd, label, folder });
            }
        }
        if !empty {
            path.push(name);
        }
    }
    Ok(out)
}

// A menu as a tree of titles.
#[derive(Clone, Debug)]
pub enum Node {
    Item(String),
    Menu(String, Vec<Node>),
}

// Port of purgeMenuItemString: no '&', and nothing from the tab.
pub fn purge(s: &str) -> String {
    s.split('\t').next().unwrap_or("").replace('&', "")
}

fn same(a: &str, b: &str) -> bool {
    purge(a).to_lowercase() == purge(b).to_lowercase()
}

fn find_leaf(nodes: &[Node], name: &str, at: &mut Vec<usize>) -> bool {
    for (i, n) in nodes.iter().enumerate() {
        at.push(i);
        let found = match n {
            Node::Item(t) => same(t, name),
            Node::Menu(_, sub) => find_leaf(sub, name, at),
        };
        if found {
            return true;
        }
        at.pop();
    }
    false
}

// Port of NppParameters::getCmdIdFromMenuEntryItemName: the index path of the first item called `name` under the top menu `entry`.
pub fn find_path(top: &[Node], entry: &str, name: &str) -> Option<Vec<usize>> {
    top.iter().enumerate().find_map(|(i, n)| match n {
        Node::Menu(t, sub) if same(t, entry) => {
            let mut at = vec![i];
            find_leaf(sub, name, &mut at).then_some(at)
        }
        _ => None,
    })
}

#[derive(Debug, PartialEq)]
pub enum Out<T> {
    Sep,
    Cmd(T),
    Folder(String, Vec<Out<T>>),
}

// Port of ContextMenu::create: next items with the same folder share one submenu; no separator at the ends or two in a row.
pub fn layout<T>(items: Vec<(String, Option<T>)>) -> Vec<Out<T>> {
    let n = items.len();
    let mut out: Vec<Out<T>> = vec![];
    let mut cur: Option<String> = None;
    let mut last_sep = false;
    for (i, (folder, cmd)) in items.into_iter().enumerate() {
        if folder.is_empty() {
            cur = None;
        } else if cur.as_deref() != Some(folder.as_str()) {
            out.push(Out::Folder(folder.clone(), vec![]));
            cur = Some(folder);
        }
        let entry = cmd.map_or(Out::Sep, Out::Cmd);
        if cur.is_some() {
            if let Some(Out::Folder(_, sub)) = out.last_mut() {
                sub.push(entry);
            }
            last_sep = false;
        } else if matches!(entry, Out::Sep) {
            if i != 0 && i + 1 != n && !last_sep {
                out.push(Out::Sep);
            }
            last_sep = true;
        } else {
            out.push(entry);
            last_sep = false;
        }
    }
    while matches!(out.last(), Some(Out::Sep)) {
        out.pop();
    }
    out
}

fn tree(m: &NSMenu) -> Vec<Node> {
    m.itemArray()
        .iter()
        .map(|i| match i.submenu() {
            Some(s) => Node::Menu(i.title().to_string(), tree(&s)),
            None => Node::Item(i.title().to_string()),
        })
        .collect()
}

fn item_at(m: &NSMenu, path: &[usize]) -> Option<Retained<NSMenuItem>> {
    let (first, rest) = path.split_first()?;
    let it = m.itemAtIndex(*first as isize)?;
    if rest.is_empty() {
        Some(it)
    } else {
        let sub = it.submenu()?;
        item_at(&sub, rest)
    }
}

// The main menu item of a contextMenu.xml command; None when the app does not have it.
fn resolve(bar: &NSMenu, top: &[Node], cmd: &Cmd) -> Option<Retained<NSMenuItem>> {
    match cmd {
        Cmd::Sep => None,
        Cmd::Id(id) => {
            let (action, tag) = macros::menu_action(*id)?;
            let action = Sel::register(&std::ffi::CString::new(action).ok()?);
            let (m, i) = macros::find_item(bar, action, tag)?;
            m.itemAtIndex(i)
        }
        Cmd::Path(entry, name) => item_at(bar, &find_path(top, entry, name)?),
    }
}

fn copy_item(mtm: MainThreadMarker, src: &NSMenuItem, label: &str) -> Retained<NSMenuItem> {
    let title = if label.is_empty() {
        purge(&src.title().to_string())
    } else {
        label.to_string()
    };
    let it = NSMenuItem::new(mtm);
    it.setTitle(&ns(&title));
    unsafe {
        it.setAction(src.action());
        it.setTarget(src.target().as_deref());
    }
    it.setTag(src.tag());
    it
}

fn fill(mtm: MainThreadMarker, m: &NSMenu, items: Vec<Out<Retained<NSMenuItem>>>) {
    for o in items {
        match o {
            Out::Sep => m.addItem(&NSMenuItem::separatorItem(mtm)),
            Out::Cmd(i) => m.addItem(&i),
            Out::Folder(name, sub) => {
                let top = nested(mtm, &name, vec![]);
                if let Some(s) = top.submenu() {
                    fill(mtm, &s, sub);
                }
                m.addItem(&top);
            }
        }
    }
}

// The default tab context menu of NppNotification.cpp NM_RCLICK, without the items this app does not have.
fn tab_items() -> Vec<(&'static str, &'static str, Option<(Sel, isize)>)> {
    let close = |tag| Some((sel!(closeMultiple:), tag));
    let multi = "Close Multiple Tabs";
    let open = "Open into";
    let clip = "Copy to Clipboard";
    let doc = "Move Document";
    vec![
        ("Close", "", Some((sel!(closeTab:), -1))),
        ("Close All BUT This", multi, close(1)),
        ("Close All to the Left", multi, close(2)),
        ("Close All to the Right", multi, close(3)),
        ("Close All Unchanged", multi, close(4)),
        ("Save", "", Some((sel!(saveDocument:), -1))),
        ("Save As...", "", Some((sel!(saveDocumentAs:), -1))),
        (
            "Open Containing Folder in Finder",
            open,
            Some((sel!(openFolderFinder:), -1)),
        ),
        (
            "Open Containing Folder in Terminal",
            open,
            Some((sel!(openFolderTerminal:), -1)),
        ),
        ("", open, None),
        (
            "Open in Default Viewer",
            open,
            Some((sel!(openDefaultViewer:), -1)),
        ),
        ("Rename", "", Some((sel!(renameFile:), -1))),
        ("Move to Trash", "", Some((sel!(moveToTrash:), -1))),
        ("Reload", "", Some((sel!(reloadFromDisk:), -1))),
        ("", "", None),
        (
            "Read-Only in Notepad++",
            "",
            Some((sel!(toggleReadOnly:), -1)),
        ),
        ("", "", None),
        ("Copy Full File Path", clip, Some((sel!(copyPathInfo:), 0))),
        ("Copy Filename", clip, Some((sel!(copyPathInfo:), 1))),
        (
            "Copy Current Dir. Path",
            clip,
            Some((sel!(copyPathInfo:), 2)),
        ),
        ("Move to Start", doc, Some((sel!(moveTab:), 0))),
        ("Move to End", doc, Some((sel!(moveTab:), 1))),
    ]
}

fn user_file() -> Option<PathBuf> {
    Some(crate::config::app_support_dir()?.join("contextMenu.xml"))
}

// The user contextMenu.xml when it loads, else the Notepad++ default.
fn load() -> Vec<Entry> {
    let user = user_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| parse(&t, "ScintillaContextMenu").ok());
    user.unwrap_or_else(|| parse(&default_xml(), "ScintillaContextMenu").unwrap_or_default())
}

// Writes a new file through a temporary file; refuses a path that exists.
fn create(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("xml.tmp");
    std::fs::write(&tmp, text)?;
    let r = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    r
}

struct State {
    entries: Vec<Entry>,
    edit: Retained<NSMenu>,
    tabs: Retained<NSMenu>,
    _delegate: Retained<Menus>,
}

thread_local! {
    static S: OnceCell<State> = const { OnceCell::new() };
}

define_class!(
    // Fills the editor and tab context menus just before they open.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NppContextMenus"]
    struct Menus;

    unsafe impl NSObjectProtocol for Menus {}

    unsafe impl NSMenuDelegate for Menus {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, m: &NSMenu) {
            let Some(d) = NSApplication::sharedApplication(self.mtm()).delegate() else {
                return;
            };
            let app: Retained<App> = unsafe { Retained::cast_unchecked(d) };
            let tabs = S.with(|s| s.get().is_some_and(|s| std::ptr::eq(&*s.tabs, m)));
            if tabs {
                app.fill_tab_menu(m);
            } else {
                app.fill_edit_menu(m);
            }
        }
    }
);

// Adds Settings > Edit Popup ContextMenu, makes the Settings menu when it is not there, and loads contextMenu.xml.
pub fn install(mtm: MainThreadMarker, bar: &NSMenu, t: Option<&AnyObject>) {
    let find = |title: &str| {
        bar.itemArray()
            .iter()
            .position(|i| i.title().to_string() == title)
    };
    let settings = match find("Settings").and_then(|i| bar.itemAtIndex(i as isize)?.submenu()) {
        Some(m) => m,
        None => {
            let top = nested(mtm, "Settings", vec![]);
            let at = find("Language").map_or(bar.numberOfItems(), |i| i as isize + 1);
            bar.insertItem_atIndex(&top, at);
            top.submenu().unwrap_or_else(|| NSMenu::new(mtm))
        }
    };
    if settings.numberOfItems() > 0 {
        settings.addItem(&NSMenuItem::separatorItem(mtm));
    }
    settings.addItem(&item(
        mtm,
        "Edit Popup ContextMenu",
        sel!(editContextMenu:),
        "",
        t,
    ));
    let delegate: Retained<Menus> = unsafe { msg_send![Menus::alloc(mtm), init] };
    let edit = NSMenu::new(mtm);
    let tabs = NSMenu::new(mtm);
    edit.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    tabs.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let state = State {
        entries: load(),
        edit,
        tabs,
        _delegate: delegate,
    };
    S.with(|s| {
        let _ = s.set(state);
    });
}

impl App {
    // Gives an editor the contextMenu.xml menu in place of the Scintilla menu, and the tab bar its menu.
    pub(crate) fn context_menu_setup(&self, v: &NSView) {
        sci::send(v, SCI_USEPOPUP, 0, 0);
        S.with(|s| {
            let Some(s) = s.get() else { return };
            unsafe { v.setMenu(Some(&s.edit)) };
        });
        self.tab_bar_menu();
    }

    // The tab bar of NSTabView is a segmented control subview, so it gets the menu too.
    pub(crate) fn tab_bar_menu(&self) {
        let tv = self.tab_view();
        S.with(|s| {
            let Some(s) = s.get() else { return };
            unsafe { tv.setMenu(Some(&s.tabs)) };
            for v in tv.subviews().iter() {
                if v.isKindOfClass(NSSegmentedControl::class()) {
                    unsafe { v.setMenu(Some(&s.tabs)) };
                }
            }
        });
    }

    // Scintilla on Windows moves the caret to a right click outside the selection before the menu shows.
    fn caret_to_click(&self, v: &NSView) {
        let mtm = self.mtm();
        let Some(e) = NSApplication::sharedApplication(mtm).currentEvent() else {
            return;
        };
        if !matches!(
            e.r#type(),
            NSEventType::RightMouseDown | NSEventType::LeftMouseDown
        ) {
            return;
        }
        let content = sci::content(v);
        if let Some(w) = self.ivars().window.get() {
            w.makeFirstResponder(Some(&content));
        }
        let p = content.convertPoint_fromView(e.locationInWindow(), None);
        let vis = content.visibleRect();
        let (x, y) = (
            (p.x - vis.origin.x) as isize as usize,
            (p.y - vis.origin.y) as isize,
        );
        let c = sci::send(v, SCI_CHARPOSITIONFROMPOINT, x, y);
        let n = sci::send(v, SCI_GETSELECTIONS, 0, 0).max(0) as usize;
        let inside = (0..n).any(|i| {
            let s = sci::send(v, SCI_GETSELECTIONNSTART, i, 0);
            let e = sci::send(v, SCI_GETSELECTIONNEND, i, 0);
            s < e && s <= c && c < e
        });
        if !inside {
            sci::send(v, SCI_AUTOCCANCEL, 0, 0);
            sci::send(v, SCI_CALLTIPCANCEL, 0, 0);
            let pos = sci::send(v, SCI_POSITIONFROMPOINT, x, y);
            sci::send(v, SCI_SETEMPTYSELECTION, pos.max(0) as usize, 0);
        }
    }

    fn fill_edit_menu(&self, m: &NSMenu) {
        let mtm = self.mtm();
        m.removeAllItems();
        if let Some(v) = self.editor() {
            self.caret_to_click(&v);
        }
        let Some(bar) = NSApplication::sharedApplication(mtm).mainMenu() else {
            return;
        };
        let top = tree(&bar);
        let items = S.with(|s| {
            s.get().map_or(vec![], |s| {
                s.entries
                    .iter()
                    .filter_map(|e| match &e.cmd {
                        Cmd::Sep => Some((e.folder.clone(), None)),
                        c => resolve(&bar, &top, c)
                            .map(|i| (e.folder.clone(), Some(copy_item(mtm, &i, &e.label)))),
                    })
                    .collect()
            })
        });
        fill(mtm, m, layout(items));
    }

    // Notepad++ selects the tab under a right click, then the commands work on the current tab.
    fn fill_tab_menu(&self, m: &NSMenu) {
        let mtm = self.mtm();
        m.removeAllItems();
        let tv = self.tab_view();
        if let Some(e) = NSApplication::sharedApplication(mtm).currentEvent() {
            let p = tv.convertPoint_fromView(e.locationInWindow(), None);
            let Some(hit) = tv.tabViewItemAtPoint(p) else {
                return;
            };
            tv.selectTabViewItem(Some(&hit));
        }
        let t: &AnyObject = self;
        let items = tab_items()
            .into_iter()
            .map(|(title, folder, cmd)| {
                (
                    folder.to_string(),
                    cmd.map(|(a, tag)| tagged(mtm, title, a, tag, Some(t))),
                )
            })
            .collect();
        fill(mtm, m, layout(items));
    }

    // NppCommands.cpp IDM_SETTING_EDITCONTEXTMENU; the app writes the default file first when it is missing.
    pub(crate) fn edit_context_menu(&self) {
        self.alert(
            "Editing contextMenu",
            "Editing contextMenu.xml allows you to modify your Notepad++ popup context menu on edit zone.\nYou have to restart your Notepad++ to take effect after modifying contextMenu.xml.",
            &["OK"],
        );
        let Some(p) = user_file() else { return };
        if let Err(e) = std::fs::symlink_metadata(&p) {
            let made = if e.kind() == std::io::ErrorKind::NotFound {
                create(&p, &default_xml())
            } else {
                Err(e)
            };
            if let Err(e) = made {
                self.alert(
                    &format!("Cannot create {}", p.display()),
                    &e.to_string(),
                    &["OK"],
                );
                return;
            }
        }
        self.open_path(&p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_parses() {
        let xml = default_xml();
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n"));
        assert!(xml.ends_with("</NotepadPlus>\r\n"));
        let e = parse(&xml, "ScintillaContextMenu").unwrap();
        let path = |a: &str, b: &str| Cmd::Path(a.into(), b.into());
        assert_eq!(e.len(), 37);
        assert_eq!(
            e[0],
            Entry {
                cmd: path("Edit", "Cut"),
                label: "".into(),
                folder: "".into()
            }
        );
        assert_eq!(e[6].cmd, path("Edit", "Begin/End Select in Column Mode"));
        assert_eq!(e[7].cmd, Cmd::Sep);
        assert_eq!(
            e[8],
            Entry {
                cmd: Cmd::Id(43022),
                label: "".into(),
                folder: "Style all occurrences of token".into()
            }
        );
        assert_eq!(e[23].cmd, Cmd::Id(43032));
        assert_eq!(e[24].cmd, Cmd::Sep);
        assert_eq!(e[25].cmd, Cmd::Sep);
        assert_eq!(e[26].cmd, path("Edit", "UPPERCASE"));
        assert_eq!(e[36].cmd, path("View", "Hide lines"));
        assert!(parse(&xml, "TabContextMenu").unwrap().is_empty());
    }

    #[test]
    fn parse_attributes() {
        let xml = r#"<NotepadPlus><ScintillaContextMenu>
            <Item id="42001" ItemNameAs="Cut &amp; keep" FolderName="F"/>
            <Item id="x" MenuEntryName="Edit"/>
            <Item PluginEntryName="P" PluginCommandItemName="C"/>
            <Other><Item id="1"/></Other>
            </ScintillaContextMenu><TabContextMenu><Item id="0"/></TabContextMenu></NotepadPlus>"#;
        let e = parse(xml, "ScintillaContextMenu").unwrap();
        assert_eq!(
            e,
            vec![Entry {
                cmd: Cmd::Id(42001),
                label: "Cut & keep".into(),
                folder: "F".into()
            }]
        );
        assert_eq!(parse(xml, "TabContextMenu").unwrap()[0].cmd, Cmd::Sep);
        assert!(parse("<NotepadPlus><a></b></NotepadPlus>", "x").is_err());
    }

    fn sample() -> Vec<Node> {
        let i = |s: &str| Node::Item(s.into());
        vec![
            Node::Menu("File".into(), vec![i("New"), i("Close")]),
            Node::Menu(
                "&Edit".into(),
                vec![
                    i("Undo"),
                    i(""),
                    Node::Menu(
                        "Convert Case to".into(),
                        vec![i("UPPERCASE"), i("lowercase")],
                    ),
                    i("Select All\tCtrl+A"),
                    Node::Menu("Close".into(), vec![]),
                ],
            ),
            Node::Menu("View".into(), vec![i("Hide Lines")]),
        ]
    }

    #[test]
    fn menu_paths() {
        let t = sample();
        assert_eq!(find_path(&t, "Edit", "Undo"), Some(vec![1, 0]));
        assert_eq!(find_path(&t, "edit", "lowercase"), Some(vec![1, 2, 1]));
        assert_eq!(find_path(&t, "Edit", "Select all"), Some(vec![1, 3]));
        assert_eq!(find_path(&t, "View", "Hide lines"), Some(vec![2, 0]));
        assert_eq!(find_path(&t, "Edit", "Close"), None);
        assert_eq!(find_path(&t, "Edit", "New"), None);
        assert_eq!(find_path(&t, "Search", "Undo"), None);
        assert_eq!(purge("&Open...\tCtrl+O"), "Open...");
    }

    #[test]
    fn layout_rules() {
        let c = |f: &str, n: Option<u8>| (f.to_string(), n);
        let out = layout(vec![
            c("", None),
            c("", Some(1)),
            c("", None),
            c("", None),
            c("A", Some(2)),
            c("A", None),
            c("A", Some(3)),
            c("B", Some(4)),
            c("", Some(5)),
            c("A", Some(6)),
            c("", None),
        ]);
        assert_eq!(
            out,
            vec![
                Out::Cmd(1),
                Out::Sep,
                Out::Folder("A".into(), vec![Out::Cmd(2), Out::Sep, Out::Cmd(3)]),
                Out::Folder("B".into(), vec![Out::Cmd(4)]),
                Out::Cmd(5),
                Out::Folder("A".into(), vec![Out::Cmd(6)]),
            ]
        );
        let out = layout(vec![
            c("", Some(1)),
            c("", None),
            c("", Some(2)),
            c("", None),
        ]);
        assert_eq!(out, vec![Out::Cmd(1), Out::Sep, Out::Cmd(2)]);
    }

    #[test]
    fn tab_menu_layout() {
        let items = tab_items()
            .into_iter()
            .map(|(t, f, c)| (f.to_string(), c.map(|_| t)))
            .collect();
        let out = layout(items);
        assert_eq!(out[0], Out::Cmd("Close"));
        assert!(matches!(&out[1], Out::Folder(n, s) if n == "Close Multiple Tabs" && s.len() == 4));
        assert!(
            matches!(&out[4], Out::Folder(n, s) if n == "Open into" && s.len() == 4 && s[2] == Out::Sep)
        );
        assert!(
            matches!(out.last(), Some(Out::Folder(n, s)) if n == "Move Document" && s.len() == 2)
        );
    }
}
