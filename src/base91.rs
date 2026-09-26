//! Base-91 numbers: each byte is a digit, `!` (33) to `{` (123).

pub(crate) fn is_digit(b: u8) -> bool {
    (33..=123).contains(&b)
}

/// The value of a run of base-91 digits, most significant first.
pub(crate) fn decode(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0u32, |n, &b| is_digit(b).then(|| n * 91 + u32::from(b - 33)))
}

/// `value` as `width` base-91 digits, most significant first.
pub(crate) fn encode(mut value: u32, width: usize, out: &mut alloc::vec::Vec<u8>) {
    let start = out.len();
    for _ in 0..width {
        out.push(0);
    }
    for i in (0..width).rev() {
        out[start + i] = (value % 91) as u8 + 33;
        value /= 91;
    }
}
