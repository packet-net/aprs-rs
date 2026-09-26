//! Fluent construction of the packets an application sends, starting from the sending station.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::packet::{Address, EncodeError, InvalidAddress, Packet, PathEntry};
use crate::{
    Ack, BeamHeading, Bulletin, CommentTelemetry, Dao, DaoPrecision, Data, ItemReport, Message, MicEMessage, MicEReport, ObjectReport, Phg,
    Position, PositionReport, Positioned, Reject, Status, Symbol, Telemetry, TelemetryBits, TelemetryCoefficients, TelemetryLabels,
    Timestamp, Tone, VoiceFrequency, Weather, WeatherReport,
};

/// The destination address a [`Station`] uses until [`Station::to`] sets one: `APZ001`, in the range the
/// APRS device identification database keeps for experimental software. An application should use its own
/// allocated destination (its "tocall").
pub const DEFAULT_DESTINATION: &str = "APZ001";

/// A sending station and its header: the source, destination and path every packet built from it shares,
/// and the starting point for each kind of packet.
///
/// ```
/// use pdn_aprs::{Station, Symbol};
///
/// let me = Station::new("M0LTE-9")?.via(&["WIDE1-1", "WIDE2-1"])?;
/// let packet = me.position(51.4543, -0.9781).symbol(Symbol::CAR).course(88).speed(36.0).comment("Mobile").build()?;
/// assert_eq!(packet.to_tnc2(), b"M0LTE-9>APZ001,WIDE1-1,WIDE2-1:!5127.26N/00058.69W>088/036Mobile");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// A station is a value: [`Station::to`] and [`Station::via`] return a new one, so it can be kept and
/// reused. `build()` encodes the packet, so anything APRS12c does not allow is refused there with an
/// [`EncodeError`], as [`Packet::create`] would. `to_data()` gives the [`Data`] alone, for anything the
/// builder does not cover; [`Station::data`] sends it.
#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    source: Address,
    destination: Address,
    path: Vec<PathEntry>,
}

impl Station {
    /// The station sending the packets, e.g. `M0LTE-9`.
    pub fn new(source: &str) -> Result<Station, InvalidAddress> {
        Ok(Station { source: Address::new(source)?, destination: Address::new(DEFAULT_DESTINATION)?, path: Vec::new() })
    }

    /// This station with another destination address, usually the application's allocated tocall.
    pub fn to(&self, destination: &str) -> Result<Station, InvalidAddress> {
        Ok(Station { destination: Address::new(destination)?, ..self.clone() })
    }

    /// This station with a digipeater path, e.g. `via(&["WIDE1-1", "WIDE2-1"])`, replacing any set before.
    pub fn via(&self, path: &[&str]) -> Result<Station, InvalidAddress> {
        let path = path.iter().map(|p| Address::new(p).map(PathEntry::new)).collect::<Result<Vec<_>, _>>()?;
        Ok(Station { path, ..self.clone() })
    }

    /// The source address.
    pub fn source(&self) -> &Address {
        &self.source
    }

    /// The destination address. A Mic-E report computes its own and ignores this.
    pub fn destination(&self) -> &Address {
        &self.destination
    }

    /// The digipeater path.
    pub fn path(&self) -> &[PathEntry] {
        &self.path
    }

    /// A position report for this station (APRS12c ch. 8 and 9).
    pub fn position(&self, latitude: f64, longitude: f64) -> PositionBuilder {
        PositionBuilder { station: self.clone(), common: Common::at(latitude, longitude), messaging: false, timestamp: None }
    }

    /// An object: something other than this station placed on the map under its own name, which needs
    /// [`at`](ObjectBuilder::at) and a [`timestamp`](ObjectBuilder::timestamp) (APRS12c ch. 11).
    pub fn object(&self, name: &str) -> ObjectBuilder {
        ObjectBuilder { station: self.clone(), common: Common::default(), name: name.to_string(), killed: false, timestamp: None }
    }

    /// An item: like an object but with no timestamp, for things that do not move, which needs
    /// [`at`](ItemBuilder::at) (APRS12c ch. 11).
    pub fn item(&self, name: &str) -> ItemBuilder {
        ItemBuilder { station: self.clone(), common: Common::default(), name: name.to_string(), killed: false }
    }

    /// A Mic-E position report, which carries part of its position in the destination address, so ignores
    /// this station's destination (APRS12c ch. 10).
    pub fn mic_e(&self, latitude: f64, longitude: f64) -> MicEBuilder {
        MicEBuilder { station: self.clone(), common: Common::at(latitude, longitude), message: MicEMessage::OffDuty, messaging: false }
    }

    /// A weather report. With [`at`](WeatherBuilder::at) it is a position report with the weather station
    /// symbol, as APRS12c recommends; without, a positionless weather report (APRS12c ch. 12).
    pub fn weather(&self) -> WeatherBuilder {
        WeatherBuilder {
            station: self.clone(),
            weather: Weather::default(),
            position: None,
            symbol: Symbol::WEATHER_STATION,
            timestamp: None,
            messaging: false,
            compressed: false,
            dao: None,
        }
    }

    /// A message to another station (APRS12c ch. 14).
    pub fn message(&self, addressee: &str, text: &str) -> MessageBuilder {
        MessageBuilder {
            station: self.clone(),
            message: Message { addressee: addressee.to_string(), text: text.to_string(), ..Message::default() },
        }
    }

    /// An acknowledgement of message `message_id` from `addressee` (APRS12c ch. 14).
    pub fn ack(&self, addressee: &str, message_id: &str) -> AckBuilder {
        AckBuilder { station: self.clone(), addressee: addressee.to_string(), id: message_id.to_string(), reject: false, reply_ack: None }
    }

    /// A rejection of message `message_id` from `addressee` (APRS12c ch. 14).
    pub fn reject(&self, addressee: &str, message_id: &str) -> AckBuilder {
        AckBuilder { station: self.clone(), addressee: addressee.to_string(), id: message_id.to_string(), reject: true, reply_ack: None }
    }

    /// A general bulletin (`id` `0`-`9`) or an announcement (`A`-`Z`) (APRS12c ch. 14).
    pub fn bulletin(&self, id: char, text: &str) -> DataBuilder {
        self.data(Data::Bulletin(Bulletin { addressee: format!("BLN{id}"), text: text.to_string(), message_id: None }))
    }

    /// A group bulletin, e.g. `BLN4WX` for id `4` and group `WX` (APRS12c ch. 14).
    pub fn group_bulletin(&self, id: char, group: &str, text: &str) -> DataBuilder {
        self.data(Data::Bulletin(Bulletin { addressee: format!("BLN{id}{group}"), text: text.to_string(), message_id: None }))
    }

    /// A status report (APRS12c ch. 16).
    pub fn status(&self, text: &str) -> StatusBuilder {
        StatusBuilder { station: self.clone(), status: Status { text: text.to_string(), ..Status::default() } }
    }

    /// A telemetry report with this sequence number, sent as three digits when it is 0-999 (APRS12c ch. 13).
    pub fn telemetry(&self, sequence: u16) -> TelemetryBuilder {
        TelemetryBuilder { station: self.clone(), sequence, analog: Vec::new(), digital: 0, comment: String::new() }
    }

    /// The names of this station's telemetry channels, A1-A5 then B1-B8 (`PARM.`, APRS12c ch. 13).
    pub fn telemetry_names(&self, names: &[&str]) -> DataBuilder {
        self.data(Data::TelemetryNames(self.labels(names)))
    }

    /// The units of this station's analog telemetry channels, then the labels of its digital ones
    /// (`UNIT.`, APRS12c ch. 13).
    pub fn telemetry_units(&self, units: &[&str]) -> DataBuilder {
        self.data(Data::TelemetryUnits(self.labels(units)))
    }

    /// The scaling for this station's analog telemetry channels: a, b and c for each in turn, giving
    /// a x v^2 + b x v + c (`EQNS.`, APRS12c ch. 13).
    pub fn telemetry_coefficients(&self, coefficients: &[f64]) -> DataBuilder {
        self.data(Data::TelemetryCoefficients(TelemetryCoefficients {
            addressee: self.source.as_str().to_string(),
            coefficients: coefficients.iter().map(|c| format!("{c}")).collect(),
            message_id: None,
        }))
    }

    /// Which state of each digital channel matches its label (bit 0 is B1), and the project title
    /// (`BITS.`, APRS12c ch. 13).
    pub fn telemetry_bits(&self, bits: u8, project: &str) -> DataBuilder {
        self.data(Data::TelemetryBits(TelemetryBits {
            addressee: self.source.as_str().to_string(),
            bits: bit_string(bits),
            project: project.to_string(),
            message_id: None,
        }))
    }

    /// Any data built by hand, sent from this station.
    pub fn data(&self, data: Data) -> DataBuilder {
        DataBuilder { station: self.clone(), data }
    }

    fn labels(&self, labels: &[&str]) -> TelemetryLabels {
        TelemetryLabels {
            addressee: self.source.as_str().to_string(),
            labels: labels.iter().map(|l| l.to_string()).collect(),
            message_id: None,
        }
    }

    fn packet(&self, data: Data) -> Result<Packet, EncodeError> {
        match data {
            Data::MicE(report) => Packet::create_mic_e(self.source.clone(), report, self.path.clone()),
            data => Packet::create(self.source.clone(), self.destination.clone(), self.path.clone(), data),
        }
    }
}

/// Sends a [`Data`] built by hand, from [`Station::data`] and the other one-step packets.
#[derive(Clone, Debug, PartialEq)]
pub struct DataBuilder {
    station: Station,
    data: Data,
}

impl DataBuilder {
    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        Ok(self.data.clone())
    }

    /// The packet: the station's header and this data, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.data.clone())
    }
}

/// What every report that puts something on the map carries besides its own fields.
#[derive(Clone, Debug, Default, PartialEq)]
struct Common {
    position: Option<Position>,
    ambiguity: u8,
    symbol: Option<Symbol>,
    fields: Positioned,
    tone: Option<f64>,
    offset_khz: Option<i32>,
}

impl Common {
    fn at(latitude: f64, longitude: f64) -> Common {
        Common { position: Some(Position { latitude, longitude, ambiguity: 0 }), ..Common::default() }
    }

    /// The shared fields, checked for what the builder itself needs; the encoder checks the rest.
    fn positioned(&self, what: &str) -> Result<Positioned, EncodeError> {
        let position = self.position.ok_or_else(|| EncodeError::new(format!("{what} needs a position: call at(latitude, longitude)")))?;
        let symbol =
            self.symbol.ok_or_else(|| EncodeError::new(format!("{what} needs a symbol: call symbol(...), e.g. symbol(Symbol::CAR)")))?;
        let mut fields = self.fields.clone();
        fields.position = Position { ambiguity: self.ambiguity, ..position };
        fields.symbol = symbol;
        if self.tone.is_some() || self.offset_khz.is_some() {
            let frequency = fields
                .frequency
                .as_mut()
                .ok_or_else(|| EncodeError::new("a tone or offset needs a frequency: call frequency(mhz) first"))?;
            if let Some(hz) = self.tone {
                frequency.tone = Some(Tone::Tone);
                frequency.tone_value = Some(hz as u16);
            }
            if let Some(khz) = self.offset_khz {
                frequency.offset_khz = Some(khz);
            }
        }
        Ok(fields)
    }
}

/// The methods every positioned builder shares, as inherent methods so they need no trait import.
macro_rules! positioned_methods {
    () => {
        /// Where it is, in decimal degrees: north and east positive.
        pub fn at(mut self, latitude: f64, longitude: f64) -> Self {
            self.common.position = Some(Position { latitude, longitude, ambiguity: 0 });
            self
        }

        /// How to draw it, e.g. [`Symbol::CAR`].
        pub fn symbol(mut self, symbol: Symbol) -> Self {
            self.common.symbol = Some(symbol);
            self
        }

        /// Course over ground in degrees clockwise from north, 1-360 (360 is north).
        pub fn course(mut self, degrees: u16) -> Self {
            self.common.fields.course_degrees = Some(degrees);
            self
        }

        /// Speed over ground in knots.
        pub fn speed(mut self, knots: f64) -> Self {
            self.common.fields.speed_knots = Some(knots);
            self
        }

        /// Speed over ground in kilometres per hour, sent in knots.
        pub fn speed_kmh(self, kilometres_per_hour: f64) -> Self {
            self.speed(kilometres_per_hour / 1.852)
        }

        /// Altitude above mean sea level in feet (`/A=`).
        pub fn altitude(mut self, feet: f64) -> Self {
            self.common.fields.altitude_feet = Some(feet);
            self
        }

        /// Altitude above mean sea level in metres, sent in feet.
        pub fn altitude_metres(self, metres: f64) -> Self {
            self.altitude(metres / 0.3048)
        }

        /// Free text after the structured parts. May contain UTF-8.
        pub fn comment(mut self, text: &str) -> Self {
            self.common.fields.comment = text.to_string();
            self
        }

        /// Power, antenna height, gain and directivity codes (`PHGphgd`, APRS12c ch. 7).
        pub fn phg(mut self, phg: Phg) -> Self {
            self.common.fields.phg = Some(phg);
            self
        }

        /// Omnidirectional radio range in miles (`RNGrrrr`, APRS12c ch. 7).
        pub fn range(mut self, miles: f64) -> Self {
            self.common.fields.range_miles = Some(miles);
            self
        }

        /// A voice frequency at the start of the comment, as radios with a tune button read it
        /// (`145.725MHz`, APRS12c ch. 18). Add [`tone`](Self::tone) and [`offset_khz`](Self::offset_khz) after it.
        pub fn frequency(mut self, mhz: f64) -> Self {
            self.common.fields.frequency = Some(VoiceFrequency { mhz, ..VoiceFrequency::default() });
            self
        }

        /// The CTCSS access tone for [`frequency`](Self::frequency), in Hz; its tenths are not sent
        /// (`118.8` sends `T118`).
        pub fn tone(mut self, hz: f64) -> Self {
            self.common.tone = Some(hz);
            self
        }

        /// The transmit offset for [`frequency`](Self::frequency), in kHz, e.g. `-600`; sent in tens of kHz.
        pub fn offset_khz(mut self, khz: i32) -> Self {
            self.common.offset_khz = Some(khz);
            self
        }

        /// A voice frequency with every option (APRS12c ch. 18).
        pub fn voice_frequency(mut self, frequency: VoiceFrequency) -> Self {
            self.common.fields.frequency = Some(frequency);
            self
        }

        /// Sends the position in compressed (base-91) form: shorter and more precise, but with no room for
        /// PHG or other data extensions (APRS12c ch. 9).
        pub fn compressed(mut self) -> Self {
            self.common.fields.compressed = true;
            self
        }

        /// Blanks the last 1-4 digits of the position to hide it: to about 0.1, 1, 10 or 60 nautical miles
        /// (APRS12c ch. 6). Not with [`compressed`](Self::compressed).
        pub fn ambiguity(mut self, digits: u8) -> Self {
            self.common.ambiguity = digits;
            self
        }

        /// Adds a `!DAO!` extension carrying extra position precision, in the WGS84 base-91 form (about 0.2 m).
        pub fn dao(mut self) -> Self {
            self.common.fields.dao = Some(Dao { datum: 'W', precision: DaoPrecision::Base91 });
            self
        }

        /// Base-91 telemetry in the comment (`|ss1122..|`, APRS12c ch. 13): a sequence number and 1-5 analog
        /// values, each 0-8280.
        pub fn telemetry(mut self, sequence: u16, analog: &[u16]) -> Self {
            self.common.fields.telemetry = Some(CommentTelemetry { sequence, analog: analog.to_vec(), digital: None });
            self
        }

        /// Base-91 telemetry in the comment with the 8 digital bits (bit 0 is B1), which can only follow all 5
        /// analog values.
        pub fn telemetry_with_bits(mut self, sequence: u16, analog: &[u16], digital: u8) -> Self {
            self.common.fields.telemetry = Some(CommentTelemetry { sequence, analog: analog.to_vec(), digital: Some(digital) });
            self
        }
    };
}

/// A position report, from [`Station::position`] (APRS12c ch. 8 and 9).
#[derive(Clone, Debug, PartialEq)]
pub struct PositionBuilder {
    station: Station,
    common: Common,
    messaging: bool,
    timestamp: Option<Timestamp>,
}

impl PositionBuilder {
    positioned_methods!();

    /// Says the station can receive APRS messages (`=` or `@` rather than `!` or `/`).
    pub fn messaging(mut self) -> Self {
        self.messaging = true;
        self
    }

    /// When the position was valid, for a report that is not current (APRS12c ch. 6).
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        Ok(Data::Position(PositionReport {
            timestamp: self.timestamp,
            messaging: self.messaging,
            fields: self.common.positioned("a position report")?,
        }))
    }

    /// The packet: the station's header and this report, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// An object, from [`Station::object`] (APRS12c ch. 11).
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectBuilder {
    station: Station,
    common: Common,
    name: String,
    killed: bool,
    timestamp: Option<Timestamp>,
}

impl ObjectBuilder {
    positioned_methods!();

    /// When the object report was made; an object must have one. Usually the time now,
    /// `Timestamp::dhm(day, hour, minute)` in UTC. This crate has no clock, so it cannot fill it in.
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Marks the object permanent, one only this station may change, with the `111111z` timestamp
    /// (APRS12c ch. 18).
    pub fn permanent(self) -> Self {
        self.timestamp(Timestamp::dhm(11, 11, 11))
    }

    /// Kills the object, removing it from maps (`_` rather than `*`).
    pub fn kill(mut self) -> Self {
        self.killed = true;
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        let fields = self.common.positioned("an object")?;
        let timestamp =
            self.timestamp.ok_or_else(|| EncodeError::new("an object needs a timestamp: call timestamp(...) or permanent()"))?;
        Ok(Data::Object(ObjectReport { name: self.name.clone(), killed: self.killed, timestamp: Some(timestamp), fields }))
    }

    /// The packet: the station's header and this object, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// An item, from [`Station::item`] (APRS12c ch. 11).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemBuilder {
    station: Station,
    common: Common,
    name: String,
    killed: bool,
}

impl ItemBuilder {
    positioned_methods!();

    /// Kills the item, removing it from maps (`_` rather than `!`).
    pub fn kill(mut self) -> Self {
        self.killed = true;
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        Ok(Data::Item(ItemReport { name: self.name.clone(), killed: self.killed, fields: self.common.positioned("an item")? }))
    }

    /// The packet: the station's header and this item, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// A Mic-E position report, from [`Station::mic_e`] (APRS12c ch. 10). The position comment is
/// [`MicEMessage::OffDuty`] unless set; [`MicEMessage::Emergency`] sets off alarms, so is only sent when asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct MicEBuilder {
    station: Station,
    common: Common,
    message: MicEMessage,
    messaging: bool,
}

impl MicEBuilder {
    positioned_methods!();

    /// The position comment, e.g. [`MicEMessage::EnRoute`].
    pub fn message(mut self, message: MicEMessage) -> Self {
        self.message = message;
        self
    }

    /// Says the station can receive APRS messages (the `` ` `` type code, APRS 1.2).
    pub fn messaging(mut self) -> Self {
        self.messaging = true;
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        Ok(Data::MicE(MicEReport {
            message: self.message,
            type_code: if self.messaging { Some('`') } else { None },
            fields: self.common.positioned("a Mic-E report")?,
            ..MicEReport::default()
        }))
    }

    /// The packet: the station's source and path, the destination this report computes, and the report.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// A weather report, from [`Station::weather`] (APRS12c ch. 12). With a position it is a position report
/// with the weather station symbol; without, a positionless weather report. Values are in the units APRS
/// sends (mph, Fahrenheit, inches, millibars); the `_celsius` and `_mm` methods convert.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherBuilder {
    station: Station,
    weather: Weather,
    position: Option<Position>,
    symbol: Symbol,
    timestamp: Option<Timestamp>,
    messaging: bool,
    compressed: bool,
    dao: Option<Dao>,
}

impl WeatherBuilder {
    /// Where the weather station is, in decimal degrees: north and east positive.
    pub fn at(mut self, latitude: f64, longitude: f64) -> Self {
        self.position = Some(Position { latitude, longitude, ambiguity: 0 });
        self
    }

    /// The symbol, when not the plain weather station `/_`; it must still be a weather station, e.g.
    /// [`Symbol::WEATHER_STATION_WITH_DIGIPEATER`]. Only with a position.
    pub fn symbol(mut self, symbol: Symbol) -> Self {
        self.symbol = symbol;
        self
    }

    /// When the observations were made. A positionless report must have one, in the month-day form
    /// (`Timestamp::mdhm`); a report with a position without one is current.
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Says the station can receive APRS messages. Only with a position.
    pub fn messaging(mut self) -> Self {
        self.messaging = true;
        self
    }

    /// Sends the position in compressed (base-91) form, with the wind in the course and speed bytes
    /// (APRS12c ch. 9). Only with a position.
    pub fn compressed(mut self) -> Self {
        self.compressed = true;
        self
    }

    /// Adds a `!DAO!` extension in the WGS84 base-91 form. Only with a position.
    pub fn dao(mut self) -> Self {
        self.dao = Some(Dao { datum: 'W', precision: DaoPrecision::Base91 });
        self
    }

    /// Wind direction in degrees clockwise from north, and sustained one-minute speed in mph.
    pub fn wind(mut self, direction_degrees: u16, speed_mph: f64) -> Self {
        self.weather.wind_direction_degrees = Some(direction_degrees);
        self.weather.wind_speed_mph = Some(speed_mph);
        self
    }

    /// The peak wind gust in the last five minutes, in mph.
    pub fn gust(mut self, mph: f64) -> Self {
        self.weather.wind_gust_mph = Some(mph);
        self
    }

    /// Temperature in degrees Fahrenheit, sent to the nearest degree.
    pub fn temperature(mut self, fahrenheit: f64) -> Self {
        self.weather.temperature_f = Some(fahrenheit);
        self
    }

    /// Temperature in degrees Celsius, sent to the nearest degree Fahrenheit.
    pub fn temperature_celsius(self, celsius: f64) -> Self {
        self.temperature(celsius * 9.0 / 5.0 + 32.0)
    }

    /// Relative humidity, 1-100 percent.
    pub fn humidity(mut self, percent: u8) -> Self {
        self.weather.humidity_percent = Some(percent);
        self
    }

    /// Barometric pressure in millibars (hPa), sent to a tenth.
    pub fn pressure(mut self, millibars: f64) -> Self {
        self.weather.pressure_mbar = Some(millibars);
        self
    }

    /// Rainfall in the last hour, in inches.
    pub fn rain_last_hour(mut self, inches: f64) -> Self {
        self.weather.rain_1h_in = Some(inches);
        self
    }

    /// Rainfall in the last 24 hours, in inches.
    pub fn rain_last_24_hours(mut self, inches: f64) -> Self {
        self.weather.rain_24h_in = Some(inches);
        self
    }

    /// Rainfall since local midnight, in inches.
    pub fn rain_since_midnight(mut self, inches: f64) -> Self {
        self.weather.rain_midnight_in = Some(inches);
        self
    }

    /// Rainfall in the last hour, in millimetres, sent in inches.
    pub fn rain_last_hour_mm(self, millimetres: f64) -> Self {
        self.rain_last_hour(millimetres / 25.4)
    }

    /// Rainfall in the last 24 hours, in millimetres, sent in inches.
    pub fn rain_last_24_hours_mm(self, millimetres: f64) -> Self {
        self.rain_last_24_hours(millimetres / 25.4)
    }

    /// Rainfall since local midnight, in millimetres, sent in inches.
    pub fn rain_since_midnight_mm(self, millimetres: f64) -> Self {
        self.rain_since_midnight(millimetres / 25.4)
    }

    /// Luminosity in watts per square metre, 0-1999.
    pub fn luminosity(mut self, watts_per_square_metre: u16) -> Self {
        self.weather.luminosity_w_m2 = Some(watts_per_square_metre);
        self
    }

    /// Snowfall in the last 24 hours, in inches.
    pub fn snowfall(mut self, inches: f64) -> Self {
        self.weather.snow_24h_in = Some(inches);
        self
    }

    /// Every observation at once, replacing any set before.
    pub fn observations(mut self, weather: Weather) -> Self {
        self.weather = weather;
        self
    }

    /// The data, without a header: a position report with a position, otherwise a positionless report.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        let mut weather = self.weather.clone();
        weather.temperature_f = weather.temperature_f.map(libm::round);
        if let Some(position) = self.position {
            let fields = Positioned {
                position,
                symbol: self.symbol,
                compressed: self.compressed,
                dao: self.dao,
                weather: Some(weather),
                ..Positioned::default()
            };
            return Ok(Data::Position(PositionReport { timestamp: self.timestamp, messaging: self.messaging, fields }));
        }
        if self.messaging || self.compressed || self.dao.is_some() || self.symbol != Symbol::WEATHER_STATION {
            return Err(EncodeError::new("a symbol, messaging, compression and DAO need a position: call at(latitude, longitude)"));
        }
        let timestamp = self
            .timestamp
            .ok_or_else(|| EncodeError::new("a positionless weather report needs a timestamp: call timestamp(Timestamp::mdhm(...))"))?;
        Ok(Data::Weather(WeatherReport { timestamp: Some(timestamp), weather, comment: String::new() }))
    }

    /// The packet: the station's header and this report, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// A message to another station, from [`Station::message`] (APRS12c ch. 14).
#[derive(Clone, Debug, PartialEq)]
pub struct MessageBuilder {
    station: Station,
    message: Message,
}

impl MessageBuilder {
    /// A message ID (1-5 letters or digits), which asks the other station to acknowledge it.
    pub fn id(mut self, id: &str) -> Self {
        self.message.message_id = Some(id.to_string());
        self
    }

    /// Uses the reply-ack format (APRS12c ch. 14), which needs [`id`](Self::id): acknowledges the other
    /// station's message `their_id` in the same packet, or with `""` just says this station understands
    /// reply-acks.
    pub fn reply_ack(mut self, their_id: &str) -> Self {
        self.message.reply_ack = Some(their_id.to_string());
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        Ok(Data::Message(self.message.clone()))
    }

    /// The packet: the station's header and this message, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// An acknowledgement or rejection, from [`Station::ack`] or [`Station::reject`] (APRS12c ch. 14).
#[derive(Clone, Debug, PartialEq)]
pub struct AckBuilder {
    station: Station,
    addressee: String,
    id: String,
    reject: bool,
    reply_ack: Option<String>,
}

impl AckBuilder {
    /// For a message that came in reply-ack format (`{MM}AA`), echoes back its `AA` part.
    pub fn reply_ack(mut self, their_ack: &str) -> Self {
        self.reply_ack = Some(their_ack.to_string());
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        let addressee = self.addressee.clone();
        let reply_ack = self.reply_ack.clone();
        Ok(if self.reject {
            Data::Reject(Reject { addressee, rejected_id: self.id.clone(), message_id: None, reply_ack })
        } else {
            Data::Ack(Ack { addressee, acked_id: self.id.clone(), message_id: None, reply_ack })
        })
    }

    /// The packet: the station's header and this acknowledgement or rejection, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// A status report, from [`Station::status`] (APRS12c ch. 16).
#[derive(Clone, Debug, PartialEq)]
pub struct StatusBuilder {
    station: Station,
    status: Status,
}

impl StatusBuilder {
    /// When the status was set, as day, hours and minutes UTC (`Timestamp::dhm`). Not with a locator.
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.status.timestamp = Some(timestamp);
        self
    }

    /// A 4- or 6-character Maidenhead locator and a symbol at the start, e.g. `IO91SX/-`.
    pub fn locator(mut self, maidenhead_locator: &str, symbol: Symbol) -> Self {
        self.status.locator = Some(maidenhead_locator.to_string());
        self.status.symbol = Some(symbol);
        self
    }

    /// A meteor-scatter beam heading and power at the end (`^HP`).
    pub fn beam(mut self, beam: BeamHeading) -> Self {
        self.status.beam = Some(beam);
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        Ok(Data::Status(self.status.clone()))
    }

    /// The packet: the station's header and this status, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// A telemetry report, from [`Station::telemetry`] (APRS12c ch. 13): five analog values and eight digital
/// bits. The channel names, units and scaling go in separate messages: [`Station::telemetry_names`] and the
/// methods after it.
#[derive(Clone, Debug, PartialEq)]
pub struct TelemetryBuilder {
    station: Station,
    sequence: u16,
    analog: Vec<f64>,
    digital: u8,
    comment: String,
}

impl TelemetryBuilder {
    /// Up to 5 analog values, A1 first; any not given are sent empty.
    pub fn analog(mut self, values: &[f64]) -> Self {
        self.analog = values.to_vec();
        self
    }

    /// The 8 digital channels; bit 0 is B1.
    pub fn digital(mut self, bits: u8) -> Self {
        self.digital = bits;
        self
    }

    /// Free text after the values.
    pub fn comment(mut self, text: &str) -> Self {
        self.comment = text.to_string();
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        if self.analog.len() > 5 {
            return Err(EncodeError::new("a telemetry report carries at most 5 analog values (APRS12c ch. 13)"));
        }
        let mut analog: Vec<Option<String>> = self.analog.iter().map(|&v| Some(telemetry_value(v))).collect();
        analog.resize(5, None);
        Ok(Data::Telemetry(Telemetry {
            sequence: format!("{:03}", self.sequence),
            analog,
            bits: Some(bit_string(self.digital)),
            comment: self.comment.clone(),
        }))
    }

    /// The packet: the station's header and this report, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// A telemetry value as sent: three digits for a whole number 0-999, as APRS12c writes them, otherwise
/// the shortest decimal form.
fn telemetry_value(v: f64) -> String {
    if (0.0..=999.0).contains(&v) && libm::trunc(v) == v { format!("{:03}", v as u16) } else { format!("{v}") }
}

/// Eight `0`/`1` characters, B1 (bit 0) first.
fn bit_string(bits: u8) -> String {
    (0..8).map(|i| if bits & (1 << i) != 0 { '1' } else { '0' }).collect()
}
