# Notepad++ for macOS (unofficial)

An unofficial, modified version of Notepad++ for macOS on Apple Silicon (arm64).
The app is written in Rust. It uses the Scintilla and Lexilla code in this repository
and the language and style files from `PowerEditor/src`.

Not affiliated with the Notepad++ project.

## Changes from Notepad++

2026-10-10 (Typing helpers):

- Auto-indent (Settings > Preferences > Indentation: None, Basic, Advanced; default Advanced; `<GUIConfig name="MaintainIndent">`). Advanced has the Notepad++ `{` `}` and `if`/`for`/`while`/`else` rules for C-like languages and the `:` rule for Python.
- Brace highlight: the brace at the caret and its match use the "Brace highlight style", an unmatched brace uses "Bad brace colour", and the indent guide of the pair is highlighted.
- Auto-Insert of `()`, `[]`, `{}`, `""`, `''` and the HTML/XML close tag, with the Notepad++ rules: a typed closing character goes over the inserted one, quotes only next to blanks or brackets, void HTML tags get no close tag, and nothing is inserted with more than one selection. `<UserDefinePair open close>` pairs of `config.xml` also work.
- Not done: the three "Matched pair" fields of the Auto-Completion page, auto-indent for external lexers, and the large file limits.

2026-10-10 (Shortcut Mapper):

- Settings > Shortcut Mapper...: tabs Main menu, Macros, Run commands and Scintilla commands, with Name, Shortcut and Category columns, a filter, and Modify, Clear, Delete and Close. Rows with a conflict are red, and the conflict list shows below the table, as in Notepad++. A double click opens Modify. There is no Plugin commands tab.
- Key rule in `shortcuts.xml`: Ctrl means Cmd and Alt means Option, so a Windows `shortcuts.xml` maps to the usual macOS keys. The macOS Control key is the extra attribute `MacControl="yes"`. Notepad++ on Windows ignores it. Key values are Windows virtual keys, as in Notepad++.
- Changes apply at once and go to `shortcuts.xml`: `<InternalCommands>` for menu commands (only changed items, as in Notepad++), the keys of `<Macros>` and `<UserDefinedCommands>`, and `<ScintillaKeys>`. Entries for commands that this app does not have stay in the file. An unreadable file is not changed.
- At start, the app applies `<InternalCommands>` (entries without `nth`), the saved macro and run command keys, and `<ScintillaKeys>` (all editors, also new tabs). With no `<ScintillaKeys>` entries, Scintilla keeps its own macOS keys.
- The Scintilla commands list is the Notepad++ list. Its default keys are the macOS defaults of Scintilla, not the Windows ones. With Shift, a letter or a symbol key goes to Scintilla as the shifted character (Cmd+Shift+K is `K`), because Scintilla on macOS compares that character. When `<ScintillaKeys>` has entries, the Scintilla defaults with Shift (for example Cmd+Shift+L) get this rule too.
- Menu keys with Shift use the shifted character without the Shift flag (Cmd+Shift+S is `S`, Cmd+Shift+= is `+`, Cmd+Shift+7 is `&`, US layout), because AppKit matches that character. F-keys and navigation keys keep the Shift flag. Backspace is the macOS delete key.
- AppKit does not tell numeric keypad keys from the main keys, so Numpad 1 and 1 show as a conflict. Keys of the Window menu are in the conflict check too. Cmd+Tab, Cmd+Shift+Tab, Cmd+Space, and Cmd+Shift+3, 4 and 5 belong to macOS, so OK is disabled for them.
- macOS does not keep a menu key that another menu item already has: the item then has no key, and the mapper shows it empty. Look at the red rows before you choose a key.
- Not done: the Window menu, the Language menu, and the character set items of the Encoding menu are not in the Main menu tab. A Scintilla command with a Notepad++ menu command (for example SCI_ZOOMIN) does not change the menu item key. One menu item has one key, so `nth="1"` entries are kept but not used.

2026-10-10 (Toolbar):

- The window has the Notepad++ toolbar, as a macOS toolbar below the title bar. The buttons and separators are in the Notepad++ order. Each button sends its menu command, so it is enabled, disabled, and checked as the menu item is, and a macro records it as the menu command.
- The icons are the Notepad++ icon files (`PowerEditor/src/icons`, built into the app): Fluent UI and Filled Fluent UI, small and large, and the standard icons. In dark appearance, the dark Fluent icons show; the standard icons are not available in dark mode, as in Notepad++.
- Preferences > Toolbar: Hide, the five icon sets, and the Fluent colorization (Complete or Partial; Red, Green, Blue, Purple, Cyan, Olive, Yellow, Default, System Accent, Custom with a colour well). It is saved in `<GUIConfig name="ToolBar" visible fluentColor fluentCustomColor fluentMono>` as in Notepad++.
- View > Show Toolbar (Cmd+Opt+T) is the macOS item. It changes the Hide setting too.
- A button finds its menu item by action and tag, not by title, so it works with translated menus.
- View > Post-It and Distraction Free Mode hide the toolbar without a change to the Hide setting.
- When the window is narrow, macOS puts the last buttons in the >> menu. Disabled buttons use the macOS dimmed look, not the Notepad++ disabled icons.

2026-10-10 (Edit menu extras):

- Insert > Date Time (customized) uses the Notepad++ format (`<GUIConfig name="insertDateTime" customizedFormat="..." />`, default `yyyy-MM-dd HH:mm:ss`) with the Windows pictures: `d dd ddd dddd M MM MMM MMMM y yy yyyy g h hh H HH m mm s ss t tt` and `'text'`. As in Notepad++, the time pass runs first and the date pass reads its result, so letters in quoted text can change in the second pass. Names come from the macOS locale.
- Preferences > Multi-Instance & Date has the "Customize insert Date Time" group: Reverse default date time order (for Date Time short and long), and the custom format with its result for the Notepad++ example time. The result shows after Return or when the field loses focus. There are no multi-instance or panel state settings.
- Copy to Clipboard > Copy All Filenames and Copy All File Paths: all tabs in tab order, each name followed by CR LF, as in Notepad++. An untitled tab gives its name.
- Line Operations: Randomize Line Order, Sort Lines In Locale Order (macOS locale collation: ignore case, digits as numbers, as the Notepad++ defaults), and Sort Lines By Length (UTF-16 units, as Notepad++). With a rectangular selection, all sorts use the selected columns as the key, as in Notepad++. Columns count bytes; Notepad++ counts UTF-16 units, so lines with non-ASCII text before the columns can sort differently. Sort In Locale Order uses the selected text of each line, as Notepad++ does.
- Paste Special > Paste HTML Content and Paste RTF Content paste the `public.html` or `public.rtf` pasteboard data as text. macOS HTML has no CF_HTML header.
- On Selection: Open File, Open Containing Folder in Finder, Redact Selection (Shift gives ●), Search on Internet (the Search Engine setting; the word is percent-encoded), and Change Search Engine... (opens Preferences on the Search Engine page). A path starting with `~/` uses the home folder; Notepad++ expands `%VAR%` instead.
- Read-Only in Notepad++: Read-Only for All Documents and Clear Read-Only for All Documents. "Read-Only Attribute in macOS" (Notepad++: "in Windows") toggles the owner write permission of the file; the check mark shows it. A file without owner write permission opens read-only, as Notepad++ does with the Windows attribute. The tab stays read-only while the file flag or the user flag is on, and "Read-Only on Current Document" is off while the file flag is on.

2026-10-10 (context menus and View extras):

- A right click in the editor shows the Notepad++ context menu from `contextMenu.xml`: the default file of Notepad++ (`CONTEXTMENU_XML_CONTENT`), or `contextMenu.xml` in the settings folder. Items can use MenuEntryName and MenuItemName, or a command id; FolderName, ItemNameAs, and `id="0"` separators work. An item whose command is not in this app is not shown. Plugin items are not shown. A user file that does not load uses the default. The Scintilla menu is off.
- A right click outside the selection moves the caret first, as in Notepad++.
- Settings > Edit Popup ContextMenu writes the default `contextMenu.xml` when the file is missing, then opens it. A change applies at the next start, as in Notepad++.
- A right click on a tab selects it and shows the Notepad++ tab menu: Close, Close Multiple Tabs, Save, Save As..., Open into (Finder, Terminal, Default Viewer), Rename, Move to Trash, Reload, Read-Only, Copy to Clipboard, and Move Document (Start, End). There is no Pin, Print, other view, new instance, or tab colour. `tabContextMenu.xml` is not read.
- View > Hide Lines (Cmd+Opt+H, Alt+H in Notepad++) hides the selected lines with the Notepad++ markers in the bookmark margin. A click on a marker shows the lines again.
- View > Monitoring (tail -f) makes the tab read-only, checks the file every 250 ms, and reloads it and goes to the end when it changes. Monitoring stops when the file is deleted or renamed. The monitoring state is not saved in the session.
- View > Post-It (F12) hides the tab bar, the status bar, and the window title, and keeps the window on top. The macOS menu bar stays.
- View > Distraction Free Mode uses full screen, Post-It, no panels, and a text column with a margin of a quarter of the screen width on each side.

2026-10-10 (Print):

- File > Print... (Cmd+P) shows the macOS print panel. File > Print Now prints with no panel, with the printer and paper of the last Print.... Each page is drawn by Scintilla `SCI_FORMATRANGEFULL`, as in Notepad++ `Printer.cpp`.
- When text is selected, the panel shows the Selection choice, and it is on at first, as in Notepad++. Print Now prints the whole document.
- Settings > Preferences > Print has the Notepad++ controls: Color Options (`SCI_SETPRINTCOLOURMODE`), Margin Setting in mm, Print line number, Print formfeed as page break, and the Header and Footer parts with font name, size, Bold, Italic and the Variable list with Add. They are stored in `<GUIConfig name="Print" ...>` of `config.xml`.
- Header and footer variables: `$(SHORT_DATE)`, `$(LONG_DATE)`, `$(TIME)` (macOS date and time formats), `$(CURRENT_PRINTING_PAGE)`, and the Run menu variables such as `$(FULL_CURRENT_PATH)` and `$(FILE_NAME)`. As in Notepad++, only the first `$(SHORT_DATE)`, `$(LONG_DATE)`, `$(TIME)` and page variable of a part changes.
- An empty document prints nothing, as in Notepad++.
- Not done: right-to-left header and footer text.

2026-10-10 (session snapshot and backup):

- Session snapshot and periodic backup is on by default, every 7 seconds, as in Notepad++. Each modified tab, and each untitled tab with text, is written to `backup/<name>@<YYYY-MM-DD_HHMMSS>` in the settings folder, in the tab encoding and line ends. Only text that changed after the last backup is written. Each write goes to a temporary file first, so a failed write keeps the old backup. The timer also runs while a dialog shows. Then `session.xml` is written with the Notepad++ `backupFilePath` and `originalFileLastModifTimestamp` attributes.
- When snapshot mode is on, Quit does not ask to save. Modified and untitled tabs open again at the next start as modified tabs, with the file path kept. A file that Finder opens at start gets its backup text too. The empty "new 1" tab closes when the session restores tabs. If a backup file is missing at quit, or the last write failed, Notepad++'s "Your backup file cannot be found" question shows. When the option is off, or with `-nosession`, Quit asks for each modified tab as before.
- At start, if a file changed on disk after its backup, Notepad++'s "This file has been modified by another program. Do you want to reload it and lose the changes made in Notepad++?" question shows. No is the default.
- A backup is deleted when its tab is saved, or closed without a save. The app never deletes other files in the backup folder. A backup that does not load stays. An unreadable `session.xml` is moved to `session.xml.unreadable`.
- In snapshot mode, if the app panics on the main thread, it writes the backups and `session.xml` before it stops. A panic on another thread writes nothing; the last periodic backup is at most 7 seconds old.
- Backup on save: None (default), Simple (`<file>.bak` next to the file), or Verbose (`nppBackup/<file>.<YYYY-MM-DD_HHMMSS>.bak`), with an optional custom folder, as in Notepad++. Settings > Preferences > Backup has the Notepad++ controls: Remember current session for next launch, Enable session snapshot and periodic backup with the seconds, the backup path, Backup on save, and Custom Backup Directory. They are stored in `<GUIConfig name="Backup" ...>` and `RememberLastSession` of `config.xml`. There is no "Remember inaccessible files from past session" option.
- A backup of a tab whose text the encoding cannot hold is written as UTF-8 with a BOM. A character set tab keeps its character set after a restore, so Save shows the "characters cannot be saved" question. For ANSI and UTF-16 tabs an alert tells you once that the tab opens as UTF-8 with BOM after a restore.
- Not done: a file deleted after its backup opens with its path, not as an untitled tab. Environment variables in the custom backup folder are not expanded.


2026-10-10 (Localization):

- Settings > Preferences > General > Localization lists the 94 Notepad++ translations (`PowerEditor/installer/nativeLang`, built into the app) by their native names, in file name order, as in Notepad++. A choice copies the file to `nativeLang.xml` in the settings folder and changes the menus and open dialogs at once. At start the app reads `nativeLang.xml`; without it, or with `english.xml`, the app stays in English, as in Notepad++.
- Menus: the menu bar entries (`<Entries>`), submenus (`<SubEntries>`), and commands (`<Commands>`) by the menuCmdID.h id of the macro command table, else by the English name in `english.xml`. The `&` accelerators and the text after a TAB are removed. The macOS key equivalents stay. Menu items that the app adds later (recent files, UDL, Window list) are translated too. The tab context menu uses `<TabBar>`.
- Dialogs: the title and the labels, buttons, and check boxes of a dialog whose title is a `<Dialog>` title in `english.xml` (for example Find, Find in Files, and Mark), matched by the English text, when the dialog becomes the key window. Message boxes: an alert text that is a `<MessageBox>` title or message of `english.xml` shows the translation, with `$STR_REPLACE$` and `$INT_REPLACE$` filled in.
- Arabic, Farsi, Hebrew, Kurdish, Urdu, and Uyghur (`RTL="yes"`) set the right-to-left layout on the menus. The editor and the dialogs stay left-to-right.
- Not done: a change to English restores the app's own English text, not the `english.xml` names. Items, labels, and alerts whose English text is not in `english.xml` stay English (for example Move to Trash, Find Previous, and most alert texts of this app). Alert buttons, popup lists, status texts (`<MiscStrings>`), the Preferences page list, and `<ComboBox>` lists are not translated. Translated button text can be cut off.


2026-10-10 (Preferences):

- Settings > Preferences... (Cmd+,) opens the Notepad++ Preferences window: a list of pages on the left and the page on the right. The pages are Toolbar, Editing 1, Editing 2, Margins/Border/Edge, New Document, Default Directory, Recent Files History, Indentation, Highlighting, Searching, Auto-Completion, Cloud & Link, Search Engine, and MISC., with the Notepad++ labels and defaults.
- A change applies at once (a slider when you release it) and is saved in the `<GUIConfigs>` element of `config.xml`, in the Notepad++ `<GUIConfig name="...">` format. The values of a Notepad++ `config.xml` from Windows are read. The other elements and attributes of the file stay as they are.
- Applied now: current line indicator (frame width), caret width and blink rate, line wrap indent, smooth font, virtual space, Copy/Cut line without selection, scrolling beyond the last line, multi-editing, fold margin style, vertical edge columns (background mode), change history margin and text, line number margin (dynamic or constant width), padding, bookmark margin, the EOL, encoding, and language of new documents, "Apply to opened ANSI files", the Open and Save folder, the recent files options (with "Only File Name" and the customized length), tab size, tabs or spaces, and Backspace unindent (also for each language), and "Fill Find Field with Selected Text" (with the maximum length and the word under the caret).
- The tab settings of a language are saved in `langs.xml` (the `tabSettings` and `backspaceUnindent` attributes of its `<Language>` element), as in Notepad++. When there is no `langs.xml`, the app first copies `langs.model.xml`, as Notepad++ does.
- Highlighting: Smart Highlighting (Enable, Match case, Match whole word only) and the Style All Occurrences of Token options apply. Auto-Completion: the enable choice, the completion kind, "From Nth character", Ignore numbers, and the parameter hint apply.
- Saved for other features, not used yet: Highlight Matching Tags, "Use Find dialog settings" and "Highlight another view" of Smart Highlighting, Insert Selection (TAB, ENTER), the brief list, Auto-Insert, Clickable Link, Search Engine, File Status Auto-Detection, and "Enable Column Selection to Multi-Editing".
- `RememberLastSession` and `addNewDocumentOnStartup` of `config.xml` are used at start and at quit. The Backup page sets them.
- Not done: the General page (it has only Localization), the Tab Bar, Dark Mode (macOS uses the system appearance), File Association, Language, Multi-Instance & Date, Delimiter, and Performance pages; the user defined auto-insert pairs; the Indentation auto-indent choice.

2026-10-10 (User Defined Languages):

- The Notepad++ UDL lexer (`lexilla/lexers/LexUser.cxx`) is built without changes. It includes `windows.h` only for `_itoa`. The small header `src/shim/windows.h` gives that function. Before, an empty lexer took its place.
- UDL files load from `userDefineLang.xml` and `userDefineLangs/*.xml` in the settings folder, as in Notepad++. The UDL 2.1, 2.0 and older formats load. At the first start, the app puts the two Markdown UDLs of the Notepad++ installer in `userDefineLangs`.
- Language menu: the User Defined Language submenu (Define your language..., Open User Defined Language folder..., Notepad++ User Defined Languages Collection), the UDL names, and User-Defined, at the bottom as in Notepad++.
- A file whose extension is in the `ext` list of a UDL uses that UDL before the built-in languages, as in Notepad++. In dark mode, a UDL with `darkModeTheme="yes"` comes first (for example "Markdown (preinstalled dark mode)"), else a light one. The status bar shows "User Defined language file - name". The session keeps the UDL name. Style Configurator and theme changes keep the UDL of a tab.
- The lexer properties, keyword lists, and styles are set as `ScintillaEditView::setUserLexer` sets them.
- Define your language... opens the User Defined Language dialog: the language list, Create new..., Save as..., Rename, Remove, Import..., Export..., Ignore case, Ext., and all four tabs (Folder & Default, Keywords Lists, Comment & Number, Operators & Delimiters). Each Styler button opens the Styler Dialog (font, size, bold, italic, underline, colours, transparent, nesting). Changes apply to open UDL tabs at once. The UDL file is written 1 second after the last change, when the dialog closes, and at quit.
- Import... copies the file into the `userDefineLangs` folder. Notepad++ adds the imported UDLs to `userDefineLang.xml`.
- Not done: Dock, Transparency, live preview in the Styler Dialog (changes apply on OK), the theme colours for a new UDL (`startAtTheme`), comment commands and auto-completion for UDL tabs, and the font list check (`isInFontList`).
- Quoted keywords with non-ASCII characters work. Notepad++ on Windows drops these characters, because it reads them as signed `char`.

2026-10-10 (Style Configurator, themes):

- Settings > Style Configurator...: Select theme, Language (Global Styles first), Style, Foreground and Background colour, Font name (the macOS fonts), Font size, Bold, Italic, Underline, Default ext. and User ext., Default and User-defined keywords, and the 7 Global override check boxes. Changes show at once in all open editors. Cancel and the close button go back to the styles from before. There is no Apply button and no Transparency, as in Notepad++. There is no "Go to settings" link.
- The 22 Notepad++ themes are in the app. User themes are `.xml` files in `themes/` in the settings folder; a user theme replaces an installed theme with the same name. Settings > Import > Import style theme(s)... copies files into that folder.
- Save & Close writes the styles to the file of the theme: `stylers.xml` for "Default (stylers.xml)", else the theme file. Changes to an installed theme go to a copy in `themes/`, as in Notepad++. A file that exists but does not load is not replaced. When `stylers.xml` does not exist, the app uses `stylers.model.xml`.
- A theme that is older than `stylers.model.xml` gets the missing styles from the model, with the Default Style colours, as in Notepad++.
- The theme choice is in `config.xml`, in `<GUIConfig name="DarkMode" lightThemeName="..." darkThemeName="..." />`, as in Notepad++ 8.x. The Global override check boxes are in `<GUIConfig name="globalOverride" ... />`.
- Dark mode: macOS has no Notepad++ dark mode switch. When the macOS appearance is dark at start, the app uses `darkThemeName`; its default is DarkModeDefault, as in Notepad++ with dark mode on. A theme that you select in dark appearance is stored in `darkThemeName`, so your choice wins. A change of the macOS appearance shows after a restart.
- User ext. comes before the language extensions.
- Not done: the "colorStyle" transparency attribute; `addDefaultStyles` (the app has its own fallback colours).
2026-10-10 (panels):

- View > Document List and View > Function List show docked panels with a title and a close button. A checkmark shows an open panel.
- Each panel opens on its Notepad++ default side: Document List and Folder as Workspace on the left; Function List, Document Map, Clipboard History and Character Panel on the right. Panels on one side share it as tabs, with a tab strip below them. The panels cannot move to the other side. They keep their width when the window changes size. Which panels are open is not saved when the app quits.
- View > Document Map shows the current document at the smallest zoom in a second view of the same document. An orange zone marks the lines that the editor shows. A click or a drag on the map scrolls the editor. The map follows tab switches, edits, scrolls, folds and word wrap. The map never takes the keyboard focus. A closed map keeps no document.
- File > Open Folder as Workspace..., File > Open Containing Folder > Folder as Workspace and View > Folder as Workspace show a tree of root folders. Folders load when they unfold. Folders come first, sorted without case. Hidden folders do not show. A double click opens a file. The context menu has Add, Remove All, Remove, Copy path, Copy file name, Find in Files..., Finder here, Terminal here, Run by system and Open. The tree reads its unfolded folders again (at most 200) when the app becomes active and when the panel opens; there is no file system watcher. The roots, the unfolded folders and the selected item are saved in the FileBrowser element of config.xml when config.xml is saved, as Notepad++ does, and come back at the first View > Folder as Workspace. There is no toolbar (Unfold all, Fold all, Locate current file).
- Edit > Clipboard History lists the texts copied to the clipboard while the panel exists, newest first, with no duplicates. macOS has no clipboard event, so the panel reads the clipboard change count twice a second while the panel shows and the app is active. It keeps the last 100 texts, and a text longer than 1,048,576 UTF-16 units is not kept; Notepad++ keeps all texts of any size. A double click inserts the text.
- Edit > Character Panel lists the 256 characters of the code page of the document (Windows-1252 for ANSI and Unicode documents) with Value, Hex, Character, HTML Name, HTML Decimal and HTML Hexadecimal. A double click on the Character column inserts the character as Unicode; a double click on an other column inserts its text. The Return key does not insert.
- Function List uses the Notepad++ parser files (`PowerEditor/installer/functionList`, built into the app) and `overrideMap.xml`, with the same Boost regex engine as Find. All 44 Notepad++ function list unit tests (`PowerEditor/Test/FunctionList`, without the UDL tests) give the expected result.
- Function List has a search box, a Sort button, and a Reload button. Sort and search text stay for each file. A double click on a function puts its line in the middle of the editor. The function at the caret is selected. The list loads again on tab switch, save, and language change, as in Notepad++. There is no Return key action, no Preferences menu, and no "Sort functions (A to Z) by default" option.
- Document List shows Name and Ext. columns and an icon for a modified or read-only tab. A click on a row switches to the tab. There is no context menu, no Path column, and no sort by column.
- Function List does not parse a file larger than 10 MB and shows "File too large for Function List". This limit is only in this app: Notepad++ parses any size, and the parse blocks the window. Notepad++ uses Normal Text, so no Function List, only for files of 200 MB or more.
- User Defined Language parsers (KRL, NppExec, Sinumerik, UniVerse BASIC) are not used yet.

2026-10-10 (Paste Special, substyles):

- Edit > Paste Special: Copy Binary Content, Cut Binary Content, and Paste Binary Content, as in Notepad++. They keep all bytes of the selection, also invalid UTF-8 bytes and NUL bytes. The bytes go on the pasteboard in a private type, and the text goes on it too for other apps. If there are no binary bytes on the pasteboard, Paste Binary Content pastes the text up to the first NUL. A macro does not record them, as in Notepad++.
- Cut and Copy change invalid UTF-8 bytes, as in Notepad++ on Windows (Scintilla `UTF16FromUTF8`). A single invalid byte becomes the character with the same number (0x80 becomes U+0080). A broken multibyte sequence can also take the next byte (C3 28 becomes U+00E8). A part of a surrogate pair (for example ED A0 80) becomes U+FFFD, because the macOS pasteboard drops text that contains it. Use the binary items to keep the bytes. Before this change, Cut, Copy, and drag put nothing on the pasteboard, and Cut removed the text.
- Modification of Scintilla: `scintilla/cocoa/ScintillaCocoa.mm` (`CFStringFromSelection`, used by `SetPasteboardData` and the drag source) converts text with invalid UTF-8 bytes with `UTF16FromUTF8`, as `ScintillaWin` does, and replaces unpaired surrogates with U+FFFD.
- Substyles: the "USER KEYWORDS" (substyle1 to substyle8) word lists and colours of `stylers.model.xml` and `langs.model.xml` apply, as in Notepad++. This is for C, C++, Java, C#, RC, ActionScript, Swift, Go, JavaScript, TypeScript, Python, GDScript, Lua, Bash, XML, HTML, PHP, ASP, and JSP. The user keywords of the other keyword classes (for example Perl `carp croak`) are added to the `langs.model.xml` lists, as in Notepad++.

2026-10-10 (Auto-Completion):

- Edit > Auto-Completion: Function Completion (Ctrl+Space), Word Completion (Cmd+Return), Function Parameters Hint (Ctrl+Shift+Space), Previous and Next Hint (Opt+Up, Opt+Down), and Path Completion (Ctrl+Opt+Space). The Space keys use Ctrl, because Cmd+Space is Spotlight.
- The 34 files in `PowerEditor/installer/APIs` are built into the app. The file name is the language name, as in Notepad++. `coffee.xml` is not used, because the language name is `coffeescript` (the same in Notepad++).
- The Notepad++ defaults apply: function and word completion after 1 character, numbers ignored, a parameter hint on `(` and `,`, and no auto-insert of pairs or close tags. Preferences > Auto-Completion changes them.
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
- Smart highlighting is on, as in Notepad++: when a whole word is selected, its other occurrences in the visible lines get the "Smart Highlighting" colour (whole word, no match case; Preferences > Highlighting changes this). It runs 50 ms after the last screen update. There is no Highlight matching tags for XML and HTML.
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
- Language menu: None (Normal Text) and the A to V groups of the compact Notepad++ menu, with the same text and order. A checkmark shows the language of the tab. The status bar shows the language name.
- A language set from the Language menu stays when Save As or Rename changes the file name, as in Notepad++. Otherwise, the language is found again from the new name.
- The language comes from the file name too: Makefile, GNUmakefile, CMakeLists.txt, SConstruct, SConscript, wscript, Rakefile, Vagrantfile, crontab, PKGBUILD, and APKBUILD (any case). A name that starts with a dot uses the text after the dot (`.bashrc` is Shell). Notepad++ does not find a language for `Dockerfile`, and neither does this app. The first line of the file (for example `#!/bin/sh`) is not used yet.
- Edit > Comment/Uncomment: Toggle Single Line Comment (Cmd+/), Single Line Comment (Cmd+K), Single Line Uncomment (Shift+Cmd+K), Block Comment (Opt+Cmd+/, because Shift+Cmd+/ is the macOS Help search), and Block Uncomment. The commands use the selection start and end, so a rectangular selection changes each of its lines. The comment tokens come from `langs.model.xml`. Each command is one undo step and does nothing in a read-only tab.
- Sessions: the open files are saved in `session.xml` when the app quits and open again at the next start, as with the Notepad++ option "Remember current session for next launch". The first visible line, the selection, the language (as the Language menu text), a character set encoding, and the read-only flag are kept. A file with a BOM ignores the stored character set, as in Notepad++. Files given on the command line or opened from Finder open after the session and stay active. `-nosession` starts without the session and does not save it.
- File > Load Session... and Save Session... use the Notepad++ `session.xml` format. Sessions from Notepad++ on Windows load; files that do not exist are skipped. The `subView` files open in the second view, and a file in both views opens as a clone.
- Recent files: the last 10 closed files show at the end of the File menu, with Restore Recent Closed File (Shift+Cmd+T), Open All Recent Files, and Empty Recent Files List. The list is in the `<History>` element of `config.xml`. `nbMaxFile` and `inSubMenu` in that file change the size and the place of the list. A recent file that does not exist leaves the list, and Notepad++'s "Create it?" question shows.
- The settings folder is `~/Library/Application Support/notepadpp-mac/`. Each file is written to a temporary file first, then renamed. Before each write, the old `session.xml` is copied to `session.xml.inCaseOfCorruption.bak`, and that copy is used when `session.xml` does not load, as in Notepad++. If `config.xml` cannot be read, an alert shows at start and the app does not write `config.xml`.
- Not done: bookmarks and folds in sessions; the wrapped first line position.
- Tools menu: MD5, SHA-1, SHA-256, and SHA-512, each with Generate..., Generate from files..., and Generate from selection into clipboard. The hashes come from macOS CommonCrypto.
- As in Notepad++, the selection and the Generate... text stop at the first NUL byte. The selection hash uses the UTF-8 bytes of the tab, also when the file encoding is different.
- In Generate..., the text box uses LF line ends. A hash of more than one line is different from Notepad++ on Windows (CR LF). Generate from files... skips a file larger than 4 GiB.
- ? menu: Command Line Arguments, the four Notepad++ links, Debug Info, and About. About is also in the app menu. There is no Update Notepad++ or Set Updater Proxy.
- File menu: Open Containing Folder (Finder, Terminal), Open in Default Viewer, Reload from Disk, Save a Copy As, Save All, Rename, Close All, Close Multiple Documents, and Move to Trash ("Move to Recycle Bin" in Notepad++).
- One file opens in one tab only. Paths are compared after symbolic links and `..` are resolved. Save As, Save a Copy As, and Rename refuse a file that is open in another tab, as in Notepad++.
- Rename also refuses a file that is open in another tab. Notepad++ does not check this.
- Close All and the Close Multiple Documents items ask about each modified tab, like Close. Cancel stops the operation, and no tab closes.
- Save All has no "Always yes" button, because there are no Preferences yet. Close All but Pinned Documents and Folder as Workspace are not in the menu.
- File Status Auto-Detection, as in Notepad++: when the app becomes active, the current tab (or all tabs) is checked against the file on disk. A tab switch also checks the new tab. A changed file shows the "Reload" question, or reloads at once when the tab has no changes and "Update silently" is on. A deleted file shows "Keep non existing file"; No closes the tab. No to a reload keeps the text, marks the tab modified, and does not ask again until the next change. There is no check while Replace in Files runs or while another dialog is open.
- The setting is File Status Auto-Detection in Preferences > MISC. (`Auto-detection` in `config.xml`). A tab under View > Monitoring is not checked, because Monitoring reloads it. Not done: changes of the file read-only attribute.
- View menu: Always on Top, Toggle Full Screen Mode (Ctrl+Cmd+F), Show Symbol (7 items), Zoom (Cmd+=, Cmd+-, Cmd+0), Tab (Cmd+1..9, Cmd+Shift+] and [, move tab), Word wrap, fold commands, and Summary. The options apply to all tabs and to new tabs. They are not saved when the app quits.
- Two views, as in Notepad++: View > Move/Clone Current Document > Move to Other View and Clone to Other View, Focus on Another View (F8), Synchronize Vertical Scrolling and Synchronize Horizontal Scrolling. The second view shows at the right when a document goes there, and hides when its last tab closes. A clone shows the same document, so an edit, a save, and the modified state apply to both tabs. A cloned document closes without a question while the other tab stays open. Rotate to Right and Rotate to Left change the split to top and bottom; Notepad++ has these items on a right click of the splitter. Tab 1..9, Next and Previous Tab, the move tab items, and Close All to the Left or Right work in the active view. In snapshot mode, a cloned document has one backup file, and a closed clone gives its backup to the tab that stays open. Not done: the tab drag between views, Move to New Instance and Open in New Instance, Zoom > Synchronize Across Views (the zoom is the same for all tabs), and the split position in the session.
- Code folding: a fold margin with the Notepad++ box markers, the "Fold" and "Fold margin" colours, and the Notepad++ fold properties for each language. Click a fold symbol to fold or unfold. Fold All is Opt+Cmd+0, Fold Level N is Opt+Cmd+N, Fold Current Level is Ctrl+Opt+F. The Unfold items add Ctrl (Unfold Current Level adds Shift), because AppKit does not match Shift with a digit. Unfold Level 8 has no shortcut, because Ctrl+Opt+Cmd+8 is the macOS Invert colors shortcut.
- Fold margin clicks: a click folds or unfolds the block, Shift+click unfolds it with all its children, and Cmd+click folds or unfolds it with all its children. Unlike Notepad++, Shift+Cmd+click folds or unfolds all blocks (Scintilla automatic fold).
- Tab width is 4 with tabs, or the `tabSettings` of the language (Python and YAML use 4 spaces).
- The Search results panel also gets a fold margin, as in Notepad++.
- No Text Direction RTL or LTR: Scintilla on macOS stores `SC_BIDIRECTIONAL_R2L` but does not draw right to left. No Post-It, Distraction Free Mode, View Current File in, Hide Lines, or Monitoring.
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
