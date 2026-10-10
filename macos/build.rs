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

fn main() {
    embed_apis();
    base()
        .files(files("../scintilla/src", ".cxx"))
        .file("../boostregex/BoostRegExSearch.cxx")
        .file("../boostregex/UTF8DocumentIterator.cxx")
        .file("src/docsearch.cxx")
        .compile("scintilla");
    base()
        .include("../scintilla/cocoa")
        .flag("-fobjc-arc")
        .files(files("../scintilla/cocoa", ".mm"))
        .compile("scintilla_cocoa");
    base()
        .define("LEXILLA_NO_EXPORT", None)
        .files(files("../lexilla/src", ".cxx"))
        .files(files("../lexilla/lexlib", ".cxx"))
        .files(
            files("../lexilla/lexers", ".cxx")
                .into_iter()
                .filter(|f| !f.ends_with("LexUser.cxx")),
        )
        .file("src/lexuser_stub.cxx")
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
        "src/lexuser_stub.cxx",
        "../boostregex",
        "src/docsearch.cxx",
        "../PowerEditor/src/uchardet",
        "src/charset.cxx",
        "../PowerEditor/installer/APIs",
    ] {
        println!("cargo:rerun-if-changed={d}");
    }
}
