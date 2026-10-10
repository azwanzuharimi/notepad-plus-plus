// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{Config, Language};
use std::path::Path;

// Copied from ScintillaEditView::_langNameInfoArray (language name, Lexilla lexer name).
pub(crate) const LEXERS: &[(&str, &str)] = &[
    ("php", "phpscript"),
    ("c", "cpp"),
    ("cpp", "cpp"),
    ("cs", "cpp"),
    ("objc", "objc"),
    ("java", "cpp"),
    ("rc", "cpp"),
    ("html", "hypertext"),
    ("xml", "xml"),
    ("makefile", "makefile"),
    ("pascal", "pascal"),
    ("batch", "batch"),
    ("ini", "props"),
    ("asp", "hypertext"),
    ("sql", "sql"),
    ("vb", "vb"),
    ("javascript", "cpp"),
    ("css", "css"),
    ("perl", "perl"),
    ("python", "python"),
    ("lua", "lua"),
    ("tex", "tex"),
    ("fortran", "fortran"),
    ("bash", "bash"),
    ("actionscript", "cpp"),
    ("nsis", "nsis"),
    ("tcl", "tcl"),
    ("lisp", "lisp"),
    ("scheme", "lisp"),
    ("asm", "asm"),
    ("diff", "diff"),
    ("props", "props"),
    ("postscript", "ps"),
    ("ruby", "ruby"),
    ("smalltalk", "smalltalk"),
    ("vhdl", "vhdl"),
    ("kix", "kix"),
    ("autoit", "au3"),
    ("caml", "caml"),
    ("ada", "ada"),
    ("verilog", "verilog"),
    ("matlab", "matlab"),
    ("haskell", "haskell"),
    ("inno", "inno"),
    ("searchResult", "searchResult"),
    ("cmake", "cmake"),
    ("yaml", "yaml"),
    ("cobol", "COBOL"),
    ("gui4cli", "gui4cli"),
    ("d", "d"),
    ("powershell", "powershell"),
    ("r", "r"),
    ("jsp", "hypertext"),
    ("coffeescript", "coffeescript"),
    ("json", "json"),
    ("javascript.js", "cpp"),
    ("fortran77", "f77"),
    ("baanc", "baan"),
    ("srec", "srec"),
    ("ihex", "ihex"),
    ("tehex", "tehex"),
    ("swift", "cpp"),
    ("asn1", "asn1"),
    ("avs", "avs"),
    ("blitzbasic", "blitzbasic"),
    ("purebasic", "purebasic"),
    ("freebasic", "freebasic"),
    ("csound", "csound"),
    ("erlang", "erlang"),
    ("escript", "escript"),
    ("forth", "forth"),
    ("latex", "latex"),
    ("mmixal", "mmixal"),
    ("nim", "nimrod"),
    ("nncrontab", "nncrontab"),
    ("oscript", "oscript"),
    ("rebol", "rebol"),
    ("registry", "registry"),
    ("rust", "rust"),
    ("spice", "spice"),
    ("txt2tags", "txt2tags"),
    ("visualprolog", "visualprolog"),
    ("typescript", "cpp"),
    ("json5", "json"),
    ("mssql", "mssql"),
    ("gdscript", "gdscript"),
    ("hollywood", "hollywood"),
    ("go", "cpp"),
    ("raku", "raku"),
    ("toml", "toml"),
    ("sas", "sas"),
    ("errorlist", "errorlist"),
    ("escseq", "escseq"),
    ("fcST", "fcST"),
];

const KW_CLASSES: [&str; 9] = [
    "instre1", "instre2", "type1", "type2", "type3", "type4", "type5", "type6", "type7",
];

pub struct Setup<'a> {
    pub lexer: &'static str,
    pub keywords: Vec<(usize, String)>,
    pub stylers: Vec<&'a str>,
    pub props: Vec<(&'static str, &'static str)>,
    pub eol_filled: Vec<usize>,
}

pub fn lexer_name(lang: &str) -> &'static str {
    LEXERS
        .iter()
        .find(|(l, _)| *l == lang)
        .map_or("null", |(_, x)| x)
}

const NAME_INFO: &str =
    include_str!("../../PowerEditor/src/ScintillaComponent/ScintillaEditView.cpp");

// Long name from ScintillaEditView::_langNameInfoArray, as the Notepad++ status bar shows it.
pub fn long_name(lang: &str) -> String {
    NAME_INFO
        .lines()
        .filter_map(|l| l.trim().strip_prefix("{L\""))
        .map(|l| l.split('"').step_by(2).collect::<Vec<_>>())
        .find(|f| f.len() > 2 && f[0] == lang)
        .map_or(lang.to_string(), |f| f[2].to_string())
}

// File names that Buffer::setFileName maps to a language when the extension gives normal text.
const FILE_NAMES: [(&str, &[&str]); 5] = [
    ("makefile", &["makefile", "GNUmakefile"]),
    ("cmake", &["CmakeLists.txt"]),
    ("python", &["SConstruct", "SConscript", "wscript"]),
    ("ruby", &["Rakefile", "Vagrantfile"]),
    ("bash", &["crontab", "PKGBUILD", "APKBUILD"]),
];

// Buffer::setFileName: the text after the last dot of the file name (PathFindExtension), then the file name list.
pub fn language_for_path<'a>(cfg: &'a Config, path: &Path) -> Option<&'a Language> {
    let name = path.file_name()?.to_string_lossy();
    let ext = name
        .rfind('.')
        .map(|i| name[i + 1..].to_lowercase())
        .filter(|e| !e.contains(' '));
    let by_ext = ext.and_then(|e| cfg.languages.iter().rev().find(|l| l.exts.contains(&e)));
    if by_ext.is_some_and(|l| l.name != "normal") {
        return by_ext;
    }
    FILE_NAMES
        .iter()
        .find(|(_, names)| names.iter().any(|n| n.eq_ignore_ascii_case(&name)))
        .and_then(|(lang, _)| cfg.languages.iter().find(|l| l.name == *lang))
        .or(by_ext)
}

// FileManager::detectLanguageFromTextBeginning: the language of the first line, else None for normal text.
pub fn language_from_text(data: &[u8]) -> Option<&'static str> {
    if data.len() <= 3 {
        return None;
    }
    let bom = matches!(data[..3], [0xEF, 0xBB, 0xBF] | [0xFE, 0xFF, 0x00] | [0xFF, 0xFE, 0x00]);
    let rest = &data[if bom { 3 } else { 0 }..];
    let rest = &rest[rest.iter().position(|b| !b" \t\n\r".contains(b))?..];
    let line = rest[..rest.len().min(40)].split(|b| *b == b'\r' || *b == b'\n').next()?;
    let has = |p: &str| line.windows(p.len()).any(|w| w == p.as_bytes());
    if line.starts_with(b"#!") {
        return SHEBANGS.iter().find(|(p, _)| has(p)).map(|(_, l)| *l);
    }
    FIRST_LINES.iter().find(|(p, _)| line.starts_with(p.as_bytes())).map(|(_, l)| *l)
}

const SHEBANGS: [(&str, &str); 6] = [
    ("sh", "bash"),
    ("python", "python"),
    ("perl", "perl"),
    ("php", "php"),
    ("ruby", "ruby"),
    ("node", "javascript.js"),
];

const FIRST_LINES: [(&str, &str); 5] = [
    ("<?xml", "xml"),
    ("<?php", "php"),
    ("<html", "html"),
    ("<!DOCTYPE html", "html"),
    ("<?", "php"),
];

// Buffer::setFileName and FileManager::loadFileData: the first line gives the language when the file name gives normal text.
pub fn language_for_file<'a>(cfg: &'a Config, path: &Path, text: &[u8]) -> Option<&'a Language> {
    let by_name = language_for_path(cfg, path);
    if by_name.is_some_and(|l| l.name != "normal") {
        return by_name;
    }
    language_from_text(text)
        .and_then(|n| cfg.languages.iter().find(|l| l.name == n))
        .or(by_name)
}

fn words<'a>(cfg: &'a Config, lang: &str, class: &str) -> Option<&'a str> {
    let l = cfg.languages.iter().find(|l| l.name == lang)?;
    l.keywords
        .iter()
        .find(|(c, _)| c == class)
        .map(|(_, w)| w.as_str())
}

// ScintillaEditView::concatToBuildKeywordList: the styler user words, then the langs.model.xml words.
fn user_and_lang_words(cfg: &Config, lang: &str, class: &str) -> String {
    let user = cfg
        .lexer_styles
        .iter()
        .find(|(n, _)| n == lang)
        .and_then(|(_, v)| {
            v.iter()
                .rev()
                .find(|s| s.keyword_class == class && !s.keywords.is_empty())
        })
        .map_or("", |s| s.keywords.as_str());
    format!("{user} {}", words(cfg, lang, class).unwrap_or(""))
}

// ScintillaEditView::populateSubStyleKeywords calls: (base style, identifier list of each substyle).
pub fn substyles(cfg: &Config, name: &str) -> Vec<(usize, Vec<String>)> {
    let blocks: &[(&str, usize, usize, usize)] = match name {
        "c" | "cpp" | "java" | "rc" | "cs" | "actionscript" | "swift" | "go" | "typescript" => {
            &[(name, 11, 8, 1)]
        }
        "javascript" | "javascript.js" => &[("javascript.js", 11, 8, 1)],
        "python" | "gdscript" => &[(name, 11, 8, 1)],
        "lua" => &[(name, 11, 4, 1)],
        "bash" => &[(name, 8, 4, 1), (name, 9, 4, 5)],
        "xml" => &[(name, 3, 8, 1)],
        "html" | "php" | "asp" | "jsp" => &[
            ("html", 1, 4, 1),
            ("html", 3, 4, 5),
            ("javascript", 46, 8, 1),
            ("php", 121, 8, 1),
            ("asp", 74, 8, 1),
        ],
        _ => &[],
    };
    blocks
        .iter()
        .map(|&(lang, base, n, first)| {
            let lists = (first..first + n)
                .map(|k| user_and_lang_words(cfg, lang, &format!("substyle{k}")))
                .collect();
            (base, lists)
        })
        .collect()
}

// Mirrors ScintillaEditView.cpp lexer setup: keywords, stylers, properties and EOL fill per language (no fold properties).
pub fn setup<'a>(cfg: &'a Config, name: &'a str) -> Setup<'a> {
    let doxygen = (2, "cpp", "type2");
    let pick = |list: &[(usize, &str, &str)]| -> Vec<(usize, String)> {
        list.iter()
            .filter_map(|&(i, l, c)| {
                let w = words(cfg, l, c)?;
                Some((
                    i,
                    if (i, l, c) == doxygen {
                        w.to_string()
                    } else {
                        user_and_lang_words(cfg, l, c)
                    },
                ))
            })
            .collect()
    };
    let track = ("lexer.cpp.track.preprocessor", "0");
    let backquoted = |v| ("lexer.cpp.backquoted.strings", v);
    let (keywords, stylers, props) = match name {
        "c" | "cpp" | "java" | "cs" | "actionscript" | "swift" | "go" => (
            pick(&[
                (0, name, "instre1"),
                (1, name, "type1"),
                (3, name, "instre2"),
                doxygen,
            ]),
            vec![name],
            if name == "go" {
                vec![backquoted("1"), track]
            } else {
                vec![track]
            },
        ),
        "rc" => (
            pick(&[
                (0, name, "instre1"),
                (1, name, "type1"),
                (3, name, "instre2"),
            ]),
            vec![name],
            vec![track],
        ),
        "javascript" | "javascript.js" => (
            pick(&[
                (0, "javascript.js", "instre1"),
                (1, "javascript.js", "type1"),
                (3, "javascript.js", "instre2"),
                doxygen,
            ]),
            vec!["javascript.js"],
            vec![track, backquoted("2")],
        ),
        "typescript" => (
            pick(&[(0, name, "instre1"), (1, name, "type1"), doxygen]),
            vec![name],
            vec![track, backquoted("1")],
        ),
        "objc" => (
            pick(&[
                (0, name, "instre1"),
                (1, name, "type1"),
                doxygen,
                (3, name, "instre2"),
                (4, name, "type2"),
            ]),
            vec![name],
            vec![],
        ),
        "xml" => (
            pick(&[(5, name, "instre1")]),
            vec![name],
            vec![("lexer.xml.allow.scripts", "0")],
        ),
        "html" | "php" | "asp" | "jsp" => {
            return Setup {
                lexer: "hypertext",
                keywords: pick(&[
                    (0, "html", "instre1"),
                    (5, "html", "instre2"),
                    (1, "javascript", "instre1"),
                    (4, "php", "instre1"),
                    (2, "vb", "instre1"),
                ]),
                stylers: vec!["html", "javascript", "php", "asp"],
                props: vec![("asp.default.language", "2")],
                eol_filled: vec![41, 42, 44, 53, 68, 118, 124, 81],
            };
        }
        _ => {
            let own = cfg.languages.iter().find(|l| l.name == name);
            let kws = own.map_or(vec![], |l| {
                l.keywords
                    .iter()
                    .filter_map(|(c, _)| {
                        Some((
                            KW_CLASSES.iter().position(|k| k == c)?,
                            user_and_lang_words(cfg, name, c),
                        ))
                    })
                    .collect()
            });
            (kws, vec![name], vec![])
        }
    };
    Setup {
        lexer: lexer_name(name),
        keywords,
        stylers,
        props,
        eol_filled: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load;

    #[test]
    fn first_line_languages() {
        let cases: [(&[u8], Option<&str>); 17] = [
            (b"#!/bin/sh\necho", Some("bash")),
            (b"#!/usr/bin/env python3\n", Some("python")),
            (b"#!/usr/bin/perl -w", Some("perl")),
            (b"#!/usr/bin/php", Some("php")),
            (b"#!/usr/bin/env ruby", Some("ruby")),
            (b"#!/usr/bin/env node", Some("javascript.js")),
            (b"#!/usr/bin/env lua", None),
            (b"\xEF\xBB\xBF  \r\n<?xml version=\"1.0\"?>", Some("xml")),
            (b"<?php echo 1;", Some("php")),
            (b"<html><body>", Some("html")),
            (b"<!DOCTYPE html>", Some("html")),
            (b"<!doctype html>", None),
            (b"<? echo", Some("php")),
            (b"abc", None),
            (b"hello <?xml", None),
            (b"echo\n#!/bin/sh", None),
            (b"#!/x\nbin/sh python", None),
        ];
        for (text, lang) in cases {
            assert_eq!(language_from_text(text), lang, "{}", String::from_utf8_lossy(text));
        }
        assert_eq!(language_from_text(&[b' '; 50]), None);
        let long = format!("#!{}python", " ".repeat(40));
        assert_eq!(language_from_text(long.as_bytes()), None);
    }

    #[test]
    fn first_line_needs_normal_text_name() {
        let c = load();
        let lang = |p: &str, t: &[u8]| language_for_file(&c, Path::new(p), t).map(|l| l.name.clone());
        assert_eq!(lang("run", b"#!/bin/sh\n").as_deref(), Some("bash"));
        assert_eq!(lang("a.txt", b"<?xml?>").as_deref(), Some("xml"));
        assert_eq!(lang("a.py", b"#!/bin/sh\n").as_deref(), Some("python"));
        assert_eq!(lang("node", b"#!/usr/bin/env node\n").as_deref(), Some("javascript.js"));
        for (_, l) in SHEBANGS.iter().chain(FIRST_LINES.iter()) {
            assert!(c.languages.iter().any(|x| x.name == *l), "{l}");
        }
    }

    #[test]
    fn long_names() {
        assert_eq!(long_name("normal"), "Normal text file");
        assert_eq!(long_name("python"), "Python file");
        assert_eq!(long_name("cpp"), "C++ source file");
        assert_eq!(long_name("toml"), "Tom's Obvious Minimal Language file");
    }

    fn lang_of(c: &Config, p: &str) -> String {
        language_for_path(c, Path::new(p))
            .map(|l| l.name.clone())
            .unwrap_or_default()
    }

    fn kw<'b>(s: &'b Setup, i: usize) -> Vec<&'b str> {
        s.keywords
            .iter()
            .filter(|k| k.0 == i)
            .map(|k| k.1.trim_start())
            .collect()
    }

    fn has(s: &Setup, i: usize, word: &str) -> bool {
        kw(s, i)
            .iter()
            .any(|w| w.split_whitespace().any(|x| x == word))
    }

    #[test]
    fn ext_mapping() {
        let c = load();
        assert_eq!(lang_of(&c, "/a/b.py"), "python");
        assert_eq!(lang_of(&c, "x.CPP"), "cpp");
        assert_eq!(lang_of(&c, "x.h"), "cpp");
        assert_eq!(lang_of(&c, "x.rs"), "rust");
        assert_eq!(lang_of(&c, "x.unknownext"), "");
        assert_eq!(lang_of(&c, "noext"), "");
    }

    #[test]
    fn file_name_mapping() {
        let c = load();
        assert_eq!(lang_of(&c, "/src/Makefile"), "makefile");
        assert_eq!(lang_of(&c, "makefile"), "makefile");
        assert_eq!(lang_of(&c, "GNUmakefile"), "makefile");
        assert_eq!(lang_of(&c, "CMakeLists.txt"), "cmake");
        assert_eq!(lang_of(&c, "SConstruct"), "python");
        assert_eq!(lang_of(&c, "wscript"), "python");
        assert_eq!(lang_of(&c, "Rakefile"), "ruby");
        assert_eq!(lang_of(&c, "Vagrantfile"), "ruby");
        assert_eq!(lang_of(&c, "crontab"), "bash");
        assert_eq!(lang_of(&c, "PKGBUILD"), "bash");
        assert_eq!(lang_of(&c, "APKBUILD"), "bash");
        assert_eq!(lang_of(&c, "/home/me/.bashrc"), "bash");
        assert_eq!(lang_of(&c, ".bash_profile"), "bash");
        assert_eq!(lang_of(&c, ".profile"), "bash");
        assert_eq!(lang_of(&c, "notes.txt"), "normal");
        assert_eq!(lang_of(&c, "Dockerfile"), "");
        assert_eq!(lang_of(&c, "Makefile.am"), "");
        assert_eq!(lang_of(&c, "x.mk"), "makefile");
        assert_eq!(lang_of(&c, "a.b c"), "");
        assert_eq!(lang_of(&c, "file."), "");
    }

    #[test]
    fn ext_mapping_searches_from_end() {
        let c = load();
        assert_eq!(lang_of(&c, "paper.tex"), "tex");
    }

    #[test]
    fn lexers() {
        assert_eq!(lexer_name("python"), "python");
        assert_eq!(lexer_name("cpp"), "cpp");
        assert_eq!(lexer_name("javascript.js"), "cpp");
        assert_eq!(lexer_name("html"), "hypertext");
        assert_eq!(lexer_name("normal"), "null");
        assert_eq!(lexer_name("nope"), "null");
        let c = load();
        for l in c
            .languages
            .iter()
            .filter(|l| !["normal", "nfo"].contains(&l.name.as_str()))
        {
            assert_ne!(lexer_name(&l.name), "null", "{}", l.name);
        }
    }

    #[test]
    fn generic_keywords() {
        let c = load();
        let s = setup(&c, "python");
        assert_eq!(s.lexer, "python");
        assert!(has(&s, 0, "lambda"));
        assert!(has(&s, 1, "ArithmeticError"));
        assert_eq!(s.stylers, ["python"]);
        let perl = setup(&c, "perl");
        assert!(has(&perl, 0, "carp") && has(&perl, 0, "croak") && has(&perl, 0, "foreach"));
    }

    #[test]
    fn cpp_keywords() {
        let c = load();
        let s = setup(&c, "cpp");
        assert!(has(&s, 0, "co_await"));
        assert!(has(&s, 1, "constexpr"));
        assert!(has(&s, 2, "brief"));
        assert!(kw(&s, 3).len() <= 1);
        let s = setup(&c, "swift");
        assert_eq!(s.lexer, "cpp");
        assert!(has(&s, 2, "brief"));
        assert!(kw(&setup(&c, "rc"), 2).is_empty());
    }

    #[test]
    fn javascript_keywords() {
        let c = load();
        let s = setup(&c, "javascript.js");
        assert_eq!(s.lexer, "cpp");
        assert!(has(&s, 0, "function"));
        assert!(has(&s, 2, "brief"));
        assert_eq!(s.stylers, ["javascript.js"]);
    }

    #[test]
    fn objc_keywords() {
        let c = load();
        let s = setup(&c, "objc");
        assert_eq!(s.lexer, "objc");
        for (i, class) in [(0, "instre1"), (1, "type1"), (3, "instre2"), (4, "type2")] {
            let want = words(&c, "objc", class).unwrap();
            assert_eq!(kw(&s, i), [want], "{class}");
        }
        assert!(has(&s, 2, "brief"));
    }

    #[test]
    fn xml_keywords() {
        let c = load();
        let s = setup(&c, "xml");
        assert_eq!(s.lexer, "xml");
        assert_eq!(kw(&s, 5), [words(&c, "xml", "instre1").unwrap()]);
        assert!(kw(&s, 0).is_empty());
        assert!(s.props.contains(&("lexer.xml.allow.scripts", "0")));
    }

    #[test]
    fn html_family_keywords() {
        let c = load();
        for name in ["html", "php", "asp", "jsp"] {
            let s = setup(&c, name);
            assert_eq!(s.lexer, "hypertext", "{name}");
            assert!(has(&s, 0, "div"), "{name}");
            assert_eq!(kw(&s, 5), [words(&c, "html", "instre2").unwrap()]);
            assert!(has(&s, 1, "function"), "{name}");
            assert!(has(&s, 4, "echo"), "{name}");
            assert_eq!(kw(&s, 2), [words(&c, "vb", "instre1").unwrap()]);
            assert_eq!(s.stylers, ["html", "javascript", "php", "asp"]);
            assert!(s.props.contains(&("asp.default.language", "2")));
        }
    }

    #[test]
    fn substyle_lists() {
        let c = load();
        let bases = |n: &str| -> Vec<(usize, usize)> {
            substyles(&c, n)
                .iter()
                .map(|(b, l)| (*b, l.len()))
                .collect()
        };
        for name in [
            "c",
            "cpp",
            "rc",
            "go",
            "javascript.js",
            "typescript",
            "python",
            "gdscript",
        ] {
            assert_eq!(bases(name), [(11, 8)], "{name}");
        }
        assert_eq!(bases("lua"), [(11, 4)]);
        assert_eq!(bases("bash"), [(8, 4), (9, 4)]);
        assert_eq!(bases("xml"), [(3, 8)]);
        assert_eq!(bases("php"), [(1, 4), (3, 4), (46, 8), (121, 8), (74, 8)]);
        assert!(bases("objc").is_empty() && bases("normal").is_empty());
        let php = substyles(&c, "jsp");
        assert!(php[1].1[0].starts_with("download "));
        assert_eq!(php[3].1[5].split_whitespace().next(), Some("__class__"));
        assert_eq!(substyles(&c, "cpp")[0].1[0], " ");
    }

    #[test]
    fn lexer_properties() {
        let c = load();
        let track = ("lexer.cpp.track.preprocessor", "0");
        let bq = |v| ("lexer.cpp.backquoted.strings", v);
        for name in ["c", "cpp", "java", "rc", "cs", "actionscript", "swift"] {
            assert_eq!(setup(&c, name).props, [track], "{name}");
        }
        assert_eq!(setup(&c, "go").props, [bq("1"), track]);
        assert_eq!(setup(&c, "javascript.js").props, [track, bq("2")]);
        assert_eq!(setup(&c, "typescript").props, [track, bq("1")]);
        assert!(setup(&c, "objc").props.is_empty());
        let h = setup(&c, "php");
        assert_eq!(h.eol_filled, [41, 42, 44, 53, 68, 118, 124, 81]);
        assert!(setup(&c, "python").eol_filled.is_empty());
    }
}
