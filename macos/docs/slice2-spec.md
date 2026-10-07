# Slice 2 spec: Find, Replace, Find in Files

Date: 2026-10-08. Issue: #9. Reference: `PowerEditor/src/ScintillaComponent/FindReplaceDlg.cpp`.

## Goal
Search behaviour that matches Notepad++, including its regex engine.

## Regex engine
Compile `boostregex/BoostRegExSearch.cxx` and `UTF8DocumentIterator.cxx` (header-only Boost in `boostregex/boost`) into Scintilla with `SCI_OWNREGEX`, the same way `boostregex/nppSpecifics.mak` does. Regex searches then use the same engine and syntax as Notepad++.

## Features
1. **Find/Replace panel** (one NSPanel, not modal; Find tab and Replace tab, or one panel with both fields):
   - Find what, Replace with.
   - Match whole word only, Match case, Wrap around.
   - Search mode: Normal, Extended (`\n \r \t \0 \\ \xNN \uNNNN`, see `FindReplaceDlg::convertExtendedToString`), Regular expression (with ". matches newline" option).
   - Buttons: Find Next, Find Previous, Count, Replace, Replace All, Close.
   - Status line in the panel for results such as "Count: 3 matches" or "Replace All: 5 occurrences were replaced", same wording as Notepad++.
2. **Menu Search** with: Find (Cmd+F), Replace (Cmd+Option+F), Find Next (Cmd+G), Find Previous (Cmd+Shift+G), Find in Files (Cmd+Shift+F), Go to Line (Cmd+L).
3. Selected text fills "Find what" when the panel opens (single line selection only, like Notepad++).
4. Replace All is one undo action.
5. **Find in Files panel**: Find what, Replace with, Filters (default `*.*`, `;`/space separated, `!` exclude as in Notepad++), Directory (with a Browse button), In all sub-folders, In hidden folders, same match options and modes as above. Buttons: Find All, Replace in Files (asks to confirm first), Close.
6. **Search results panel** at the bottom of the main window (a read-only Scintilla view): one header per search, file paths, `Line N: text` rows; matched text highlighted; double-click opens the file in a tab and selects the match. Same format as Notepad++ "Search results".
7. Find in Files runs off the main thread and skips binary files; Replace in Files writes only files that change.

## Out of scope
Mark tab, bookmarks, Find in all open documents, incremental search bar, search history dropdowns, transparency options.

## Testing
- Unit tests (`cargo test`) for: extended-mode conversion, filter parsing (include/exclude), file walk with sub-folder and hidden options, result line formatting.
- Integration test through a hidden Scintilla view if possible: Normal, whole word, match case, Boost regex with back references (`(\w+) \1`), `\r\n` in Extended mode, Replace All as one undo step.
- `cargo build --release` and `cargo clippy --release --all-targets` with no warnings.
- Manual run with osascript on scratchpad files only.

## Licence
New files start with `// SPDX-License-Identifier: GPL-3.0-or-later`. Boost files keep their Boost Software License; add a line about Boost to `macos/README.md`.
