// SPDX-License-Identifier: GPL-3.0-or-later
use crate::panel::Form;
use crate::{cfg, item, lang, nested, ns, sci, tagged, App, Tab};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSControlTextEditingDelegate,
    NSMenu, NSMenuItem, NSScrollView, NSTabViewItem, NSTableColumn, NSTableView,
    NSTableViewDataSource, NSTableViewDelegate, NSWindowDelegate,
};
use objc2_foundation::{
    NSDate, NSDateFormatter, NSNotification, NSObject, NSObjectProtocol, NSString,
};
use std::cell::{Cell, OnceCell, RefCell};
use std::cmp::Ordering;
use std::time::UNIX_EPOCH;

// Notepad++ IDM_WINDOW_MRU_FIRST to IDM_WINDOW_MRU_LIMIT.
const MAX_LIST: usize = 40;
const COLUMNS: [(&str, f64); 5] = [
    ("Name", 130.),
    ("Path", 170.),
    ("Type", 60.),
    ("Size", 60.),
    ("Modified time", 130.),
];

// One open document as WindowsDlg sorts it.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub lang: String,
    pub len: usize,
    pub mtime: u128,
}

// Port of NumericStringEquivalence::numstrcmp: digit runs compare as numbers, ASCII letters without case.
pub fn num_cmp(a: &str, b: &str) -> Ordering {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (mut i, mut j) = (0, 0);
    let digits = |s: &[char], k: usize| s[k..].iter().take_while(|c| c.is_ascii_digit()).count();
    let number = |s: &[char]| {
        s.iter().fold(0u128, |n, c| {
            n.saturating_mul(10)
                .saturating_add(*c as u128 - '0' as u128)
        })
    };
    loop {
        match (a.get(i), b.get(j)) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (n, m) = (digits(&a, i), digits(&b, j));
                let o = number(&a[i..i + n])
                    .cmp(&number(&b[j..j + m]))
                    .then(m.cmp(&n));
                if o != Ordering::Equal {
                    return o;
                }
                i += n;
                j += m;
            }
            (Some(x), Some(y)) => {
                let o = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                i += 1;
                j += 1;
            }
        }
    }
}

// Port of BufferEquivalent::compare: column 0 name, 1 path, 2 type, 3 size, 4 modified time; the path breaks ties.
fn compare(a: &Entry, b: &Entry, col: usize) -> Ordering {
    let o = match col {
        0 => num_cmp(&a.name, &b.name),
        2 => num_cmp(&a.lang, &b.lang),
        3 => a.len.cmp(&b.len),
        4 => a.mtime.cmp(&b.mtime),
        _ => Ordering::Equal,
    };
    o.then_with(|| num_cmp(&a.path, &b.path))
}

// New order of the documents for a WindowsDlg sort; a reverse sort swaps the two sides of each comparison.
pub fn sort_order(e: &[Entry], col: usize, reverse: bool) -> Vec<usize> {
    let mut v: Vec<usize> = (0..e.len()).collect();
    v.sort_by(|&x, &y| {
        if reverse {
            compare(&e[y], &e[x], col)
        } else {
            compare(&e[x], &e[y], col)
        }
    });
    v
}

// Title of a document in the Window menu, as BuildMenuFileName makes it.
pub fn menu_title(pos: usize, name: &str, dirty: bool) -> String {
    format!("{}: {name}{}", pos + 1, if dirty { "*" } else { "" })
}

fn permute<T: Clone>(v: &mut Vec<T>, order: &[usize]) {
    *v = order.iter().map(|&k| v[k].clone()).collect();
}

#[derive(Default)]
pub struct ListIvars {
    items: RefCell<Vec<Retained<NSTabViewItem>>>,
    entries: RefCell<Vec<Entry>>,
    cells: RefCell<Vec<[String; 5]>>,
    sort: Cell<Option<(usize, bool)>>,
}

define_class!(
    // Rows of the Windows dialog in list order; a click on a column header sorts them.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ListIvars]
    pub struct WinList;

    unsafe impl NSObjectProtocol for WinList {}

    unsafe impl NSTableViewDataSource for WinList {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _t: &NSTableView) -> isize {
            self.ivars().cells.borrow().len() as isize
        }

        #[unsafe(method_id(tableView:objectValueForTableColumn:row:))]
        fn value(
            &self,
            _t: &NSTableView,
            col: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<AnyObject>> {
            let c = col.and_then(|c| c.identifier().to_string().parse::<usize>().ok());
            let s = c.and_then(|c| Some(self.ivars().cells.borrow().get(row as usize)?.get(c)?.clone()));
            s.map(|s| Retained::into_super(Retained::into_super(NSString::from_str(&s))))
        }
    }

    unsafe impl NSControlTextEditingDelegate for WinList {}

    unsafe impl NSTableViewDelegate for WinList {
        #[unsafe(method(tableView:didClickTableColumn:))]
        fn clicked(&self, t: &NSTableView, col: &NSTableColumn) {
            let Ok(c) = col.identifier().to_string().parse::<usize>() else {
                return;
            };
            let reverse = matches!(self.ivars().sort.get(), Some((p, false)) if p == c);
            self.sort_rows(t, c, reverse);
        }
    }

    unsafe impl NSWindowDelegate for WinList {
        #[unsafe(method(windowWillClose:))]
        fn will_close(&self, _n: &NSNotification) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
    }
);

impl WinList {
    fn sort_rows(&self, t: &NSTableView, c: usize, reverse: bool) {
        self.ivars().sort.set(Some((c, reverse)));
        let order = sort_order(&self.ivars().entries.borrow(), c, reverse);
        let i = self.ivars();
        permute(&mut i.items.borrow_mut(), &order);
        permute(&mut i.entries.borrow_mut(), &order);
        permute(&mut i.cells.borrow_mut(), &order);
        unsafe { t.deselectAll(None) };
        t.reloadData();
    }
}

struct WinDlg {
    form: Form,
    table: Retained<NSTableView>,
    list: Retained<WinList>,
}

thread_local! {
    static DLG: OnceCell<&'static WinDlg> = const { OnceCell::new() };
}

fn date(secs: f64) -> String {
    let f = NSDateFormatter::new();
    f.setDateFormat(Some(&ns("yyyy-MM-dd HH:mm:ss")));
    f.stringFromDate(&NSDate::dateWithTimeIntervalSince1970(secs))
        .to_string()
}

impl App {
    fn entry(&self, t: &Tab) -> (Entry, [String; 5]) {
        let lang = t
            .path
            .as_deref()
            .and_then(|p| lang::language_for_path(cfg(), p))
            .map_or("normal", |l| l.name.as_str());
        let mtime = t
            .path
            .as_deref()
            .and_then(|p| std::fs::metadata(p).ok()?.modified().ok())
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok());
        let len = sci::length(&t.view) as usize;
        let dir = t
            .path
            .as_deref()
            .and_then(|p| p.parent())
            .map_or(String::new(), |d| {
                format!("{}/", d.display()).replace("//", "/")
            });
        let mark = if self.dirty(t) {
            "*"
        } else if t.ro {
            " [Read Only]"
        } else {
            ""
        };
        let e = Entry {
            name: t.name.clone(),
            path: t
                .path
                .as_deref()
                .map_or(t.name.clone(), |p| p.display().to_string()),
            lang: lang.into(),
            len,
            mtime: mtime.map_or(0, |d| d.as_nanos()),
        };
        let cells = [
            format!("{}{mark}", t.name),
            dir,
            lang.into(),
            len.to_string(),
            mtime.map_or(String::new(), |d| date(d.as_secs_f64())),
        ];
        (e, cells)
    }

    // Moves the tabs to the given order of their current indices; the active tab stays active.
    fn reorder_tabs(&self, order: &[usize]) {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        let active = self
            .current()
            .and_then(|i| tabs.get(i))
            .map(|t| t.item.clone());
        for (to, &k) in order.iter().enumerate() {
            let from = self
                .ivars()
                .tabs
                .borrow()
                .iter()
                .position(|t| std::ptr::eq(&*t.item, &*tabs[k].item));
            if let Some(from) = from.filter(|&f| f != to) {
                self.move_tab_to(from, to);
            }
        }
        if let Some(a) = active {
            self.tab_view().selectTabViewItem(Some(&a));
        }
    }

    fn tab_entries(&self) -> Vec<Entry> {
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        tabs.iter().map(|t| self.entry(t).0).collect()
    }

    // NppCommands.cpp IDM_WINDOW_SORT_*: tag / 2 is the column and an odd tag sorts in reverse.
    pub(crate) fn sort_tabs(&self, tag: usize) {
        let order = sort_order(&self.tab_entries(), tag / 2, tag % 2 == 1);
        self.reorder_tabs(&order);
    }

    pub(crate) fn select_window(&self, i: usize) {
        if let Some(t) = self.tab(i) {
            self.tab_view().selectTabViewItem(Some(&t.item));
        }
    }

    // WindowsMenu::initPopupMenu: the open documents after the separator, with a check on the active one.
    pub(crate) fn update_window_menu(&self, m: &NSMenu) {
        for i in m.itemArray().iter() {
            if i.action() == Some(sel!(selectWindow:)) {
                m.removeItem(&i);
            }
        }
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        let t: &AnyObject = self;
        for (k, tab) in tabs.iter().take(MAX_LIST).enumerate() {
            let title = menu_title(k, &tab.name, self.dirty(tab));
            m.addItem(&tagged(
                self.mtm(),
                &title,
                sel!(selectWindow:),
                k as isize,
                Some(t),
            ));
        }
    }

    pub(crate) fn validate_window(&self, item: &NSMenuItem) -> Option<bool> {
        if item.action() != Some(sel!(selectWindow:)) {
            return None;
        }
        let on = self.current() == Some(item.tag() as usize);
        item.setState(if on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        Some(true)
    }

    fn win_dlg(&self) -> &'static WinDlg {
        DLG.with(|c| *c.get_or_init(|| Box::leak(Box::new(self.build_win_dlg()))))
    }

    fn build_win_dlg(&self) -> WinDlg {
        let mtm = self.mtm();
        let t: &AnyObject = self;
        let form = Form::new(mtm, "Windows", 720., 380.);
        let list: Retained<WinList> = {
            let l = WinList::alloc(mtm).set_ivars(ListIvars::default());
            unsafe { msg_send![super(l), init] }
        };
        let table = NSTableView::new(mtm);
        for (k, (title, w)) in COLUMNS.iter().enumerate() {
            let c =
                NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns(&k.to_string()));
            c.setTitle(&ns(title));
            c.setWidth(*w);
            table.addTableColumn(&c);
        }
        table.setAllowsMultipleSelection(true);
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*list)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*list)));
            table.setTarget(Some(t));
            table.setDoubleAction(Some(sel!(windowsActivate:)));
        }
        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setHasVerticalScroller(true);
        form.place(&scroll, 16., 16., 570., 348.);
        let (x, w) = (596., 110.);
        form.button("Activate", x, 14., w, t, sel!(windowsActivate:))
            .setKeyEquivalent(&ns("\r"));
        form.button("Save", x, 46., w, t, sel!(windowsSave:));
        form.button("Close window(s)", x, 78., w, t, sel!(windowsClose:));
        form.button("Sort tabs", x, 110., w, t, sel!(windowsSortTabs:));
        form.button("OK", x, 336., w, t, sel!(windowsOk:))
            .setKeyEquivalent(&ns("\u{1b}"));
        form.panel
            .setDelegate(Some(ProtocolObject::from_ref(&*list)));
        form.panel.setFloatingPanel(false);
        WinDlg { form, table, list }
    }

    // Keeps the list order of the documents that are still open and adds new ones at the end.
    fn refresh_windows(&self) {
        let d = self.win_dlg();
        let tabs: Vec<Tab> = self.ivars().tabs.borrow().clone();
        let old = d.list.ivars().items.borrow().clone();
        let mut order: Vec<&Tab> = old
            .iter()
            .filter_map(|i| tabs.iter().find(|t| std::ptr::eq(&*t.item, &**i)))
            .collect();
        order.extend(
            tabs.iter()
                .filter(|t| !old.iter().any(|i| std::ptr::eq(&**i, &*t.item))),
        );
        let rows: Vec<_> = order.iter().map(|t| self.entry(t)).collect();
        let l = d.list.ivars();
        *l.items.borrow_mut() = order.iter().map(|t| t.item.clone()).collect();
        *l.entries.borrow_mut() = rows.iter().map(|r| r.0.clone()).collect();
        *l.cells.borrow_mut() = rows.into_iter().map(|r| r.1).collect();
        d.table.reloadData();
    }

    fn selected_windows(&self) -> Vec<Retained<NSTabViewItem>> {
        let d = self.win_dlg();
        let rows = d.table.selectedRowIndexes();
        let items = d.list.ivars().items.borrow();
        let mut out = vec![];
        let mut i = rows.firstIndex();
        while let Some(it) = items.get(i) {
            out.push(it.clone());
            i = rows.indexGreaterThanIndex(i);
        }
        out
    }

    fn index_of_item(&self, item: &NSTabViewItem) -> Option<usize> {
        self.ivars()
            .tabs
            .borrow()
            .iter()
            .position(|t| std::ptr::eq(&*t.item, item))
    }

    pub(crate) fn show_windows(&self) {
        let d = self.win_dlg();
        d.list.ivars().items.borrow_mut().clear();
        d.list.ivars().sort.set(None);
        self.refresh_windows();
        if let Some(i) = self.current() {
            let set = objc2_foundation::NSIndexSet::indexSetWithIndex(i);
            d.table.selectRowIndexes_byExtendingSelection(&set, false);
        }
        d.form.panel.center();
        NSApplication::sharedApplication(self.mtm()).runModalForWindow(&d.form.panel);
        d.form.panel.orderOut(None);
        self.focus();
    }

    pub(crate) fn windows_cmd(&self, cmd: &str) {
        let app = NSApplication::sharedApplication(self.mtm());
        match cmd {
            "activate" => {
                if let Some(i) = self
                    .selected_windows()
                    .first()
                    .and_then(|i| self.index_of_item(i))
                {
                    self.select_window(i);
                }
                app.stopModal();
            }
            "save" => {
                for item in self.selected_windows() {
                    if let Some(i) = self.index_of_item(&item) {
                        self.save(i, false);
                    }
                }
                self.refresh_windows();
            }
            "close" => {
                for item in self.selected_windows() {
                    let Some(i) = self.index_of_item(&item) else {
                        continue;
                    };
                    self.tab_view().selectTabViewItem(Some(&item));
                    // NppBigSwitch.cpp WDT_CLOSE: Cancel keeps this document and goes on with the next one.
                    if self.confirm_close(i) {
                        self.drop_tabs(&[item]);
                    }
                }
                self.refresh_windows();
            }
            "sort" => {
                let d = self.win_dlg();
                if d.list.ivars().sort.get().is_none() {
                    d.list.sort_rows(&d.table, 0, false);
                }
                let items = d.list.ivars().items.borrow().clone();
                let order: Vec<usize> =
                    items.iter().filter_map(|i| self.index_of_item(i)).collect();
                self.reorder_tabs(&order);
                self.refresh_windows();
            }
            _ => app.stopModal(),
        }
    }
}

// The Window menu; the App fills the document list each time the menu opens.
pub fn window_menu(mtm: MainThreadMarker, app: &App) -> Retained<NSMenuItem> {
    let t: Option<&AnyObject> = Some(app);
    let sorts = [
        "Name A to Z",
        "Name Z to A",
        "Path A to Z",
        "Path Z to A",
        "Type A to Z",
        "Type Z to A",
        "Content Length Ascending",
        "Content Length Descending",
        "Modified Time Ascending",
        "Modified Time Descending",
    ];
    let top = nested(
        mtm,
        "Window",
        vec![
            nested(
                mtm,
                "Sort By",
                sorts
                    .iter()
                    .enumerate()
                    .map(|(k, s)| tagged(mtm, s, sel!(sortTabs:), k as isize, t))
                    .collect(),
            ),
            item(mtm, "Windows...", sel!(showWindows:), "", t),
            NSMenuItem::separatorItem(mtm),
        ],
    );
    if let Some(m) = top.submenu() {
        m.setDelegate(Some(ProtocolObject::from_ref(app)));
    }
    top
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, path: &str, lang: &str, len: usize, mtime: u128) -> Entry {
        Entry {
            name: name.into(),
            path: path.into(),
            lang: lang.into(),
            len,
            mtime,
        }
    }

    #[test]
    fn numeric_string_order() {
        assert_eq!(num_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(num_cmp("File", "file"), Ordering::Equal);
        assert_eq!(num_cmp("a", "ab"), Ordering::Less);
        assert_eq!(num_cmp("x01", "x1"), Ordering::Less);
        assert_eq!(num_cmp("b", "A"), Ordering::Greater);
        assert_eq!(num_cmp("new 9", "new 10"), Ordering::Less);
    }

    #[test]
    fn tab_sort_orders() {
        let v = [
            e("b.txt", "/z/b.txt", "normal", 30, 5),
            e("a10.py", "/a/a10.py", "python", 10, 9),
            e("a2.c", "/m/a2.c", "c", 20, 0),
            e("b.txt", "/a/b.txt", "normal", 10, 7),
        ];
        assert_eq!(sort_order(&v, 0, false), [2, 1, 3, 0]);
        assert_eq!(sort_order(&v, 0, true), [0, 3, 1, 2]);
        assert_eq!(sort_order(&v, 1, false), [1, 3, 2, 0]);
        assert_eq!(sort_order(&v, 1, true), [0, 2, 3, 1]);
        assert_eq!(sort_order(&v, 2, false), [2, 3, 0, 1]);
        assert_eq!(sort_order(&v, 3, false), [1, 3, 2, 0]);
        assert_eq!(sort_order(&v, 3, true), [0, 2, 3, 1]);
        assert_eq!(sort_order(&v, 4, false), [2, 0, 3, 1]);
        assert_eq!(sort_order(&v, 4, true), [1, 3, 0, 2]);
    }

    #[test]
    fn window_menu_titles() {
        assert_eq!(menu_title(0, "a.txt", false), "1: a.txt");
        assert_eq!(menu_title(9, "new 3", true), "10: new 3*");
    }
}
