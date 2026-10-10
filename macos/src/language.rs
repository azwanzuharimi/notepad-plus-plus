// SPDX-License-Identifier: GPL-3.0-or-later
use crate::comment::{self, CMDS};
use crate::config::{Config, Language};
use crate::{cfg, lang, nested, sci, tagged, App, Tab};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, DefinedClass, MainThreadMarker};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSMenuItem,
};
use std::path::Path;

const RC: &str = include_str!("../../PowerEditor/src/Notepad_plus.rc");
const SCI_GETCURRENTPOS: u32 = 2008;

// IDM_LANG_ suffixes whose language name is not the suffix in lower case.
const IDM_NAMES: [(&str, &str); 9] = [
    ("JS", "javascript.js"),
    ("ASCII", "nfo"),
    ("TEXT", "normal"),
    ("FORTRAN_77", "fortran77"),
    ("FLASH", "actionscript"),
    ("PS", "postscript"),
    ("AU3", "autoit"),
    ("GOLANG", "go"),
    ("STTXT", "fcST"),
];

#[derive(Debug, PartialEq)]
pub enum Entry {
    Item(String, String),
    Group(String, Vec<(String, String)>),
    Separator,
}

// The compact Language menu of Notepad_plus.rc (menu text, language name); items of languages not loaded are left out.
pub fn menu_entries(c: &Config) -> Vec<Entry> {
    let loaded = |id: &str| {
        let suffix = id.strip_prefix("IDM_LANG_")?;
        let name = IDM_NAMES
            .iter()
            .find(|(s, _)| *s == suffix)
            .map_or(suffix.to_lowercase(), |(_, n)| n.to_string());
        c.languages.iter().any(|l| l.name == name).then_some(name)
    };
    let start = RC
        .rfind("POPUP \"&Language\"")
        .expect("Language menu in Notepad_plus.rc");
    let mut out = vec![];
    let mut group: Option<(String, Vec<(String, String)>)> = None;
    let mut depth = 0;
    for line in RC[start..].lines().skip(1).map(str::trim) {
        let quoted = line.split('"').nth(1).unwrap_or_default().to_string();
        let id = line.rsplit(',').next().unwrap_or_default().trim();
        match line.split_whitespace().next() {
            Some("BEGIN") => depth += 1,
            Some("END") => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                if let Some((g, v)) = group.take().filter(|(_, v)| !v.is_empty()) {
                    out.push(Entry::Group(g, v));
                }
            }
            Some("POPUP") => group = Some((quoted, vec![])),
            Some("MENUITEM") if line.contains("SEPARATOR") => out.push(Entry::Separator),
            Some("MENUITEM") => {
                if let Some(name) = loaded(id) {
                    match group.as_mut() {
                        Some((_, v)) => v.push((quoted, name)),
                        None => out.push(Entry::Item(quoted, name)),
                    }
                }
            }
            _ => {}
        }
    }
    while out.last() == Some(&Entry::Separator) {
        out.pop();
    }
    out
}

pub fn language_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    let c = cfg();
    let it = |(text, name): &(String, String)| {
        let i = c
            .languages
            .iter()
            .position(|l| &l.name == name)
            .unwrap_or(0);
        tagged(mtm, text, sel!(setLanguage:), i as isize, t)
    };
    menu_entries(c)
        .into_iter()
        .map(|e| match e {
            Entry::Item(text, name) => it(&(text, name)),
            Entry::Group(g, v) => nested(mtm, &g, v.iter().map(it).collect()),
            Entry::Separator => NSMenuItem::separatorItem(mtm),
        })
        .collect()
}

// Edit > Comment/Uncomment; Notepad++ uses Ctrl+Q, Ctrl+K, Ctrl+Shift+K and Ctrl+Shift+Q; Shift+Cmd+/ is the macOS Help search.
pub fn comment_menu(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Retained<NSMenuItem> {
    let keys = ["/", "k", "K", "/", ""];
    let items = CMDS
        .iter()
        .zip(keys)
        .enumerate()
        .map(|(k, ((title, _), key))| {
            let i = tagged(mtm, title, sel!(comment:), k as isize, t);
            i.setKeyEquivalent(&crate::ns(key));
            if k == 3 {
                i.setKeyEquivalentModifierMask(
                    NSEventModifierFlags::Command | NSEventModifierFlags::Option,
                );
            }
            i
        })
        .collect();
    nested(mtm, "Comment/Uncomment", items)
}

// The language set from the Language menu, else the language of the file name (the tab name for a new file).
pub(crate) fn tab_language(t: &Tab) -> Option<&'static Language> {
    match &t.lang {
        Some(n) => cfg().languages.iter().find(|l| &l.name == n),
        None => lang::language_for_path(cfg(), t.path.as_deref().unwrap_or(Path::new(&t.name))),
    }
}

impl App {
    pub(crate) fn apply_tab_language(&self, i: usize) {
        if let Some(t) = self.tab(i) {
            let l = tab_language(&t);
            sci::apply_language(&t.view, cfg(), l);
            self.apply_view(&t.view, l.map_or("normal", |l| l.name.as_str()), cfg());
        }
        self.function_list_reload();
    }

    // NppCommands.cpp IDM_LANG_*: the menu choice stays when the file name changes later.
    pub(crate) fn set_language(&self, tag: isize) {
        let Some(i) = self.current() else { return };
        let Some(l) = cfg().languages.get(tag as usize) else {
            return;
        };
        if let Some(t) = self.ivars().tabs.borrow_mut().get_mut(i) {
            t.lang = Some(l.name.clone());
        }
        self.apply_tab_language(i);
        self.update_status();
    }

    pub(crate) fn comment(&self, tag: isize) {
        let Some(t) = self.current().and_then(|i| self.tab(i)) else {
            return;
        };
        let (Some(&(_, cmd)), Some(l)) = (CMDS.get(tag as usize), tab_language(&t)) else {
            return;
        };
        let v = &t.view;
        let (start, end) = sci::selection(v);
        let sel = comment::from_limits(start, end, sci::send(v, SCI_GETCURRENTPOS, 0, 0));
        if let Some(s) = comment::run(&sci::doc(v), l, cmd, sel) {
            sci::select(v, s);
        }
    }

    // Checkmark for the language of the current tab; None for items of other menus.
    pub(crate) fn validate_language(&self, item: &NSMenuItem) -> Option<bool> {
        let action = item.action()?;
        let tab = self.current().and_then(|i| self.tab(i));
        if action == sel!(setLanguage:) {
            let cur = tab
                .as_ref()
                .map(|t| tab_language(t).map_or("normal", |l| l.name.as_str()));
            let on = cfg()
                .languages
                .get(item.tag() as usize)
                .is_some_and(|l| cur == Some(l.name.as_str()));
            item.setState(if on {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            return Some(tab.is_some());
        }
        (action == sel!(comment:)).then_some(tab.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load;

    fn names(e: &[Entry]) -> Vec<String> {
        e.iter()
            .flat_map(|e| match e {
                Entry::Item(_, n) => vec![n.clone()],
                Entry::Group(_, v) => v.iter().map(|(_, n)| n.clone()).collect(),
                Entry::Separator => vec![],
            })
            .collect()
    }

    #[test]
    fn menu_follows_notepad_plus_rc() {
        let c = load();
        let e = menu_entries(&c);
        assert_eq!(
            e[0],
            Entry::Item("None (Normal Text)".into(), "normal".into())
        );
        assert_eq!(e[1], Entry::Separator);
        let Entry::Group(a, v) = &e[2] else { panic!() };
        assert_eq!(a, "A");
        assert_eq!(v[0], ("ActionScript".into(), "actionscript".into()));
        assert!(e.contains(&Entry::Item("KIXtart".into(), "kix".into())));
        assert_eq!(e.last(), Some(&Entry::Item("YAML".into(), "yaml".into())));
        let Some(Entry::Group(_, s)) = e
            .iter()
            .find(|x| matches!(x, Entry::Group(g, _) if g == "S"))
        else {
            panic!()
        };
        assert!(s.contains(&("Shell".into(), "bash".into())));
        assert!(s.contains(&("Structured Text".into(), "fcST".into())));
        let groups: String = e
            .iter()
            .filter_map(|x| match x {
                Entry::Group(g, _) => Some(g.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(groups, "ABCDEFGHIJLMNOPRSTV");
    }

    #[test]
    fn menu_lists_each_loaded_language_once() {
        let c = load();
        let mut listed = names(&menu_entries(&c));
        listed.sort();
        let n = listed.len();
        listed.dedup();
        assert_eq!(listed.len(), n);
        let mut want: Vec<String> = c
            .languages
            .iter()
            .map(|l| l.name.clone())
            .filter(|n| n != "javascript" && n != "searchResult")
            .collect();
        want.sort();
        assert_eq!(listed, want);
    }
}
