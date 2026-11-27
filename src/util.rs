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

/// A unique folder below the system temp folder for one test, removed again on drop.
#[cfg(test)]
pub(crate) struct TestDir(PathBuf);

#[cfg(test)]
impl TestDir {
    pub(crate) fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("nemotron-bubble-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("the test folder can be created");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_null_appends_terminator() {
        assert_eq!(wide_null("ab"), vec![97, 98, 0]);
        assert_eq!(wide_null(""), vec![0]);
    }

    #[test]
    fn fill_wide_fixed_truncates_and_keeps_terminator() {
        let mut target = [0u16; 4];
        fill_wide_fixed(&mut target, "abcdef");
        assert_eq!(target, [97, 98, 99, 0]);
    }

    #[test]
    fn fill_wide_fixed_clears_previous_content() {
        let mut target = [1u16; 4];
        fill_wide_fixed(&mut target, "a");
        assert_eq!(target, [97, 0, 0, 0]);
    }

    #[test]
    fn lparam_point_keeps_negative_coordinates() {
        // Points on a monitor left of or above the primary one are negative.
        let lparam = (((-5i16 as u16 as u32) << 16) | (-3i16 as u16 as u32)) as LPARAM;
        assert_eq!(lparam_point(lparam), (-3, -5));
        assert_eq!(lparam_point(((20 << 16) | 10) as LPARAM), (10, 20));
    }

    #[test]
    fn loword_takes_lower_16_bits() {
        assert_eq!(loword(0x1234_5678), 0x5678);
    }

    fn read_bytes(name: &str, bytes: &[u8]) -> std::io::Result<String> {
        let dir = TestDir::new(name);
        let path = dir.path().join("file.txt");
        fs::write(&path, bytes).expect("the test file can be written");
        read_text_file(&path)
    }

    #[test]
    fn read_text_file_reads_utf8_without_bom() {
        assert_eq!(read_bytes("utf8", "Grüße".as_bytes()).unwrap(), "Grüße");
    }

    #[test]
    fn read_text_file_strips_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("Grüße".as_bytes());
        assert_eq!(read_bytes("utf8-bom", &bytes).unwrap(), "Grüße");
    }

    #[test]
    fn read_text_file_reads_utf16_le_and_be() {
        let mut little = vec![0xFF, 0xFE];
        let mut big = vec![0xFE, 0xFF];
        for unit in "Grüße".encode_utf16() {
            little.extend_from_slice(&unit.to_le_bytes());
            big.extend_from_slice(&unit.to_be_bytes());
        }
        assert_eq!(read_bytes("utf16-le", &little).unwrap(), "Grüße");
        assert_eq!(read_bytes("utf16-be", &big).unwrap(), "Grüße");
    }

    #[test]
    fn read_text_file_rejects_invalid_utf8() {
        let err = read_bytes("invalid", &[0xC3, 0x28]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
