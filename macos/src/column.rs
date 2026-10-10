// SPDX-License-Identifier: GPL-3.0-or-later
use crate::panel::{self, Form};
use crate::{item, nested, ns, sci, tagged, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBeep, NSButton, NSEventModifierFlags, NSMenuItem, NSPopUpButton, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::{Cell, OnceCell, RefCell};

const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GETLENGTH: u32 = 2006;
const SCI_BEGINUNDOACTION: u32 = 2078;
const SCI_ENDUNDOACTION: u32 = 2079;
const SCI_GETCOLUMN: u32 = 2129;
const SCI_GETLINEENDPOSITION: u32 = 2136;
const SCI_SETSELECTIONSTART: u32 = 2142;
const SCI_GETSELECTIONSTART: u32 = 2143;
const SCI_SETSELECTIONEND: u32 = 2144;
const SCI_GETSELECTIONEND: u32 = 2145;
const SCI_REPLACETARGET: u32 = 2194;
const SCI_SETSEARCHFLAGS: u32 = 2198;
const SCI_WORDSTARTPOSITION: u32 = 2266;
const SCI_WORDENDPOSITION: u32 = 2267;
const SCI_SELECTIONISRECTANGLE: u32 = 2372;
const SCI_FINDCOLUMN: u32 = 2456;
const SCI_GETSELECTIONS: u32 = 2570;
const SCI_GETSELECTIONNCARET: u32 = 2577;
const SCI_GETSELECTIONNANCHOR: u32 = 2579;
const SCI_GETSELECTIONNCARETVIRTUALSPACE: u32 = 2581;
const SCI_GETSELECTIONNANCHORVIRTUALSPACE: u32 = 2583;
const SCI_SETSELECTIONNSTART: u32 = 2584;
const SCI_SETSELECTIONNEND: u32 = 2586;
const SCI_DROPSELECTIONN: u32 = 2671;
const SCI_SETTARGETRANGE: u32 = 2686;
const SCI_MULTIPLESELECTADDNEXT: u32 = 2688;
const SCI_MULTIPLESELECTADDEACH: u32 = 2689;
const SCI_TARGETWHOLEDOCUMENT: u32 = 2690;
const SCFIND_WHOLEWORD: usize = 2;
const SCFIND_MATCHCASE: usize = 4;

// Multi-select submenu items and their Notepad++ search flags.
const MATCH: [(&str, usize); 4] = [
    ("Ignore Case & Whole Word", 0),
    ("Match Case Only", SCFIND_MATCHCASE),
    ("Match Whole Word Only", SCFIND_WHOLEWORD),
    (
        "Match Case & Whole Word",
        SCFIND_MATCHCASE | SCFIND_WHOLEWORD,
    ),
];
const NEXT: isize = 4;
const UNDO: isize = 8;
const SKIP: isize = 9;
// Format radio buttons in the Notepad++ dialog order.
const BASES: [(&str, u32); 4] = [("Dec", 10), ("Hex", 16), ("Oct", 8), ("Bin", 2)];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lead {
    None,
    Zeros,
    Spaces,
}

// Port of getNbDigits.
pub fn digits(mut n: u64, base: u32) -> usize {
    let mut d = 0;
    loop {
        d += 1;
        n /= base as u64;
        if n == 0 {
            return d;
        }
    }
}

// Port of variedFormatNumber2String: None aligns left, Zeros and Spaces align right.
pub fn format_number(n: u64, base: u32, upper: bool, width: usize, lead: Lead) -> String {
    let s = match base {
        2 => format!("{n:b}"),
        8 => format!("{n:o}"),
        16 if upper => format!("{n:X}"),
        16 => format!("{n:x}"),
        _ => n.to_string(),
    };
    match lead {
        Lead::None => format!("{s:<width$}"),
        Lead::Zeros => format!("{s:0>width$}"),
        Lead::Spaces => format!("{s:>width$}"),
    }
}

// Numbers as ColumnEditorDlg makes them: C++ size_t arithmetic, so a negative value wraps to a large one.
pub fn numbers(
    initial: i32,
    incr: i32,
    repeat: i32,
    count: usize,
    base: u32,
    upper: bool,
    lead: Lead,
) -> Vec<String> {
    let (init, inc) = (initial as i64 as u64, incr as i64 as u64);
    let rep = if repeat == 0 { 1 } else { repeat as i64 as u64 };
    let vals: Vec<u64> = (0..count as u64)
        .map(|k| init.wrapping_add(inc.wrapping_mul(k / rep)))
        .collect();
    let width = digits(init, base).max(vals.last().map_or(1, |&v| digits(v, base)));
    vals.iter()
        .map(|&v| format_number(v, base, upper, width, lead))
        .collect()
}

// Port of getNumericFieldValueFromText (std::stoi); -1 is the Notepad++ error value, so "-1" is also an error.
pub fn parse_field(s: &str, base: u32) -> Option<i32> {
    if s.is_empty() {
        return Some(0);
    }
    let t = s.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let hex_prefix = base == 16
        && (t.starts_with("0x") || t.starts_with("0X"))
        && t[2..].starts_with(|c: char| c.is_ascii_hexdigit());
    let t = if hex_prefix { &t[2..] } else { t };
    if t.is_empty() || !t.chars().all(|c| c.is_digit(base)) {
        return None;
    }
    let v = i64::from_str_radix(t, base).ok()?;
    let v = i32::try_from(if neg { -v } else { v }).ok()?;
    (v != -1).then_some(v)
}

// Field text after a base change, as ColumnEditorDlg::setNumericFields: SetDlgItemInt (unsigned) for Dec.
pub fn field_text(n: i32, base: u32, upper: bool) -> String {
    if base == 10 {
        return (n as u32).to_string();
    }
    let n = n as i64 as u64;
    format_number(n, base, upper, digits(n, base), Lead::None)
}

// Text of the Notepad++ "Invalid Numeric Entry" balloon.
pub fn invalid_message(s: &str, base: u32) -> String {
    let note = match base {
        16 => "Hex numbers use 0-9, A-F!",
        8 => "Oct numbers only use 0-7!",
        2 => "Bin numbers only use 0-1!",
        _ => "Decimal numbers only use 0-9!",
    };
    format!("Entered string \"{s}\":\n{note}")
}

#[derive(Clone, Copy, Debug)]
pub struct Sel {
    pub anchor: isize,
    pub caret: isize,
    pub anchor_vs: isize,
    pub caret_vs: isize,
}

pub struct Plan {
    // (start, end, text) from the last position to the first.
    pub edits: Vec<(isize, isize, Vec<u8>)>,
    // New (start, end) of each selection in the original order, for SCI_SETSELECTIONNSTART and SCI_SETSELECTIONNEND.
    pub ranges: Vec<(isize, isize)>,
}

// Port of ScintillaEditView::columnReplace: texts[k] replaces the k-th selection in position order; virtual space becomes spaces.
pub fn plan(sels: &[Sel], rect: bool, texts: &[Vec<u8>]) -> Plan {
    let mut order: Vec<usize> = (0..sels.len()).collect();
    order.sort_by_key(|&i| sels[i].anchor.min(sels[i].caret));
    let mut edits = vec![];
    let mut ranges = vec![(0, 0); sels.len()];
    let mut shift = 0;
    for (k, &i) in order.iter().enumerate() {
        let s = sels[i];
        let (l, r) = (s.anchor.min(s.caret), s.anchor.max(s.caret));
        let l2r = if s.anchor == s.caret && rect {
            s.anchor_vs < s.caret_vs
        } else {
            s.anchor <= s.caret
        };
        let pad = s.anchor_vs.min(s.caret_vs).max(0) as usize;
        let mut t = vec![b' '; pad];
        t.extend_from_slice(&texts[k]);
        let start = l + shift + pad as isize;
        let end = start + texts[k].len() as isize;
        ranges[i] = if l2r { (start, end) } else { (end, start) };
        shift += t.len() as isize - (r - l);
        edits.push((l, r, t));
    }
    edits.reverse();
    Plan { edits, ranges }
}

// Port of the ColumnEditorDlg no-selection case for one line: pads a short line with spaces up to the column.
pub fn insert_in_line(line: &[u8], end_col: isize, col: isize, at: usize, text: &[u8]) -> Vec<u8> {
    let mut out = line.to_vec();
    if end_col < col {
        out.extend(std::iter::repeat_n(b' ', (col - end_col) as usize));
        out.extend_from_slice(text);
    } else {
        let at = at.min(out.len());
        out.splice(at..at, text.iter().copied());
    }
    out
}

fn s(v: &NSView, m: u32, w: isize, l: isize) -> isize {
    sci::send(v, m, w as usize, l)
}

// Selects the word at the caret, as ScintillaEditView::expandWordSelection.
fn expand_word(v: &NSView) {
    let caret = s(v, SCI_GETCURRENTPOS, 0, 0);
    let (ws, we) = (
        s(v, SCI_WORDSTARTPOSITION, caret, 1),
        s(v, SCI_WORDENDPOSITION, caret, 1),
    );
    if ws != we {
        s(v, SCI_SETSELECTIONSTART, ws, 0);
        s(v, SCI_SETSELECTIONEND, we, 0);
    }
}

fn add_next(v: &NSView, flags: usize) {
    s(v, SCI_TARGETWHOLEDOCUMENT, 0, 0);
    s(v, SCI_SETSEARCHFLAGS, flags as isize, 0);
    s(v, SCI_MULTIPLESELECTADDNEXT, 0, 0);
}

// Port of NppCommands.cpp IDM_EDIT_MULTISELECT*; `last` keeps the flags of the last command for Skip.
pub fn multi_select(v: &NSView, tag: isize, last: &Cell<usize>) {
    match tag {
        0..4 => {
            last.set(MATCH[tag as usize].1);
            if s(v, SCI_GETSELECTIONSTART, 0, 0) == s(v, SCI_GETSELECTIONEND, 0, 0) {
                expand_word(v);
            }
            s(v, SCI_TARGETWHOLEDOCUMENT, 0, 0);
            s(v, SCI_SETSEARCHFLAGS, last.get() as isize, 0);
            s(v, SCI_MULTIPLESELECTADDEACH, 0, 0);
        }
        NEXT..UNDO => {
            last.set(MATCH[(tag - NEXT) as usize].1);
            add_next(v, last.get());
        }
        UNDO => {
            let n = s(v, SCI_GETSELECTIONS, 0, 0);
            if n > 0 {
                s(v, SCI_DROPSELECTIONN, n - 1, 0);
            }
        }
        SKIP => {
            add_next(v, last.get());
            let n = s(v, SCI_GETSELECTIONS, 0, 0);
            if n > 1 {
                s(v, SCI_DROPSELECTIONN, n - 2, 0);
            }
        }
        _ => {}
    }
}

// Inserts make(n) on n targets as one undo step: each selection of a rectangular or multiple selection, else each line from the caret line to the end.
pub fn insert(v: &NSView, make: impl FnOnce(usize) -> Vec<Vec<u8>>) {
    let n = s(v, SCI_GETSELECTIONS, 0, 0);
    let rect = s(v, SCI_SELECTIONISRECTANGLE, 0, 0) != 0;
    s(v, SCI_BEGINUNDOACTION, 0, 0);
    if rect || n > 1 {
        if n > 1 {
            let sels: Vec<Sel> = (0..n)
                .map(|i| Sel {
                    anchor: s(v, SCI_GETSELECTIONNANCHOR, i, 0),
                    caret: s(v, SCI_GETSELECTIONNCARET, i, 0),
                    anchor_vs: s(v, SCI_GETSELECTIONNANCHORVIRTUALSPACE, i, 0),
                    caret_vs: s(v, SCI_GETSELECTIONNCARETVIRTUALSPACE, i, 0),
                })
                .collect();
            let p = plan(&sels, rect, &make(n as usize));
            for (a, b, t) in &p.edits {
                s(v, SCI_SETTARGETRANGE, *a, *b);
                s(v, SCI_REPLACETARGET, t.len() as isize, t.as_ptr() as isize);
            }
            for (i, (a, b)) in p.ranges.iter().enumerate() {
                s(v, SCI_SETSELECTIONNSTART, i as isize, *a);
                s(v, SCI_SETSELECTIONNEND, i as isize, *b);
            }
        }
    } else {
        let pos = s(v, SCI_GETCURRENTPOS, 0, 0);
        let col = s(v, SCI_GETCOLUMN, pos, 0);
        let first = s(v, sci::SCI_LINEFROMPOSITION, pos, 0);
        let last = s(v, sci::SCI_LINEFROMPOSITION, s(v, SCI_GETLENGTH, 0, 0), 0);
        let texts = make((last - first + 1) as usize);
        for (line, text) in (first..=last).zip(&texts) {
            let begin = s(v, sci::SCI_POSITIONFROMLINE, line, 0);
            let end = s(v, SCI_GETLINEENDPOSITION, line, 0);
            let at = s(v, SCI_FINDCOLUMN, line, col) - begin;
            let old = sci::doc(v).range(begin, end);
            let new = insert_in_line(
                &old,
                s(v, SCI_GETCOLUMN, end, 0),
                col,
                at.max(0) as usize,
                text,
            );
            s(v, SCI_SETTARGETRANGE, begin, end);
            s(
                v,
                SCI_REPLACETARGET,
                new.len() as isize,
                new.as_ptr() as isize,
            );
        }
    }
    s(v, SCI_ENDUNDOACTION, 0, 0);
}

pub fn insert_text(v: &NSView, text: &str) {
    insert(v, |n| vec![text.as_bytes().to_vec(); n]);
}

pub fn multi_select_menu(
    mtm: MainThreadMarker,
    t: Option<&AnyObject>,
    next: bool,
) -> Retained<NSMenuItem> {
    let base = if next { NEXT } else { 0 };
    let items = MATCH
        .iter()
        .enumerate()
        .map(|(k, (n, _))| tagged(mtm, n, sel!(multiSelect:), base + k as isize, t))
        .collect();
    nested(
        mtm,
        if next {
            "Multi-select Next"
        } else {
            "Multi-select All"
        },
        items,
    )
}

pub fn undo_item(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    tagged(
        mtm,
        "Undo the Latest Added Multi-Select",
        sel!(multiSelect:),
        UNDO,
        t,
    )
}

pub fn skip_item(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    tagged(
        mtm,
        "Skip Current & Go to Next Multi-select",
        sel!(multiSelect:),
        SKIP,
        t,
    )
}

pub fn column_mode_item(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    item(mtm, "Column Mode...", sel!(columnModeTip:), "", t)
}

// Alt+C in Notepad++; Option+C types a character on macOS, so the key is Cmd+Opt+C.
pub fn column_editor_item(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    let i = item(mtm, "Column Editor...", sel!(columnEditor:), "c", t);
    i.setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Option);
    i
}

const TIP: &str = "There are 3 ways to switch to column-select mode:\n\n\
1. (Keyboard and Mouse)  Hold Option while left-click dragging\n\n\
2. (Keyboard only)  Hold Option+Shift while using arrow keys\n\n\
3. (Keyboard or Mouse)\n\
      Put caret at desired start of column block position, then\n\
       execute \"Begin/End Select in Column Mode\" command;\n\
      Move caret to desired end of column block position, then\n\
       execute \"Begin/End Select in Column Mode\" command again\n";

struct ColUi {
    form: Form,
    text_radio: Retained<NSButton>,
    text: Retained<NSTextField>,
    bases: Vec<Retained<NSButton>>,
    hex_case: Retained<NSPopUpButton>,
    fields: Vec<Retained<NSTextField>>,
    lead: Retained<NSPopUpButton>,
    shown: Cell<(u32, bool)>,
    vals: RefCell<[(Option<i32>, String); 3]>,
}

thread_local! {
    static UI: OnceCell<ColUi> = const { OnceCell::new() };
    static LAST_FLAGS: Cell<usize> = const { Cell::new(0) };
}

fn radio(
    f: &Form,
    title: &str,
    x: f64,
    top: f64,
    w: f64,
    t: &AnyObject,
    action: objc2::runtime::Sel,
) -> Retained<NSButton> {
    let b = unsafe {
        NSButton::radioButtonWithTitle_target_action(
            &ns(title),
            Some(t),
            Some(action),
            f.panel.mtm(),
        )
    };
    f.place(&b, x, top, w, 20.);
    b
}

fn popup(
    f: &Form,
    items: &[&str],
    x: f64,
    top: f64,
    w: f64,
    t: Option<&AnyObject>,
) -> Retained<NSPopUpButton> {
    let p = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(f.panel.mtm()),
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(w, 26.)),
        false,
    );
    items.iter().for_each(|i| p.addItemWithTitle(&ns(i)));
    if t.is_some() {
        unsafe {
            p.setTarget(t);
            p.setAction(Some(sel!(columnFormat:)));
        }
    }
    f.place(&p, x, top, w, 26.);
    p
}

// Dialog of ColumnEditorDlg with the Notepad++ defaults: Number to Insert, empty fields, Dec, a-f, Leading None.
fn col_ui(app: &App) -> ColUi {
    let t: &AnyObject = app;
    let f = Form::new(app.mtm(), "Column / Multi-Selection Editor", 380., 330.);
    let text_radio = radio(&f, "Text to Insert", 16., 12., 200., t, sel!(columnChoice:));
    let text = f.field(36., 38., 200.);
    let num_radio = radio(
        &f,
        "Number to Insert",
        16.,
        76.,
        200.,
        t,
        sel!(columnChoice:),
    );
    f.label("Format", 36., 100., 100.);
    let bases = BASES
        .iter()
        .enumerate()
        .map(|(k, (n, _))| {
            radio(
                &f,
                n,
                48. + (k % 2) as f64 * 100.,
                126. + (k / 2) as f64 * 24.,
                90.,
                t,
                sel!(columnFormat:),
            )
        })
        .collect::<Vec<_>>();
    let hex_case = popup(&f, &["a-f", "A-F"], 230., 122., 70., Some(t));
    let mut fields = vec![];
    for (k, l) in ["Initial number:", "Increase by:", "Repeat:"]
        .iter()
        .enumerate()
    {
        let top = 184. + k as f64 * 30.;
        f.label(l, 36., top, 110.);
        fields.push(f.field(150., top, 90.));
    }
    f.label("Leading:", 36., 274., 110.);
    let lead = popup(&f, &["None", "Zeros", "Spaces"], 150., 272., 110., None);
    f.button("OK", 270., 10., 96., t, sel!(columnOk:))
        .setKeyEquivalent(&ns("\r"));
    f.button("Cancel", 270., 42., 96., t, sel!(closePanel:))
        .setKeyEquivalent(&ns("\u{1b}"));
    panel::set_on(&num_radio, true);
    panel::set_on(&bases[0], true);
    ColUi {
        form: f,
        text_radio,
        text,
        bases,
        hex_case,
        fields,
        lead,
        shown: Cell::new((10, false)),
        vals: RefCell::default(),
    }
}

fn with_ui<R>(app: &App, f: impl FnOnce(&ColUi) -> R) -> R {
    UI.with(|c| f(c.get_or_init(|| col_ui(app))))
}

impl ColUi {
    fn format(&self) -> (u32, bool) {
        let k = self.bases.iter().position(|b| panel::on(b)).unwrap_or(0);
        (
            BASES[k].1,
            BASES[k].1 == 16 && self.hex_case.indexOfSelectedItem() == 1,
        )
    }

    fn lead(&self) -> Lead {
        match self.lead.indexOfSelectedItem() {
            1 => Lead::Zeros,
            2 => Lead::Spaces,
            _ => Lead::None,
        }
    }

    // Port of ColumnEditorDlg::switchTo.
    fn switch(&self) {
        let text = panel::on(&self.text_radio);
        self.text.setEnabled(text);
        for c in self.fields.iter() {
            c.setEnabled(!text);
        }
        self.bases.iter().for_each(|b| b.setEnabled(!text));
        self.lead.setEnabled(!text);
        self.hex_case.setEnabled(!text && self.format().0 == 16);
        let focus: &NSView = if text { &self.text } else { &self.fields[0] };
        self.form.panel.makeFirstResponder(Some(focus));
    }

    // Writes the field values again in the new base, as ColumnEditorDlg::setNumericFields.
    fn reformat(&self) {
        let (old, new) = (self.shown.get(), self.format());
        for (f, (val, wrote)) in self.fields.iter().zip(self.vals.borrow_mut().iter_mut()) {
            let s = panel::text(f);
            if s != *wrote {
                *val = if s.is_empty() { None } else { parse_field(&s, old.0).or(*val) };
            }
            *wrote = val.map_or(String::new(), |n| field_text(n, new.0, new.1));
            f.setStringValue(&ns(wrote));
        }
        self.shown.set(new);
        self.hex_case.setEnabled(new.0 == 16);
    }
}

impl App {
    pub(crate) fn multi_select(&self, tag: isize) {
        if let Some(v) = self.editor() {
            LAST_FLAGS.with(|l| multi_select(&v, tag, l));
        }
    }

    pub(crate) fn column_mode_tip(&self) {
        self.alert("Column Mode Tip", TIP, &["OK"]);
    }

    pub(crate) fn show_column_editor(&self) {
        with_ui(self, |u| {
            u.form.panel.makeKeyAndOrderFront(None);
            u.switch();
        });
    }

    pub(crate) fn column_choice(&self) {
        with_ui(self, |u| u.switch());
    }

    pub(crate) fn column_format(&self) {
        with_ui(self, |u| u.reformat());
    }

    // Port of ColumnEditorDlg IDOK; a read-only document does not change.
    pub(crate) fn column_ok(&self) {
        let Some(v) = self.editor() else { return };
        if sci::read_only(&v) {
            NSBeep();
            return;
        }
        let done = with_ui(self, |u| {
            if panel::on(&u.text_radio) {
                let text = panel::text(&u.text);
                if text.is_empty() {
                    NSBeep();
                    return false;
                }
                u.form.panel.orderOut(None);
                insert_text(&v, &text);
                return true;
            }
            let (base, upper) = u.format();
            let mut vals = [0; 3];
            for (k, f) in u.fields.iter().enumerate() {
                let s = panel::text(f);
                match parse_field(&s, base) {
                    Some(n) => vals[k] = n,
                    None => {
                        self.alert("Invalid Numeric Entry", &invalid_message(&s, base), &["OK"]);
                        u.form.panel.makeFirstResponder(Some(&**f));
                        return false;
                    }
                }
            }
            let lead = u.lead();
            u.form.panel.orderOut(None);
            insert(&v, |n| {
                numbers(vals[0], vals[1], vals[2], n, base, upper, lead)
                    .into_iter()
                    .map(String::into_bytes)
                    .collect()
            });
            true
        });
        if done {
            self.front_editor();
        }
    }

    fn front_editor(&self) {
        use objc2::DefinedClass;
        if let Some(w) = self.ivars().window.get() {
            w.makeKeyAndOrderFront(None);
        }
        self.focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nums(i: i32, inc: i32, rep: i32, n: usize, base: u32, lead: Lead) -> Vec<String> {
        numbers(i, inc, rep, n, base, false, lead)
    }

    #[test]
    fn digits_count_like_get_nb_digits() {
        assert_eq!(digits(0, 10), 1);
        assert_eq!(digits(9, 10), 1);
        assert_eq!(digits(10, 10), 2);
        assert_eq!(digits(255, 16), 2);
        assert_eq!(digits(256, 16), 3);
        assert_eq!(digits(8, 8), 2);
        assert_eq!(digits(5, 2), 3);
        assert_eq!(digits(u64::MAX, 10), 20);
    }

    #[test]
    fn format_number_in_all_bases_and_leads() {
        assert_eq!(format_number(10, 10, false, 4, Lead::None), "10  ");
        assert_eq!(format_number(10, 10, false, 4, Lead::Zeros), "0010");
        assert_eq!(format_number(10, 10, false, 4, Lead::Spaces), "  10");
        assert_eq!(format_number(255, 16, false, 3, Lead::Zeros), "0ff");
        assert_eq!(format_number(255, 16, true, 3, Lead::Spaces), " FF");
        assert_eq!(format_number(8, 8, false, 2, Lead::None), "10");
        assert_eq!(format_number(5, 2, false, 4, Lead::Zeros), "0101");
        assert_eq!(format_number(0, 2, false, 1, Lead::None), "0");
    }

    #[test]
    fn numbers_pad_to_the_widest_of_first_and_last() {
        assert_eq!(nums(1, 1, 1, 3, 10, Lead::None), ["1", "2", "3"]);
        assert_eq!(nums(8, 1, 1, 3, 10, Lead::None), ["8 ", "9 ", "10"]);
        assert_eq!(nums(8, 1, 1, 3, 10, Lead::Zeros), ["08", "09", "10"]);
        assert_eq!(nums(8, 1, 1, 3, 10, Lead::Spaces), [" 8", " 9", "10"]);
        assert_eq!(nums(14, 1, 1, 3, 16, Lead::Zeros), ["0e", "0f", "10"]);
        assert_eq!(numbers(10, 5, 1, 2, 16, true, Lead::None), ["A", "F"]);
        assert_eq!(nums(6, 1, 1, 3, 8, Lead::None), ["6 ", "7 ", "10"]);
        assert_eq!(nums(0, 1, 1, 3, 2, Lead::Zeros), ["00", "01", "10"]);
    }

    #[test]
    fn repeat_gives_each_number_n_times() {
        assert_eq!(nums(1, 1, 2, 5, 10, Lead::None), ["1", "1", "2", "2", "3"]);
        assert_eq!(nums(5, 10, 0, 3, 10, Lead::None), ["5 ", "15", "25"]);
        assert_eq!(nums(0, 0, 1, 2, 10, Lead::None), ["0", "0"]);
        assert!(nums(0, 0, 1, 0, 10, Lead::None).is_empty());
    }

    #[test]
    fn negative_values_wrap_like_size_t() {
        assert_eq!(nums(-5, 0, 1, 1, 10, Lead::None), ["18446744073709551611"]);
        assert_eq!(
            nums(1, -1, 1, 3, 10, Lead::None)[..2],
            ["1                   ", "0                   "]
        );
        assert_eq!(nums(1, -1, 1, 3, 10, Lead::None)[2], "18446744073709551615");
        assert_eq!(nums(3, 1, -2, 3, 10, Lead::None), ["3", "3", "3"]);
        assert_eq!(
            nums(i32::MAX, i32::MAX, 1, 3, 10, Lead::None)[2],
            "6442450941"
        );
    }

    #[test]
    fn parse_field_follows_stoi() {
        assert_eq!(parse_field("", 10), Some(0));
        assert_eq!(parse_field("12", 10), Some(12));
        assert_eq!(parse_field("  12", 10), Some(12));
        assert_eq!(parse_field("12 ", 10), None);
        assert_eq!(parse_field("+7", 10), Some(7));
        assert_eq!(parse_field("-5", 10), Some(-5));
        assert_eq!(parse_field("-1", 10), None);
        assert_eq!(parse_field("1a", 10), None);
        assert_eq!(parse_field("ff", 16), Some(255));
        assert_eq!(parse_field("0xFF", 16), Some(255));
        assert_eq!(parse_field("0x", 16), None);
        assert_eq!(parse_field("0x10", 10), None);
        assert_eq!(parse_field("17", 8), Some(15));
        assert_eq!(parse_field("8", 8), None);
        assert_eq!(parse_field("101", 2), Some(5));
        assert_eq!(parse_field("2", 2), None);
        assert_eq!(parse_field("2147483647", 10), Some(i32::MAX));
        assert_eq!(parse_field("2147483648", 10), None);
        assert_eq!(parse_field("-2147483648", 10), Some(i32::MIN));
        assert_eq!(parse_field("99999999999999999999999", 10), None);
        assert_eq!(parse_field("-", 10), None);
    }

    #[test]
    fn field_text_writes_the_stored_number() {
        assert_eq!(field_text(255, 16, true), "FF");
        assert_eq!(field_text(8, 8, false), "10");
        assert_eq!(field_text(5, 2, false), "101");
        assert_eq!(field_text(12, 10, false), "12");
        assert_eq!(field_text(-5, 16, false), "fffffffffffffffb");
        assert_eq!(field_text(-5, 10, false), "4294967291");
    }

    #[test]
    fn invalid_message_names_the_base() {
        assert_eq!(
            invalid_message("z", 10),
            "Entered string \"z\":\nDecimal numbers only use 0-9!"
        );
        assert_eq!(
            invalid_message("g", 16),
            "Entered string \"g\":\nHex numbers use 0-9, A-F!"
        );
    }

    fn sel(anchor: isize, caret: isize, anchor_vs: isize, caret_vs: isize) -> Sel {
        Sel {
            anchor,
            caret,
            anchor_vs,
            caret_vs,
        }
    }

    fn apply(doc: &str, p: &Plan) -> String {
        let mut b = doc.as_bytes().to_vec();
        for (a, e, t) in &p.edits {
            b.splice(*a as usize..*e as usize, t.iter().copied());
        }
        String::from_utf8(b).unwrap()
    }

    #[test]
    fn plan_replaces_each_selection_in_position_order() {
        let doc = "abc\nabc\nabc";
        let sels = [sel(9, 9, 0, 0), sel(1, 1, 0, 0), sel(5, 5, 0, 0)];
        let texts = [b"1".to_vec(), b"2".to_vec(), b"3".to_vec()];
        let p = plan(&sels, true, &texts);
        assert_eq!(apply(doc, &p), "a1bc\na2bc\na3bc");
        assert_eq!(p.ranges, [(12, 11), (2, 1), (7, 6)]);
        let p = plan(&sels, false, &texts);
        assert_eq!(p.ranges, [(11, 12), (1, 2), (6, 7)]);
    }

    #[test]
    fn plan_replaces_selected_text_and_keeps_direction() {
        let doc = "xxab\nxxab";
        let p = plan(
            &[sel(2, 4, 0, 0), sel(9, 7, 0, 0)],
            true,
            &[b"Q".to_vec(), b"R".to_vec()],
        );
        assert_eq!(apply(doc, &p), "xxQ\nxxR");
        assert_eq!(p.ranges, [(2, 3), (7, 6)]);
    }

    #[test]
    fn plan_turns_virtual_space_into_spaces() {
        let doc = "abcd\nab\nabcd";
        let sels = [sel(4, 4, 0, 0), sel(7, 7, 2, 2), sel(12, 12, 0, 0)];
        let p = plan(&sels, true, &[b"1".to_vec(), b"2".to_vec(), b"3".to_vec()]);
        assert_eq!(apply(doc, &p), "abcd1\nab  2\nabcd3");
        assert_eq!(p.ranges[1], (11, 10));
        let p = plan(&[sel(7, 7, 1, 3)], true, &[b"Z".to_vec()]);
        assert_eq!(apply(doc, &p), "abcd\nab Z\nabcd");
        assert_eq!(p.ranges, [(8, 9)]);
    }

    #[test]
    fn insert_in_line_pads_short_lines() {
        assert_eq!(insert_in_line(b"ab", 2, 4, 2, b"X"), b"ab  X");
        assert_eq!(insert_in_line(b"abcd", 4, 2, 2, b"X"), b"abXcd");
        assert_eq!(insert_in_line(b"abcd", 4, 4, 4, b"X"), b"abcdX");
        assert_eq!(insert_in_line(b"", 0, 0, 0, b"X"), b"X");
    }
}
