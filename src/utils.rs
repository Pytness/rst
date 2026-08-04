use crate::BETWEEN;

pub fn is_control_c0(c: char) -> bool {
    BETWEEN!(c, '\0', '\u{1F}') || c == '\u{7F}'
}

pub fn is_control_c1(c: char) -> bool {
    BETWEEN!(c, '\u{80}', '\u{9F}')
}

pub fn is_control(c: char) -> bool {
    is_control_c0(c) || is_control_c1(c)
}

/// Decodes a single Unicode scalar value from the start of `buffer`.
///
/// On success, returns the decoded `char` along with the number of bytes it
/// occupied in `buffer`.
///
/// If `buffer` starts with an invalid or malformed UTF-8 sequence, returns
/// [`char::REPLACEMENT_CHARACTER`] along with the number of bytes that
/// sequence should be skipped.
///
/// Returns `None` if `buffer` is empty or it starts with a truncated and
/// potentially valid sequence once more bytes arrive.
pub fn utf8decode(buffer: &[u8]) -> Option<(char, usize)> {
    let probe = &buffer[..buffer.len().min(4)];

    match std::str::from_utf8(probe) {
        Ok(s) => {
            let c = s.chars().next()?;
            Some((c, c.len_utf8()))
        }
        Err(e) if e.valid_up_to() > 0 => {
            let valid_bytes = &probe[..e.valid_up_to()];
            let utf = unsafe { std::str::from_utf8_unchecked(valid_bytes) };

            let c = utf.chars().next()?;

            Some((c, c.len_utf8()))
        }
        Err(e) => match e.error_len() {
            Some(len) => Some((std::char::REPLACEMENT_CHARACTER, len)),
            None => None,
        },
    }
}
