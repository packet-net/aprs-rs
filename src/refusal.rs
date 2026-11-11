//! Refusal reason codes: why an encoder or builder would not write the data as given.

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
    /// Free text or a list is longer than its field allows.
    TooLong,
    /// Text in a fixed-form field is not that form (a locator, a bit sense, a capability token).
    Malformed,
    /// Free text holds a character its field cannot carry, such as a line break.
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
