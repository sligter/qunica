//! Decoding captured child-process output into text.
//!
//! The shell is asked to speak UTF-8 — a `chcp 65001` prefix for `cmd.exe`, an
//! explicit `[Console]::OutputEncoding` for PowerShell — so a strict decode is
//! the fast path and the usual outcome. Legacy console programs ignore that and
//! write raw bytes in the host's ANSI code page anyway.
//!
//! That is not a theoretical concern. On a Simplified-Chinese Windows install
//! the ANSI code page is CP936 (GBK), and the previous
//! `String::from_utf8_lossy` turned every Chinese byte pair into `U+FFFD` — so
//! a `git log` with Chinese commit messages, an `npm` error, or a `dir`
//! listing reached the model as a wall of `���` with no recoverable content.
//!
//! Falling back per line rather than per buffer keeps a stream that mixes both
//! encodings readable: a UTF-8 line decodes as UTF-8 even when the line above it
//! needed the code-page path. Splitting on `\n` is safe because a UTF-8
//! continuation byte is never `0x0A`, so a multi-byte sequence cannot straddle
//! the split.

use std::borrow::Cow;

/// Decode `bytes` captured from a child process.
pub fn decode_output(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes
            .split_inclusive(|byte| *byte == b'\n')
            .map(decode_line)
            .collect(),
    }
}

fn decode_line(line: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(line) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => Cow::Owned(decode_ansi(line)),
    }
}

/// Decode `bytes` using the host's ANSI code page.
#[cfg(windows)]
fn decode_ansi(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::{GetACP, MultiByteToWideChar};

    if bytes.is_empty() {
        return String::new();
    }
    let length = match i32::try_from(bytes.len()) {
        Ok(length) => length,
        Err(_) => return String::from_utf8_lossy(bytes).into_owned(),
    };

    // SAFETY: both calls receive a valid pointer/length pair for `bytes`, and the
    // second writes at most `wide.capacity()` UTF-16 units into `wide`, whose
    // capacity is the size the first call reported.
    unsafe {
        let codepage = GetACP();
        let needed =
            MultiByteToWideChar(codepage, 0, bytes.as_ptr(), length, std::ptr::null_mut(), 0);
        if needed <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide = vec![0u16; needed as usize];
        let written = MultiByteToWideChar(
            codepage,
            0,
            bytes.as_ptr(),
            length,
            wide.as_mut_ptr(),
            needed,
        );
        if written <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        wide.truncate(written as usize);
        String::from_utf16_lossy(&wide)
    }
}

/// POSIX hosts have no ANSI code page; a stream that is not UTF-8 there is
/// genuinely undecodable, so the lossy result stands.
#[cfg(not(windows))]
fn decode_ansi(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_utf8_decodes_unchanged() {
        assert_eq!(decode_output("hello 世界\n".as_bytes()), "hello 世界\n");
    }

    #[test]
    fn empty_input_decodes_to_empty() {
        assert_eq!(decode_output(b""), "");
    }

    #[test]
    fn a_utf8_line_survives_a_neighbouring_undecodable_line() {
        // Line 1 is not valid UTF-8, so the whole buffer takes the fallback path.
        // Line 2 must still come back intact rather than being re-encoded.
        let mut bytes = vec![0xC4, 0xE3, b'\n'];
        bytes.extend_from_slice("keep 世界\n".as_bytes());
        let decoded = decode_output(&bytes);
        assert!(
            decoded.contains("keep 世界"),
            "utf-8 line should survive: {decoded:?}"
        );
        assert_eq!(decoded.lines().count(), 2, "{decoded:?}");
    }

    #[test]
    fn undecodable_bytes_do_not_panic() {
        let decoded = decode_output(&[0xFF, 0xFE, 0xFD]);
        assert!(!decoded.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn code_page_936_bytes_decode_to_readable_text() {
        use windows_sys::Win32::Globalization::GetACP;

        // SAFETY: `GetACP` takes no arguments and only reads process state.
        if unsafe { GetACP() } != 936 {
            // The assertion below is only meaningful where GBK is the ANSI code
            // page; elsewhere the bytes legitimately mean something else.
            return;
        }
        // "你好" in CP936.
        let decoded = decode_output(&[0xC4, 0xE3, 0xBA, 0xC3]);
        assert_eq!(decoded, "你好", "GBK output should not become U+FFFD");
    }
}
