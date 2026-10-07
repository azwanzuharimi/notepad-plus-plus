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

fn main() {
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
        .files(files("../PowerEditor/src/uchardet", ".cpp"))
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
    ] {
        println!("cargo:rerun-if-changed={d}");
    }
}
