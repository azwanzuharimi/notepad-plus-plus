// SPDX-License-Identifier: GPL-3.0-or-later
use crate::panel::{self, Form};
use crate::shortcuts::Command;
use crate::{macros, ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSButton, NSComboBox, NSMenu, NSMenuItem, NSModalResponseOK, NSOpenPanel};
use objc2_foundation::NSPoint;
use std::cell::OnceCell;
use std::path::Path;
use std::process::Stdio;

const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_GETCOLUMN: u32 = 2129;
const SCI_SETSELECTIONSTART: u32 = 2142;
const SCI_SETSELECTIONEND: u32 = 2144;
const SCI_GETLINE: u32 = 2153;
const SCI_WORDSTARTPOSITION: u32 = 2266;
const SCI_WORDENDPOSITION: u32 = 2267;
const SCI_LINELENGTH: u32 = 2350;

// The Run dialog variables in Notepad++ menu order, with the hint text of the variables menu.
pub const VARS: [(&str, &str); 11] = [
    ("FULL_CURRENT_PATH", "Full path to active file"),
    ("CURRENT_DIRECTORY", "Active file's directory"),
    ("FILE_NAME", "Active file's name"),
    ("NAME_PART", "File name without extension"),
    ("EXT_PART", "File extension (with .)"),
    ("CURRENT_WORD", "Selected word or word under caret"),
    ("NPP_DIRECTORY", "Directory of this app's program file"),
    ("NPP_FULL_FILE_PATH", "Full path of this app's program file"),
    ("CURRENT_LINE", "Line number of caret"),
    ("CURRENT_COLUMN", "Column number of caret"),
    ("CURRENT_LINESTR", "Current line text"),
];

// FULL_CURRENT_PATH, CURRENT_DIRECTORY, FILE_NAME, NAME_PART and EXT_PART with the Windows PathFindExtension rules.
pub fn path_parts(full: &str) -> [String; 5] {
    let (dir, file) = match full.rfind('/') {
        Some(0) => ("/", &full[1..]),
        Some(i) => (&full[..i], &full[i + 1..]),
        None => ("", full),
    };
    let ext = file
        .rfind('.')
        .filter(|&i| !file[i..].contains(' '))
        .map_or("", |i| &file[i..]);
    [
        full.into(),
        dir.into(),
        file.into(),
        file[..file.len() - ext.len()].into(),
        ext.into(),
    ]
}

#[derive(Clone, Copy, PartialEq)]
enum Quote {
    None,
    Single,
    Double,
}

pub fn env_name(i: usize) -> String {
    format!("NPP_{}", VARS[i].0)
}

// Quotes a path for /bin/sh as one word.
pub fn sh_quote(v: &str) -> String {
    if !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%".contains(c))
    {
        return v.into();
    }
    format!("'{}'", v.replace('\'', "'\\''"))
}

// Port of expandNppEnvironmentStrs for /bin/sh: a known $(NAME) becomes a reference to the environment variable NPP_NAME.
// The value never goes into the command text, so the shell cannot read it as code. Returns the command and the VARS indexes it uses.
// The quote state follows $( ), backquotes and quotes; if it is wrong, the result is a broken string, not code.
pub fn expand(cmd: &str) -> (String, Vec<usize>) {
    struct Ctx {
        tick: bool,
        depth: usize,
        q: Quote,
    }
    let mut out = String::new();
    let mut used = vec![];
    let mut st = vec![Ctx {
        tick: false,
        depth: 0,
        q: Quote::None,
    }];
    let mut rest = cmd;
    while let Some(c) = rest.chars().next() {
        let q = st.last().unwrap().q;
        if let Some(after) = rest.strip_prefix("$(") {
            let var = after
                .find(')')
                .and_then(|e| Some((VARS.iter().position(|v| v.0 == &after[..e])?, e)));
            if let Some((i, e)) = var {
                let r = format!("${{{}}}", env_name(i));
                out += &match q {
                    Quote::None => format!("\"{r}\""),
                    Quote::Double => r,
                    Quote::Single => format!("'\"{r}\"'"),
                };
                if !used.contains(&i) {
                    used.push(i);
                }
                rest = &after[e + 1..];
                continue;
            }
            if q != Quote::Single {
                out += "$(";
                rest = after;
                st.push(Ctx {
                    tick: false,
                    depth: 0,
                    q: Quote::None,
                });
                continue;
            }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
        let nested = st.len() > 1;
        let top = st.last_mut().unwrap();
        match (top.q, c) {
            (Quote::None | Quote::Double, '\\') => {
                if let Some(n) = rest.chars().next() {
                    out.push(n);
                    rest = &rest[n.len_utf8()..];
                }
            }
            (Quote::None, '`') if top.tick => {
                st.pop();
            }
            (Quote::None | Quote::Double, '`') => st.push(Ctx {
                tick: true,
                depth: 0,
                q: Quote::None,
            }),
            (Quote::None, '(') => top.depth += 1,
            (Quote::None, ')') if top.depth > 0 => top.depth -= 1,
            (Quote::None, ')') if nested && !top.tick => {
                st.pop();
            }
            (Quote::None, '\'') => top.q = Quote::Single,
            (Quote::None, '"') => top.q = Quote::Double,
            (Quote::Single, '\'') | (Quote::Double, '"') => top.q = Quote::None,
            _ => {}
        }
    }
    (out, used)
}

fn sh(cmd: &str, dir: &Path, env: &[(String, String)]) -> std::process::Command {
    let mut c = std::process::Command::new("/bin/sh");
    c.arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

// Starts the command with /bin/sh in its own process group and does not wait for it.
pub fn spawn(cmd: &str, dir: &Path, env: &[(String, String)]) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut child = sh(cmd, dir, env).process_group(0).spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

struct RunUi {
    form: Form,
    cmd: Retained<NSComboBox>,
    vars: Retained<NSButton>,
}

thread_local! {
    static RUN_UI: OnceCell<RunUi> = const { OnceCell::new() };
}

fn with_ui<R>(app: &App, f: impl FnOnce(&RunUi) -> R) -> R {
    RUN_UI.with(|c| {
        f(c.get_or_init(|| {
            let mtm = app.mtm();
            let t: &AnyObject = app;
            let f = Form::new(mtm, "Run...", 520., 130.);
            f.label("The Program to Run", 16., 12., 300.);
            let cmd = NSComboBox::new(mtm);
            cmd.setNumberOfVisibleItems(10);
            f.place(&cmd, 16., 40., 408., 26.);
            f.button("...", 428., 38., 40., t, sel!(runBrowse:));
            let vars = f.button("+", 468., 38., 40., t, sel!(runVariables:));
            f.button("Run", 120., 86., 90., t, sel!(runExecute:))
                .setKeyEquivalent(&ns("\r"));
            f.button("Save...", 215., 86., 90., t, sel!(runSave:));
            f.button("Cancel", 310., 86., 90., t, sel!(closePanel:))
                .setKeyEquivalent(&ns("\u{1b}"));
            RunUi { form: f, cmd, vars }
        }))
    })
}

impl App {
    fn run_dir(&self) -> std::path::PathBuf {
        self.current()
            .and_then(|i| self.tab(i)?.path?.parent().map(Path::to_path_buf))
            .or_else(|| std::env::var_os("HOME").map(Into::into))
            .unwrap_or_else(|| "/".into())
    }

    // Value of VARS[i] for the active tab, as the Notepad++ RUNCOMMAND_USER messages give it.
    fn run_value(&self, i: usize) -> String {
        let tab = self.current().and_then(|i| self.tab(i));
        let full = tab
            .as_ref()
            .map(|t| {
                t.path
                    .as_ref()
                    .map_or(t.name.clone(), |p| p.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(v) = tab.map(|t| t.view) else {
            return match i {
                0..=4 => String::new(),
                6 => path_parts(&exe)[1].clone(),
                7 => exe,
                _ => String::new(),
            };
        };
        let pos = sci::send(&v, SCI_GETCURRENTPOS, 0, 0);
        match i {
            0..=4 => path_parts(&full)[i].clone(),
            5 => {
                let (mut s, mut e) = sci::selection(&v);
                if s == e {
                    let ws = sci::send(&v, SCI_WORDSTARTPOSITION, pos as usize, 1);
                    let we = sci::send(&v, SCI_WORDENDPOSITION, pos as usize, 1);
                    if ws != we {
                        sci::send(&v, SCI_SETSELECTIONSTART, ws as usize, 0);
                        sci::send(&v, SCI_SETSELECTIONEND, we as usize, 0);
                        (s, e) = (ws, we);
                    }
                }
                String::from_utf8_lossy(&sci::doc(&v).range(s, e)).into_owned()
            }
            6 => path_parts(&exe)[1].clone(),
            7 => exe,
            8 => sci::send(&v, sci::SCI_LINEFROMPOSITION, pos as usize, 0).to_string(),
            9 => sci::send(&v, SCI_GETCOLUMN, pos as usize, 0).to_string(),
            _ => {
                let line = sci::send(&v, sci::SCI_LINEFROMPOSITION, pos as usize, 0) as usize;
                let n = sci::send(&v, SCI_LINELENGTH, line, 0).max(0) as usize;
                let mut b = vec![0u8; n + 1];
                sci::send(&v, SCI_GETLINE, line, b.as_mut_ptr() as isize);
                b.truncate(n);
                String::from_utf8_lossy(&b).into_owned()
            }
        }
    }

    // Port of Command::run: expands the variables, then starts the command in the folder of the active file.
    pub(crate) fn run_command(&self, cmd: &str) -> bool {
        let (line, used) = expand(cmd);
        let env: Vec<_> = used
            .iter()
            .map(|&i| (env_name(i), self.run_value(i)))
            .collect();
        match spawn(&line, &self.run_dir(), &env) {
            Ok(()) => true,
            Err(e) => {
                self.alert(
                    "Run - ERROR",
                    &format!(
                        "{e}\nAn attempt was made to execute the below command.\n----------------------------------------------------------\nCommand: {line}\n----------------------------------------------------------"
                    ),
                    &["OK"],
                );
                false
            }
        }
    }

    pub(crate) fn run_show(&self) {
        with_ui(self, |u| {
            u.form.panel.makeKeyAndOrderFront(None);
            u.form.panel.makeFirstResponder(Some(&u.cmd));
        });
    }

    pub(crate) fn run_execute(&self) {
        let cmd = with_ui(self, |u| panel::text(&u.cmd));
        let ok = self.run_command(&cmd);
        let v = ns(&cmd);
        with_ui(self, |u| {
            if !ok {
                unsafe { u.cmd.removeItemWithObjectValue(&v) };
                return;
            }
            if unsafe { u.cmd.indexOfItemWithObjectValue(&v) } < 0 {
                unsafe { u.cmd.addItemWithObjectValue(&v) };
            }
            u.form.panel.orderOut(None);
        });
    }

    pub(crate) fn run_save(&self) {
        let cmd = with_ui(self, |u| panel::text(&u.cmd));
        let Some(name) = self.ask_shortcut_name() else {
            return;
        };
        macros::with_store(|s| {
            s.commands.push(Command {
                name,
                cmd,
                ..Command::default()
            })
        });
        self.store_changed();
    }

    pub(crate) fn run_browse(&self) {
        let p = NSOpenPanel::openPanel(self.mtm());
        if p.runModal() != NSModalResponseOK {
            return;
        }
        if let Some(path) = p.URL().and_then(|u| u.path()) {
            let s = sh_quote(&path.to_string());
            with_ui(self, |u| u.cmd.setStringValue(&ns(&s)));
        }
    }

    pub(crate) fn run_variables(&self) {
        let mtm = self.mtm();
        let m = NSMenu::new(mtm);
        for (i, (name, hint)) in VARS.iter().enumerate() {
            m.addItem(&crate::tagged(
                mtm,
                &format!("{name}  ({hint})"),
                sel!(runInsertVariable:),
                i as isize,
                Some(self),
            ));
        }
        with_ui(self, |u| {
            let h = u.vars.frame().size.height;
            m.popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(0., h), Some(&u.vars));
        });
    }

    pub(crate) fn run_insert_variable(&self, s: &NSMenuItem) {
        if let Some((name, _)) = VARS.get(s.tag() as usize) {
            with_ui(self, |u| {
                let t = format!("{}$({name})", panel::text(&u.cmd));
                u.cmd.setStringValue(&ns(&t));
            });
        }
    }

    pub(crate) fn run_user_command(&self, s: &NSMenuItem) {
        let cmd = macros::with_store(|st| st.commands.get(s.tag() as usize).map(|c| c.cmd.clone()));
        if let Some(c) = cmd {
            self.run_command(&c);
        }
    }
}

pub fn run_menu_items(mtm: MainThreadMarker, t: Option<&AnyObject>) -> Vec<Retained<NSMenuItem>> {
    vec![crate::item(mtm, "Run...", sel!(runShow:), "\u{F708}", t)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(values: &[(&str, &str)]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|(k, v)| (format!("NPP_{k}"), v.to_string()))
            .collect()
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("npp-run-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // Runs the expanded command and waits, so that the test can look at the files it made.
    fn run_sync(cmd: &str, dir: &Path, values: &[(&str, &str)]) -> String {
        let (line, _) = expand(cmd);
        let mut c = sh(&line, dir, &env(values));
        c.stdout(Stdio::piped());
        String::from_utf8(c.output().unwrap().stdout).unwrap()
    }

    #[test]
    fn parts() {
        assert_eq!(
            path_parts("/a/b/c.tar.gz"),
            ["/a/b/c.tar.gz", "/a/b", "c.tar.gz", "c.tar", ".gz"]
        );
        assert_eq!(path_parts("new 1"), ["new 1", "", "new 1", "new 1", ""]);
        assert_eq!(path_parts("/x"), ["/x", "/", "x", "x", ""]);
        assert_eq!(
            path_parts("/d/.bashrc"),
            ["/d/.bashrc", "/d", ".bashrc", "", ".bashrc"]
        );
        assert_eq!(
            path_parts("/d/a.b c"),
            ["/d/a.b c", "/d", "a.b c", "a.b c", ""]
        );
    }

    #[test]
    fn expansion() {
        let r = |n: &str| format!("${{NPP_{n}}}");
        assert_eq!(
            expand("echo $(CURRENT_LINE)"),
            (format!("echo \"{}\"", r("CURRENT_LINE")), vec![8])
        );
        assert_eq!(
            expand("open \"$(FULL_CURRENT_PATH)\" $(FULL_CURRENT_PATH)").0,
            format!("open \"{0}\" \"{0}\"", r("FULL_CURRENT_PATH"))
        );
        assert_eq!(
            expand("echo '$(FILE_NAME)'").0,
            format!("echo ''\"{}\"''", r("FILE_NAME"))
        );
        assert_eq!(
            expand("echo $(NOPE) $(date) $(").0,
            "echo $(NOPE) $(date) $("
        );
        assert_eq!(expand("a \\\"$(EXT_PART)").1, vec![4]);
        assert_eq!(sh_quote("/a/b.txt"), "/a/b.txt");
        assert_eq!(sh_quote("/a b/it's"), "'/a b/it'\\''s'");
    }

    #[test]
    fn shell_reads_values_back() {
        let dir = temp_dir("values");
        let path = "/tmp/a b/it's.txt";
        let word = "x\"$y`z";
        let v = [("FULL_CURRENT_PATH", path), ("CURRENT_WORD", word)];
        let out = run_sync(
            "printf '%s|%s|%s|%s' \"$(CURRENT_WORD)\" $(FULL_CURRENT_PATH) '$(FULL_CURRENT_PATH)' \"$(dirname \"$(FULL_CURRENT_PATH)\")\"",
            &dir,
            &v,
        );
        assert_eq!(out, format!("{word}|{path}|{path}|/tmp/a b"));
        let (line, _) = expand("printf %s $(CURRENT_WORD) > out");
        spawn(&line, &dir, &env(&v)).unwrap();
        let mut got = String::new();
        for _ in 0..100 {
            got = std::fs::read_to_string(dir.join("out")).unwrap_or_default();
            if got == word {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(got, word);
    }

    #[test]
    fn values_never_run_as_code() {
        let templates = [
            "echo \"$(dirname \"$(FULL_CURRENT_PATH)\")\"",
            "echo \"`echo $(FULL_CURRENT_PATH)`\"",
            "cat <<EOF\n$(FULL_CURRENT_PATH)\nEOF",
            "cat <<'EOF'\n$(FULL_CURRENT_PATH)\nEOF",
            "echo $(FULL_CURRENT_PATH) '$(FULL_CURRENT_PATH)' \"$(FULL_CURRENT_PATH)\"",
            "echo \"$(echo '$(FULL_CURRENT_PATH)')\"",
            "echo `echo \"$(FULL_CURRENT_PATH)\"` $((1+1))",
        ];
        let payloads = [
            "\"; touch pwned; \"",
            "$(touch pwned)",
            "`touch pwned`",
            "'; touch pwned; '",
            "a\ntouch pwned\n",
        ];
        let dir = temp_dir("inject");
        for t in templates {
            for p in payloads {
                run_sync(t, &dir, &[("FULL_CURRENT_PATH", p)]);
                assert!(!dir.join("pwned").exists(), "{t:?} with {p:?}");
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
