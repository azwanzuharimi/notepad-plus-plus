# Notepad++ for macOS (Rust) — Slice 1 spec

Date: 2026-10-07. Branch: `macos-rust`. Target: macOS arm64 only.

## Goal
A native Mac app that edits files with the real Notepad++ editing engine and the real Notepad++ syntax colours.

## Architecture
```
macos/  (Rust crate "notepadpp-mac")
├── build.rs      compiles ../scintilla (src + cocoa) and ../lexilla (src, lexlib, lexers) with the cc crate, links Cocoa/QuartzCore
├── src/main.rs   AppKit app via objc2 crates: window, menus, tab bar, one ScintillaView per tab
├── src/sci.rs    thin wrapper: send SCI_* messages to a ScintillaView
├── src/config.rs parse ../PowerEditor/src/langs.model.xml + stylers.model.xml (quick-xml)
└── src/lang.rs   file extension -> Notepad++ language -> Lexilla lexer + keywords + styles
```
Config files are read from the repo at build time (`include_str!`) so the app needs no install step.

## Slice 1 features
1. Window with tabs. New tab (Cmd+N), Close tab (Cmd+W), switch tabs by click.
2. Open (Cmd+O, NSOpenPanel), Save (Cmd+S), Save As (Cmd+Shift+S, NSSavePanel). UTF-8 only in this slice.
3. Modified marker on tab title; ask before closing a modified tab.
4. Syntax highlighting: pick the language by file extension from `langs.model.xml`; set the Lexilla lexer, keyword lists, and styles from `stylers.model.xml` (default theme, the `LexerType` entries, plus `GlobalStyles` default/line number/caret line).
5. Line numbers margin. Standard Edit menu: Undo, Redo, Cut, Copy, Paste, Select All.
6. Open files passed on the command line.

## Out of scope (later slices)
Find/Replace, encodings other than UTF-8, sessions, macros, plugins (Win32 DLL plugins cannot run on macOS), docking panels, preferences UI, themes switch, .app bundle signing.

## Testing
- `cargo test`: unit tests for config parsing and extension mapping (e.g. `.py` -> python lexer, `.cpp` -> cpp, keywords non-empty, colours parsed to BGR ints).
- `cargo build --release` must pass with no warnings from our Rust code.
- Smoke test: launch the binary with a `.py` file argument, confirm the process stays alive for 3 seconds, and capture a screenshot with `screencapture` to confirm colours.

## Licence
GPL v3, same as Notepad++.
