//! Text in the information field: UTF-8, with Latin-1 for bytes that are not.

use alloc::string::String;

use crate::Code;
use crate::context::Context;

/// Decodes text. Valid UTF-8 is read as UTF-8; any invalid byte is read as Latin-1, which is a
/// tolerated defect ([`Code::NonUtf8Text`]). A byte below 0x20 or 0x7F is another tolerated defect
/// ([`Code::ControlCharacterInText`]), judged last and once for the text, which is kept as sent.
/// `None` when a defect is not tolerated.
pub(crate) fn decode(ctx: &mut Context, bytes: &[u8], offset: usize) -> Option<String> {
    let text = decode_unjudged(ctx, bytes, offset)?;
    if has_control_character(bytes)
        && !ctx.tolerate(Code::ControlCharacterInText, "text holds a control character (a byte below 0x20 or 0x7F)", Some(offset))
    {
        return None;
    }
    Some(text)
}

/// Like [`decode`], but a control character is not judged (test data and the like).
pub(crate) fn decode_unjudged(ctx: &mut Context, bytes: &[u8], offset: usize) -> Option<String> {
    match core::str::from_utf8(bytes) {
        Ok(text) => Some(String::from(text)),
        Err(_) => {
            if !ctx.tolerate(Code::NonUtf8Text, "text that is not valid UTF-8, read as Latin-1 (UAP 5.16)", Some(offset)) {
                return None;
            }
            // Not UTF-8, so the whole field is read as Latin-1 (UAP 5.16).
            Some(bytes.iter().map(|&b| b as char).collect())
        }
    }
}

/// UTF-8 if valid, otherwise Latin-1; never fails, and reports nothing.
pub(crate) fn for_display(bytes: &[u8]) -> String {
    match core::str::from_utf8(bytes) {
        Ok(text) => String::from(text),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Whether text has a byte below 0x20 or the byte 0x7F (TAB, CR and LF included).
pub(crate) fn has_control_character(bytes: &[u8]) -> bool {
    bytes.iter().any(|&b| b < 0x20 || b == 0x7F)
}

/// Whether text has a CR or LF, which would end the packet on the air or on APRS-IS.
pub(crate) fn has_line_break(bytes: &[u8]) -> bool {
    bytes.iter().any(|&b| b == b'\r' || b == b'\n')
}

/// Whether every byte is printable ASCII (space to `~`).
pub(crate) fn is_printable_ascii(bytes: &[u8]) -> bool {
    bytes.iter().all(|&b| (0x20..=0x7E).contains(&b))
}

pub(crate) fn all_digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

pub(crate) fn digits(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |n, &b| n * 10 + u32::from(b - b'0'))
}
