# Notepad++ for macOS (unofficial)

An unofficial, modified version of Notepad++ for macOS on Apple Silicon (arm64).
The app is written in Rust. It uses the Scintilla and Lexilla code in this repository
and the language and style files from `PowerEditor/src`.

Not affiliated with the Notepad++ project.

## Changes from Notepad++

2026-10-08:

- Search menu: Find, Replace, Find Next, Find Previous, Find in Files, and Go to Line.
- Find and Replace use the Notepad++ Boost regex engine (`boostregex/`), built into Scintilla with `SCI_OWNREGEX`.
- One Find panel holds both the Find and the Replace fields. There is no Mark tab, no search history, and no "In selection" or "Backward direction" option.
- Find in Files reads files from disk, also when a file is open in a tab with unsaved changes. It skips files that contain a NUL byte.
- Replace in Files writes to disk. An open tab of a changed file does not reload.
- The Search results panel does not fold old searches.

2026-10-07:

- New native macOS app in Rust (AppKit through the objc2 crates). It replaces the Win32 user interface.
- Tabs, New, Open, Save, Save As, Close, and a prompt before a modified tab closes.
- Syntax colours from `langs.model.xml` and `stylers.model.xml` (default theme only).
- UTF-8 files only. No Find/Replace, sessions, macros, plugins, or preferences yet.
- `lexilla/lexers/LexUser.cxx` is not built because it needs `windows.h`. A stub replaces it, so user defined languages have no colours.

## Build and run

Requirements: macOS on arm64, Rust (cargo), and the Xcode Command Line Tools.

```sh
cd macos
cargo test
cargo build --release
./target/release/notepadpp-mac samples/hello.py
```

## Licence

- The code in `macos/` is licensed under GPL-3.0-or-later, the same as Notepad++.
- Scintilla and Lexilla have their own licence: see `scintilla/License.txt` and `lexilla/License.txt`.
- The Boost files in `boostregex/boost` are licensed under the Boost Software License 1.0.
- The Rust crates this app uses are licensed under MIT, Apache-2.0, Zlib, or Unlicense terms.
