// SPDX-License-Identifier: GPL-3.0-or-later
use crate::panel::Form;
use crate::{ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertSecondButtonReturn, NSButton, NSFont, NSMenuItem, NSModalResponseOK,
    NSOpenPanel, NSPasteboard, NSPasteboardTypeString, NSTextView, NSWorkspace,
};
use objc2_foundation::{NSProcessInfo, NSURL};
use std::cell::{Cell, OnceCell};
use std::ffi::c_void;

const SCI_GETSELECTIONS: u32 = 2570;

#[link(name = "System")]
extern "C" {
    fn CC_MD5(data: *const c_void, len: u32, md: *mut u8) -> *mut u8;
    fn CC_SHA1(data: *const c_void, len: u32, md: *mut u8) -> *mut u8;
    fn CC_SHA256(data: *const c_void, len: u32, md: *mut u8) -> *mut u8;
    fn CC_SHA512(data: *const c_void, len: u32, md: *mut u8) -> *mut u8;
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hash {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

pub const HASHES: [(&str, Hash); 4] = [
    ("MD5", Hash::Md5),
    ("SHA-1", Hash::Sha1),
    ("SHA-256", Hash::Sha256),
    ("SHA-512", Hash::Sha512),
];

fn tag_hash(t: isize) -> Hash {
    HASHES.get(t as usize).map_or(Hash::Md5, |h| h.1)
}

fn name(h: Hash) -> &'static str {
    HASHES.iter().find(|x| x.1 == h).unwrap().0
}

// Lower case hex digest, or None when the data is longer than CommonCrypto takes in one call.
pub fn hex(h: Hash, data: &[u8]) -> Option<String> {
    let len = u32::try_from(data.len()).ok()?;
    let mut md = [0u8; 64];
    let (f, n): (
        unsafe extern "C" fn(*const c_void, u32, *mut u8) -> *mut u8,
        usize,
    ) = match h {
        Hash::Md5 => (CC_MD5, 16),
        Hash::Sha1 => (CC_SHA1, 20),
        Hash::Sha256 => (CC_SHA256, 32),
        Hash::Sha512 => (CC_SHA512, 64),
    };
    unsafe { f(data.as_ptr().cast(), len, md.as_mut_ptr()) };
    Some(md[..n].iter().map(|b| format!("{b:02x}")).collect())
}

// Notepad++ hashes a C string, so the data stops at the first NUL byte.
pub fn hex_c(h: Hash, data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    hex(h, &data[..end]).unwrap_or_default()
}

// Port of HashFromTextDlg::generateHashPerLine: an empty line gives an empty result line.
pub fn per_line(h: Hash, text: &str) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        let l = line.trim_end_matches('\n').trim_end_matches('\r');
        if !l.is_empty() {
            out += &hex_c(h, l.as_bytes());
        }
        out.push('\n');
    }
    out
}

fn define<'a>(src: &'a str, name: &str) -> &'a str {
    src.lines()
        .find_map(|l| {
            l.strip_prefix("#define ")?
                .strip_prefix(name)?
                .strip_prefix(' ')
        })
        .and_then(|v| Some(&v[v.find('"')? + 1..v.rfind('"')?]))
        .unwrap_or("N/A")
}

fn npp_version() -> &'static str {
    define(
        include_str!("../../PowerEditor/src/resource.h"),
        "NOTEPAD_PLUS_VERSION",
    )
}

fn sci_lex_version() -> String {
    format!(
        "{}/{}",
        define(
            include_str!("../../scintilla/win32/ScintRes.rc"),
            "VERSION_SCINTILLA"
        ),
        define(
            include_str!("../../lexilla/src/LexillaVersion.rc"),
            "VERSION_LEXILLA"
        )
    )
}

pub const APP_NAME: &str = "Notepad++ for macOS (unofficial)";

pub fn about_text() -> String {
    format!(
        "Version {}\n\
         A modified version of {} for macOS.\n\
         Not affiliated with or endorsed by the Notepad++ project.\n\n\
         Licence: GPL-3.0-or-later. This program comes with no warranty.\n\n\
         Credits:\n\
         Notepad++ by Don Ho and the Notepad++ contributors (https://notepad-plus-plus.org/).\n\
         Scintilla and Lexilla {} by Neil Hodgson and contributors (https://www.scintilla.org/).",
        env!("CARGO_PKG_VERSION"),
        npp_version(),
        sci_lex_version()
    )
}

pub const CMD_LINE_HELP: &str = "Usage:\n\n\
notepadpp-mac [filePath ...]\n\n\
filePath: file to open in a new tab (absolute or relative path name). Each file opens in its own tab.\n\n\
The app ignores arguments that start with \"-\". It does not support the Notepad++ options (-n, -l, -multiInst, -ro and others) or folders.";

pub fn debug_info(os: &str, path: &str, cmd: &str) -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "ARM 64-bit",
        "x86_64" => "64-bit",
        a => a,
    };
    format!(
        "{APP_NAME} v{}   ({arch})\n\
         Based on: {}\n\
         Scintilla/Lexilla included: {}\n\
         Boost Regex included: {}\n\
         Path: {path}\n\
         Command Line: {cmd}\n\
         OS Name: macOS\n\
         OS Version: {os}\n\
         OS Architecture: {}\n",
        env!("CARGO_PKG_VERSION"),
        npp_version(),
        sci_lex_version(),
        define(
            include_str!("../../boostregex/boost/version.hpp"),
            "BOOST_LIB_VERSION"
        ),
        std::env::consts::ARCH
    )
}

pub const LINKS: [(&str, &str); 4] = [
    ("Notepad++ Home", "https://notepad-plus-plus.org/"),
    (
        "Notepad++ Project Page",
        "https://github.com/notepad-plus-plus/notepad-plus-plus/",
    ),
    (
        "Notepad++ Online User Manual",
        "https://npp-user-manual.org/",
    ),
    (
        "Notepad++ Community (Forum)",
        "https://community.notepad-plus-plus.org/",
    ),
];

fn to_clipboard(s: &str) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    pb.setString_forType(&ns(s), unsafe { NSPasteboardTypeString });
}

struct HashUi {
    form: Form,
    hash: Cell<Hash>,
    check: Retained<NSButton>,
    choose: Retained<NSButton>,
    input: Retained<NSTextView>,
    result: Retained<NSTextView>,
}

thread_local! {
    static TEXT_UI: OnceCell<HashUi> = const { OnceCell::new() };
    static FILES_UI: OnceCell<HashUi> = const { OnceCell::new() };
}

fn text_box(f: &Form, mtm: MainThreadMarker, top: f64, editable: bool) -> Retained<NSTextView> {
    let s = NSTextView::scrollableTextView(mtm);
    f.place(&s, 16., top, 528., 110.);
    let t = s.documentView().unwrap().downcast::<NSTextView>().unwrap();
    t.setEditable(editable);
    t.setRichText(false);
    t.setAutomaticQuoteSubstitutionEnabled(false);
    t.setAutomaticDashSubstitutionEnabled(false);
    t.setAutomaticTextReplacementEnabled(false);
    t.setAutomaticSpellingCorrectionEnabled(false);
    t.setFont(NSFont::userFixedPitchFontOfSize(12.).as_deref());
    t
}

// Text dialog when `files` is false: a check box, the input box and the result box. Files dialog: a button, the path box and the result box.
fn hash_ui(app: &App, files: bool) -> HashUi {
    let mtm = app.mtm();
    let t: &AnyObject = app;
    let f = Form::new(mtm, "", 560., 310.);
    let check = f.check(
        "Treat each line as a separate string",
        16.,
        10.,
        400.,
        t,
        sel!(hashEachLine:),
    );
    let choose = f.button("", 12., 6., 300., t, sel!(hashChooseFiles:));
    check.setHidden(files);
    choose.setHidden(!files);
    let input = text_box(&f, mtm, 36., !files);
    let result = text_box(&f, mtm, 154., false);
    if !files {
        let _: () = unsafe { msg_send![&input, setDelegate: app] };
    }
    f.button("Copy to Clipboard", 404., 270., 140., t, sel!(hashCopy:))
        .setTag(files as isize);
    f.button("Close", 260., 270., 90., t, sel!(closePanel:))
        .setKeyEquivalent(&ns("\u{1b}"));
    HashUi {
        form: f,
        hash: Cell::new(Hash::Md5),
        check,
        choose,
        input,
        result,
    }
}

fn with_ui<R>(app: &App, files: bool, f: impl FnOnce(&HashUi) -> R) -> R {
    let cell = if files { &FILES_UI } else { &TEXT_UI };
    cell.with(|c| f(c.get_or_init(|| hash_ui(app, files))))
}

fn text(v: &NSTextView) -> String {
    v.string().to_string()
}

fn set_text(v: &NSTextView, s: &str) {
    v.setString(&ns(s));
}

impl App {
    pub(crate) fn hash_to_clipboard(&self, s: &NSMenuItem) {
        let Some(v) = self.editor() else { return };
        if sci::send(&v, SCI_GETSELECTIONS, 0, 0) != 1 {
            return;
        }
        let (a, b) = sci::selection(&v);
        if a < b {
            to_clipboard(&hex_c(tag_hash(s.tag()), &sci::doc(&v).range(a, b)));
        }
    }

    pub(crate) fn hash_show(&self, s: &NSMenuItem, files: bool) {
        let h = tag_hash(s.tag());
        with_ui(self, files, |u| {
            let n = name(h);
            if files {
                u.form
                    .panel
                    .setTitle(&ns(&format!("Generate {n} digest from files")));
                u.choose
                    .setTitle(&ns(&format!("Choose files to generate {n}...")));
                if u.hash.get() != h {
                    set_text(&u.input, "");
                    set_text(&u.result, "");
                }
            } else {
                u.form.panel.setTitle(&ns(&format!("Generate {n} digest")));
            }
            u.hash.set(h);
            if !files {
                self.hash_text_changed();
            }
            u.form.panel.makeKeyAndOrderFront(None);
            u.form
                .panel
                .makeFirstResponder(Some(if files { &*u.choose } else { &*u.input }));
        });
    }

    pub(crate) fn hash_text_changed(&self) {
        with_ui(self, false, |u| {
            let s = text(&u.input);
            let r = match (s.is_empty(), crate::panel::on(&u.check)) {
                (true, _) => String::new(),
                (false, true) => per_line(u.hash.get(), &s),
                (false, false) => hex_c(u.hash.get(), s.as_bytes()),
            };
            set_text(&u.result, &r);
        });
    }

    pub(crate) fn hash_choose_files(&self) {
        let p = NSOpenPanel::openPanel(self.mtm());
        p.setAllowsMultipleSelection(true);
        if p.runModal() != NSModalResponseOK {
            return;
        }
        with_ui(self, true, |u| {
            let (mut paths, mut out) = (String::new(), String::new());
            for path in p
                .URLs()
                .iter()
                .filter_map(|url| url.path())
                .map(|s| s.to_string())
            {
                // ponytail: whole file in memory, max 4 GiB; stream with CC_*_Update if larger files matter.
                let Some(d) = std::fs::read(&path)
                    .ok()
                    .and_then(|b| hex(u.hash.get(), &b))
                else {
                    continue;
                };
                let file = std::path::Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                paths += &format!("{path}\n");
                out += &format!("{d}  {file}\n");
            }
            if !out.is_empty() {
                set_text(&u.input, &paths);
                set_text(&u.result, &out);
            }
        });
    }

    pub(crate) fn hash_copy(&self, files: bool) {
        let s = with_ui(self, files, |u| text(&u.result));
        if !s.is_empty() {
            to_clipboard(&s);
        }
    }

    pub(crate) fn show_about(&self) {
        self.alert(APP_NAME, &about_text(), &["OK"]);
    }

    pub(crate) fn show_cmd_line_args(&self) {
        self.alert("Command Line Arguments", CMD_LINE_HELP, &["OK"]);
    }

    pub(crate) fn open_link(&self, s: &NSMenuItem) {
        if let Some(url) = LINKS
            .get(s.tag() as usize)
            .and_then(|l| NSURL::URLWithString(&ns(l.1)))
        {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }

    pub(crate) fn show_debug_info(&self) {
        let os = NSProcessInfo::processInfo()
            .operatingSystemVersionString()
            .to_string();
        let path = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let cmd = std::env::args().collect::<Vec<_>>().join(" ");
        let info = debug_info(&os, &path, &cmd);
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&ns("Debug Info"));
        a.setInformativeText(&ns(&info));
        a.addButtonWithTitle(&ns("OK"));
        a.addButtonWithTitle(&ns("Copy debug info into clipboard"));
        if a.runModal() == NSAlertSecondButtonReturn {
            to_clipboard(&info);
        }
    }
}

pub fn tools_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let items: [(&str, Sel); 3] = [
        ("Generate...", sel!(hashGenerate:)),
        ("Generate from files...", sel!(hashFromFiles:)),
        (
            "Generate from selection into clipboard",
            sel!(hashToClipboard:),
        ),
    ];
    HASHES
        .iter()
        .enumerate()
        .map(|(i, (n, _))| {
            let sub = items
                .iter()
                .map(|(title, a)| crate::tagged(mtm, title, *a, i as isize, t))
                .collect();
            crate::nested(mtm, n, sub)
        })
        .collect()
}

pub fn help_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let mut v = vec![
        crate::item(
            mtm,
            "Command Line Arguments...",
            sel!(showCmdLineArgs:),
            "",
            t,
        ),
        NSMenuItem::separatorItem(mtm),
    ];
    v.extend(
        LINKS
            .iter()
            .enumerate()
            .map(|(i, (n, _))| crate::tagged(mtm, n, sel!(openLink:), i as isize, t)),
    );
    v.push(NSMenuItem::separatorItem(mtm));
    v.push(crate::item(
        mtm,
        "Debug Info...",
        sel!(showDebugInfo:),
        "",
        t,
    ));
    v.push(crate::item(mtm, "About Notepad++", sel!(showAbout:), "", t));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(
            hex(Hash::Md5, b"").unwrap(),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            hex(Hash::Md5, b"abc").unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hex(Hash::Sha1, b"abc").unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(Hash::Sha256, b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(Hash::Sha512, b"abc").unwrap(),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            hex(Hash::Sha1, b"The quick brown fox jumps over the lazy dog").unwrap(),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        );
    }

    #[test]
    fn stops_at_nul() {
        assert_eq!(
            hex_c(Hash::Md5, b"abc\0def"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn each_line() {
        let abc = "900150983cd24fb0d6963f7d28e17f72";
        assert_eq!(
            per_line(Hash::Md5, "abc\n\nabc"),
            format!("{abc}\n\n{abc}\n")
        );
        assert_eq!(
            per_line(Hash::Md5, "abc\r\n\r\nabc\n"),
            format!("{abc}\n\n{abc}\n")
        );
    }

    #[test]
    fn versions_and_texts() {
        assert!(npp_version().starts_with("Notepad++ v"));
        assert!(sci_lex_version().chars().next().unwrap().is_ascii_digit());
        assert!(sci_lex_version().contains('/'));
        let a = about_text();
        assert!(a.contains("Not affiliated with or endorsed by the Notepad++ project"));
        assert!(
            a.contains("GPL-3.0-or-later")
                && a.contains("Scintilla")
                && a.contains(env!("CARGO_PKG_VERSION"))
        );
        let d = debug_info("Version 1", "/p", "x");
        assert!(d.contains("OS Version: Version 1") && d.contains("Boost Regex included: 1_"));
    }
}
