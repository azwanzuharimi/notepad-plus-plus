// SPDX-License-Identifier: GPL-3.0-or-later
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use std::io::Write;

pub const SCI_REPLACESEL: i32 = 2170;
pub const SCI_NEWLINE: i32 = 2329;
pub const TYPE_L: u8 = 0;
pub const TYPE_S: u8 = 1;
pub const TYPE_MENU: u8 = 2;
pub const TYPE_SNR: u8 = 3;

// Scintilla messages whose lParam is a string, from the recordedMacroStep constructor.
const STRING_MSGS: [i32; 16] = [
    2181, 2170, 2194, 2195, 2197, 2001, 2002, 2003, 2282, 2077, 2443, 2073, 2276, 2056, 2367, 2368,
];
// The two lists of recordedMacroStep::isMacroable: messages with a string, then messages with a number.
const MACROABLE_S: [i32; 6] = [2170, 2001, 2003, 2282, 2367, 2368];
const MACROABLE_L: [i32; 104] = [
    2024, 2025, 2422, 2177, 2178, 2179, 2180, 2004, 2013, 2366, 2300, 2301, 2413, 2414, 2302, 2303,
    2415, 2416, 2304, 2305, 2306, 2307, 2308, 2309, 2310, 2311, 2390, 2391, 2392, 2393, 2439, 2440,
    2441, 2442, 2312, 2313, 2314, 2315, 2349, 2450, 2451, 2452, 2316, 2317, 2318, 2319, 2435, 2436,
    2437, 2438, 2320, 2321, 2322, 2323, 2324, 2325, 2326, 2327, 2328, 2330, 2331, 2332, 2453, 2454,
    2652, 2653, 2335, 2336, 2518, 2395, 2396, 2455, 2337, 2338, 2339, 2404, 2340, 2341, 2342, 2343,
    2344, 2345, 2346, 2347, 2348, 2426, 2427, 2428, 2429, 2430, 2431, 2432, 2433, 2434, 2469, 2519,
    2619, 2620, 2621, 2628, 2629, 2596, 2470, 2329,
];

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Step {
    pub kind: u8,
    pub message: i32,
    pub w: usize,
    pub l: isize,
    pub s: String,
}

impl Step {
    pub fn new(kind: u8, message: i32, w: usize, l: isize, s: &str) -> Step {
        Step {
            kind,
            message,
            w,
            l,
            s: s.into(),
        }
    }

    pub fn menu(id: i32) -> Step {
        Step::new(TYPE_MENU, 0, id as usize, 0, "")
    }

    pub fn is_macroable(&self) -> bool {
        match self.kind {
            TYPE_S => MACROABLE_S.contains(&self.message),
            TYPE_L => MACROABLE_L.contains(&self.message),
            _ => false,
        }
    }
}

pub fn is_string_message(msg: i32) -> bool {
    STRING_MSGS.contains(&msg)
}

// Port of the recordedMacroStep constructor and the SCN_MACRORECORD EOL rule of NppNotification.cpp.
pub fn record(m: &mut Vec<Step>, msg: i32, w: usize, l: isize, s: Option<&str>, crlf: bool) {
    let step = match s.filter(|_| l != 0 && is_string_message(msg)) {
        Some(s) => Step::new(TYPE_S, msg, w, 0, s),
        None => Step::new(TYPE_L, msg, w, l, ""),
    };
    if msg == SCI_REPLACESEL && matches!(step.s.as_str(), "\n" | "\r") {
        if crlf && step.s == "\n" && m.last().is_some_and(|p| p.message == SCI_NEWLINE) {
            m.pop();
        }
        m.push(Step::new(TYPE_L, SCI_NEWLINE, 0, 0, ""));
        return;
    }
    m.push(step);
}

// Notepad++ semantics: ctrl is Command and alt is Option on macOS; meta is the macOS Control key (MacControl="yes").
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Key {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: u8,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Macro {
    pub name: String,
    pub key: Key,
    pub folder: String,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Command {
    pub name: String,
    pub key: Key,
    pub folder: String,
    pub cmd: String,
}

// A <Shortcut> of <InternalCommands>: a new key for the menu command `id`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Internal {
    pub id: i32,
    pub nth: i32,
    pub key: Key,
}

// A <ScintKey> of <ScintillaKeys>: all the keys of the Scintilla command `id`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ScintKey {
    pub id: i32,
    pub menu_id: i32,
    pub keys: Vec<Key>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Shortcuts {
    pub macros: Vec<Macro>,
    pub commands: Vec<Command>,
    pub internal: Vec<Internal>,
    pub scint: Vec<ScintKey>,
}

// The Notepad++ default macro, and macOS forms of the Notepad++ default user commands; the Windows ones do not run on macOS.
pub fn defaults() -> Shortcuts {
    let cmd = |name: &str, key: u8, cmd: &str| Command {
        name: name.into(),
        key: Key {
            alt: true,
            key,
            ..Key::default()
        },
        folder: String::new(),
        cmd: cmd.into(),
    };
    Shortcuts {
        macros: vec![Macro {
            name: "Trim Trailing Space and Save".into(),
            key: Key {
                alt: true,
                shift: true,
                key: 83,
                ..Key::default()
            },
            folder: String::new(),
            steps: vec![Step::menu(42024), Step::menu(41006)],
        }],
        internal: vec![],
        scint: vec![],
        commands: vec![
            cmd(
                "Get PHP help",
                112,
                "open \"https://www.php.net/$(CURRENT_WORD)\"",
            ),
            cmd(
                "Wikipedia Search",
                114,
                "open \"https://en.wikipedia.org/wiki/Special:Search?search=$(CURRENT_WORD)\"",
            ),
        ],
    }
}

fn attr(e: &BytesStart, key: &str) -> Option<String> {
    let a = e.try_get_attribute(key).ok().flatten()?;
    let raw = a.value.into_owned();
    Some(
        quick_xml::escape::unescape(&raw)
            .map(|v| v.into_owned())
            .unwrap_or(raw),
    )
}

fn int(e: &BytesStart, key: &str) -> Option<i64> {
    let v = attr(e, key)?;
    let v = v.trim();
    v.parse::<i64>()
        .ok()
        .or_else(|| v.parse::<u64>().ok().map(|u| u as i64))
}

fn yes(e: &BytesStart, key: &str) -> bool {
    attr(e, key).is_some_and(|v| v.eq_ignore_ascii_case("yes"))
}

// Port of NppParameters::getShortcuts: an item without a Key attribute is not loaded.
fn key_of(e: &BytesStart) -> Option<Key> {
    let key = int(e, "Key")?;
    (key != -1).then(|| Key {
        ctrl: yes(e, "Ctrl"),
        alt: yes(e, "Alt"),
        shift: yes(e, "Shift"),
        meta: yes(e, "MacControl"),
        key: key as u8,
    })
}

fn key_and_folder(e: &BytesStart) -> Option<(String, Key, String)> {
    Some((
        attr(e, "name").unwrap_or_default(),
        key_of(e)?,
        attr(e, "FolderName").unwrap_or_default(),
    ))
}

// Port of NppParameters::getActions, with its rule for old CR, CR LF and LF steps.
fn push_loaded(m: &mut Vec<Step>, step: Step) {
    let newline = || Step::new(TYPE_L, SCI_NEWLINE, 0, 0, "");
    let prev_cr = m
        .last()
        .is_some_and(|p| p.message == SCI_REPLACESEL && p.s == "\r");
    let is_cr = step.s == "\r";
    if step.message == SCI_REPLACESEL && matches!(step.s.as_str(), "\r" | "\r\n" | "\n") {
        if prev_cr {
            if is_cr {
                *m.last_mut().unwrap() = newline();
            } else {
                m.pop();
            }
        }
        m.push(if is_cr { step } else { newline() });
    } else {
        if prev_cr {
            *m.last_mut().unwrap() = newline();
        }
        m.push(step);
    }
}

fn end_loaded(m: &mut [Step]) {
    if let Some(p) = m
        .last_mut()
        .filter(|p| p.message == SCI_REPLACESEL && p.s == "\r")
    {
        *p = Step::new(TYPE_L, SCI_NEWLINE, 0, 0, "");
    }
}

pub fn parse(xml: &str) -> Result<Shortcuts, String> {
    let mut r = Reader::from_str(xml);
    let mut out = Shortcuts::default();
    let (mut section, mut in_macro, mut in_scint) = ("", false, false);
    let mut cmd: Option<Command> = None;
    loop {
        let ev = r.read_event().map_err(|e| e.to_string())?;
        if let Event::Start(e) | Event::Empty(e) = &ev {
            match (section, e.name().as_ref()) {
                ("Internal", "Shortcut") => {
                    if let (Some(id @ 1..), Some(key)) = (int(e, "id"), key_of(e)) {
                        out.internal.push(Internal {
                            id: id as i32,
                            nth: int(e, "nth").unwrap_or(0) as i32,
                            key,
                        });
                    }
                }
                ("Scint", "ScintKey") => {
                    let id = int(e, "ScintID").filter(|v| *v != -1);
                    let menu = int(e, "menuCmdID").filter(|v| *v != -1);
                    if let (Some(id), Some(menu), Some(key)) = (id, menu, key_of(e)) {
                        out.scint.push(ScintKey {
                            id: id as i32,
                            menu_id: menu as i32,
                            keys: vec![key],
                        });
                        in_scint = matches!(ev, Event::Start(_));
                    }
                }
                ("Scint", "NextKey") if in_scint => {
                    if let (Some(k), Some(sk)) = (key_of(e), out.scint.last_mut()) {
                        sk.keys.push(k);
                    }
                }
                _ => {}
            }
        }
        match ev {
            Event::Start(e) => match e.name().as_ref() {
                "InternalCommands" => section = "Internal",
                "Macros" => section = "Macros",
                "UserDefinedCommands" => section = "Cmds",
                "ScintillaKeys" => section = "Scint",
                "Macro" if section == "Macros" => {
                    in_macro = match key_and_folder(&e) {
                        Some((name, key, folder)) => {
                            out.macros.push(Macro {
                                name,
                                key,
                                folder,
                                steps: vec![],
                            });
                            true
                        }
                        None => false,
                    }
                }
                "Command" if section == "Cmds" => {
                    cmd = key_and_folder(&e).map(|(name, key, folder)| Command {
                        name,
                        key,
                        folder,
                        cmd: String::new(),
                    })
                }
                _ => {}
            },
            Event::Empty(e) if in_macro && e.name().as_ref() == "Action" => {
                let kind = int(&e, "type").unwrap_or(4);
                if (0..=3).contains(&kind) {
                    let step = Step::new(
                        kind as u8,
                        int(&e, "message").unwrap_or(0) as i32,
                        int(&e, "wParam").unwrap_or(0) as usize,
                        int(&e, "lParam").unwrap_or(0) as isize,
                        &attr(&e, "sParam").unwrap_or_default(),
                    );
                    push_loaded(&mut out.macros.last_mut().unwrap().steps, step);
                }
            }
            Event::Text(t) => {
                if let Some(c) = cmd.as_mut() {
                    c.cmd.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(g) => {
                if let Some(c) = cmd.as_mut() {
                    match g.resolve_char_ref() {
                        Ok(Some(ch)) => c.cmd.push(ch),
                        _ => c.cmd.push_str(
                            quick_xml::escape::resolve_predefined_entity(&g).unwrap_or(""),
                        ),
                    }
                }
            }
            Event::CData(t) => {
                if let Some(c) = cmd.as_mut() {
                    c.cmd.push_str(&t);
                }
            }
            Event::End(e) => match e.name().as_ref() {
                "InternalCommands" | "Macros" | "UserDefinedCommands" | "ScintillaKeys" => {
                    section = ""
                }
                "ScintKey" => in_scint = false,
                "Macro" if in_macro => {
                    end_loaded(&mut out.macros.last_mut().unwrap().steps);
                    in_macro = false;
                }
                "Command" => out
                    .commands
                    .extend(cmd.take().filter(|c| !c.cmd.is_empty())),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

// Escapes an attribute or text value; line ends become character references so that they stay unchanged.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\r' => o.push_str("&#x0D;"),
            '\n' => o.push_str("&#x0A;"),
            '\t' => o.push_str("&#x09;"),
            c => o.push(c),
        }
    }
    o
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn key_attrs(k: &Key) -> String {
    let mut s = format!(
        "Ctrl=\"{}\" Alt=\"{}\" Shift=\"{}\" Key=\"{}\"",
        yes_no(k.ctrl),
        yes_no(k.alt),
        yes_no(k.shift),
        k.key
    );
    if k.meta {
        s += " MacControl=\"yes\"";
    }
    s
}

fn head(tag: &str, name: &str, k: &Key, folder: &str) -> String {
    let mut s = format!("<{tag} name=\"{}\" {}", esc(name), key_attrs(k));
    if !folder.is_empty() {
        s += &format!(" FolderName=\"{}\"", esc(folder));
    }
    s
}

fn internal_xml(s: &Shortcuts) -> String {
    let mut o = String::from("<InternalCommands>\r\n");
    for c in &s.internal {
        o += &format!("\t\t<Shortcut id=\"{}\" {}", c.id, key_attrs(&c.key));
        if c.nth != 0 {
            o += &format!(" nth=\"{}\"", c.nth);
        }
        o += " />\r\n";
    }
    o + "\t</InternalCommands>"
}

fn scint_xml(s: &Shortcuts) -> String {
    let mut o = String::from("<ScintillaKeys>\r\n");
    for c in &s.scint {
        let first = c.keys.first().copied().unwrap_or_default();
        o += &format!(
            "\t\t<ScintKey ScintID=\"{}\" menuCmdID=\"{}\" {}",
            c.id,
            c.menu_id,
            key_attrs(&first)
        );
        if c.keys.len() < 2 {
            o += " />\r\n";
            continue;
        }
        o += ">\r\n";
        for k in &c.keys[1..] {
            o += &format!("\t\t\t<NextKey {} />\r\n", key_attrs(k));
        }
        o += "\t\t</ScintKey>\r\n";
    }
    o + "\t</ScintillaKeys>"
}

fn macros_xml(s: &Shortcuts) -> String {
    let mut o = String::from("<Macros>\r\n");
    for m in &s.macros {
        o += &format!("\t\t{}>\r\n", head("Macro", &m.name, &m.key, &m.folder));
        for a in &m.steps {
            o += &format!(
                "\t\t\t<Action type=\"{}\" message=\"{}\" wParam=\"{}\" lParam=\"{}\" sParam=\"{}\" />\r\n",
                a.kind,
                a.message,
                a.w,
                a.l,
                esc(&a.s)
            );
        }
        o += "\t\t</Macro>\r\n";
    }
    o + "\t</Macros>"
}

fn commands_xml(s: &Shortcuts) -> String {
    let mut o = String::from("<UserDefinedCommands>\r\n");
    for c in &s.commands {
        o += &format!(
            "\t\t{}>{}</Command>\r\n",
            head("Command", &c.name, &c.key, &c.folder),
            esc(&c.cmd)
        );
    }
    o + "\t</UserDefinedCommands>"
}

// Writes the InternalCommands, Macros, UserDefinedCommands and ScintillaKeys sections into `existing` and keeps its other content.
pub fn write(existing: Option<&str>, s: &Shortcuts) -> Result<String, String> {
    let Some(src) = existing else {
        return Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n\t{}\r\n\t{}\r\n\t{}\r\n\t<PluginCommands />\r\n\t{}\r\n</NotepadPlus>\r\n",
            internal_xml(s),
            macros_xml(s),
            commands_xml(s),
            scint_xml(s)
        ));
    };
    let sections = [
        ("InternalCommands", internal_xml(s)),
        ("Macros", macros_xml(s)),
        ("UserDefinedCommands", commands_xml(s)),
        ("ScintillaKeys", scint_xml(s)),
    ];
    let mut done = [false; 4];
    let mut r = Reader::from_str(src);
    let mut w = Writer::new(Vec::new());
    let (mut skip, mut depth) = (false, 0);
    let raw = |w: &mut Writer<Vec<u8>>, t: &str| w.get_mut().write_all(t.as_bytes());
    loop {
        let ev = r.read_event().map_err(|e| e.to_string())?;
        if skip {
            match ev {
                Event::Start(_) => depth += 1,
                Event::End(_) if depth == 0 => skip = false,
                Event::End(_) => depth -= 1,
                Event::Eof => return Err("shortcuts.xml: unexpected end".into()),
                _ => {}
            }
            continue;
        }
        match &ev {
            Event::Start(e) | Event::Empty(e) => {
                if let Some(i) = sections.iter().position(|x| x.0 == e.name().as_ref()) {
                    raw(&mut w, &sections[i].1).map_err(|e| e.to_string())?;
                    done[i] = true;
                    skip = matches!(ev, Event::Start(_));
                    depth = 0;
                    continue;
                }
            }
            Event::End(e) if e.name().as_ref() == "NotepadPlus" => {
                let mut t = String::new();
                for (i, (_, x)) in sections.iter().enumerate() {
                    if !done[i] {
                        t += &format!("\t{x}\r\n");
                    }
                }
                raw(&mut w, &t).map_err(|e| e.to_string())?;
                done = [true; 4];
            }
            Event::Eof => break,
            _ => {}
        }
        w.write_event(ev).map_err(|e| e.to_string())?;
    }
    if done.contains(&false) {
        return Err("shortcuts.xml: the NotepadPlus element is missing.".into());
    }
    String::from_utf8(w.into_inner()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sci_number(h: &str, name: &str) -> i32 {
        let p = format!("#define {name} ");
        h.lines()
            .find_map(|l| l.strip_prefix(&p))
            .unwrap_or_else(|| panic!("{name}"))
            .trim()
            .parse()
            .unwrap()
    }

    #[test]
    fn message_lists_match_notepad_plus_plus() {
        let h = include_str!("../../scintilla/include/Scintilla.h");
        let src = include_str!("../../PowerEditor/src/WinControls/shortcut/shortcut.cpp");
        let cases = |s: &str| -> Vec<i32> {
            s.split("case ")
                .skip(1)
                .filter_map(|c| {
                    let n = c.split([' ', ':']).next().unwrap();
                    n.starts_with("SCI_").then(|| sci_number(h, n))
                })
                .collect()
        };
        let m = &src[src.find("bool recordedMacroStep::isMacroable").unwrap()..];
        let m = &m[..m.find("default:").unwrap()];
        let split = m.find("mtUseSParameter").unwrap();
        assert_eq!(cases(&m[..split]), MACROABLE_S);
        assert_eq!(cases(&m[split..]), MACROABLE_L);
        let c = &src[src
            .find("recordedMacroStep::recordedMacroStep(int iMessage")
            .unwrap()..];
        let c = &c[..c.find("default").unwrap()];
        assert_eq!(cases(c), STRING_MSGS);
        assert_eq!(sci_number(h, "SCI_NEWLINE"), SCI_NEWLINE);
    }

    #[test]
    fn default_macro_is_the_notepad_plus_plus_one() {
        let h =
            include_str!("../../PowerEditor/src/MISC/Common/NppConstants.h").replace("\r\n", "\n");
        let x = &h[h.find("SHORTCUT_XML_CONTENT[] = \"").unwrap() + 26..];
        let x = &x[..x.find("\";\n").unwrap()];
        let xml = x
            .replace("\\\n", "")
            .replace("\\r\\n", "\r\n")
            .replace("\\\"", "\"");
        let npp = parse(&xml).unwrap();
        assert_eq!(npp.macros, defaults().macros);
        assert_eq!(npp.commands.len(), 3);
    }

    #[test]
    fn record_steps() {
        let mut m = vec![];
        record(&mut m, 2170, 0, 99, Some("ab"), false);
        record(&mut m, 2300, 0, 0, None, false);
        record(&mut m, 2024, 5, 0, None, false);
        assert_eq!(
            m,
            vec![
                Step::new(TYPE_S, 2170, 0, 0, "ab"),
                Step::new(TYPE_L, 2300, 0, 0, ""),
                Step::new(TYPE_L, 2024, 5, 0, ""),
            ]
        );
        let mut m = vec![];
        record(&mut m, 2170, 0, 1, Some("\r"), true);
        record(&mut m, 2170, 0, 1, Some("\n"), true);
        assert_eq!(m, vec![Step::new(TYPE_L, SCI_NEWLINE, 0, 0, "")]);
        record(&mut m, 2170, 0, 1, Some("\n"), false);
        assert_eq!(m.len(), 2);
        assert!(m.iter().all(Step::is_macroable));
        assert!(!Step::new(TYPE_L, 2170, 0, 0, "").is_macroable());
        assert!(!Step::new(TYPE_S, 2300, 0, 0, "x").is_macroable());
    }

    const SAMPLE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n<NotepadPlus>\r\n\t<InternalCommands>\r\n\t\t<Shortcut id=\"41001\" Ctrl=\"yes\" Alt=\"no\" Shift=\"no\" Key=\"78\" />\r\n\t</InternalCommands>\r\n\t<Macros>\r\n\t\t<!-- c -->\r\n\t\t<Macro name=\"Trim Trailing Space and Save\" Ctrl=\"no\" Alt=\"yes\" Shift=\"yes\" Key=\"83\">\r\n\t\t\t<Action type=\"2\" message=\"0\" wParam=\"42024\" lParam=\"0\" sParam=\"\" />\r\n\t\t\t<Action type=\"2\" message=\"0\" wParam=\"41006\" lParam=\"0\" sParam=\"\" />\r\n\t\t</Macro>\r\n\t\t<Macro name=\"a&amp;b\" Ctrl=\"no\" Alt=\"no\" Shift=\"no\" Key=\"0\" FolderName=\"words\">\r\n\t\t\t<Action type=\"1\" message=\"2170\" wParam=\"0\" lParam=\"0\" sParam=\"x&quot;y\" />\r\n\t\t\t<Action type=\"1\" message=\"2170\" wParam=\"0\" lParam=\"0\" sParam=\"&#x0D;\" />\r\n\t\t\t<Action type=\"1\" message=\"2170\" wParam=\"0\" lParam=\"0\" sParam=\"&#x0A;\" />\r\n\t\t\t<Action type=\"3\" message=\"1601\" wParam=\"0\" lParam=\"0\" sParam=\"find\" />\r\n\t\t\t<Action type=\"9\" message=\"1\" wParam=\"0\" lParam=\"0\" sParam=\"\" />\r\n\t\t</Macro>\r\n\t\t<Macro name=\"nokey\" Ctrl=\"no\" Alt=\"no\" Shift=\"no\">\r\n\t\t</Macro>\r\n\t</Macros>\r\n\t<UserDefinedCommands>\r\n\t\t<Command name=\"Get PHP help\" Ctrl=\"no\" Alt=\"yes\" Shift=\"no\" Key=\"112\">https://www.php.net/$(CURRENT_WORD)?a=1&amp;b=2</Command>\r\n\t</UserDefinedCommands>\r\n\t<PluginCommands />\r\n\t<ScintillaKeys />\r\n</NotepadPlus>\r\n";

    #[test]
    fn parse_notepad_plus_plus_file() {
        let s = parse(SAMPLE).unwrap();
        assert_eq!(s.macros.len(), 2);
        let m = &s.macros[0];
        assert_eq!(m.name, "Trim Trailing Space and Save");
        assert_eq!(
            m.key,
            Key {
                ctrl: false,
                alt: true,
                shift: true,
                meta: false,
                key: 83
            }
        );
        assert_eq!(m.steps, vec![Step::menu(42024), Step::menu(41006)]);
        let m = &s.macros[1];
        assert_eq!((m.name.as_str(), m.folder.as_str()), ("a&b", "words"));
        assert_eq!(
            m.steps,
            vec![
                Step::new(TYPE_S, 2170, 0, 0, "x\"y"),
                Step::new(TYPE_L, SCI_NEWLINE, 0, 0, ""),
                Step::new(TYPE_SNR, 1601, 0, 0, "find"),
            ]
        );
        assert_eq!(s.commands.len(), 1);
        assert_eq!(
            s.commands[0].cmd,
            "https://www.php.net/$(CURRENT_WORD)?a=1&b=2"
        );
        assert_eq!(s.commands[0].key.key, 112);
    }

    const KEYS: &str = "<NotepadPlus>\r\n\t<InternalCommands>\r\n\t\t<Shortcut id=\"41001\" Ctrl=\"yes\" Alt=\"no\" Shift=\"no\" Key=\"78\" />\r\n\t\t<Shortcut id=\"43003\" Ctrl=\"yes\" Alt=\"no\" Shift=\"no\" Key=\"72\" nth=\"1\" />\r\n\t\t<Shortcut id=\"41002\" Ctrl=\"no\" Alt=\"no\" Shift=\"no\" Key=\"0\" />\r\n\t\t<Shortcut id=\"0\" Ctrl=\"no\" Alt=\"no\" Shift=\"no\" Key=\"65\" />\r\n\t\t<Shortcut id=\"41003\" Ctrl=\"yes\" />\r\n\t\t<Shortcut id=\"41004\" Ctrl=\"no\" Alt=\"no\" Shift=\"no\" Key=\"87\" MacControl=\"yes\" />\r\n\t</InternalCommands>\r\n\t<ScintillaKeys>\r\n\t\t<ScintKey ScintID=\"2180\" menuCmdID=\"42006\" Ctrl=\"no\" Alt=\"no\" Shift=\"no\" Key=\"46\" />\r\n\t\t<ScintKey ScintID=\"2308\" menuCmdID=\"0\" Ctrl=\"no\" Alt=\"yes\" Shift=\"no\" Key=\"37\">\r\n\t\t\t<NextKey Ctrl=\"no\" Alt=\"no\" Shift=\"no\" Key=\"37\" MacControl=\"yes\" />\r\n\t\t</ScintKey>\r\n\t\t<ScintKey ScintID=\"2309\" Ctrl=\"no\" Alt=\"no\" Shift=\"no\" Key=\"37\" />\r\n\t</ScintillaKeys>\r\n</NotepadPlus>\r\n";

    fn k(ctrl: bool, alt: bool, shift: bool, meta: bool, key: u8) -> Key {
        Key {
            ctrl,
            alt,
            shift,
            meta,
            key,
        }
    }

    #[test]
    fn internal_commands_and_scintilla_keys_round_trip() {
        let s = parse(KEYS).unwrap();
        let ids: Vec<_> = s.internal.iter().map(|c| (c.id, c.nth, c.key)).collect();
        assert_eq!(
            ids,
            vec![
                (41001, 0, k(true, false, false, false, 78)),
                (43003, 1, k(true, false, false, false, 72)),
                (41002, 0, Key::default()),
                (41004, 0, k(false, false, false, true, 87)),
            ]
        );
        assert_eq!(
            s.scint,
            vec![
                ScintKey {
                    id: 2180,
                    menu_id: 42006,
                    keys: vec![k(false, false, false, false, 46)]
                },
                ScintKey {
                    id: 2308,
                    menu_id: 0,
                    keys: vec![k(false, true, false, false, 37), k(false, false, false, true, 37)]
                },
            ]
        );
        let out = write(Some(KEYS), &s).unwrap();
        assert_eq!(parse(&out).unwrap(), s);
        assert!(out.contains("<Shortcut id=\"43003\" Ctrl=\"yes\" Alt=\"no\" Shift=\"no\" Key=\"72\" nth=\"1\" />"));
        assert!(out.contains("<ScintKey ScintID=\"2180\" menuCmdID=\"42006\" Ctrl=\"no\" Alt=\"no\" Shift=\"no\" Key=\"46\" />"));
        assert!(out.contains("<Macros>") && out.contains("<UserDefinedCommands>"));
        let mut t = s.clone();
        t.internal.remove(0);
        t.scint[1].keys.pop();
        t.macros = defaults().macros;
        let out = write(Some(&out), &t).unwrap();
        assert_eq!(parse(&out).unwrap(), t);
        assert_eq!(parse(&write(None, &t).unwrap()).unwrap(), t);
        let bare = write(Some("<NotepadPlus><Macros /></NotepadPlus>"), &t).unwrap();
        assert_eq!(parse(&bare).unwrap(), t);
        assert!(!write(None, &Shortcuts::default()).unwrap().contains("MacControl"));
    }

    #[test]
    fn loaded_line_ends() {
        let mut m = vec![];
        for s in ["\r", "\r"] {
            push_loaded(&mut m, Step::new(TYPE_S, SCI_REPLACESEL, 0, 0, s));
        }
        end_loaded(&mut m);
        assert_eq!(m, vec![Step::new(TYPE_L, SCI_NEWLINE, 0, 0, ""); 2]);
        let mut m = vec![];
        push_loaded(&mut m, Step::new(TYPE_S, SCI_REPLACESEL, 0, 0, "x"));
        push_loaded(&mut m, Step::new(TYPE_S, SCI_REPLACESEL, 0, 0, "\r\n"));
        assert_eq!(m[1], Step::new(TYPE_L, SCI_NEWLINE, 0, 0, ""));
    }

    #[test]
    fn write_round_trip() {
        let mut s = parse(SAMPLE).unwrap();
        s.macros.push(Macro {
            name: "new <one>".into(),
            key: Key::default(),
            folder: String::new(),
            steps: vec![
                Step::new(TYPE_S, 2170, 0, 0, "a\tb\r\nc & d"),
                Step::new(TYPE_L, 2300, 0, -1, ""),
                Step::menu(41001),
            ],
        });
        s.commands.push(Command {
            name: "Echo".into(),
            key: Key::default(),
            folder: "Tools".into(),
            cmd: "echo \"$(FILE_NAME)\" > /tmp/x && true".into(),
        });
        let out = write(Some(SAMPLE), &s).unwrap();
        assert_eq!(parse(&out).unwrap(), s);
        assert!(out.contains("<Shortcut id=\"41001\""));
        assert!(out.contains("<PluginCommands />"));
        assert!(!out.contains("<!-- c -->"));
        let fresh = write(None, &s).unwrap();
        assert_eq!(parse(&fresh).unwrap(), s);
        assert_eq!(
            parse(&write(None, &defaults()).unwrap()).unwrap(),
            defaults()
        );
        let bare = "<NotepadPlus>\n<Macros />\n</NotepadPlus>";
        let out = write(Some(bare), &s).unwrap();
        assert_eq!(parse(&out).unwrap(), s);
        assert!(write(Some("<NotepadPlus><Macros>"), &s).is_err());
        assert!(write(Some("<Other><Item /></Other>"), &s).is_err());
        assert!(write(Some("<Other><Macros /></Other>"), &s).is_err());
        assert!(write(Some(""), &s).is_err());
    }
}
