// SPDX-License-Identifier: GPL-3.0-or-later
use crate::sci::{self, send};
use crate::{nested, ns, tagged, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSMenuItem, NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSData;

const SCI_GETSELTEXT: u32 = 2161;
const SCI_REPLACESEL: u32 = 2170;
const SCI_ADDTEXT: u32 = 2001;
// Private pasteboard type for the raw bytes, like the Notepad++ "Notepad++ Binary Length" clipboard format.
const BINARY_TYPE: &str = "org.notepad-plus-plus.binary";

// Notepad_plus.rc "Paste Special" submenu.
pub fn paste_special_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    let mut items = crate::edit_extras::paste_markup_items(mtm, t);
    items.extend([
        tagged(mtm, "Copy Binary Content", sel!(copyBinary:), 0, t),
        tagged(mtm, "Cut Binary Content", sel!(copyBinary:), 1, t),
        tagged(mtm, "Paste Binary Content", sel!(pasteBinary:), 0, t),
    ]);
    nested(mtm, "Paste Special", items)
}

fn write(pb: &NSPasteboard, b: &[u8]) {
    pb.clearContents();
    pb.setData_forType(Some(&NSData::with_bytes(b)), &ns(BINARY_TYPE));
    pb.setString_forType(&ns(&String::from_utf8_lossy(b)), unsafe {
        NSPasteboardTypeString
    });
}

// The raw bytes, or else the UTF-8 text up to the first NUL, as Notepad++ reads CF_TEXT without the length format.
fn read(pb: &NSPasteboard) -> Option<Vec<u8>> {
    if let Some(d) = pb.dataForType(&ns(BINARY_TYPE)) {
        return Some(d.to_vec());
    }
    let s = pb
        .stringForType(unsafe { NSPasteboardTypeString })?
        .to_string();
    Some(s.split('\0').next().unwrap_or("").as_bytes().to_vec())
}

impl App {
    // IDM_EDIT_COPY_BINARY and IDM_EDIT_CUT_BINARY: the bytes of the selection, unchanged.
    pub(crate) fn copy_binary(&self, cut: bool) {
        let Some(v) = self.editor() else { return };
        let len = send(&v, SCI_GETSELTEXT, 0, 0).max(0) as usize;
        if len == 0 {
            return;
        }
        let (s, e) = sci::selection(&v);
        let mut b = sci::doc(&v).range(s, e);
        b.truncate(len);
        write(&NSPasteboard::generalPasteboard(), &b);
        if cut {
            send(&v, SCI_REPLACESEL, 0, c"".as_ptr() as isize);
        }
    }

    // IDM_EDIT_PASTE_BINARY.
    pub(crate) fn paste_binary(&self) {
        let Some(v) = self.editor() else { return };
        let Some(b) = read(&NSPasteboard::generalPasteboard()) else {
            return;
        };
        send(&v, SCI_REPLACESEL, 0, c"".as_ptr() as isize);
        send(&v, SCI_ADDTEXT, b.len(), b.as_ptr() as isize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip() {
        let pb = NSPasteboard::pasteboardWithUniqueName();
        let b = b"a\x80b\0c\xC3(\xFF";
        write(&pb, b);
        assert_eq!(read(&pb).as_deref(), Some(&b[..]));
        pb.clearContents();
        pb.setString_forType(&ns("x\u{e9}\0y"), unsafe { NSPasteboardTypeString });
        assert_eq!(read(&pb).as_deref(), Some("x\u{e9}".as_bytes()));
        let _: () = unsafe { objc2::msg_send![&pb, releaseGlobally] };
    }
}
