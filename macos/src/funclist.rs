// SPDX-License-Identifier: GPL-3.0-or-later
use crate::lang::LEXERS;
use crate::search::Doc;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

include!(concat!(env!("OUT_DIR"), "/function_lists.rs"));

// FunctionParser search flags: SCFIND_REGEXP | SCFIND_POSIX | SCFIND_REGEXP_DOTMATCHESNL.
const FLAGS: u32 = 0x0020_0000 | 0x0040_0000 | 0x1000_0000;

#[derive(Debug, Default)]
struct Elem {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Elem>,
}

impl Elem {
    fn child(&self, name: &str) -> Option<&Elem> {
        self.children.iter().find(|c| c.name == name)
    }

    fn attr(&self, key: &str) -> &str {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map_or("", |(_, v)| v.as_str())
    }

    // Values of `key` on the `name` children of the first `parent` child, without empty values.
    fn exprs(&self, parent: &str, name: &str) -> Vec<String> {
        self.child(parent).map_or(vec![], |p| {
            p.children
                .iter()
                .filter(|c| c.name == name && !c.attr("expr").is_empty())
                .map(|c| c.attr("expr").to_string())
                .collect()
        })
    }
}

fn elem(e: &BytesStart) -> Elem {
    let attrs = e
        .attributes()
        .flatten()
        .map(|a| {
            let raw = a.value.to_string();
            let v = quick_xml::escape::unescape(&raw).map_or(raw.clone(), |v| v.into_owned());
            (a.key.as_ref().to_string(), v)
        })
        .collect();
    Elem {
        name: e.name().as_ref().to_string(),
        attrs,
        children: vec![],
    }
}

// Moves the open element at the top of the stack into its parent.
fn close(stack: &mut Vec<Elem>) {
    if stack.len() > 1 {
        if let Some(el) = stack.pop() {
            add(stack, el);
        }
    }
}

fn add(stack: &mut [Elem], el: Elem) {
    if let Some(top) = stack.last_mut() {
        top.children.push(el);
    }
}

// Loads like pugixml with parse_eol: line ends become LF, attribute values keep their white space.
fn load_xml(text: &str) -> Elem {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut r = Reader::from_str(&text);
    let mut stack = vec![Elem::default()];
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => stack.push(elem(&e)),
            Ok(Event::Empty(e)) => add(&mut stack, elem(&e)),
            Ok(Event::End(_)) => close(&mut stack),
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().unwrap_or_default()
}

fn rule_text(file: &str) -> Option<&'static str> {
    FUNCTION_LISTS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(file))
        .map(|(_, t)| *t)
}

// LangType of a language name: the index in ScintillaEditView::_langNameInfoArray.
fn lang_id(lang: &str) -> Option<usize> {
    LEXERS.iter().position(|(l, _)| *l == lang).map(|i| i + 1)
}

// FunctionParsersManager: an overrideMap.xml association for the LangType, else "<language name>.xml".
pub fn rule_file(lang: &str) -> String {
    let lang = if lang == "javascript" {
        "javascript.js"
    } else {
        lang
    };
    let id = lang_id(lang);
    let map = rule_text("overrideMap.xml")
        .map(load_xml)
        .unwrap_or_default();
    let assoc = map
        .child("NotepadPlus")
        .and_then(|r| r.child("functionList"))
        .and_then(|r| r.child("associationMap"))
        .and_then(|m| {
            m.children.iter().find(|a| {
                a.name == "association"
                    && !a.attr("id").is_empty()
                    && a.attr("langID").parse::<usize>().ok() == id
            })
        });
    match assoc {
        Some(a) if id.is_some() => a.attr("id").to_string(),
        _ => format!("{lang}.xml"),
    }
}

#[derive(Debug, Default)]
struct Rule {
    main: String,
    names: Vec<String>,
    classes: Vec<String>,
}

#[derive(Debug, Default)]
struct Zone {
    main: String,
    open: String,
    close: String,
    rule: Rule,
}

#[derive(Debug)]
pub struct Parser {
    comment: String,
    zone: Option<Zone>,
    unit: Option<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub name: String,
    pub class: String,
    pub pos: isize,
}

type Zones = Vec<(isize, isize)>;

fn search(doc: &Doc, pat: &str, b: isize, e: isize) -> Option<(isize, isize)> {
    doc.find(b, e, pat.as_bytes(), FLAGS).ok().flatten()
}

// ScintillaEditView::getGenericText into a 1024 byte buffer: stops at a NUL byte and after 1023 bytes.
fn text(doc: &Doc, s: isize, e: isize) -> String {
    let mut b = doc.range(s, e);
    if let Some(n) = b.iter().position(|&c| c == 0) {
        b.truncate(n);
    }
    b.truncate(1023);
    String::from_utf8_lossy(&b).into_owned()
}

fn in_zones(pos: isize, zones: &[(isize, isize)]) -> bool {
    zones.iter().any(|&(s, e)| pos >= s && pos < e)
}

// FunctionParser::getInvertZones, with its one character gap on each side of a zone.
fn invert(src: &[(isize, isize)], b: isize, e: isize) -> Zones {
    let Some(first) = src.first() else {
        return vec![(b, e)];
    };
    let mut v = vec![];
    if b < first.0 {
        v.push((b, first.0 - 1));
    }
    for w in src.windows(2) {
        if w[0].1 + 1 < w[1].0 - 1 {
            v.push((w[0].1 + 1, w[1].0 - 1));
        }
    }
    let last = src.last().map_or(first.1, |z| z.1) + 1;
    if last < e {
        v.push((last, e));
    }
    v
}

fn sub_level(doc: &Doc, b: isize, e: isize, exprs: &[String], found: &mut isize) -> String {
    if b >= e {
        *found = -1;
        return String::new();
    }
    let Some(first) = exprs.first() else {
        return String::new();
    };
    let Some((s, t)) = search(doc, first, b, e) else {
        *found = -1;
        return String::new();
    };
    if exprs.len() >= 2 {
        return sub_level(doc, s, t, &exprs[1..], found);
    }
    *found = s;
    text(doc, s, t)
}

impl Rule {
    // FunctionParser::funcParse.
    fn parse(
        &self,
        doc: &Doc,
        out: &mut Vec<Found>,
        mut b: isize,
        e: isize,
        class: &str,
        comments: Option<&[(isize, isize)]>,
    ) {
        if b >= e || self.main.is_empty() {
            return;
        }
        while let Some((s, t)) = search(doc, &self.main, b, e) {
            if t >= e {
                break;
            }
            let mut f = Found {
                name: String::new(),
                class: String::new(),
                pos: -1,
            };
            let mut pos2 = -1;
            if self.names.is_empty() && self.classes.is_empty() {
                f.name = text(doc, s, t);
                f.pos = s;
            } else {
                let mut found = -1;
                if !self.names.is_empty() {
                    f.name = sub_level(doc, s, t, &self.names, &mut found);
                    f.pos = found;
                }
                if !class.is_empty() {
                    f.class = class.to_string();
                } else if !self.classes.is_empty() {
                    f.class = sub_level(doc, s, t, &self.classes, &mut found);
                    pos2 = found;
                }
            }
            let outside = comments.is_none_or(|z| !in_zones(f.pos, z) && !in_zones(pos2, z));
            if (f.pos != -1 || pos2 != -1) && outside {
                out.push(f);
            }
            b = t;
        }
    }
}

impl Zone {
    // FunctionZoneParser::getBodyClosePos.
    fn body_close(&self, doc: &Doc, b: isize, comments: &[(isize, isize)]) -> isize {
        let len = doc.len();
        if b >= len {
            return len;
        }
        let expr = format!("({}|{})", self.open, self.close);
        let mut open = 1;
        let mut hit = search(doc, &expr, b, len);
        loop {
            let end = match hit {
                Some((s, t)) => {
                    if !in_zones(s, comments) {
                        if search(doc, &self.open, s, t).is_some() {
                            open += 1;
                        } else {
                            open -= 1;
                        }
                    }
                    t
                }
                None => {
                    open = 0;
                    b
                }
            };
            if open == 0 {
                return end;
            }
            hit = search(doc, &expr, end, len);
        }
    }

    // FunctionZoneParser::classParse.
    fn parse(
        &self,
        doc: &Doc,
        out: &mut Vec<Found>,
        scanned: &mut Zones,
        comments: &[(isize, isize)],
        mut b: isize,
        e: isize,
    ) {
        if b >= e {
            return;
        }
        while let Some((s, mut t)) = search(doc, &self.main, b, e) {
            let mut found = 0;
            let class = sub_level(doc, s, t, &self.rule.classes, &mut found);
            if !self.open.is_empty() && !self.close.is_empty() {
                t = self.body_close(doc, t, comments);
            }
            if t > e {
                break;
            }
            scanned.push((s, t));
            if t == e {
                break;
            }
            if !in_zones(s, comments) {
                self.rule.parse(doc, out, s, t, &class, Some(comments));
            }
            b = t;
        }
    }
}

impl Parser {
    fn comment_zones(&self, doc: &Doc, mut b: isize, e: isize) -> Zones {
        let mut z = vec![];
        if b >= e || self.comment.is_empty() {
            return z;
        }
        while let Some((s, t)) = search(doc, &self.comment, b, e) {
            if t > e {
                break;
            }
            z.push((s, t));
            if t == e {
                break;
            }
            b = t;
        }
        z
    }

    // FunctionMixParser, FunctionZoneParser and FunctionUnitParser::parse over the whole document.
    pub fn parse(&self, doc: &Doc) -> Vec<Found> {
        let (b, e) = (0, doc.len());
        let mut out = vec![];
        let comments = self.comment_zones(doc, b, e);
        match (&self.zone, &self.unit) {
            (Some(z), Some(u)) => {
                let mut scanned = vec![];
                z.parse(doc, &mut out, &mut scanned, &comments, b, e);
                for &(s, t) in &scanned {
                    z.parse(doc, &mut out, &mut vec![], &comments, s, t);
                }
                for (s, t) in invert(&scanned, b, e) {
                    u.parse(doc, &mut out, s, t, "", Some(&comments));
                }
            }
            (Some(z), None) => {
                let mut scanned = vec![];
                for (s, t) in invert(&comments, b, e) {
                    z.parse(doc, &mut out, &mut scanned, &comments, s, t);
                }
            }
            (None, Some(u)) => {
                for (s, t) in invert(&comments, b, e) {
                    u.parse(doc, &mut out, s, t, "", None);
                }
            }
            (None, None) => {}
        }
        out
    }
}

// FunctionParsersManager::loadFuncListFromXmlTree.
fn load_parser(text: &str) -> Option<Parser> {
    let root = load_xml(text);
    let p = root
        .child("NotepadPlus")?
        .child("functionList")?
        .child("parser")?;
    if p.attr("id").is_empty() {
        return None;
    }
    let zone = p.child("classRange").map(|c| Zone {
        main: c.attr("mainExpr").to_string(),
        open: c.attr("openSymbole").to_string(),
        close: c.attr("closeSymbole").to_string(),
        rule: Rule {
            main: c
                .child("function")
                .map_or("", |f| f.attr("mainExpr"))
                .to_string(),
            names: c
                .child("function")
                .map_or(vec![], |f| f.exprs("functionName", "funcNameExpr")),
            classes: c.exprs("className", "nameExpr"),
        },
    });
    let unit = p.child("function").map(|f| Rule {
        main: f.attr("mainExpr").to_string(),
        names: f.exprs("functionName", "nameExpr"),
        classes: f.exprs("className", "nameExpr"),
    });
    if zone.is_none() && unit.is_none() {
        return None;
    }
    Some(Parser {
        comment: p.attr("commentExpr").to_string(),
        zone,
        unit,
    })
}

pub fn parser_for(lang: &str) -> Option<Parser> {
    load_parser(rule_text(&rule_file(lang))?)
}

pub struct Node {
    pub label: String,
    pub pos: isize,
    pub children: Vec<Node>,
}

// FunctionListPanel::addEntry: the first top item with the class name gets the function, also when that item is a function.
pub fn tree(found: &[Found]) -> Vec<Node> {
    let mut v: Vec<Node> = vec![];
    for f in found {
        let leaf = Node {
            label: f.name.clone(),
            pos: f.pos,
            children: vec![],
        };
        if f.class.is_empty() {
            v.push(leaf);
            continue;
        }
        match v.iter_mut().find(|n| n.label == f.class)
        {
            Some(n) => n.children.push(leaf),
            None => v.push(Node {
                label: f.class.clone(),
                pos: f.pos,
                children: vec![leaf],
            }),
        }
    }
    v
}

// Sort (TVM_SORTCHILDREN, by name) or the unsorted view (categorySortFunc, by position), on all levels.
pub fn sort(nodes: &mut [Node], by_name: bool) {
    if by_name {
        nodes.sort_by_key(|n| n.label.to_lowercase());
    } else {
        nodes.sort_by_key(|n| n.pos);
    }
    for n in nodes {
        sort(&mut n.children, by_name);
    }
}

// TreeView::searchLeafAndBuildTree: the leaves whose name contains the text, ignoring case, in one level.
pub fn filter(nodes: &[Node], text: &str) -> Vec<Node> {
    let t = text.to_uppercase();
    let mut v = vec![];
    for n in nodes {
        if n.children.is_empty() {
            if n.label.to_uppercase().contains(&t) {
                v.push(Node {
                    label: n.label.clone(),
                    pos: n.pos,
                    children: vec![],
                });
            }
        } else {
            v.extend(filter(&n.children, text));
        }
    }
    v
}

#[cfg(test)]
fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o += "\\\"",
            '\\' => o += "\\\\",
            '\n' => o += "\\n",
            '\r' => o += "\\r",
            '\t' => o += "\\t",
            '\u{8}' => o += "\\b",
            '\u{c}' => o += "\\f",
            c if (c as u32) < 0x20 => o += &format!("\\u{:04x}", c as u32),
            c => o.push(c),
        }
    }
    o + "\""
}

// FunctionListPanel::serialize, in the key order of nlohmann::json.
#[cfg(test)]
fn to_json(root: &str, found: &[Found]) -> String {
    let list = |v: &[&str]| v.iter().map(|s| json_str(s)).collect::<Vec<_>>().join(",");
    let mut leaves = vec![];
    let mut nodes: Vec<(&str, Vec<&str>)> = vec![];
    for f in found {
        if f.class.is_empty() {
            leaves.push(f.name.as_str());
        } else if let Some(n) = nodes.iter_mut().find(|n| n.0 == f.class) {
            n.1.push(&f.name);
        } else {
            nodes.push((&f.class, vec![&f.name]));
        }
    }
    let mut parts = vec![];
    if !leaves.is_empty() {
        parts.push(format!("\"leaves\":[{}]", list(&leaves)));
    }
    if !nodes.is_empty() {
        let n: Vec<String> = nodes
            .iter()
            .map(|(name, l)| format!("{{\"leaves\":[{}],\"name\":{}}}", list(l), json_str(name)))
            .collect();
        parts.push(format!("\"nodes\":[{}]", n.join(",")));
    }
    parts.push(format!("\"root\":{}", json_str(root)));
    format!("{{{}}}", parts.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parse(lang: &str, src: &[u8]) -> Vec<Found> {
        let doc = Doc::new(src).unwrap();
        parser_for(lang).map_or(vec![], |p| p.parse(&doc))
    }

    // The Notepad++ CI runs on Windows, where git checks the test files out with CR LF line ends.
    fn crlf(b: &[u8]) -> Vec<u8> {
        let mut v = vec![];
        for (i, &c) in b.iter().enumerate() {
            if c == b'\n' && (i == 0 || b[i - 1] != b'\r') {
                v.push(b'\r');
            }
            v.push(c);
        }
        v
    }

    // Runs one PowerEditor/Test/FunctionList folder as unitTest.ps1 does; returns false when it has no unitTest file.
    fn run_case(dir: &Path, lang: &str, failed: &mut Vec<String>) -> bool {
        let Some(file) = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("unitTest"))
            })
        else {
            return false;
        };
        let (_, text, _) = crate::encoding::load(&std::fs::read(&file).unwrap());
        let got = to_json("unitTest", &parse(lang, &crlf(&text))).replace("\\r\\n", "\\n");
        let want = std::fs::read_to_string(dir.join("unitTest.expected.result"))
            .unwrap()
            .replace("\\r\\n", "\\n");
        if got.trim() != want.trim() {
            failed.push(format!(
                "{}\n  want {}\n  got  {}",
                dir.display(),
                want.trim(),
                got
            ));
        }
        true
    }

    #[test]
    fn notepad_plus_plus_unit_tests() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../PowerEditor/Test/FunctionList");
        let mut dirs: Vec<_> = std::fs::read_dir(&base)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        let (mut failed, mut ran) = (vec![], 0);
        for d in dirs {
            let lang = d.file_name().unwrap().to_string_lossy().into_owned();
            if lang.starts_with("udl-") {
                continue;
            }
            let mut subs: Vec<_> = std::fs::read_dir(&d)
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            subs.sort();
            for dir in std::iter::once(d.clone()).chain(subs) {
                ran += run_case(&dir, &lang, &mut failed) as usize;
            }
        }
        assert!(ran >= 40, "{ran}");
        assert!(
            failed.is_empty(),
            "{} of {ran} failed:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }

    #[test]
    fn rule_files() {
        assert_eq!(rule_file("cpp"), "cpp.xml");
        assert_eq!(rule_file("javascript"), "javascript.js.xml");
        assert_eq!(rule_file("javascript.js"), "javascript.js.xml");
        assert!(parser_for("c").is_some());
        assert!(parser_for("normal").is_none());
        assert!(parser_for("json").is_none());
    }

    #[test]
    fn sample_sources() {
        let names = |f: Vec<Found>| -> Vec<(String, String)> {
            f.into_iter().map(|f| (f.class, f.name)).collect()
        };
        let s = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            names(parse("c", b"/* int no(void) {} */\nint add(int a, int b) {\n return a + b;\n}\nstatic void run(void) {}\n")),
            [s("", "add(int a, int b)"), s("", "run(void)")]
        );
        assert_eq!(
            names(parse("cpp", b"class A {\npublic:\n  void f() {}\n};\nint A::g(int x) {\n  return x;\n}\nint main() {\n  return 0;\n}\n")),
            [s("A", "f"), s("A", "g"), s("", "main")]
        );
        assert_eq!(
            names(parse(
                "python",
                b"def top(a):\n    pass\n\nclass K:\n    def m(self):\n        pass\n\n"
            )),
            [s("K", "m(self)"), s("", "top(a)")]
        );
        assert_eq!(
            names(parse(
                "javascript",
                b"function one(a) {\n}\nvar two = function () {\n};\n"
            )),
            [s("", "one"), s("", "two")]
        );
        assert_eq!(
            names(parse(
                "php",
                b"<?php\nfunction top(){\n}\nclass C {\n  public function m() {\n  }\n}\n?>\n"
            )),
            [s("C", "m"), s("", "top()")]
        );
        assert_eq!(
            names(parse("xml", b"<a>\n<b x=\"1\"/>\n<c/>\n</a>\n")),
            [s("", "b x=\"1\"")]
        );
    }

    #[test]
    fn invert_keeps_the_notepad_plus_plus_gaps() {
        assert_eq!(invert(&[], 0, 10), [(0, 10)]);
        assert_eq!(invert(&[(2, 4), (6, 7)], 0, 10), [(0, 1), (8, 10)]);
        assert_eq!(invert(&[(2, 4), (8, 9)], 0, 10), [(0, 1), (5, 7)]);
    }

    #[test]
    fn tree_sort_and_filter() {
        let f = |c: &str, n: &str, p| Found {
            class: c.into(),
            name: n.into(),
            pos: p,
        };
        let mut t = tree(&[
            f("", "zeta", 30),
            f("B", "beta", 20),
            f("", "Alpha", 40),
            f("B", "able", 10),
        ]);
        assert_eq!(t.len(), 3);
        sort(&mut t, false);
        let labels = |t: &[Node]| t.iter().map(|n| n.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&t), ["B", "zeta", "Alpha"]);
        assert_eq!(labels(&t[0].children), ["able", "beta"]);
        sort(&mut t, true);
        assert_eq!(labels(&t), ["Alpha", "B", "zeta"]);
        assert_eq!(labels(&filter(&t, "ET")), ["beta", "zeta"]);
        let merged = tree(&[f("", "Foo", 5), f("Foo", "bar", 9)]);
        assert_eq!(merged.len(), 1);
        assert_eq!(labels(&merged[0].children), ["bar"]);
    }
}
