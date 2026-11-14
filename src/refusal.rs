//! Refusal reason codes: why an encoder or builder would not write the data as given.

use alloc::format;
use core::fmt;

/// Why data was refused. The ids match the `refusals` list in the conformance vectors' `codes.json`.
///
/// New codes may be added in later 1.x releases, so match with a wildcard arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RefusalCode {
    /// A header address is not 1-9 letters, digits or `-` in TNC2 text.
    InvalidAddress,
    /// An AX.25 frame cannot carry the header: a call over 6 characters or in lower case, an SSID over 15 or with a leading zero, more than 8 path entries.
    Ax25CannotCarry,
    /// A TNC2 line cannot carry the packet: its information field is not UTF-8 text.
    Tnc2CannotCarry,
    /// A third-party packet's inner header cannot be written as received.
    InvalidThirdPartyHeader,
    /// A message, bulletin, telemetry metadata or directed query addressee is not the form its report needs.
    InvalidAddressee,
    /// An object or item name is the wrong length or holds a character a name cannot.
    InvalidName,
    /// A message ID is empty, too long or not letters and digits, or an ack or reject carries the wrong kind of ID.
    InvalidMessageId,
    /// Text starts or ends with a space that would be read back as padding.
    PaddingSpace,
    /// Free text or a list is longer than its field allows, or the information field is over 256 bytes.
    TooLong,
    /// Text in a fixed-form field is not that form (a locator, a bit sense, a capability token).
    Malformed,
    /// Free text holds a character its field cannot carry, such as a control character (a byte below 0x20 or 0x7F).
    InvalidCharacter,
    /// Free text would read back as a structured element and no clean form avoids it.
    TextReadsAsField,
    /// A value is NaN or infinite.
    NotANumber,
    /// A number is outside what its field holds, after rounding.
    ValueOutOfRange,
    /// A value inside the range that no form reads back as.
    ValueHasNoForm,
    /// A builder input was given in two units.
    ValueGivenTwice,
    /// A timestamp has a field out of range, or is a format the report does not take.
    InvalidTimestamp,
    /// The data lacks something its report or another field needs.
    MissingField,
    /// The symbol table or code is not a valid symbol.
    InvalidSymbol,
    /// The symbol disagrees with the data.
    SymbolMismatch,
    /// Two things want the one data extension or the compressed cs bytes.
    ExtensionConflict,
    /// The report has no place for a field, or it contradicts another.
    FieldNotAllowed,
    /// An enumerated value is outside its set.
    InvalidCode,
    /// An NMEA sentence is malformed, its checksum does not match, or a field given beside it disagrees.
    InvalidNmea,
    /// The call has nothing it can write.
    NotEncodable,
    /// Fallback: the library's own read-back check found the bytes decode to different data and no rule names why.
    DoesNotReadBack,
}

impl RefusalCode {
    /// Every code, in the order of the vectors' `refusals` list.
    pub const ALL: [RefusalCode; 26] = [
        RefusalCode::InvalidAddress,
        RefusalCode::Ax25CannotCarry,
        RefusalCode::Tnc2CannotCarry,
        RefusalCode::InvalidThirdPartyHeader,
        RefusalCode::InvalidAddressee,
        RefusalCode::InvalidName,
        RefusalCode::InvalidMessageId,
        RefusalCode::PaddingSpace,
        RefusalCode::TooLong,
        RefusalCode::Malformed,
        RefusalCode::InvalidCharacter,
        RefusalCode::TextReadsAsField,
        RefusalCode::NotANumber,
        RefusalCode::ValueOutOfRange,
        RefusalCode::ValueHasNoForm,
        RefusalCode::ValueGivenTwice,
        RefusalCode::InvalidTimestamp,
        RefusalCode::MissingField,
        RefusalCode::InvalidSymbol,
        RefusalCode::SymbolMismatch,
        RefusalCode::ExtensionConflict,
        RefusalCode::FieldNotAllowed,
        RefusalCode::InvalidCode,
        RefusalCode::InvalidNmea,
        RefusalCode::NotEncodable,
        RefusalCode::DoesNotReadBack,
    ];

    /// The kebab-case id used in the conformance vectors, e.g. `value-out-of-range`.
    pub const fn id(self) -> &'static str {
        match self {
            RefusalCode::InvalidAddress => "invalid-address",
            RefusalCode::Ax25CannotCarry => "ax25-cannot-carry",
            RefusalCode::Tnc2CannotCarry => "tnc2-cannot-carry",
            RefusalCode::InvalidThirdPartyHeader => "invalid-third-party-header",
            RefusalCode::InvalidAddressee => "invalid-addressee",
            RefusalCode::InvalidName => "invalid-name",
            RefusalCode::InvalidMessageId => "invalid-message-id",
            RefusalCode::PaddingSpace => "padding-space",
            RefusalCode::TooLong => "too-long",
            RefusalCode::Malformed => "malformed",
            RefusalCode::InvalidCharacter => "invalid-character",
            RefusalCode::TextReadsAsField => "text-reads-as-field",
            RefusalCode::NotANumber => "not-a-number",
            RefusalCode::ValueOutOfRange => "value-out-of-range",
            RefusalCode::ValueHasNoForm => "value-has-no-form",
            RefusalCode::ValueGivenTwice => "value-given-twice",
            RefusalCode::InvalidTimestamp => "invalid-timestamp",
            RefusalCode::MissingField => "missing-field",
            RefusalCode::InvalidSymbol => "invalid-symbol",
            RefusalCode::SymbolMismatch => "symbol-mismatch",
            RefusalCode::ExtensionConflict => "extension-conflict",
            RefusalCode::FieldNotAllowed => "field-not-allowed",
            RefusalCode::InvalidCode => "invalid-code",
            RefusalCode::InvalidNmea => "invalid-nmea",
            RefusalCode::NotEncodable => "not-encodable",
            RefusalCode::DoesNotReadBack => "does-not-read-back",
        }
    }

    /// The code for a number a check refused: not-a-number for NaN or an infinity, else out of range.
    pub(crate) fn for_value(x: f64) -> RefusalCode {
        if x.is_finite() { RefusalCode::ValueOutOfRange } else { RefusalCode::NotANumber }
    }

    /// The code with this vectors id, if there is one.
    pub fn from_id(id: &str) -> Option<RefusalCode> {
        RefusalCode::ALL.iter().copied().find(|c| c.id() == id)
    }
}

impl fmt::Display for RefusalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// The longest information field an encoder writes, in bytes.
pub(crate) const MAX_INFORMATION_FIELD: usize = 256;

/// `too-long` when the information field, `bytes` long, is over 256 bytes. It is the last check.
pub(crate) fn check_size(bytes: usize) -> Result<(), crate::EncodeError> {
    if bytes > MAX_INFORMATION_FIELD {
        return Err(crate::EncodeError::size(format!(
            "the information field is at most {MAX_INFORMATION_FIELD} bytes, and this one is {bytes}"
        )));
    }
    Ok(())
}

/// The order in which a refusal with several faults names its code (vectors DESIGN.md, ruling 1):
/// the first code that applies.
const ORDER: [RefusalCode; 26] = [
    RefusalCode::NotEncodable,
    RefusalCode::InvalidAddress,
    RefusalCode::Ax25CannotCarry,
    RefusalCode::Tnc2CannotCarry,
    RefusalCode::InvalidThirdPartyHeader,
    RefusalCode::InvalidAddressee,
    RefusalCode::InvalidName,
    RefusalCode::InvalidTimestamp,
    RefusalCode::NotANumber,
    RefusalCode::ValueOutOfRange,
    RefusalCode::ValueHasNoForm,
    RefusalCode::InvalidCode,
    RefusalCode::InvalidSymbol,
    RefusalCode::InvalidCharacter,
    RefusalCode::TooLong,
    RefusalCode::PaddingSpace,
    RefusalCode::Malformed,
    RefusalCode::InvalidMessageId,
    RefusalCode::InvalidNmea,
    RefusalCode::ExtensionConflict,
    RefusalCode::FieldNotAllowed,
    RefusalCode::SymbolMismatch,
    RefusalCode::MissingField,
    RefusalCode::ValueGivenTwice,
    RefusalCode::TextReadsAsField,
    RefusalCode::DoesNotReadBack,
];

impl RefusalCode {
    /// Whether this code comes before `other` in the order a refusal with several faults names its code.
    pub(crate) fn precedes(self, other: RefusalCode) -> bool {
        self.rank() < other.rank()
    }

    fn rank(self) -> usize {
        ORDER.iter().position(|c| *c == self).unwrap_or(ORDER.len())
    }
}

/// Collects the faults of one piece of data and keeps the one whose code comes first in the order
/// above, so the refusal does not depend on which check happens to run first. The encoder goes on
/// after a fault, writing bytes that are thrown away, and gives the result at the end.
#[derive(Default)]
pub(crate) struct Pending(Option<crate::EncodeError>, Option<usize>);

impl Pending {
    pub(crate) fn note(&mut self, e: crate::EncodeError) {
        if self.0.as_ref().is_none_or(|old| e.code().rank() < old.code().rank()) {
            self.0 = Some(e);
        }
    }

    /// Notes a fault, if there is one, and gives the value of a success.
    pub(crate) fn run<T>(&mut self, r: Result<T, crate::EncodeError>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.note(e);
                None
            }
        }
    }

    /// Remembers the size of the information field, `bytes` long. It is judged last, on the bytes
    /// written: only when nothing else is wrong does `finish` name `too-long` for it.
    pub(crate) fn size(&mut self, bytes: usize) {
        self.1 = Some(bytes);
    }

    /// The first fault in the order; failing that, `too-long` when the size remembered is over 256 bytes.
    pub(crate) fn finish(self) -> Result<(), crate::EncodeError> {
        match self.0 {
            Some(e) => Err(e),
            None => self.1.map_or(Ok(()), check_size),
        }
    }

    /// The faults only, for a caller that judges the size itself after a later check.
    pub(crate) fn finish_faults(self) -> Result<(), crate::EncodeError> {
        self.0.map_or(Ok(()), Err)
    }
}
