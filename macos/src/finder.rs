// SPDX-License-Identifier: GPL-3.0-or-later
use crate::search::{self, FifOut, Opts};
use crate::{sci, App};
use objc2::DefinedClass;
use objc2_app_kit::NSView;
use std::path::PathBuf;

const SCI_MARKERDEFINE: u32 = 2040;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_GETFOLDLEVEL: u32 = 2223;
const SCI_TOGGLEFOLD: u32 = 2231;
const SCI_FOLDLINE: u32 = 2237;
const SC_FOLDLEVELBASE: isize = 0x400;
const SC_FOLDLEVELHEADERFLAG: isize = 0x2000;
const SC_FOLDLEVELNUMBERMASK: isize = 0x0FFF;
const SC_MARK_EMPTY: isize = 5;
const SC_MARK_MINUS: isize = 7;
const SC_MARK_PLUS: isize = 8;

fn is_header(level: isize) -> bool {
    level & SC_FOLDLEVELHEADERFLAG != 0
}

// Finder::beginNewFilesSearch: collapse the old search blocks; the new block at the top stays open.
pub fn fold_actions(levels: &[isize]) -> Vec<(usize, bool)> {
    let mut search = 0;
    let mut out = vec![];
    for (line, &l) in levels.iter().enumerate() {
        if !is_header(l) {
            continue;
        }
        let top = l & SC_FOLDLEVELNUMBERMASK == SC_FOLDLEVELBASE;
        if top {
            search += 1;
        }
        if search <= 1 {
            out.push((line, true));
        } else if top {
            out.push((line, false));
        }
    }
    out
}

impl App {
    // Text of the open tabs that have a file, for Find in Files.
    pub(crate) fn open_texts(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let tabs = self.ivars().tabs.borrow().clone();
        tabs.iter()
            .filter_map(|t| Some((search::canonical(t.path.as_deref()?), sci::bytes(&t.view))))
            .collect()
    }

    // Port of Notepad_plus::replaceInFilelist for open files: replace in the tab, then save the tab.
    pub(crate) fn replace_in_tabs(&self, o: &Opts, out: &mut FifOut) -> Result<(), String> {
        for f in std::mem::take(&mut out.open) {
            let c = search::canonical(&f);
            let i = self
                .ivars()
                .tabs
                .borrow()
                .iter()
                .position(|t| t.path.as_deref().is_some_and(|p| search::canonical(p) == c));
            let Some((i, t)) = i.and_then(|i| Some((i, self.tab(i)?))) else {
                search::replace_file(&f, o, out)?;
                continue;
            };
            let doc = sci::doc(&t.view);
            if doc.read_only() {
                continue;
            }
            let n = search::replace_all(&doc, o, (0, doc.len()))?;
            if n > 0 {
                out.count += n;
                if !self.save(i, false) {
                    out.errors.push((f, "the tab was not saved".into()));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn collapse_old_searches(&self) {
        let v = &self.ivars().results.get().unwrap().0;
        let n = sci::send(v, SCI_GETLINECOUNT, 0, 0).max(0) as usize;
        let levels: Vec<isize> = (0..n)
            .map(|l| sci::send(v, SCI_GETFOLDLEVEL, l, 0))
            .collect();
        for (line, expand) in fold_actions(&levels) {
            sci::send(v, SCI_FOLDLINE, line, expand as isize);
        }
    }

    // Finder::gotoFoundLine: a double-click on a header line toggles its fold.
    pub(crate) fn toggle_result_header(&self, line: isize) -> bool {
        let v = &self.ivars().results.get().unwrap().0;
        let header = line >= 0 && is_header(sci::send(v, SCI_GETFOLDLEVEL, line as usize, 0));
        if header {
            sci::send(v, SCI_TOGGLEFOLD, line as usize, 0);
        }
        header
    }
}

// FOLDER_STYLE_SIMPLE markers, as the Finder uses.
pub fn simple_fold_markers(v: &NSView) {
    for n in 25..=29 {
        sci::send(v, SCI_MARKERDEFINE, n, SC_MARK_EMPTY);
    }
    sci::send(v, SCI_MARKERDEFINE, 30, SC_MARK_PLUS);
    sci::send(v, SCI_MARKERDEFINE, 31, SC_MARK_MINUS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_searches_collapse() {
        let h = SC_FOLDLEVELHEADERFLAG;
        let (search, file, hit) = (
            SC_FOLDLEVELBASE | h,
            (SC_FOLDLEVELBASE + 1) | h,
            SC_FOLDLEVELBASE + 2,
        );
        let levels = [
            search, file, hit, hit, file, hit, search, file, hit, search, file, hit,
        ];
        assert_eq!(
            fold_actions(&levels),
            [(0, true), (1, true), (4, true), (6, false), (9, false)]
        );
        assert_eq!(fold_actions(&[search, file, hit]), [(0, true), (1, true)]);
        assert!(fold_actions(&[]).is_empty());
    }
}
