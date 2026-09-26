//! Timestamps (APRS12c ch. 6).

use alloc::vec::Vec;

use crate::Timestamp;
use crate::text::all_digits;

impl Timestamp {
    /// Day of month, hour and minute, UTC: `DDHHMMz`, the form most reports use.
    pub const fn dhm(day: u8, hour: u8, minute: u8) -> Timestamp {
        Timestamp::DayHourMinute { day, hour, minute, utc: true }
    }

    /// Hour, minute and second, UTC: `HHMMSSh`.
    pub const fn hms(hour: u8, minute: u8, second: u8) -> Timestamp {
        Timestamp::HourMinuteSecond { hour, minute, second }
    }

    /// Month, day, hour and minute, UTC: `MMDDHHMM`, the form positionless weather reports use.
    pub const fn mdhm(month: u8, day: u8, hour: u8, minute: u8) -> Timestamp {
        Timestamp::MonthDayHourMinute { month, day, hour, minute }
    }

    /// Reads a 7-character DHM (`z` or `/`) or HMS (`h`) timestamp. `None` if it is not that shape.
    pub fn parse(bytes: &[u8]) -> Option<Timestamp> {
        if bytes.len() != 7 || !all_digits(&bytes[..6]) {
            return None;
        }
        let [a, b, c] = [pair(&bytes[0..2]), pair(&bytes[2..4]), pair(&bytes[4..6])];
        match bytes[6] {
            b'z' => Some(Timestamp::DayHourMinute { day: a, hour: b, minute: c, utc: true }),
            b'/' => Some(Timestamp::DayHourMinute { day: a, hour: b, minute: c, utc: false }),
            b'h' => Some(Timestamp::HourMinuteSecond { hour: a, minute: b, second: c }),
            _ => None,
        }
    }

    /// Reads an 8-digit month-day-hour-minute timestamp.
    pub fn parse_mdhm(bytes: &[u8]) -> Option<Timestamp> {
        if bytes.len() != 8 || !all_digits(bytes) {
            return None;
        }
        Some(Timestamp::MonthDayHourMinute {
            month: pair(&bytes[0..2]),
            day: pair(&bytes[2..4]),
            hour: pair(&bytes[4..6]),
            minute: pair(&bytes[6..8]),
        })
    }

    /// Whether the fields make sense: a day 1-31 (or the permanent-object marker `111111z`), an
    /// hour 0-23, a minute and second 0-59, a month 1-12.
    pub fn is_valid(&self) -> bool {
        match *self {
            Timestamp::DayHourMinute { day, hour, minute, .. } => (1..=31).contains(&day) && hour < 24 && minute < 60,
            Timestamp::HourMinuteSecond { hour, minute, second } => hour < 24 && minute < 60 && second < 60,
            Timestamp::MonthDayHourMinute { month, day, hour, minute } => {
                (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60
            }
        }
    }

    /// Whether this is `111111z`, which APRS12c ch. 18 uses to mark a permanent object.
    pub fn is_permanent_object_marker(&self) -> bool {
        matches!(self, Timestamp::DayHourMinute { day: 11, hour: 11, minute: 11, utc: true })
    }

    /// The timestamp as sent on air: `092345z`, `092345/`, `234517h` or `10090556`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8);
        let mut two = |n: u8| {
            out.push(b'0' + n / 10 % 10);
            out.push(b'0' + n % 10);
        };
        match *self {
            Timestamp::DayHourMinute { day, hour, minute, .. } => {
                two(day);
                two(hour);
                two(minute);
            }
            Timestamp::HourMinuteSecond { hour, minute, second } => {
                two(hour);
                two(minute);
                two(second);
            }
            Timestamp::MonthDayHourMinute { month, day, hour, minute } => {
                two(month);
                two(day);
                two(hour);
                two(minute);
            }
        }
        match *self {
            Timestamp::DayHourMinute { utc: true, .. } => out.push(b'z'),
            Timestamp::DayHourMinute { utc: false, .. } => out.push(b'/'),
            Timestamp::HourMinuteSecond { .. } => out.push(b'h'),
            Timestamp::MonthDayHourMinute { .. } => {}
        }
        out
    }
}

fn pair(b: &[u8]) -> u8 {
    (b[0] - b'0') * 10 + (b[1] - b'0')
}
