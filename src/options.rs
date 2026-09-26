//! Which defects a decoder tolerates.

use crate::Code;

/// Which spec deviations a decoder tolerates. Each tolerable [`Code`] is one switch: when it is
/// tolerated the defect is accepted and reported as a [`Severity::Warning`](crate::Severity), and
/// when it is not, the defect is an error and the data is not decoded (or, for the few codes that
/// are about an extra reading rather than a defect, the extra reading is not made).
///
/// [`ParseOptions::LENIENT`] tolerates everything tolerable and is the default;
/// [`ParseOptions::STRICT`] tolerates nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParseOptions {
    tolerated: u64,
}

impl ParseOptions {
    /// Tolerates nothing: exactly what APRS12c allows.
    pub const STRICT: ParseOptions = ParseOptions { tolerated: 0 };

    /// Tolerates every tolerable defect, each with a warning.
    pub const LENIENT: ParseOptions = {
        let mut tolerated = 0u64;
        let mut i = 0;
        while i < Code::ALL.len() {
            if Code::ALL[i].is_tolerable() {
                tolerated |= Code::ALL[i].bit();
            }
            i += 1;
        }
        ParseOptions { tolerated }
    };

    /// Whether `code` is tolerated. Always false for a code that is not tolerable.
    pub const fn tolerates(self, code: Code) -> bool {
        self.tolerated & code.bit() != 0
    }

    /// These options, also tolerating `code` (ignored if the code is not tolerable).
    #[must_use]
    pub const fn with(self, code: Code) -> ParseOptions {
        if code.is_tolerable() { ParseOptions { tolerated: self.tolerated | code.bit() } } else { self }
    }

    /// These options, no longer tolerating `code`.
    #[must_use]
    pub const fn without(self, code: Code) -> ParseOptions {
        ParseOptions { tolerated: self.tolerated & !code.bit() }
    }
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions::LENIENT
    }
}
