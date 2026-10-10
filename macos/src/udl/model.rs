// SPDX-License-Identifier: GPL-3.0-or-later
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

pub const KW_TOTAL: usize = 28;
pub const STYLE_TOTAL: usize = 24;
pub const KW_COMMENTS: usize = 0;
pub const KW_KEYWORDS1: usize = 19;
pub const KW_DELIMITERS: usize = 27;
pub const MAX_LANGS: usize = 30;
const MAX_CHAR: usize = 1024 * 30;
const VERSION: &str = "2.1";
pub const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n";

// GlobalMappers::keywordNameMapper: the keyword list names that Notepad++ writes.
pub const KW_NAMES: [&str; KW_TOTAL] = [
    "Comments",
    "Numbers, prefix1",
    "Numbers, prefix2",
    "Numbers, extras1",
    "Numbers, extras2",
    "Numbers, suffix1",
    "Numbers, suffix2",
    "Numbers, range",
    "Operators1",
    "Operators2",
    "Folders in code1, open",
    "Folders in code1, middle",
    "Folders in code1, close",
    "Folders in code2, open",
    "Folders in code2, middle",
    "Folders in code2, close",
    "Folders in comment, open",
    "Folders in comment, middle",
    "Folders in comment, close",
    "Keywords1",
    "Keywords2",
    "Keywords3",
    "Keywords4",
    "Keywords5",
    "Keywords6",
    "Keywords7",
    "Keywords8",
    "Delimiters",
];

// GlobalMappers::keywordIdMapper: the names of UDL versions before 2.1.
const OLD_KW_NAMES: [(&str, usize); 11] = [
    ("Operators", 8),
    ("Folder+", 10),
    ("Folder-", 12),
    ("Words1", 19),
    ("Words2", 20),
    ("Words3", 21),
    ("Words4", 22),
    ("Numbers, additional", 7),
    ("Numbers, prefixes", 2),
    ("Numbers, extras with prefixes", 4),
    ("Numbers, suffixes", 6),
];

// GlobalMappers::styleNameMapper, in SCE_USER_STYLE_* order.
pub const STYLE_NAMES: [&str; STYLE_TOTAL] = [
    "DEFAULT",
    "COMMENTS",
    "LINE COMMENTS",
    "NUMBERS",
    "KEYWORDS1",
    "KEYWORDS2",
    "KEYWORDS3",
    "KEYWORDS4",
    "KEYWORDS5",
    "KEYWORDS6",
    "KEYWORDS7",
    "KEYWORDS8",
    "OPERATORS",
    "FOLDER IN CODE1",
    "FOLDER IN CODE2",
    "FOLDER IN COMMENT",
    "DELIMITERS1",
    "DELIMITERS2",
    "DELIMITERS3",
    "DELIMITERS4",
    "DELIMITERS5",
    "DELIMITERS6",
    "DELIMITERS7",
    "DELIMITERS8",
];

const OLD_STYLE_NAMES: [(&str, usize); 13] = [
    ("FOLDEROPEN", 13),
    ("FOLDERCLOSE", 13),
    ("KEYWORD1", 4),
    ("KEYWORD2", 5),
    ("KEYWORD3", 6),
    ("KEYWORD4", 7),
    ("COMMENT", 1),
    ("COMMENT LINE", 2),
    ("NUMBER", 3),
    ("OPERATOR", 12),
    ("DELIMINER1", 16),
    ("DELIMINER2", 17),
    ("DELIMINER3", 18),
];

// GlobalMappers::setLexerMapper: the lists that go to the lexer as properties.
const LEXER_PROPS: [(usize, &str); 13] = [
    (0, "userDefine.comments"),
    (27, "userDefine.delimiters"),
    (8, "userDefine.operators1"),
    (1, "userDefine.numberPrefix1"),
    (2, "userDefine.numberPrefix2"),
    (3, "userDefine.numberExtras1"),
    (4, "userDefine.numberExtras2"),
    (5, "userDefine.numberSuffix1"),
    (6, "userDefine.numberSuffix2"),
    (7, "userDefine.numberRange"),
    (10, "userDefine.foldersInCode1Open"),
    (11, "userDefine.foldersInCode1Middle"),
    (12, "userDefine.foldersInCode1Close"),
];

pub const COLORSTYLE_FG: i32 = 1;
pub const COLORSTYLE_BG: i32 = 2;

// Style of Parameters.h for one SCE_USER_STYLE_* id; colours are RGB, None is STYLE_NOT_USED.
#[derive(Debug, Clone, PartialEq)]
pub struct UStyle {
    pub id: usize,
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub color_style: i32,
    pub font_name: String,
    pub font_style: i32,
    pub font_size: i32,
    pub nesting: i32,
}

impl UStyle {
    // StyleArray::addStyler(styleID, styleName): black on white.
    pub fn new(id: usize) -> UStyle {
        UStyle {
            id,
            fg: Some(0),
            bg: Some(0xFFFFFF),
            color_style: COLORSTYLE_FG | COLORSTYLE_BG,
            font_name: String::new(),
            font_style: -1,
            font_size: -1,
            nesting: 0,
        }
    }
}

// UserLangContainer.
#[derive(Debug, Clone, PartialEq)]
pub struct Udl {
    pub name: String,
    pub ext: String,
    pub version: String,
    pub dark: bool,
    pub case_ignored: bool,
    pub fold_comments: bool,
    pub fold_compact: bool,
    pub pure_lc: i32,
    pub decimal: i32,
    pub prefix: [bool; 8],
    pub keywords: Vec<String>,
    pub styles: Vec<UStyle>,
}

impl Udl {
    fn blank(name: &str) -> Udl {
        Udl {
            name: name.into(),
            ext: String::new(),
            version: String::new(),
            dark: false,
            case_ignored: false,
            fold_comments: false,
            fold_compact: false,
            pure_lc: 0,
            decimal: 0,
            prefix: [false; 8],
            keywords: vec![String::new(); KW_TOTAL],
            styles: vec![],
        }
    }

    // UserDefineDialog constructor: a language with the 24 default styles.
    pub fn new(name: &str) -> Udl {
        let mut u = Udl::blank(name);
        u.styles = (0..STYLE_TOTAL).map(UStyle::new).collect();
        u
    }

    // UserLangContainer copy constructor: unset colours become black on white.
    pub fn copy_as(&self, name: &str) -> Udl {
        let mut u = self.clone();
        u.name = name.into();
        for s in u.styles.iter_mut() {
            s.fg = s.fg.or(Some(0));
            s.bg = s.bg.or(Some(0xFFFFFF));
        }
        u
    }

    pub fn style(&self, id: usize) -> Option<&UStyle> {
        self.styles.iter().find(|s| s.id == id)
    }
}

#[derive(Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    kids: Vec<Node>,
    text: String,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.kids.iter().find(|k| k.name == name)
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.kids.iter().filter(move |k| k.name == name)
    }

    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    fn yes(&self, k: &str) -> bool {
        self.attr(k) == Some("yes")
    }

    // pugixml as_int: the leading decimal number, 0 if there is none.
    fn int(&self, k: &str, def: i32) -> i32 {
        self.attr(k).map_or(def, |v| {
            let v = v.trim_start();
            let end = v
                .char_indices()
                .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))))
                .map_or(v.len(), |(i, _)| i);
            v[..end].parse().unwrap_or(0)
        })
    }

    // pugixml drops PCDATA that has only white space.
    fn value(&self) -> Option<&str> {
        (!self
            .text
            .chars()
            .all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')))
        .then_some(&self.text)
    }
}

fn element(e: &BytesStart) -> Node {
    Node {
        name: e.name().as_ref().to_string(),
        attrs: e
            .attributes()
            .flatten()
            .map(|a| {
                let v = a
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map(|v| v.into_owned())
                    .unwrap_or_default();
                (a.key.as_ref().to_string(), v)
            })
            .collect(),
        ..Node::default()
    }
}

fn dom(xml: &str) -> Option<Node> {
    let mut r = Reader::from_str(xml);
    let mut stack = vec![Node::default()];
    loop {
        match r.read_event().ok()? {
            Event::Start(e) => stack.push(element(&e)),
            Event::Empty(e) => stack.last_mut()?.kids.push(element(&e)),
            Event::End(_) => {
                let n = stack.pop()?;
                stack.last_mut()?.kids.push(n);
            }
            Event::Text(t) => stack.last_mut()?.text.push_str(&t.xml10_content()),
            Event::CData(t) => stack.last_mut()?.text.push_str(&t.xml10_content()),
            Event::GeneralRef(g) => {
                let s = match g.resolve_char_ref().ok()? {
                    Some(c) => c.to_string(),
                    None => quick_xml::escape::resolve_predefined_entity(&g)?.to_string(),
                };
                stack.last_mut()?.text.push_str(&s);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    (stack.len() == 1).then(|| stack.pop()).flatten()
}

// Parameters.cpp hexStrVal: None for an invalid value.
fn hex(s: &str) -> Option<u32> {
    if s.is_empty() {
        return Some(0);
    }
    u32::from_str_radix(s, 16).ok().map(|v| v & 0xFFFFFF)
}

// NppParameters::feedUserKeywordList, the "Delimiters" list of a file without udlVersion.
fn old_delimiters(kwl: &str) -> String {
    let b = kwl.as_bytes();
    let c = |i: usize| b.get(i).copied().unwrap_or(b'0');
    let mut t: Vec<u8> = b"00".to_vec();
    let mut add = |pre: &[u8], i: Option<usize>, post: &[u8]| {
        t.extend_from_slice(pre);
        if let Some(ch) = i.map(c).filter(|&ch| ch != b'0') {
            t.push(ch);
        }
        t.extend_from_slice(post);
    };
    add(b"", Some(0), b" 01");
    add(b" 02", Some(3), b"");
    add(b" 03", Some(1), b" 04");
    add(b" 05", Some(4), b"");
    add(b" 06", Some(2), b" 07");
    add(b" 08", Some(5), b"");
    add(b" 09 10 11 12 13 14 15 16 17 18 19 20 21 22 23", None, b"");
    String::from_utf8_lossy(&t).into_owned()
}

// NppParameters::feedUserKeywordList, the "Comment" list of UDL versions before 2.0.
fn old_comments(kwl: &str) -> String {
    let mut t = format!(" {kwl}");
    let mut pos = t.find(" 0");
    while let Some(p) = pos {
        t.replace_range(p..p + 2, " 00");
        pos = t[p + 1..].find(" 0").map(|q| q + p + 1);
    }
    for (from, to) in [(" 1", " 03"), (" 2", " 04")] {
        while let Some(p) = t.find(from) {
            t.replace_range(p..p + 2, to);
        }
    }
    t += " 01 02";
    t.strip_prefix(' ').map(String::from).unwrap_or(t)
}

fn kw_id(name: &str) -> Option<usize> {
    KW_NAMES.iter().position(|n| *n == name).or_else(|| {
        OLD_KW_NAMES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, i)| *i)
    })
}

pub fn style_id(name: &str) -> Option<usize> {
    STYLE_NAMES.iter().position(|n| *n == name).or_else(|| {
        OLD_STYLE_NAMES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, i)| *i)
    })
}

// NppParameters::feedUserLang with feedUserSettings, feedUserKeywordList and feedUserStyles.
fn user_lang(n: &Node) -> Option<Udl> {
    let name = n.attr("name").unwrap_or_default();
    if name.is_empty() {
        return None;
    }
    let mut u = Udl::blank(name);
    u.ext = n.attr("ext").unwrap_or_default().into();
    u.version = n.attr("udlVersion").unwrap_or_default().into();
    u.dark = n.yes("darkModeTheme");
    let settings = n.child("Settings")?;
    if let Some(g) = settings.child("Global") {
        u.case_ignored = g.yes("caseIgnored");
        u.fold_comments = g.yes("allowFoldOfComments");
        u.pure_lc = g.int("forcePureLC", 0);
        u.decimal = g.int("decimalSeparator", 0);
        u.fold_compact = g.yes("foldCompact");
    }
    if let Some(p) = settings.child("Prefix") {
        if u.version == "2.1" || u.version == "2.0" {
            for i in 0..8 {
                u.prefix[i] = p.yes(KW_NAMES[KW_KEYWORDS1 + i]);
            }
        } else {
            for i in 0..4 {
                u.prefix[i] = p.yes(&format!("words{}", i + 1));
            }
        }
    }
    for k in n.child("KeywordLists")?.all("Keywords") {
        let name = k.attr("name").unwrap_or_default();
        let Some(v) = k.value() else { continue };
        if u.version.is_empty() && name == "Delimiters" {
            u.keywords[KW_DELIMITERS] = old_delimiters(v);
        } else if name == "Comment" {
            u.keywords[KW_COMMENTS] = old_comments(v);
        } else if let Some(id) = kw_id(name) {
            u.keywords[id] = if v.len() < MAX_CHAR {
                v.into()
            } else {
                "imported string too long, needs to be < max_char(30720)".into()
            };
        }
    }
    for w in n.child("Styles")?.all("WordsStyle") {
        let Some(id) = w.attr("name").and_then(style_id) else {
            continue;
        };
        if u.style(id).is_some() {
            continue;
        }
        u.styles.push(UStyle {
            id,
            fg: w.attr("fgColor").and_then(hex),
            bg: w.attr("bgColor").and_then(hex),
            color_style: w.int("colorStyle", COLORSTYLE_FG | COLORSTYLE_BG),
            font_name: w.attr("fontName").unwrap_or_default().into(),
            font_style: w.int("fontStyle", -1),
            font_size: w.int("fontSize", -1),
            nesting: w.int("nesting", 0),
        });
    }
    for id in 0..STYLE_TOTAL {
        if u.style(id).is_none() {
            u.styles.push(UStyle::new(id));
        }
    }
    Some(u)
}

// The UserLang elements of a UDL file; None when the file is not valid XML.
pub fn parse(xml: &str) -> Option<Vec<Udl>> {
    let doc = dom(xml.trim_start_matches('\u{feff}'))?;
    Some(doc.child("NotepadPlus").map_or(vec![], |r| {
        r.all("UserLang").filter_map(user_lang).collect()
    }))
}

// The text before the root element: the declaration and comments, which Notepad++ keeps on save.
pub fn prolog(xml: &str) -> String {
    let xml = xml.trim_start_matches('\u{feff}');
    xml.find("<NotepadPlus")
        .map_or(DECLARATION.to_string(), |i| xml[..i].to_string())
}

// pugixml text escapes: PCDATA keeps tab, CR and LF; attributes keep only tab.
fn esc(s: &str, attr: bool) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o += "&amp;",
            '<' => o += "&lt;",
            '>' => o += "&gt;",
            '"' if attr => o += "&quot;",
            '\t' => o.push(c),
            '\r' | '\n' if !attr => o.push(c),
            c if (c as u32) < 32 => o += &format!("&#{};", c as u32),
            c => o.push(c),
        }
    }
    o
}

fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

// NppParameters::insertUserLang2Tree, written as saveFileUDL does (4 space indent, LF line ends).
fn lang_xml(o: &mut String, u: &Udl) {
    let dark = if u.dark { " darkModeTheme=\"yes\"" } else { "" };
    *o += &format!(
        "    <UserLang name=\"{}\" ext=\"{}\"{dark} udlVersion=\"{VERSION}\">\n        <Settings>\n",
        esc(&u.name, true),
        esc(&u.ext, true)
    );
    *o += &format!(
        "            <Global caseIgnored=\"{}\" allowFoldOfComments=\"{}\" foldCompact=\"{}\" forcePureLC=\"{}\" decimalSeparator=\"{}\" />\n",
        yn(u.case_ignored),
        yn(u.fold_comments),
        yn(u.fold_compact),
        u.pure_lc,
        u.decimal
    );
    let prefix: Vec<String> = (0..8)
        .map(|i| format!("{}=\"{}\"", KW_NAMES[KW_KEYWORDS1 + i], yn(u.prefix[i])))
        .collect();
    *o += &format!(
        "            <Prefix {} />\n        </Settings>\n        <KeywordLists>\n",
        prefix.join(" ")
    );
    for (name, words) in KW_NAMES.iter().zip(&u.keywords) {
        let words = words.replace("\r\n", "\n").replace('\r', "\n");
        *o += &format!(
            "            <Keywords name=\"{}\">{}</Keywords>\n",
            esc(name, true),
            esc(&words, false)
        );
    }
    *o += "        </KeywordLists>\n        <Styles>\n";
    for s in &u.styles {
        let mut a = format!(
            "name=\"{}\" fgColor=\"{:06X}\" bgColor=\"{:06X}\"",
            STYLE_NAMES[s.id],
            s.fg.unwrap_or(0xFFFFFF),
            s.bg.unwrap_or(0xFFFFFF)
        );
        if s.color_style != COLORSTYLE_FG | COLORSTYLE_BG {
            a += &format!(" colorStyle=\"{}\"", s.color_style);
        }
        if !s.font_name.is_empty() {
            a += &format!(" fontName=\"{}\"", esc(&s.font_name, true));
        }
        a += &format!(" fontStyle=\"{}\"", s.font_style.max(0));
        match s.font_size {
            -1 => {}
            0 => a += " fontSize=\"\"",
            n => a += &format!(" fontSize=\"{n}\""),
        }
        a += &format!(" nesting=\"{}\"", s.nesting);
        *o += &format!("            <WordsStyle {a} />\n");
    }
    *o += "        </Styles>\n    </UserLang>\n";
}

pub fn to_xml(prolog: &str, langs: &[&Udl]) -> String {
    let mut o = format!("{prolog}<NotepadPlus>\n");
    langs.iter().for_each(|u| lang_xml(&mut o, u));
    o + "</NotepadPlus>\n"
}

// NppParameters::getUserDefinedLangNameFromExt: the first UDL of the current mode that matches, else the last match.
pub fn for_file_name<'a>(
    langs: impl IntoIterator<Item = &'a Udl>,
    file_name: &str,
    dark: bool,
) -> Option<usize> {
    let ext = file_name.rfind('.').map(|i| &file_name[i + 1..])?;
    if ext.is_empty() || ext.contains(' ') {
        return None;
    }
    let mut matched = None;
    for (i, u) in langs.into_iter().enumerate() {
        for e in u.ext.split_whitespace() {
            if e.eq_ignore_ascii_case(ext) || e.eq_ignore_ascii_case(file_name) {
                if u.dark == dark {
                    return Some(i);
                }
                matched = Some(i);
            }
        }
    }
    matched
}

// ScintillaEditView::setUserLexer, the quote handling for the SCI_SETKEYWORDS lists.
fn keyword_list(s: &str) -> String {
    let b = s.as_bytes();
    let at = |j: usize| b.get(j).copied().unwrap_or(0);
    let (mut dq, mut sq, mut non_ws) = (false, false, false);
    let mut out: Vec<u8> = vec![];
    let mut j = 0;
    while j < b.len() && out.len() < MAX_CHAR - 1 {
        let c = b[j];
        if !sq && c == b'"' {
            dq = !dq;
        } else if !dq && c == b'\'' {
            sq = !sq;
        } else if c == b'\\' && matches!(at(j + 1), b'"' | b'\'' | b'\\') {
            j += 1;
            out.push(b[j]);
        } else if dq || sq {
            if c > b' ' {
                out.push(c);
                non_ws = true;
            } else if non_ws
                && at(j.wrapping_sub(1)) != b'"'
                && at(j + 1) != b'"'
                && at(j + 1) > b' '
            {
                out.push(if dq { 0x0B } else { 0x08 });
            }
        } else {
            out.push(c);
        }
        j += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ScintillaEditView::setUserLexer: the SCI_SETPROPERTY pairs and the SCI_SETKEYWORDS lists, in its order.
pub fn lexer_setup(
    u: &Udl,
    udl_id: usize,
    buffer_id: usize,
) -> (Vec<(String, String)>, Vec<(usize, String)>) {
    let b = |v: bool| if v { "1" } else { "0" }.to_string();
    let mut props = vec![
        ("fold".to_string(), "1".to_string()),
        ("userDefine.isCaseIgnored".into(), b(u.case_ignored)),
        ("userDefine.allowFoldOfComments".into(), b(u.fold_comments)),
        ("userDefine.foldCompact".into(), b(u.fold_compact)),
    ];
    for (i, p) in u.prefix.iter().enumerate() {
        props.push((format!("userDefine.prefixKeywords{}", i + 1), b(*p)));
    }
    let mut words = vec![];
    for (i, list) in u.keywords.iter().enumerate() {
        match LEXER_PROPS.iter().find(|(k, _)| *k == i) {
            Some((_, key)) => props.push((key.to_string(), list.clone())),
            None => words.push((words.len(), keyword_list(list))),
        }
    }
    props.push(("userDefine.forcePureLC".into(), u.pure_lc.to_string()));
    props.push(("userDefine.decimalSeparator".into(), u.decimal.to_string()));
    props.push(("userDefine.udlName".into(), udl_id.to_string()));
    props.push(("userDefine.currentBufferID".into(), buffer_id.to_string()));
    for s in &u.styles {
        props.push((
            format!("userDefine.nesting.{:02}", s.id),
            s.nesting.to_string(),
        ));
    }
    (props, words)
}

// Port of convertTo in UserDefineDialog.cpp: one dialog field into a prefixed list.
pub fn convert_to(dest: &mut String, field: &str, prefix: &str) {
    let b = field.as_bytes();
    let at = |i: isize| {
        if i < 0 {
            0
        } else {
            b.get(i as usize).copied().unwrap_or(0)
        }
    };
    let p = prefix.as_bytes();
    let mut out: Vec<u8> = vec![];
    if !dest.is_empty() {
        out.push(b' ');
    }
    out.extend_from_slice(&p[..2]);
    let mut in_group = false;
    let mut i = 0isize;
    while (i as usize) < b.len() && dest.len() + out.len() < MAX_CHAR - 7 {
        if i == 0 && at(0) == b'(' && at(1) == b'(' {
            in_group = true;
        } else if at(i) == b' ' && at(i + 1) == b'(' && at(i + 2) == b'(' {
            in_group = true;
            out.extend_from_slice(&[b' ', p[0], p[1]]);
            i += 1;
        }
        if in_group && at(i - 1) == b')' && at(i - 2) == b')' {
            in_group = false;
        }
        if at(i) == b' ' {
            if at(i + 1) != b' ' && at(i + 1) != 0 {
                out.push(b' ');
                if !in_group {
                    out.extend_from_slice(&p[..2]);
                }
            }
        } else {
            out.push(at(i));
        }
        i += 1;
    }
    dest.push_str(&String::from_utf8_lossy(&out));
}

// Port of CommentStyleDialog::retrieve and SymbolsStyleDialog::retrieve: the dialog field for one prefix.
pub fn retrieve(list: &str, prefix: &str) -> String {
    let b = list.as_bytes();
    let at = |i: isize| {
        if i < 0 {
            0
        } else {
            b.get(i as usize).copied().unwrap_or(0)
        }
    };
    let p = prefix.as_bytes();
    let mut out: Vec<u8> = vec![];
    let (mut copy, mut in_group) = (false, false);
    let mut i = 0isize;
    while (i as usize) < b.len() {
        if (i == 0 || at(i - 1) == b' ') && at(i) == p[0] && at(i + 1) == p[1] {
            if !out.is_empty() {
                out.push(b' ');
            }
            copy = true;
            i += 2;
            continue;
        }
        if at(i) == b'(' && at(i + 1) == b'(' && !in_group && copy {
            in_group = true;
        }
        if at(i) != b')' && at(i - 1) == b')' && at(i - 2) == b')' && in_group {
            in_group = false;
        }
        if at(i) == b' ' && copy {
            copy = false;
        }
        if copy || in_group {
            out.push(at(i));
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// The two character prefix of field k: "00".."09", then "10".."23".
pub fn prefix(k: usize) -> String {
    format!("{k:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Written by hand from NppParameters::insertUserLang2Tree and the userDefinedLanguages collection format.
    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<!-- Sample UDL -->
<NotepadPlus>
    <UserLang name="Sample &amp; Co" ext="smp SAMPLE.cfg" udlVersion="2.1">
        <Settings>
            <Global caseIgnored="yes" allowFoldOfComments="no" foldCompact="yes" forcePureLC="1" decimalSeparator="2" />
            <Prefix Keywords1="no" Keywords2="yes" Keywords3="no" Keywords4="no" Keywords5="no" Keywords6="no" Keywords7="no" Keywords8="yes" />
        </Settings>
        <KeywordLists>
            <Keywords name="Comments">00// 01 02 03/* 04*/</Keywords>
            <Keywords name="Numbers, prefix1">0x</Keywords>
            <Keywords name="Numbers, prefix2"></Keywords>
            <Keywords name="Numbers, extras1">A B C D E F</Keywords>
            <Keywords name="Numbers, extras2"></Keywords>
            <Keywords name="Numbers, suffix1"></Keywords>
            <Keywords name="Numbers, suffix2"></Keywords>
            <Keywords name="Numbers, range"></Keywords>
            <Keywords name="Operators1">+ - * / &lt; &gt;</Keywords>
            <Keywords name="Operators2">and or</Keywords>
            <Keywords name="Folders in code1, open">{</Keywords>
            <Keywords name="Folders in code1, middle"></Keywords>
            <Keywords name="Folders in code1, close">}</Keywords>
            <Keywords name="Folders in code2, open">begin</Keywords>
            <Keywords name="Folders in code2, middle"></Keywords>
            <Keywords name="Folders in code2, close">end</Keywords>
            <Keywords name="Folders in comment, open"></Keywords>
            <Keywords name="Folders in comment, middle"></Keywords>
            <Keywords name="Folders in comment, close"></Keywords>
            <Keywords name="Keywords1">if then else &quot;else if&quot;</Keywords>
            <Keywords name="Keywords2">$</Keywords>
            <Keywords name="Keywords3"></Keywords>
            <Keywords name="Keywords4"></Keywords>
            <Keywords name="Keywords5"></Keywords>
            <Keywords name="Keywords6"></Keywords>
            <Keywords name="Keywords7"></Keywords>
            <Keywords name="Keywords8">@</Keywords>
            <Keywords name="Delimiters">00&quot; 01\ 02&quot; 03' 04 05' 06 07 08 09 10 11 12 13 14 15 16 17 18 19 20 21 22 23</Keywords>
        </KeywordLists>
        <Styles>
            <WordsStyle name="DEFAULT" fgColor="000000" bgColor="FFFFFF" colorStyle="0" fontName="Menlo" fontStyle="0" fontSize="12" nesting="0" />
            <WordsStyle name="COMMENTS" fgColor="008000" bgColor="FFFFFF" colorStyle="1" fontStyle="2" nesting="0" />
            <WordsStyle name="KEYWORDS1" fgColor="0000FF" bgColor="FFFFFF" fontStyle="1" fontSize="" nesting="0" />
            <WordsStyle name="DELIMITERS1" fgColor="808080" bgColor="FFFFFF" colorStyle="1" fontStyle="0" nesting="1024" />
        </Styles>
    </UserLang>
</NotepadPlus>
"#;

    #[test]
    fn parse_sample() {
        let v = parse(SAMPLE).unwrap();
        assert_eq!(v.len(), 1);
        let u = &v[0];
        assert_eq!(u.name, "Sample & Co");
        assert_eq!(u.ext, "smp SAMPLE.cfg");
        assert_eq!(u.version, "2.1");
        assert!(u.case_ignored && !u.fold_comments && u.fold_compact);
        assert_eq!((u.pure_lc, u.decimal), (1, 2));
        assert_eq!(
            u.prefix,
            [false, true, false, false, false, false, false, true]
        );
        assert_eq!(u.keywords[KW_COMMENTS], "00// 01 02 03/* 04*/");
        assert_eq!(u.keywords[8], "+ - * / < >");
        assert_eq!(u.keywords[KW_KEYWORDS1], "if then else \"else if\"");
        assert_eq!(u.styles.len(), STYLE_TOTAL);
        let d = u.style(0).unwrap();
        assert_eq!(
            (d.color_style, d.font_name.as_str(), d.font_size),
            (0, "Menlo", 12)
        );
        let k = u.style(4).unwrap();
        assert_eq!(
            (k.fg, k.color_style, k.font_style, k.font_size),
            (Some(0xFF), 3, 1, 0)
        );
        assert_eq!(u.style(16).unwrap().nesting, 1024);
        assert_eq!(u.style(5), Some(&UStyle::new(5)));
    }

    #[test]
    fn xml_round_trip() {
        let v = parse(SAMPLE).unwrap();
        let out = to_xml(&prolog(SAMPLE), &v.iter().collect::<Vec<_>>());
        assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<!-- Sample UDL -->\n<NotepadPlus>\n    <UserLang name=\"Sample &amp; Co\" ext=\"smp SAMPLE.cfg\" udlVersion=\"2.1\">\n"));
        assert!(out
            .contains("            <Keywords name=\"Operators1\">+ - * / &lt; &gt;</Keywords>\n"));
        assert!(out.contains("            <Keywords name=\"Numbers, prefix2\"></Keywords>\n"));
        assert!(out.contains("            <WordsStyle name=\"DEFAULT\" fgColor=\"000000\" bgColor=\"FFFFFF\" colorStyle=\"0\" fontName=\"Menlo\" fontStyle=\"0\" fontSize=\"12\" nesting=\"0\" />\n"));
        assert!(out.contains("<WordsStyle name=\"KEYWORDS1\" fgColor=\"0000FF\" bgColor=\"FFFFFF\" fontStyle=\"1\" fontSize=\"\" nesting=\"0\" />"));
        assert!(out.contains("<WordsStyle name=\"KEYWORDS2\" fgColor=\"000000\" bgColor=\"FFFFFF\" fontStyle=\"0\" nesting=\"0\" />"));
        let again = parse(&out).unwrap();
        let mut want = v.clone();
        for st in want[0].styles.iter_mut() {
            st.font_style = st.font_style.max(0);
        }
        assert_eq!(again, want);
        assert_eq!(
            to_xml(&prolog(&out), &again.iter().collect::<Vec<_>>()),
            out
        );
    }

    #[test]
    fn preinstalled_markdown_round_trip() {
        let src =
            include_str!("../../../PowerEditor/bin/userDefineLangs/markdown._preinstalled.udl.xml");
        let v = parse(src).unwrap();
        assert_eq!(v[0].name, "Markdown (preinstalled)");
        assert_eq!(v[0].ext, "md markdown");
        assert_eq!(v[0].style(19).unwrap().nesting, 65600);
        let out = to_xml(&prolog(src), &v.iter().collect::<Vec<_>>());
        assert!(out.contains("Markdown-plus-plus is a project"));
        assert_eq!(parse(&out).unwrap(), v);
        let dm = include_str!(
            "../../../PowerEditor/bin/userDefineLangs/markdown._preinstalled_DM.udl.xml"
        );
        assert!(parse(dm).unwrap()[0].dark);
    }

    #[test]
    fn invalid_and_partial_files() {
        assert_eq!(parse("<NotepadPlus><UserLang name=\"x\">"), None);
        assert_eq!(parse("<Other/>"), Some(vec![]));
        let no_styles = "<NotepadPlus><UserLang name=\"a\"><Settings/><KeywordLists/></UserLang><UserLang name=\"\"><Settings/><KeywordLists/><Styles/></UserLang><UserLang name=\"b\"><Settings/><KeywordLists/><Styles/></UserLang></NotepadPlus>";
        let v = parse(no_styles).unwrap();
        assert_eq!(v.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), ["b"]);
        assert_eq!(v[0], Udl::new("b"));
    }

    #[test]
    fn old_versions() {
        let xml = "<NotepadPlus><UserLang name=\"old\" ext=\"o\"><Settings><Prefix words1=\"yes\" words3=\"yes\" /></Settings><KeywordLists>\
            <Keywords name=\"Delimiters\">&quot;0&quot;(0)</Keywords><Keywords name=\"Comment\">1/* 2*/ 0//</Keywords>\
            <Keywords name=\"Words1\">if</Keywords><Keywords name=\"Operators\">+</Keywords><Keywords name=\"Folder+\">{</Keywords></KeywordLists>\
            <Styles><WordsStyle name=\"FOLDEROPEN\" fgColor=\"FF0000\" /><WordsStyle name=\"FOLDERCLOSE\" fgColor=\"00FF00\" /><WordsStyle name=\"COMMENT LINE\" fgColor=\"zz\" /></Styles></UserLang></NotepadPlus>";
        let u = &parse(xml).unwrap()[0];
        assert_eq!(u.prefix[..4], [true, false, true, false]);
        assert_eq!(
            u.keywords[KW_DELIMITERS],
            "00\" 01 02( 03 04 05 06\" 07 08) 09 10 11 12 13 14 15 16 17 18 19 20 21 22 23"
        );
        assert_eq!(u.keywords[KW_COMMENTS], "03/* 04*/ 00// 01 02");
        assert_eq!(u.keywords[KW_KEYWORDS1], "if");
        assert_eq!(u.keywords[8], "+");
        assert_eq!(u.keywords[10], "{");
        assert_eq!(u.style(13).unwrap().fg, Some(0xFF0000));
        assert_eq!(u.style(2).unwrap().fg, None);
    }

    #[test]
    fn ext_detection() {
        let mut a = Udl::new("A");
        a.ext = "abc cfg".into();
        let mut dark = Udl::new("D");
        dark.ext = "md".into();
        dark.dark = true;
        let mut light = Udl::new("L");
        light.ext = "MD markdown".into();
        let mut full = Udl::new("F");
        full.ext = "my.conf".into();
        let v = vec![a, dark.clone(), light, full];
        assert_eq!(for_file_name(&v, "x.ABC", false), Some(0));
        assert_eq!(for_file_name(&v, "a.b.cfg", false), Some(0));
        assert_eq!(for_file_name(&v, "notes.md", false), Some(2));
        assert_eq!(for_file_name(&v, "my.conf", false), Some(3));
        assert_eq!(for_file_name(&v, "abc", false), None);
        assert_eq!(for_file_name(&v, "x.a bc", false), None);
        assert_eq!(for_file_name(&v, "x.", false), None);
        assert_eq!(for_file_name(&[dark.clone()], "r.md", false), Some(0));
        assert_eq!(for_file_name(&v, "notes.md", true), Some(1));
        assert_eq!(for_file_name(&v, "x.abc", true), Some(0));
    }

    #[test]
    fn lexer_properties_match_set_user_lexer() {
        let u = &parse(SAMPLE).unwrap()[0];
        let (props, words) = lexer_setup(u, 7, 42);
        let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(props[0], ("fold".into(), "1".into()));
        assert_eq!(get("userDefine.isCaseIgnored"), Some("1"));
        assert_eq!(get("userDefine.allowFoldOfComments"), Some("0"));
        assert_eq!(get("userDefine.foldCompact"), Some("1"));
        assert_eq!(get("userDefine.prefixKeywords2"), Some("1"));
        assert_eq!(get("userDefine.prefixKeywords8"), Some("1"));
        assert_eq!(get("userDefine.prefixKeywords1"), Some("0"));
        assert_eq!(get("userDefine.comments"), Some("00// 01 02 03/* 04*/"));
        assert_eq!(get("userDefine.operators1"), Some("+ - * / < >"));
        assert_eq!(get("userDefine.numberPrefix1"), Some("0x"));
        assert_eq!(get("userDefine.numberExtras1"), Some("A B C D E F"));
        assert_eq!(get("userDefine.foldersInCode1Open"), Some("{"));
        assert_eq!(get("userDefine.foldersInCode1Close"), Some("}"));
        assert!(get("userDefine.delimiters")
            .unwrap()
            .starts_with("00\" 01\\ 02\""));
        assert_eq!(get("userDefine.forcePureLC"), Some("1"));
        assert_eq!(get("userDefine.decimalSeparator"), Some("2"));
        assert_eq!(get("userDefine.udlName"), Some("7"));
        assert_eq!(get("userDefine.currentBufferID"), Some("42"));
        assert_eq!(get("userDefine.nesting.16"), Some("1024"));
        assert_eq!(get("userDefine.nesting.00"), Some("0"));
        assert_eq!(props.len(), 4 + 8 + 13 + 4 + STYLE_TOTAL);
        let w: Vec<(usize, &str)> = words.iter().map(|(i, s)| (*i, s.as_str())).collect();
        assert_eq!(w.len(), 15);
        assert_eq!(w[0], (0, "and or"));
        assert_eq!(w[1], (1, "begin"));
        assert_eq!(w[3], (3, "end"));
        assert_eq!(w[7], (7, "if then else else\u{0B}if"));
        assert_eq!(w[8], (8, "$"));
        assert_eq!(w[14], (14, "@"));
    }

    #[test]
    fn keyword_quotes() {
        assert_eq!(keyword_list("a 'b c' \"d  e\""), "a b\u{08}c d\u{0B}e");
        assert_eq!(keyword_list(r#"\" \' \\ x"#), "\" ' \\ x");
        assert_eq!(keyword_list("\" a\""), "a");
        assert_eq!(keyword_list("'end if'"), "end\u{08}if");
    }

    #[test]
    fn dialog_lists() {
        let mut d = String::new();
        for (k, f) in ["//", "", "", "/*", "*/"].iter().enumerate() {
            convert_to(&mut d, f, &prefix(k));
        }
        assert_eq!(d, "00// 01 02 03/* 04*/");
        for (k, f) in ["//", "", "", "/*", "*/"].iter().enumerate() {
            assert_eq!(retrieve(&d, &prefix(k)), *f);
        }
        let mut d = String::new();
        convert_to(&mut d, "a b", "00");
        convert_to(&mut d, "((EOL x))", "02");
        convert_to(&mut d, "x ((EOL y))", "05");
        assert_eq!(d, "00a 00b 02((EOL x)) 05x 05((EOL y))");
        assert_eq!(retrieve(&d, "00"), "a b");
        assert_eq!(retrieve(&d, "02"), "((EOL x))");
        assert_eq!(retrieve(&d, "05"), "x ((EOL y))");
        let md = parse(include_str!(
            "../../../PowerEditor/bin/userDefineLangs/markdown._preinstalled.udl.xml"
        ))
        .unwrap();
        let c = &md[0].keywords[KW_COMMENTS];
        assert_eq!(retrieve(c, "00"), "#");
        assert_eq!(retrieve(c, "02"), "((EOL))");
        assert_eq!(retrieve(c, "03"), "<!--");
        let delim = &md[0].keywords[KW_DELIMITERS];
        assert_eq!(retrieve(delim, "00"), "![ [");
        assert_eq!(retrieve(delim, "05"), "``` ((EOL `)) ~~~");
        let mut back = String::new();
        for k in 0..24 {
            convert_to(&mut back, &retrieve(delim, &prefix(k)), &prefix(k));
        }
        assert_eq!(&back, delim);
    }
}
