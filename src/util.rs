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

