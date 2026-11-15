//! Hostile input: a decoder fed untrusted text must stay linear in its size. Each test compares
//! the time at two sizes, a ratio and not a clock limit: a decode that is quadratic in the comment
//! takes about sixteen times as long at four times the size, a linear one about four.

use pdn_aprs::{Packet, ParseOptions};
use std::time::Instant;

const SMALL: usize = 40_000;
const LARGE: usize = 160_000;
/// Four times the size: linear is 4, quadratic 16. Well past linear, well short of quadratic.
const MAX_RATIO: f64 = 9.0;

const ELEMENTS: [&[u8]; 10] = [b"{", b"PHG1", b"|!!|", b"/A=000001", b"!W12!", b"\"4e}", b"|", b"|!", b"|!!!!", b"|!!!!|!W12!|a b"];

fn run(header: &[u8], element: &[u8], size: usize, options: ParseOptions) -> f64 {
    let mut line = header.to_vec();
    while line.len() < header.len() + size {
        line.extend_from_slice(element);
    }
    // The best of a few runs keeps a busy machine out of the ratio.
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let packet = Packet::decode_tnc2(&line, options).unwrap();
            std::hint::black_box(&packet);
            start.elapsed().as_secs_f64()
        })
        .fold(f64::MAX, f64::min)
}

fn assert_linear(header: &[u8]) {
    for element in ELEMENTS {
        for options in [ParseOptions::LENIENT, ParseOptions::STRICT] {
            // A floor under the small time, so a fast machine's clock resolution does not decide.
            let small = run(header, element, SMALL, options).max(2e-4);
            let large = run(header, element, LARGE, options);
            assert!(
                large / small < MAX_RATIO,
                "{} repeated in {}: {SMALL} bytes took {small:.4} s, {LARGE} took {large:.4} s",
                String::from_utf8_lossy(element),
                String::from_utf8_lossy(header),
            );
        }
    }
}

#[test]
fn an_uncompressed_position_comment_decodes_in_linear_time() {
    assert_linear(b"N0CALL>APZ001:!5130.00N/00005.00W-");
}

/// A signpost symbol reads braces from the comment.
#[test]
fn a_signpost_comment_decodes_in_linear_time() {
    assert_linear(b"N0CALL>APZ001:!4903.50N\\07201.75Wm");
}

#[test]
fn mic_e_status_text_decodes_in_linear_time() {
    assert_linear(b"N0CALL>S32UVT:`(_fn\"Oj/`");
}
