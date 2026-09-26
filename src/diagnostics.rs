//! Diagnostics: everything a decoder has to say about a packet, each with a stable code.

use alloc::string::String;
use core::fmt;

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Worth knowing, but nothing is wrong with the packet.
    Info,
    /// The packet breaks the spec in a way the decoder tolerated (see [`crate::ParseOptions`]).
    Warning,
    /// The packet breaks the spec and what it says could not be decoded.
    Error,
}

impl Severity {
    /// The lower-case name used in the conformance vectors: `info`, `warning` or `error`.
    pub const fn name(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// A diagnostic code. The names match the ids in the conformance vectors' `codes.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Code {
    /// The TNC2 header is not `SOURCE>DEST[,PATH]:`.
    InvalidHeader,
    /// An address is empty, too long, or has characters that cannot appear in one.
    InvalidAddress,
    /// The destination address is empty (UAP 5.2).
    EmptyDestination,
    /// The digipeater path has an empty entry (UAP 5.6).
    EmptyPathEntry,
    /// More than one path entry is marked used with *; only the last used one should be (UAP 5.30).
    MultipleUsedMarkers,
    /// The AX.25 frame is not a UI frame with PID 0xF0, or is too short (APRS12c ch. 3).
    NotAprsFrame,
    /// An AX.25 address is padded with NUL rather than spaces (UAP 5.29).
    NulPaddedAddress,
    /// An AX.25 address has characters other than upper-case letters and digits.
    InvalidAx25AddressCharacters,
    /// The AX.25 frame has more than 8 digipeater addresses.
    TooManyDigipeaters,
    /// The information field ends with CR or LF (APRS12c ch. 5, UAP 5.13).
    TrailingLineBreak,
    /// Text that is not valid UTF-8 (UAP 5.16).
    NonUtf8Text,
    /// The information field is shorter than its format requires.
    Truncated,
    /// The first byte is not a data type identifier (APRS12c ch. 20).
    NotAprs,
    /// A reserved data type identifier with no defined format (APRS12c ch. 5).
    ReservedDataType,
    /// A format the spec marks obsolete or not recommended, e.g. raw NMEA or raw weather.
    ObsoleteFormat,
    /// A value in a well-formed field is out of range and was dropped.
    OutOfRangeValue,
    /// A timestamp is malformed or out of range (APRS12c ch. 6, UAP 5.8).
    InvalidTimestamp,
    /// The position is missing or starts with something that cannot begin one.
    InvalidPosition,
    /// The latitude is malformed (APRS12c ch. 6, UAP 5.7).
    InvalidLatitude,
    /// The longitude is malformed (APRS12c ch. 6, UAP 5.7).
    InvalidLongitude,
    /// A lower-case hemisphere letter (UAP 5.9).
    LowercaseHemisphere,
    /// The symbol table identifier is not /, \, 0-9 or A-Z.
    InvalidSymbolTable,
    /// The symbol code is not printable ASCII.
    InvalidSymbolCode,
    /// A compressed position is malformed (APRS12c ch. 9).
    InvalidCompressedPosition,
    /// A !DAO! adds precision to an ambiguous position, which contradicts it.
    DaoWithAmbiguity,
    /// A data extension (PHG, RNG, DFS) appears later in the comment (UAP 5.15).
    DataExtensionInComment,
    /// The object name is empty or not printable ASCII.
    InvalidObjectName,
    /// The object name is not padded to 9 characters.
    ObjectNameNotPadded,
    /// An object report has no timestamp (APRS12c ch. 11).
    ObjectWithoutTimestamp,
    /// The item name is not 3-9 printable characters followed by ! or _.
    InvalidItemName,
    /// A weather report lacks a mandatory field (APRS12c ch. 12).
    IncompleteWeather,
    /// Text after the weather data; weather reports have no comment (UAP 2.7.1, ch. 5.33).
    WeatherComment,
    /// Positionless or raw weather data that could not be decoded.
    InvalidWeather,
    /// The destination address is not a valid Mic-E encoding (APRS12c ch. 10).
    InvalidMicEDestination,
    /// The Mic-E information field is malformed (APRS12c ch. 10).
    InvalidMicEInformation,
    /// Kenwood TM-D710 0xFF padding was removed (UAP 5.10).
    KenwoodFfPadding,
    /// A Mic-E report without a device type prefix (UAP 5.4).
    MicEMissingDeviceType,
    /// A message is malformed (APRS12c ch. 14).
    InvalidMessage,
    /// The addressee is not padded to 9 characters.
    UnpaddedAddressee,
    /// An ack or rej carries a message ID of its own (UAP 5.32).
    MessageIdOnAck,
    /// A telemetry metadata message (PARM/UNIT/EQNS/BITS) is malformed (APRS12c ch. 13).
    InvalidTelemetryMetadata,
    /// A directed query is malformed (APRS12c ch. 15, UAP 5.18).
    InvalidQuery,
    /// A telemetry report is malformed (APRS12c ch. 13).
    InvalidTelemetry,
    /// A status report is malformed (APRS12c ch. 16).
    InvalidStatus,
    /// A Maidenhead locator is malformed.
    InvalidLocator,
    /// An NMEA sentence is malformed.
    InvalidNmea,
    /// An NMEA sentence's checksum does not match, so the sentence is corrupt and is not decoded.
    NmeaChecksumMismatch,
    /// A third-party header is malformed (APRS12c ch. 17).
    InvalidThirdParty,
    /// A general query is malformed (APRS12c ch. 15).
    InvalidGeneralQuery,
    /// A station capabilities report is malformed (APRS12c ch. 15).
    InvalidCapabilities,
    /// A user-defined packet is shorter than its 3-byte header (APRS12c ch. 19).
    InvalidUserDefined,
    /// An Agrelo DF report is malformed.
    InvalidAgreloDf,
    /// A grid-locator status report lacks the mandatory space before its text (UAP 5.17).
    MissingSpaceAfterLocator,
    /// A compressed position's type byte sets its unused high bits (APRS12c ch. 9).
    CompressionTypeReservedBits,
    /// A timestamped position report whose timestamp is missing or not timestamp-shaped (UAP 5.8).
    MalformedTimestamp,
    /// A ! position found after other text (obsolete TNC beacon rule).
    PositionNotAtStart,
    /// A weather field is one character shorter or longer than its fixed width (UAP 5.31).
    NonStandardWeatherFieldWidth,
    /// Wind sent as c/s fields in a position weather report instead of the DDD/SSS extension.
    WindFieldsInsteadOfExtension,
    /// An uncompressed wind extension after a compressed weather position (UAP 5.33).
    WindExtensionAfterCompressed,
    /// A Mic-E altitude after other status text instead of first (APRS12c ch. 10).
    MicEAltitudeNotFirst,
    /// Message text contains a { that does not start a valid message ID (APRS12c ch. 14).
    BraceInMessageText,
    /// A message addressee contains a space or : (APRS12c ch. 14).
    InvalidAddresseeCharacters,
    /// A bulletin addressee has a group name after a letter, e.g. BLNCNET; group bulletins use a digit (APRS12c ch. 14).
    LetterGroupBulletin,
    /// A < station capabilities packet holds free text rather than TOKEN / TOKEN=VALUE items (APRS12c ch. 15).
    FreeTextCapabilities,
}

impl Code {
    /// Every code, in catalogue order.
    pub const ALL: [Code; 64] = [
        Code::InvalidHeader,
        Code::InvalidAddress,
        Code::EmptyDestination,
        Code::EmptyPathEntry,
        Code::MultipleUsedMarkers,
        Code::NotAprsFrame,
        Code::NulPaddedAddress,
        Code::InvalidAx25AddressCharacters,
        Code::TooManyDigipeaters,
        Code::TrailingLineBreak,
        Code::NonUtf8Text,
        Code::Truncated,
        Code::NotAprs,
        Code::ReservedDataType,
        Code::ObsoleteFormat,
        Code::OutOfRangeValue,
        Code::InvalidTimestamp,
        Code::InvalidPosition,
        Code::InvalidLatitude,
        Code::InvalidLongitude,
        Code::LowercaseHemisphere,
        Code::InvalidSymbolTable,
        Code::InvalidSymbolCode,
        Code::InvalidCompressedPosition,
        Code::DaoWithAmbiguity,
        Code::DataExtensionInComment,
        Code::InvalidObjectName,
        Code::ObjectNameNotPadded,
        Code::ObjectWithoutTimestamp,
        Code::InvalidItemName,
        Code::IncompleteWeather,
        Code::WeatherComment,
        Code::InvalidWeather,
        Code::InvalidMicEDestination,
        Code::InvalidMicEInformation,
        Code::KenwoodFfPadding,
        Code::MicEMissingDeviceType,
        Code::InvalidMessage,
        Code::UnpaddedAddressee,
        Code::MessageIdOnAck,
        Code::InvalidTelemetryMetadata,
        Code::InvalidQuery,
        Code::InvalidTelemetry,
        Code::InvalidStatus,
        Code::InvalidLocator,
        Code::InvalidNmea,
        Code::NmeaChecksumMismatch,
        Code::InvalidThirdParty,
        Code::InvalidGeneralQuery,
        Code::InvalidCapabilities,
        Code::InvalidUserDefined,
        Code::InvalidAgreloDf,
        Code::MissingSpaceAfterLocator,
        Code::CompressionTypeReservedBits,
        Code::MalformedTimestamp,
        Code::PositionNotAtStart,
        Code::NonStandardWeatherFieldWidth,
        Code::WindFieldsInsteadOfExtension,
        Code::WindExtensionAfterCompressed,
        Code::MicEAltitudeNotFirst,
        Code::BraceInMessageText,
        Code::InvalidAddresseeCharacters,
        Code::LetterGroupBulletin,
        Code::FreeTextCapabilities,
    ];

    /// The code's id in the conformance vectors, e.g. `lowercase-hemisphere`.
    pub const fn id(self) -> &'static str {
        match self {
            Code::InvalidHeader => "invalid-header",
            Code::InvalidAddress => "invalid-address",
            Code::EmptyDestination => "empty-destination",
            Code::EmptyPathEntry => "empty-path-entry",
            Code::MultipleUsedMarkers => "multiple-used-markers",
            Code::NotAprsFrame => "not-aprs-frame",
            Code::NulPaddedAddress => "nul-padded-address",
            Code::InvalidAx25AddressCharacters => "invalid-ax25-address-characters",
            Code::TooManyDigipeaters => "too-many-digipeaters",
            Code::TrailingLineBreak => "trailing-line-break",
            Code::NonUtf8Text => "non-utf8-text",
            Code::Truncated => "truncated",
            Code::NotAprs => "not-aprs",
            Code::ReservedDataType => "reserved-data-type",
            Code::ObsoleteFormat => "obsolete-format",
            Code::OutOfRangeValue => "out-of-range-value",
            Code::InvalidTimestamp => "invalid-timestamp",
            Code::InvalidPosition => "invalid-position",
            Code::InvalidLatitude => "invalid-latitude",
            Code::InvalidLongitude => "invalid-longitude",
            Code::LowercaseHemisphere => "lowercase-hemisphere",
            Code::InvalidSymbolTable => "invalid-symbol-table",
            Code::InvalidSymbolCode => "invalid-symbol-code",
            Code::InvalidCompressedPosition => "invalid-compressed-position",
            Code::DaoWithAmbiguity => "dao-with-ambiguity",
            Code::DataExtensionInComment => "data-extension-in-comment",
            Code::InvalidObjectName => "invalid-object-name",
            Code::ObjectNameNotPadded => "object-name-not-padded",
            Code::ObjectWithoutTimestamp => "object-without-timestamp",
            Code::InvalidItemName => "invalid-item-name",
            Code::IncompleteWeather => "incomplete-weather",
            Code::WeatherComment => "weather-comment",
            Code::InvalidWeather => "invalid-weather",
            Code::InvalidMicEDestination => "invalid-mic-e-destination",
            Code::InvalidMicEInformation => "invalid-mic-e-information",
            Code::KenwoodFfPadding => "kenwood-ff-padding",
            Code::MicEMissingDeviceType => "mic-e-missing-device-type",
            Code::InvalidMessage => "invalid-message",
            Code::UnpaddedAddressee => "unpadded-addressee",
            Code::MessageIdOnAck => "message-id-on-ack",
            Code::InvalidTelemetryMetadata => "invalid-telemetry-metadata",
            Code::InvalidQuery => "invalid-query",
            Code::InvalidTelemetry => "invalid-telemetry",
            Code::InvalidStatus => "invalid-status",
            Code::InvalidLocator => "invalid-locator",
            Code::InvalidNmea => "invalid-nmea",
            Code::NmeaChecksumMismatch => "nmea-checksum-mismatch",
            Code::InvalidThirdParty => "invalid-third-party",
            Code::InvalidGeneralQuery => "invalid-general-query",
            Code::InvalidCapabilities => "invalid-capabilities",
            Code::InvalidUserDefined => "invalid-user-defined",
            Code::InvalidAgreloDf => "invalid-agrelo-df",
            Code::MissingSpaceAfterLocator => "missing-space-after-locator",
            Code::CompressionTypeReservedBits => "compression-type-reserved-bits",
            Code::MalformedTimestamp => "malformed-timestamp",
            Code::PositionNotAtStart => "position-not-at-start",
            Code::NonStandardWeatherFieldWidth => "non-standard-weather-field-width",
            Code::WindFieldsInsteadOfExtension => "wind-fields-instead-of-extension",
            Code::WindExtensionAfterCompressed => "wind-extension-after-compressed",
            Code::MicEAltitudeNotFirst => "mic-e-altitude-not-first",
            Code::BraceInMessageText => "brace-in-message-text",
            Code::InvalidAddresseeCharacters => "invalid-addressee-characters",
            Code::LetterGroupBulletin => "letter-group-bulletin",
            Code::FreeTextCapabilities => "free-text-capabilities",
        }
    }

    /// Whether a lenient decoder may accept this defect with a warning (a strict one rejects it).
    pub const fn is_tolerable(self) -> bool {
        matches!(
            self,
            Code::EmptyDestination
                | Code::EmptyPathEntry
                | Code::MultipleUsedMarkers
                | Code::NulPaddedAddress
                | Code::InvalidAx25AddressCharacters
                | Code::TrailingLineBreak
                | Code::NonUtf8Text
                | Code::OutOfRangeValue
                | Code::InvalidTimestamp
                | Code::LowercaseHemisphere
                | Code::DaoWithAmbiguity
                | Code::DataExtensionInComment
                | Code::ObjectNameNotPadded
                | Code::ObjectWithoutTimestamp
                | Code::IncompleteWeather
                | Code::WeatherComment
                | Code::KenwoodFfPadding
                | Code::UnpaddedAddressee
                | Code::MessageIdOnAck
                | Code::InvalidTelemetry
                | Code::MissingSpaceAfterLocator
                | Code::CompressionTypeReservedBits
                | Code::MalformedTimestamp
                | Code::PositionNotAtStart
                | Code::NonStandardWeatherFieldWidth
                | Code::WindFieldsInsteadOfExtension
                | Code::WindExtensionAfterCompressed
                | Code::MicEAltitudeNotFirst
                | Code::BraceInMessageText
                | Code::InvalidAddresseeCharacters
                | Code::LetterGroupBulletin
                | Code::FreeTextCapabilities
        )
    }

    /// The code with this vectors id, if there is one.
    pub fn from_id(id: &str) -> Option<Code> {
        Code::ALL.iter().copied().find(|c| c.id() == id)
    }

    pub(crate) const fn bit(self) -> u64 {
        1u64 << (self as u8)
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Something a decoder noticed about a packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// How serious it is.
    pub severity: Severity,
    /// What it is.
    pub code: Code,
    /// A sentence saying what was found, for people.
    pub message: String,
    /// The byte offset in the information field (or the TNC2 line, for a header problem) it refers to, when there is one.
    pub offset: Option<usize>,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.severity.name(), self.code, self.message)?;
        if let Some(offset) = self.offset {
            write!(f, " (at {offset})")?;
        }
        Ok(())
    }
}
