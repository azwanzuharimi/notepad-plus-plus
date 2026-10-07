# Slice 3 spec: encodings and line endings

Date: 2026-10-08. Issue: #10. References: `PowerEditor/src/Utf8_16.cpp`, `PowerEditor/src/EncodingMapper.cpp`, `PowerEditor/src/uchardet/`, `PowerEditor/src/ScintillaComponent/Buffer.cpp` (FileManager load/save, EOL detection), `PowerEditor/src/menuCmdID.h` (IDM_FORMAT_*), `PowerEditor/src/Notepad_plus.rc` (Encoding menu layout).

## Goal
Open and save files in the same encodings and line endings as Notepad++, with the same detection rules.

## Detection on open (same order as Notepad++)
1. BOM: UTF-8 BOM, UTF-16 LE BOM, UTF-16 BE BOM (`Utf8_16_Read::determineEncoding`).
2. No BOM: Notepad++ rules for UTF-8 without BOM and UTF-16 without BOM (check `Utf8_16_Read` and `Buffer.cpp`).
3. Otherwise: run the vendored `uchardet` (compile it in build.rs) to pick a code page, the same way `FileManager::detectCodepage` does. If it fails, use ANSI.
4. Line endings: detect CRLF, LF or CR from the file content, the same as `Buffer.cpp` (`getEolFormatFromContent`). Set `SCI_SETEOLMODE` to match. New documents use CRLF, which is the Notepad++ default (a preference in #21 can change it later).

## Code page conversion
Use macOS CoreFoundation (`CFStringConvertWindowsCodepageToEncoding`, `CFStringCreateWithBytes`, `CFStringGetBytes`). Add no new crate for this. "ANSI" means Windows-1252 on macOS (Notepad++ uses the system ANSI code page, and macOS has none).

## Menus
1. **Encoding** menu, same layout and names as Notepad++: ANSI, UTF-8, UTF-8-BOM, UTF-16 BE BOM, UTF-16 LE BOM, a Character sets submenu (same groups and code pages as Notepad++), separator, Convert to ANSI, Convert to UTF-8, Convert to UTF-8-BOM, Convert to UTF-16 BE BOM, Convert to UTF-16 LE BOM.
   - "Encode in X" re-reads the file bytes in encoding X. If the tab has unsaved changes, ask first with the Notepad++ message (save first or cancel).
   - "Convert to X" keeps the text and changes the encoding used on save. The tab becomes modified.
   - A checkmark shows the current encoding.
2. **Edit > EOL Conversion**: Windows (CR LF), Unix (LF), Macintosh (CR). Converts the document (`SCI_CONVERTEOLS`) and sets the EOL mode. A checkmark shows the current one.

## Status bar
A one-line status bar at the bottom of the window, with the same fields as Notepad++: language name, length and lines, Ln/Col/Sel (Sel: N | M), EOL type (`Windows (CR LF)` / `Unix (LF)` / `Macintosh (CR)`), encoding name (`UTF-8`, `UTF-8-BOM`, `UTF-16 LE BOM`, `ANSI`, `Windows-1251` ...), INS/OVR. Update it on selection change, tab change and encoding change.

## Save
- Save in the tab's encoding and keep its BOM.
- **Data safety (differs from Notepad++):** if the text has characters that the target code page cannot store, show an alert before writing ("N characters cannot be saved in <encoding>. Save as UTF-8 instead / Save anyway / Cancel"). Never replace characters silently.

## Tests
- Unit tests for: BOM detection, UTF-8 without BOM detection, UTF-16 without BOM if Notepad++ supports it, uchardet on a Windows-1251 and a Shift_JIS sample, EOL detection (CRLF, LF, CR, mixed → Notepad++ rule), round trip load → save byte equality for each encoding, the unmappable-character check.
- `cargo test`, `cargo build --release`, `cargo clippy --release --all-targets` with 0 warnings.
- GUI check with osascript on scratchpad files only (the handshake rule applies).

## Out of scope
Auto-detect preference toggle, per-language default encoding, opening files larger than 2 GB.

## Licence
New files start with `// SPDX-License-Identifier: GPL-3.0-or-later`. Check the uchardet licence (MPL 1.1 / GPL 2+ / LGPL 2.1+ tri-licence in its file headers) and add a line about it to `macos/README.md`.
