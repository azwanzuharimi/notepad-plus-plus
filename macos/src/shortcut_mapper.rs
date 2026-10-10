// SPDX-License-Identifier: GPL-3.0-or-later
use crate::shortcuts::{Internal, Key, ScintKey, Shortcuts};
use crate::{macros, ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSBackingStoreType, NSButton, NSColor,
    NSControlTextEditingDelegate, NSEventModifierFlags, NSMenu, NSMenuItem, NSPopUpButton,
    NSScrollView, NSSegmentSwitchTracking, NSSegmentedControl, NSTableColumn, NSTableView,
    NSTableViewDataSource, NSTableViewDelegate, NSTextField, NSTextFieldDelegate, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString,
};
use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

const SCI_ASSIGNCMDKEY: u32 = 2070;
const SCI_CLEARALLCMDKEYS: u32 = 2072;
const SCMOD_SHIFT: i32 = 1;
const SCMOD_CTRL: i32 = 2;
const SCMOD_ALT: i32 = 4;
const SCMOD_META: i32 = 16;
const F1: u32 = 0xF704;

pub const TABS: [&str; 4] = ["Main menu", "Macros", "Run commands", "Scintilla commands"];
// The conflict groups: the tabs, then the Window menu, which has no tab.
const GROUPS: [&str; 5] = ["Main menu", "Macros", "Run commands", "Scintilla commands", "Window menu"];

// Port of namedKeyArray of shortcut.cpp, without the items that have no name.
fn key_names() -> &'static [(String, u8)] {
    static K: OnceLock<Vec<(String, u8)>> = OnceLock::new();
    K.get_or_init(|| {
        let s = |v: &[(&str, u8)]| {
            v.iter()
                .map(|(n, k)| (n.to_string(), *k))
                .collect::<Vec<_>>()
        };
        let mut v = s(&[
            ("None", 0),
            ("Backspace", 0x08),
            ("Tab", 0x09),
            ("Enter", 0x0D),
            ("Esc", 0x1B),
            ("Spacebar", 0x20),
            ("Page up", 0x21),
            ("Page down", 0x22),
            ("End", 0x23),
            ("Home", 0x24),
            ("Left", 0x25),
            ("Up", 0x26),
            ("Right", 0x27),
            ("Down", 0x28),
            ("INS", 0x2D),
            ("DEL", 0x2E),
        ]);
        v.extend(
            (b'0'..=b'9')
                .chain(b'A'..=b'Z')
                .map(|c| ((c as char).to_string(), c)),
        );
        v.extend((0..10u8).map(|n| (format!("Numpad {n}"), 0x60 + n)));
        v.extend(s(&[
            ("Num *", 0x6A),
            ("Num +", 0x6B),
            ("Num -", 0x6D),
            ("Num .", 0x6E),
            ("Num /", 0x6F),
        ]));
        v.extend((1..=24u8).map(|n| (format!("F{n}"), 0x6F + n)));
        v.extend(s(&[
            ("~", 0xC0),
            ("-", 0xBD),
            ("=", 0xBB),
            ("[", 0xDB),
            ("]", 0xDD),
            (";", 0xBA),
            ("'", 0xDE),
            ("\\", 0xDC),
            (",", 0xBC),
            (".", 0xBE),
            ("/", 0xBF),
            ("<>", 0xE2),
        ]));
        v
    })
}

// Windows virtual key, macOS menu key equivalent, Scintilla key code.
const NAV: [(u8, char, i32); 15] = [
    (0x08, '\u{7f}', 8),
    (0x09, '\t', 9),
    (0x0D, '\r', 13),
    (0x1B, '\u{1b}', 7),
    (0x20, ' ', 32),
    (0x21, '\u{F72C}', 306),
    (0x22, '\u{F72D}', 307),
    (0x23, '\u{F72B}', 305),
    (0x24, '\u{F729}', 304),
    (0x25, '\u{F702}', 302),
    (0x26, '\u{F700}', 301),
    (0x27, '\u{F703}', 303),
    (0x28, '\u{F701}', 300),
    (0x2D, '\u{F727}', 309),
    (0x2E, '\u{F728}', 308),
];
const PAD: [(u8, char, i32); 5] = [
    (0x6A, '*', 42),
    (0x6B, '+', 310),
    (0x6D, '-', 311),
    (0x6E, '.', 46),
    (0x6F, '/', 312),
];
const OEM: [(u8, char); 11] = [
    (0xC0, '`'),
    (0xBD, '-'),
    (0xBB, '='),
    (0xDB, '['),
    (0xDD, ']'),
    (0xBA, ';'),
    (0xDE, '\''),
    (0xDC, '\\'),
    (0xBC, ','),
    (0xBE, '.'),
    (0xBF, '/'),
];
// Characters of the US layout with Shift, and the key that gives them.
const SHIFTED: [(char, char); 21] = [
    ('~', '`'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    (':', ';'),
    ('"', '\''),
    ('|', '\\'),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
];

fn is_fkey(vk: u8) -> bool {
    (0x70..=0x87).contains(&vk)
}

// The menu key equivalent of a Windows virtual key, and true for a key of the numeric keypad.
pub fn menu_key(vk: u8) -> Option<(char, bool)> {
    if let Some(n) = NAV.iter().find(|n| n.0 == vk) {
        return Some((n.1, false));
    }
    if let Some(p) = PAD.iter().find(|p| p.0 == vk) {
        return Some((p.1, true));
    }
    match vk {
        b'0'..=b'9' => Some((vk as char, false)),
        b'A'..=b'Z' => Some((vk.to_ascii_lowercase() as char, false)),
        0x60..=0x69 => Some(((b'0' + vk - 0x60) as char, true)),
        _ if is_fkey(vk) => char::from_u32(F1 + (vk - 0x70) as u32).map(|c| (c, false)),
        _ => OEM.iter().find(|o| o.0 == vk).map(|o| (o.1, false)),
    }
}

// The Windows virtual key of a menu key equivalent, and true when the character needs Shift.
pub fn key_of_menu(c: char, pad: bool) -> Option<(u8, bool)> {
    if pad {
        if let Some(d) = c.to_digit(10) {
            return Some((0x60 + d as u8, false));
        }
        if let Some(p) = PAD.iter().find(|p| p.1 == c) {
            return Some((p.0, false));
        }
    }
    if c == '\u{8}' {
        return Some((0x08, false));
    }
    if let Some(n) = NAV.iter().find(|n| n.1 == c) {
        return Some((n.0, false));
    }
    if let Some(o) = OEM.iter().find(|o| o.1 == c) {
        return Some((o.0, false));
    }
    if let Some(s) = SHIFTED.iter().find(|s| s.0 == c) {
        return key_of_menu(s.1, false).map(|(k, _)| (k, true));
    }
    match c {
        'a'..='z' => Some((c.to_ascii_uppercase() as u8, false)),
        'A'..='Z' | '0'..='9' => Some((c as u8, c.is_ascii_uppercase())),
        _ => {
            let f = (c as u32).checked_sub(F1).filter(|f| *f < 24)?;
            Some((0x70 + f as u8, false))
        }
    }
}

// The character that Shift gives with `c` on the US layout: AppKit and Scintilla see this character, not `c`.
fn shifted(c: char) -> Option<char> {
    if c.is_ascii_lowercase() {
        return Some(c.to_ascii_uppercase());
    }
    SHIFTED.iter().find(|s| s.1 == c).map(|s| s.0)
}

// The menu key equivalent and the flags Command, Option, Shift, Control, numeric keypad of a key.
// Shift with a character key gives the shifted character without the Shift flag, as AppKit matches it.
pub fn menu_equiv(k: &Key) -> Option<(char, [bool; 5])> {
    let (c, pad) = menu_key(k.key)?;
    let s = (k.shift && !pad).then(|| shifted(c)).flatten();
    Some((
        s.unwrap_or(c),
        [k.ctrl, k.alt, k.shift && s.is_none(), k.meta, pad],
    ))
}

// The Scintilla key code of a Windows virtual key; letters are lower case, as in the Cocoa key map of Scintilla.
pub fn sci_key(vk: u8) -> Option<i32> {
    if let Some(n) = NAV.iter().find(|n| n.0 == vk) {
        return Some(n.2);
    }
    if let Some(p) = PAD.iter().find(|p| p.0 == vk) {
        return Some(p.2);
    }
    menu_key(vk).map(|(c, _)| c as i32)
}

pub fn key_of_sci(code: i32) -> Option<u8> {
    if let Some(n) = NAV.iter().find(|n| n.2 == code) {
        return Some(n.0);
    }
    if let Some(p) = PAD.iter().find(|p| p.2 == code && p.2 > 255) {
        return Some(p.0);
    }
    if code == 42 {
        return Some(0x6A);
    }
    key_of_menu(char::from_u32(code as u32)?, false).map(|(k, _)| k)
}

// The keyDefinition of SCI_ASSIGNCMDKEY.
pub fn sci_def(k: &Key) -> Option<usize> {
    let mods = [
        (k.shift, SCMOD_SHIFT),
        (k.ctrl, SCMOD_CTRL),
        (k.alt, SCMOD_ALT),
        (k.meta, SCMOD_META),
    ]
    .iter()
    .filter(|m| m.0)
    .fold(0, |a, m| a | m.1);
    let code = sci_key(k.key)?;
    // Scintilla on macOS compares the characters without modifiers except Shift, so Shift gives the shifted character.
    let code = match char::from_u32(code as u32) {
        Some(c) if k.shift && !(0x60..=0x6F).contains(&k.key) => shifted(c).map_or(code, |c| c as i32),
        _ => code,
    };
    Some((code | mods << 16) as usize)
}

fn key_from_sci(code: i32, mods: i32) -> Option<Key> {
    Some(Key {
        ctrl: mods & SCMOD_CTRL != 0,
        alt: mods & SCMOD_ALT != 0,
        shift: mods & SCMOD_SHIFT != 0,
        meta: mods & SCMOD_META != 0,
        key: key_of_sci(code)?,
    })
}

// Port of Shortcut::toString with the macOS key names.
pub fn key_text(k: &Key) -> String {
    if k.key == 0 {
        return String::new();
    }
    let mut s = String::new();
    for (on, n) in [
        (k.meta, "Control+"),
        (k.ctrl, "Cmd+"),
        (k.alt, "Option+"),
        (k.shift, "Shift+"),
    ] {
        if on {
            s += n;
        }
    }
    s + key_names()
        .iter()
        .find(|n| n.1 == k.key)
        .map_or("Unlisted", |n| n.0.as_str())
}

fn keys_text(keys: &[Key]) -> String {
    keys.iter()
        .filter(|k| k.key != 0)
        .map(key_text)
        .collect::<Vec<_>>()
        .join(" / ")
}

// Keys that macOS keeps: Cmd+Tab, Cmd+Shift+Tab, Cmd+Space, and the Cmd+Shift+3, 4 and 5 screenshots.
pub fn system_key(k: &Key) -> bool {
    let cmd = k.ctrl && !k.alt && !k.meta;
    cmd && (k.key == 0x09 || (k.key == 0x20 && !k.shift) || (k.shift && matches!(k.key, b'3' | b'4' | b'5')))
}

// Port of Shortcut::isValid: letters, digits, space, Caps Lock, Backspace and Enter need a modifier.
pub fn valid(k: &Key) -> bool {
    let needs = k.key.is_ascii_uppercase() || k.key.is_ascii_digit() || [0x20, 0x14, 0x08, 0x0D].contains(&k.key);
    (k.key == 0 || !needs || k.ctrl || k.alt || k.meta) && !system_key(k)
}

// AppKit does not tell numeric keypad keys from the main keys, so a keypad key is the same as its main key.
fn same_key(a: &Key) -> Key {
    let (key, shift) = match a.key {
        0x60..=0x69 => (a.key - 0x60 + b'0', a.shift),
        0x6A => (b'8', true),
        0x6B => (0xBB, true),
        0x6D => (0xBD, a.shift),
        0x6E => (0xBE, a.shift),
        0x6F => (0xBF, a.shift),
        k => (k, a.shift),
    };
    Key { key, shift, ..*a }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    pub keys: Vec<Key>,
}

// Port of ShortcutMapper::findKeyConflicts: one line for each other item with the same key.
pub fn conflicts(all: &[Vec<Item>], tab: usize, idx: usize, k: &Key) -> Vec<String> {
    let mut out = vec![];
    if k.key == 0 {
        return out;
    }
    for (g, items) in all.iter().enumerate() {
        for (i, it) in items.iter().enumerate() {
            if g == tab && i == idx {
                continue;
            }
            for (j, c) in it.keys.iter().enumerate() {
                if c.key != 0 && same_key(c) == same_key(k) {
                    let star = if j > 0 { "*" } else { "" };
                    out.push(format!(
                        "{}  |  {}{star}   {}  ( {} )",
                        GROUPS[g],
                        i + 1,
                        it.name,
                        key_text(c)
                    ));
                }
            }
        }
    }
    out
}

// The Scintilla commands of scintKeyDefs (Parameters.cpp): name, SCI message, Notepad++ command.
pub const SCINT: [(&str, i32, i32); 96] = [
    ("SCI_SELECTALL", 2013, 42007),
    ("SCI_CLEAR", 2180, 42006),
    ("SCI_CLEARALL", 2004, 0),
    ("SCI_UNDO", 2176, 42003),
    ("SCI_REDO", 2011, 42004),
    ("SCI_NEWLINE", 2329, 0),
    ("SCI_TAB", 2327, 0),
    ("SCI_BACKTAB", 2328, 0),
    ("SCI_FORMFEED", 2330, 0),
    ("SCI_ZOOMIN", 2333, 44023),
    ("SCI_ZOOMOUT", 2334, 44024),
    ("SCI_SETZOOM", 2373, 44033),
    ("SCI_SELECTIONDUPLICATE", 2469, 0),
    ("SCI_LINESJOIN", 2288, 0),
    ("SCI_SCROLLCARET", 2169, 0),
    ("SCI_EDITTOGGLEOVERTYPE", 2324, 0),
    ("SCI_MOVECARETINSIDEVIEW", 2401, 0),
    ("SCI_LINEDOWN", 2300, 0),
    ("SCI_LINEDOWNEXTEND", 2301, 0),
    ("SCI_LINEDOWNRECTEXTEND", 2426, 0),
    ("SCI_LINESCROLLDOWN", 2342, 0),
    ("SCI_LINEUP", 2302, 0),
    ("SCI_LINEUPEXTEND", 2303, 0),
    ("SCI_LINEUPRECTEXTEND", 2427, 0),
    ("SCI_LINESCROLLUP", 2343, 0),
    ("SCI_PARADOWN", 2413, 0),
    ("SCI_PARADOWNEXTEND", 2414, 0),
    ("SCI_PARAUP", 2415, 0),
    ("SCI_PARAUPEXTEND", 2416, 0),
    ("SCI_CHARLEFT", 2304, 0),
    ("SCI_CHARLEFTEXTEND", 2305, 0),
    ("SCI_CHARLEFTRECTEXTEND", 2428, 0),
    ("SCI_CHARRIGHT", 2306, 0),
    ("SCI_CHARRIGHTEXTEND", 2307, 0),
    ("SCI_CHARRIGHTRECTEXTEND", 2429, 0),
    ("SCI_WORDLEFT", 2308, 0),
    ("SCI_WORDLEFTEXTEND", 2309, 0),
    ("SCI_WORDRIGHT", 2310, 0),
    ("SCI_WORDRIGHTEXTEND", 2311, 0),
    ("SCI_WORDLEFTEND", 2439, 0),
    ("SCI_WORDLEFTENDEXTEND", 2440, 0),
    ("SCI_WORDRIGHTEND", 2441, 0),
    ("SCI_WORDRIGHTENDEXTEND", 2442, 0),
    ("SCI_WORDPARTLEFT", 2390, 0),
    ("SCI_WORDPARTLEFTEXTEND", 2391, 0),
    ("SCI_WORDPARTRIGHT", 2392, 0),
    ("SCI_WORDPARTRIGHTEXTEND", 2393, 0),
    ("SCI_HOME", 2312, 0),
    ("SCI_HOMEEXTEND", 2313, 0),
    ("SCI_HOMERECTEXTEND", 2430, 0),
    ("SCI_HOMEDISPLAY", 2345, 0),
    ("SCI_HOMEDISPLAYEXTEND", 2346, 0),
    ("SCI_HOMEWRAP", 2349, 0),
    ("SCI_HOMEWRAPEXTEND", 2450, 0),
    ("SCI_VCHOME", 2331, 0),
    ("SCI_VCHOMEEXTEND", 2332, 0),
    ("SCI_VCHOMERECTEXTEND", 2431, 0),
    ("SCI_VCHOMEDISPLAY", 2652, 0),
    ("SCI_VCHOMEDISPLAYEXTEND", 2653, 0),
    ("SCI_VCHOMEWRAP", 2453, 0),
    ("SCI_VCHOMEWRAPEXTEND", 2454, 0),
    ("SCI_LINEEND", 2314, 0),
    ("SCI_LINEENDWRAPEXTEND", 2452, 0),
    ("SCI_LINEENDRECTEXTEND", 2432, 0),
    ("SCI_LINEENDDISPLAY", 2347, 0),
    ("SCI_LINEENDDISPLAYEXTEND", 2348, 0),
    ("SCI_LINEENDWRAP", 2451, 0),
    ("SCI_LINEENDEXTEND", 2315, 0),
    ("SCI_DOCUMENTSTART", 2316, 0),
    ("SCI_DOCUMENTSTARTEXTEND", 2317, 0),
    ("SCI_DOCUMENTEND", 2318, 0),
    ("SCI_DOCUMENTENDEXTEND", 2319, 0),
    ("SCI_PAGEUP", 2320, 0),
    ("SCI_PAGEUPEXTEND", 2321, 0),
    ("SCI_PAGEUPRECTEXTEND", 2433, 0),
    ("SCI_PAGEDOWN", 2322, 0),
    ("SCI_PAGEDOWNEXTEND", 2323, 0),
    ("SCI_PAGEDOWNRECTEXTEND", 2434, 0),
    ("SCI_STUTTEREDPAGEUP", 2435, 0),
    ("SCI_STUTTEREDPAGEUPEXTEND", 2436, 0),
    ("SCI_STUTTEREDPAGEDOWN", 2437, 0),
    ("SCI_STUTTEREDPAGEDOWNEXTEND", 2438, 0),
    ("SCI_DELETEBACK", 2326, 0),
    ("SCI_DELETEBACKNOTLINE", 2344, 0),
    ("SCI_DELWORDLEFT", 2335, 0),
    ("SCI_DELWORDRIGHT", 2336, 0),
    ("SCI_DELLINELEFT", 2395, 0),
    ("SCI_DELLINERIGHT", 2396, 0),
    ("SCI_LINEDELETE", 2338, 0),
    ("SCI_LINECUT", 2337, 0),
    ("SCI_LINECOPY", 2455, 0),
    ("SCI_LINETRANSPOSE", 2339, 0),
    ("SCI_LINEDUPLICATE", 2404, 42010),
    ("SCI_CANCEL", 2325, 0),
    ("SCI_SWAPMAINANCHORCARET", 2607, 0),
    ("SCI_ROTATESELECTION", 2606, 0),
];

// macMapDefault of ScintillaCocoa.mm: key code, SCMOD flags, SCI message.
const MAC_KEYS: [(i32, i32, i32); 87] = [
    (300, 2, 2318),
    (300, 3, 2319),
    (301, 2, 2316),
    (301, 3, 2317),
    (302, 2, 2331),
    (302, 3, 2332),
    (303, 2, 2314),
    (303, 3, 2315),
    (300, 0, 2300),
    (300, 1, 2301),
    (300, 16, 2342),
    (300, 5, 2426),
    (301, 0, 2302),
    (301, 1, 2303),
    (301, 16, 2343),
    (301, 5, 2427),
    (91, 2, 2415),
    (91, 3, 2416),
    (93, 2, 2413),
    (93, 3, 2414),
    (302, 0, 2304),
    (302, 1, 2305),
    (302, 4, 2308),
    (302, 16, 2308),
    (302, 17, 2309),
    (302, 5, 2428),
    (303, 0, 2306),
    (303, 1, 2307),
    (303, 4, 2310),
    (303, 16, 2310),
    (303, 17, 2311),
    (303, 5, 2429),
    (47, 2, 2390),
    (47, 3, 2391),
    (92, 2, 2392),
    (92, 3, 2393),
    (304, 0, 2331),
    (304, 1, 2332),
    (304, 2, 2316),
    (304, 3, 2317),
    (304, 4, 2345),
    (304, 5, 2431),
    (305, 0, 2314),
    (305, 1, 2315),
    (305, 2, 2318),
    (305, 3, 2319),
    (305, 4, 2347),
    (305, 5, 2432),
    (306, 0, 2320),
    (306, 1, 2321),
    (306, 5, 2433),
    (307, 0, 2322),
    (307, 1, 2323),
    (307, 5, 2434),
    (308, 0, 2180),
    (308, 1, 2177),
    (308, 2, 2336),
    (308, 3, 2396),
    (309, 0, 2324),
    (309, 1, 2179),
    (309, 2, 2178),
    (7, 0, 2325),
    (8, 0, 2326),
    (8, 1, 2326),
    (8, 2, 2335),
    (8, 4, 2335),
    (8, 3, 2395),
    (122, 2, 2176),
    (122, 3, 2011),
    (120, 2, 2177),
    (99, 2, 2178),
    (118, 2, 2179),
    (97, 2, 2013),
    (9, 0, 2327),
    (9, 1, 2328),
    (13, 0, 2329),
    (13, 1, 2329),
    (310, 2, 2333),
    (311, 2, 2334),
    (312, 2, 2373),
    (108, 2, 2337),
    (108, 3, 2338),
    (116, 3, 2455),
    (116, 2, 2339),
    (100, 2, 2469),
    (117, 2, 2340),
    (117, 3, 2341),
];

// The keys of a Scintilla command: the shortcuts.xml ones, else the macOS defaults of Scintilla.
pub fn scint_keys(s: &Shortcuts, row: usize) -> Vec<Key> {
    let (_, msg, menu) = SCINT[row];
    if let Some(k) = s.scint.iter().find(|k| k.id == msg && k.menu_id == menu) {
        return k.keys.clone();
    }
    MAC_KEYS
        .iter()
        .filter(|m| m.2 == msg)
        .filter_map(|m| key_from_sci(m.0, m.1))
        .collect()
}

// Port of ScintillaAccelerator::updateKeys: the other macOS defaults first, then the list from the bottom, so the top wins.
pub fn keymap(s: &Shortcuts) -> Vec<(usize, i32)> {
    let mut v: Vec<(usize, i32)> = MAC_KEYS
        .iter()
        .filter(|m| !SCINT.iter().any(|r| r.1 == m.2))
        .filter_map(|m| Some((sci_def(&key_from_sci(m.0, m.1)?)?, m.2)))
        .collect();
    for row in (0..SCINT.len()).rev() {
        for k in scint_keys(s, row) {
            v.extend(
                sci_def(&k)
                    .filter(|_| k.key != 0)
                    .map(|d| (d, SCINT[row].1)),
            );
        }
    }
    v
}

fn command_table() -> Vec<(i32, &'static str, isize)> {
    macros::menu_cmds()
        .into_iter()
        .map(|c| (c.id, c.action, c.tag))
        .collect()
}

fn find_id(table: &[(i32, &str, isize)], action: &str, tag: isize) -> Option<i32> {
    table
        .iter()
        .find(|c| c.1 == action && (c.2 == macros::ANY || c.2 == tag))
        .map(|c| c.0)
}

struct MenuRow {
    id: i32,
    name: String,
    cat: String,
    items: Vec<Retained<NSMenuItem>>,
}

fn walk(m: &NSMenu, cat: &str, table: &[(i32, &str, isize)], rows: &mut Vec<MenuRow>) {
    for i in 0..m.numberOfItems() {
        let Some(it) = m.itemAtIndex(i) else { continue };
        if let Some(s) = it.submenu() {
            walk(&s, cat, table, rows);
            continue;
        }
        let Some(a) = it.action() else { continue };
        let Some(id) = find_id(table, a.name().to_str().unwrap_or(""), it.tag()) else {
            continue;
        };
        match rows.iter_mut().find(|r| r.id == id) {
            Some(r) => r.items.push(it),
            None => rows.push(MenuRow {
                id,
                name: it.title().to_string(),
                cat: cat.into(),
                items: vec![it],
            }),
        }
    }
}

// The main menu commands with a Notepad++ ID; a menu with a delegate (Window) is not used, because AppKit does not check its keys.
fn menu_rows(mtm: MainThreadMarker) -> Vec<MenuRow> {
    let mut rows = vec![];
    let Some(bar) = NSApplication::sharedApplication(mtm).mainMenu() else {
        return rows;
    };
    let table = command_table();
    for i in 0..bar.numberOfItems() {
        let Some(top) = bar.itemAtIndex(i) else { continue };
        let Some(sub) = top.submenu() else { continue };
        if sub.delegate().is_none() {
            walk(&sub, &top.title().to_string(), &table, &mut rows);
        }
    }
    rows
}

fn keyed_items(m: &NSMenu, out: &mut Vec<Item>) {
    for i in 0..m.numberOfItems() {
        let Some(it) = m.itemAtIndex(i) else { continue };
        if let Some(s) = it.submenu() {
            keyed_items(&s, out);
        } else if item_key(&it).key != 0 {
            out.push(Item {
                name: it.title().to_string(),
                keys: vec![item_key(&it)],
            });
        }
    }
}

// The items with a key in the menus that menu_rows does not use, for the conflict check.
fn window_items() -> Vec<Item> {
    let mut out = vec![];
    let Some(bar) = MainThreadMarker::new().and_then(|m| NSApplication::sharedApplication(m).mainMenu()) else {
        return out;
    };
    for i in 0..bar.numberOfItems() {
        if let Some(sub) = bar.itemAtIndex(i).and_then(|t| t.submenu()).filter(|s| s.delegate().is_some()) {
            keyed_items(&sub, &mut out);
        }
    }
    out
}

fn item_key(it: &NSMenuItem) -> Key {
    let m = it.keyEquivalentModifierMask();
    let c = it.keyEquivalent().to_string().chars().next();
    let Some((key, shift)) = c.and_then(|c| key_of_menu(c, m.contains(NSEventModifierFlags::NumericPad))) else {
        return Key::default();
    };
    Key {
        ctrl: m.contains(NSEventModifierFlags::Command),
        alt: m.contains(NSEventModifierFlags::Option),
        shift: shift || m.contains(NSEventModifierFlags::Shift),
        meta: m.contains(NSEventModifierFlags::Control),
        key,
    }
}

fn set_item_key(it: &NSMenuItem, k: &Key) {
    let (c, f) = menu_equiv(k).map_or((String::new(), [false; 5]), |(c, f)| (c.to_string(), f));
    let mut m = NSEventModifierFlags::empty();
    let flags = [
        NSEventModifierFlags::Command,
        NSEventModifierFlags::Option,
        NSEventModifierFlags::Shift,
        NSEventModifierFlags::Control,
        NSEventModifierFlags::NumericPad,
    ];
    for (on, x) in f.into_iter().zip(flags) {
        if on {
            m |= x;
        }
    }
    it.setKeyEquivalent(&ns(&c));
    it.setKeyEquivalentModifierMask(m);
}

const START_RECORD: i32 = 42018;
const STOP_RECORD: i32 = 42019;

thread_local! {
    // The keys of Start Recording and Stop Recording; only the enabled item has its key, see macros::check_state.
    static RECORD: Cell<[Key; 2]> = const { Cell::new([Key { ctrl: true, alt: false, shift: true, meta: false, key: b'R' }; 2]) };
}

pub fn record_key(it: &NSMenuItem, i: usize, on: bool) {
    let k = RECORD.with(|r| r.get()[i.min(1)]);
    set_item_key(it, &if on { k } else { Key::default() });
}

// Gives a menu item its key; Start and Stop Recording keep the key while they are disabled.
fn set_command_key(id: i32, it: &NSMenuItem, k: &Key) {
    if id == START_RECORD || id == STOP_RECORD {
        let i = (id == STOP_RECORD) as usize;
        RECORD.with(|r| {
            let mut v = r.get();
            v[i] = *k;
            r.set(v);
        });
        record_key(it, i, it.isEnabled());
    } else {
        set_item_key(it, k);
    }
}

fn command_key(id: i32, it: &NSMenuItem) -> Key {
    match id {
        START_RECORD => RECORD.with(|r| r.get()[0]),
        STOP_RECORD => RECORD.with(|r| r.get()[1]),
        _ => item_key(it),
    }
}

fn apply_rows(rows: &[MenuRow]) {
    let keys: Vec<(i32, Key)> =
        macros::with_store(|s| s.internal.iter().filter(|c| c.nth == 0).map(|c| (c.id, c.key)).collect());
    for (id, k) in keys {
        for it in rows.iter().filter(|r| r.id == id).flat_map(|r| &r.items) {
            set_command_key(id, it, &k);
        }
    }
}

// Gives the items of `m` the <InternalCommands> keys; for menus that the app makes again, as the recent files.
pub fn apply_overrides(m: &NSMenu) {
    let mut rows = vec![];
    walk(m, "", &command_table(), &mut rows);
    apply_rows(&rows);
}

fn bind(m: &NSMenu, action: Sel, keys: &[Key]) {
    for i in 0..m.numberOfItems() {
        let Some(it) = m.itemAtIndex(i) else { continue };
        if let Some(s) = it.submenu() {
            bind(&s, action, keys);
        } else if it.action() == Some(action) {
            if let Some(k) = usize::try_from(it.tag()).ok().and_then(|t| keys.get(t)) {
                set_item_key(&it, k);
            }
        }
    }
}

// Gives the saved macros and user commands of the Macro and Run menus their keys.
pub fn bind_saved(mac: &NSMenu, run: &NSMenu) {
    let (m, r) = macros::with_store(|s| {
        (
            s.macros.iter().map(|m| m.key).collect::<Vec<_>>(),
            s.commands.iter().map(|c| c.key).collect::<Vec<_>>(),
        )
    });
    bind(mac, sel!(macroRunSaved:), &m);
    bind(run, sel!(runUserCommand:), &r);
}

// The Settings menu item; it also loads the shortcuts.xml keys when the menu bar is complete.
pub fn menu_item(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    if let Some(t) = t {
        let _: () = unsafe {
            msg_send![t, performSelector: sel!(shortcutMapperStart:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]
        };
    }
    crate::item(mtm, "Shortcut Mapper...", sel!(showShortcutMapper:), "", t)
}

fn all_items(menu: &[MenuRow]) -> Vec<Vec<Item>> {
    let main = menu
        .iter()
        .map(|r| Item {
            name: r.name.clone(),
            keys: vec![command_key(r.id, &r.items[0])],
        })
        .collect();
    macros::with_store(|s| {
        vec![
            main,
            s.macros
                .iter()
                .map(|m| Item {
                    name: m.name.clone(),
                    keys: vec![m.key],
                })
                .collect(),
            s.commands
                .iter()
                .map(|c| Item {
                    name: c.name.clone(),
                    keys: vec![c.key],
                })
                .collect(),
            (0..SCINT.len())
                .map(|i| Item {
                    name: SCINT[i].0.into(),
                    keys: scint_keys(s, i),
                })
                .collect(),
            window_items(),
        ]
    })
}

// Port of ShortcutMapper::isFilterValid: each word must be in the name or in the shortcut.
fn matches(words: &[String], name: &str, keys: &str) -> bool {
    let (n, k) = (name.to_lowercase(), keys.to_lowercase());
    words.iter().all(|w| n.contains(w) || k.contains(w))
}

struct Row {
    idx: usize,
    name: String,
    key: String,
    cat: String,
    red: bool,
}

#[derive(Clone)]
struct Ui {
    window: Retained<NSWindow>,
    tabs: Retained<NSSegmentedControl>,
    table: Retained<NSTableView>,
    cat: Retained<NSTableColumn>,
    info: Retained<NSTextField>,
    filter: Retained<NSTextField>,
    buttons: [Retained<NSButton>; 3],
    _list: Retained<MapperList>,
}

struct Dlg {
    checks: [Retained<NSButton>; 4],
    key: Retained<NSPopUpButton>,
    list: Option<Retained<NSPopUpButton>>,
    status: Retained<NSTextField>,
    ok: Retained<NSButton>,
    keys: Vec<Key>,
    cur: usize,
    tab: usize,
    idx: usize,
    all: Vec<Vec<Item>>,
}

#[derive(Default)]
struct State {
    ui: RefCell<Option<Ui>>,
    tab: Cell<usize>,
    rows: RefCell<Vec<Row>>,
    dlg: RefCell<Option<Dlg>>,
}

thread_local! {
    static S: State = State::default();
}

fn ui() -> Option<Ui> {
    S.with(|s| s.ui.borrow().clone())
}

define_class!(
    // The rows of the Shortcut Mapper table, and the filter field delegate.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NppShortcutList"]
    struct MapperList;

    unsafe impl NSObjectProtocol for MapperList {}

    unsafe impl NSTableViewDataSource for MapperList {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _t: &NSTableView) -> isize {
            S.with(|s| s.rows.borrow().len() as isize)
        }

        #[unsafe(method_id(tableView:objectValueForTableColumn:row:))]
        fn value(&self, _t: &NSTableView, c: Option<&NSTableColumn>, row: isize) -> Option<Retained<AnyObject>> {
            let col = c.map(|c| c.identifier().to_string()).unwrap_or_default();
            let text = S.with(|s| {
                let rows = s.rows.borrow();
                let r = rows.get(usize::try_from(row).ok()?)?;
                Some(match col.as_str() {
                    "name" => r.name.clone(),
                    "key" => r.key.clone(),
                    _ => r.cat.clone(),
                })
            });
            text.map(|t| Retained::into_super(Retained::into_super(ns(&t))))
        }
    }

    unsafe impl NSControlTextEditingDelegate for MapperList {
        #[unsafe(method(controlTextDidChange:))]
        fn text_changed(&self, _n: &NSNotification) {
            fill(self.mtm());
        }
    }

    unsafe impl NSTextFieldDelegate for MapperList {}

    unsafe impl NSTableViewDelegate for MapperList {
        #[unsafe(method(tableView:willDisplayCell:forTableColumn:row:))]
        fn will_display(&self, _t: &NSTableView, cell: &AnyObject, _c: Option<&NSTableColumn>, row: isize) {
            let red = S.with(|s| {
                usize::try_from(row)
                    .ok()
                    .and_then(|r| s.rows.borrow().get(r).map(|r| r.red))
                    .unwrap_or(false)
            });
            let color = if red { NSColor::systemRedColor() } else { NSColor::labelColor() };
            let _: () = unsafe { msg_send![cell, setTextColor: &*color] };
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, _n: &NSNotification) {
            show_info(self.mtm());
        }
    }
);

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

fn selected() -> Option<(usize, usize)> {
    let ui = ui()?;
    let row = usize::try_from(ui.table.selectedRow()).ok()?;
    let idx = S.with(|s| s.rows.borrow().get(row).map(|r| r.idx))?;
    Some((S.with(|s| s.tab.get()), idx))
}

// Port of the BGN_ROWCHANGED handler: the conflicts of the selected item.
fn show_info(mtm: MainThreadMarker) {
    let Some(ui) = ui() else { return };
    let text = match selected() {
        None => String::new(),
        Some((tab, idx)) => {
            let all = all_items(&menu_rows(mtm));
            let lines: Vec<String> = all[tab][idx]
                .keys
                .iter()
                .flat_map(|k| conflicts(&all, tab, idx, k))
                .collect();
            if lines.is_empty() {
                "No shortcut conflicts for this item.".into()
            } else {
                lines.join("\n")
            }
        }
    };
    ui.info.setStringValue(&ns(&text));
}

// Port of ShortcutMapper::fillOutBabyGrid.
fn fill(mtm: MainThreadMarker) {
    let Some(ui) = ui() else { return };
    let tab = S.with(|s| s.tab.get());
    let menu = menu_rows(mtm);
    let all = all_items(&menu);
    let words: Vec<String> = ui
        .filter
        .stringValue()
        .to_string()
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let rows: Vec<Row> = all[tab]
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let key = keys_text(&it.keys);
            matches(&words, &it.name, &key).then(|| Row {
                idx: i,
                name: it.name.clone(),
                key,
                cat: if tab == 0 { menu[i].cat.clone() } else { String::new() },
                red: it.keys.iter().any(|k| !conflicts(&all, tab, i, k).is_empty()),
            })
        })
        .collect();
    let n = rows.len();
    S.with(|s| *s.rows.borrow_mut() = rows);
    let any = !all[tab].is_empty();
    let on = [any, any && tab < 3, any && (tab == 1 || tab == 2)];
    for (b, e) in ui.buttons.iter().zip(on) {
        b.setEnabled(e);
    }
    ui.cat.setHidden(tab != 0);
    let keep = ui.table.selectedRow().max(0) as usize;
    ui.table.reloadData();
    if n > 0 {
        let r = keep.min(n - 1);
        ui.table
            .selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(r), false);
        ui.table.scrollRowToVisible(r as isize);
    }
    show_info(mtm);
}

fn build(app: &App) -> Ui {
    let mtm = app.mtm();
    let t: &AnyObject = app;
    let w = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect(0., 0., 720., 560.),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { w.setReleasedWhenClosed(false) };
    w.setTitle(&ns("Shortcut mapper"));
    w.setMinSize(NSSize::new(560., 400.));
    let content = w.contentView().unwrap();
    let labels: Vec<Retained<NSString>> = TABS.iter().map(|n| ns(n)).collect();
    let tabs = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &NSArray::from_retained_slice(&labels),
            NSSegmentSwitchTracking::SelectOne,
            Some(t),
            Some(sel!(shortcutMapperTab:)),
            mtm,
        )
    };
    tabs.setSelectedSegment(0);
    tabs.setFrame(rect(10., 522., 700., 28.));
    tabs.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewMinYMargin);
    content.addSubview(&tabs);
    let table = NSTableView::new(mtm);
    for (id, title, width) in [("name", "Name", 330.), ("key", "Shortcut", 200.), ("cat", "Category", 120.)] {
        let col = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns(id));
        col.setTitle(&ns(title));
        col.setWidth(width);
        col.setEditable(false);
        table.addTableColumn(&col);
    }
    let cat = table.tableColumns().objectAtIndex(2);
    let list: Retained<MapperList> = unsafe { msg_send![MapperList::alloc(mtm), init] };
    unsafe {
        table.setDataSource(Some(ProtocolObject::from_ref(&*list)));
        table.setDelegate(Some(ProtocolObject::from_ref(&*list)));
        table.setTarget(Some(t));
        table.setDoubleAction(Some(sel!(shortcutMapperModify:)));
    }
    table.setUsesAlternatingRowBackgroundColors(true);
    let scroll = NSScrollView::new(mtm);
    scroll.setDocumentView(Some(&table));
    scroll.setHasVerticalScroller(true);
    scroll.setFrame(rect(10., 150., 700., 364.));
    scroll.setAutoresizingMask(
        objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
            | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    content.addSubview(&scroll);
    let info = NSTextField::wrappingLabelWithString(&NSString::new(), mtm);
    info.setSelectable(true);
    info.setFrame(rect(10., 80., 700., 64.));
    info.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);
    content.addSubview(&info);
    let label = NSTextField::labelWithString(&ns("Filter:"), mtm);
    label.setFrame(rect(10., 50., 44., 20.));
    content.addSubview(&label);
    let filter = NSTextField::textFieldWithString(&NSString::new(), mtm);
    filter.setFrame(rect(56., 48., 620., 22.));
    filter.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);
    unsafe { filter.setDelegate(Some(ProtocolObject::from_ref(&*list))) };
    content.addSubview(&filter);
    let button = |title: &str, x: f64, w: f64, target: &AnyObject, a: Sel| {
        let b = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(target), Some(a), mtm) };
        b.setFrame(rect(x, 10., w, 28.));
        content.addSubview(&b);
        b
    };
    let clear = button("\u{2715}", 680., 30., t, sel!(shortcutMapperFilterClear:));
    clear.setFrame(rect(680., 46., 30., 26.));
    clear.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewMinXMargin);
    let buttons = [
        button("Modify", 220., 90., t, sel!(shortcutMapperModify:)),
        button("Clear", 316., 90., t, sel!(shortcutMapperClear:)),
        button("Delete", 412., 90., t, sel!(shortcutMapperDelete:)),
    ];
    let close = button("Close", 508., 90., &w, sel!(performClose:));
    close.setKeyEquivalent(&ns("\u{1b}"));
    Ui {
        window: w,
        tabs,
        table,
        cat,
        info,
        filter,
        buttons,
        _list: list,
    }
}

fn dlg_key(d: &Dlg) -> Key {
    let on = |i: usize| crate::panel::on(&d.checks[i]);
    let names = key_names();
    let k = usize::try_from(d.key.indexOfSelectedItem()).ok().and_then(|i| names.get(i)).map_or(0, |n| n.1);
    Key {
        ctrl: on(0),
        alt: on(1),
        shift: on(2),
        meta: on(3),
        key: k,
    }
}

fn dlg_show(d: &Dlg, k: &Key) {
    for (b, v) in d.checks.iter().zip([k.ctrl, k.alt, k.shift, k.meta]) {
        crate::panel::set_on(b, v);
    }
    let i = key_names().iter().position(|n| n.1 == k.key).unwrap_or(0);
    d.key.selectItemAtIndex(i as isize);
}

fn dlg_list(d: &Dlg) {
    if let Some(l) = &d.list {
        l.removeAllItems();
        for k in &d.keys {
            let t = key_text(k);
            l.addItemWithTitle(&ns(if t.is_empty() { "None" } else { &t }));
        }
        l.selectItemAtIndex(d.cur as isize);
    }
}

// Port of Shortcut::updateConflictState and the IDC_WARNING_STATIC rule.
fn dlg_state(d: &Dlg) {
    let k = dlg_key(d);
    let text = if k.key == 0 {
        if d.list.is_some() {
            "This will remove shortcut from this command"
        } else {
            "This will disable the accelerator"
        }
    } else if system_key(&k) {
        "macOS uses this shortcut"
    } else if !conflicts(&d.all, d.tab, d.idx, &k).is_empty() {
        "CONFLICT FOUND!"
    } else {
        ""
    };
    d.status.setStringValue(&ns(text));
    d.ok.setEnabled(valid(&k));
}

impl App {
    pub(crate) fn mapper_show(&self) {
        if ui().is_none() {
            let u = build(self);
            S.with(|s| *s.ui.borrow_mut() = Some(u));
        }
        fill(self.mtm());
        if let Some(u) = ui() {
            u.window.center();
            u.window.makeKeyAndOrderFront(None);
        }
    }

    pub(crate) fn mapper_tab(&self) {
        let Some(u) = ui() else { return };
        S.with(|s| s.tab.set(u.tabs.selectedSegment().clamp(0, 3) as usize));
        unsafe { u.table.deselectAll(None) };
        fill(self.mtm());
    }

    pub(crate) fn mapper_filter_clear(&self) {
        let Some(u) = ui() else { return };
        u.filter.setStringValue(&NSString::new());
        u.window.makeFirstResponder(Some(&u.filter));
        fill(self.mtm());
    }

    // Applies the shortcuts.xml keys at start, like NppParameters::feedShortcut and feedScintKeys.
    pub(crate) fn mapper_start(&self) {
        apply_rows(&menu_rows(self.mtm()));
        for t in self.ivars().tabs.borrow().iter() {
            self.mapper_arm(&t.view);
        }
    }

    // Gives a Scintilla view the Scintilla keys of shortcuts.xml; with none, Scintilla keeps its own keys.
    pub(crate) fn mapper_arm(&self, v: &NSView) {
        let Some(map) = macros::with_store(|s| (!s.scint.is_empty()).then(|| keymap(s))) else {
            return;
        };
        sci::send(v, SCI_CLEARALLCMDKEYS, 0, 0);
        for (def, msg) in map {
            sci::send(v, SCI_ASSIGNCMDKEY, def, msg as isize);
        }
    }

    fn mapper_dialog(&self, tab: usize, idx: usize, all: Vec<Vec<Item>>) -> Option<(String, Vec<Key>)> {
        let mtm = self.mtm();
        let t: &AnyObject = self;
        let it = all[tab][idx].clone();
        let scint = tab == 3;
        let a = NSAlert::new(mtm);
        a.setMessageText(&ns("Shortcut"));
        let v = NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., 400., if scint { 150. } else { 120. }));
        let top = if scint { 150. } else { 120. };
        let at = |view: &NSView, x: f64, y: f64, w: f64, h: f64| {
            view.setFrame(rect(x, top - y - h, w, h));
            v.addSubview(view);
        };
        let label = |s: &str, x: f64, y: f64, w: f64| at(&NSTextField::labelWithString(&ns(s), mtm), x, y + 3., w, 18.);
        label("Name:", 0., 0., 50.);
        let name = NSTextField::textFieldWithString(&ns(&it.name), mtm);
        let rename = tab == 1 || tab == 2;
        name.setEditable(rename);
        name.setSelectable(true);
        at(&name, 54., 0., 340., 22.);
        let mut y = 34.;
        let list = scint.then(|| {
            label("Keys:", 0., y, 50.);
            let l = NSPopUpButton::new(mtm);
            unsafe {
                l.setTarget(Some(t));
                l.setAction(Some(sel!(shortcutKeyChanged:)));
            }
            l.setTag(2);
            at(&l, 54., y, 180., 26.);
            for (title, tag, x) in [("Add", 3, 240.), ("Remove", 4, 318.)] {
                let b = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(t), Some(sel!(shortcutKeyChanged:)), mtm) };
                b.setTag(tag);
                at(&b, x, y, 76., 28.);
            }
            y += 34.;
            l
        });
        let checks = ["Cmd (Ctrl)", "Option (Alt)", "Shift", "Control"].map(|n| {
            let b = unsafe { NSButton::checkboxWithTitle_target_action(&ns(n), Some(t), Some(sel!(shortcutKeyChanged:)), mtm) };
            b.setTag(1);
            b
        });
        for (i, b) in checks.iter().enumerate() {
            at(b, (i % 2) as f64 * 110., y + (i / 2) as f64 * 22., 108., 20.);
        }
        label("+", 222., y + 10., 14.);
        let key = NSPopUpButton::new(mtm);
        for n in key_names() {
            key.addItemWithTitle(&ns(&n.0));
        }
        unsafe {
            key.setTarget(Some(t));
            key.setAction(Some(sel!(shortcutKeyChanged:)));
        }
        key.setTag(1);
        at(&key, 240., y + 8., 154., 26.);
        let status = NSTextField::labelWithString(&NSString::new(), mtm);
        status.setTextColor(Some(&NSColor::systemRedColor()));
        at(&status, 0., y + 50., 394., 18.);
        a.setAccessoryView(Some(&v));
        let ok = a.addButtonWithTitle(&ns("OK"));
        a.addButtonWithTitle(&ns("Cancel"));
        let keys = if it.keys.is_empty() { vec![Key::default()] } else { it.keys.clone() };
        let d = Dlg {
            checks,
            key,
            list,
            status,
            ok,
            keys,
            cur: 0,
            tab,
            idx,
            all,
        };
        dlg_show(&d, &d.keys[0]);
        dlg_list(&d);
        dlg_state(&d);
        S.with(|s| *s.dlg.borrow_mut() = Some(d));
        a.layout();
        let done = a.runModal() == NSAlertFirstButtonReturn;
        let d = S.with(|s| s.dlg.borrow_mut().take())?;
        if !done {
            return None;
        }
        let mut k = dlg_key(&d);
        if k.key == 0 {
            k = Key::default();
        }
        let keys = if scint {
            let mut v = d.keys.clone();
            v[d.cur] = k;
            v.retain(|k| k.key != 0);
            if v.is_empty() { vec![Key::default()] } else { v }
        } else {
            vec![k]
        };
        let n = name.stringValue().to_string().trim().to_string();
        Some((if n.is_empty() { it.name } else { n }, keys))
    }

    // Port of the Shortcut and ScintillaKeyMap dialog controls; the tag tells which control changed.
    pub(crate) fn mapper_key_changed(&self, tag: isize) {
        S.with(|s| {
            let mut g = s.dlg.borrow_mut();
            let Some(d) = g.as_mut() else { return };
            match tag {
                2 => {
                    d.cur = usize::try_from(d.list.as_ref().map_or(0, |l| l.indexOfSelectedItem()))
                        .unwrap_or(0)
                        .min(d.keys.len() - 1);
                    let k = d.keys[d.cur];
                    dlg_show(d, &k);
                }
                3 => {
                    let k = dlg_key(d);
                    if k.key != 0 {
                        // Port of ScintillaKeyMap::addKeyCombo.
                        d.cur = match d.keys.iter().position(|x| *x == k) {
                            Some(i) => i,
                            None if d.keys.iter().all(|x| x.key == 0) => {
                                d.keys = vec![k];
                                0
                            }
                            None => {
                                d.keys.push(k);
                                d.keys.len() - 1
                            }
                        };
                    }
                }
                4 if d.keys.len() > 1 => {
                    d.keys.remove(d.cur);
                    d.cur = d.cur.min(d.keys.len() - 1);
                    let k = d.keys[d.cur];
                    dlg_show(d, &k);
                }
                _ => {
                    if d.list.is_some() {
                        d.keys[d.cur] = dlg_key(d);
                    }
                }
            }
            dlg_list(d);
            dlg_state(d);
        });
    }

    // Port of the IDM_BABYGRID_MODIFY and IDM_BABYGRID_CLEAR handlers.
    fn mapper_change(&self, clear: bool) {
        let mtm = self.mtm();
        let Some((tab, idx)) = selected() else { return };
        if clear && tab == 3 {
            return;
        }
        let menu = menu_rows(mtm);
        let all = all_items(&menu);
        let Some(old) = all.get(tab).and_then(|v| v.get(idx)).cloned() else {
            return;
        };
        let new = if clear {
            (old.name.clone(), vec![Key::default()])
        } else {
            match self.mapper_dialog(tab, idx, all) {
                Some(n) => n,
                None => return,
            }
        };
        let none = |k: &[Key]| k.iter().all(|k| k.key == 0);
        if new.0 == old.name && (new.1 == old.keys || none(&new.1) && none(&old.keys)) {
            return;
        }
        let (name, keys) = new;
        let k = keys[0];
        macros::with_store(|s| match tab {
            0 => {
                let id = menu[idx].id;
                s.internal.retain(|c| !(c.id == id && c.nth == 0));
                s.internal.push(Internal { id, nth: 0, key: k });
            }
            1 => {
                if let Some(m) = s.macros.get_mut(idx) {
                    (m.name, m.key) = (name, k);
                }
            }
            2 => {
                if let Some(c) = s.commands.get_mut(idx) {
                    (c.name, c.key) = (name, k);
                }
            }
            _ => {
                let (_, id, menu_id) = SCINT[idx];
                s.scint.retain(|c| !(c.id == id && c.menu_id == menu_id));
                s.scint.push(ScintKey { id, menu_id, keys });
            }
        });
        if tab == 0 {
            menu[idx].items.iter().for_each(|it| set_command_key(menu[idx].id, it, &k));
        }
        if tab == 3 {
            for t in self.ivars().tabs.borrow().iter() {
                self.mapper_arm(&t.view);
            }
        }
        self.store_changed();
        fill(mtm);
    }

    pub(crate) fn mapper_modify(&self) {
        self.mapper_change(false);
    }

    pub(crate) fn mapper_clear(&self) {
        self.mapper_change(true);
    }

    // Port of the IDM_BABYGRID_DELETE handler.
    pub(crate) fn mapper_delete(&self) {
        let Some((tab @ 1..=2, idx)) = selected() else { return };
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&ns("Are you sure?"));
        a.setInformativeText(&ns("Are you sure you want to delete this shortcut?"));
        a.addButtonWithTitle(&ns("OK"));
        a.addButtonWithTitle(&ns("Cancel"));
        if a.runModal() != NSAlertFirstButtonReturn {
            return;
        }
        macros::with_store(|s| {
            if tab == 1 && idx < s.macros.len() {
                s.macros.remove(idx);
            } else if tab == 2 && idx < s.commands.len() {
                s.commands.remove(idx);
            }
        });
        self.store_changed();
        fill(self.mtm());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const IDS: &str = include_str!("../../PowerEditor/src/menuCmdID.h");
    const SCI_H: &str = include_str!("../../scintilla/include/Scintilla.h");

    fn id_of(n: &str) -> i32 {
        let l = IDS
            .lines()
            .find(|l| l.split_whitespace().take(2).eq(["#define", n]))
            .and_then(|l| l.split("//").next())
            .unwrap_or_else(|| panic!("{n}"));
        let v: String = l.split_whitespace().skip(2).collect();
        let v = v.trim_matches(|c| c == '(' || c == ')');
        match v.split_once('+') {
            Some((b, o)) => b.parse().unwrap_or_else(|_| id_of(b)) + o.parse::<i32>().unwrap(),
            None => v.parse().unwrap_or_else(|_| id_of(v)),
        }
    }

    fn sci(n: &str) -> i32 {
        let p = format!("#define {n} ");
        SCI_H
            .lines()
            .find_map(|l| l.strip_prefix(&p))
            .unwrap_or_else(|| panic!("{n}"))
            .trim()
            .parse()
            .unwrap()
    }

    fn k(mods: &str, key: u8) -> Key {
        Key {
            ctrl: mods.contains('c'),
            alt: mods.contains('a'),
            shift: mods.contains('s'),
            meta: mods.contains('m'),
            key,
        }
    }

    #[test]
    fn key_names_match_notepad_plus_plus() {
        let src = include_str!("../../PowerEditor/src/WinControls/shortcut/shortcut.cpp");
        let a = &src[src.find("KeyIDNAME namedKeyArray[] = {").unwrap()..];
        let a = &a[..a.find("};").unwrap()];
        let names: Vec<String> = a
            .lines()
            .filter_map(|l| l.trim().strip_prefix("{\""))
            .filter_map(|l| l.split_once("\", ").map(|x| x.0.replace("\\\\", "\\")))
            .filter(|n| !n.is_empty())
            .collect();
        let ours: Vec<String> = key_names().iter().map(|n| n.0.clone()).collect();
        assert_eq!(ours, names);
        let keys = include_str!("../../PowerEditor/src/keys.h");
        for (n, v) in key_names().iter().filter(|n| n.0.len() == 1 && n.0.as_bytes()[0].is_ascii_alphanumeric()) {
            assert!(keys.contains(&format!("#define VK_{n}              0x{v:02X}")), "{n}");
        }
    }

    #[test]
    fn menu_key_translation() {
        for (n, vk) in key_names().iter().filter(|n| !matches!(n.0.as_str(), "None" | "<>")) {
            let (c, pad) = menu_key(*vk).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(key_of_menu(c, pad), Some((*vk, false)), "{n}");
        }
        assert_eq!(menu_key(b'N'), Some(('n', false)));
        assert_eq!(menu_key(0x74), Some(('\u{F708}', false)));
        assert_eq!(menu_key(0x61), Some(('1', true)));
        assert_eq!(menu_key(0xE2), None);
        assert_eq!(key_of_menu('S', false), Some((b'S', true)));
        assert_eq!(key_of_menu('+', false), Some((0xBB, true)));
        assert_eq!(key_of_menu('?', false), Some((0xBF, true)));
        assert_eq!(key_of_menu('\u{7f}', false), Some((0x08, false)));
        assert_eq!(key_of_menu('\u{8}', false), Some((0x08, false)));
        assert_eq!(key_of_menu('\u{F70F}', false), Some((0x7B, false)));
        assert_eq!(key_of_menu('é', false), None);
    }

    #[test]
    fn scintilla_key_translation() {
        for (n, vk) in key_names().iter().filter(|n| !matches!(n.0.as_str(), "None" | "<>")) {
            let c = sci_key(*vk).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(key_of_sci(c).and_then(sci_key), Some(c), "{n}");
        }
        assert_eq!(sci_key(b'A'), Some('a' as i32));
        assert_eq!(sci_key(0x25), Some(sci("SCK_LEFT")));
        assert_eq!(sci_key(0x6B), Some(sci("SCK_ADD")));
        assert_eq!(sci_def(&k("cs", b'L')), Some(('L' as usize) | 3 << 16));
        assert_eq!(sci_def(&k("m", 0x25)), Some(302 | 16 << 16));
        assert_eq!(key_from_sci(302, 17), Some(k("ms", 0x25)));
    }

    #[test]
    fn tables_match_sources() {
        let p = include_str!("../../PowerEditor/src/Parameters.cpp");
        let b = &p[p.find("scintKeyDefs[]").unwrap()..];
        let b = &b[..b.find("};").unwrap()];
        let rows: Vec<(String, i32, i32)> = b
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("{L\""))
            .filter_map(|l| {
                let f: Vec<&str> = l.trim_matches(|c| c == '{' || c == '}' || c == ',').split(',').map(str::trim).collect();
                let name = f[0].trim_start_matches("L\"").trim_end_matches('"');
                let menu = if f[6] == "0" { 0 } else { id_of(f[6]) };
                (!name.is_empty()).then(|| (name.to_string(), sci(f[1]), menu))
            })
            .collect();
        let ours: Vec<(String, i32, i32)> = SCINT.iter().map(|r| (r.0.into(), r.1, r.2)).collect();
        assert_eq!(ours, rows);
        let m = include_str!("../../scintilla/cocoa/ScintillaCocoa.mm");
        let b = &m[m.find("macMapDefault[] = {").unwrap()..];
        let b = &b[..b.find("{Key(0)").unwrap()];
        let mods = |n: &str| match n {
            "SCI_NORM" => 0,
            "SCI_SHIFT" => 1,
            "SCI_CTRL" | "SCI_CMD" => 2,
            "SCI_CSHIFT" | "SCI_SCMD" => 3,
            "SCI_ALT" => 4,
            "SCI_ASHIFT" => 5,
            "SCI_META" => 16,
            "SCI_SMETA" => 17,
            _ => panic!("{n}"),
        };
        let mac: Vec<(i32, i32, i32)> = b
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with('{'))
            .map(|l| {
                let f: Vec<&str> = l.trim_matches(|c| c == '{' || c == '}' || c == ',').rsplitn(3, ',').map(str::trim).collect();
                let key = match f[2].strip_prefix("Keys::") {
                    Some(n) => sci(&format!("SCK_{}", n.to_uppercase())),
                    None => f[2].trim_start_matches("Key('").trim_end_matches("')").replace("\\\\", "\\").chars().next().unwrap() as i32,
                };
                let msg = f[0].trim_start_matches("Message::");
                (key, mods(f[1]), sci(&format!("SCI_{}", msg.to_uppercase())))
            })
            .collect();
        assert_eq!(mac, MAC_KEYS);
    }

    fn final_map(v: &[(usize, i32)]) -> HashMap<usize, i32> {
        v.iter().copied().collect()
    }

    #[test]
    fn default_keymap_is_the_scintilla_one() {
        let s = Shortcuts::default();
        // Shift with a letter or a symbol gives the shifted character, so Cmd+Shift+L is 'L', not the 'l' of Scintilla.
        let scintilla: Vec<(usize, i32)> = MAC_KEYS
            .iter()
            .map(|m| (key_from_sci(m.0, m.1).and_then(|k| sci_def(&k)).unwrap(), m.2))
            .collect();
        assert_eq!(final_map(&keymap(&s)), final_map(&scintilla));
        assert_eq!(scint_keys(&s, 35), vec![k("a", 0x25), k("m", 0x25)]);
        assert!(scint_keys(&s, 2).is_empty());
    }

    #[test]
    fn scintilla_overrides() {
        let mut s = Shortcuts::default();
        s.scint.push(ScintKey {
            id: 2337,
            menu_id: 0,
            keys: vec![k("c", b'K')],
        });
        s.scint.push(ScintKey {
            id: 2342,
            menu_id: 0,
            keys: vec![k("c", 0x28), k("cs", 0x28)],
        });
        s.scint.push(ScintKey {
            id: 2004,
            menu_id: 7,
            keys: vec![k("c", b'Q')],
        });
        let m = final_map(&keymap(&s));
        assert_eq!(m.get(&('k' as usize | 2 << 16)), Some(&2337));
        assert_eq!(m.get(&('l' as usize | 2 << 16)), None);
        assert_eq!(m.get(&(300 | 2 << 16)), Some(&2342));
        assert_eq!(m.get(&(300 | 16 << 16)), None);
        assert_eq!(m.get(&(300 | 3 << 16)), Some(&2342));
        assert_eq!(m.get(&('q' as usize | 2 << 16)), None);
        assert_eq!(m.get(&('x' as usize | 2 << 16)), Some(&2177));
    }

    #[test]
    fn conflict_detection() {
        let item = |n: &str, keys: Vec<Key>| Item {
            name: n.into(),
            keys,
        };
        let all = vec![
            vec![item("New", vec![k("c", b'N')]), item("Open", vec![k("c", b'O')])],
            vec![item("m1", vec![k("c", b'N')]), item("m2", vec![Key::default()])],
            vec![],
            vec![item("SCI_WORDLEFT", vec![k("a", 0x25), k("m", 0x25)])],
        ];
        assert_eq!(conflicts(&all, 1, 0, &k("c", b'N')), vec!["Main menu  |  1   New  ( Cmd+N )"]);
        assert_eq!(conflicts(&all, 0, 0, &k("c", b'N')), vec!["Macros  |  1   m1  ( Cmd+N )"]);
        assert!(conflicts(&all, 0, 1, &k("c", b'O')).is_empty());
        assert!(conflicts(&all, 0, 1, &k("cs", b'O')).is_empty());
        assert!(conflicts(&all, 1, 1, &Key::default()).is_empty());
        assert_eq!(
            conflicts(&all, 0, 0, &k("m", 0x25)),
            vec!["Scintilla commands  |  1*   SCI_WORDLEFT  ( Control+Left )"]
        );
        assert!(conflicts(&all, 3, 0, &k("m", 0x25)).is_empty());
    }

    #[test]
    fn key_text_and_validity() {
        assert_eq!(key_text(&k("cs", b'S')), "Cmd+Shift+S");
        assert_eq!(key_text(&k("ma", 0x74)), "Control+Option+F5");
        assert_eq!(key_text(&k("c", 0)), "");
        assert_eq!(key_text(&k("c", 0x07)), "Cmd+Unlisted");
        assert_eq!(keys_text(&[k("a", 0x25), Key::default(), k("m", 0x25)]), "Option+Left / Control+Left");
        assert!(!valid(&k("", b'A')));
        assert!(!valid(&k("s", b'7')));
        assert!(valid(&k("a", b'A')));
        assert!(valid(&k("m", 0x20)));
        assert!(valid(&k("", 0x74)));
        assert!(valid(&Key::default()));
        let w = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert!(matches(&w("save cmd"), "Save As...", "Cmd+Option+S"));
        assert!(matches(&w("as +s"), "Save As...", "Cmd+Option+S"));
        assert!(!matches(&w("save shift"), "Save As...", "Cmd+Option+S"));
        assert!(matches(&[], "x", ""));
    }

    #[test]
    fn command_ids() {
        let table = command_table();
        assert_eq!(find_id(&table, "zoom:", 1), Some(44023));
        assert_eq!(find_id(&table, "zoom:", -1), Some(44024));
        assert_eq!(find_id(&table, "zoom:", 0), Some(44033));
        assert_eq!(find_id(&table, "openLink:", 3), Some(47004));
        assert_eq!(find_id(&table, "macroToggleRecord:", 1), Some(42019));
        assert_eq!(find_id(&table, "newDocument:", 7), Some(41001));
        assert_eq!(find_id(&table, "showDebugInfo:", 0), Some(47012));
        assert_eq!(find_id(&table, "restoreRecentClosed:", 9), Some(41021));
        let src = include_str!("macros.rs");
        assert!(src.contains("(\"Start Recording\", sel!(macroToggleRecord:), \"\"),\n        (\"Stop Recording\", sel!(macroToggleRecord:), \"\"),"));
        assert!(src.contains("it.setTag(i as isize);"));
        assert!(include_str!("tools.rs").contains("Notepad++ Community (Forum)\",\n        \"https://community"));
    }

    fn equiv(mods: &str, key: u8) -> Option<(char, [bool; 5])> {
        menu_equiv(&k(mods, key))
    }

    #[test]
    fn menu_key_equivalents() {
        let f = |c: bool, o: bool, s: bool, m: bool, p: bool| [c, o, s, m, p];
        assert_eq!(equiv("cs", b'S'), Some(('S', f(true, false, false, false, false))));
        assert_eq!(equiv("c", b'S'), Some(('s', f(true, false, false, false, false))));
        assert_eq!(equiv("cs", 0xBB), Some(('+', f(true, false, false, false, false))));
        assert_eq!(equiv("cs", b'7'), Some(('&', f(true, false, false, false, false))));
        assert_eq!(equiv("c", 0x08), Some(('\u{7f}', f(true, false, false, false, false))));
        assert_eq!(equiv("s", 0x74), Some(('\u{F708}', f(false, false, true, false, false))));
        assert_eq!(equiv("cs", 0x25), Some(('\u{F702}', f(true, false, true, false, false))));
        assert_eq!(equiv("s", 0x61), Some(('1', f(false, false, true, false, true))));
        assert_eq!(equiv("c", 0xE2), None);
        for (n, vk) in key_names().iter().filter(|n| !matches!(n.0.as_str(), "None" | "<>")) {
            for mods in ["c", "cs", "as", "ms"] {
                let key = k(mods, *vk);
                let (c, f) = menu_equiv(&key).unwrap();
                let (v, shift) = key_of_menu(c, f[4]).unwrap();
                assert_eq!((v, shift || f[2]), (*vk, key.shift), "{n} {mods}");
            }
        }
    }

    #[test]
    fn scintilla_shift_keys() {
        assert_eq!(sci_def(&k("cs", b'K')), Some('K' as usize | 3 << 16));
        assert_eq!(sci_def(&k("c", b'K')), Some('k' as usize | 2 << 16));
        assert_eq!(sci_def(&k("cs", 0xDB)), Some('{' as usize | 3 << 16));
        assert_eq!(sci_def(&k("s", 0x25)), Some(302 | 1 << 16));
        assert_eq!(sci_def(&k("s", 0x6E)), Some('.' as usize | 1 << 16));
        assert_eq!(key_from_sci('L' as i32, 3), Some(k("cs", b'L')));
    }

    #[test]
    fn numpad_and_system_keys() {
        let all = vec![vec![Item {
            name: "One".into(),
            keys: vec![k("c", b'1')],
        }], vec![], vec![], vec![], vec![Item {
            name: "Windows...".into(),
            keys: vec![k("cs", b'W')],
        }]];
        assert_eq!(conflicts(&all, 1, 0, &k("c", 0x61)), vec!["Main menu  |  1   One  ( Cmd+1 )"]);
        assert_eq!(conflicts(&all, 1, 0, &k("cs", b'W')), vec!["Window menu  |  1   Windows...  ( Cmd+Shift+W )"]);
        assert!(conflicts(&all, 1, 0, &k("cs", b'1')).is_empty());
        for bad in [k("c", 0x09), k("cs", 0x09), k("c", 0x20), k("cs", b'3'), k("cs", b'4'), k("cs", b'5')] {
            assert!(system_key(&bad) && !valid(&bad), "{bad:?}");
        }
        for ok in [k("ca", 0x09), k("cs", b'6'), k("m", 0x20), k("c", b'3')] {
            assert!(!system_key(&ok) && valid(&ok), "{ok:?}");
        }
    }
}
