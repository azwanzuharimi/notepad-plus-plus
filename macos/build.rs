// SPDX-License-Identifier: GPL-3.0-or-later
use std::fs;

fn files(dir: &str, ext: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok()?.path().to_str().map(String::from))
        .filter(|p| p.ends_with(ext))
        .collect();
    v.sort();
    v
}

fn base() -> cc::Build {
    let mut b = cc::Build::new();
    b.cpp(true)
        .std("c++17")
        .define("NDEBUG", None)
        .define("SCI_OWNREGEX", None)
        .define("BOOST_REGEX_STANDALONE", None)
        .include("../boostregex")
        .include("../scintilla/include")
        .include("../scintilla/src")
        .include("../lexilla/include")
        .include("../lexilla/lexlib")
        .warnings(false);
    b
}

// Writes APIS: the PowerEditor/installer/APIs files embedded by name, as Notepad++ installs them in autoCompletion.
fn embed_apis() {
    let mut out = String::from("pub const APIS: &[(&str, &str)] = &[\n");
    for f in files("../PowerEditor/installer/APIs", ".xml") {
        let p = fs::canonicalize(&f).unwrap();
        let name = p.file_stem().unwrap().to_str().unwrap().to_string();
        out += &format!("    ({name:?}, include_str!({:?})),\n", p.to_str().unwrap());
    }
    out += "];\n";
    fs::write(std::env::var("OUT_DIR").unwrap() + "/apis.rs", out).unwrap();
}


// Writes FUNCTION_LISTS: the PowerEditor/installer/functionList files embedded by file name.
fn embed_function_lists() {
    let mut out = String::from("pub const FUNCTION_LISTS: &[(&str, &str)] = &[\n");
    for f in files("../PowerEditor/installer/functionList", ".xml") {
        let p = fs::canonicalize(&f).unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        out += &format!("    ({name:?}, include_str!({:?})),\n", p.to_str().unwrap());
    }
    out += "];\n";
    fs::write(std::env::var("OUT_DIR").unwrap() + "/function_lists.rs", out).unwrap();
}


// Writes NATIVE_LANGS: the PowerEditor/installer/nativeLang files embedded by file name.
fn embed_native_langs() {
    let mut out = String::from("pub const NATIVE_LANGS: &[(&str, &str)] = &[\n");
    for f in files("../PowerEditor/installer/nativeLang", ".xml") {
        let p = fs::canonicalize(&f).unwrap();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        out += &format!("    ({name:?}, include_str!({:?})),\n", p.to_str().unwrap());
    }
    out += "];\n";
    fs::write(std::env::var("OUT_DIR").unwrap() + "/native_langs.rs", out).unwrap();
}


// An .ico file with only its 16, 32 and 64 px images: the small and large toolbar sizes at 1x and 2x.
fn ico_1x_2x(b: &[u8]) -> Vec<u8> {
    let le = |at: usize, n: usize| b[at..at + n].iter().rev().fold(0usize, |v, x| v << 8 | *x as usize);
    let keep: Vec<usize> = (0..le(4, 2)).map(|i| 6 + 16 * i).filter(|&e| [16, 32, 64].contains(&b[e])).collect();
    let mut head = vec![0, 0, 1, 0, keep.len() as u8, 0];
    let mut data = vec![];
    let mut at = 6 + 16 * keep.len();
    for e in keep {
        let (size, off) = (le(e + 8, 4), le(e + 12, 4));
        head.extend_from_slice(&b[e..e + 8]);
        head.extend_from_slice(&(size as u32).to_le_bytes());
        head.extend_from_slice(&(at as u32).to_le_bytes());
        data.extend_from_slice(&b[off..off + size]);
        at += size;
    }
    head.extend(data);
    head
}

// Writes TOOLBAR_ICONS: the toolbar icons of PowerEditor/src/icons by path in that folder, without the unused disabled icons and sizes.
fn embed_toolbar_icons() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let mut out = String::from("pub const TOOLBAR_ICONS: &[(&str, &[u8])] = &[\n");
    for d in ["light/toolbar/regular", "light/toolbar/filled", "dark/toolbar/regular", "dark/toolbar/filled", "standard/toolbar"] {
        let ext = if d.starts_with("standard") { ".bmp" } else { "_off.ico" };
        for f in files(&format!("../PowerEditor/src/icons/{d}"), ext) {
            let p = fs::canonicalize(&f).unwrap();
            let file = p.file_name().unwrap().to_str().unwrap();
            let name = format!("{d}/{file}");
            let mut bytes = fs::read(&p).unwrap();
            if ext != ".bmp" {
                bytes = ico_1x_2x(&bytes);
            }
            let dest = format!("{out_dir}/toolbar_{}", name.replace('/', "_"));
            fs::write(&dest, bytes).unwrap();
            out += &format!("    ({name:?}, include_bytes!({dest:?})),\n");
        }
    }
    out += "];\n";
    fs::write(format!("{out_dir}/toolbar_icons.rs"), out).unwrap();
}

fn main() {
    embed_apis();
    embed_function_lists();
    embed_native_langs();
    embed_toolbar_icons();
    base()
        .files(files("../scintilla/src", ".cxx"))
        .file("../boostregex/BoostRegExSearch.cxx")
        .file("../boostregex/UTF8DocumentIterator.cxx")
        .file("src/docsearch.cxx")
        .compile("scintilla");
    base()
        .include("../scintilla/cocoa")
        .define("SCROLL_WHEEL_MAGNIFICATION", None)
        .flag("-fobjc-arc")
        .files(files("../scintilla/cocoa", ".mm"))
        .compile("scintilla_cocoa");
    base()
        .define("LEXILLA_NO_EXPORT", None)
        .files(files("../lexilla/src", ".cxx"))
        .files(files("../lexilla/lexlib", ".cxx"))
        .include("src/shim")
        .files(files("../lexilla/lexers", ".cxx"))
        .compile("lexilla");
    base()
        .include("../PowerEditor/src/uchardet")
        .files(files("../PowerEditor/src/uchardet", ".cpp"))
        .file("src/charset.cxx")
        .compile("uchardet");
    for f in ["Cocoa", "QuartzCore"] {
        println!("cargo:rustc-link-lib=framework={f}");
    }
    println!("cargo:rustc-link-lib=c++");
    for d in [
        "../scintilla/src",
        "../scintilla/cocoa",
        "../lexilla",
        "src/shim",
        "../boostregex",
        "src/docsearch.cxx",
        "../PowerEditor/src/uchardet",
        "src/charset.cxx",
        "../PowerEditor/installer/APIs",
        "../PowerEditor/installer/functionList",
        "../PowerEditor/installer/nativeLang",
        "../PowerEditor/src/icons",
    ] {
        println!("cargo:rerun-if-changed={d}");
    }
}
