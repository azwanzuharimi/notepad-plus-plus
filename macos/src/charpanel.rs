// SPDX-License-Identifier: GPL-3.0-or-later
use crate::docking::{column, content_box, insert_text, scroll};
use crate::encoding::{self, Enc};
use crate::{ns, App};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{sel, MainThreadOnly};
use objc2_app_kit::{NSTableView, NSUserInterfaceItemIdentification, NSView};
use std::cell::{Cell, RefCell};


// asciiListView.cpp getAscii: names of the control characters and the space.
const CONTROL: [&str; 33] = [
    "NULL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "TAB", "LF", "VT", "FF", "CR",
    "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC",
    "FS", "GS", "RS", "US", "Space",
];
// asciiListView.cpp getHtmlName below 160.
const NAMES: &str = "33 excl 34 quot 35 num 36 dollar 37 percnt 38 amp 39 apos 40 lpar 41 rpar 42 ast 43 plus 44 comma 45 minus 46 period 47 sol 58 colon 59 semi 60 lt 61 equals 62 gt 63 quest 64 commat 91 lbrack 92 bsol 93 rbrack 94 Hat 95 lowbar 96 grave 123 lbrace 124 vert 125 rbrace 128 euro 130 sbquo 131 fnof 132 bdquo 133 hellip 134 dagger 135 Dagger 136 circ 137 permil 138 Scaron 139 lsaquo 140 OElig 142 Zcaron 145 lsquo 146 rsquo 147 ldquo 148 rdquo 149 bull 150 ndash 151 mdash 152 tilde 153 trade 154 scaron 155 rsaquo 156 oelig 158 zcaron 159 Yuml";
// asciiListView.cpp getHtmlName from 160 to 255.
const HIGH_NAMES: &str = "nbsp iexcl cent pound curren yen brvbar sect uml copy ordf laquo not shy reg macr deg plusmn sup2 sup3 acute micro para middot cedil sup1 ordm raquo frac14 frac12 frac34 iquest Agrave Aacute Acirc Atilde Auml Aring AElig Ccedil Egrave Eacute Ecirc Euml Igrave Iacute Icirc Iuml ETH Ntilde Ograve Oacute Ocirc Otilde Ouml times Oslash Ugrave Uacute Ucirc Uuml Yacute THORN szlig agrave aacute acirc atilde auml aring aelig ccedil egrave eacute ecirc euml igrave iacute icirc iuml eth ntilde ograve oacute ocirc otilde ouml divide oslash ugrave uacute ucirc uuml yacute thorn yuml";

pub struct Chars {
    table: Retained<NSTableView>,
    cp: Cell<Option<u32>>,
    rows: RefCell<Vec<[String; 6]>>,
}

// The character of a byte in the code page; 0 is the ANSI code page. A byte that has no character gives "".
pub fn char_of(cp: u32, b: u8) -> String {
    let e = if cp == 0 { Enc::Ansi } else { Enc::Cp(cp) };
    let s = String::from_utf8(encoding::decode(&[b], e).0).unwrap_or_default();
    if s.contains('\u{FFFD}') {
        String::new()
    } else {
        s
    }
}

fn html_name(b: u8) -> String {
    let named = |list: &str| {
        let w: Vec<&str> = list.split(' ').collect();
        w.chunks(2)
            .find(|p| p[0] == b.to_string())
            .map(|p| p[1].to_string())
    };
    let name = if b >= 160 {
        HIGH_NAMES.split(' ').nth(b as usize - 160).map(str::to_string)
    } else {
        named(NAMES)
    };
    name.map_or(String::new(), |n| format!("&{n};"))
}

// asciiListView.cpp getHtmlNumber: the Unicode value of the Windows-1252 characters from 128 to 159, and U+2212 for the hyphen.
fn html_number(b: u8) -> Option<u32> {
    match b {
        32..=126 if b != 45 => Some(b as u32),
        160..=255 => Some(b as u32),
        45 => Some(8722),
        128..=159 => char_of(1252, b)
            .chars()
            .next()
            .map(|c| c as u32)
            .filter(|&c| c != b as u32),
        _ => None,
    }
}

// AsciiListView::setValues: Value, Hex, Character, HTML Name, HTML Decimal, HTML Hexadecimal; the HTML columns only for code page 0 or 1252.
pub fn rows(cp: u32) -> Vec<[String; 6]> {
    (0..=255u8)
        .map(|b| {
            let c = match b {
                0..=32 => CONTROL[b as usize].to_string(),
                127 => "DEL".to_string(),
                _ => char_of(cp, b),
            };
            let html = cp == 0 || cp == 1252;
            let n = html.then(|| html_number(b)).flatten();
            [
                b.to_string(),
                format!("{b:02X}"),
                c,
                if html { html_name(b) } else { String::new() },
                n.map_or(String::new(), |n| format!("&#{n};")),
                n.map_or(String::new(), |n| format!("&#x{n:x};")),
            ]
        })
        .collect()
}

impl App {
    fn char_page(&self) -> u32 {
        match self.current().and_then(|i| self.tab(i)).map(|t| t.enc) {
            Some(Enc::Cp(cp)) => cp,
            _ => 0,
        }
    }

    pub(crate) fn chars_build(&self) -> Retained<NSView> {
        let mtm = self.mtm();
        let b = content_box(mtm);
        let size = b.frame().size;
        let table = NSTableView::initWithFrame(NSTableView::alloc(mtm), b.frame());
        table.setIdentifier(Some(&ns("chars")));
        let titles = [
            ("Value", 45.),
            ("Hex", 45.),
            ("Character", 70.),
            ("HTML Name", 90.),
            ("HTML Decimal", 100.),
            ("HTML Hexadecimal", 120.),
        ];
        for (k, (t, w)) in titles.iter().enumerate() {
            table.addTableColumn(&column(mtm, &k.to_string(), t, *w));
        }
        let Some(d) = self.dock_ui() else { return b };
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*d.target)));
            table.setTarget(Some(&d.target));
            table.setDoubleAction(Some(sel!(charInsert:)));
        }
        let s = scroll(mtm, &table, size.width, size.height);
        s.setHasHorizontalScroller(true);
        b.addSubview(&s);
        let _ = d.chars.set(Chars {
            table,
            cp: Cell::new(None),
            rows: RefCell::new(vec![]),
        });
        b
    }

    // AnsiCharPanel::switchEncoding: the rows follow the code page of the current document.
    pub(crate) fn chars_sync(&self) {
        let Some(c) = self.dock_ui().and_then(|d| d.chars.get()) else {
            return;
        };
        let cp = self.char_page();
        if c.cp.replace(Some(cp)) != Some(cp) {
            *c.rows.borrow_mut() = rows(cp);
            c.table.reloadData();
        }
    }

    pub(crate) fn chars_text(&self, row: isize, col: usize) -> Option<String> {
        let c = self.dock_ui()?.chars.get()?;
        let rows = c.rows.borrow();
        Some(rows.get(row as usize)?.get(col)?.clone())
    }

    // AnsiCharPanel NM_DBLCLK: the Character column inserts the character, an other column inserts its text.
    pub(crate) fn chars_insert(&self) {
        if let Some(c) = self.dock_ui().and_then(|d| d.chars.get()) {
            self.chars_insert_at(c.table.clickedRow(), c.table.clickedColumn());
        }
    }

    pub(crate) fn chars_insert_at(&self, row: isize, col: isize) {
        let Some(v) = self.editor() else { return };
        if !(0..256).contains(&row) {
            return;
        }
        let text = if col == 2 {
            char_of(self.char_page(), row as u8)
        } else {
            self.chars_text(row, col.max(0) as usize).unwrap_or_default()
        };
        insert_text(&v, &text);
        self.focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp1252_rows() {
        let r = rows(1252);
        assert_eq!(r.len(), 256);
        assert_eq!(r[65], ["65", "41", "A", "", "&#65;", "&#x41;"].map(String::from));
        assert_eq!(r[0][2], "NULL");
        assert_eq!(r[32], ["32", "20", "Space", "", "&#32;", "&#x20;"].map(String::from));
        assert_eq!(r[127][2], "DEL");
        assert_eq!(r[127][4], "");
        assert_eq!(r[38][3], "&amp;");
        assert_eq!(r[45], ["45", "2D", "-", "&minus;", "&#8722;", "&#x2212;"].map(String::from));
        assert_eq!(r[126][3], "");
        assert_eq!(
            r[128],
            ["128", "80", "\u{20AC}", "&euro;", "&#8364;", "&#x20ac;"].map(String::from)
        );
        assert_eq!(r[129][3..], ["", "", ""].map(String::from));
        assert_eq!(r[159][3], "&Yuml;");
        assert_eq!(r[160][3], "&nbsp;");
        assert_eq!(r[233], ["233", "E9", "\u{e9}", "&eacute;", "&#233;", "&#xe9;"].map(String::from));
        assert_eq!(r[255][3], "&yuml;");
        assert_eq!(HIGH_NAMES.split(' ').count(), 96);
        assert_eq!(rows(0), r);
    }

    #[test]
    fn other_code_page_rows() {
        let r = rows(1251);
        assert_eq!(r[192][2], "\u{410}");
        assert_eq!(r[192][3..], ["", "", ""].map(String::from));
        assert_eq!(r[65][4], "");
        assert_eq!(char_of(0, 0x80), "\u{20AC}");
        assert_eq!(char_of(0, b'a'), "a");
    }
}
