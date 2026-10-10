// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::attr;
use crate::docking::{column, content_box, frame, scroll, PROJECTS};
use crate::search::{self, FifArgs, Opts};
use crate::session::{read_config, replace_element, write_file};
use crate::{filebrowser, l10n, ns, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSAttributedStringAttachmentConveniences,
    NSAutoresizingMaskOptions, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSImage, NSImageSymbolConfiguration, NSMenu, NSMenuDelegate,
    NSMenuItem, NSModalResponseOK, NSOpenPanel, NSOutlineView, NSOutlineViewDataSource,
    NSPopUpButton, NSSavePanel, NSTableColumn, NSTableColumnResizingOptions,
    NSTableViewColumnAutoresizingStyle, NSTextAttachment, NSTextField, NSView,
};
use objc2_foundation::{
    NSAttributedString, NSDictionary, NSIndexSet, NSMutableAttributedString, NSNumber,
    NSObjectProtocol, NSURL,
};
use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Component, Path, PathBuf};

// ProjectPanel_rc.h IDM_PROJECT_*.
const RENAME: isize = 3111;
const NEW_FOLDER: isize = 3112;
const ADD_FILES: isize = 3113;
const DELETE_FOLDER: isize = 3114;
const DELETE_FILE: isize = 3115;
const MODIFY_FILE_PATH: isize = 3116;
const ADD_FILES_RECURSIVELY: isize = 3117;
const MOVE_UP: isize = 3118;
const MOVE_DOWN: isize = 3119;
const NEW_PROJECT: isize = 3121;
const NEW_WS: isize = 3122;
const OPEN_WS: isize = 3123;
const RELOAD_WS: isize = 3124;
const SAVE_WS: isize = 3125;
const SAVE_AS_WS: isize = 3126;
const SAVE_COPY_AS_WS: isize = 3127;
const BAR_H: f64 = 28.;
const MODIFIED: &str =
    "The current workspace was modified. Do you want to save the current project?";

thread_local! {
    static APP: OnceCell<Retained<App>> = const { OnceCell::new() };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Root,
    Project,
    Folder,
    File,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub name: String,
    pub path: PathBuf,
    pub parent: usize,
    pub kids: Vec<usize>,
}

// The tree of one workspace; node 0 is the workspace. A removed node stays in the list, but no parent holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    pub nodes: Vec<Node>,
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map_or(p.to_string_lossy(), |n| n.to_string_lossy())
        .into_owned()
}

impl Tree {
    pub fn new(root: &str) -> Tree {
        Tree {
            nodes: vec![Node {
                kind: Kind::Root,
                name: root.into(),
                path: PathBuf::new(),
                parent: 0,
                kids: vec![],
            }],
        }
    }

    pub fn add(&mut self, parent: usize, kind: Kind, name: &str, path: PathBuf) -> usize {
        let k = self.nodes.len();
        self.nodes.push(Node {
            kind,
            name: name.into(),
            path,
            parent,
            kids: vec![],
        });
        if let Some(p) = self.nodes.get_mut(parent) {
            p.kids.push(k);
        }
        k
    }

    pub fn add_file(&mut self, parent: usize, path: PathBuf) -> usize {
        self.add(parent, Kind::File, &file_name(&path), path)
    }

    pub fn remove(&mut self, i: usize) {
        if i == 0 {
            return;
        }
        let Some(parent) = self.nodes.get(i).map(|n| n.parent) else {
            return;
        };
        if let Some(p) = self.nodes.get_mut(parent) {
            p.kids.retain(|&k| k != i);
        }
    }

    // TreeView::moveUp and moveDown: swap with the sibling before or after.
    pub fn move_by(&mut self, i: usize, up: bool) -> bool {
        let Some(parent) = self.nodes.get(i).filter(|_| i != 0).map(|n| n.parent) else {
            return false;
        };
        let kids = &mut self.nodes[parent].kids;
        let Some(k) = kids.iter().position(|&x| x == i) else {
            return false;
        };
        let other = if up { k.checked_sub(1) } else { Some(k + 1) };
        match other.filter(|&o| o < kids.len()) {
            Some(o) => {
                kids.swap(k, o);
                true
            }
            None => false,
        }
    }

    // ProjectPanel::recursiveAddFilesFrom: the folders that are not hidden first, each as a folder node, then the files.
    pub fn add_dir(&mut self, parent: usize, dir: &Path) {
        for (name, is_dir) in filebrowser::list(dir) {
            if is_dir {
                let f = self.add(parent, Kind::Folder, &name, PathBuf::new());
                self.add_dir(f, &dir.join(&name));
            } else {
                self.add_file(parent, dir.join(&name));
            }
        }
    }

    // ProjectPanel::enumWorkSpaceFiles: the files whose name matches the filters, in tree order.
    pub fn files(&self, pats: &[String]) -> Vec<PathBuf> {
        let mut out = vec![];
        let mut todo = vec![0];
        while let Some(i) = todo.pop() {
            let Some(n) = self.nodes.get(i) else { continue };
            if n.kind == Kind::File {
                if search::match_file(&n.name, pats) {
                    out.push(n.path.clone());
                }
            } else {
                todo.extend(n.kids.iter().rev());
            }
        }
        out
    }
}

// ProjectPanel::getRelativePath: a file in the folder of the workspace file is written relative to it; other files keep the full path.
pub fn relative_path(file: &Path, ws: &Path) -> String {
    match ws.parent().and_then(|d| file.strip_prefix(d).ok()) {
        Some(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().into_owned(),
        _ => file.to_string_lossy().into_owned(),
    }
}

// ProjectPanel::getAbsoluteFilePath: PathAppend to the folder of the workspace file, which also removes "." and "..".
pub fn absolute_path(name: &str, ws: &Path) -> PathBuf {
    let p = Path::new(name);
    if p.is_absolute() || windows_absolute(name) {
        return p.to_path_buf();
    }
    let mut out = ws.parent().map(Path::to_path_buf).unwrap_or_default();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s),
            _ => {}
        }
    }
    out
}

// PathIsRelative is false for a drive path (C:\ or C:/) and a UNC path (\\server); such a path stays as it is.
fn windows_absolute(name: &str) -> bool {
    let b = name.as_bytes();
    name.starts_with(r"\\")
        || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))
}

// ProjectPanel TVN_ENDLABELEDIT: the last occurrence of the old label in the file path becomes the new label.
pub fn renamed_path(path: &str, old: &str, new: &str) -> String {
    match path.rfind(old).filter(|_| !old.is_empty()) {
        Some(i) => format!("{}{new}{}", &path[..i], &path[i + old.len()..]),
        None => path.to_string(),
    }
}

enum Ctx {
    Doc,
    Root,
    Node(usize),
    Skip,
}

// ProjectPanel::openWorkSpace and buildTreeFrom: None when the file is not a workspace (no NotepadPlus element or no Project).
pub fn parse_workspace(xml: &str, ws: &Path) -> Option<Tree> {
    let mut t = Tree::new(&file_name(ws));
    let mut stack = vec![Ctx::Doc];
    let mut r = Reader::from_str(xml);
    let mut seen_root = false;
    loop {
        let (e, start) = match r.read_event() {
            Ok(Event::Start(e)) => (e, true),
            Ok(Event::Empty(e)) => (e, false),
            Ok(Event::End(_)) => {
                stack.pop();
                continue;
            }
            Ok(Event::Eof) if stack.len() == 1 => break,
            Ok(Event::Eof) | Err(_) => return None,
            _ => continue,
        };
        let ctx = match stack.last() {
            Some(Ctx::Doc) if !seen_root && e.name().as_ref() == "NotepadPlus" => {
                seen_root = true;
                Ctx::Root
            }
            Some(Ctx::Root) if e.name().as_ref() == "Project" => {
                Ctx::Node(t.add(0, Kind::Project, &attr(&e, "name"), PathBuf::new()))
            }
            Some(Ctx::Node(p)) => node(&mut t, *p, &e, ws),
            _ => Ctx::Skip,
        };
        if start {
            stack.push(ctx);
        }
    }
    (!t.nodes[0].kids.is_empty()).then_some(t)
}

fn node(t: &mut Tree, parent: usize, e: &BytesStart, ws: &Path) -> Ctx {
    let name = attr(e, "name");
    match e.name().as_ref() {
        "Folder" => Ctx::Node(t.add(parent, Kind::Folder, &name, PathBuf::new())),
        "File" if !name.is_empty() => {
            let path = absolute_path(&name, ws);
            t.add(parent, Kind::File, &file_name(Path::new(&name)), path);
            Ctx::Skip
        }
        _ => Ctx::Skip,
    }
}

// ProjectPanel::writeWorkSpace: the pugixml output with four spaces of indent and no declaration.
pub fn workspace_xml(t: &Tree, ws: &Path) -> String {
    fn write(t: &Tree, i: usize, depth: usize, ws: &Path, out: &mut String) {
        let Some(n) = t.nodes.get(i) else { return };
        let pad = "    ".repeat(depth);
        let (tag, value) = match n.kind {
            Kind::Root => ("NotepadPlus", None),
            Kind::Project => ("Project", Some(n.name.clone())),
            Kind::Folder => ("Folder", Some(n.name.clone())),
            Kind::File => ("File", Some(relative_path(&n.path, ws))),
        };
        let a = value.map_or(String::new(), |v| {
            format!(" name=\"{}\"", escape(v.as_str()))
        });
        if n.kind == Kind::File || n.kids.is_empty() {
            *out += &format!("{pad}<{tag}{a} />\n");
            return;
        }
        *out += &format!("{pad}<{tag}{a}>\n");
        for &k in &n.kids {
            write(t, k, depth + 1, ws, out);
        }
        *out += &format!("{pad}</{tag}>\n");
    }
    let mut out = String::new();
    write(t, 0, 0, ws, &mut out);
    out
}

// The workspace file and the open state of each panel.
pub type Saved = [(String, bool); 3];

// NppParameters::feedProjectPanelsParameters.
pub fn parse_saved(xml: &str) -> Saved {
    let mut s = Saved::default();
    let mut r = Reader::from_str(xml);
    let mut inside = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "ProjectPanels" => inside = true,
                "ProjectPanel" if inside => {
                    if let Some(p) = attr(&e, "id")
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| s.get_mut(i))
                    {
                        *p = (attr(&e, "workSpaceFile"), attr(&e, "isVisible") == "yes");
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == "ProjectPanels" => inside = false,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    s
}

// NppParameters::writeProjectPanelsSettings; port only: isVisible keeps the open state, which Notepad++ keeps in DockingManager.
pub fn saved_xml(s: &Saved) -> String {
    let mut out = "<ProjectPanels>\r\n".to_string();
    for (i, (file, visible)) in s.iter().enumerate() {
        out += &format!(
            "        <ProjectPanel id=\"{i}\" workSpaceFile=\"{}\" isVisible=\"{}\" />\r\n",
            escape(file.as_str()),
            if *visible { "yes" } else { "no" }
        );
    }
    out + "    </ProjectPanels>"
}

// The ProjectPanels element for session::save_config; config.xml keeps its old element when no panel was launched.
pub fn patch_config(x: &str) -> Result<String, String> {
    let Some(app) = APP.with(|a| a.get().cloned()) else {
        return Ok(x.to_string());
    };
    let mut s = parse_saved(x);
    for (k, p) in s.iter_mut().enumerate() {
        match app.project(k) {
            Some(panel) => {
                let file = panel.file.borrow();
                *p = (
                    file.as_deref().map_or(panel.lost.borrow().clone(), |f| {
                        f.to_string_lossy().into_owned()
                    }),
                    app.panel_visible(PROJECTS[k]),
                );
            }
            None => p.1 = false,
        }
    }
    replace_element(Some(x), "ProjectPanels", &saved_xml(&s))
}

// Notepad_plus::createFilelistForProjects for workspace trees.
pub fn gather(trees: &[&Tree], filters: &str) -> Vec<PathBuf> {
    let pats = search::patterns(filters);
    trees.iter().flat_map(|t| t.files(&pats)).collect()
}

pub struct Ivars {
    app: Retained<App>,
    k: usize,
}

define_class!(
    // Receives the data requests, menus and actions of one project panel.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub(crate) struct ProjectTarget;

    impl ProjectTarget {
        #[unsafe(method(projectCmd:))]
        fn cmd(&self, s: &NSMenuItem) {
            self.ivars().app.project_cmd(self.ivars().k, s.tag());
        }

        #[unsafe(method(projectOpen:))]
        fn open(&self, _s: Option<&AnyObject>) {
            self.ivars().app.project_open_item(self.ivars().k);
        }
    }

    unsafe impl NSObjectProtocol for ProjectTarget {}

    unsafe impl NSOutlineViewDataSource for ProjectTarget {
        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        fn count(&self, _o: &NSOutlineView, item: Option<&AnyObject>) -> isize {
            self.ivars().app.project_kids(self.ivars().k, item).len() as isize
        }

        #[unsafe(method_id(outlineView:child:ofItem:))]
        fn child(&self, _o: &NSOutlineView, n: isize, item: Option<&AnyObject>) -> Option<Retained<AnyObject>> {
            self.ivars().app.project_child(self.ivars().k, n, item)
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        fn expandable(&self, _o: &NSOutlineView, item: &AnyObject) -> bool {
            self.ivars().app.project_node(self.ivars().k, Some(item)).is_some_and(|n| !n.kids.is_empty())
        }

        #[unsafe(method_id(outlineView:objectValueForTableColumn:byItem:))]
        fn object(&self, _o: &NSOutlineView, _c: Option<&NSTableColumn>, item: Option<&AnyObject>) -> Option<Retained<AnyObject>> {
            self.ivars().app.project_label(self.ivars().k, item)
        }
    }

    unsafe impl NSMenuDelegate for ProjectTarget {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, m: &NSMenu) {
            self.ivars().app.project_menu(self.ivars().k, m);
        }
    }
);

pub struct Panel {
    outline: Retained<NSOutlineView>,
    target: Retained<ProjectTarget>,
    edit: Retained<NSMenu>,
    tree: RefCell<Tree>,
    objs: RefCell<Vec<Retained<NSNumber>>>,
    file: RefCell<Option<PathBuf>>,
    // The workspace file of config.xml that did not load; config.xml keeps it until the panel gets another workspace.
    lost: RefCell<String>,
    dirty: Cell<bool>,
    last_dir: RefCell<Option<PathBuf>>,
}

impl Panel {
    fn obj(&self, i: usize) -> Retained<NSNumber> {
        let mut objs = self.objs.borrow_mut();
        while objs.len() <= i {
            let n = objs.len() as isize;
            objs.push(NSNumber::new_isize(n));
        }
        objs[i].clone()
    }

    fn node(&self, i: usize) -> Option<Node> {
        self.tree.borrow().nodes.get(i).cloned()
    }
}

fn item_index(item: Option<&AnyObject>) -> Option<usize> {
    item?
        .downcast_ref::<NSNumber>()
        .map(|n| n.integerValue() as usize)
}

// The icon of a node, as the TvIndex images of ProjectPanel.cpp, and its name.
fn label(symbol: &str, color: &NSColor, text: &str) -> Retained<AnyObject> {
    let out = NSMutableAttributedString::new();
    let cfg = NSImageSymbolConfiguration::configurationWithHierarchicalColor(color);
    let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(symbol), None)
        .and_then(|i| i.imageWithSymbolConfiguration(&cfg));
    if let Some(img) = img {
        let a = NSTextAttachment::new();
        a.setImage(Some(&img));
        out.appendAttributedString(&NSAttributedString::attributedStringWithAttachment(&a));
    }
    let font = NSFont::systemFontOfSize(NSFont::systemFontSize());
    let color = NSColor::labelColor();
    let attrs = unsafe {
        NSDictionary::from_slices(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &[&*font as &AnyObject, &*color as &AnyObject],
        )
    };
    let text = unsafe { NSAttributedString::new_with_attributes(&ns(&format!(" {text}")), &attrs) };
    out.appendAttributedString(&text);
    Retained::into_super(Retained::into_super(Retained::into_super(out)))
}

impl App {
    pub(crate) fn project(&self, k: usize) -> Option<&Panel> {
        self.dock_ui()?.projects.get(k)?.get()
    }

    fn project_title(&self, k: usize) -> String {
        let file = self.project(k).and_then(|p| p.file.borrow().clone());
        file.map_or(panel_title(k), |f| file_name(&f))
    }

    pub(crate) fn project_build(&self, k: usize) -> Retained<NSView> {
        let mtm = self.mtm();
        let b = content_box(mtm);
        let Some(d) = self.dock_ui() else { return b };
        let size = b.frame().size;
        let target: Retained<ProjectTarget> = {
            let this = ProjectTarget::alloc(mtm).set_ivars(Ivars {
                app: self.retain(),
                k,
            });
            unsafe { msg_send![super(this), init] }
        };
        let t: &AnyObject = &target;
        let outline = NSOutlineView::initWithFrame(
            NSOutlineView::alloc(mtm),
            frame(0., 0., size.width, size.height - BAR_H),
        );
        let c = column(mtm, "name", "", size.width);
        c.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        outline.addTableColumn(&c);
        outline.setHeaderView(None);
        outline.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        let menu = |title: &str| {
            let m = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(title));
            m.setDelegate(Some(ProtocolObject::from_ref(&*target)));
            m
        };
        let ctx = menu("");
        unsafe {
            outline.setMenu(Some(&ctx));
            outline.setOutlineTableColumn(Some(&c));
            outline.setDataSource(Some(ProtocolObject::from_ref(&*target)));
            outline.setTarget(Some(t));
            outline.setDoubleAction(Some(sel!(projectOpen:)));
        }
        b.addSubview(&scroll(mtm, &outline, size.width, size.height - BAR_H));
        let ws = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(""));
        ws.addItem(&NSMenuItem::new(mtm));
        for (tag, title) in WORKSPACE_MENU {
            ws.addItem(&menu_item(self, t, "WorkspaceMenu", tag, title));
        }
        let edit = menu("");
        edit.addItem(&NSMenuItem::new(mtm));
        for (x, (title, m)) in [(4., (entry(0), &ws)), (128., (entry(1), &edit))] {
            let p = NSPopUpButton::initWithFrame_pullsDown(
                NSPopUpButton::alloc(mtm),
                frame(x, size.height - BAR_H + 2., 120., 24.),
                true,
            );
            if let Some(first) = m.itemAtIndex(0) {
                first.setTitle(&ns(&title));
            }
            p.setMenu(Some(m));
            p.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
            b.addSubview(&p);
        }
        let _ = d.projects[k].set(Panel {
            outline,
            target,
            edit,
            tree: RefCell::new(Tree::new(&root_name())),
            objs: RefCell::new(vec![]),
            file: RefCell::new(None),
            lost: RefCell::new(String::new()),
            dirty: Cell::new(false),
            last_dir: RefCell::new(None),
        });
        APP.with(|a| {
            let _ = a.set(self.retain());
        });
        b
    }

    fn project_kids(&self, k: usize, item: Option<&AnyObject>) -> Vec<usize> {
        let Some(p) = self.project(k) else {
            return vec![];
        };
        match item_index(item) {
            Some(i) => p.node(i).map(|n| n.kids).unwrap_or_default(),
            None => vec![0],
        }
    }

    fn project_child(
        &self,
        k: usize,
        n: isize,
        item: Option<&AnyObject>,
    ) -> Option<Retained<AnyObject>> {
        let i = *self.project_kids(k, item).get(n as usize)?;
        let o = self.project(k)?.obj(i);
        Some(Retained::into_super(Retained::into_super(
            Retained::into_super(o),
        )))
    }

    fn project_node(&self, k: usize, item: Option<&AnyObject>) -> Option<Node> {
        self.project(k)?.node(item_index(item)?)
    }

    fn project_label(&self, k: usize, item: Option<&AnyObject>) -> Option<Retained<AnyObject>> {
        let p = self.project(k)?;
        let n = p.node(item_index(item)?)?;
        let (symbol, color) = match n.kind {
            Kind::Root if p.dirty.get() => ("square.stack.3d.up.fill", NSColor::systemRedColor()),
            Kind::Root => ("square.stack.3d.up", NSColor::systemBlueColor()),
            Kind::Project => ("shippingbox", NSColor::systemBrownColor()),
            Kind::Folder => ("folder", NSColor::systemBlueColor()),
            Kind::File if n.path.exists() => ("doc", NSColor::secondaryLabelColor()),
            Kind::File => ("doc.questionmark", NSColor::systemRedColor()),
        };
        Some(label(symbol, &color, &n.name))
    }

    fn project_reload(&self, k: usize) {
        if let Some(p) = self.project(k) {
            p.outline.reloadData();
        }
    }

    fn project_expand(&self, k: usize, i: usize) {
        if let Some(p) = self.project(k) {
            unsafe { p.outline.expandItem(Some(&p.obj(i))) };
        }
    }

    fn project_select(&self, k: usize, i: usize) {
        let Some(p) = self.project(k) else { return };
        let row = unsafe { p.outline.rowForItem(Some(&p.obj(i))) };
        if row >= 0 {
            p.outline.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(row as usize),
                false,
            );
            p.outline.scrollRowToVisible(row);
        }
    }

    fn project_selected(&self, k: usize) -> Option<usize> {
        let p = self.project(k)?;
        let row = p.outline.selectedRow();
        (row >= 0).then(|| item_index(p.outline.itemAtRow(row).as_deref()))?
    }

    // ProjectPanel::setWorkSpaceDirty: the workspace icon shows the state.
    fn project_dirty(&self, k: usize, dirty: bool) {
        let Some(p) = self.project(k) else { return };
        p.dirty.set(dirty);
        unsafe { p.outline.reloadItem(Some(&p.obj(0))) };
    }

    fn project_set(&self, k: usize, tree: Tree, file: Option<PathBuf>) {
        let Some(p) = self.project(k) else { return };
        *p.tree.borrow_mut() = tree;
        *p.file.borrow_mut() = file;
        p.lost.borrow_mut().clear();
        p.dirty.set(false);
        unsafe { p.outline.collapseItem_collapseChildren(None, true) };
        p.outline.reloadData();
        self.project_expand(k, 0);
    }

    // ProjectPanel::newWorkSpace.
    fn project_new(&self, k: usize) {
        self.project_set(k, Tree::new(&root_name()), None);
    }

    // ProjectPanel::openWorkSpace with force: false when the file cannot be read or is not a workspace.
    fn project_load(&self, k: usize, file: &Path) -> bool {
        let Some(tree) = std::fs::read_to_string(file)
            .ok()
            .and_then(|x| parse_workspace(&x, file))
        else {
            return false;
        };
        self.project_set(k, tree, Some(file.to_path_buf()));
        true
    }

    // NppCommands.cpp IDM_VIEW_PROJECT_PANEL_1..3: the first launch opens the workspace of config.xml; close asks to save.
    pub(crate) fn project_toggle(&self, k: usize) {
        let id = PROJECTS[k];
        if self.panel_visible(id) {
            if self.project_check_save(k) {
                self.dock_close_panel(id);
            }
            return;
        }
        let first = self.project(k).is_none();
        self.dock_open_panel(id);
        if first {
            let file = read_config()
                .map(|x| parse_saved(&x)[k].0.clone())
                .unwrap_or_default();
            if file.is_empty() || !self.project_load(k, Path::new(&file)) {
                self.project_new(k);
                if let Some(p) = self.project(k) {
                    *p.lost.borrow_mut() = file;
                }
            }
        }
    }

    // Port only: at launch, the panels that were open at quit open again.
    pub(crate) fn projects_restore(&self) {
        let saved = read_config().map(|x| parse_saved(&x)).unwrap_or_default();
        for (k, (_, visible)) in saved.iter().enumerate() {
            if *visible {
                self.project_toggle(k);
            }
        }
    }

    // ProjectPanel::checkIfNeedSave.
    fn project_check_save(&self, k: usize) -> bool {
        if !self.project(k).is_some_and(|p| p.dirty.get()) {
            return true;
        }
        let r = self.alert(
            &self.project_title(k),
            "The workspace was modified. Do you want to save it?",
            &["Yes", "No", "Cancel"],
        );
        self.project_answer(k, r)
    }

    // Yes saves, No goes on without a save, Cancel stops.
    fn project_answer(&self, k: usize, r: isize) -> bool {
        if r == NSAlertFirstButtonReturn {
            self.project_save(k)
        } else {
            r == NSAlertSecondButtonReturn
        }
    }

    // Notepad_plus::saveProjectPanelsParams at quit: false when the user cancels.
    pub(crate) fn projects_quit(&self) -> bool {
        (0..3).all(|k| self.project(k).is_none() || self.project_check_save(k))
    }

    // ProjectPanel::saveWorkspaceRequest and the IDM_PROJECT_NEWWS question.
    fn project_save_request(&self, k: usize, title: &str) -> bool {
        if !self.project(k).is_some_and(|p| p.dirty.get()) {
            return true;
        }
        let r = self.alert(title, MODIFIED, &["Yes", "No", "Cancel"]);
        self.project_answer(k, r)
    }

    // ProjectPanel::saveWorkSpace.
    fn project_save(&self, k: usize) -> bool {
        let file = self.project(k).and_then(|p| p.file.borrow().clone());
        let Some(file) = file else {
            return self.project_save_as(k, false);
        };
        if !self.project_write(k, &file, true) {
            return false;
        }
        self.project_dirty(k, false);
        true
    }

    // ProjectPanel::writeWorkSpace: the file is written as a whole and then replaces the old file.
    fn project_write(&self, k: usize, file: &Path, update_gui: bool) -> bool {
        let Some(p) = self.project(k) else {
            return false;
        };
        let xml = workspace_xml(&p.tree.borrow(), file);
        if write_file(file, &xml, false).is_err() {
            self.alert(
                &self.project_title(k),
                "An error occurred while writing your workspace file.\nYour workspace has not been saved.",
                &["OK"],
            );
            return false;
        }
        if update_gui {
            if let Some(root) = p.tree.borrow_mut().nodes.get_mut(0) {
                root.name = file_name(file);
            }
            unsafe { p.outline.reloadItem(Some(&p.obj(0))) };
        }
        true
    }

    // ProjectPanel::saveWorkSpaceAs.
    fn project_save_as(&self, k: usize, copy: bool) -> bool {
        let s = NSSavePanel::savePanel(self.mtm());
        let dir = self
            .project(k)
            .and_then(|p| p.file.borrow().as_deref()?.parent().map(Path::to_path_buf));
        if let Some(dir) = dir {
            s.setDirectoryURL(Some(&NSURL::fileURLWithPath(&ns(&dir.to_string_lossy()))));
        }
        if s.runModal() != NSModalResponseOK {
            return false;
        }
        let Some(file) = s.URL().and_then(|u| u.path()) else {
            return false;
        };
        let file = PathBuf::from(file.to_string());
        if !self.project_write(k, &file, !copy) {
            return false;
        }
        if !copy {
            if let Some(p) = self.project(k) {
                *p.file.borrow_mut() = Some(file);
            }
            self.project_dirty(k, false);
        }
        true
    }

    fn project_ask(&self, title: &str, text: &str) -> Option<String> {
        let a = objc2_app_kit::NSAlert::new(self.mtm());
        a.setMessageText(&ns(title));
        let f = NSTextField::textFieldWithString(&ns(text), self.mtm());
        f.setFrame(frame(0., 0., 340., 24.));
        a.setAccessoryView(Some(&f));
        a.addButtonWithTitle(&ns("OK"));
        a.addButtonWithTitle(&ns("Cancel"));
        a.window().setInitialFirstResponder(Some(&f));
        (a.runModal() == NSAlertFirstButtonReturn).then(|| f.stringValue().to_string())
    }

    // TVN_ENDLABELEDIT: a file gets the new label in its path too; the workspace keeps its name.
    fn project_rename(&self, k: usize, i: usize) {
        let Some(p) = self.project(k) else { return };
        let Some(n) = p.node(i).filter(|n| n.kind != Kind::Root) else {
            return;
        };
        let Some(name) = self.project_ask("Rename", &n.name) else {
            return;
        };
        if let Some(m) = p.tree.borrow_mut().nodes.get_mut(i) {
            if m.kind == Kind::File {
                m.path = renamed_path(&m.path.to_string_lossy(), &n.name, &name).into();
            }
            m.name = name;
        }
        unsafe { p.outline.reloadItem(Some(&p.obj(i))) };
        self.project_dirty(k, true);
    }

    fn project_add(&self, k: usize, parent: usize, kind: Kind, name: &str) {
        let Some(p) = self.project(k) else { return };
        let i = p.tree.borrow_mut().add(parent, kind, name, PathBuf::new());
        self.project_dirty(k, true);
        self.project_reload(k);
        self.project_expand(k, parent);
        self.project_select(k, i);
        self.project_rename(k, i);
    }

    fn project_files_dialog(&self, k: usize, i: usize) {
        let o = NSOpenPanel::openPanel(self.mtm());
        o.setAllowsMultipleSelection(true);
        if o.runModal() != NSModalResponseOK {
            return;
        }
        let files: Vec<PathBuf> = o
            .URLs()
            .iter()
            .filter_map(|u| u.path())
            .map(|s| PathBuf::from(s.to_string()))
            .collect();
        let Some(p) = self.project(k).filter(|_| !files.is_empty()) else {
            return;
        };
        for f in files {
            p.tree.borrow_mut().add_file(i, f);
        }
        self.project_reload(k);
        self.project_expand(k, i);
        self.project_dirty(k, true);
    }

    // ProjectPanel::addFilesFromDirectory: the dialog starts in the last folder, or the folder of the workspace file.
    fn project_dir_dialog(&self, k: usize, i: usize) {
        let Some(p) = self.project(k) else { return };
        let start = p
            .last_dir
            .borrow()
            .clone()
            .or_else(|| p.file.borrow().as_deref()?.parent().map(Path::to_path_buf));
        let o = NSOpenPanel::openPanel(self.mtm());
        o.setCanChooseDirectories(true);
        o.setCanChooseFiles(false);
        if let Some(d) = start {
            o.setDirectoryURL(Some(&NSURL::fileURLWithPath(&ns(&d.to_string_lossy()))));
        }
        if o.runModal() != NSModalResponseOK {
            return;
        }
        let Some(dir) = o
            .URL()
            .and_then(|u| u.path())
            .map(|s| PathBuf::from(s.to_string()))
        else {
            return;
        };
        p.tree.borrow_mut().add_dir(i, &dir);
        *p.last_dir.borrow_mut() = Some(dir);
        self.project_reload(k);
        self.project_expand(k, i);
        self.project_dirty(k, true);
    }

    // ProjectPanel::popupMenuCmd.
    pub(crate) fn project_cmd(&self, k: usize, cmd: isize) {
        let Some(p) = self.project(k) else { return };
        match cmd {
            NEW_WS => {
                if self.project_save_request(k, "New Workspace") {
                    self.project_new(k);
                }
                return;
            }
            OPEN_WS => {
                if !self.project_save_request(k, "Open Workspace") {
                    return;
                }
                let o = NSOpenPanel::openPanel(self.mtm());
                if o.runModal() != NSModalResponseOK {
                    return;
                }
                let Some(f) = o.URL().and_then(|u| u.path()) else {
                    return;
                };
                if !self.project_load(k, Path::new(&f.to_string())) {
                    self.alert(
                        "Open Workspace",
                        "The workspace could not be opened.\nIt seems the file to open is not a valid project file.",
                        &["OK"],
                    );
                }
                return;
            }
            RELOAD_WS => {
                if p.dirty.get()
                    && self.alert(
                        "Reload Workspace",
                        "The current workspace was modified. Reloading will discard all modifications.\nDo you want to continue?",
                        &["Yes", "No"],
                    ) != NSAlertFirstButtonReturn
                {
                    return;
                }
                let file = p.file.borrow().clone().filter(|f| f.exists());
                match file {
                    Some(f) => {
                        self.project_load(k, &f);
                    }
                    None => {
                        self.alert(
                            "Reload Workspace",
                            "Cannot find the file to reload.",
                            &["OK"],
                        );
                    }
                }
                return;
            }
            SAVE_WS => {
                self.project_save(k);
                return;
            }
            SAVE_AS_WS | SAVE_COPY_AS_WS => {
                self.project_save_as(k, cmd == SAVE_COPY_AS_WS);
                return;
            }
            NEW_PROJECT => {
                let name = pm_name("NewProjectName", "Project Name");
                self.project_add(k, 0, Kind::Project, &name);
                return;
            }
            _ => {}
        }
        let Some(i) = self.project_selected(k) else {
            return;
        };
        let Some(n) = p.node(i) else { return };
        match cmd {
            RENAME => self.project_rename(k, i),
            NEW_FOLDER => {
                let name = pm_name("NewFolderName", "Folder Name");
                self.project_add(k, i, Kind::Folder, &name);
            }
            MOVE_UP | MOVE_DOWN => {
                if p.tree.borrow_mut().move_by(i, cmd == MOVE_UP) {
                    self.project_reload(k);
                    self.project_select(k, i);
                    self.project_dirty(k, true);
                }
            }
            ADD_FILES => self.project_files_dialog(k, i),
            ADD_FILES_RECURSIVELY => self.project_dir_dialog(k, i),
            DELETE_FOLDER => {
                if !n.kids.is_empty()
                    && self.alert(
                        "Remove folder from project",
                        "All the sub-items will be removed.\nAre you sure you want to remove this folder from the project?",
                        &["Yes", "No"],
                    ) != NSAlertFirstButtonReturn
                {
                    return;
                }
                p.tree.borrow_mut().remove(i);
                self.project_reload(k);
                self.project_dirty(k, true);
            }
            DELETE_FILE => {
                if self.alert(
                    "Remove file from project",
                    "Are you sure you want to remove this file from the project?",
                    &["Yes", "No"],
                ) == NSAlertFirstButtonReturn
                {
                    p.tree.borrow_mut().remove(i);
                    self.project_reload(k);
                    self.project_dirty(k, true);
                }
            }
            MODIFY_FILE_PATH => {
                let old = n.path.to_string_lossy().into_owned();
                let Some(new) = self.project_ask("Change file full path name", &old) else {
                    return;
                };
                if new == old {
                    return;
                }
                if let Some(m) = p.tree.borrow_mut().nodes.get_mut(i) {
                    m.path = PathBuf::from(&new);
                    m.name = file_name(&m.path);
                }
                unsafe { p.outline.reloadItem(Some(&p.obj(i))) };
                self.project_dirty(k, true);
            }
            _ => {}
        }
    }

    // ProjectPanel::openSelectFile on a double click; a double click on another node folds or unfolds it.
    fn project_open_item(&self, k: usize) {
        let Some(p) = self.project(k) else { return };
        let row = p.outline.clickedRow();
        let item = (row >= 0).then(|| p.outline.itemAtRow(row)).flatten();
        let Some(n) = p.node(item_index(item.as_deref()).unwrap_or(usize::MAX)) else {
            return;
        };
        if n.kind == Kind::File {
            if n.path.is_file() {
                self.open_path(&n.path);
            }
            unsafe { p.outline.reloadItem(item.as_deref()) };
        } else if unsafe { p.outline.isItemExpanded(item.as_deref()) } {
            unsafe { p.outline.collapseItem(item.as_deref()) };
        } else {
            unsafe { p.outline.expandItem(item.as_deref()) };
        }
    }

    // ProjectPanel::getMenuHandler: the menu of the clicked node for the context menu, of the selected node for the Edit button.
    fn project_menu(&self, k: usize, m: &NSMenu) {
        let Some(p) = self.project(k) else { return };
        let edit = std::ptr::eq(m, &*p.edit);
        m.removeAllItems();
        if edit {
            m.addItem(&NSMenuItem::new(self.mtm()));
            if let Some(first) = m.itemAtIndex(0) {
                first.setTitle(&ns(&entry(1)));
            }
        } else {
            let row = p.outline.clickedRow();
            if row < 0 {
                return;
            }
            p.outline.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(row as usize),
                false,
            );
        }
        let Some(kind) = self
            .project_selected(k)
            .and_then(|i| p.node(i))
            .map(|n| n.kind)
        else {
            return;
        };
        let (section, items): (&str, &[(isize, &str)]) = match kind {
            Kind::Root if edit => ("", &[]),
            Kind::Root => ("WorkspaceMenu", &WORKSPACE_MENU),
            Kind::Project => ("ProjectMenu", &FOLDER_MENU),
            Kind::Folder => ("FolderMenu", &FOLDER_MENU),
            Kind::File => ("FileMenu", &FILE_MENU),
        };
        let t: &AnyObject = &p.target;
        for &(tag, title) in items {
            m.addItem(&menu_item(self, t, section, tag, title));
        }
    }

    fn project_trees(&self, panels: [bool; 3]) -> Option<Vec<Tree>> {
        let trees: Vec<Tree> = (0..3)
            .filter(|&k| panels[k] && self.panel_visible(PROJECTS[k]))
            .filter_map(|k| Some(self.project(k)?.tree.borrow().clone()))
            .collect();
        (!trees.is_empty()).then_some(trees)
    }

    // Notepad_plus::findInProjects: the files of the checked open panels go to the Find in Files engine and its results panel.
    #[allow(dead_code)]
    pub(crate) fn find_in_projects(&self, panels: [bool; 3], filters: &str, opts: Opts) -> bool {
        if self.ivars().fif_running.get() || opts.find.is_empty() {
            return false;
        }
        let Some(trees) = self.project_trees(panels) else {
            return false;
        };
        let files = gather(&trees.iter().collect::<Vec<_>>(), filters);
        let args = FifArgs {
            dir: PathBuf::new(),
            filters: filters.to_string(),
            sub: false,
            hidden: false,
            opts,
            replace: false,
            open: self.open_texts(),
        };
        self.fif_ui().c.set_status("Find In Files progress...");
        self.ivars().fif_running.set(true);
        let app = self as *const Self as usize;
        search::spawn_search(
            args,
            move |a| search::find_in_list(a, &files),
            move |args, r| {
                if let Ok(mut done) = crate::FIF_DONE.lock() {
                    *done = Some((args, r));
                }
                let app = unsafe { &*(app as *const AnyObject) };
                let _: () = unsafe {
                    msg_send![app, performSelectorOnMainThread: sel!(fifDone:), withObject: None::<&AnyObject>, waitUntilDone: false]
                };
            },
        );
        true
    }
}

// NativeLangSpeaker::getAttrNameStr below ProjectManager.
fn pm_name(node: &str, default: &str) -> String {
    l10n::native_name(&["ProjectManager", node], None, default)
}

fn root_name() -> String {
    pm_name("WorkspaceRootName", "Workspace")
}

// Notepad_plus::launchProjectPanel: PanelTitle and the panel number.
pub fn panel_title(k: usize) -> String {
    format!("{} {}", pm_name("PanelTitle", "Project Panel"), k + 1)
}

// The Workspace (0) and Edit (1) buttons.
fn entry(id: usize) -> String {
    let default = ["Workspace", "Edit"][id.min(1)];
    l10n::native_name(&["ProjectManager", "Menus", "Entries"], Some(&id.to_string()), default)
}

// ProjectPanel::initMenus with getProjectPanelLangMenuStr.
fn menu_item(app: &App, t: &AnyObject, section: &str, tag: isize, title: &str) -> Retained<NSMenuItem> {
    if tag == 0 {
        return NSMenuItem::separatorItem(app.mtm());
    }
    let title = l10n::native_name(&["ProjectManager", "Menus", section], Some(&tag.to_string()), title);
    let title = title.split('\t').next().unwrap_or_default();
    let it = crate::item(app.mtm(), title, sel!(projectCmd:), "", Some(t));
    it.setTag(tag);
    it
}

// ProjectPanel::initMenus; Find in Projects... waits for the Find in Projects tab of the Find dialog.
const WORKSPACE_MENU: [(isize, &str); 8] = [
    (NEW_WS, "New Workspace"),
    (OPEN_WS, "Open Workspace"),
    (RELOAD_WS, "Reload Workspace"),
    (SAVE_WS, "Save"),
    (SAVE_AS_WS, "Save As..."),
    (SAVE_COPY_AS_WS, "Save a Copy As..."),
    (0, ""),
    (NEW_PROJECT, "Add New Project"),
];

const FOLDER_MENU: [(isize, &str); 8] = [
    (MOVE_UP, "Move Up"),
    (MOVE_DOWN, "Move Down"),
    (0, ""),
    (RENAME, "Rename"),
    (NEW_FOLDER, "Add Folder"),
    (ADD_FILES, "Add Files..."),
    (ADD_FILES_RECURSIVELY, "Add Files from Directory..."),
    (DELETE_FOLDER, "Remove"),
];

const FILE_MENU: [(isize, &str); 6] = [
    (MOVE_UP, "Move Up"),
    (MOVE_DOWN, "Move Down"),
    (0, ""),
    (RENAME, "Rename"),
    (DELETE_FILE, "Remove"),
    (MODIFY_FILE_PATH, "Modify File Path"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ws: &Path) -> Tree {
        let dir = ws.parent().unwrap();
        let mut t = Tree::new(&file_name(ws));
        let p = t.add(0, Kind::Project, "Démo & \"Ünï\"", PathBuf::new());
        let src = t.add(p, Kind::Folder, "src", PathBuf::new());
        let deep = t.add(src, Kind::Folder, "日本語 <deep>", PathBuf::new());
        t.add_file(deep, dir.join("src/深い/main.rs"));
        t.add_file(src, dir.join("src/lib.rs"));
        t.add(p, Kind::Folder, "empty", PathBuf::new());
        t.add_file(p, PathBuf::from("/other/place/notes.txt"));
        let q = t.add(0, Kind::Project, "Second", PathBuf::new());
        t.add_file(q, dir.join("README"));
        t
    }

    #[test]
    fn workspace_round_trip() {
        let ws = Path::new("/w/proj/my.workspace");
        let t = sample(ws);
        let xml = workspace_xml(&t, ws);
        assert!(xml.starts_with("<NotepadPlus>\n    <Project name=\"Démo &amp; &quot;Ünï&quot;\">\n        <Folder name=\"src\">\n"));
        assert!(xml.contains("<File name=\"src/深い/main.rs\" />"));
        assert!(xml.contains("<File name=\"/other/place/notes.txt\" />"));
        assert!(xml.contains("        <Folder name=\"empty\" />\n"));
        assert!(xml.ends_with("</NotepadPlus>\n"));
        assert_eq!(parse_workspace(&xml, ws), Some(t.clone()));
        let moved = Path::new("/elsewhere/copy.workspace");
        let copy = parse_workspace(&workspace_xml(&t, moved), moved).unwrap();
        assert_eq!(
            copy.files(&search::patterns("")),
            t.files(&search::patterns(""))
        );
    }

    #[test]
    fn invalid_workspaces() {
        let ws = Path::new("/w/a.workspace");
        assert_eq!(parse_workspace("<NotepadPlus />", ws), None);
        assert_eq!(
            parse_workspace("<Other><Project name=\"x\" /></Other>", ws),
            None
        );
        assert_eq!(
            parse_workspace("<NotepadPlus><Project name=\"x\">", ws),
            None
        );
        assert_eq!(parse_workspace("not xml <<", ws), None);
        let bom = "\u{feff}<NotepadPlus><Project name=\"x\" /></NotepadPlus>";
        assert_eq!(parse_workspace(bom, ws).map(|t| t.nodes.len()), Some(2));
        let t = parse_workspace(
            "<?xml version=\"1.0\"?>\r\n<NotepadPlus>\r\n<Project name=\"P\">\r\n<File name=\"..\\win.c\"/>\r\n<Unknown><File name=\"skip.c\"/></Unknown>\r\n<File name=\"../up.c\"/>\r\n</Project>\r\n<File name=\"top.c\"/>\r\n</NotepadPlus>",
            ws,
        )
        .unwrap();
        let names: Vec<&str> = t.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["a.workspace", "P", "..\\win.c", "up.c"]);
        assert_eq!(t.nodes[3].path, PathBuf::from("/up.c"));
    }

    #[test]
    fn relative_paths() {
        let ws = Path::new("/w/proj/my.workspace");
        assert_eq!(relative_path(Path::new("/w/proj/a.txt"), ws), "a.txt");
        assert_eq!(
            relative_path(Path::new("/w/proj/sub/b.txt"), ws),
            "sub/b.txt"
        );
        assert_eq!(
            relative_path(Path::new("/w/project/c.txt"), ws),
            "/w/project/c.txt"
        );
        assert_eq!(
            relative_path(Path::new("/w/other/d.txt"), ws),
            "/w/other/d.txt"
        );
        assert_eq!(
            absolute_path("sub/b.txt", ws),
            PathBuf::from("/w/proj/sub/b.txt")
        );
        assert_eq!(
            absolute_path("./x/../y.txt", ws),
            PathBuf::from("/w/proj/y.txt")
        );
        assert_eq!(absolute_path("../../../z.txt", ws), PathBuf::from("/z.txt"));
        assert_eq!(absolute_path("/abs/e.txt", ws), PathBuf::from("/abs/e.txt"));
        assert_eq!(absolute_path(r"C:\Users\a.txt", ws), PathBuf::from(r"C:\Users\a.txt"));
        assert_eq!(absolute_path("d:/x/b.txt", ws), PathBuf::from("d:/x/b.txt"));
        assert_eq!(absolute_path(r"\\server\share\c.txt", ws), PathBuf::from(r"\\server\share\c.txt"));
        assert_eq!(absolute_path("C:rel.txt", ws), PathBuf::from("/w/proj/C:rel.txt"));
        let win = Path::new(r"C:\Users\a.txt");
        let elsewhere = Path::new("/elsewhere/n.workspace");
        assert_eq!(relative_path(&absolute_path(r"C:\Users\a.txt", ws), elsewhere), r"C:\Users\a.txt");
        assert_eq!(relative_path(win, elsewhere), r"C:\Users\a.txt");
        assert_eq!(
            renamed_path("/a/old/old.txt", "old.txt", "new.md"),
            "/a/old/new.md"
        );
        assert_eq!(renamed_path("/a/b.txt", "zzz", "c"), "/a/b.txt");
    }

    #[test]
    fn tree_edits() {
        let mut t = Tree::new("w");
        let p = t.add(0, Kind::Project, "p", PathBuf::new());
        let a = t.add_file(p, "/a".into());
        let b = t.add_file(p, "/b".into());
        assert!(!t.move_by(a, true));
        assert!(t.move_by(a, false));
        assert_eq!(t.nodes[p].kids, [b, a]);
        assert!(!t.move_by(a, false));
        assert!(!t.move_by(0, true));
        t.remove(b);
        assert_eq!(t.nodes[p].kids, [a]);
        let pats = search::patterns("");
        assert_eq!(t.files(&pats), [PathBuf::from("/a")]);
    }

    #[test]
    fn gathers_files_of_the_panels() {
        let ws = Path::new("/w/proj/my.workspace");
        let t = sample(ws);
        let u = parse_workspace("<NotepadPlus><Project name=\"Q\"><File name=\"q.rs\"/><File name=\"q.txt\"/></Project></NotepadPlus>", Path::new("/q/q.workspace")).unwrap();
        let all = gather(&[&t, &u], "");
        let want: Vec<PathBuf> = [
            "/w/proj/src/深い/main.rs",
            "/w/proj/src/lib.rs",
            "/other/place/notes.txt",
            "/w/proj/README",
            "/q/q.rs",
            "/q/q.txt",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(all, want);
        let rs: Vec<PathBuf> = gather(&[&t, &u], "*.rs").into_iter().collect();
        assert_eq!(rs, [want[0].clone(), want[1].clone(), want[4].clone()]);
        assert_eq!(gather(&[&t, &u], "!*.rs !*.txt"), [want[3].clone()]);
    }

    #[test]
    fn adds_a_directory_and_searches_the_project_files() {
        let dir = std::env::temp_dir().join(format!("npp-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join("sub/one.txt"), "alpha beta\n").unwrap();
        std::fs::write(dir.join("two.txt"), "beta\n").unwrap();
        std::fs::write(dir.join(".hidden/three.txt"), "beta\n").unwrap();
        let mut t = Tree::new("w");
        let p = t.add(0, Kind::Project, "p", PathBuf::new());
        t.add_dir(p, &dir);
        t.add_file(p, dir.join("missing.txt"));
        let names: Vec<&str> = t.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            ["w", "p", "sub", "one.txt", "two.txt", "missing.txt"]
        );
        let files = gather(&[&t], "*.txt");
        let a = FifArgs {
            dir: PathBuf::new(),
            filters: "*.txt".into(),
            sub: false,
            hidden: false,
            opts: Opts {
                find: "beta".into(),
                ..Default::default()
            },
            replace: false,
            open: vec![(
                search::canonical(&dir.join("two.txt")),
                b"no match here\n".to_vec(),
            )],
        };
        let out = search::find_in_list(&a, &files).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(out.count, 1);
        let head = String::from_utf8_lossy(&out.lines[0].text).into_owned();
        assert!(
            head.starts_with("Search \"beta\" (1 hit in 1 file of 3 searched)"),
            "{head}"
        );
        assert!(String::from_utf8_lossy(&out.lines[1].text).contains("one.txt"));
    }

    #[test]
    fn panels_settings_round_trip() {
        let s: Saved = [
            ("/w/a.workspace".into(), true),
            (String::new(), false),
            ("/w/b & \"c\".workspace".into(), false),
        ];
        let config = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    <History nbMaxFile=\"10\" />\r\n    <ProjectPanels>\r\n        <ProjectPanel id=\"0\" workSpaceFile=\"/old\" />\r\n    </ProjectPanels>\r\n</NotepadPlus>\r\n";
        assert_eq!(parse_saved(config)[0], ("/old".to_string(), false));
        let out = replace_element(Some(config), "ProjectPanels", &saved_xml(&s)).unwrap();
        assert_eq!(parse_saved(&out), s);
        assert!(out.contains("<History nbMaxFile=\"10\" />"));
        assert!(!out.contains("/old"));
        assert_eq!(parse_saved("<NotepadPlus />"), Saved::default());
    }
}
