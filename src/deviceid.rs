//! Device identification from the APRS device identification database
//! (<https://github.com/aprsorg/aprs-deviceid>, CC BY-SA 2.0, maintained by OH7LZB): the
//! destination address ("tocall") for most packets, the comment prefix and suffix for Mic-E.

use crate::deviceid_data::{COMMIT, MICE, MICE_LEGACY, TOCALLS};
use crate::{Data, Packet};

/// A device or program, as the database names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    /// Who makes it.
    pub vendor: Option<&'static str>,
    /// What it is.
    pub model: Option<&'static str>,
    /// The database's device class, e.g. `rig`, `ht`, `tracker`, `software`.
    pub class: Option<&'static str>,
    /// The operating system, for software.
    pub os: Option<&'static str>,
    /// Feature flags, e.g. `messaging`.
    pub features: &'static [&'static str],
}

pub(crate) struct Entry {
    pub(crate) pattern: &'static str,
    pub(crate) vendor: Option<&'static str>,
    pub(crate) model: Option<&'static str>,
    pub(crate) class: Option<&'static str>,
    pub(crate) os: Option<&'static str>,
    pub(crate) features: &'static [&'static str],
}

pub(crate) struct Suffix {
    pub(crate) suffix: &'static str,
    pub(crate) vendor: Option<&'static str>,
    pub(crate) model: Option<&'static str>,
    pub(crate) class: Option<&'static str>,
    pub(crate) os: Option<&'static str>,
    pub(crate) features: &'static [&'static str],
}

pub(crate) struct Legacy {
    pub(crate) prefix: &'static str,
    pub(crate) suffix: Option<&'static str>,
    pub(crate) vendor: Option<&'static str>,
    pub(crate) model: Option<&'static str>,
    pub(crate) class: Option<&'static str>,
    pub(crate) os: Option<&'static str>,
    pub(crate) features: &'static [&'static str],
}

/// The database version this crate carries: the aprs-deviceid commit and its date.
pub fn database_version() -> &'static str {
    COMMIT
}

impl Packet {
    /// The sending device or program, if the database knows it: from the Mic-E type code and
    /// suffix for Mic-E, otherwise from the destination address.
    pub fn device(&self) -> Option<Device> {
        match &self.data {
            Data::MicE(m) => {
                let prefix = m.type_code?;
                match prefix {
                    '>' | ']' => MICE_LEGACY
                        .iter()
                        .find(|l| l.prefix.starts_with(prefix) && l.suffix.unwrap_or("") == m.device_suffix)
                        .map(|l| Device { vendor: l.vendor, model: l.model, class: l.class, os: l.os, features: l.features }),
                    '`' | '\'' => MICE.iter().find(|s| s.suffix == m.device_suffix).map(|s| Device {
                        vendor: s.vendor,
                        model: s.model,
                        class: s.class,
                        os: s.os,
                        features: s.features,
                    }),
                    _ => None,
                }
            }
            _ => tocall(self.destination.callsign()),
        }
    }
}

/// The best match for a destination callsign: exact characters beat `?` (any character) and `n`
/// (a digit), which beat a trailing `*` (anything).
pub fn tocall(callsign: &str) -> Option<Device> {
    let call = callsign.as_bytes();
    TOCALLS.iter().filter_map(|e| score(e.pattern.as_bytes(), call).map(|s| (s, e))).max_by_key(|(s, _)| *s).map(|(_, e)| Device {
        vendor: e.vendor,
        model: e.model,
        class: e.class,
        os: e.os,
        features: e.features,
    })
}

fn score(pattern: &[u8], call: &[u8]) -> Option<(usize, usize)> {
    let mut literal = 0;
    for (i, &p) in pattern.iter().enumerate() {
        if p == b'*' {
            return (call.len() >= i).then_some((literal, 0));
        }
        let c = *call.get(i)?;
        match p {
            b'?' => {}
            b'n' => {
                if !c.is_ascii_digit() {
                    return None;
                }
            }
            _ if p == c => literal += 1,
            _ => return None,
        }
    }
    (call.len() == pattern.len()).then_some((literal, 1))
}

/// How many bytes at the end of a Mic-E status text are a device suffix known for this type code.
pub(crate) fn mic_e_suffix_len(type_code: Option<u8>, status: &[u8]) -> usize {
    match type_code {
        Some(b'`' | b'\'') => {
            if status.len() >= 2 && MICE.iter().any(|s| s.suffix.as_bytes() == &status[status.len() - 2..]) {
                2
            } else {
                0
            }
        }
        Some(p @ (b'>' | b']')) => match status.last() {
            Some(&last) if MICE_LEGACY.iter().any(|l| l.prefix.as_bytes() == [p] && l.suffix.is_some_and(|s| s.as_bytes() == [last])) => 1,
            _ => 0,
        },
        _ => 0,
    }
}
