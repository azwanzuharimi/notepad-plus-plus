// SPDX-License-Identifier: GPL-3.0-or-later
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct Language {
    pub name: String,
    pub exts: Vec<String>,
    pub keywords: Vec<(String, String)>,
    pub comment_line: String,
    pub comment_start: String,
    pub comment_end: String,
}

#[derive(Debug, Clone, Default)]
pub struct Style {
    pub name: String,
    pub id: usize,
    pub fg: Option<isize>,
    pub bg: Option<isize>,
    pub font_name: String,
    pub font_style: Option<u32>,
    pub font_size: Option<isize>,
    pub keyword_class: String,
    pub keywords: String,
}

#[derive(Debug, Default)]
pub struct Config {
    pub languages: Vec<Language>,
    pub lexer_styles: Vec<(String, Vec<Style>)>,
    pub global_styles: Vec<Style>,
}

pub fn rgb_to_bgr(hex: &str) -> Option<isize> {
    let v = u32::from_str_radix(hex, 16)
        .ok()
        .filter(|_| hex.len() == 6)?;
    Some((((v & 0xFF) << 16) | (v & 0xFF00) | (v >> 16)) as isize)
}

const LANGS: &str = include_str!("../../PowerEditor/src/langs.model.xml");
const STYLERS: &str = include_str!("../../PowerEditor/src/stylers.model.xml");

pub fn attr(e: &BytesStart, key: &str) -> String {
    e.try_get_attribute(key)
        .ok()
        .flatten()
        .and_then(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned())
        })
        .unwrap_or_default()
}

fn style(e: &BytesStart) -> Style {
    Style {
        name: attr(e, "name"),
        id: attr(e, "styleID").parse().unwrap_or(0),
        fg: rgb_to_bgr(&attr(e, "fgColor")),
        bg: rgb_to_bgr(&attr(e, "bgColor")),
        font_name: attr(e, "fontName"),
        font_style: attr(e, "fontStyle").parse().ok(),
        font_size: attr(e, "fontSize").parse().ok(),
        keyword_class: attr(e, "keywordClass"),
        keywords: String::new(),
    }
}

pub fn load() -> Config {
    let mut c = Config::default();
    let mut r = Reader::from_str(LANGS);
    let mut kw: Option<(String, String)> = None;
    loop {
        match r.read_event().expect("langs.model.xml") {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == "Language" => {
                c.languages.push(Language {
                    name: attr(&e, "name"),
                    exts: attr(&e, "ext")
                        .split_whitespace()
                        .map(str::to_lowercase)
                        .collect(),
                    keywords: vec![],
                    comment_line: attr(&e, "commentLine"),
                    comment_start: attr(&e, "commentStart"),
                    comment_end: attr(&e, "commentEnd"),
                })
            }
            Event::Start(e) if e.name().as_ref() == "Keywords" => {
                kw = Some((attr(&e, "name"), String::new()))
            }
            Event::Text(t) => {
                if let Some((_, s)) = kw.as_mut() {
                    s.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(g) => {
                if let Some((_, s)) = kw.as_mut() {
                    s.push_str(quick_xml::escape::resolve_predefined_entity(&g).unwrap_or(""));
                }
            }
            Event::End(e) if e.name().as_ref() == "Keywords" => {
                if let (Some(k), Some(l)) = (kw.take(), c.languages.last_mut()) {
                    l.keywords.push(k);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut r = Reader::from_str(STYLERS);
    let mut in_global = false;
    let mut in_words = false;
    loop {
        let ev = r.read_event().expect("stylers.model.xml");
        let start = matches!(ev, Event::Start(_));
        let last = c.lexer_styles.last_mut().and_then(|(_, v)| v.last_mut());
        match ev {
            Event::Start(e) if e.name().as_ref() == "LexerType" => {
                c.lexer_styles.push((attr(&e, "name"), vec![]))
            }
            Event::Start(e) if e.name().as_ref() == "GlobalStyles" => in_global = true,
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == "WordsStyle" => {
                in_words = start;
                if let Some((_, v)) = c.lexer_styles.last_mut() {
                    v.push(style(&e));
                }
            }
            Event::Text(t) if in_words => {
                if let Some(s) = last {
                    s.keywords.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(g) if in_words => {
                if let Some(s) = last {
                    s.keywords
                        .push_str(quick_xml::escape::resolve_predefined_entity(&g).unwrap_or(""));
                }
            }
            Event::End(e) if e.name().as_ref() == "WordsStyle" => in_words = false,
            Event::Start(e) | Event::Empty(e)
                if in_global && e.name().as_ref() == "WidgetStyle" =>
            {
                c.global_styles.push(style(&e))
            }
            Event::Eof => break,
            _ => {}
        }
    }
    c
}

// Folder of the user files: config.xml, session.xml and shortcuts.xml.
pub fn app_support_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/notepadpp-mac"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_hex_to_bgr() {
        assert_eq!(rgb_to_bgr("FF8000"), Some(0x0080FF));
        assert_eq!(rgb_to_bgr("E8E8FF"), Some(0xFFE8E8));
        assert_eq!(rgb_to_bgr(""), None);
        assert_eq!(rgb_to_bgr("zz"), None);
    }

    #[test]
    fn langs_parsed() {
        let c = load();
        assert!(c.languages.len() >= 95, "{}", c.languages.len());
        let py = c.languages.iter().find(|l| l.name == "python").unwrap();
        assert!(py.exts.contains(&"py".to_string()));
        let kw = &py.keywords.iter().find(|(k, _)| k == "instre1").unwrap().1;
        assert!(kw.split_whitespace().any(|w| w == "lambda"));
    }

    #[test]
    fn comment_tokens_parsed() {
        let c = load();
        let get = |n: &str| c.languages.iter().find(|l| l.name == n).unwrap();
        let cpp = get("cpp");
        assert_eq!(
            (
                cpp.comment_line.as_str(),
                cpp.comment_start.as_str(),
                cpp.comment_end.as_str()
            ),
            ("//", "/*", "*/")
        );
        let html = get("html");
        assert_eq!(
            (html.comment_start.as_str(), html.comment_end.as_str()),
            ("<!--", "-->")
        );
        assert_eq!(get("vb").comment_line, "'");
        assert_eq!(get("batch").comment_line, "REM");
        assert!(get("normal").comment_line.is_empty());
    }

    #[test]
    fn user_keywords_parsed() {
        let c = load();
        let styles = |n: &str| &c.lexer_styles.iter().find(|(l, _)| l == n).unwrap().1;
        let attr = styles("html").iter().find(|s| s.id == 196).unwrap();
        assert_eq!((attr.keyword_class.as_str(), attr.keywords.as_str()), ("substyle5", "download"));
        let perl = styles("perl").iter().find(|s| s.id == 5).unwrap();
        assert_eq!((perl.keyword_class.as_str(), perl.keywords.as_str()), ("instre1", "carp croak"));
        let cpp = styles("cpp");
        let user1 = cpp.iter().find(|s| s.id == 128).unwrap();
        assert_eq!((user1.keyword_class.as_str(), user1.keywords.as_str()), ("substyle1", ""));
        assert!(cpp.iter().find(|s| s.id == 11).unwrap().keyword_class.is_empty());
    }

    #[test]
    fn styles_parsed() {
        let c = load();
        let py = &c
            .lexer_styles
            .iter()
            .find(|(n, _)| n == "python")
            .unwrap()
            .1;
        let kw = py.iter().find(|s| s.id == 5).unwrap();
        assert_eq!(kw.fg, Some(0xFF0000));
        assert_eq!(kw.font_style, Some(1));
        let def = c.global_styles.iter().find(|s| s.id == 32).unwrap();
        assert_eq!(def.font_name, "Courier New");
        assert_eq!(def.font_size, Some(10));
        let caret = c
            .global_styles
            .iter()
            .find(|s| s.name == "Current line background colour")
            .unwrap();
        assert_eq!(caret.bg, Some(0xFFE8E8));
        assert_eq!(caret.fg, None);
    }
}
