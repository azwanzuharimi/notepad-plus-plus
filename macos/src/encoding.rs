// SPDX-License-Identifier: GPL-3.0-or-later
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Enc {
    Ansi,
    Utf8,
    Utf8Bom,
    Utf16Be,
    Utf16Le,
    Utf16LeNoBom,
    Cp(u32),
}

pub const SC_EOL_CRLF: usize = 0;
pub const SC_EOL_CR: usize = 1;
pub const SC_EOL_LF: usize = 2;

// Notepad++ uses the system ANSI code page; macOS has none, so ANSI is Windows-1252.
const ANSI_CP: u32 = 1252;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
// FileManager reads 128 KiB + 4 blocks and runs uchardet on the first one.
const BLOCK: usize = 128 * 1024 + 4;

// EncodingMapper.cpp encodings table (code page, aliases).
const ALIASES: &[(u32, &str)] = &[
    (1250, "windows-1250"),
    (1251, "windows-1251"),
    (1252, "windows-1252"),
    (1253, "windows-1253"),
    (1254, "windows-1254"),
    (1255, "windows-1255"),
    (1256, "windows-1256"),
    (1257, "windows-1257"),
    (1258, "windows-1258"),
    (
        28591,
        "latin1 ISO_8859-1 ISO-8859-1 CP819 IBM819 csISOLatin1 iso-ir-100 l1",
    ),
    (
        28592,
        "latin2 ISO_8859-2 ISO-8859-2 csISOLatin2 iso-ir-101 l2",
    ),
    (
        28593,
        "latin3 ISO_8859-3 ISO-8859-3 csISOLatin3 iso-ir-109 l3",
    ),
    (
        28594,
        "latin4 ISO_8859-4 ISO-8859-4 csISOLatin4 iso-ir-110 l4",
    ),
    (
        28595,
        "cyrillic ISO_8859-5 ISO-8859-5 csISOLatinCyrillic iso-ir-144",
    ),
    (
        28596,
        "arabic ISO_8859-6 ISO-8859-6 csISOLatinArabic iso-ir-127 ASMO-708 ECMA-114",
    ),
    (
        28597,
        "greek ISO_8859-7 ISO-8859-7 csISOLatinGreek greek8 iso-ir-126 ELOT_928 ECMA-118",
    ),
    (
        28598,
        "hebrew ISO_8859-8 ISO-8859-8 csISOLatinHebrew iso-ir-138",
    ),
    (
        28599,
        "latin5 ISO_8859-9 ISO-8859-9 csISOLatin5 iso-ir-148 l5",
    ),
    (28603, "ISO_8859-13 ISO-8859-13"),
    (
        28604,
        "iso-celtic latin8 ISO_8859-14 ISO-8859-14 18 iso-ir-199",
    ),
    (28605, "Latin-9 ISO_8859-15 ISO-8859-15"),
    (437, "IBM437 cp437 437 csPC8CodePage437"),
    (720, "IBM720 cp720 oem720 720"),
    (737, "IBM737 cp737 oem737 737"),
    (775, "IBM775 cp775 oem775 775"),
    (850, "IBM850 cp850 oem850 850"),
    (852, "IBM852 cp852 oem852 852"),
    (855, "IBM855 cp855 oem855 855 csIBM855"),
    (857, "IBM857 cp857 oem857 857"),
    (858, "IBM858 cp858 oem858 858"),
    (860, "IBM860 cp860 oem860 860"),
    (861, "IBM861 cp861 oem861 861"),
    (862, "IBM862 cp862 oem862 862"),
    (863, "IBM863 cp863 oem863 863"),
    (865, "IBM865 cp865 oem865 865"),
    (866, "IBM866 cp866 oem866 866"),
    (869, "IBM869 cp869 oem869 869"),
    (950, "big5 csBig5"),
    (936, "gb2312 gbk csGB2312 gb18030"),
    (932, "Shift_JIS MS_Kanji csShiftJIS csWindows31J"),
    (949, "windows-949 korean"),
    (51949, "euc-kr csEUCKR"),
    (874, "tis-620"),
    (10007, "x-mac-cyrillic xmaccyrillic"),
    (21866, "koi8_u"),
    (20866, "koi8_r csKOI8R"),
];

// Notepad_plus.rc Encoding > Character sets submenu.
pub const CHARSETS: &[(&str, &[(&str, u32)])] = &[
    (
        "Arabic",
        &[
            ("ISO 8859-6", 28596),
            ("OEM 720", 720),
            ("Windows-1256", 1256),
        ],
    ),
    (
        "Baltic",
        &[
            ("ISO 8859-4", 28594),
            ("ISO 8859-13", 28603),
            ("OEM 775", 775),
            ("Windows-1257", 1257),
        ],
    ),
    ("Celtic", &[("ISO 8859-14", 28604)]),
    (
        "Cyrillic",
        &[
            ("ISO 8859-5", 28595),
            ("KOI8-R", 20866),
            ("KOI8-U", 21866),
            ("Macintosh", 10007),
            ("OEM 855", 855),
            ("OEM 866", 866),
            ("Windows-1251", 1251),
        ],
    ),
    (
        "Central European",
        &[("OEM 852", 852), ("Windows-1250", 1250)],
    ),
    (
        "Chinese",
        &[("Big5 (Traditional)", 950), ("GB2312 (Simplified)", 936)],
    ),
    ("Eastern European", &[("ISO 8859-2", 28592)]),
    (
        "Greek",
        &[
            ("ISO 8859-7", 28597),
            ("OEM 737", 737),
            ("OEM 869", 869),
            ("Windows-1253", 1253),
        ],
    ),
    (
        "Hebrew",
        &[
            ("ISO 8859-8", 28598),
            ("OEM 862", 862),
            ("Windows-1255", 1255),
        ],
    ),
    ("Japanese", &[("Shift-JIS", 932)]),
    ("Korean", &[("Windows 949", 949), ("EUC-KR", 51949)]),
    (
        "North European",
        &[("OEM 861 : Icelandic", 861), ("OEM 865 : Nordic", 865)],
    ),
    ("Thai", &[("TIS-620", 874)]),
    (
        "Turkish",
        &[
            ("ISO 8859-3", 28593),
            ("ISO 8859-9", 28599),
            ("OEM 857", 857),
            ("Windows-1254", 1254),
        ],
    ),
    (
        "Western European",
        &[
            ("ISO 8859-1", 28591),
            ("ISO 8859-15", 28605),
            ("OEM 850", 850),
            ("OEM 858", 858),
            ("OEM 860 : Portuguese", 860),
            ("OEM 863 : French", 863),
            ("OEM-US : CP437", 437),
            ("Windows-1252", 1252),
        ],
    ),
    ("Vietnamese", &[("Windows-1258", 1258)]),
];

#[repr(C)]
struct CFRange {
    location: isize,
    length: isize,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringConvertWindowsCodepageToEncoding(cp: u32) -> u32;
    fn CFStringCreateWithBytes(
        a: *const c_void,
        b: *const u8,
        n: isize,
        e: u32,
        ext: u8,
    ) -> *const c_void;
    fn CFStringGetLength(s: *const c_void) -> isize;
    fn CFStringGetCharacterAtIndex(s: *const c_void, i: isize) -> u16;
    #[allow(clippy::too_many_arguments)]
    fn CFStringGetBytes(
        s: *const c_void,
        r: CFRange,
        e: u32,
        loss: u8,
        ext: u8,
        buf: *mut u8,
        max: isize,
        used: *mut isize,
    ) -> isize;
    fn CFRelease(p: *const c_void);
}

extern "C" {
    fn npp_detect_charset(data: *const c_char, len: usize, out: *mut c_char, out_len: usize);
}

const CF_UTF8: u32 = 0x0800_0100;

pub fn name(e: Enc) -> String {
    match e {
        Enc::Ansi => "ANSI".into(),
        Enc::Utf8 => "UTF-8".into(),
        Enc::Utf8Bom => "UTF-8-BOM".into(),
        Enc::Utf16Be => "UTF-16 BE BOM".into(),
        Enc::Utf16Le => "UTF-16 LE BOM".into(),
        Enc::Utf16LeNoBom => "UTF-16 Little Endian".into(),
        Enc::Cp(cp) => CHARSETS
            .iter()
            .flat_map(|(_, l)| l.iter())
            .find(|(_, c)| *c == cp)
            .map_or(format!("CP{cp}"), |(n, _)| n.to_string()),
    }
}

pub fn eol_name(mode: usize) -> &'static str {
    match mode {
        SC_EOL_CR => "Macintosh (CR)",
        SC_EOL_LF => "Unix (LF)",
        _ => "Windows (CR LF)",
    }
}

fn cf_encoding(cp: u32) -> Option<u32> {
    Some(unsafe { CFStringConvertWindowsCodepageToEncoding(cp) }).filter(|&e| e != 0xFFFF_FFFF)
}

fn cf_string(b: &[u8], e: u32) -> Option<*const c_void> {
    let s =
        unsafe { CFStringCreateWithBytes(std::ptr::null(), b.as_ptr(), b.len() as isize, e, 0) };
    (!s.is_null()).then_some(s)
}

// Converts s[from..] to encoding e until the end or the first character e cannot store.
fn cf_bytes(s: *const c_void, from: isize, e: u32, out: &mut Vec<u8>) -> isize {
    let len = unsafe { CFStringGetLength(s) };
    let r = || CFRange {
        location: from,
        length: len - from,
    };
    let mut used = 0;
    let n = unsafe { CFStringGetBytes(s, r(), e, 0, 0, std::ptr::null_mut(), 0, &mut used) };
    let start = out.len();
    out.resize(start + used as usize, 0);
    unsafe { CFStringGetBytes(s, r(), e, 0, 0, out[start..].as_mut_ptr(), used, &mut used) };
    from + n
}

pub fn codepage_from_name(cs: &str) -> Option<u32> {
    let has = |list: &str| list.split(' ').any(|w| w.eq_ignore_ascii_case(cs));
    if has("utf-8 utf8") {
        return Some(65001);
    }
    ALIASES.iter().find(|(_, l)| has(l)).map(|(cp, _)| *cp)
}

// FileManager::detectCodepage: uchardet result, but TIS-620 is ignored.
fn detect_codepage(b: &[u8]) -> Option<u32> {
    let mut out = [0 as c_char; 64];
    unsafe {
        npp_detect_charset(
            b.as_ptr() as *const c_char,
            b.len(),
            out.as_mut_ptr(),
            out.len(),
        )
    };
    let cs = unsafe { CStr::from_ptr(out.as_ptr()) }.to_string_lossy();
    (!cs.eq_ignore_ascii_case("TIS-620")).then(|| codepage_from_name(&cs))?
}

fn bom(b: &[u8]) -> Option<Enc> {
    if b.starts_with(b"\xFE\xFF") {
        Some(Enc::Utf16Be)
    } else if b.starts_with(b"\xFF\xFE") {
        Some(Enc::Utf16Le)
    } else if b.starts_with(UTF8_BOM) {
        Some(Enc::Utf8Bom)
    } else {
        None
    }
}

// ponytail: simple stand-in for Win32 IsTextUnicode(IS_TEXT_UNICODE_STATISTICS); more than half the high bytes must be zero.
fn looks_utf16le(b: &[u8]) -> bool {
    let zero_high = b.iter().skip(1).step_by(2).filter(|&&c| c == 0).count();
    zero_high * 2 > b.len() / 2
}

// Utf8_16_Read::utf8_7bits_8bits: NUL or an invalid sequence means 8 bits.
fn utf8_7bits_8bits(b: &[u8]) -> Enc {
    if !b.contains(&0) && std::str::from_utf8(b).is_ok() {
        Enc::Utf8
    } else {
        Enc::Ansi
    }
}

// Order of FileManager::loadFileData: BOM, uchardet, then the Utf8_16_Read rules (7 bit text opens as UTF-8).
pub fn detect(b: &[u8]) -> Enc {
    if let Some(e) = bom(b) {
        return e;
    }
    match detect_codepage(&b[..b.len().min(BLOCK)]) {
        Some(65001) => Enc::Utf8,
        Some(cp) => Enc::Cp(cp),
        None if b.len() > 1
            && b.len().is_multiple_of(2)
            && b[0] != 0
            && b[1] == 0
            && looks_utf16le(b) =>
        {
            Enc::Utf16LeNoBom
        }
        None => utf8_7bits_8bits(b),
    }
}

// Buffer.cpp getEOLFormatForm: the first line end in the file decides.
pub fn detect_eol(text: &[u8]) -> Option<usize> {
    let i = text.iter().position(|&c| c == b'\r' || c == b'\n')?;
    Some(match (text[i], text.get(i + 1)) {
        (b'\r', Some(b'\n')) => SC_EOL_CRLF,
        (b'\r', _) => SC_EOL_CR,
        _ => SC_EOL_LF,
    })
}

const MULTI_BYTE: [u32; 5] = [932, 936, 949, 950, 51949];

pub fn supported(cp: u32) -> bool {
    cp == 858 || cf_encoding(cp).is_some()
}

struct Table {
    dec: [char; 256],
    enc: HashMap<char, u8>,
}

// One char per byte, as MultiByteToWideChar gives. A byte with no character maps to U+00XX,
// or to the private use U+F7XX when a real byte already gives U+00XX.
fn build_table(cp: u32) -> Option<Table> {
    let e = cf_encoding(if cp == 858 { 850 } else { cp })?;
    let mut real: [Option<char>; 256] = [None; 256];
    for (i, c) in real.iter_mut().enumerate() {
        *c = cf_string(&[i as u8], e).and_then(|s| {
            let mut out = vec![];
            cf_bytes(s, 0, CF_UTF8, &mut out);
            unsafe { CFRelease(s) };
            String::from_utf8(out).ok().and_then(|x| x.chars().next())
        });
    }
    if cp == 858 {
        real[0xD5] = Some('\u{20AC}');
    }
    let mut enc = HashMap::new();
    for (i, c) in real.iter().enumerate().rev() {
        if let Some(c) = c {
            enc.insert(*c, i as u8);
        }
    }
    let mut dec = ['\0'; 256];
    for (i, c) in dec.iter_mut().enumerate() {
        *c = real[i].unwrap_or_else(|| {
            let latin = char::from(i as u8);
            let f = if enc.contains_key(&latin) {
                char::from_u32(0xF700 + i as u32).unwrap()
            } else {
                latin
            };
            enc.insert(f, i as u8);
            f
        });
    }
    Some(Table { dec, enc })
}

fn byte_table(cp: u32) -> Option<&'static Table> {
    static CACHE: OnceLock<Mutex<HashMap<u32, Option<&'static Table>>>> = OnceLock::new();
    if MULTI_BYTE.contains(&cp) {
        return None;
    }
    let mut c = CACHE.get_or_init(Default::default).lock().unwrap();
    *c.entry(cp)
        .or_insert_with(|| build_table(cp).map(|t| &*Box::leak(Box::new(t))))
}

// Returns the UTF-8 text and true when some bytes could not be read.
fn decode_cp(b: &[u8], cp: u32) -> (Vec<u8>, bool) {
    if let Some(t) = byte_table(cp) {
        let s: String = b.iter().map(|&c| t.dec[c as usize]).collect();
        return (s.into_bytes(), false);
    }
    let Some(e) = cf_encoding(cp) else {
        return (String::from_utf8_lossy(b).into_owned().into_bytes(), true);
    };
    let to_utf8 = |b: &[u8], out: &mut Vec<u8>| {
        let s = cf_string(b, e)?;
        cf_bytes(s, 0, CF_UTF8, out);
        unsafe { CFRelease(s) };
        Some(())
    };
    let (mut out, mut lost, mut rest) = (vec![], false, b);
    // One bad sequence makes CoreFoundation reject all bytes, so decode the longest good run and skip one byte.
    while to_utf8(rest, &mut out).is_none() {
        let mut ends = vec![0];
        let mut i = 0;
        while i < rest.len() {
            i += if lead_byte(cp, rest[i]) { 2 } else { 1 };
            ends.push(i.min(rest.len()));
        }
        let (mut lo, mut hi) = (0, ends.len() - 1);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            match cf_string(&rest[..ends[mid]], e) {
                Some(s) => {
                    unsafe { CFRelease(s) };
                    lo = mid;
                }
                None => hi = mid - 1,
            }
        }
        to_utf8(&rest[..ends[lo]], &mut out);
        out.extend("\u{FFFD}".as_bytes());
        lost = true;
        rest = &rest[ends[lo] + 1..];
    }
    (out, lost)
}

fn lead_byte(cp: u32, c: u8) -> bool {
    match cp {
        932 => matches!(c, 0x81..=0x9F | 0xE0..=0xFC),
        51949 => (0xA1..=0xFE).contains(&c),
        _ => (0x81..=0xFE).contains(&c),
    }
}

fn decode_utf16(b: &[u8], be: bool) -> (Vec<u8>, bool) {
    let units = b.chunks_exact(2).map(|c| {
        if be {
            u16::from_be_bytes([c[0], c[1]])
        } else {
            u16::from_le_bytes([c[0], c[1]])
        }
    });
    let mut lost = !b.len().is_multiple_of(2);
    let s: String = char::decode_utf16(units)
        .map(|c| {
            c.unwrap_or_else(|_| {
                lost = true;
                char::REPLACEMENT_CHARACTER
            })
        })
        .collect();
    (s.into_bytes(), lost)
}

// File bytes to the UTF-8 text Scintilla holds, and true when some bytes could not be read.
// Only the BOM of `e` itself is skipped.
pub fn decode(b: &[u8], e: Enc) -> (Vec<u8>, bool) {
    let strip = |bom: &[u8]| b.strip_prefix(bom).unwrap_or(b);
    match e {
        Enc::Utf8 => (b.to_vec(), false),
        Enc::Utf8Bom => (strip(UTF8_BOM).to_vec(), false),
        Enc::Utf16Be => decode_utf16(strip(b"\xFE\xFF"), true),
        Enc::Utf16Le => decode_utf16(strip(b"\xFF\xFE"), false),
        Enc::Utf16LeNoBom => decode_utf16(b, false),
        Enc::Ansi => decode_cp(b, ANSI_CP),
        Enc::Cp(cp) => decode_cp(b, cp),
    }
}

pub fn load(b: &[u8]) -> (Enc, Vec<u8>, bool) {
    let e = detect(b);
    let (text, lost) = decode(b, e);
    (e, text, lost)
}

// "Encode in" between ANSI and a Unicode mode: Notepad++ changes only the Scintilla code page,
// so the same bytes are read again. True when the save can lose some of them.
pub fn reinterpret(text: &[u8], from: Enc, to: Enc) -> (Vec<u8>, bool) {
    let (raw, mut lost) = match encode(text, Enc::Ansi, false) {
        _ if from != Enc::Ansi => (text.to_vec(), false),
        Ok(b) => (b, false),
        Err(_) => (encode(text, Enc::Ansi, true).unwrap_or_default(), true),
    };
    if to == Enc::Ansi {
        return decode(&raw, Enc::Ansi);
    }
    // Invalid UTF-8 stays as raw bytes, which Scintilla shows as hex blobs; only UTF-16 cannot save them.
    lost |= matches!(to, Enc::Utf16Be | Enc::Utf16Le) && std::str::from_utf8(&raw).is_err();
    (raw, lost)
}

fn encode_cp(s: &str, cp: u32, out: &mut Vec<u8>) -> usize {
    if let Some(t) = byte_table(cp) {
        let mut bad = 0;
        for c in s.chars() {
            out.push(t.enc.get(&c).copied().unwrap_or_else(|| {
                bad += 1;
                b'?'
            }));
        }
        return bad;
    }
    let Some((e, cf)) = cf_encoding(cp).and_then(|e| Some((e, cf_string(s.as_bytes(), CF_UTF8)?)))
    else {
        return s.chars().count();
    };
    let len = unsafe { CFStringGetLength(cf) };
    let (mut pos, mut bad) = (0, 0);
    loop {
        pos = cf_bytes(cf, pos, e, out);
        if pos >= len {
            break;
        }
        bad += 1;
        out.push(b'?');
        let c = unsafe { CFStringGetCharacterAtIndex(cf, pos) };
        pos += if (0xD800..0xDC00).contains(&c) { 2 } else { 1 };
    }
    unsafe { CFRelease(cf) };
    bad
}

// Scintilla text to file bytes; Err(n) when n characters cannot be stored and `lossy` is off.
pub fn encode(text: &[u8], e: Enc, lossy: bool) -> Result<Vec<u8>, usize> {
    let mut out = match e {
        Enc::Utf8 => return Ok(text.to_vec()),
        Enc::Utf8Bom => return Ok([UTF8_BOM, text].concat()),
        Enc::Utf16Be => b"\xFE\xFF".to_vec(),
        Enc::Utf16Le => b"\xFF\xFE".to_vec(),
        _ => vec![],
    };
    let mut bad = 0;
    for chunk in text.utf8_chunks() {
        let s = chunk.valid();
        match e {
            Enc::Utf16Be => s.encode_utf16().for_each(|u| out.extend(u.to_be_bytes())),
            Enc::Utf16Le | Enc::Utf16LeNoBom => {
                s.encode_utf16().for_each(|u| out.extend(u.to_le_bytes()))
            }
            Enc::Cp(cp) => bad += encode_cp(s, cp, &mut out),
            _ => bad += encode_cp(s, ANSI_CP, &mut out),
        }
        if !chunk.invalid().is_empty() {
            bad += 1;
            match e {
                Enc::Utf16Be => out.extend(b"\0?"),
                Enc::Utf16Le | Enc::Utf16LeNoBom => out.extend(b"?\0"),
                _ => out.push(b'?'),
            }
        }
    }
    if bad > 0 && !lossy {
        Err(bad)
    } else {
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RU: &str =
        "Привет, как дела? Это тестовый файл в кодировке Windows-1251. Москва, Россия.\r\n\
        Ёжик в тумане. Широкая электрификация южных губерний.\r\n";
    const JA: &str = "いろはにほへと ちりぬるを わかよたれそ つねならむ うゐのおくやま けふこえて \
        あさきゆめみし ゑひもせす。日本語の文章を正しく判定できるか確認します。\r\n";

    fn cp_bytes(s: &str, cp: u32) -> Vec<u8> {
        encode(s.as_bytes(), Enc::Cp(cp), false).unwrap()
    }

    #[test]
    fn bom_detection() {
        assert_eq!(detect(b"\xEF\xBB\xBFhi"), Enc::Utf8Bom);
        assert_eq!(detect(b"\xFF\xFEh\0i\0"), Enc::Utf16Le);
        assert_eq!(detect(b"\xFE\xFF\0h\0i"), Enc::Utf16Be);
        assert_eq!(load(b"\xFF\xFEh\0i\0").1, b"hi");
        assert_eq!(load(b"\xFE\xFF\0h\0i").1, b"hi");
        assert_eq!(load(b"\xEF\xBB\xBFhi").1, b"hi");
        assert_eq!(decode(b"\xEF\xBB\xBFhi", Enc::Utf8).0, b"\xEF\xBB\xBFhi");
    }

    #[test]
    fn utf8_without_bom() {
        assert_eq!(detect(b""), Enc::Utf8);
        assert_eq!(detect(b"plain ascii\n"), Enc::Utf8);
        assert_eq!(
            detect("caf\u{e9} na\u{ef}ve r\u{e9}sum\u{e9}\n".as_bytes()),
            Enc::Utf8
        );
        assert_eq!(detect(RU.as_bytes()), Enc::Utf8);
    }

    #[test]
    fn utf16le_without_bom() {
        let b: Vec<u8> = "hello world\r\n"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(detect(&b), Enc::Utf16LeNoBom);
        assert_eq!(load(&b).1, b"hello world\r\n");
    }

    #[test]
    fn uchardet_codepages() {
        assert_eq!(detect(&cp_bytes(&RU.repeat(3), 1251)), Enc::Cp(1251));
        assert_eq!(detect(&cp_bytes(&JA.repeat(3), 932)), Enc::Cp(932));
        assert_eq!(codepage_from_name("WINDOWS-1251"), Some(1251));
        assert_eq!(codepage_from_name("SHIFT_JIS"), Some(932));
        assert_eq!(codepage_from_name("UTF-8"), Some(65001));
        assert_eq!(codepage_from_name("EUC-JP"), None);
    }

    #[test]
    fn invalid_utf8_is_ansi() {
        assert_eq!(utf8_7bits_8bits(b"a\xE9b"), Enc::Ansi);
        assert_eq!(utf8_7bits_8bits(b"a\0b"), Enc::Ansi);
        assert_eq!(decode(b"caf\xE9", Enc::Ansi).0, "caf\u{e9}".as_bytes());
    }

    #[test]
    fn eol_detection() {
        assert_eq!(detect_eol(b"a\r\nb\nc"), Some(SC_EOL_CRLF));
        assert_eq!(detect_eol(b"a\nb\r\n"), Some(SC_EOL_LF));
        assert_eq!(detect_eol(b"a\rb\r\n"), Some(SC_EOL_CR));
        assert_eq!(detect_eol(b"a\r"), Some(SC_EOL_CR));
        assert_eq!(detect_eol(b"abc"), None);
    }

    #[test]
    fn round_trip() {
        let text = "Hello \u{e9}\u{20ac} \u{1F600}\r\nline 2\n";
        for e in [
            Enc::Utf8,
            Enc::Utf8Bom,
            Enc::Utf16Be,
            Enc::Utf16Le,
            Enc::Utf16LeNoBom,
        ] {
            let file = encode(text.as_bytes(), e, false).unwrap();
            assert_eq!(load(&file), (e, text.as_bytes().to_vec(), false), "{e:?}");
        }
        for (s, e) in [
            ("caf\u{e9} \u{20ac}\r\n", Enc::Ansi),
            (RU, Enc::Cp(1251)),
            (JA, Enc::Cp(932)),
            ("\u{3b1}\u{3b2}\u{3b3}", Enc::Cp(28597)),
        ] {
            let file = encode(s.as_bytes(), e, false).unwrap();
            let (got, text, _) = load(&file);
            assert_eq!(
                encode(&decode(&file, e).0, e, false).unwrap(),
                file,
                "{e:?}"
            );
            if got == e {
                assert_eq!(text, s.as_bytes());
            }
        }
        let raw = b"\x80\x81\x8D\x8F\x90\x9D\xFF";
        assert_eq!(
            encode(&decode(raw, Enc::Ansi).0, Enc::Ansi, false).unwrap(),
            raw
        );
    }

    #[test]
    fn unmappable_characters() {
        let s = "abc Привет \u{1F600}".as_bytes();
        assert_eq!(encode(s, Enc::Ansi, false), Err(7));
        assert_eq!(encode(s, Enc::Ansi, true).unwrap(), b"abc ?????? ?");
        assert_eq!(encode(s, Enc::Cp(1251), false), Err(1));
        assert_eq!(encode(s, Enc::Cp(932), false), Err(1));
        assert_eq!(
            encode("\u{e9}\u{1F600}".as_bytes(), Enc::Cp(932), true).unwrap(),
            b"??"
        );
        assert_eq!(encode(b"a\xFFb", Enc::Utf16Le, false), Err(1));
        assert!(encode(s, Enc::Utf16Le, false).is_ok());
        assert_eq!(encode(b"a\xFFb", Enc::Utf8, false).unwrap(), b"a\xFFb");
    }

    #[test]
    fn every_menu_codepage_converts() {
        for (_, list) in CHARSETS {
            for (n, cp) in *list {
                assert_eq!(supported(*cp), *cp != 720, "{n} {cp}");
            }
        }
        assert_eq!(name(Enc::Cp(1251)), "Windows-1251");
        assert_eq!(name(Enc::Cp(932)), "Shift-JIS");
        assert_eq!(decode(b"\xD5", Enc::Cp(858)).0, "\u{20AC}".as_bytes());
        assert_eq!(decode(b"\xD5", Enc::Cp(850)).0, "\u{131}".as_bytes());
    }

    #[test]
    fn every_single_byte_round_trips() {
        let all: Vec<u8> = (0..=255).collect();
        let cps = CHARSETS
            .iter()
            .flat_map(|(_, l)| l.iter().map(|(_, cp)| *cp));
        for cp in cps
            .chain([ANSI_CP])
            .filter(|cp| supported(*cp) && !MULTI_BYTE.contains(cp))
        {
            let (text, lost) = decode(&all, Enc::Cp(cp));
            assert!(!lost, "{cp}");
            assert_eq!(encode(&text, Enc::Cp(cp), false).unwrap(), all, "{cp}");
        }
        assert_eq!(decode(b"\xE5", Enc::Cp(857)).0, "\u{d5}".as_bytes());
        assert_eq!(decode(b"\xD5", Enc::Cp(857)).0, "\u{f7d5}".as_bytes());
    }

    #[test]
    fn decode_reports_loss() {
        assert!(decode(b"\xFF\xFEa\0b", Enc::Utf16Le).1);
        assert!(decode(b"\xFF\xFE\x00\xD8a\0", Enc::Utf16Le).1);
        assert!(decode(b"\xFE\xFF\xDC\x00", Enc::Utf16Be).1);
        assert!(!decode(b"\xFF\xFEa\0", Enc::Utf16Le).1);
        assert!(decode(b"\x82\xA0\xFF\xFF", Enc::Cp(932)).1);
        assert!(!decode(b"a\xFFb", Enc::Utf8).1);
    }

    #[test]
    fn invalid_utf8_to_utf16_keeps_alignment() {
        assert_eq!(
            encode(b"a\xFFb", Enc::Utf16Le, true).unwrap(),
            b"\xFF\xFEa\0?\0b\0"
        );
        assert_eq!(
            encode(b"a\xFFb", Enc::Utf16Be, true).unwrap(),
            b"\xFE\xFF\0a\0?\0b"
        );
        assert_eq!(encode(b"a\xFFb", Enc::Utf16Le, false), Err(1));
    }

    #[test]
    fn multi_byte_decodes_around_bad_byte() {
        let mut b = cp_bytes(JA, 932);
        let mid = cp_bytes("いろは", 932).len();
        b.insert(mid, 0xA0);
        let (text, lost) = decode(&b, Enc::Cp(932));
        let want = format!("いろは\u{FFFD}{}", &JA["いろは".len()..]);
        assert_eq!((String::from_utf8(text).unwrap(), lost), (want, true));
        let (text, lost) = decode(b"\x82\xA0\x81 \xA0\x82", Enc::Cp(932));
        assert_eq!(
            (text.as_slice(), lost),
            ("あ\u{FFFD} \u{FFFD}\u{FFFD}".as_bytes(), true)
        );
        assert_eq!(
            decode(&cp_bytes(JA, 936), Enc::Cp(936)),
            (JA.as_bytes().to_vec(), false)
        );
    }

    #[test]
    fn ansi_reinterprets_bytes() {
        let (e, text, _) = load(b"caf\xC3\xA9 \xFF");
        assert_eq!(e, Enc::Ansi);
        assert_eq!(text, "caf\u{c3}\u{a9} \u{ff}".as_bytes());
        let (utf8, lost) = reinterpret(&text, Enc::Ansi, Enc::Utf8);
        assert_eq!(
            (utf8.as_slice(), lost),
            (b"caf\xC3\xA9 \xFF".as_slice(), false)
        );
        assert!(reinterpret(&text, Enc::Ansi, Enc::Utf16Le).1);
        assert_eq!(
            reinterpret(&utf8, Enc::Utf8, Enc::Ansi),
            (text.clone(), false)
        );
        assert_eq!(
            reinterpret("\u{e9}".as_bytes(), Enc::Utf8Bom, Enc::Ansi).0,
            "\u{c3}\u{a9}".as_bytes()
        );
        assert_eq!(
            reinterpret("\u{e9}".as_bytes(), Enc::Ansi, Enc::Utf8).0,
            b"\xE9"
        );
        assert!(reinterpret("\u{416}".as_bytes(), Enc::Ansi, Enc::Utf8).1);
    }
}
