//! A whole packet: header, raw information field, decoded data and diagnostics.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use crate::context::Context;
use crate::{Code, Data, Diagnostic, ParseOptions, Severity};

/// A station address as it appears in a header: `CALL-SSID`, or an APRS-IS name such as `T2SPAIN`.
///
/// On APRS-IS an address is 1-9 letters, digits or hyphens. Over the air (AX.25) it must also be a
/// callsign of at most six upper-case letters and digits with an SSID of 0-15;
/// [`Address::is_ax25`] says whether it is. The source of a packet inside a third-party packet may
/// be more: see [`Address::third_party_source`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Address(String);

impl Address {
    /// An address from text. Fails if it is empty, longer than 9 characters, or has characters
    /// other than letters, digits and `-`.
    pub fn new(text: &str) -> Result<Address, InvalidAddress> {
        if Address::is_valid(text) { Ok(Address(text.to_string())) } else { Err(InvalidAddress(text.to_string())) }
    }

    pub(crate) fn is_valid(text: &str) -> bool {
        (1..=9).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }

    /// The source address of a packet carried inside a third-party packet ([`Data::ThirdParty`]).
    /// APRS12c ch. 17 lets it be any 1-9 printable ASCII characters other than `>` and `:`
    /// (`PY2SP_R-R`, say), since it "does not need to adhere to the AX.25 address restrictions".
    /// Fails for anything else. Such an address is not an APRS-IS address, so use it only there.
    pub fn third_party_source(text: &str) -> Result<Address, InvalidAddress> {
        if Address::is_third_party_source(text) { Ok(Address(text.to_string())) } else { Err(InvalidAddress(text.to_string())) }
    }

    pub(crate) fn is_third_party_source(text: &str) -> bool {
        (1..=9).contains(&text.len()) && text.bytes().all(|b| (0x20..=0x7E).contains(&b) && b != b'>' && b != b':')
    }

    /// An address taken as it came, without checks (an AX.25 address the decoder tolerated).
    pub(crate) fn unchecked(text: String) -> Address {
        Address(text)
    }

    /// The address as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before any `-SSID`.
    pub fn callsign(&self) -> &str {
        self.0.split_once('-').map_or(&self.0, |(call, _)| call)
    }

    /// The SSID after `-`, or 0 if there is none or it is not a number.
    pub fn ssid(&self) -> u8 {
        self.0.split_once('-').and_then(|(_, ssid)| ssid.parse().ok()).unwrap_or(0)
    }

    /// Whether this address can be sent in an AX.25 frame: a callsign of 1-6 upper-case letters and
    /// digits, and an SSID of 0-15 written without a leading zero.
    pub fn is_ax25(&self) -> bool {
        let call = self.callsign();
        let call_ok = (1..=6).contains(&call.len()) && call.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
        let ssid_ok = match self.0.split_once('-') {
            None => true,
            Some((_, ssid)) => {
                !ssid.is_empty()
                    && ssid.bytes().all(|b| b.is_ascii_digit())
                    && !(ssid.len() > 1 && ssid.starts_with('0'))
                    && ssid.parse::<u8>().is_ok_and(|n| (1..=15).contains(&n))
            }
        };
        call_ok && ssid_ok
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::str::FromStr for Address {
    type Err = InvalidAddress;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Address::new(s)
    }
}

/// Text that cannot be an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidAddress(pub String);

impl fmt::Display for InvalidAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' is not an address: 1-9 letters, digits or -", self.0)
    }
}

impl core::error::Error for InvalidAddress {}

/// An entry in the digipeater path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathEntry {
    /// The address.
    pub address: Address,
    /// The packet has been through this digipeater (the H bit in AX.25; `*` on the last such entry in TNC2 text).
    pub used: bool,
}

impl PathEntry {
    /// An entry not yet used.
    pub fn new(address: Address) -> PathEntry {
        PathEntry { address, used: false }
    }
}

/// An APRS-IS q-construct in the path (`qAR`, `qAC`, ...) and the station after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QConstruct<'a> {
    /// The construct, e.g. `qAR`.
    pub construct: &'a str,
    /// The station named after it: for `qAR`, the IGate.
    pub station: Option<&'a Address>,
}

/// A header that cannot be decoded, so the packet cannot be either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderError {
    /// What is wrong: at least one [`Severity::Error`], plus any warnings found before it.
    pub diagnostics: Vec<Diagnostic>,
}

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.diagnostics.iter().find(|d| d.severity == Severity::Error) {
            Some(d) => write!(f, "unusable header: {d}"),
            None => f.write_str("unusable header"),
        }
    }
}

impl core::error::Error for HeaderError {}

/// A packet that cannot be written as asked, with the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeError {
    /// Why, naming the field at fault.
    pub message: String,
}

impl EncodeError {
    pub(crate) fn new(message: impl Into<String>) -> EncodeError {
        EncodeError { message: message.into() }
    }
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl core::error::Error for EncodeError {}

/// An APRS packet: the header, the information field as received, what it decodes to, and
/// everything the decoder noticed on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    /// Who sent it.
    pub source: Address,
    /// The destination address: usually a software or device identifier (`APxxxx`); for Mic-E, half the position.
    pub destination: Address,
    /// The digipeater path, including any APRS-IS q-construct.
    pub path: Vec<PathEntry>,
    /// The information field exactly as received (for a TNC2 line, without a trailing CR or LF).
    pub information: Vec<u8>,
    /// What the information field says.
    pub data: Data,
    /// What the decoder noticed, header first.
    pub diagnostics: Vec<Diagnostic>,
}

impl Packet {
    /// Decodes a TNC2 / APRS-IS text line, `SOURCE>DEST,PATH:information`.
    pub fn decode_tnc2(line: &[u8], options: ParseOptions) -> Result<Packet, HeaderError> {
        Packet::decode_tnc2_line(line, options, false)
    }

    /// Decodes the TNC2 packet inside a third-party packet, whose source need not be an APRS-IS
    /// address (APRS12c ch. 17; [`Address::third_party_source`]).
    pub(crate) fn decode_third_party(line: &[u8], options: ParseOptions) -> Result<Packet, HeaderError> {
        Packet::decode_tnc2_line(line, options, true)
    }

    fn decode_tnc2_line(line: &[u8], options: ParseOptions, third_party: bool) -> Result<Packet, HeaderError> {
        let mut ctx = Context::new(options);
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            ctx.error(Code::InvalidHeader, "no ':' ends the header (SOURCE>DEST[,PATH]:)", None);
            return Err(ctx.header_error());
        };
        let (header, info) = (&line[..colon], &line[colon + 1..]);
        let Ok(header) = core::str::from_utf8(header) else {
            ctx.error(Code::InvalidHeader, "the header is not text", Some(0));
            return Err(ctx.header_error());
        };
        let Some((source, rest)) = header.split_once('>') else {
            ctx.error(Code::InvalidHeader, "no '>' between source and destination", None);
            return Err(ctx.header_error());
        };
        if source.is_empty() {
            ctx.error(Code::InvalidHeader, "the source address is empty", Some(0));
            return Err(ctx.header_error());
        }

        let mut fields = rest.split(',');
        let destination = fields.next().unwrap_or("");
        let source = if third_party && Address::is_third_party_source(source) {
            Address::unchecked(source.to_string())
        } else {
            header_address(&mut ctx, source)?
        };
        let destination = if destination.is_empty() {
            if !ctx.tolerate(Code::EmptyDestination, "the destination address is empty (UAP 5.2)", None) {
                return Err(ctx.header_error());
            }
            Address::unchecked(String::new())
        } else {
            header_address(&mut ctx, destination)?
        };

        let mut path = Vec::new();
        let mut used_markers = 0;
        for field in fields {
            if field.is_empty() {
                if !ctx.tolerate(Code::EmptyPathEntry, "the path has an empty entry (UAP 5.6)", None) {
                    return Err(ctx.header_error());
                }
                continue;
            }
            let (text, used) = match field.strip_suffix('*') {
                Some(text) => (text, true),
                None => (field, false),
            };
            used_markers += usize::from(used);
            path.push(PathEntry { address: header_address(&mut ctx, text)?, used });
        }

        if used_markers > 1 && !ctx.tolerate(Code::MultipleUsedMarkers, "more than one path entry is marked used with * (UAP 5.30)", None) {
            return Err(ctx.header_error());
        }

        // Every entry up to the last one marked used has been used.
        if let Some(last) = path.iter().rposition(|e| e.used) {
            for entry in &mut path[..last] {
                entry.used = true;
            }
        }

        Ok(Packet::decode_with_context(ctx, source, destination, path, info))
    }

    /// Decodes an AX.25 UI frame in KISS form: addresses, control and PID, information; no flags, no FCS.
    pub fn decode_ax25(frame: &[u8], options: ParseOptions) -> Result<Packet, HeaderError> {
        let mut ctx = Context::new(options);
        let mut addresses = Vec::new();
        let mut at = 0;
        loop {
            if frame.len() < at + 7 {
                ctx.error(Code::NotAprsFrame, "the frame ends inside its address field", Some(at));
                return Err(ctx.header_error());
            }
            let chunk = &frame[at..at + 7];
            addresses.push(chunk);
            at += 7;
            if chunk[6] & 1 == 1 {
                break;
            }
        }

        if addresses.len() < 2 {
            ctx.error(Code::NotAprsFrame, "the frame has no source address", Some(0));
            return Err(ctx.header_error());
        }
        if addresses.len() > 10 {
            ctx.error(Code::TooManyDigipeaters, "the frame has more than 8 digipeater addresses", Some(0));
            return Err(ctx.header_error());
        }
        if frame.len() < at + 2 || frame[at] & !0x10 != 0x03 || frame[at + 1] != 0xF0 {
            ctx.error(Code::NotAprsFrame, "not a UI frame with PID 0xF0 (APRS12c ch. 3)", Some(at));
            return Err(ctx.header_error());
        }

        let mut decoded = Vec::with_capacity(addresses.len());
        for (i, bytes) in addresses.iter().enumerate() {
            decoded.push(ax25_address(&mut ctx, bytes, i * 7)?);
        }

        let destination = decoded.remove(0);
        let source = decoded.remove(0);
        let path = addresses[2..].iter().zip(decoded).map(|(bytes, address)| PathEntry { address, used: bytes[6] & 0x80 != 0 }).collect();
        Ok(Packet::decode_with_context(ctx, source, destination, path, &frame[at + 2..]))
    }

    /// Decodes an information field whose header you already have.
    pub fn decode(source: Address, destination: Address, path: Vec<PathEntry>, information: &[u8], options: ParseOptions) -> Packet {
        Packet::decode_with_context(Context::new(options), source, destination, path, information)
    }

    fn decode_with_context(mut ctx: Context, source: Address, destination: Address, path: Vec<PathEntry>, information: &[u8]) -> Packet {
        let data = crate::decode::information(&mut ctx, &source, &destination, &path, information);
        Packet { source, destination, path, information: information.to_vec(), data, diagnostics: ctx.diagnostics }
    }

    /// Builds a packet from data, encoding its information field. For Mic-E data use
    /// [`Packet::create_mic_e`], which works out the destination address too.
    pub fn create(source: Address, destination: Address, path: Vec<PathEntry>, data: Data) -> Result<Packet, EncodeError> {
        if matches!(data, Data::MicE(_)) {
            return Err(EncodeError::new("Mic-E data carries half its position in the destination: use Packet::create_mic_e"));
        }
        let information = data.encode()?;
        let decoded = Packet::decode(source.clone(), destination.clone(), path.clone(), &information, ParseOptions::STRICT);
        Ok(Packet { source, destination, path, information, data, diagnostics: decoded.diagnostics })
    }

    /// Builds a Mic-E packet: the information field and the destination address that carries the
    /// latitude, message code and digipeat path (APRS12c ch. 10).
    pub fn create_mic_e(source: Address, report: crate::MicEReport, path: Vec<PathEntry>) -> Result<Packet, EncodeError> {
        let (destination, information) = crate::mic_e::encode(&report)?;
        let decoded = Packet::decode(source.clone(), destination.clone(), path.clone(), &information, ParseOptions::STRICT);
        Ok(Packet { source, destination, path, information, data: Data::MicE(report), diagnostics: decoded.diagnostics })
    }

    /// Whether any diagnostic is a warning.
    pub fn has_warnings(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Warning)
    }

    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    /// The first APRS-IS q-construct in the path, and the station after it.
    pub fn q_construct(&self) -> Option<QConstruct<'_>> {
        let i = self.path.iter().position(|e| is_q_construct(e.address.as_str()))?;
        Some(QConstruct { construct: self.path[i].address.as_str(), station: self.path.get(i + 1).map(|e| &e.address) })
    }

    /// The packet as a TNC2 text line (without a line ending). The last used path entry is marked `*`.
    pub fn to_tnc2(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.information.len());
        out.extend_from_slice(self.source.as_str().as_bytes());
        out.push(b'>');
        out.extend_from_slice(self.destination.as_str().as_bytes());
        let last_used = self.path.iter().rposition(|e| e.used);
        for (i, entry) in self.path.iter().enumerate() {
            out.push(b',');
            out.extend_from_slice(entry.address.as_str().as_bytes());
            if Some(i) == last_used {
                out.push(b'*');
            }
        }
        out.push(b':');
        out.extend_from_slice(&self.information);
        out
    }

    /// The packet as an AX.25 UI frame in KISS form (no flags, no FCS). Fails if an address
    /// cannot be sent over the air, or the path is longer than 8 entries.
    pub fn to_ax25(&self) -> Result<Vec<u8>, EncodeError> {
        if self.path.len() > 8 {
            return Err(EncodeError::new("an AX.25 frame has room for at most 8 digipeaters"));
        }
        let mut out = Vec::with_capacity(16 + 7 * self.path.len() + self.information.len());
        let all = [(&self.destination, 0xE0u8), (&self.source, 0x60u8)];
        for (address, flags) in all {
            write_ax25_address(&mut out, address, flags)?;
        }
        for entry in &self.path {
            write_ax25_address(&mut out, &entry.address, if entry.used { 0xE0 } else { 0x60 })?;
        }
        let last = out.len() - 1;
        out[last] |= 1;
        out.push(0x03);
        out.push(0xF0);
        out.extend_from_slice(&self.information);
        Ok(out)
    }
}

/// `q`, an upper-case letter, then a letter of either case: `qAr` and `qAo` (UAP section 4) are in use.
fn is_q_construct(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 3 && b[0] == b'q' && b[1].is_ascii_uppercase() && b[2].is_ascii_alphabetic()
}

fn header_address(ctx: &mut Context, text: &str) -> Result<Address, HeaderError> {
    if Address::is_valid(text) {
        Ok(Address::unchecked(text.to_string()))
    } else {
        ctx.error(Code::InvalidAddress, &format!("'{text}' is not an address: 1-9 letters, digits or -"), None);
        Err(ctx.header_error())
    }
}

fn ax25_address(ctx: &mut Context, bytes: &[u8], offset: usize) -> Result<Address, HeaderError> {
    let chars: Vec<u8> = bytes[..6].iter().map(|b| b >> 1).collect();
    let mut end = 6;
    while end > 0 && chars[end - 1] == b' ' {
        end -= 1;
    }
    let mut call = &chars[..end];
    if call.contains(&0) {
        // Only trailing NULs are padding; a NUL inside a callsign is garbage.
        let nul_start = call.iter().position(|&c| c == 0).unwrap_or(call.len());
        if call[nul_start..].iter().any(|&c| c != 0) {
            ctx.error(Code::InvalidAddress, "an AX.25 address has a NUL inside it", Some(offset));
            return Err(ctx.header_error());
        }
        if !ctx.tolerate(Code::NulPaddedAddress, "an AX.25 address is padded with NUL rather than spaces (UAP 5.29)", Some(offset)) {
            return Err(ctx.header_error());
        }
        call = &call[..nul_start];
    }
    if call.is_empty() {
        ctx.error(Code::InvalidAddress, "an AX.25 address is empty", Some(offset));
        return Err(ctx.header_error());
    }
    if !call.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        if !call.iter().all(|c| c.is_ascii_graphic()) {
            ctx.error(Code::InvalidAddress, "an AX.25 address has characters that cannot be shown", Some(offset));
            return Err(ctx.header_error());
        }
        if !ctx.tolerate(
            Code::InvalidAx25AddressCharacters,
            "an AX.25 address has characters other than upper-case letters and digits",
            Some(offset),
        ) {
            return Err(ctx.header_error());
        }
    }

    let mut text: String = call.iter().map(|&c| c as char).collect();
    let ssid = (bytes[6] >> 1) & 0x0F;
    if ssid != 0 {
        text.push_str(&format!("-{ssid}"));
    }
    Ok(Address::unchecked(text))
}

fn write_ax25_address(out: &mut Vec<u8>, address: &Address, flags: u8) -> Result<(), EncodeError> {
    if !address.is_ax25() {
        return Err(EncodeError::new(format!(
            "'{address}' cannot be sent in an AX.25 frame: 1-6 upper-case letters and digits, SSID 0-15"
        )));
    }
    let call = address.callsign().as_bytes();
    for i in 0..6 {
        out.push(call.get(i).copied().unwrap_or(b' ') << 1);
    }
    out.push(flags | (address.ssid() << 1));
    Ok(())
}
