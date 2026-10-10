// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{self, app_support_dir, Config, Style};
use crate::session::{read_file, write_file};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    El(El),
    Text(String),
    Raw(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Node>,
}

impl El {
    pub fn new(name: &str) -> El {
        El {
            name: name.into(),
            ..El::default()
        }
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.0 == k).map(|a| a.1.as_str())
    }

    pub fn set(&mut self, k: &str, v: &str) {
        match self.attrs.iter_mut().find(|a| a.0 == k) {
            Some(a) => a.1 = v.into(),
            None => self.attrs.push((k.into(), v.into())),
        }
    }

    pub fn els<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> {
        self.kids.iter().filter_map(move |n| match n {
            Node::El(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    pub fn els_mut<'a>(&'a mut self, name: &'a str) -> impl Iterator<Item = &'a mut El> {
        self.kids.iter_mut().filter_map(move |n| match n {
            Node::El(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    pub fn first(&self, name: &str) -> Option<&El> {
        self.kids.iter().find_map(|n| match n {
            Node::El(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    pub fn first_mut(&mut self, name: &str) -> Option<&mut El> {
        self.kids.iter_mut().find_map(|n| match n {
            Node::El(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    pub fn text(&self) -> String {
        self.kids
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn set_text(&mut self, t: &str) {
        self.kids.retain(|n| !matches!(n, Node::Text(_)));
        if !t.is_empty() {
            self.kids.insert(0, Node::Text(t.into()));
        }
    }

    // Adds a child with the same line break and indent as the first child element.
    pub fn push(&mut self, e: El) -> &mut El {
        let is_ws = |n: &Node| matches!(n, Node::Text(t) if t.trim().is_empty());
        let first = self.kids.iter().position(|n| matches!(n, Node::El(_)));
        let sep = first
            .and_then(|i| i.checked_sub(1))
            .map(|i| &self.kids[i])
            .filter(|n| is_ws(n))
            .cloned();
        let mut at = self.kids.len();
        let sep = match (sep, self.kids.last()) {
            (None, Some(Node::Text(t))) if first.is_none() && t.trim().is_empty() => {
                Some(Node::Text(format!("{t}    ")))
            }
            (s, _) => s,
        };
        if self.kids.last().is_some_and(is_ws) && sep.is_some() {
            at -= 1;
        }
        if let Some(s) = sep {
            self.kids.insert(at, s);
            at += 1;
        }
        self.kids.insert(at, Node::El(e));
        match &mut self.kids[at] {
            Node::El(e) => e,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc {
    pub nodes: Vec<Node>,
}

fn element(e: &BytesStart) -> Result<El, String> {
    let mut el = El::new(e.name().as_ref());
    for a in e.attributes() {
        let a = a.map_err(|x| x.to_string())?;
        let v = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|x| x.to_string())?;
        el.attrs.push((
            a.key.as_ref().to_string(),
            v.into_owned(),
        ));
    }
    Ok(el)
}

fn escape(s: &str, attr: bool) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' if attr => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

fn write_node(o: &mut String, n: &Node) {
    match n {
        Node::Raw(s) => o.push_str(s),
        Node::Text(t) => o.push_str(&escape(t, false)),
        Node::El(e) => {
            o.push('<');
            o.push_str(&e.name);
            for (k, v) in &e.attrs {
                o.push_str(&format!(" {k}=\"{}\"", escape(v, true)));
            }
            if e.kids.is_empty() {
                o.push_str(" />");
            } else {
                o.push('>');
                e.kids.iter().for_each(|k| write_node(o, k));
                o.push_str(&format!("</{}>", e.name));
            }
        }
    }
}

impl Doc {
    // Keeps every element, attribute, text, comment and declaration, so a write gives the same document.
    pub fn parse(xml: &str) -> Result<Doc, String> {
        let mut r = Reader::from_str(xml);
        let mut stack: Vec<El> = vec![];
        let mut top: Vec<Node> = vec![];
        fn add(stack: &mut [El], top: &mut Vec<Node>, n: Node) {
            let kids = match stack.last_mut() {
                Some(e) => &mut e.kids,
                None => top,
            };
            match (kids.last_mut(), n) {
                (Some(Node::Text(t)), Node::Text(more)) => t.push_str(&more),
                (_, n) => kids.push(n),
            }
        }
        loop {
            let pos = r.buffer_position();
            let n = match r.read_event().map_err(|e| format!("{e} (at byte {pos})"))? {
                Event::Start(e) => {
                    stack.push(element(&e)?);
                    continue;
                }
                Event::Empty(e) => Node::El(element(&e)?),
                Event::End(_) => Node::El(stack.pop().ok_or("unexpected end tag")?),
                Event::Text(t) => Node::Text(t.to_string()),
                Event::CData(t) => Node::Text(t.to_string()),
                Event::GeneralRef(g) => {
                    let c = match g.resolve_char_ref().map_err(|e| e.to_string())? {
                        Some(c) => c.to_string(),
                        None => quick_xml::escape::resolve_predefined_entity(&g)
                            .ok_or_else(|| format!("unknown entity &{};", &*g))?
                            .to_string(),
                    };
                    Node::Text(c)
                }
                Event::Comment(c) => Node::Raw(format!("<!--{}-->", &*c)),
                Event::Decl(d) => Node::Raw(format!("<?{}?>", &*d)),
                Event::PI(p) => Node::Raw(format!("<?{}?>", &*p)),
                Event::DocType(d) => Node::Raw(format!("<!DOCTYPE {}>", &*d)),
                Event::Eof => break,
            };
            add(&mut stack, &mut top, n);
        }
        if !stack.is_empty() {
            return Err("unexpected end of file".into());
        }
        let d = Doc { nodes: top };
        d.root().ok_or("no root element")?;
        Ok(d)
    }

    pub fn write(&self) -> String {
        let mut o = String::new();
        self.nodes.iter().for_each(|n| write_node(&mut o, n));
        o
    }

    pub fn root(&self) -> Option<&El> {
        self.nodes.iter().find_map(|n| match n {
            Node::El(e) => Some(e),
            _ => None,
        })
    }

    pub fn root_mut(&mut self) -> Option<&mut El> {
        self.nodes.iter_mut().find_map(|n| match n {
            Node::El(e) => Some(e),
            _ => None,
        })
    }

    pub fn lexers(&self) -> Vec<&El> {
        self.root()
            .and_then(|r| r.first("LexerStyles"))
            .map_or(vec![], |l| l.els("LexerType").collect())
    }

    pub fn widgets(&self) -> Vec<&El> {
        self.root()
            .and_then(|r| r.first("GlobalStyles"))
            .map_or(vec![], |g| g.els("WidgetStyle").collect())
    }
}

fn int(e: &El, k: &str) -> i64 {
    e.get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
}

// updateStylesXml key of a WidgetStyle: the styleID from 1 to 256, else the name.
fn widget_key(e: &El) -> String {
    match int(e, "styleID") {
        id @ 1..=256 => id.to_string(),
        _ => e.get("name").unwrap_or("").to_string(),
    }
}

// updateStylesXml mapDotJs: javascript.js styles that take their colours from the embedded javascript styles.
const DOT_JS: &[(&str, &str)] = &[
    ("11", "41"),
    ("4", "45"),
    ("16", "46"),
    ("5", "47"),
    ("19", "47"),
    ("6", "48"),
    ("20", "48"),
    ("7", "49"),
    ("10", "50"),
    ("14", "52"),
    ("1", "42"),
    ("2", "43"),
    ("3", "44"),
    ("15", "44"),
    ("17", "44"),
    ("18", "44"),
    ("19", "44"),
    ("128", "200"),
    ("129", "201"),
    ("130", "202"),
    ("131", "203"),
    ("132", "204"),
    ("133", "205"),
    ("134", "206"),
    ("135", "207"),
];

type Colours = Vec<(String, Option<String>, Option<String>)>;

// The fgColor or bgColor that a style copied from the model gets: the Default Style colour or the embedded javascript colour.
fn theme_colour(e: &mut El, k: &str, default: &str, js: &Colours) {
    if e.get(k).is_none() {
        return;
    }
    let id = e.get("styleID").unwrap_or("");
    let from_js = js
        .iter()
        .rev()
        .find(|(d, f, b)| {
            d == id
                && if k == "fgColor" {
                    f.is_some()
                } else {
                    b.is_some()
                }
        })
        .and_then(|(_, f, b)| if k == "fgColor" { f.clone() } else { b.clone() });
    e.set(k, from_js.as_deref().unwrap_or(default));
}

// Adds the model attributes that `user` lacks; a theme gets the Default Style colours for them.
fn fill_attrs(user: &mut El, model: &El, colours: Option<(&str, &str, &Colours)>) {
    for (k, v) in &model.attrs {
        if user.get(k).is_none() {
            user.set(k, v);
            if let Some((fg, bg, js)) = colours {
                match k.as_str() {
                    "fgColor" => theme_colour(user, k, fg, js),
                    "bgColor" => theme_colour(user, k, bg, js),
                    _ => {}
                }
            }
        }
    }
}

fn recolour(e: &mut El, colours: Option<(&str, &str, &Colours)>) {
    if let Some((fg, bg, js)) = colours {
        theme_colour(e, "fgColor", fg, js);
        theme_colour(e, "bgColor", bg, js);
    }
}

// NppParameters::updateFromModelXml and updateStylesXml: when the model is newer, copy the styles, lexers and attributes that `user` lacks.
pub fn merge_model(user: &mut Doc, model: &Doc, theme: bool) {
    let (Some(u), Some(m)) = (user.root_mut(), model.root()) else {
        return;
    };
    let v_model = int(m, "modelDate");
    if v_model == 0 || int(u, "modelDate") >= v_model {
        return;
    }
    u.set("modelDate", &v_model.to_string());
    let (Some(lm), Some(gm)) = (m.first("LexerStyles"), m.first("GlobalStyles")) else {
        return;
    };
    if u.first("LexerStyles").is_none() {
        return;
    }
    let Some(gu) = u.first_mut("GlobalStyles") else {
        return;
    };
    let def = gu
        .els("WidgetStyle")
        .filter(|w| widget_key(w) == "32")
        .last();
    let (fg, bg) = def.map_or((String::new(), String::new()), |d| {
        (
            d.get("fgColor").unwrap_or("").to_string(),
            d.get("bgColor").unwrap_or("").to_string(),
        )
    });
    let none = vec![];
    let colours = theme.then_some((fg.as_str(), bg.as_str(), &none));
    for wm in gm.els("WidgetStyle") {
        let key = widget_key(wm);
        if key.is_empty() {
            continue;
        }
        match gu
            .els_mut("WidgetStyle")
            .filter(|w| widget_key(w) == key)
            .last()
        {
            Some(w) => fill_attrs(w, wm, colours),
            None => recolour(gu.push(wm.clone()), colours),
        }
    }
    let Some(lu) = u.first_mut("LexerStyles") else {
        return;
    };
    for lex in lm.els("LexerType") {
        let Some(name) = lex.get("name") else {
            continue;
        };
        let mut js: Colours = vec![];
        if name == "javascript.js" {
            if let Some(emb) = lu
                .els("LexerType")
                .filter(|l| l.get("name") == Some("javascript"))
                .last()
            {
                for ws in emb.els("WordsStyle") {
                    let Some(id) = ws.get("styleID") else {
                        continue;
                    };
                    for (dest, _) in DOT_JS.iter().filter(|(_, s)| *s == id) {
                        js.push((
                            dest.to_string(),
                            ws.get("fgColor").map(String::from),
                            ws.get("bgColor").map(String::from),
                        ));
                    }
                }
            }
        }
        let colours = theme.then_some((fg.as_str(), bg.as_str(), &js));
        let Some(ul) = lu
            .els_mut("LexerType")
            .filter(|l| l.get("name") == Some(name))
            .last()
        else {
            let c = lu.push(lex.clone());
            c.els_mut("WordsStyle").for_each(|w| recolour(w, colours));
            continue;
        };
        for wm in lex.els("WordsStyle") {
            let Some(id) = wm.get("styleID").filter(|i| !i.is_empty()) else {
                continue;
            };
            match ul
                .els_mut("WordsStyle")
                .filter(|w| w.get("styleID") == Some(id))
                .last()
            {
                Some(w) => fill_attrs(w, wm, colours),
                None => recolour(ul.push(wm.clone()), colours),
            }
        }
    }
}

// GUIConfig name="globalOverride" attributes, in the order of the Global override check boxes.
pub const OVERRIDE_KEYS: [&str; 7] = [
    "fg",
    "bg",
    "font",
    "fontSize",
    "bold",
    "italic",
    "underline",
];
pub type Override = [bool; 7];
const STYLE_DEFAULT: usize = 32;

// ScintillaEditView::setStyle: the "Global override" values replace the values of a style for each check box that is on.
pub fn override_style(s: &mut Style, g: &Style, go: &Override) {
    if go[0] {
        if g.fg.is_some() {
            s.fg = g.fg;
        } else if s.id != STYLE_DEFAULT {
            s.fg = None;
        }
    }
    if go[1] {
        if g.bg.is_some() {
            s.bg = g.bg;
        } else if s.id != STYLE_DEFAULT {
            s.bg = None;
        }
    }
    if go[2] && !g.font_name.is_empty() {
        s.font_name = g.font_name.clone();
    }
    if go[3] && g.font_size.is_some_and(|z| z > 0) {
        s.font_size = g.font_size;
    }
    if let Some(gf) = g.font_style {
        for (i, bit) in [(4, 1), (5, 2), (6, 4)] {
            if go[i] {
                let f = s.font_style.unwrap_or(0);
                s.font_style = Some(if gf & bit != 0 { f | bit } else { f & !bit });
            }
        }
    }
}

// The styles that go through setStyle: Default Style, brace highlight, bad brace, indent guideline and all lexer styles.
pub fn apply_override(c: &mut Config, go: &Override) {
    if !go.contains(&true) {
        return;
    }
    let Some(g) = c
        .global_styles
        .iter()
        .find(|s| s.name == "Global override")
        .cloned()
    else {
        return;
    };
    c.global_styles
        .iter_mut()
        .filter(|s| [STYLE_DEFAULT, 34, 35, 37].contains(&s.id))
        .chain(c.lexer_styles.iter_mut().flat_map(|(_, v)| v.iter_mut()))
        .for_each(|s| override_style(s, &g, go));
}

// User ext.: getLangFromExt checks it before the language extensions.
pub fn apply_user_exts(c: &mut Config, doc: &Doc) {
    let mut lexers = doc.lexers();
    lexers.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    for lex in lexers.iter().rev() {
        let name = lex.get("name").unwrap_or("");
        for ext in lex.get("ext").unwrap_or("").split_whitespace() {
            let ext = ext.to_lowercase();
            if !c.languages.iter().any(|l| l.name == name) {
                break;
            }
            for l in c.languages.iter_mut() {
                l.exts.retain(|e| *e != ext);
                if l.name == name {
                    l.exts.push(ext.clone());
                }
            }
        }
    }
}

// SortLexersInAlphabeticalOrder: by description, with Search result last.
fn sort_key(l: &El) -> (bool, String) {
    let d = l.get("desc").unwrap_or("");
    (d == "Search result", d.to_string())
}

pub fn sorted_lexers(doc: &Doc) -> Vec<usize> {
    let lexers = doc.lexers();
    let mut v: Vec<usize> = (0..lexers.len()).collect();
    v.sort_by(|&a, &b| sort_key(lexers[a]).cmp(&sort_key(lexers[b])));
    v
}

fn base() -> &'static Config {
    static B: OnceLock<Config> = OnceLock::new();
    B.get_or_init(config::load)
}

pub fn base_language(name: &str) -> Option<&'static crate::config::Language> {
    base().languages.iter().find(|l| l.name == name)
}

pub fn model() -> Doc {
    Doc::parse(config::STYLERS).expect("stylers.model.xml")
}

// The editor configuration: the languages of langs.model.xml with the styles of `doc`.
pub fn build(doc: &Doc, go: &Override) -> Config {
    let mut c = Config {
        languages: base().languages.clone(),
        ..Config::default()
    };
    config::load_styles(&mut c, &doc.write());
    apply_user_exts(&mut c, doc);
    apply_override(&mut c, go);
    c
}

macro_rules! themes {
    ($($n:literal),* $(,)?) => {
        &[$(($n, include_str!(concat!("../../PowerEditor/installer/themes/", $n, ".xml")))),*]
    };
}

// PowerEditor/installer/themes; the test builtin_themes_match_folder keeps this list complete.
pub const BUILTIN: &[(&str, &str)] = themes!(
    "Bespin",
    "Black board",
    "Choco",
    "DansLeRuSH-Dark",
    "DarkModeDefault",
    "Deep Black",
    "Hello Kitty",
    "HotFudgeSundae",
    "khaki",
    "Mono Industrial",
    "Monokai",
    "MossyLawn",
    "Navajo",
    "Obsidian",
    "Plastic Code Wrap",
    "Ruby Blue",
    "Solarized-light",
    "Solarized",
    "Twilight",
    "Vibrant Ink",
    "vim Dark Blue",
    "Zenburn",
);

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Src {
    #[default]
    Stylers,
    File(PathBuf),
    Builtin(&'static str),
}

pub const DEFAULT_THEME: &str = "Default (stylers.xml)";

impl Src {
    pub fn file_name(&self) -> String {
        match self {
            Src::Stylers => "stylers.xml".into(),
            Src::File(p) => p.file_name().unwrap_or_default().to_string_lossy().into(),
            Src::Builtin(n) => format!("{n}.xml"),
        }
    }

    // ThemeSwitcher::getSavePathFrom: an installed theme saves to the user themes folder.
    pub fn save_path(&self, dir: &Path) -> PathBuf {
        match self {
            Src::Stylers => dir.join("stylers.xml"),
            Src::File(p) => p.clone(),
            Src::Builtin(n) => dir.join("themes").join(format!("{n}.xml")),
        }
    }
}

fn stem(p: &Path) -> String {
    p.file_stem().unwrap_or_default().to_string_lossy().into()
}

// Notepad_plus_Window::init theme list: the default, the user themes, then the installed themes that the user has not replaced.
pub fn theme_list(user: &[PathBuf]) -> Vec<(String, Src)> {
    let mut v = vec![(DEFAULT_THEME.to_string(), Src::Stylers)];
    v.extend(user.iter().map(|p| (stem(p), Src::File(p.clone()))));
    for (n, _) in BUILTIN {
        if !v.iter().any(|(m, _)| m == n) {
            v.push((n.to_string(), Src::Builtin(n)));
        }
    }
    v
}

pub fn user_themes(dir: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = dir
        .and_then(|d| std::fs::read_dir(d.join("themes")).ok())
        .map_or(vec![], |r| {
            r.filter_map(|e| Some(e.ok()?.path()))
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")))
                .collect()
        });
    v.sort();
    v
}

// The DarkMode GUIConfig theme file names: darkThemeName defaults to DarkModeDefault.xml, lightThemeName to stylers.xml.
pub fn theme_name(dark: bool, gui: Option<&El>) -> String {
    let get = |k| gui.and_then(|g| g.get(k)).unwrap_or("").to_string();
    match (dark, get("darkThemeName"), get("lightThemeName")) {
        (true, d, _) if d.is_empty() => "DarkModeDefault.xml".into(),
        (true, d, _) => d,
        (false, _, l) => l,
    }
}

// Parameters.cpp: the theme file of the user themes folder, else the installed one, else stylers.xml.
pub fn resolve(name: &str, user: &[PathBuf]) -> Src {
    if name.is_empty() || name == "stylers.xml" {
        return Src::Stylers;
    }
    if let Some(p) = user
        .iter()
        .find(|p| p.file_name().is_some_and(|f| f == name))
    {
        return Src::File(p.clone());
    }
    BUILTIN
        .iter()
        .find(|(n, _)| format!("{n}.xml") == name)
        .map_or(Src::Stylers, |(n, _)| Src::Builtin(n))
}

// WordStyleDlg Save & Close: NppDarkMode::setThemeName stores "" for stylers.xml in light mode.
pub fn theme_attr(dark: bool, src: &Src) -> (&'static str, String) {
    let f = src.file_name();
    if dark {
        ("darkThemeName", f)
    } else {
        (
            "lightThemeName",
            if f == "stylers.xml" { String::new() } else { f },
        )
    }
}

pub fn gui_config<'a>(conf: &'a Doc, name: &str) -> Option<&'a El> {
    conf.root()?
        .first("GUIConfigs")?
        .els("GUIConfig")
        .find(|g| g.get("name") == Some(name))
}

pub fn read_override(conf: Option<&Doc>) -> Override {
    let g = conf.and_then(|c| gui_config(c, "globalOverride"));
    OVERRIDE_KEYS.map(|k| g.and_then(|g| g.get(k)) == Some("yes"))
}

pub fn override_attrs(go: &Override) -> Vec<(&'static str, String)> {
    OVERRIDE_KEYS
        .iter()
        .zip(go)
        .map(|(k, on)| (*k, if *on { "yes" } else { "no" }.to_string()))
        .collect()
}

// config.xml with new values for some attributes of one GUIConfig; the other content stays as it is.
pub fn set_gui_config(
    existing: Option<&str>,
    name: &str,
    attrs: &[(&str, String)],
) -> Result<String, String> {
    let el = crate::prefs::Elem {
        name: name.into(),
        attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        text: None,
    };
    crate::prefs::patch(existing, &crate::prefs::GUI_PATH, "GUIConfig", &[el])
}

pub fn read_doc(path: &Path) -> Result<Option<Doc>, String> {
    match read_file(path)? {
        Some(t) => Doc::parse(&t)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        None => Ok(None),
    }
}

// Never replaces a file that exists but does not load as XML.
pub fn save_doc(path: &Path, doc: &Doc) -> Result<(), String> {
    read_doc(path)?;
    write_file(path, &doc.write(), false)
}

pub fn update_config(attrs: &[(&str, &[(&str, String)])]) -> Result<(), String> {
    let path = app_support_dir()
        .ok_or("no HOME folder")?
        .join("config.xml");
    let mut text = read_file(&path)?;
    for (name, a) in attrs {
        text = Some(set_gui_config(text.as_deref(), name, a)?);
    }
    write_file(&path, text.as_deref().unwrap_or(""), false)
}

// The styles document of a theme, merged with the model; Err when the file does not load.
pub fn load(src: &Src, dir: Option<&Path>) -> Result<Doc, String> {
    let model = model();
    let mut doc = match src {
        Src::Stylers => match dir {
            Some(d) => read_doc(&d.join("stylers.xml"))?.unwrap_or_else(|| model.clone()),
            None => model.clone(),
        },
        Src::File(p) => read_doc(p)?.ok_or_else(|| format!("{}: file not found", p.display()))?,
        Src::Builtin(n) => {
            let t = BUILTIN.iter().find(|(m, _)| m == n).map_or("", |t| t.1);
            Doc::parse(t)?
        }
    };
    merge_model(&mut doc, &model, *src != Src::Stylers);
    Ok(doc)
}

pub struct Current {
    pub cfg: &'static Config,
    pub src: Src,
    pub doc: Doc,
    pub go: Override,
    pub error: Option<String>,
}

pub static CURRENT: Mutex<Option<Current>> = Mutex::new(None);

fn system_dark() -> bool {
    use objc2_app_kit::{NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication};
    use objc2_foundation::NSArray;
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return false;
    };
    let names = unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
    let best = NSApplication::sharedApplication(mtm)
        .effectiveAppearance()
        .bestMatchFromAppearancesWithNames(&names);
    best.is_some_and(|b| &*b == unsafe { NSAppearanceNameDarkAqua })
}

pub fn dark() -> bool {
    !cfg!(test) && system_dark()
}

// Startup: the theme of config.xml for the system appearance, with the Global override check boxes.
fn startup() -> Current {
    let dir = app_support_dir().filter(|_| !cfg!(test));
    let conf = dir
        .as_ref()
        .and_then(|d| read_doc(&d.join("config.xml")).ok().flatten());
    let go = read_override(conf.as_ref());
    let name = theme_name(
        dark(),
        conf.as_ref().and_then(|c| gui_config(c, "DarkMode")),
    );
    let src = resolve(&name, &user_themes(dir.as_deref()));
    let (doc, error) = match load(&src, dir.as_deref()) {
        Ok(d) => (d, None),
        Err(e) => (model(), Some(e)),
    };
    Current {
        cfg: Box::leak(Box::new(build(&doc, &go))),
        src,
        doc,
        go,
        error,
    }
}

pub fn with_current<R>(f: impl FnOnce(&mut Current) -> R) -> R {
    let mut g = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(startup))
}

pub fn cfg() -> &'static Config {
    with_current(|c| c.cfg)
}

// ponytail: each saved change leaks one Config because cfg() gives out &'static; an Arc would need changes in all callers.
pub fn set_current(src: Src, doc: Doc, go: Override, c: Config) {
    with_current(|cur| {
        *cur = Current {
            cfg: Box::leak(Box::new(c)),
            src,
            doc,
            go,
            error: None,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(x: &str) -> Doc {
        Doc::parse(x).unwrap()
    }

    #[test]
    fn round_trip_keeps_everything() {
        let x = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<!-- c -->\r\n<NotepadPlus modelDate=\"1\" extra=\"a &amp; b\">\r\n    <LexerStyles>\r\n        <LexerType name=\"x\" desc=\"X\" ext=\"\" excluded=\"no\">\r\n            <WordsStyle name=\"K\" styleID=\"5\" fgColor=\"FF0000\" myAttr=\"&quot;q&quot;\" keywordClass=\"instre1\">a &lt;b&gt; c</WordsStyle>\r\n        </LexerType>\r\n    </LexerStyles>\r\n    <Unknown><Child/></Unknown>\r\n</NotepadPlus>\r\n";
        let d = doc(x);
        let out = d.write();
        assert_eq!(doc(&out), d);
        assert_eq!(doc(&out).write(), out);
        assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<!-- c -->\r\n"));
        assert!(out.contains("myAttr=\"&quot;q&quot;\""));
        assert!(out.contains("excluded=\"no\""));
        assert!(out.contains(">a &lt;b&gt; c</WordsStyle>"));
        assert!(out.contains("<Unknown><Child /></Unknown>"));
        let ws = d.lexers()[0].first("WordsStyle").unwrap();
        assert_eq!(ws.text(), "a <b> c");
        assert_eq!(d.root().unwrap().get("extra"), Some("a & b"));
        assert!(Doc::parse("<a><b></a>").is_err());
        assert!(Doc::parse("<a>").is_err());
        assert!(Doc::parse("").is_err());
    }

    #[test]
    fn model_round_trip() {
        let m = model();
        let out = m.write();
        assert_eq!(doc(&out), m);
        assert_eq!(out.lines().count(), config::STYLERS.lines().count());
        let a = Config {
            languages: vec![],
            ..build(&m, &[false; 7])
        };
        let b = config::load();
        assert_eq!(
            format!("{:?}", a.lexer_styles),
            format!("{:?}", b.lexer_styles)
        );
        assert_eq!(
            format!("{:?}", a.global_styles),
            format!("{:?}", b.global_styles)
        );
    }

    #[test]
    fn push_keeps_indent() {
        let mut d = doc("<a>\r\n    <b />\r\n</a>");
        d.root_mut().unwrap().push(El::new("c"));
        assert_eq!(d.write(), "<a>\r\n    <b />\r\n    <c />\r\n</a>");
        let mut e = doc("<a></a>");
        e.root_mut().unwrap().push(El::new("c"));
        assert_eq!(e.write(), "<a><c /></a>");
    }

    #[test]
    fn builtin_themes_match_folder() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../PowerEditor/installer/themes"
        );
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| stem(&e.unwrap().path()))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        let mut ours: Vec<String> = BUILTIN.iter().map(|t| t.0.to_string()).collect();
        ours.sort_by_key(|n| n.to_lowercase());
        assert_eq!(ours, names);
        for (n, t) in BUILTIN {
            assert!(Doc::parse(t).is_ok(), "{n}");
        }
    }

    const MODEL: &str = r#"<NotepadPlus modelDate="20">
    <LexerStyles>
        <LexerType name="cpp" desc="C++" ext="">
            <WordsStyle name="DEFAULT" styleID="11" fgColor="000000" bgColor="FFFFFF" fontStyle="0" />
            <WordsStyle name="NEW" styleID="99" fgColor="123456" bgColor="654321" fontStyle="1" />
        </LexerType>
        <LexerType name="javascript.js" desc="JavaScript" ext="">
            <WordsStyle name="DEFAULT" styleID="11" fgColor="000000" bgColor="FFFFFF" />
            <WordsStyle name="NUMBER" styleID="4" fgColor="FF0000" bgColor="FFFFFF" />
        </LexerType>
        <LexerType name="rust" desc="Rust" ext="">
            <WordsStyle name="DEFAULT" styleID="0" fgColor="111111" bgColor="222222" />
        </LexerType>
    </LexerStyles>
    <GlobalStyles>
        <WidgetStyle name="Default Style" styleID="32" fgColor="000000" bgColor="FFFFFF" fontName="Courier New" fontSize="10" />
        <WidgetStyle name="Caret colour" styleID="2069" fgColor="8000FF" />
        <WidgetStyle name="Brand new" styleID="0" bgColor="ABCDEF" />
    </GlobalStyles>
</NotepadPlus>"#;

    const THEME: &str = r#"<NotepadPlus modelDate="10">
    <LexerStyles>
        <LexerType name="cpp" desc="C++" ext="" custom="kept">
            <WordsStyle name="DEFAULT" styleID="11" fgColor="EEEEEE" />
        </LexerType>
        <LexerType name="javascript" desc="JavaScript (embedded)" ext="">
            <WordsStyle name="NUMBER" styleID="45" fgColor="0000AA" />
        </LexerType>
        <LexerType name="javascript.js" desc="JavaScript" ext="">
            <WordsStyle name="DEFAULT" styleID="11" fgColor="EEEEEE" bgColor="111111" />
        </LexerType>
    </LexerStyles>
    <GlobalStyles>
        <WidgetStyle name="Default Style" styleID="32" fgColor="F0F0F0" bgColor="101010" />
        <WidgetStyle name="Caret colour" styleID="2069" fgColor="FFFFFF" />
    </GlobalStyles>
</NotepadPlus>"#;

    fn lexer<'a>(d: &'a Doc, n: &str) -> &'a El {
        d.lexers()
            .into_iter()
            .find(|l| l.get("name") == Some(n))
            .unwrap()
    }

    fn style<'a>(l: &'a El, id: &str) -> &'a El {
        l.els("WordsStyle")
            .find(|w| w.get("styleID") == Some(id))
            .unwrap()
    }

    #[test]
    fn theme_merge_uses_default_colours() {
        let mut t = doc(THEME);
        merge_model(&mut t, &doc(MODEL), true);
        assert_eq!(t.root().unwrap().get("modelDate"), Some("20"));
        let cpp = lexer(&t, "cpp");
        assert_eq!(cpp.get("custom"), Some("kept"));
        let def = style(cpp, "11");
        assert_eq!(
            (def.get("fgColor"), def.get("bgColor")),
            (Some("EEEEEE"), Some("101010"))
        );
        assert_eq!(def.get("fontStyle"), Some("0"));
        let new = style(cpp, "99");
        assert_eq!(
            (new.get("fgColor"), new.get("bgColor")),
            (Some("F0F0F0"), Some("101010"))
        );
        assert_eq!(new.get("fontStyle"), Some("1"));
        let num = style(lexer(&t, "javascript.js"), "4");
        assert_eq!(
            (num.get("fgColor"), num.get("bgColor")),
            (Some("0000AA"), Some("101010"))
        );
        let rust = style(lexer(&t, "rust"), "0");
        assert_eq!(
            (rust.get("fgColor"), rust.get("bgColor")),
            (Some("F0F0F0"), Some("101010"))
        );
        let w = t.widgets();
        let brand = w
            .iter()
            .find(|w| w.get("name") == Some("Brand new"))
            .unwrap();
        assert_eq!(brand.get("bgColor"), Some("101010"));
        let caret = w
            .iter()
            .find(|w| w.get("name") == Some("Caret colour"))
            .unwrap();
        assert_eq!(caret.get("fgColor"), Some("FFFFFF"));
        let d = w.iter().find(|w| w.get("styleID") == Some("32")).unwrap();
        assert_eq!(d.get("fontName"), Some("Courier New"));
        assert_eq!(d.get("fgColor"), Some("F0F0F0"));
        let again = t.clone();
        merge_model(&mut t, &doc(MODEL), true);
        assert_eq!(t, again);
    }

    #[test]
    fn stylers_merge_uses_model_colours() {
        let mut t = doc(THEME);
        merge_model(&mut t, &doc(MODEL), false);
        let new = style(lexer(&t, "cpp"), "99");
        assert_eq!(
            (new.get("fgColor"), new.get("bgColor")),
            (Some("123456"), Some("654321"))
        );
        let def = style(lexer(&t, "cpp"), "11");
        assert_eq!(
            (def.get("fgColor"), def.get("bgColor")),
            (Some("EEEEEE"), Some("FFFFFF"))
        );
        let mut same = doc(MODEL);
        merge_model(&mut same, &doc(MODEL), false);
        assert_eq!(same, doc(MODEL));
    }

    #[test]
    fn builtin_themes_get_new_model_styles() {
        let m = model();
        let mut t = doc(BUILTIN.iter().find(|t| t.0 == "Monokai").unwrap().1);
        let before = t.lexers().len();
        merge_model(&mut t, &m, true);
        assert!(t.lexers().len() >= before);
        for l in m.lexers() {
            assert!(t.lexers().iter().any(|u| u.get("name") == l.get("name")));
        }
    }

    fn st(id: usize, fg: Option<isize>, font_style: Option<u32>) -> Style {
        Style {
            id,
            fg,
            bg: Some(0xFFFFFF),
            font_name: "Courier New".into(),
            font_style,
            font_size: Some(10),
            ..Style::default()
        }
    }

    #[test]
    fn global_override_rules() {
        let g = Style {
            name: "Global override".into(),
            fg: Some(0x0000FF),
            bg: None,
            font_name: "Menlo".into(),
            font_style: Some(1 | 4),
            font_size: Some(0),
            ..Style::default()
        };
        let mut s = st(5, Some(0x00FF00), None);
        override_style(&mut s, &g, &[false; 7]);
        assert_eq!(s.fg, Some(0x00FF00));
        override_style(&mut s, &g, &[true, true, true, true, true, false, false]);
        assert_eq!(s.fg, Some(0x0000FF));
        assert_eq!(s.bg, None);
        assert_eq!(s.font_name, "Menlo");
        assert_eq!(s.font_size, Some(10));
        assert_eq!(s.font_style, Some(1));
        let mut s = st(5, None, Some(2 | 4));
        override_style(&mut s, &g, &[false, false, false, false, false, true, true]);
        assert_eq!(s.font_style, Some(4));
        let mut d = st(STYLE_DEFAULT, Some(1), None);
        override_style(&mut d, &g, &[true, true, false, false, false, false, false]);
        assert_eq!((d.fg, d.bg), (Some(0x0000FF), Some(0xFFFFFF)));
        let mut c = Config {
            global_styles: vec![
                g.clone(),
                st(32, Some(1), None),
                st(33, Some(1), None),
                st(34, Some(1), None),
            ],
            lexer_styles: vec![("cpp".into(), vec![st(5, Some(1), None)])],
            ..Config::default()
        };
        apply_override(&mut c, &[true, false, false, false, false, false, false]);
        let fg: Vec<_> = c.global_styles.iter().map(|s| s.fg).collect();
        assert_eq!(
            fg,
            [Some(0x0000FF), Some(0x0000FF), Some(1), Some(0x0000FF)]
        );
        assert_eq!(c.lexer_styles[0].1[0].fg, Some(0x0000FF));
    }

    #[test]
    fn user_ext_and_keywords_once() {
        let mut d = model();
        {
            let lu = d.root_mut().unwrap().first_mut("LexerStyles").unwrap();
            let py = lu
                .els_mut("LexerType")
                .find(|l| l.get("name") == Some("python"))
                .unwrap();
            py.set("ext", "foo CPP");
            let kw = py
                .els_mut("WordsStyle")
                .find(|w| w.get("keywordClass") == Some("instre1"))
                .unwrap();
            kw.set_text("myword other");
        }
        let c = build(&d, &[false; 7]);
        let lang_of =
            |p: &str| crate::lang::language_for_path(&c, Path::new(p)).map(|l| l.name.clone());
        assert_eq!(lang_of("a.foo").as_deref(), Some("python"));
        assert_eq!(lang_of("a.cpp").as_deref(), Some("python"));
        assert_eq!(lang_of("a.hpp").as_deref(), Some("cpp"));
        let s = crate::lang::setup(&c, "python");
        let kw0: Vec<&str> = s.keywords.iter().filter(|k| k.0 == 0).flat_map(|k| k.1.split_whitespace()).collect();
        assert_eq!(kw0.iter().filter(|w| **w == "myword").count(), 1);
        assert!(kw0.contains(&"lambda"));
    }

    #[test]
    fn theme_names() {
        let g = |x: &str| doc(x).root().cloned().unwrap();
        assert_eq!(theme_name(true, None), "DarkModeDefault.xml");
        assert_eq!(theme_name(false, None), "");
        let set = g(
            r#"<GUIConfig name="DarkMode" darkThemeName="Zenburn.xml" lightThemeName="Monokai.xml" />"#,
        );
        assert_eq!(theme_name(true, Some(&set)), "Zenburn.xml");
        assert_eq!(theme_name(false, Some(&set)), "Monokai.xml");
        let user = vec![
            PathBuf::from("/u/themes/Monokai.xml"),
            PathBuf::from("/u/themes/Mine.xml"),
        ];
        assert_eq!(resolve("", &user), Src::Stylers);
        assert_eq!(resolve("stylers.xml", &user), Src::Stylers);
        assert_eq!(resolve("Monokai.xml", &user), Src::File(user[0].clone()));
        assert_eq!(resolve("Zenburn.xml", &user), Src::Builtin("Zenburn"));
        assert_eq!(resolve("Gone.xml", &user), Src::Stylers);
        let list = theme_list(&user);
        assert_eq!(list[0], (DEFAULT_THEME.into(), Src::Stylers));
        assert_eq!(list[1].0, "Monokai");
        assert_eq!(list[2].0, "Mine");
        assert_eq!(list.iter().filter(|t| t.0 == "Monokai").count(), 1);
        assert_eq!(list.len(), 2 + BUILTIN.len());
        assert_eq!(
            theme_attr(false, &Src::Stylers),
            ("lightThemeName", String::new())
        );
        assert_eq!(
            theme_attr(true, &Src::Stylers),
            ("darkThemeName", "stylers.xml".into())
        );
        assert_eq!(
            theme_attr(false, &Src::Builtin("Zenburn")),
            ("lightThemeName", "Zenburn.xml".into())
        );
        let dir = Path::new("/d");
        assert_eq!(
            Src::Builtin("Zenburn").save_path(dir),
            dir.join("themes/Zenburn.xml")
        );
        assert_eq!(Src::Stylers.save_path(dir), dir.join("stylers.xml"));
    }

    #[test]
    fn gui_config_write() {
        let x = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    <GUIConfigs>\r\n        <GUIConfig name=\"TabBar\" dragAndDrop=\"yes\" />\r\n        <GUIConfig name=\"DarkMode\" enable=\"no\" darkThemeName=\"DarkModeDefault.xml\" />\r\n    </GUIConfigs>\r\n    <History nbMaxFile=\"10\" />\r\n</NotepadPlus>\r\n";
        let out = set_gui_config(
            Some(x),
            "DarkMode",
            &[("lightThemeName", "Zenburn.xml".into())],
        )
        .unwrap();
        assert!(out.contains("<GUIConfig name=\"DarkMode\" enable=\"no\" darkThemeName=\"DarkModeDefault.xml\" lightThemeName=\"Zenburn.xml\" />"));
        assert!(out.contains("<GUIConfig name=\"TabBar\" dragAndDrop=\"yes\" />"));
        assert!(out.contains("<History nbMaxFile=\"10\" />"));
        let go = [true, false, false, false, false, false, true];
        let out = set_gui_config(Some(&out), "globalOverride", &override_attrs(&go)).unwrap();
        assert!(out.contains("\r\n        <GUIConfig name=\"globalOverride\" fg=\"yes\" bg=\"no\" font=\"no\" fontSize=\"no\" bold=\"no\" italic=\"no\" underline=\"yes\" />\r\n    </GUIConfigs>"));
        assert_eq!(read_override(Some(&doc(&out))), go);
        assert_eq!(read_override(None), [false; 7]);
        let new =
            set_gui_config(None, "DarkMode", &[("darkThemeName", "Zenburn.xml".into())]).unwrap();
        assert_eq!(new, "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n    <GUIConfigs>\r\n        <GUIConfig name=\"DarkMode\" darkThemeName=\"Zenburn.xml\" />\r\n    </GUIConfigs>\r\n</NotepadPlus>\r\n");
        let d = doc(&new);
        assert_eq!(
            gui_config(&d, "DarkMode").unwrap().get("darkThemeName"),
            Some("Zenburn.xml")
        );
        assert!(set_gui_config(Some("<NotepadPlus><broken></NotepadPlus>"), "x", &[]).is_err());
    }

    #[test]
    fn save_refuses_unreadable_file() {
        let dir = std::env::temp_dir().join(format!("npp-styler-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("stylers.xml");
        std::fs::write(&p, "<NotepadPlus><oops></NotepadPlus>").unwrap();
        assert!(save_doc(&p, &doc(MODEL)).is_err());
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "<NotepadPlus><oops></NotepadPlus>"
        );
        std::fs::remove_file(&p).unwrap();
        save_doc(&p, &doc(MODEL)).unwrap();
        assert_eq!(read_doc(&p).unwrap(), Some(doc(MODEL)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
