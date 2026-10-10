// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{sci, App};
use objc2::DefinedClass;
use objc2_app_kit::NSView;
use std::path::Path;

const SCI_CREATEDOCUMENT: u32 = 2375;
const SCI_SETDOCPOINTER: u32 = 2358;
const SCI_RELEASEDOCUMENT: u32 = 2377;
const SCI_GETDOCUMENTOPTIONS: u32 = 2379;
const SC_DOCUMENTOPTION_STYLES_NONE: usize = 1;
const SC_DOCUMENTOPTION_TEXT_LARGE: usize = 0x100;

// FileManager::loadFile: a file of the large file size or more is large while the restriction is on.
pub fn is_large_size(size: u64, enabled: bool, mb: i64) -> bool {
    enabled && size >= mb.clamp(1, 4096) as u64 * 1024 * 1024
}

// A large file has a document without styles, as FileManager::loadFile makes it.
pub fn is_large(v: &NSView) -> bool {
    sci::send(v, SCI_GETDOCUMENTOPTIONS, 0, 0) as usize & SC_DOCUMENTOPTION_STYLES_NONE != 0
}

fn allow(v: &NSView, f: impl FnOnce(&crate::prefs::Prefs) -> bool) -> bool {
    !is_large(v) || crate::prefs::with(|p| !p.large_on || f(p))
}

// Buffer::allowBraceMatch.
#[allow(dead_code)]
pub fn allow_brace_match(v: &NSView) -> bool {
    allow(v, |p| p.large_brace)
}

// Buffer::allowAutoCompletion.
pub fn allow_auto_completion(v: &NSView) -> bool {
    allow(v, |p| p.large_autoc)
}

// Buffer::allowSmartHilite.
pub fn allow_smart_highlight(v: &NSView) -> bool {
    allow(v, |p| p.large_smart)
}

// Buffer::allowClickableLink.
pub fn allow_clickable_link(v: &NSView) -> bool {
    allow(v, |p| p.large_link)
}

impl App {
    // Gives a new editor a document without styles when the file is large, and turns off word wrap.
    pub(crate) fn open_large(&self, v: &NSView, path: Option<&Path>) -> bool {
        let (on, mb, no_wrap) = crate::prefs::with(|p| (p.large_on, p.large_mb, p.large_nowrap));
        let size = path
            .and_then(|p| std::fs::metadata(p).ok())
            .map_or(0, |m| m.len());
        if path.is_none() || !is_large_size(size, on, mb) {
            return false;
        }
        if no_wrap && self.ivars().view.get().on[crate::view::WRAP] {
            self.view_option(crate::view::WRAP);
        }
        let doc = sci::send(
            v,
            SCI_CREATEDOCUMENT,
            0,
            (SC_DOCUMENTOPTION_STYLES_NONE | SC_DOCUMENTOPTION_TEXT_LARGE) as isize,
        );
        if doc != 0 {
            sci::send(v, SCI_SETDOCPOINTER, 0, doc);
            sci::send(v, SCI_RELEASEDOCUMENT, 0, doc);
        }
        doc != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_file_threshold() {
        let mb = 1024 * 1024;
        assert!(is_large_size(200 * mb, true, 200));
        assert!(!is_large_size(200 * mb - 1, true, 200));
        assert!(!is_large_size(500 * mb, false, 200));
        assert!(is_large_size(mb, true, 0));
        assert!(!is_large_size(4096 * mb - 1, true, 9999));
    }
}
