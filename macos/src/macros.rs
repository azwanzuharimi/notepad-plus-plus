// SPDX-License-Identifier: GPL-3.0-or-later
use crate::panel::{self, Form};
use crate::search::{self, Mode, Next, Opts};
use crate::shortcuts::{self, Macro, Shortcuts, Step, TYPE_MENU, TYPE_S, TYPE_SNR};
use crate::{ns, sci, App};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSButton, NSMenu,
    NSMenuDidSendActionNotification, NSMenuItem, NSMenuWillSendActionNotification, NSPopUpButton,
    NSTextField, NSView,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};

pub const SCN_MACRORECORD: u32 = 2009;
const SCI_GETCURRENTPOS: u32 = 2008;
const SCI_BEGINUNDOACTION: u32 = 2078;
const SCI_ENDUNDOACTION: u32 = 2079;
const SCI_GETLINECOUNT: u32 = 2154;
const SCI_STARTRECORD: u32 = 3001;
const SCI_STOPRECORD: u32 = 3002;
const FIXED_ITEMS: isize = 5;

const IDC_FRCOMMAND_INIT: i32 = 1700;
const IDC_FRCOMMAND_EXEC: i32 = 1701;
const IDC_FRCOMMAND_BOOLEANS: i32 = 1702;
const IDFINDWHAT: i32 = 1601;
const IDREPLACEWITH: i32 = 1602;
const IDNORMAL: i32 = 1625;
const IDOK: isize = 1;
const IDREPLACE: isize = 1608;
const IDREPLACEALL: isize = 1609;
const IDC_FINDPREV: isize = 1721;
const IDC_FINDNEXT: isize = 1723;
const IDF_WHOLEWORD: isize = 1;
const IDF_MATCHCASE: isize = 2;
const IDF_WRAP: isize = 256;
const IDF_WHICH_DIRECTION: isize = 512;
const IDF_REDOTMATCHNL: isize = 1024;

// Paste calls Scintilla directly on macOS, so Scintilla does not record it; Notepad++ records SCI_PASTE.
const SCI_ACTIONS: [(&str, i32); 1] = [("paste:", 2179)];
// Commands that Notepad++ does not record as type 2 steps; they still play back.
const NOT_RECORDED: [&str; 118] = [
    "IDM_FILE_PRINT",
    "IDM_FILE_PRINTNOW",
    "IDM_EDIT_CUT",
    "IDM_EDIT_COPY",
    "IDM_EDIT_PASTE",
    "IDM_EDIT_LINE_UP",
    "IDM_EDIT_LINE_DOWN",
    "IDM_EDIT_STREAM_UNCOMMENT",
    "IDM_VIEW_TAB_START",
    "IDM_VIEW_TAB_END",
    "IDM_FOCUS_ON_FOUND_RESULTS",
    "IDM_SEARCH_FINDINCREMENT",
    "IDM_SEARCH_GOTONEXTFOUND",
    "IDM_SEARCH_GOTOPREVFOUND",
    "IDM_SEARCH_CHANGED_NEXT",
    "IDM_SEARCH_CHANGED_PREV",
    "IDM_SEARCH_CLEAR_CHANGE_HISTORY",
    "IDM_EDIT_MULTISELECTALL",
    "IDM_EDIT_MULTISELECTALLMATCHCASE",
    "IDM_EDIT_COLUMNMODE",
    "IDM_EDIT_COLUMNMODETIP",
    "IDM_SEARCH_MARK",
    "IDM_WINDOW_WINDOWS",
    "IDM_WINDOW_SORT_FN_ASC",
    "IDM_WINDOW_SORT_FN_DSC",
    "IDM_WINDOW_SORT_FP_ASC",
    "IDM_WINDOW_SORT_FP_DSC",
    "IDM_WINDOW_SORT_FT_ASC",
    "IDM_WINDOW_SORT_FT_DSC",
    "IDM_WINDOW_SORT_FS_ASC",
    "IDM_WINDOW_SORT_FS_DSC",
    "IDM_WINDOW_SORT_FD_ASC",
    "IDM_WINDOW_SORT_FD_DSC",
    "IDM_EDIT_AUTOCOMPLETE",
    "IDM_EDIT_AUTOCOMPLETE_CURRENTFILE",
    "IDM_EDIT_FUNCCALLTIP",
    "IDM_EDIT_FUNCCALLTIP_PREVIOUS",
    "IDM_EDIT_FUNCCALLTIP_NEXT",
    "IDM_EDIT_AUTOCOMPLETE_PATH",
    "IDM_EDIT_COPY_BINARY",
    "IDM_EDIT_CUT_BINARY",
    "IDM_EDIT_PASTE_BINARY",
    "IDM_VIEW_DOCLIST",
    "IDM_VIEW_FUNC_LIST",
    "IDM_VIEW_DOC_MAP",
    "IDM_VIEW_FILEBROWSER",
    "IDM_VIEW_PROJECT_PANEL_1",
    "IDM_VIEW_PROJECT_PANEL_2",
    "IDM_VIEW_PROJECT_PANEL_3",
    "IDM_FILE_OPENFOLDERASWORKSPACE",
    "IDM_FILE_CONTAININGFOLDERASWORKSPACE",
    "IDM_EDIT_CHAR_PANEL",
    "IDM_EDIT_CLIPBOARDHISTORY_PANEL",
    "IDM_LANGSTYLE_CONFIG_DLG",
    "IDM_SETTING_IMPORTSTYLETHEMES",
    "IDM_LANG_USER",
    "IDM_LANG_USER_DLG",
    "IDM_LANG_OPENUDLDIR",
    "IDM_LANG_UDLCOLLECTION_PROJECT_SITE",
    "IDM_SETTING_PREFERENCE",
    "IDM_EDIT_PASTE_AS_HTML",
    "IDM_EDIT_PASTE_AS_RTF",
    "IDM_EDIT_OPENSELECTEDFILETOEDIT",
    "IDM_EDIT_OPENSELECTEDFILEFOLDERINEXPLORER",
    "IDM_EDIT_SEARCHONINTERNET",
    "IDM_EDIT_CHANGESEARCHENGINE",
    "IDM_VIEW_POSTIT",
    "IDM_VIEW_DISTRACTIONFREE",
    "IDM_VIEW_HIDELINES",
    "IDM_VIEW_MONITORING",
    "IDM_SETTING_EDITCONTEXTMENU",
    "IDM_VIEW_SWITCHTO_OTHER_VIEW",
    "IDM_FILE_OPEN",
    "IDM_FILE_SAVEAS",
    "IDM_FILE_SAVECOPYAS",
    "IDM_FILE_RENAME",
    "IDM_FILE_DELETE",
    "IDM_FILE_OPEN_FOLDER",
    "IDM_FILE_OPEN_CMD",
    "IDM_FILE_OPEN_DEFAULT_VIEWER",
    "IDM_FILE_LOADSESSION",
    "IDM_FILE_SAVESESSION",
    "IDM_FILE_RESTORELASTCLOSEDFILE",
    "IDM_OPEN_ALL_RECENT_FILE",
    "IDM_CLEAN_RECENT_FILE_LIST",
    "IDM_FILE_EXIT",
    "IDM_SEARCH_FIND",
    "IDM_SEARCH_REPLACE",
    "IDM_SEARCH_FINDINFILES",
    "IDM_SEARCH_GOTOLINE",
    "IDM_VIEW_ZOOMIN",
    "IDM_VIEW_ZOOMOUT",
    "IDM_VIEW_ZOOMRESTORE",
    "IDM_VIEW_SUMMARY",
    "IDM_FORMAT_ANSI",
    "IDM_FORMAT_AS_UTF_8",
    "IDM_FORMAT_UTF_8",
    "IDM_FORMAT_UTF_16BE",
    "IDM_FORMAT_UTF_16LE",
    "IDM_FORMAT_CONV2_ANSI",
    "IDM_FORMAT_CONV2_AS_UTF_8",
    "IDM_FORMAT_CONV2_UTF_8",
    "IDM_FORMAT_CONV2_UTF_16BE",
    "IDM_FORMAT_CONV2_UTF_16LE",
    "IDM_SETTING_SHORTCUT_MAPPER",
    "IDM_MACRO_STARTRECORDINGMACRO",
    "IDM_MACRO_STOPRECORDINGMACRO",
    "IDM_MACRO_PLAYBACKRECORDEDMACRO",
    "IDM_MACRO_SAVECURRENTMACRO",
    "IDM_MACRO_RUNMULTIMACRODLG",
    "IDM_EXECUTE",
    "IDM_CMDLINEARGUMENTS",
    "IDM_HOMESWEETHOME",
    "IDM_PROJECTPAGE",
    "IDM_ONLINEDOCUMENT",
    "IDM_FORUM",
    "IDM_DEBUGINFO",
    "IDM_ABOUT",
];

pub(crate) struct Cmd {
    pub(crate) name: String,
    pub(crate) id: i32,
    pub(crate) action: &'static str,
    pub(crate) tag: isize,
}

// The menu item tag that matches all tags.
pub(crate) const ANY: isize = isize::MIN;

// Notepad++ commands (menuCmdID.h) that this app has in its menus; a tag of ANY matches all tags.
pub(crate) fn menu_cmds() -> Vec<Cmd> {
    let mut v: Vec<(String, i32, &'static str, isize)> = [
        ("IDM_FILE_NEW", 41001, "newDocument:", ANY),
        ("IDM_SETTING_PREFERENCE", 48011, "showPreferences:", ANY),
        ("IDM_FILE_CLOSE", 41003, "closeTab:", ANY),
        ("IDM_FILE_CLOSEALL", 41004, "closeMultiple:", 0),
        ("IDM_FILE_CLOSEALL_BUT_CURRENT", 41005, "closeMultiple:", 1),
        ("IDM_FILE_CLOSEALL_TOLEFT", 41009, "closeMultiple:", 2),
        ("IDM_FILE_CLOSEALL_TORIGHT", 41018, "closeMultiple:", 3),
        ("IDM_FILE_CLOSEALL_UNCHANGED", 41024, "closeMultiple:", 4),
        ("IDM_FILE_SAVE", 41006, "saveDocument:", ANY),
        ("IDM_FILE_SAVEALL", 41007, "saveAll:", ANY),
        ("IDM_FILE_RELOAD", 41014, "reloadFromDisk:", ANY),
        ("IDM_FILE_PRINT", 41010, "filePrint:", ANY),
        ("IDM_FILE_PRINTNOW", 1001, "filePrintNow:", ANY),
        ("IDM_EDIT_CUT", 42001, "cut:", ANY),
        ("IDM_EDIT_COPY", 42002, "copy:", ANY),
        ("IDM_EDIT_UNDO", 42003, "undo:", ANY),
        ("IDM_EDIT_REDO", 42004, "redo:", ANY),
        ("IDM_EDIT_PASTE", 42005, "paste:", ANY),
        ("IDM_EDIT_DELETE", 42006, "sciCommand:", 2180),
        ("IDM_EDIT_SELECTALL", 42007, "selectAll:", ANY),
        ("IDM_EDIT_INS_TAB", 42008, "editOp:", 10),
        ("IDM_EDIT_RMV_TAB", 42009, "editOp:", 11),
        ("IDM_EDIT_DUP_LINE", 42010, "sciCommand:", 2404),
        ("IDM_EDIT_SPLIT_LINES", 42012, "editOp:", 3),
        ("IDM_EDIT_JOIN_LINES", 42013, "editOp:", 4),
        ("IDM_EDIT_LINE_UP", 42014, "sciCommand:", 2620),
        ("IDM_EDIT_LINE_DOWN", 42015, "sciCommand:", 2621),
        ("IDM_EDIT_BEGINENDSELECT", 42020, "beginEndSelect:", 0),
        (
            "IDM_EDIT_BEGINENDSELECT_COLUMNMODE",
            42089,
            "beginEndSelect:",
            1,
        ),
        ("IDM_EDIT_BLOCK_COMMENT", 42022, "comment:", 0),
        ("IDM_EDIT_BLOCK_COMMENT_SET", 42035, "comment:", 1),
        ("IDM_EDIT_BLOCK_UNCOMMENT", 42036, "comment:", 2),
        ("IDM_EDIT_STREAM_COMMENT", 42023, "comment:", 3),
        ("IDM_EDIT_STREAM_UNCOMMENT", 42047, "comment:", 4),
        ("IDM_EDIT_TRIMTRAILING", 42024, "editOp:", 60),
        ("IDM_EDIT_TRIMLINEHEAD", 42042, "editOp:", 61),
        ("IDM_EDIT_TRIM_BOTH", 42043, "editOp:", 62),
        ("IDM_EDIT_EOL2WS", 42044, "editOp:", 63),
        ("IDM_EDIT_TRIMALL", 42045, "editOp:", 64),
        ("IDM_EDIT_TAB2SW", 42046, "editOp:", 65),
        ("IDM_EDIT_SW2TAB_ALL", 42054, "editOp:", 66),
        ("IDM_EDIT_SW2TAB_LEADING", 42053, "editOp:", 67),
        ("IDM_EDIT_TOGGLEREADONLY", 42028, "toggleReadOnly:", ANY),
        ("IDM_EDIT_FULLPATHTOCLIP", 42029, "copyPathInfo:", 0),
        ("IDM_EDIT_FILENAMETOCLIP", 42030, "copyPathInfo:", 1),
        ("IDM_EDIT_CURRENTDIRTOCLIP", 42031, "copyPathInfo:", 2),
        ("IDM_EDIT_REMOVEEMPTYLINES", 42055, "editOp:", 5),
        ("IDM_EDIT_REMOVEEMPTYLINESWITHBLANK", 42056, "editOp:", 6),
        ("IDM_EDIT_BLANKLINEABOVECURRENT", 42057, "editOp:", 7),
        ("IDM_EDIT_BLANKLINEBELOWCURRENT", 42058, "editOp:", 8),
        ("IDM_EDIT_REMOVE_ANY_DUP_LINES", 42079, "editOp:", 1),
        ("IDM_EDIT_REMOVE_CONSECUTIVE_DUP_LINES", 42077, "editOp:", 2),
        ("IDM_EDIT_SORTLINES_REVERSE_ORDER", 42083, "editOp:", 9),
        (
            "IDM_EDIT_INSERT_DATETIME_SHORT",
            42084,
            "insertDateTime:",
            0,
        ),
        ("IDM_EDIT_INSERT_DATETIME_LONG", 42085, "insertDateTime:", 1),
        ("IDM_EDIT_AUTOCOMPLETE", 50000, "autoComplete:", 0),
        ("IDM_EDIT_AUTOCOMPLETE_CURRENTFILE", 50001, "autoComplete:", 1),
        ("IDM_EDIT_FUNCCALLTIP", 50002, "autoComplete:", 2),
        ("IDM_EDIT_FUNCCALLTIP_PREVIOUS", 50010, "autoComplete:", 3),
        ("IDM_EDIT_FUNCCALLTIP_NEXT", 50011, "autoComplete:", 4),
        ("IDM_EDIT_AUTOCOMPLETE_PATH", 50006, "autoComplete:", 5),
        ("IDM_SEARCH_FINDNEXT", 43002, "findNext:", ANY),
        ("IDM_SEARCH_FINDPREV", 43010, "findPrevious:", ANY),
        ("IDM_SEARCH_FINDINCREMENT", 43011, "showIncrementalSearch:", ANY),
        ("IDM_SEARCH_SETANDFINDNEXT", 43048, "searchCmd:", 0),
        ("IDM_SEARCH_SETANDFINDPREV", 43049, "searchCmd:", 1),
        ("IDM_SEARCH_VOLATILE_FINDNEXT", 43014, "searchCmd:", 2),
        ("IDM_SEARCH_VOLATILE_FINDPREV", 43015, "searchCmd:", 3),
        ("IDM_FOCUS_ON_FOUND_RESULTS", 43045, "searchCmd:", 4),
        ("IDM_SEARCH_GOTONEXTFOUND", 43046, "searchCmd:", 5),
        ("IDM_SEARCH_GOTOPREVFOUND", 43047, "searchCmd:", 6),
        ("IDM_SEARCH_GOTOMATCHINGBRACE", 43009, "searchCmd:", 7),
        ("IDM_SEARCH_SELECTMATCHINGBRACES", 43053, "searchCmd:", 8),
        ("IDM_SEARCH_CHANGED_NEXT", 43067, "searchCmd:", 10),
        ("IDM_SEARCH_CHANGED_PREV", 43068, "searchCmd:", 11),
        ("IDM_SEARCH_CLEAR_CHANGE_HISTORY", 43069, "searchCmd:", 12),
        ("IDM_SEARCH_TOGGLE_BOOKMARK", 43005, "searchCmd:", 20),
        ("IDM_SEARCH_NEXT_BOOKMARK", 43006, "searchCmd:", 21),
        ("IDM_SEARCH_PREV_BOOKMARK", 43007, "searchCmd:", 22),
        ("IDM_SEARCH_CLEAR_BOOKMARKS", 43008, "searchCmd:", 23),
        ("IDM_SEARCH_CUTMARKEDLINES", 43018, "searchCmd:", 24),
        ("IDM_SEARCH_COPYMARKEDLINES", 43019, "searchCmd:", 25),
        ("IDM_SEARCH_PASTEMARKEDLINES", 43020, "searchCmd:", 26),
        ("IDM_SEARCH_DELETEMARKEDLINES", 43021, "searchCmd:", 27),
        ("IDM_SEARCH_DELETEUNMARKEDLINES", 43051, "searchCmd:", 28),
        ("IDM_SEARCH_INVERSEMARKS", 43050, "searchCmd:", 29),
        ("IDM_VIEW_ALWAYSONTOP", 44034, "alwaysOnTop:", ANY),
        ("IDM_VIEW_FULLSCREENTOGGLE", 44032, "fullScreen:", ANY),
        ("IDM_VIEW_IN_FIREFOX", 44100, "viewInBrowser:", 0),
        ("IDM_VIEW_IN_CHROME", 44101, "viewInBrowser:", 1),
        ("IDM_VIEW_IN_EDGE", 44102, "viewInBrowser:", 2),
        ("IDM_VIEW_DOCLIST", 44070, "toggleDocList:", ANY),
        ("IDM_VIEW_FUNC_LIST", 44084, "toggleFunctionList:", ANY),
        ("IDM_VIEW_POSTIT", 44009, "postIt:", ANY),
        ("IDM_VIEW_DISTRACTIONFREE", 44011, "distractionFree:", ANY),
        ("IDM_VIEW_HIDELINES", 44042, "hideLines:", ANY),
        ("IDM_VIEW_MONITORING", 44097, "monitoring:", ANY),
        ("IDM_SETTING_EDITCONTEXTMENU", 48018, "editContextMenu:", ANY),
        ("IDM_VIEW_DOC_MAP", 44080, "toggleDocMap:", ANY),
        ("IDM_VIEW_FILEBROWSER", 44085, "toggleFolderAsWorkspace:", ANY),
        ("IDM_VIEW_PROJECT_PANEL_1", 44081, "toggleProjectPanel1:", ANY),
        ("IDM_VIEW_PROJECT_PANEL_2", 44082, "toggleProjectPanel2:", ANY),
        ("IDM_VIEW_PROJECT_PANEL_3", 44083, "toggleProjectPanel3:", ANY),
        ("IDM_FILE_OPENFOLDERASWORKSPACE", 41022, "openFolderAsWorkspace:", ANY),
        ("IDM_FILE_CONTAININGFOLDERASWORKSPACE", 41025, "containingFolderAsWorkspace:", ANY),
        ("IDM_EDIT_CHAR_PANEL", 42051, "toggleCharPanel:", ANY),
        ("IDM_EDIT_CLIPBOARDHISTORY_PANEL", 42052, "toggleClipboardHistory:", ANY),
        (
            "IDM_VIEW_WRAP",
            44022,
            "viewOption:",
            crate::view::WRAP as isize,
        ),
        ("IDM_VIEW_FOLDALL", 44010, "foldAll:", 0),
        ("IDM_VIEW_UNFOLDALL", 44029, "foldAll:", 1),
        ("IDM_VIEW_FOLD_CURRENT", 44030, "foldCurrent:", 0),
        ("IDM_VIEW_UNFOLD_CURRENT", 44031, "foldCurrent:", 1),
        ("IDM_VIEW_TAB_START", 44116, "selectTab:", 9),
        ("IDM_VIEW_TAB_END", 44117, "selectTab:", 10),
        ("IDM_VIEW_TAB_NEXT", 44095, "selectTab:", 11),
        ("IDM_VIEW_TAB_PREV", 44096, "selectTab:", 12),
        ("IDM_VIEW_GOTO_START", 10005, "moveTab:", 0),
        ("IDM_VIEW_GOTO_END", 10006, "moveTab:", 1),
        ("IDM_VIEW_TAB_MOVEFORWARD", 44098, "moveTab:", 2),
        ("IDM_VIEW_TAB_MOVEBACKWARD", 44099, "moveTab:", 3),
        ("IDM_VIEW_GOTO_ANOTHER_VIEW", 10001, "moveToOtherView:", ANY),
        ("IDM_VIEW_CLONE_TO_ANOTHER_VIEW", 10002, "cloneToOtherView:", ANY),
        ("IDM_VIEW_SWITCHTO_OTHER_VIEW", 44072, "focusOtherView:", ANY),
        ("IDM_VIEW_SYNSCROLLV", 44035, "syncScroll:", 0),
        ("IDM_VIEW_SYNSCROLLH", 44036, "syncScroll:", 1),
        ("IDM_FORMAT_TODOS", 45001, "eolConvert:", 0),
        ("IDM_FORMAT_TOUNIX", 45002, "eolConvert:", 2),
        ("IDM_FORMAT_TOMAC", 45003, "eolConvert:", 1),
        ("IDM_LANGSTYLE_CONFIG_DLG", 46001, "styleConfigurator:", ANY),
        ("IDM_SETTING_IMPORTSTYLETHEMES", 48006, "importStyleThemes:", ANY),
    ]
    .into_iter()
    .map(|(n, i, a, t)| (n.to_string(), i, a, t))
    .collect();
    let cases = [
        ("IDM_EDIT_UPPERCASE", 42016),
        ("IDM_EDIT_LOWERCASE", 42017),
        ("IDM_EDIT_PROPERCASE_FORCE", 42067),
        ("IDM_EDIT_PROPERCASE_BLEND", 42068),
        ("IDM_EDIT_SENTENCECASE_FORCE", 42069),
        ("IDM_EDIT_SENTENCECASE_BLEND", 42070),
        ("IDM_EDIT_INVERTCASE", 42071),
        ("IDM_EDIT_RANDOMCASE", 42072),
    ];
    for (k, (n, id)) in cases.into_iter().enumerate() {
        v.push((n.into(), id, "editOp:", 40 + k as isize));
    }
    let sorts = [
        ("LEXICOGRAPHIC", 42059),
        ("LEXICO_CASE_INSENS", 42080),
        ("INTEGER", 42061),
        ("DECIMALCOMMA", 42063),
        ("DECIMALDOT", 42065),
        ("LENGTH", 42104),
    ];
    for (k, (n, id)) in sorts.into_iter().enumerate() {
        for (d, dir) in ["ASCENDING", "DESCENDING"].into_iter().enumerate() {
            let tag = 20 + 2 * k as isize + d as isize;
            v.push((
                format!("IDM_EDIT_SORTLINES_{n}_{dir}"),
                id + d as i32,
                "editOp:",
                tag,
            ));
        }
    }
    for n in 0..8 {
        v.push((
            format!("IDM_VIEW_FOLD_{}", n + 1),
            44051 + n,
            "foldLevel:",
            n as isize,
        ));
        v.push((
            format!("IDM_VIEW_UNFOLD_{}", n + 1),
            44061 + n,
            "unfoldLevel:",
            n as isize,
        ));
    }
    let multi = [
        "ALL",
        "ALLMATCHCASE",
        "ALLWHOLEWORD",
        "ALLMATCHCASEWHOLEWORD",
        "NEXT",
        "NEXTMATCHCASE",
        "NEXTWHOLEWORD",
        "NEXTMATCHCASEWHOLEWORD",
        "UNDO",
        "SSKIP",
    ];
    for (k, n) in multi.into_iter().enumerate() {
        v.push((format!("IDM_EDIT_MULTISELECT{n}"), 42090 + k as i32, "multiSelect:", k as isize));
    }
    v.push(("IDM_EDIT_SORTLINES_RANDOMLY".into(), 42078, "editOp:", 12));
    v.push(("IDM_EDIT_SORTLINES_LOCALE_ASCENDING".into(), 42100, "sortLocale:", 0));
    v.push(("IDM_EDIT_SORTLINES_LOCALE_DESCENDING".into(), 42101, "sortLocale:", 1));
    v.push(("IDM_EDIT_INSERT_DATETIME_CUSTOMIZED".into(), 42086, "insertDateTimeCustom:", ANY));
    v.push(("IDM_EDIT_COPY_ALL_NAMES".into(), 42087, "copyAllNames:", 0));
    v.push(("IDM_EDIT_COPY_ALL_PATHS".into(), 42088, "copyAllNames:", 1));
    v.push(("IDM_EDIT_SETREADONLYFORALLDOCS".into(), 42102, "readOnlyAll:", 1));
    v.push(("IDM_EDIT_CLEARREADONLYFORALLDOCS".into(), 42103, "readOnlyAll:", 0));
    v.push(("IDM_EDIT_TOGGLESYSTEMREADONLY".into(), 42033, "toggleFileReadOnly:", ANY));
    v.push(("IDM_EDIT_PASTE_AS_HTML".into(), 42038, "pasteMarkup:", 0));
    v.push(("IDM_EDIT_PASTE_AS_RTF".into(), 42039, "pasteMarkup:", 1));
    v.push(("IDM_EDIT_OPENSELECTEDFILETOEDIT".into(), 42073, "onSelection:", 0));
    v.push(("IDM_EDIT_OPENSELECTEDFILEFOLDERINEXPLORER".into(), 42074, "onSelection:", 1));
    v.push(("IDM_EDIT_SEARCHONINTERNET".into(), 42075, "onSelection:", 2));
    v.push(("IDM_EDIT_CHANGESEARCHENGINE".into(), 42076, "onSelection:", 3));
    v.push(("IDM_EDIT_REDACT_SELECTION".into(), 42106, "redactSelection:", ANY));
    v.push(("IDM_EDIT_COLUMNMODE".into(), 42034, "columnEditor:", ANY));
    v.push(("IDM_EDIT_COLUMNMODETIP".into(), 42037, "columnModeTip:", ANY));
    v.push(("IDM_EDIT_COPY_BINARY".into(), 42048, "copyBinary:", 0));
    v.push(("IDM_EDIT_CUT_BINARY".into(), 42049, "copyBinary:", 1));
    v.push(("IDM_EDIT_PASTE_BINARY".into(), 42050, "pasteBinary:", ANY));
    for n in 0..9 {
        v.push((
            format!("IDM_VIEW_TAB{}", n + 1),
            44086 + n,
            "selectTab:",
            n as isize,
        ));
    }
    v.push(("IDM_SEARCH_MARK".into(), 43054, "showMark:", ANY));
    for k in 0..5 {
        let (n, t) = (k + 1, k as isize);
        v.push((format!("IDM_SEARCH_MARKALLEXT{n}"), 43022 + 2 * k, "markCmd:", t));
        v.push((format!("IDM_SEARCH_MARKONEEXT{n}"), 43062 + k, "markCmd:", 10 + t));
        v.push((format!("IDM_SEARCH_UNMARKALLEXT{n}"), 43023 + 2 * k, "markCmd:", 20 + t));
        v.push((format!("IDM_SEARCH_GOPREVMARKER{n}"), 43033 + k, "markCmd:", 30 + t));
        v.push((format!("IDM_SEARCH_GONEXTMARKER{n}"), 43039 + k, "markCmd:", 40 + t));
        v.push((format!("IDM_SEARCH_STYLE{n}TOCLIP"), 43055 + k, "markCmd:", 50 + t));
    }
    v.push(("IDM_SEARCH_CLEARALLMARKS".into(), 43032, "markCmd:", 25));
    v.push(("IDM_SEARCH_GOPREVMARKER_DEF".into(), 43038, "markCmd:", 36));
    v.push(("IDM_SEARCH_GONEXTMARKER_DEF".into(), 43044, "markCmd:", 46));
    v.push(("IDM_SEARCH_ALLSTYLESTOCLIP".into(), 43060, "markCmd:", 55));
    v.push(("IDM_SEARCH_MARKEDTOCLIP".into(), 43061, "markCmd:", 56));
    v.push(("IDM_LANG_USER".into(), 46180, "userDefined:", ANY));
    v.push(("IDM_LANG_USER_DLG".into(), 46250, "defineUdl:", ANY));
    v.push(("IDM_LANG_OPENUDLDIR".into(), 46300, "openUdlFolder:", ANY));
    v.push(("IDM_LANG_UDLCOLLECTION_PROJECT_SITE".into(), 46301, "udlCollection:", ANY));
    v.push(("IDM_WINDOW_WINDOWS".into(), 11001, "showWindows:", ANY));
    for (k, n) in ["FN_ASC", "FN_DSC", "FP_ASC", "FP_DSC", "FT_ASC", "FT_DSC", "FS_ASC", "FS_DSC", "FD_ASC", "FD_DSC"]
        .into_iter()
        .enumerate()
    {
        v.push((format!("IDM_WINDOW_SORT_{n}"), 11002 + k as i32, "sortTabs:", k as isize));
    }
    for (n, id, a, t) in [
        ("IDM_FILE_OPEN", 41002, "openDocument:", ANY),
        ("IDM_FILE_SAVEAS", 41008, "saveDocumentAs:", ANY),
        ("IDM_FILE_SAVECOPYAS", 41015, "saveCopyAs:", ANY),
        ("IDM_FILE_RENAME", 41017, "renameFile:", ANY),
        ("IDM_FILE_DELETE", 41016, "moveToTrash:", ANY),
        ("IDM_FILE_OPEN_FOLDER", 41019, "openFolderFinder:", ANY),
        ("IDM_FILE_OPEN_CMD", 41020, "openFolderTerminal:", ANY),
        ("IDM_FILE_OPEN_DEFAULT_VIEWER", 41023, "openDefaultViewer:", ANY),
        ("IDM_FILE_LOADSESSION", 41012, "loadSession:", ANY),
        ("IDM_FILE_SAVESESSION", 41013, "saveSession:", ANY),
        ("IDM_FILE_RESTORELASTCLOSEDFILE", 41021, "restoreRecentClosed:", ANY),
        ("IDM_OPEN_ALL_RECENT_FILE", 42040, "openAllRecent:", ANY),
        ("IDM_CLEAN_RECENT_FILE_LIST", 42041, "emptyRecent:", ANY),
        ("IDM_FILE_EXIT", 41011, "terminate:", ANY),
        ("IDM_SEARCH_FIND", 43001, "showFind:", ANY),
        ("IDM_SEARCH_REPLACE", 43003, "showReplace:", ANY),
        ("IDM_SEARCH_FINDINFILES", 43013, "showFindInFiles:", ANY),
        ("IDM_SEARCH_GOTOLINE", 43004, "goToLine:", ANY),
        ("IDM_VIEW_ZOOMIN", 44023, "zoom:", 1),
        ("IDM_VIEW_ZOOMOUT", 44024, "zoom:", -1),
        ("IDM_VIEW_ZOOMRESTORE", 44033, "zoom:", 0),
        ("IDM_VIEW_SUMMARY", 44049, "summary:", ANY),
        ("IDM_FORMAT_ANSI", 45004, "encodeIn:", 1),
        ("IDM_FORMAT_AS_UTF_8", 45008, "encodeIn:", 2),
        ("IDM_FORMAT_UTF_8", 45005, "encodeIn:", 3),
        ("IDM_FORMAT_UTF_16BE", 45006, "encodeIn:", 4),
        ("IDM_FORMAT_UTF_16LE", 45007, "encodeIn:", 5),
        ("IDM_FORMAT_CONV2_ANSI", 45009, "convertTo:", 1),
        ("IDM_FORMAT_CONV2_AS_UTF_8", 45010, "convertTo:", 2),
        ("IDM_FORMAT_CONV2_UTF_8", 45011, "convertTo:", 3),
        ("IDM_FORMAT_CONV2_UTF_16BE", 45012, "convertTo:", 4),
        ("IDM_FORMAT_CONV2_UTF_16LE", 45013, "convertTo:", 5),
        ("IDM_SETTING_SHORTCUT_MAPPER", 48009, "showShortcutMapper:", ANY),
        ("IDM_MACRO_STARTRECORDINGMACRO", 42018, "macroToggleRecord:", 0),
        ("IDM_MACRO_STOPRECORDINGMACRO", 42019, "macroToggleRecord:", 1),
        ("IDM_MACRO_PLAYBACKRECORDEDMACRO", 42021, "macroPlayback:", ANY),
        ("IDM_MACRO_SAVECURRENTMACRO", 42025, "macroSave:", ANY),
        ("IDM_MACRO_RUNMULTIMACRODLG", 42032, "macroShowMulti:", ANY),
        ("IDM_EXECUTE", 49000, "runShow:", ANY),
        ("IDM_CMDLINEARGUMENTS", 47010, "showCmdLineArgs:", ANY),
        ("IDM_HOMESWEETHOME", 47001, "openLink:", 0),
        ("IDM_PROJECTPAGE", 47002, "openLink:", 1),
        ("IDM_ONLINEDOCUMENT", 47003, "openLink:", 2),
        ("IDM_FORUM", 47004, "openLink:", 3),
        ("IDM_DEBUGINFO", 47012, "showDebugInfo:", ANY),
        ("IDM_ABOUT", 47000, "showAbout:", ANY),
    ] {
        v.push((n.into(), id, a, t));
    }
    v.into_iter()
        .map(|(name, id, action, tag)| Cmd {
            name,
            id,
            action,
            tag,
        })
        .collect()
}

pub fn menu_step(action: &str, tag: isize) -> Option<Step> {
    if let Some((_, m)) = SCI_ACTIONS.iter().find(|a| a.0 == action) {
        return Some(Step::new(shortcuts::TYPE_L, *m, 0, 0, ""));
    }
    menu_cmds()
        .into_iter()
        .find(|c| c.action == action && (c.tag == ANY || c.tag == tag))
        .filter(|c| !NOT_RECORDED.contains(&c.name.as_str()))
        .map(|c| Step::menu(c.id))
}

// (action, tag, menuCmdID.h ID) of each menu command in the table.
pub(crate) fn menu_ids() -> Vec<(&'static str, isize, i32)> {
    menu_cmds().into_iter().map(|c| (c.action, c.tag, c.id)).collect()
}

pub(crate) fn menu_action(id: i32) -> Option<(&'static str, isize)> {
    menu_cmds()
        .into_iter()
        .find(|c| c.id == id)
        .map(|c| (c.action, c.tag))
}

// Port of the "Run until the end of file" loop of the WM_MACRODLGRUNMACRO handler in NppBigSwitch.cpp.
#[derive(Debug, Default)]
pub struct UntilEof {
    last: isize,
    cur: isize,
    d_last: isize,
    d_cur: isize,
    up: bool,
    n: usize,
}

impl UntilEof {
    pub fn new(line_count: isize, cur_line: isize) -> UntilEof {
        UntilEof {
            last: line_count - 1,
            cur: cur_line,
            ..UntilEof::default()
        }
    }

    // Call after each run; true means run again.
    pub fn again(&mut self, line_count: isize, cur_line: isize) -> bool {
        self.n += 1;
        if self.n > 2 && self.up != (self.d_cur < 0) && self.d_last >= 0 {
            return false;
        }
        self.up = self.d_cur < 0;
        self.d_last = line_count - 1 - self.last;
        self.d_cur = cur_line - self.cur;
        if self.d_cur == 0 && self.d_last >= 0 {
            return false;
        }
        if self.d_last < self.d_cur {
            self.last += self.d_last;
        }
        self.cur += self.d_cur;
        !(self.cur > self.last
            || self.cur < 0
            || (self.d_cur == 0 && self.cur == 0 && (self.d_last >= 0 || self.up)))
    }
}

#[derive(Default, Clone)]
struct SnR {
    find: String,
    replace: String,
    flags: isize,
    mode: isize,
}

impl SnR {
    fn opts(&self) -> Opts {
        Opts {
            find: self.find.clone(),
            replace: self.replace.clone(),
            whole_word: self.flags & IDF_WHOLEWORD != 0,
            match_case: self.flags & IDF_MATCHCASE != 0,
            wrap: self.flags & IDF_WRAP != 0,
            mode: match self.mode {
                1 => Mode::Extended,
                2 => Mode::Regex,
                _ => Mode::Normal,
            },
            dot_nl: self.flags & IDF_REDOTMATCHNL != 0,
        }
    }
}

#[repr(C)]
struct Scn {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
    position: isize,
    ch: i32,
    modifiers: i32,
    modification_type: i32,
    text: *const c_char,
    length: isize,
    lines_added: isize,
    message: i32,
    w_param: usize,
    l_param: isize,
}

struct MultiUi {
    form: Form,
    list: Retained<NSPopUpButton>,
    times: Retained<NSTextField>,
    multi: Retained<NSButton>,
}

#[derive(Default)]
struct State {
    recording: Cell<bool>,
    saved: Cell<bool>,
    current: RefCell<Vec<Step>>,
    store: RefCell<Shortcuts>,
    menus: OnceCell<[Retained<NSMenu>; 2]>,
    load_error: RefCell<Option<String>>,
    mark: Cell<Option<usize>>,
    multi: OnceCell<MultiUi>,
}

thread_local! {
    static S: State = State::default();
}

pub fn with_store<R>(f: impl FnOnce(&mut Shortcuts) -> R) -> R {
    S.with(|s| f(&mut s.store.borrow_mut()))
}

fn file() -> Option<PathBuf> {
    Some(crate::config::app_support_dir()?.join("shortcuts.xml"))
}

// The text of the file, or None when the file does not exist.
fn read_file(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read(path) {
        Ok(b) => String::from_utf8(b)
            .map(Some)
            .map_err(|_| format!("{}: the file is not UTF-8.", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

// Writes a temporary file and renames it, so that an error cannot leave a part of the file.
fn write_file(path: &Path, text: &str) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(path.parent().unwrap()).map_err(err)?;
    let tmp = path.with_extension("xml.tmp");
    std::fs::write(&tmp, text).map_err(err)?;
    std::fs::rename(&tmp, path).map_err(err)
}

fn load() -> Result<Shortcuts, String> {
    let Some(path) = file() else {
        return Ok(shortcuts::defaults());
    };
    match read_file(&path)? {
        None => Ok(shortcuts::defaults()),
        Some(t) => shortcuts::parse(&t).map_err(|e| format!("{}: {e}", path.display())),
    }
}

fn save() -> Result<(), String> {
    if let Some(e) = S.with(|s| s.load_error.borrow().clone()) {
        return Err(format!(
            "{e}\nThe app does not change this file. Correct or move it, then start the app again."
        ));
    }
    let path = file().ok_or("HOME is not set")?;
    let old = read_file(&path)?;
    let text = with_store(|s| shortcuts::write(old.as_deref(), s))?;
    write_file(&path, &text)
}

pub(crate) fn find_item(m: &NSMenu, action: Sel, tag: isize) -> Option<(Retained<NSMenu>, isize)> {
    for i in 0..m.numberOfItems() {
        let it = m.itemAtIndex(i)?;
        if it.action() == Some(action) && (tag == ANY || it.tag() == tag) {
            return Some((m.retain(), i));
        }
        if let Some(r) = it.submenu().and_then(|s| find_item(&s, action, tag)) {
            return Some(r);
        }
    }
    None
}

// Adds the items with Notepad++ FolderName grouping: next items with the same folder share one submenu.
fn add_items(
    mtm: MainThreadMarker,
    m: &NSMenu,
    items: Vec<(String, String)>,
    action: Sel,
    t: Option<&AnyObject>,
) {
    let mut folder: Option<(String, Retained<NSMenu>)> = None;
    for (i, (name, f)) in items.into_iter().enumerate() {
        let it = crate::tagged(mtm, &name, action, i as isize, t);
        if f.is_empty() {
            folder = None;
            m.addItem(&it);
            continue;
        }
        if folder.as_ref().is_none_or(|x| x.0 != f) {
            let top = crate::nested(mtm, &f, vec![]);
            m.addItem(&top);
            folder = Some((f, top.submenu().unwrap()));
        }
        folder.as_ref().unwrap().1.addItem(&it);
    }
}

fn rebuild(mtm: MainThreadMarker, t: Option<&AnyObject>) {
    let Some([mac, run]) = S.with(|s| s.menus.get().cloned()) else {
        return;
    };
    let (ms, cs) = with_store(|s| {
        (
            s.macros
                .iter()
                .map(|m| (m.name.clone(), m.folder.clone()))
                .collect::<Vec<_>>(),
            s.commands
                .iter()
                .map(|c| (c.name.clone(), c.folder.clone()))
                .collect::<Vec<_>>(),
        )
    });
    while mac.numberOfItems() > FIXED_ITEMS {
        mac.removeItemAtIndex(FIXED_ITEMS);
    }
    if !ms.is_empty() {
        mac.addItem(&NSMenuItem::separatorItem(mtm));
        add_items(mtm, &mac, ms, sel!(macroRunSaved:), t);
    }
    while run.numberOfItems() > 1 {
        run.removeItemAtIndex(1);
    }
    if !cs.is_empty() {
        run.addItem(&NSMenuItem::separatorItem(mtm));
        add_items(mtm, &run, cs, sel!(runUserCommand:), t);
    }
    crate::shortcut_mapper::bind_saved(&mac, &run);
}

// Port of Notepad_plus::checkMacroState; Start and Stop share the Notepad++ toggle key, so only the enabled one keeps it.
fn check_state() {
    S.with(|s| {
        let Some([m, _]) = s.menus.get() else { return };
        let (rec, empty) = (s.recording.get(), s.current.borrow().is_empty());
        let has_saved = !s.store.borrow().macros.is_empty();
        let on = [
            !rec,
            rec,
            !empty && !rec,
            !empty && !rec && !s.saved.get(),
            (!empty && !rec) || has_saved,
        ];
        for (i, e) in on.iter().enumerate() {
            if let Some(it) = m.itemAtIndex(i as isize) {
                it.setEnabled(*e);
                if i < 2 {
                    crate::shortcut_mapper::record_key(&it, i, *e);
                }
            }
        }
    });
}

pub fn menus(mtm: MainThreadMarker, bar: &NSMenu, t: Option<&AnyObject>) {
    let items = [
        ("Start Recording", sel!(macroToggleRecord:), ""),
        ("Stop Recording", sel!(macroToggleRecord:), ""),
        ("Playback", sel!(macroPlayback:), "P"),
        ("Save Current Recorded Macro...", sel!(macroSave:), ""),
        ("Run a Macro Multiple Times...", sel!(macroShowMulti:), ""),
    ];
    let mac = crate::nested(
        mtm,
        "Macro",
        items
            .iter()
            .enumerate()
            .map(|(i, (n, a, k))| {
                let it = crate::item(mtm, n, *a, k, t);
                it.setTag(i as isize);
                it
            })
            .collect(),
    );
    let run = crate::nested(mtm, "Run", crate::run::run_menu_items(mtm, t));
    run.submenu()
        .unwrap()
        .itemAtIndex(0)
        .unwrap()
        .setKeyEquivalentModifierMask(objc2_app_kit::NSEventModifierFlags::empty());
    bar.addItem(&mac);
    bar.addItem(&run);
    let mac = mac.submenu().unwrap();
    mac.setAutoenablesItems(false);
    S.with(|s| {
        let _ = s.menus.set([mac, run.submenu().unwrap()]);
        match load() {
            Ok(st) => *s.store.borrow_mut() = st,
            Err(e) => *s.load_error.borrow_mut() = Some(e),
        }
    });
    rebuild(mtm, t);
    check_state();
    if let Some(t) = t {
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                t,
                sel!(macroMenuDidSend:),
                Some(NSMenuDidSendActionNotification),
                None,
            );
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                t,
                sel!(macroMenuWillSend:),
                Some(NSMenuWillSendActionNotification),
                None,
            );
        };
        if S.with(|s| s.load_error.borrow().is_some()) {
            let _: () = unsafe {
                msg_send![t, performSelector: sel!(macroLoadError:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]
            };
        }
    }
}

pub(crate) fn recording() -> bool {
    S.with(|s| s.recording.get())
}

impl App {
    pub(crate) fn macro_show_load_error(&self) {
        if let Some(e) = S.with(|s| s.load_error.borrow().clone()) {
            self.alert("Cannot read shortcuts.xml", &format!("{e}\nThe saved macros and commands are not loaded. The app does not change the file."), &["OK"]);
        }
    }

    // Records in a new tab too while a recording runs.
    pub(crate) fn macro_arm(&self, v: &NSView) {
        if recording() {
            sci::send(v, SCI_STARTRECORD, 0, 0);
        }
    }

    fn macro_tab_views(&self) -> Vec<Retained<NSView>> {
        self.ivars()
            .tabs
            .borrow()
            .iter()
            .map(|t| t.view.clone())
            .collect()
    }

    fn macro_arm_tabs(&self) {
        self.macro_tab_views()
            .iter()
            .for_each(|v| _ = sci::send(v, SCI_STARTRECORD, 0, 0));
    }

    pub(crate) fn macro_toggle_record(&self) {
        let rec = recording();
        if rec {
            for v in self.macro_tab_views() {
                sci::send(&v, SCI_STOPRECORD, 0, 0);
            }
        } else {
            S.with(|s| s.current.borrow_mut().clear());
            self.macro_arm_tabs();
        }
        S.with(|s| {
            s.recording.set(!rec);
            s.saved.set(false);
        });
        check_state();
        self.macro_fill_list();
    }

    // Port of the SCN_MACRORECORD handler of NppNotification.cpp.
    pub(crate) fn macro_record(&self, scn: *const c_void) {
        let n = unsafe { &*(scn as *const Scn) };
        // CLEARALL and APPENDTEXT come only from the app (for example Reload from Disk), not from the user.
        if !recording() || n.id_from == sci::RESULTS_ID || matches!(n.message, 2004 | 2282) {
            return;
        }
        let s = (n.l_param != 0 && shortcuts::is_string_message(n.message)).then(|| {
            let p = n.l_param as *const c_char;
            // ADDTEXT, ADDSTYLEDTEXT and APPENDTEXT give the length in wParam.
            if matches!(n.message, 2001 | 2002 | 2282) {
                let b = unsafe { std::slice::from_raw_parts(p as *const u8, n.w_param) };
                String::from_utf8_lossy(b).into_owned()
            } else {
                unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
            }
        });
        let crlf = self
            .editor()
            .is_some_and(|v| sci::eol_mode(&v) == crate::encoding::SC_EOL_CRLF);
        S.with(|st| {
            shortcuts::record(
                &mut st.current.borrow_mut(),
                n.message,
                n.w_param,
                n.l_param,
                s.as_deref(),
                crlf,
            )
        });
    }

    // Records the menu commands that Notepad++ records as type 2 steps.
    pub(crate) fn macro_menu_did_send(&self, n: &NSNotification) {
        if !recording() {
            return;
        }
        self.macro_arm_tabs();
        let item: Option<Retained<AnyObject>> = n
            .userInfo()
            .and_then(|d| unsafe { msg_send![&d, objectForKey: &*ns("MenuItem")] });
        let Some(item) = item.and_then(|i| i.downcast::<NSMenuItem>().ok()) else {
            return;
        };
        let Some(action) = item.action() else { return };
        let main_key = NSApplication::sharedApplication(self.mtm())
            .keyWindow()
            .as_deref()
            == self.ivars().window.get().map(|w| &**w);
        if item.target().is_none() && !main_key {
            return;
        }
        let mark = S.with(|s| s.mark.take());
        if let Some(step) = menu_step(action.name().to_str().unwrap_or(""), item.tag()) {
            S.with(|s| {
                let mut cur = s.current.borrow_mut();
                // The command plays back as one step, so the Scintilla steps that it sent are not kept.
                if let Some(m) = mark.filter(|_| step.kind == TYPE_MENU) {
                    cur.truncate(m);
                }
                cur.push(step);
            });
        }
    }

    pub(crate) fn macro_menu_will_send(&self) {
        S.with(|s| {
            s.mark
                .set(s.recording.get().then(|| s.current.borrow().len()))
        });
    }

    fn macro_menu_command(&self, id: i32) {
        let Some((name, tag)) = menu_action(id) else {
            return;
        };
        let action = Sel::register(&CString::new(name).unwrap());
        let app = NSApplication::sharedApplication(self.mtm());
        let Some((m, i)) = app.mainMenu().and_then(|m| find_item(&m, action, tag)) else {
            return;
        };
        let it = m.itemAtIndex(i).unwrap();
        if it.target().is_some() {
            m.performActionForItemAtIndex(i);
        } else if let Some(v) = self.editor() {
            unsafe { app.sendAction_to_from(action, Some(&sci::content(&v)), Some(&it)) };
        }
    }

    // Port of FindReplaceDlg::execSavedCommand for Find Next, Replace and Replace All.
    fn macro_search(&self, step: &Step, env: &mut SnR) {
        match step.message {
            IDC_FRCOMMAND_INIT => *env = SnR::default(),
            IDFINDWHAT => env.find = step.s.clone(),
            IDREPLACEWITH => env.replace = step.s.clone(),
            IDNORMAL => env.mode = step.l,
            IDC_FRCOMMAND_BOOLEANS => env.flags = step.l,
            IDC_FRCOMMAND_EXEC => {
                let Some(v) = self.editor() else { return };
                let o = env.opts();
                let up = match step.l {
                    IDC_FINDNEXT => false,
                    IDC_FINDPREV => true,
                    _ => env.flags & IDF_WHICH_DIRECTION == 0,
                };
                if up && o.regex() {
                    return;
                }
                let doc = sci::doc(&v);
                match step.l {
                    IDOK | IDC_FINDNEXT | IDC_FINDPREV => {
                        if let Ok(Some((m, _))) =
                            search::find_next(&doc, &o, sci::selection(&v), up, Next::Find)
                        {
                            sci::select(&v, m);
                        }
                    }
                    IDREPLACE if !o.find.is_empty() => _ = self.replace_once(&v, &o),
                    IDREPLACEALL => {
                        let start = if o.wrap { 0 } else { sci::selection(&v).0 };
                        _ = search::replace_all(&doc, &o, (start, doc.len()));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Port of Notepad_plus::macroPlayback: each touched document gets one undo action, which `views` keeps open.
    fn macro_play(&self, m: &[Step], views: &mut Vec<Retained<NSView>>) {
        let mut env = SnR::default();
        for step in m {
            let Some(v) = self.editor() else { return };
            if !views.contains(&v) {
                sci::send(&v, SCI_BEGINUNDOACTION, 0, 0);
                views.push(v.clone());
            }
            match step.kind {
                TYPE_MENU => self.macro_menu_command(step.w as i32),
                TYPE_SNR => self.macro_search(step, &mut env),
                _ if !step.is_macroable() => {}
                TYPE_S => {
                    let s = CString::new(step.s.replace('\0', "")).unwrap();
                    // ADDTEXT and APPENDTEXT read wParam bytes, so wParam must be the real length.
                    let w = match step.message {
                        2001 | 2282 => s.as_bytes().len(),
                        _ => step.w,
                    };
                    sci::send(&v, step.message as u32, w, s.as_ptr() as isize);
                }
                _ => _ = sci::send(&v, step.message as u32, step.w, step.l),
            }
        }
    }

    fn macro_end_undo(views: Vec<Retained<NSView>>) {
        for v in views.iter().rev() {
            sci::send(v, SCI_ENDUNDOACTION, 0, 0);
        }
    }

    fn macro_run(&self, m: &[Step]) {
        let mut views = vec![];
        self.macro_play(m, &mut views);
        Self::macro_end_undo(views);
    }

    pub(crate) fn macro_playback(&self) {
        if recording() {
            return;
        }
        let m = S.with(|s| s.current.borrow().clone());
        self.macro_run(&m);
    }

    pub(crate) fn macro_run_saved(&self, s: &NSMenuItem) {
        let m = with_store(|st| st.macros.get(s.tag() as usize).map(|m| m.steps.clone()));
        if let Some(m) = m {
            self.macro_run(&m);
        }
    }

    // Name part of the Notepad++ Shortcut dialog; the key is not asked and stays empty.
    pub(crate) fn ask_shortcut_name(&self) -> Option<String> {
        let a = NSAlert::new(self.mtm());
        a.setMessageText(&ns("Shortcut"));
        a.setInformativeText(&ns("Name:"));
        let f = NSTextField::textFieldWithString(&NSString::new(), self.mtm());
        f.setFrame(NSRect::new(NSPoint::new(0., 0.), NSSize::new(260., 24.)));
        a.setAccessoryView(Some(&f));
        a.addButtonWithTitle(&ns("OK"));
        a.addButtonWithTitle(&ns("Cancel"));
        a.window().setInitialFirstResponder(Some(&f));
        let ok = a.runModal() == NSAlertFirstButtonReturn;
        let name = panel::text(&f).trim().to_string();
        (ok && !name.is_empty()).then_some(name)
    }

    pub(crate) fn store_changed(&self) {
        if let Err(e) = save() {
            self.alert("Cannot save shortcuts.xml", &e, &["OK"]);
        }
        let t: &AnyObject = self;
        rebuild(self.mtm(), Some(t));
        check_state();
        self.macro_fill_list();
    }

    pub(crate) fn macro_save(&self) {
        let Some(name) = self.ask_shortcut_name() else {
            return;
        };
        let steps = S.with(|s| s.current.borrow().clone());
        with_store(|s| {
            s.macros.push(Macro {
                name,
                steps,
                ..Macro::default()
            })
        });
        S.with(|s| s.saved.set(true));
        self.store_changed();
    }

    fn with_multi<R>(&self, f: impl FnOnce(&MultiUi) -> R) -> R {
        S.with(|s| {
            f(s.multi.get_or_init(|| {
                let mtm = self.mtm();
                let t: &AnyObject = self;
                let f = Form::new(mtm, "Run a Macro Multiple Times", 340., 170.);
                f.label("Macro to run", 16., 10., 300.);
                let list = NSPopUpButton::new(mtm);
                f.place(&list, 16., 34., 308., 26.);
                let radio = |title: &str, top: f64, w: f64| {
                    let b = unsafe {
                        NSButton::radioButtonWithTitle_target_action(
                            &ns(title),
                            Some(t),
                            Some(sel!(macroRunMode:)),
                            mtm,
                        )
                    };
                    f.place(&b, 24., top, w, 20.);
                    b
                };
                let multi = radio("Run", 74., 60.);
                let times = f.field(86., 72., 50.);
                times.setStringValue(&ns("1"));
                f.label("times", 142., 72., 80.);
                let eof = radio("Run until the end of file", 100., 260.);
                panel::set_on(&multi, true);
                panel::set_on(&eof, false);
                f.button("Run", 76., 130., 90., t, sel!(macroRunMulti:))
                    .setKeyEquivalent(&ns("\r"));
                f.button("Cancel", 174., 130., 90., t, sel!(closePanel:))
                    .setKeyEquivalent(&ns("\u{1b}"));
                MultiUi {
                    form: f,
                    list,
                    times,
                    multi,
                }
            }))
        })
    }

    // Port of RunMacroDlg::initMacroList.
    fn macro_fill_list(&self) {
        if S.with(|s| s.multi.get().is_none()) {
            return;
        }
        let mut names = vec![];
        S.with(|s| {
            if !s.recording.get() && !s.current.borrow().is_empty() {
                names.push("Current recorded macro".to_string());
            }
            names.extend(s.store.borrow().macros.iter().map(|m| m.name.clone()));
        });
        self.with_multi(|u| {
            u.list.removeAllItems();
            for n in &names {
                u.list.addItemWithTitle(&ns(n));
            }
            u.list.selectItemAtIndex(0);
        });
    }

    pub(crate) fn macro_show_multi(&self) {
        if recording() {
            return;
        }
        self.with_multi(|_| ());
        self.macro_fill_list();
        self.with_multi(|u| u.form.panel.makeKeyAndOrderFront(None));
    }

    pub(crate) fn macro_run_mode(&self) {
        self.with_multi(|u| u.times.setEnabled(panel::on(&u.multi)));
    }

    // Port of the WM_MACRODLGRUNMACRO handler: all runs together make one undo action per document.
    pub(crate) fn macro_run_multi(&self) {
        if recording() {
            return;
        }
        let (idx, times) = self.with_multi(|u| {
            let n = panel::text(&u.times)
                .trim()
                .parse::<usize>()
                .unwrap_or(1)
                .max(1);
            u.times.setStringValue(&ns(&n.to_string()));
            (
                u.list.indexOfSelectedItem(),
                panel::on(&u.multi).then_some(n),
            )
        });
        if idx < 0 {
            return;
        }
        let m = S.with(|s| {
            let cur = !s.current.borrow().is_empty();
            match (cur, idx) {
                (true, 0) => Some(s.current.borrow().clone()),
                (true, i) => s
                    .store
                    .borrow()
                    .macros
                    .get(i as usize - 1)
                    .map(|m| m.steps.clone()),
                (false, i) => s
                    .store
                    .borrow()
                    .macros
                    .get(i as usize)
                    .map(|m| m.steps.clone()),
            }
        });
        let (Some(m), Some(v)) = (m, self.editor()) else {
            return;
        };
        let line = |v: &NSView| {
            let caret = sci::send(v, SCI_GETCURRENTPOS, 0, 0);
            (
                sci::send(v, SCI_GETLINECOUNT, 0, 0),
                sci::send(v, sci::SCI_LINEFROMPOSITION, caret as usize, 0),
            )
        };
        let (count, cur) = line(&v);
        let mut eof = UntilEof::new(count, cur);
        let mut views = vec![];
        let mut n = 0;
        loop {
            self.macro_play(&m, &mut views);
            n += 1;
            match times {
                Some(t) if n >= t => break,
                Some(_) => {}
                None => {
                    let Some(v) = self.editor() else { break };
                    let (count, cur) = line(&v);
                    if !eof.again(count, cur) {
                        break;
                    }
                }
            }
        }
        Self::macro_end_undo(views);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id_of(src: &str, n: &str) -> i32 {
        let l = src
            .lines()
            .find(|l| l.split_whitespace().take(2).eq(["#define", n]))
            .and_then(|l| l.split("//").next())
            .unwrap_or_else(|| panic!("{n}"));
        let v: String = l.split_whitespace().skip(2).collect();
        let v = v.trim_matches(|c| c == '(' || c == ')');
        match v.split_once('+') {
            Some((b, o)) => b.parse().unwrap_or_else(|_| id_of(src, b)) + o.parse::<i32>().unwrap(),
            None => v.parse().unwrap(),
        }
    }

    #[test]
    fn menu_steps() {
        assert_eq!(menu_step("newDocument:", 0), Some(Step::menu(41001)));
        assert_eq!(menu_step("eolConvert:", 2), Some(Step::menu(45002)));
        assert_eq!(menu_step("eolConvert:", 1), Some(Step::menu(45003)));
        assert_eq!(menu_step("cut:", 0), None);
        assert_eq!(menu_step("comment:", 4), None);
        assert_eq!(menu_action(42001), Some(("cut:", ANY)));
        assert_eq!(menu_action(42047), Some(("comment:", 4)));
        assert_eq!(
            menu_step("paste:", 0),
            Some(Step::new(shortcuts::TYPE_L, 2179, 0, 0, ""))
        );
        assert_eq!(menu_step("macroPlayback:", 0), None);
        assert_eq!(menu_step("selectAll:", 0), Some(Step::menu(42007)));
        assert_eq!(menu_step("editOp:", 60), Some(Step::menu(42024)));
        assert_eq!(menu_step("editOp:", 25), Some(Step::menu(42062)));
        assert_eq!(menu_step("selectTab:", 3), Some(Step::menu(44089)));
        assert_eq!(menu_step("selectTab:", 9), None);
        assert_eq!(menu_action(44116), Some(("selectTab:", 9)));
        assert_eq!(menu_step("unfoldLevel:", 7), Some(Step::menu(44068)));
        assert_eq!(menu_step("editOp:", 12), Some(Step::menu(42078)));
        assert_eq!(menu_step("editOp:", 31), Some(Step::menu(42105)));
        assert_eq!(menu_step("sortLocale:", 1), Some(Step::menu(42101)));
        assert_eq!(menu_step("insertDateTimeCustom:", 0), Some(Step::menu(42086)));
        assert_eq!(menu_step("onSelection:", 2), None);
        assert_eq!(menu_step("pasteMarkup:", 0), None);
    }

    #[test]
    fn command_table_matches_sources() {
        let ids = include_str!("../../PowerEditor/src/menuCmdID.h");
        let cmds = include_str!("../../PowerEditor/src/NppCommands.cpp").replace("\r\n", "\n");
        let block = &cmds[cmds
            .find("\tif (_recordingMacro)\n\t\tswitch (id)")
            .unwrap()..];
        let block = &block[..block.find("\n}\n").unwrap()];
        let (rec, rest) = block.split_once("// No need to record").unwrap();
        let (_, rec2) = rest.split_once("// The following 3 commands").unwrap();
        let recorded: Vec<&str> = [rec, rec2]
            .iter()
            .flat_map(|b| b.split("case ").skip(1))
            .map(|c| c.split([' ', ':', '\t']).next().unwrap())
            .collect();
        let menus = [
            include_str!("main.rs"),
            include_str!("view.rs"),
            include_str!("fileops.rs"),
            include_str!("edit.rs"),
            include_str!("edit_extras.rs"),
            include_str!("search_extras.rs"),
            include_str!("language.rs"),
            include_str!("column.rs"),
            include_str!("mark.rs"),
            include_str!("window.rs"),
            include_str!("autoc.rs"),
            include_str!("binary.rs"),
            include_str!("style_dlg.rs"),
            include_str!("udl/mod.rs"),
            include_str!("prefs.rs"),
            include_str!("context_menu.rs"),
            include_str!("views.rs"),
            include_str!("session.rs"),
            include_str!("macros.rs"),
            include_str!("run.rs"),
            include_str!("tools.rs"),
            include_str!("shortcut_mapper.rs"),
        ]
        .concat();
        let table = menu_cmds();
        for c in &table {
            assert_eq!(id_of(ids, &c.name), c.id, "{}", c.name);
            assert!(
                menus.contains(&format!("sel!({})", c.action)),
                "{}",
                c.action
            );
            assert_eq!(
                recorded.contains(&c.name.as_str()),
                !NOT_RECORDED.contains(&c.name.as_str()),
                "{}",
                c.name
            );
            assert_eq!(
                table.iter().filter(|d| d.id == c.id).count(),
                1,
                "{}",
                c.name
            );
            let same =
                |d: &&Cmd| d.action == c.action && (d.tag == c.tag || d.tag == ANY || c.tag == ANY);
            assert_eq!(table.iter().filter(same).count(), 1, "{}", c.name);
        }
        let consts = [
            (include_str!("view.rs"), "NEXT: usize = 11"),
            (include_str!("view.rs"), "PREV: usize = 12"),
            (include_str!("view.rs"), "FIRST: usize = 9"),
            (include_str!("view.rs"), "LAST: usize = 10"),
            (include_str!("view.rs"), "TO_START: usize = 0"),
            (include_str!("view.rs"), "TO_END: usize = 1"),
            (include_str!("view.rs"), "FORWARD: usize = 2"),
            (include_str!("view.rs"), "BACKWARD: usize = 3"),
            (include_str!("edit.rs"), "DEDUP: isize = 1;"),
            (include_str!("edit.rs"), "DEDUP_NEXT: isize = 2;"),
            (include_str!("edit.rs"), "SPLIT: isize = 3;"),
            (include_str!("edit.rs"), "JOIN: isize = 4;"),
            (include_str!("edit.rs"), "RM_EMPTY: isize = 5;"),
            (include_str!("edit.rs"), "RM_BLANK: isize = 6;"),
            (include_str!("edit.rs"), "LINE_ABOVE: isize = 7;"),
            (include_str!("edit.rs"), "LINE_BELOW: isize = 8;"),
            (include_str!("edit.rs"), "REVERSE: isize = 9;"),
            (include_str!("edit.rs"), "INDENT: isize = 10;"),
            (include_str!("edit.rs"), "OUTDENT: isize = 11;"),
            (include_str!("edit.rs"), "RANDOM: isize = 12;"),
            (include_str!("edit_extras.rs"), "OPEN_FILE: isize = 0;\nconst OPEN_FOLDER: isize = 1;\nconst SEARCH_INTERNET: isize = 2;\nconst CHANGE_SEARCH_ENGINE: isize = 3;"),
            (include_str!("edit.rs"), "SORT: isize = 20;"),
            (include_str!("edit.rs"), "CASE: isize = 40;"),
            (include_str!("edit.rs"), "TRIM_TRAIL: isize = 60;"),
            (include_str!("edit.rs"), "TRIM_LEAD: isize = 61;"),
            (include_str!("edit.rs"), "TRIM_BOTH: isize = 62;"),
            (include_str!("edit.rs"), "EOL_TO_SPACE: isize = 63;"),
            (include_str!("edit.rs"), "TRIM_ALL: isize = 64;"),
            (include_str!("edit.rs"), "TAB_TO_SPACE: isize = 65;"),
            (include_str!("edit.rs"), "SPACE_TO_TAB: isize = 66;"),
            (include_str!("edit.rs"), "SPACE_TO_TAB_LEAD: isize = 67;"),
            (include_str!("edit.rs"), "(\"Lexicographically\", Sort::Lex),\n    (\"Lex. %s Ignoring Case\", Sort::LexIgnoreCase),\n    (\"As Integers\", Sort::Integer),\n    (\"As Decimals (Comma)\", Sort::DecimalComma),\n    (\"As Decimals (Dot)\", Sort::DecimalDot),\n    (\"By Length\", Sort::Length),"),
            (include_str!("edit.rs"), "Case::Upper),\n    (\"lowercase\", Case::Lower),\n    (\"Proper Case\", Case::ProperForce),\n    (\"Proper Case (blend)\", Case::ProperBlend),\n    (\"Sentence case\", Case::SentenceForce),\n    (\"Sentence case (blend)\", Case::SentenceBlend),\n    (\"iNVERT cASE\", Case::Invert),\n    (\"ranDOm CasE\", Case::Random),"),
            (include_str!("comment.rs"), "(\"Toggle Single Line Comment\", Cmd::Toggle),\n    (\"Single Line Comment\", Cmd::Comment),\n    (\"Single Line Uncomment\", Cmd::Uncomment),\n    (\"Block Comment\", Cmd::Stream),\n    (\"Block Uncomment\", Cmd::StreamUncomment),"),
            (include_str!("mark.rs"), "STYLE_ALL: isize = 0;"),
            (include_str!("mark.rs"), "STYLE_ONE: isize = 10;"),
            (include_str!("mark.rs"), "CLEAR: isize = 20;"),
            (include_str!("mark.rs"), "UP: isize = 30;"),
            (include_str!("mark.rs"), "DOWN: isize = 40;"),
            (include_str!("mark.rs"), "COPY: isize = 50;"),
            (include_str!("mark.rs"), "ALL: isize = 5;"),
            (include_str!("mark.rs"), "FIND_STYLE: isize = 6;"),
            (include_str!("autoc.rs"), "FUNC_COMPLETION: isize = 0;\nconst WORD_COMPLETION: isize = 1;\nconst PARAMS_HINT: isize = 2;\nconst PREV_HINT: isize = 3;\nconst NEXT_HINT: isize = 4;\nconst PATH_COMPLETION: isize = 5;"),
            (include_str!("edit.rs"), "SCI_CLEAR: u32 = 2180;"),
            (include_str!("edit.rs"), "SCI_LINEDUPLICATE: u32 = 2404;"),
            (include_str!("edit.rs"), "SCI_MOVESELECTEDLINESUP: u32 = 2620;"),
            (include_str!("edit.rs"), "SCI_MOVESELECTEDLINESDOWN: u32 = 2621;"),
        ];
        for (src, c) in consts {
            assert!(src.contains(c), "{c}");
        }
        let se = include_str!("search_extras.rs");
        let names = [
            "SELECT_NEXT",
            "SELECT_PREV",
            "VOLATILE_NEXT",
            "VOLATILE_PREV",
            "RESULTS_WINDOW",
            "NEXT_RESULT",
            "PREV_RESULT",
            "GOTO_BRACE",
            "SELECT_BRACES",
        ];
        for (k, n) in names.iter().enumerate() {
            assert!(se.contains(&format!("const {n}: isize = {k};")), "{n}");
        }
        let names = [
            "TOGGLE_BOOKMARK",
            "NEXT_BOOKMARK",
            "PREV_BOOKMARK",
            "CLEAR_BOOKMARKS",
            "CUT_MARKED",
            "COPY_MARKED",
            "PASTE_MARKED",
            "REMOVE_MARKED",
            "REMOVE_UNMARKED",
            "INVERSE_MARKS",
        ];
        for (k, n) in names.iter().enumerate() {
            assert!(
                se.contains(&format!("const {n}: isize = {};", 20 + k)),
                "{n}"
            );
        }
        for (k, n) in ["NEXT_CHANGE", "PREV_CHANGE", "CLEAR_CHANGES"]
            .iter()
            .enumerate()
        {
            assert!(
                se.contains(&format!("const {n}: isize = {};", 10 + k)),
                "{n}"
            );
        }
        use crate::fileops::Close;
        assert_eq!(
            [
                Close::All,
                Close::ButActive,
                Close::Left,
                Close::Right,
                Close::Unchanged
            ]
            .map(|c| c as isize),
            [0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn file_read_and_write() {
        let dir = std::env::temp_dir().join(format!("npp-macros-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("sub/shortcuts.xml");
        assert_eq!(read_file(&p), Ok(None));
        write_file(&p, "one").unwrap();
        write_file(&p, "two").unwrap();
        assert_eq!(read_file(&p), Ok(Some("two".into())));
        assert!(!p.with_extension("xml.tmp").exists());
        std::fs::write(&p, b"\xff\xfe<").unwrap();
        assert!(read_file(&p).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn run(lines: isize, mut f: impl FnMut(&mut isize, &mut isize)) -> usize {
        let (mut count, mut cur) = (lines, 0);
        let mut e = UntilEof::new(count, cur);
        let mut n = 0;
        loop {
            f(&mut count, &mut cur);
            n += 1;
            if n > 1000 || !e.again(count, cur) {
                return n;
            }
        }
    }

    #[test]
    fn until_end_of_file() {
        assert_eq!(run(5, |c, l| *l = (*l + 1).min(*c - 1)), 5);
        assert_eq!(run(5, |_, _| {}), 1);
        assert_eq!(run(5, |c, _| *c = (*c - 1).max(1)), 5);
        assert_eq!(
            run(3, |c, l| {
                *c += 1;
                *l += 2
            }),
            3
        );
    }
}
