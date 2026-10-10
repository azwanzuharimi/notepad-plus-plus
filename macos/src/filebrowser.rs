// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::attr;
use crate::docking::{column, content_box, scroll, FOLDERS};
use crate::session::{read_config, replace_element};
use crate::{ns, tools, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{sel, MainThreadOnly, Message};
use objc2_app_kit::{
    NSMenu, NSMenuItem, NSModalResponseOK, NSOpenPanel, NSOutlineView,
    NSTableColumnResizingOptions, NSTableViewColumnAutoresizingStyle, NSView, NSWorkspace,
};
use objc2_foundation::{NSArray, NSIndexSet, NSNumber, NSURL};
use quick_xml::escape::escape;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::cell::{OnceCell, RefCell};
use std::path::{Path, PathBuf};

// fileBrowser_rc.h IDM_FILEBROWSER_*.
const REMOVE_ROOT: isize = 3511;
const REMOVE_ALL: isize = 3512;
const ADD_ROOT: isize = 3513;
const RUN_BY_SYSTEM: isize = 3514;
const OPEN: isize = 3515;
const COPY_PATH: isize = 3516;
const FIND_IN_FILES: isize = 3517;
const FINDER_HERE: isize = 3518;
const TERMINAL_HERE: isize = 3519;
const COPY_FILE_NAME: isize = 3520;
const SELECT_FOLDER: &str = "Select a folder to add in Folder as Workspace panel";
// Port only: one refresh reads at most this number of unfolded folders.
const MAX_REFRESH: usize = 200;

thread_local! {
    static APP: OnceCell<Retained<App>> = const { OnceCell::new() };
}

// The FileBrowser element of config.xml: the selected item and each root with its unfolded folders.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Saved {
    pub selected: String,
    pub roots: Vec<(String, Vec<String>)>,
}

struct Node {
    path: PathBuf,
    name: String,
    dir: bool,
    kids: Option<Vec<usize>>,
    obj: Retained<NSNumber>,
}

pub struct Folders {
    outline: Retained<NSOutlineView>,
    nodes: RefCell<Vec<Node>>,
    roots: RefCell<Vec<usize>>,
    free: RefCell<Vec<usize>>,
}

// FileBrowser::categorySortFunc: folders first, then names without case (lstrcmpi).
pub fn sort(v: &mut [(String, bool)]) {
    v.sort_by_cached_key(|(name, dir)| (!*dir, name.to_lowercase(), name.clone()));
}

// FileBrowser::getDirectoryStructure: all files, and the folders that are not hidden; a link to a folder is not followed.
pub fn list(dir: &Path) -> Vec<(String, bool)> {
    let mut v: Vec<(String, bool)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let t = e.file_type().ok()?;
            let dir = t.is_dir();
            let skip = (t.is_symlink() && e.path().is_dir()) || (dir && name.starts_with('.'));
            (!skip).then_some((name, dir))
        })
        .collect();
    sort(&mut v);
    v
}

// FileBrowser::isRelatedRootFolder: the path is in the root folder.
pub fn related(root: &str, path: &str) -> bool {
    path.strip_prefix(root.trim_end_matches('/'))
        .is_some_and(|rest| rest.starts_with('/'))
}

// NppParameters::feedFileBrowserParameters.
pub fn parse_saved(xml: &str) -> Saved {
    let mut s = Saved::default();
    let mut r = Reader::from_str(xml);
    let mut inside = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "FileBrowser" => {
                    s.selected = attr(&e, "latestSelectedItem");
                    inside = true;
                }
                "root" if inside => {
                    let f = attr(&e, "foldername");
                    if !f.is_empty() {
                        s.roots.push((f, vec![]));
                    }
                }
                "expanded" if inside => {
                    let p = attr(&e, "path");
                    if let Some(root) = s.roots.last_mut().filter(|_| !p.is_empty()) {
                        root.1.push(p);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == "FileBrowser" => inside = false,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    s
}

// NppParameters::writeFileBrowserSettings: no attribute and no root when there is no root.
pub fn saved_xml(s: &Saved) -> String {
    if s.roots.is_empty() {
        return "<FileBrowser />".into();
    }
    let mut out = format!(
        "<FileBrowser latestSelectedItem=\"{}\">\r\n",
        escape(s.selected.as_str())
    );
    for (root, expanded) in &s.roots {
        out += &format!(
            "        <root foldername=\"{}\">\r\n",
            escape(root.as_str())
        );
        for p in expanded {
            out += &format!(
                "            <expanded path=\"{}\" />\r\n",
                escape(p.as_str())
            );
        }
        out += "        </root>\r\n";
    }
    out + "    </FileBrowser>"
}

// The FileBrowser element for session::save_config; config.xml keeps its old element when the panel was not launched.
pub fn patch_config(x: &str) -> Result<String, String> {
    match APP.with(|a| a.get().and_then(|app| app.folders_saved())) {
        Some(s) => replace_element(Some(x), "FileBrowser", &saved_xml(&s)),
        None => Ok(x.to_string()),
    }
}

fn file_url(p: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&ns(&p.to_string_lossy()))
}

fn node_name(p: &Path) -> String {
    p.file_name()
        .map_or(p.to_string_lossy(), |n| n.to_string_lossy())
        .into_owned()
}

impl App {
    fn folders(&self) -> Option<&Folders> {
        self.dock_ui()?.folders.get()
    }

    pub(crate) fn folders_build(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let b = content_box(mtm);
        let size = b.frame().size;
        let outline = NSOutlineView::initWithFrame(NSOutlineView::alloc(mtm), b.frame());
        let c = column(mtm, "name", "", size.width);
        c.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        outline.addTableColumn(&c);
        outline.setHeaderView(None);
        outline.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        let Some(d) = self.dock_ui() else { return b };
        let menu = NSMenu::new(mtm);
        menu.setDelegate(Some(ProtocolObject::from_ref(&*d.target)));
        unsafe { outline.setMenu(Some(&menu)) };
        unsafe {
            outline.setOutlineTableColumn(Some(&c));
            outline.setDataSource(Some(ProtocolObject::from_ref(&*d.target)));
            outline.setTarget(Some(&d.target));
            outline.setDoubleAction(Some(sel!(folderOpen:)));
        }
        b.addSubview(&scroll(mtm, &outline, size.width, size.height));
        let _ = d.folders.set(Folders {
            outline,
            nodes: RefCell::new(vec![]),
            roots: RefCell::new(vec![]),
            free: RefCell::new(vec![]),
        });
        APP.with(|a| {
            let _ = a.set(self.retain());
        });
        b
    }

    fn node_index(&self, item: Option<&AnyObject>) -> Option<usize> {
        item?
            .downcast_ref::<NSNumber>()
            .map(|n| n.integerValue() as usize)
    }

    fn node_add(&self, f: &Folders, path: PathBuf, name: String, dir: bool) -> usize {
        let mut nodes = f.nodes.borrow_mut();
        let k = f.free.borrow_mut().pop().unwrap_or(nodes.len());
        let n = Node {
            path,
            name,
            dir,
            kids: None,
            obj: NSNumber::new_isize(k as isize),
        };
        match nodes.get_mut(k) {
            Some(old) => *old = n,
            None => nodes.push(n),
        }
        k
    }

    // A node that left the tree gives its index, and the indexes of its loaded entries, for reuse.
    fn node_release(&self, f: &Folders, i: usize) {
        let mut todo = vec![i];
        while let Some(k) = todo.pop() {
            let kids = f.nodes.borrow_mut().get_mut(k).and_then(|n| n.kids.take());
            todo.extend(kids.unwrap_or_default());
            f.free.borrow_mut().push(k);
        }
    }

    // Lazy loading: a folder reads its entries the first time the tree asks for them.
    fn folder_kids(&self, f: &Folders, i: usize) -> Vec<usize> {
        let (path, kids) = match f.nodes.borrow().get(i) {
            Some(n) => (n.path.clone(), n.kids.clone()),
            None => return vec![],
        };
        if let Some(k) = kids {
            return k;
        }
        let kids: Vec<usize> = list(&path)
            .into_iter()
            .map(|(name, dir)| self.node_add(f, path.join(&name), name, dir))
            .collect();
        if let Some(n) = f.nodes.borrow_mut().get_mut(i) {
            n.kids = Some(kids.clone());
        }
        kids
    }

    fn folders_children(&self, item: Option<&AnyObject>) -> Vec<usize> {
        let Some(f) = self.folders() else {
            return vec![];
        };
        match self.node_index(item) {
            Some(i) => self.folder_kids(f, i),
            None => f.roots.borrow().clone(),
        }
    }

    pub(crate) fn folders_count(&self, item: Option<&AnyObject>) -> isize {
        self.folders_children(item).len() as isize
    }

    fn node_obj(&self, i: usize) -> Option<Retained<NSNumber>> {
        Some(self.folders()?.nodes.borrow().get(i)?.obj.clone())
    }

    pub(crate) fn folders_child(
        &self,
        n: isize,
        item: Option<&AnyObject>,
    ) -> Option<Retained<AnyObject>> {
        let k = *self.folders_children(item).get(n as usize)?;
        let obj = self.node_obj(k)?;
        Some(Retained::into_super(Retained::into_super(
            Retained::into_super(obj),
        )))
    }

    fn node_info(&self, i: usize) -> Option<(PathBuf, String, bool)> {
        let n = self.folders()?.nodes.borrow();
        let n = n.get(i)?;
        Some((n.path.clone(), n.name.clone(), n.dir))
    }

    pub(crate) fn folders_expandable(&self, item: &AnyObject) -> bool {
        self.node_index(Some(item))
            .and_then(|i| self.node_info(i))
            .is_some_and(|n| n.2)
    }

    pub(crate) fn folders_value(&self, item: Option<&AnyObject>) -> Option<Retained<AnyObject>> {
        let name = self.node_info(self.node_index(item)?)?.1;
        Some(Retained::into_super(Retained::into_super(ns(&name))))
    }

    fn folders_expand(&self, i: usize) {
        if let (Some(f), Some(obj)) = (self.folders(), self.node_obj(i)) {
            unsafe { f.outline.expandItem(Some(&obj)) };
        }
    }

    // FileBrowser::addRootFolder: a folder in a root is selected; a folder that holds a root is refused.
    pub(crate) fn folders_add_root(&self, path: &Path) {
        let Some(f) = self.folders() else { return };
        if !path.is_dir() {
            return;
        }
        let text = path.to_string_lossy();
        let text = if text.len() > 1 {
            text.trim_end_matches('/')
        } else {
            &text
        };
        let roots: Vec<String> = f
            .roots
            .borrow()
            .iter()
            .filter_map(|&r| self.node_info(r))
            .map(|n| n.0.to_string_lossy().into_owned())
            .collect();
        for r in &roots {
            if r == text {
                return;
            }
            if related(r, text) {
                self.folders_select(text);
                return;
            }
            if related(text, r) {
                self.alert(
                    "Folder as Workspace adding folder problem",
                    &format!("A sub-folder of the folder you want to add exists.\nPlease remove its root from the panel before you add folder \"{text}\"."),
                    &["OK"],
                );
                return;
            }
        }
        let p = PathBuf::from(text);
        let k = self.node_add(f, p.clone(), node_name(&p), true);
        f.roots.borrow_mut().push(k);
        f.outline.reloadData();
    }

    // FileBrowser::selectItemFromPath: unfold the folders down to the item and select it.
    fn folders_select(&self, path: &str) {
        let Some(f) = self.folders() else { return };
        let roots = f.roots.borrow().clone();
        let found = roots.into_iter().find_map(|r| {
            let root = self.node_info(r)?.0.to_string_lossy().into_owned();
            (root == path || related(&root, path)).then(|| (r, path[root.len()..].to_string()))
        });
        let Some((mut cur, rest)) = found else { return };
        for part in rest.split('/').filter(|p| !p.is_empty()) {
            let next = self
                .folder_kids(f, cur)
                .into_iter()
                .find(|&k| self.node_info(k).is_some_and(|n| n.1 == part));
            let Some(next) = next else { break };
            self.folders_expand(cur);
            cur = next;
        }
        let Some(obj) = self.node_obj(cur) else {
            return;
        };
        let row = unsafe { f.outline.rowForItem(Some(&obj)) };
        if row >= 0 {
            f.outline.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(row as usize),
                false,
            );
            f.outline.scrollRowToVisible(row);
        }
    }

    fn folders_restore_expanded(&self, i: usize, expanded: &[String]) {
        let Some(f) = self.folders() else { return };
        let Some((path, _, true)) = self.node_info(i) else {
            return;
        };
        if !expanded.iter().any(|p| Path::new(p) == path) {
            return;
        }
        self.folders_expand(i);
        for k in self.folder_kids(f, i) {
            self.folders_restore_expanded(k, expanded);
        }
    }

    // NppCommands.cpp IDM_VIEW_FILEBROWSER: the first launch reads the roots of config.xml.
    pub(crate) fn folders_toggle(&self) {
        let first = self.folders().is_none();
        self.toggle_panel(FOLDERS);
        if !first {
            return;
        }
        let saved = read_config()
            .map(|x| parse_saved(&x))
            .unwrap_or_default();
        for (root, expanded) in &saved.roots {
            self.folders_add_root(Path::new(root));
            let k = self
                .folders()
                .and_then(|f| f.roots.borrow().last().copied());
            if let Some(k) = k {
                self.folders_restore_expanded(k, expanded);
            }
        }
        self.folders_select(&saved.selected);
    }

    // NppCommands.cpp IDM_FILE_OPENFOLDERASWORKSPACE.
    pub(crate) fn open_folder_as_workspace(&self) {
        let p = NSOpenPanel::openPanel(self.mtm());
        p.setCanChooseDirectories(true);
        p.setCanChooseFiles(false);
        p.setMessage(Some(&ns(SELECT_FOLDER)));
        if p.runModal() != NSModalResponseOK {
            return;
        }
        let Some(path) = p.URL().and_then(|u| u.path()) else {
            return;
        };
        if !self.panel_visible(FOLDERS) {
            self.dock_open_panel(FOLDERS);
        }
        self.folders_add_root(Path::new(&path.to_string()));
    }

    // NppCommands.cpp IDM_FILE_CONTAININGFOLDERASWORKSPACE: the folder of the current file becomes a root, and the file is selected.
    pub(crate) fn containing_folder_as_workspace(&self) {
        let Some(file) = self.current().and_then(|i| self.tab(i)?.path) else {
            return;
        };
        if self.folders().is_none() {
            self.folders_toggle();
        }
        if let Some(dir) = file.parent() {
            self.folders_add_root(dir);
        }
        self.dock_open_panel(FOLDERS);
        self.folders_select(&file.to_string_lossy());
    }

    // The unfolded folders, at most MAX_REFRESH of them.
    fn expanded_folders(&self) -> Vec<usize> {
        let Some(f) = self.folders() else {
            return vec![];
        };
        let mut todo = f.roots.borrow().clone();
        let mut out = vec![];
        while let Some(i) = todo.pop() {
            let Some(obj) = self.node_obj(i) else { continue };
            if !unsafe { f.outline.isItemExpanded(Some(&obj)) } {
                continue;
            }
            out.push(i);
            if out.len() == MAX_REFRESH {
                break;
            }
            todo.extend(f.nodes.borrow().get(i).and_then(|n| n.kids.clone()).unwrap_or_default());
        }
        out
    }

    // Port only: in place of the ReadDirectoryChangesW watcher, the unfolded folders read their entries again when the app becomes active.
    pub(crate) fn folders_refresh(&self) {
        let Some(f) = self.folders() else { return };
        if !self.panel_visible(FOLDERS) {
            return;
        }
        let selected = f.outline.itemAtRow(f.outline.selectedRow());
        for i in self.expanded_folders() {
            let Some(path) = self.node_info(i).map(|n| n.0) else {
                continue;
            };
            let old: Vec<(usize, String, bool)> = self
                .folder_kids(f, i)
                .into_iter()
                .filter_map(|k| self.node_info(k).map(|n| (k, n.1, n.2)))
                .collect();
            let new = list(&path);
            let same = old.len() == new.len()
                && old.iter().zip(&new).all(|(o, n)| o.1 == n.0 && o.2 == n.1);
            if same {
                continue;
            }
            let kids: Vec<usize> = new
                .into_iter()
                .map(|(name, dir)| {
                    old.iter()
                        .find(|o| o.1 == name && o.2 == dir)
                        .map(|o| o.0)
                        .unwrap_or_else(|| self.node_add(f, path.join(&name), name, dir))
                })
                .collect();
            for o in old.iter().filter(|o| !kids.contains(&o.0)) {
                self.node_release(f, o.0);
            }
            if let Some(n) = f.nodes.borrow_mut().get_mut(i) {
                n.kids = Some(kids);
            }
            if let Some(obj) = self.node_obj(i) {
                unsafe { f.outline.reloadItem_reloadChildren(Some(&obj), true) };
            }
        }
        let row = unsafe { f.outline.rowForItem(selected.as_deref()) };
        if row >= 0 && row != f.outline.selectedRow() {
            f.outline.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(row as usize),
                false,
            );
        }
    }

    // Notepad_plus::saveFileBrowserParam: the state to save, when the panel was launched.
    fn folders_saved(&self) -> Option<Saved> {
        let f = self.folders()?;
        let selected = f.outline.selectedRow();
        let selected = (selected >= 0)
            .then(|| self.node_index(f.outline.itemAtRow(selected).as_deref()))
            .flatten()
            .and_then(|i| self.node_info(i))
            .map_or(String::new(), |n| n.0.to_string_lossy().into_owned());
        let mut saved = Saved {
            selected,
            roots: vec![],
        };
        for r in f.roots.borrow().iter() {
            let Some(root) = self.node_info(*r) else {
                continue;
            };
            let mut expanded = vec![];
            let mut todo = vec![*r];
            while let Some(i) = todo.pop() {
                let Some(obj) = self.node_obj(i) else {
                    continue;
                };
                if !unsafe { f.outline.isItemExpanded(Some(&obj)) } {
                    continue;
                }
                if let Some(n) = self.node_info(i) {
                    expanded.push(n.0.to_string_lossy().into_owned());
                }
                todo.extend(
                    f.nodes
                        .borrow()
                        .get(i)
                        .and_then(|n| n.kids.clone())
                        .unwrap_or_default(),
                );
            }
            expanded.sort();
            saved
                .roots
                .push((root.0.to_string_lossy().into_owned(), expanded));
        }
        Some(saved)
    }

    // FileBrowser NM_DBLCLK: a file opens; a folder folds or unfolds.
    pub(crate) fn folders_open(&self) {
        let Some(f) = self.folders() else { return };
        let row = f.outline.clickedRow();
        let item = (row >= 0).then(|| f.outline.itemAtRow(row)).flatten();
        let Some((path, _, dir)) = self
            .node_index(item.as_deref())
            .and_then(|i| self.node_info(i))
        else {
            return;
        };
        if dir {
            unsafe {
                if f.outline.isItemExpanded(item.as_deref()) {
                    f.outline.collapseItem(item.as_deref());
                } else {
                    f.outline.expandItem(item.as_deref());
                }
            }
        } else if path.is_file() {
            self.open_path(&path);
        }
    }

    fn folders_selected(&self) -> Option<(usize, PathBuf, String, bool)> {
        let f = self.folders()?;
        let row = f.outline.selectedRow();
        let i = self.node_index(f.outline.itemAtRow(row).as_deref())?;
        let (p, n, d) = self.node_info(i)?;
        Some((i, p, n, d))
    }

    // FileBrowser::showContextMenu: the menu of the clicked item, or of the panel when no item is clicked.
    pub(crate) fn folders_menu(&self, m: &NSMenu) {
        let Some(f) = self.folders() else { return };
        m.removeAllItems();
        let row = f.outline.clickedRow();
        if row >= 0 {
            f.outline.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(row as usize),
                false,
            );
        }
        let kind = (row >= 0)
            .then(|| self.folders_selected())
            .flatten()
            .map(|s| match (f.roots.borrow().contains(&s.0), s.3) {
                (true, _) => 0,
                (false, true) => 1,
                _ => 2,
            });
        let here = [
            (TERMINAL_HERE, "Terminal here"),
            (FINDER_HERE, "Finder here"),
        ];
        let mut items: Vec<(isize, &str)> = match kind {
            None => vec![(REMOVE_ALL, "Remove All"), (ADD_ROOT, "Add")],
            Some(2) => here
                .into_iter()
                .chain([
                    (0, ""),
                    (RUN_BY_SYSTEM, "Run by system"),
                    (COPY_FILE_NAME, "Copy file name"),
                    (COPY_PATH, "Copy path"),
                    (0, ""),
                    (OPEN, "Open"),
                ])
                .collect(),
            Some(_) => here
                .into_iter()
                .chain([
                    (0, ""),
                    (FIND_IN_FILES, "Find in Files..."),
                    (COPY_PATH, "Copy path"),
                ])
                .collect(),
        };
        if kind == Some(0) {
            items.extend([(0, ""), (REMOVE_ROOT, "Remove")]);
        }
        let Some(d) = self.dock_ui() else { return };
        for (tag, title) in items {
            if tag == 0 {
                m.addItem(&NSMenuItem::separatorItem(self.mtm()));
                continue;
            }
            let it = crate::item(self.mtm(), title, sel!(folderCmd:), "", Some(&d.target));
            it.setTag(tag);
            m.addItem(&it);
        }
    }

    // FileBrowser::popupMenuCmd.
    pub(crate) fn folders_cmd(&self, cmd: isize) {
        let Some(f) = self.folders() else { return };
        match cmd {
            ADD_ROOT => {
                let p = NSOpenPanel::openPanel(self.mtm());
                p.setCanChooseDirectories(true);
                p.setCanChooseFiles(false);
                p.setMessage(Some(&ns(SELECT_FOLDER)));
                if p.runModal() == NSModalResponseOK {
                    if let Some(path) = p.URL().and_then(|u| u.path()) {
                        self.folders_add_root(Path::new(&path.to_string()));
                    }
                }
                return;
            }
            REMOVE_ALL => {
                f.roots.borrow_mut().clear();
                f.nodes.borrow_mut().clear();
                f.free.borrow_mut().clear();
                f.outline.reloadData();
                return;
            }
            _ => {}
        }
        let Some((i, path, name, dir)) = self.folders_selected() else {
            return;
        };
        match cmd {
            REMOVE_ROOT => {
                if f.roots.borrow().contains(&i) {
                    f.roots.borrow_mut().retain(|&r| r != i);
                    f.outline.reloadData();
                    self.node_release(f, i);
                }
            }
            COPY_PATH => tools::to_clipboard(&path.to_string_lossy()),
            COPY_FILE_NAME => tools::to_clipboard(&name),
            OPEN if path.is_file() => self.open_path(&path),
            RUN_BY_SYSTEM if path.exists() => {
                NSWorkspace::sharedWorkspace().openURL(&file_url(&path));
            }
            FIND_IN_FILES => {
                let u = self.fif_ui();
                u.dir.setStringValue(&ns(&path.to_string_lossy()));
                self.show_panel(&u.form, &u.c.find, &u.c.find);
            }
            FINDER_HERE if dir && path.exists() => {
                NSWorkspace::sharedWorkspace().openURL(&file_url(&path));
            }
            FINDER_HERE if path.exists() => {
                NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(
                    &NSArray::from_retained_slice(&[file_url(&path)]),
                );
            }
            TERMINAL_HERE => {
                let dir = if dir {
                    path.as_path()
                } else {
                    path.parent().unwrap_or(&path)
                };
                if dir.exists() {
                    if let Err(e) = std::process::Command::new("open")
                        .arg("-a")
                        .arg("Terminal")
                        .arg(dir)
                        .spawn()
                    {
                        self.alert("Cannot open Terminal", &e.to_string(), &["OK"]);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_first_without_case() {
        let mut v: Vec<(String, bool)> = [
            ("b.txt", false),
            ("Zeta", true),
            ("a.txt", false),
            ("alpha", true),
            ("C.txt", false),
        ]
        .iter()
        .map(|(n, d)| (n.to_string(), *d))
        .collect();
        sort(&mut v);
        let names: Vec<&str> = v.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(names, ["alpha", "Zeta", "a.txt", "b.txt", "C.txt"]);
    }

    #[test]
    fn lists_a_folder() {
        let dir = std::env::temp_dir().join(format!("npp-fb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["sub", ".git"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in ["B.md", "a.rs", ".env"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let got = list(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let want: Vec<(String, bool)> = [
            ("sub", true),
            (".env", false),
            ("a.rs", false),
            ("B.md", false),
        ]
        .iter()
        .map(|(n, d)| (n.to_string(), *d))
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn related_folders() {
        assert!(related("/a/b", "/a/b/c"));
        assert!(related("/a/b/", "/a/b/c/d.txt"));
        assert!(!related("/a/b", "/a/bc"));
        assert!(!related("/a/b", "/a/b"));
    }

    #[test]
    fn file_browser_round_trip() {
        let s = Saved {
            selected: "/w/proj/src/main.rs".into(),
            roots: vec![
                (
                    "/w/proj".into(),
                    vec!["/w/proj".into(), "/w/proj/src".into()],
                ),
                ("/w/a&b \"q\"".into(), vec![]),
            ],
        };
        let config = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    <History nbMaxFile=\"10\" />\r\n    <FileBrowser latestSelectedItem=\"/old\">\r\n        <root foldername=\"/old\" />\r\n    </FileBrowser>\r\n    <GUIConfigs />\r\n</NotepadPlus>\r\n";
        let write_saved = |src: Option<&str>, s: &Saved| replace_element(src, "FileBrowser", &saved_xml(s));
        let out = write_saved(Some(config), &s).unwrap();
        assert_eq!(parse_saved(&out), s);
        assert!(out.contains("<History nbMaxFile=\"10\" />"));
        assert!(out.contains("<GUIConfigs />"));
        assert!(!out.contains("/old"));
        let added = write_saved(Some("<NotepadPlus>\r\n</NotepadPlus>\r\n"), &s).unwrap();
        assert_eq!(parse_saved(&added), s);
        assert_eq!(parse_saved(&write_saved(None, &s).unwrap()), s);
        let empty = write_saved(Some(&out), &Saved::default()).unwrap();
        assert!(empty.contains("<FileBrowser />"));
        assert_eq!(parse_saved(&empty), Saved::default());
    }
}
