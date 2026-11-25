//! Small Win32 helpers (wide strings, message parameters).

use crate::*;

pub(crate) fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) fn fill_wide_fixed<const N: usize>(target: &mut [u16; N], text: &str) {
    target.fill(0);
    for (slot, value) in target
        .iter_mut()
        .take(N.saturating_sub(1))
        .zip(text.encode_utf16())
    {
        *slot = value;
    }
}

pub(crate) fn lparam_point(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam & 0xffff) as i16 as i32;
    let y = ((lparam >> 16) & 0xffff) as i16 as i32;
    (x, y)
}

pub(crate) fn loword(value: usize) -> u16 {
    (value & 0xffff) as u16
}


/// Reads a text file regardless of how an editor saved it: UTF-8 with or without
/// a byte order mark, or UTF-16 (LE/BE) with one, as Notepad's "Unicode" does.
/// The app itself always writes UTF-8 without a BOM.
pub(crate) fn read_text_file(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let utf16 = |bytes: &[u8], from: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| from([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    };
    match bytes.as_slice() {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8(rest.to_vec())
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ => String::from_utf8(bytes)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
    }
}
