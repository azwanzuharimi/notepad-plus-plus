# Notepad++ for macOS (unofficial)

An unofficial, modified version of Notepad++ for macOS on Apple Silicon (arm64).
The app is written in Rust. It uses the Scintilla and Lexilla code in this repository
and the language and style files from `PowerEditor/src`.

Not affiliated with the Notepad++ project.

## Changes from Notepad++

2026-10-10 (Search menu):

- Search menu in the Notepad++ order: Select and Find Next/Previous, Find (Volatile) Next/Previous, Search Results Window, Next/Previous Search Result, Go to Matching Brace, Select All In-between {} [] or (), Change History, and Bookmark.
- Shortcuts use Cmd for Ctrl (for example Cmd+F2 toggles a bookmark, Cmd+B goes to the matching brace). F2, F3, F4, and F7 are as in Notepad++.
- Bookmarks use the Notepad++ icon in margin 1. A click in that margin toggles a bookmark. The Bookmark submenu has all ten Notepad++ commands. Each line edit is one undo action. Read-only tabs do not change.
- Change history markers show in margin 2 by default, as in Notepad++. Clear Change History also clears the undo history, as in Notepad++. A modified tab stays modified.
- No Incremental Search, Mark, token styles, or Find characters in range yet.

2026-10-10:

- Tools menu: MD5, SHA-1, SHA-256, and SHA-512, each with Generate..., Generate from files..., and Generate from selection into clipboard. The hashes come from macOS CommonCrypto.
- As in Notepad++, the selection and the Generate... text stop at the first NUL byte. The selection hash uses the UTF-8 bytes of the tab, also when the file encoding is different.
- In Generate..., the text box uses LF line ends. A hash of more than one line is different from Notepad++ on Windows (CR LF). Generate from files... skips a file larger than 4 GiB.
- ? menu: Command Line Arguments, the four Notepad++ links, Debug Info, and About. About is also in the app menu. There is no Update Notepad++ or Set Updater Proxy.
- File menu: Open Containing Folder (Finder, Terminal), Open in Default Viewer, Reload from Disk, Save a Copy As, Save All, Rename, Close All, Close Multiple Documents, and Move to Trash ("Move to Recycle Bin" in Notepad++).
- One file opens in one tab only. Paths are compared after symbolic links and `..` are resolved. Save As, Save a Copy As, and Rename refuse a file that is open in another tab, as in Notepad++.
- Rename also refuses a file that is open in another tab. Notepad++ does not check this.
- Close All and the Close Multiple Documents items ask about each modified tab, like Close. Cancel stops the operation, and no tab closes.
- Save All has no "Always yes" button, because there are no Preferences yet. Close All but Pinned Documents and Folder as Workspace are not in the menu.
- View menu: Always on Top, Toggle Full Screen Mode (Ctrl+Cmd+F), Show Symbol (7 items), Zoom (Cmd+=, Cmd+-, Cmd+0), Tab (Cmd+1..9, Cmd+Shift+] and [, move tab), Word wrap, fold commands, and Summary. The options apply to all tabs and to new tabs. They are not saved when the app quits.
- Code folding: a fold margin with the Notepad++ box markers, the "Fold" and "Fold margin" colours, and the Notepad++ fold properties for each language. Click a fold symbol to fold or unfold. Fold All is Opt+Cmd+0, Fold Level N is Opt+Cmd+N, Fold Current Level is Ctrl+Opt+F. The Unfold items add Ctrl (Unfold Current Level adds Shift), because AppKit does not match Shift with a digit. Unfold Level 8 has no shortcut, because Ctrl+Opt+Cmd+8 is the macOS Invert colors shortcut.
- Fold margin clicks: a click folds or unfolds the block, Shift+click unfolds it with all its children, and Cmd+click folds or unfolds it with all its children. Unlike Notepad++, Shift+Cmd+click folds or unfolds all blocks (Scintilla automatic fold).
- Tab width is 4 with tabs, or the `tabSettings` of the language (Python and YAML use 4 spaces).
- The Search results panel also gets a fold margin, as in Notepad++.
- No Text Direction RTL or LTR: Scintilla on macOS stores `SC_BIDIRECTIONAL_R2L` but does not draw right to left. No Post-It, Distraction Free Mode, View Current File in, Hide Lines, Synchronize scrolling, or Monitoring.
- Summary counts characters in UTF-8 for all encodings. Notepad++ uses a byte count for ANSI and UTF-16 files. Pinch zoom changes only the current tab.

2026-10-08:

- Encoding menu: ANSI, UTF-8, UTF-8-BOM, UTF-16 BE BOM, UTF-16 LE BOM, Character sets, and the Convert to items. A checkmark shows the current encoding.
- Edit > EOL Conversion: Windows (CR LF), Unix (LF), and Macintosh (CR).
- Files open with the Notepad++ detection order: BOM, then `uchardet` (built from `PowerEditor/src/uchardet`), then the UTF-8 and UTF-16 rules. The first line end sets the EOL mode. New documents are UTF-8 with CR LF.
- A status bar shows the language, length and lines, Ln, Col, Pos or Sel, the EOL type, the encoding, and INS or OVR.
- Code page conversion uses macOS CoreFoundation. "ANSI" is Windows-1252, because macOS has no system ANSI code page.
- Save keeps the encoding and the BOM of the tab. If the encoding cannot store some characters, an alert shows before the write: Save as UTF-8 instead, Save anyway (writes `?`), or Cancel. Notepad++ replaces such characters silently.
- "Encode in" a character set reads the file bytes from disk again in that encoding. This also occurs when the current encoding is a character set. If the tab has unsaved changes, the Notepad++ "Save Current Modification" question shows first. Between UTF-8, UTF-8-BOM, and the UTF-16 items, only the save encoding changes, as in Notepad++. To or from ANSI, the same bytes are read again in the new encoding, as in Notepad++.
- If some bytes of a file cannot be read in its encoding (for example a broken UTF-16 file), the save alert shows before the next save. Replace in Files skips such files and names them.
- In a single byte character set, a byte with no character opens as U+00XX, or as U+F7XX when a real byte already gives U+00XX. These bytes save back unchanged.
- OEM 720 is in the menu but disabled, because CoreFoundation does not support it. OEM 858 uses the OEM 850 table with the euro sign at 0xD5.
- UTF-16 LE without a BOM uses a simple test (more than half of the high bytes are zero) in place of the Windows `IsTextUnicode` function.
- The status bar shows only the single selection forms (Pos and Sel: N | M), not the rectangular or multiple selection forms.
- Find in Files and Replace in Files read each file in the encoding of its open tab, or else in its detected encoding. Replace in Files writes it back in the same encoding. Open tabs reload in their own encoding.

- Search menu: Find, Replace, Find Next, Find Previous, Find in Files, and Go to Line.
- Find and Replace use the Notepad++ Boost regex engine (`boostregex/`), built into Scintilla with `SCI_OWNREGEX`.
- One Find panel holds both the Find and the Replace fields. There is no Mark tab, no search history, and no "In selection" or "Backward direction" option.
- Find in Files reads files from disk, also when a file is open in a tab with unsaved changes. It skips files that contain a NUL byte, but not UTF-16 files with a BOM.
- Replace in Files writes to disk. It skips files that are open in a tab with unsaved changes, and names them in the status line. Open tabs of changed files reload.
- The Search results panel does not fold old searches.

2026-10-07:

- New native macOS app in Rust (AppKit through the objc2 crates). It replaces the Win32 user interface.
- Tabs, New, Open, Save, Save As, Close, and a prompt before a modified tab closes.
- Syntax colours from `langs.model.xml` and `stylers.model.xml` (default theme only).
- UTF-8 files only (changed in a later slice). No Find/Replace, sessions, macros, plugins, or preferences yet.
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
- The Boost files in `boostregex/boost` are licensed under the Boost Software License 1.0: see `scintilla/test/unit/LICENSE_1_0.txt`.
- The `uchardet` files in `PowerEditor/src/uchardet` have a tri-licence: MPL 1.1, GPL 2.0 or later, or LGPL 2.1 or later (see their file headers). This app uses them under the GPL.
- The Rust crates this app uses are licensed under MIT, Apache-2.0, Zlib, or Unlicense terms.
