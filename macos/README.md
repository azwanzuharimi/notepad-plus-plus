# Notepad++ for macOS (unofficial)

An unofficial, modified version of Notepad++ for macOS on Apple Silicon (arm64).
The app is written in Rust. It uses the Scintilla and Lexilla code in this repository
and the language and style files from `PowerEditor/src`.

Not affiliated with the Notepad++ project.

## Changes from Notepad++

2026-10-10 (Paste Special, substyles):

- Edit > Paste Special: Copy Binary Content, Cut Binary Content, and Paste Binary Content, as in Notepad++. They keep all bytes of the selection, also invalid UTF-8 bytes and NUL bytes. The bytes go on the pasteboard in a private type, and the text goes on it too for other apps. If there are no binary bytes on the pasteboard, Paste Binary Content pastes the text up to the first NUL. A macro does not record them, as in Notepad++. There is no Paste HTML Content or Paste RTF Content.
- Cut and Copy change invalid UTF-8 bytes, as in Notepad++ on Windows (Scintilla `UTF16FromUTF8`). A single invalid byte becomes the character with the same number (0x80 becomes U+0080). A broken multibyte sequence can also take the next byte (C3 28 becomes U+00E8). A part of a surrogate pair (for example ED A0 80) becomes U+FFFD, because the macOS pasteboard drops text that contains it. Use the binary items to keep the bytes. Before this change, Cut, Copy, and drag put nothing on the pasteboard, and Cut removed the text.
- Modification of Scintilla: `scintilla/cocoa/ScintillaCocoa.mm` (`CFStringFromSelection`, used by `SetPasteboardData` and the drag source) converts text with invalid UTF-8 bytes with `UTF16FromUTF8`, as `ScintillaWin` does, and replaces unpaired surrogates with U+FFFD.
- Substyles: the "USER KEYWORDS" (substyle1 to substyle8) word lists and colours of `stylers.model.xml` and `langs.model.xml` apply, as in Notepad++. This is for C, C++, Java, C#, RC, ActionScript, Swift, Go, JavaScript, TypeScript, Python, GDScript, Lua, Bash, XML, HTML, PHP, ASP, and JSP. The user keywords of the other keyword classes (for example Perl `carp croak`) are added to the `langs.model.xml` lists, as in Notepad++.

2026-10-10 (Auto-Completion):

- Edit > Auto-Completion: Function Completion (Ctrl+Space), Word Completion (Cmd+Return), Function Parameters Hint (Ctrl+Shift+Space), Previous and Next Hint (Opt+Up, Opt+Down), and Path Completion (Ctrl+Opt+Space). The Space keys use Ctrl, because Cmd+Space is Spotlight.
- The 34 files in `PowerEditor/installer/APIs` are built into the app. The file name is the language name, as in Notepad++. `coffee.xml` is not used, because the language name is `coffeescript` (the same in Notepad++).
- The Notepad++ defaults apply: function and word completion after 1 character, numbers ignored, a parameter hint on `(` and `,`, and no auto-insert of pairs or close tags. There is no Preferences page for these settings yet.
- Path completion lists the files of a Unix path that starts with `/` or `~/`. The path starts at the last `/` or `~/` at the line start or after a space, a quote, or `(`, as Notepad++ does with `C:`. A path can contain spaces, but a `/` right after a space starts a new path.
- Ctrl+Space and Ctrl+Opt+Space are also the macOS shortcuts to change the input source. If you use more than one input source, macOS can take these keys first. Change them in System Settings > Keyboard > Keyboard Shortcuts > Input Sources.
- Function Parameters Previous Hint and Next Hint are disabled when no call tip shows, so Opt+Up and Opt+Down work in text fields. Notepad++ keeps them enabled.
- No completion shows while a macro records, as in Notepad++. The auto-completion commands are not recorded in macros, as in Notepad++.
- The language comes from the Language menu, else from the file name. Dark mode images and colours for the list are not used.

2026-10-10 (multi-select and column mode):

- Edit menu: Multi-select All and Multi-select Next (4 items each), Undo the Latest Added Multi-Select, Skip Current & Go to Next Multi-select, Column Mode..., and Column Editor... (Cmd+Opt+C, Alt+C in Notepad++).
- Editors use the Notepad++ settings: multiple selection, typing in all selections, paste into each selection, and virtual space in rectangular selections.
- Rectangular selection: Option+drag or Option+Shift+arrow keys (Alt in Notepad++). Scintilla on macOS does not support another modifier. Cmd+click adds a caret, and Cmd+drag adds a selection (Ctrl in Notepad++).
- The Column / Multi-Selection Editor has Text to Insert and Number to Insert (initial number, increase, repeat, leading none, zeros, or spaces, and Dec, Hex with a-f or A-F, Oct, Bin). Without a rectangular or multiple selection, it inserts at the caret column on each line from the caret line to the end, as in Notepad++. Each insert is one undo step. A read-only tab does not change.
- Negative numbers wrap to large numbers, as in Notepad++. A negative Repeat inserts the initial number on each line; Notepad++ stops responding in that case without a selection.
- The Column Editor settings are kept until the app quits, not saved. OK is not disabled when the text is empty; it does nothing. An invalid number shows an alert, not a balloon tip.
- Not done: the Notepad++ Delete key fix for multiple carets at line ends, and the "column selection to multi-editing" key handling. There is no Character Panel or Clipboard History.

2026-10-10 (Mark, token styles, Window menu):

- Search > Mark... (Cmd+Shift+M, because Cmd+M minimizes on macOS) opens a Mark panel: Find what, Bookmark line, Purge for each search, Match whole word only, Match case, Wrap around, In selection, Search Mode, Mark All, Clear all marks, and Copy Marked Text. Marks use the "Find Mark Style" colour (indicator 31). There is no Backward direction option.
- Search menu: Style All Occurrences of Token, Style One Token, Clear Style, Jump Up, Jump Down, and Copy Styled Text, with the "Mark Style 1" to "Mark Style 5" colours. Style All uses whole word and no match case, as in Notepad++. Jump Down is Ctrl+1..5 and Ctrl+0 as in Notepad++. Jump Up is Ctrl+Opt (not Ctrl+Shift), because AppKit does not match Shift with a digit.
- Smart highlighting is on, as in Notepad++: when a whole word is selected, its other occurrences in the visible lines get the "Smart Highlighting" colour (whole word, no match case). It runs 50 ms after the last screen update. There is no Highlight matching tags for XML and HTML.
- Window menu: Sort By (10 orders), Windows..., and the open documents (up to 40, as in Notepad++) with a check on the active tab. The Windows dialog has the Name, Path, Type, Size, and Modified time columns. A click on a column header sorts the list, and a second click reverses it. Activate, Save, Close window(s), Sort tabs, and OK work as in Notepad++. Sort tabs without a column click sorts by name first. Cancel in Close window(s) keeps that document and goes on with the next one.
- A macro records the style token commands, as in Notepad++. It does not record Mark..., Sort By, or Windows....
- Type is the language name from the file extension. Modified time is the time of the file on disk.

2026-10-10 (Search menu):

- Search menu in the Notepad++ order: Select and Find Next/Previous, Find (Volatile) Next/Previous, Search Results Window, Next/Previous Search Result, Go to Matching Brace, Select All In-between {} [] or (), Change History, and Bookmark.
- Shortcuts use Cmd for Ctrl (for example Cmd+F2 toggles a bookmark, Cmd+B goes to the matching brace). F2, F3, F4, and F7 are as in Notepad++.
- Bookmarks use the Notepad++ icon in margin 1. A click in that margin toggles a bookmark. The Bookmark submenu has all ten Notepad++ commands. Each line edit is one undo action. Read-only tabs do not change.
- Change history markers show in margin 2 by default, as in Notepad++. Clear Change History also clears the undo history, as in Notepad++. A modified tab stays modified.
- No Incremental Search or Find characters in range yet.

2026-10-10:

- `bundle.sh` makes `target/bundle/Notepad++ for macOS (unofficial).app`: release build, Info.plist, icon, Scintilla cursor images, licence files, and an ad hoc signature. It is not notarised.
- The app shows in Open With for text files and other files, and accepts files dropped on its Dock icon. It does not become the default editor (`LSHandlerRank` is `Alternate`).
- The icon is a neutral placeholder, not the Notepad++ icon.
- Edit menu: Delete, Begin/End Select (also in Column Mode), Insert Date Time (short and long, in the macOS locale format), Copy to Clipboard (full path, file name, folder), Indent, Convert Case to (all 8 items), Line Operations, Blank Operations, and Read-Only on Current Document.
- Line Operations: Duplicate, Remove Duplicate Lines, Remove Consecutive Duplicate Lines, Split, Join, Move Up/Down, Remove Empty Lines (2 items), Insert Blank Line Above/Below, Reverse Line Order, and the Lexicographic, Ignoring Case, Integer, and Decimal (comma and dot) sorts. There is no Randomize, Locale, or Length sort.
- Each line or blank operation is one undo step. It changes the selected lines, or the whole document when there is no selection, as in Notepad++. Invalid UTF-8 bytes stay unchanged.
- Cut and Copy are always on in the editor. Without a selection they cut or copy the current line, as in Notepad++ with its default settings.
- Shortcuts: Cmd+D duplicates the line, Ctrl+Shift+Up/Down moves it, Cmd+Opt+Return and Cmd+Opt+Shift+Return insert a blank line, Cmd+Shift+U is UPPERCASE, Cmd+Opt+U is Sentence case, Cmd+Shift+B is Begin/End Select. Notepad++ shortcuts that are macOS standards (Cmd+U, Cmd+I, Cmd+J) are not used.
- Case conversion maps one character to one character, as Windows does (for example, ß stays ß in UPPERCASE). Text sorts compare UTF-8 bytes, not UTF-16 units.
- Multiple and rectangular selections: Convert Case and the sorts do nothing. The other commands use only the main selection.
- Read-Only on Current Document disables EOL Conversion and the Encoding commands. A read-only tab still reloads when its file changes.
- Language menu: None (Normal Text) and the A to V groups of the compact Notepad++ menu, with the same text and order. A checkmark shows the language of the tab. The status bar shows the language name. There are no User Defined Language items.
- A language set from the Language menu stays when Save As or Rename changes the file name, as in Notepad++. Otherwise, the language is found again from the new name.
- The language comes from the file name too: Makefile, GNUmakefile, CMakeLists.txt, SConstruct, SConscript, wscript, Rakefile, Vagrantfile, crontab, PKGBUILD, and APKBUILD (any case). A name that starts with a dot uses the text after the dot (`.bashrc` is Shell). Notepad++ does not find a language for `Dockerfile`, and neither does this app. The first line of the file (for example `#!/bin/sh`) is not used yet.
- Edit > Comment/Uncomment: Toggle Single Line Comment (Cmd+/), Single Line Comment (Cmd+K), Single Line Uncomment (Shift+Cmd+K), Block Comment (Opt+Cmd+/, because Shift+Cmd+/ is the macOS Help search), and Block Uncomment. The commands use the selection start and end, so a rectangular selection changes each of its lines. The comment tokens come from `langs.model.xml`. Each command is one undo step and does nothing in a read-only tab.
- Sessions: the open files are saved in `session.xml` when the app quits and open again at the next start, as with the Notepad++ option "Remember current session for next launch". The first visible line, the selection, the language (as the Language menu text), a character set encoding, and the read-only flag are kept. A file with a BOM ignores the stored character set, as in Notepad++. Files given on the command line or opened from Finder open after the session and stay active. `-nosession` starts without the session and does not save it.
- File > Load Session... and Save Session... use the Notepad++ `session.xml` format. Sessions from Notepad++ on Windows load; files that do not exist are skipped. The files of the second view open in the same tab bar.
- Recent files: the last 10 closed files show at the end of the File menu, with Restore Recent Closed File (Shift+Cmd+T), Open All Recent Files, and Empty Recent Files List. The list is in the `<History>` element of `config.xml`. `nbMaxFile` and `inSubMenu` in that file change the size and the place of the list. A recent file that does not exist leaves the list, and Notepad++'s "Create it?" question shows.
- The settings folder is `~/Library/Application Support/notepadpp-mac/`. Each file is written to a temporary file first, then renamed. Before each write, the old `session.xml` is copied to `session.xml.inCaseOfCorruption.bak`, and that copy is used when `session.xml` does not load, as in Notepad++. If `config.xml` cannot be read, an alert shows at start and the app does not write `config.xml`.
- Not done: the backup of new and modified tabs (snapshot mode); bookmarks and folds in sessions; the wrapped first line position; customLength (the list always shows the full path).
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
- Macro menu: Start Recording and Stop Recording (Cmd+Shift+R), Playback (Cmd+Shift+P), Save Current Recorded Macro..., Run a Macro Multiple Times..., and the saved macros. A playback is one undo action for each document.
- Reload from Disk is recorded as one command, not as the text of the file. Recording keeps the Scintilla steps and the menu commands that Notepad++ records as type 2 steps (for example New, Save, Close All, Undo, the Line Operations, Convert Case, Blank Operations, Comment/Uncomment, Insert Date Time, bookmarks, Find Next, Word wrap, fold, tab, and EOL commands). Such a command is recorded as one step: the Scintilla steps that it sends are not kept. Notepad++ keeps both. Steps from the Find dialog buttons are not recorded, but Find Next, Replace, and Replace All steps (type 3) from Notepad++ macros play back.
- Run menu: Run... (F5) with the Notepad++ variables (the + button), Save..., and the saved commands. A command runs with `/bin/sh -c` in the folder of the active file (the home folder for a new file). The app does not put variable values in the command text. It gives each value to the shell in an environment variable (`$(FILE_NAME)` becomes `"${NPP_FILE_NAME}"`), so a file name or a selected word cannot run as a command. Windows `%VAR%` expansion is not done; the shell expands `$VAR`. As in Notepad++, `$(CURRENT_LINE)` and `$(CURRENT_COLUMN)` start at 0.
- Macros and commands are saved in `~/Library/Application Support/notepadpp-mac/shortcuts.xml`, in the Notepad++ `<Macros>` and `<UserDefinedCommands>` format. You can copy them from a Notepad++ `shortcuts.xml`. A save keeps the other parts of the file. If the app cannot read the file, it shows an alert and does not change the file.
- The default commands are "Get PHP help" and "Wikipedia Search" with `open`. The default macro "Trim Trailing Space and Save" is the Notepad++ one. The Windows default command is not included. The keys in shortcuts.xml are kept, but they do not work: there is no Shortcut Mapper, no Modify Shortcut/Delete Macro, and no Validate shortcuts.xml.

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
- Replace in Files writes each file back in its detected encoding.

- Search menu: Find, Replace, Find Next, Find Previous, Find in Files, and Go to Line.
- Find and Replace use the Notepad++ Boost regex engine (`boostregex/`), built into Scintilla with `SCI_OWNREGEX`.
- One Find panel holds both the Find and the Replace fields. There is no Mark tab, no search history, and no "In selection" or "Backward direction" option.
- Find in Files searches the text of a tab for a file that is open, also when the tab has unsaved changes. For other files it reads the disk copy. It skips files that contain a NUL byte, but not UTF-16 files with a BOM.
- Replace in Files replaces in the tab for a file that is open, as one undo step, and then saves the tab, as Notepad++ does (the save also writes the other unsaved changes of that tab). It writes other files on disk. A read only tab is not changed, and the status line names it.
- The Search results panel folds: a new search collapses the old search blocks, a double-click on a header line folds or unfolds it, and the fold margin uses the Notepad++ plus and minus markers.
- Search > Incremental Search (Opt+Cmd+I, Ctrl+Alt+I in Notepad++) shows a bar at the bottom of the window, with the selected text as the search text: Find:, < and >, Match case, Highlight all, Count, and the status ("Phrase not found", "Reached end of page, continued from top"). It searches as you type. Return finds the next match, Shift+Return the previous match, and Esc closes the bar. Highlight all uses the "Incremental highlight all" colour (indicator 28). The text field is red when the phrase is not found. Find Next and Find Previous in the menu do not use the bar text.

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

To make the `.app` bundle (needs `jq`; works offline):

```sh
./bundle.sh
open -a "target/bundle/Notepad++ for macOS (unofficial).app" samples/hello.py
```

The app name, the short menu bar name (`CFBundleName`), and the bundle identifier are variables at the top of `bundle.sh`.

## Licence

- The code in `macos/` is licensed under GPL-3.0-or-later, the same as Notepad++.
- Scintilla and Lexilla have their own licence: see `scintilla/License.txt` and `lexilla/License.txt`. This app changes `scintilla/cocoa/ScintillaCocoa.mm` (see Changes from Notepad++).
- The Boost files in `boostregex/boost` are licensed under the Boost Software License 1.0: see `scintilla/test/unit/LICENSE_1_0.txt`.
- The `uchardet` files in `PowerEditor/src/uchardet` have a tri-licence: MPL 1.1, GPL 2.0 or later, or LGPL 2.1 or later (see their file headers). This app uses them under the GPL.
- The Rust crates this app uses are licensed under MIT, Apache-2.0, Zlib, or Unlicense terms.
- The `.app` bundle has these licences and a `README.txt` with the source code link in `Contents/Resources`. Where a Rust crate gives a choice of licences, the app uses it under MIT. `THIRD_PARTY_LICENSES.txt` holds the licence files of each Rust crate. The `objc2` crates do not ship licence files, so `bundle/licenses/objc2` holds a copy from the objc2 repository.
