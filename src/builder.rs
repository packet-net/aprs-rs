//! Fluent construction of the packets an application sends, starting from the sending station.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::RefusalCode;
use crate::packet::{Address, EncodeError, InvalidAddress, Packet, PathEntry};
use crate::{
    Ack, BeamHeading, Bulletin, CommentTelemetry, Dao, DaoPrecision, Data, ItemReport, Message, MicEMessage, MicEReport, ObjectReport, Phg,
    Position, PositionReport, Positioned, Reject, Status, Symbol, Telemetry, TelemetryBits, TelemetryCoefficients, TelemetryNames,
    TelemetryUnits, Timestamp, Tone, VoiceFrequency, Weather, WeatherReport,
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
/// let packet = me.position(51.4543, -0.9781).symbol(Symbol::CAR).course_degrees(88.0).speed_knots(36.0).comment("Mobile").build()?;
/// assert_eq!(packet.to_tnc2(), b"M0LTE-9>APZ001,WIDE1-1,WIDE2-1:!5127.26N/00058.69W>088/036Mobile");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// A station is a value: [`Station::to`] and [`Station::via`] return a new one, so it can be kept and
/// reused. `build()` encodes the packet, so anything APRS12c does not allow is refused there with an
/// [`EncodeError`], as [`Packet::create`] would. `to_data()` gives the [`Data`] alone, for anything the
/// builder does not cover; [`Station::data`] sends it.
///
/// A setter has the name of the field it fills, so its unit is visible at the call site
/// (`altitude_feet`, `speed_kmh`), and a setter that converts changes only the unit (`altitude_m`,
/// `temperature_c`, `rain_1h_mm`). A value between the steps a report holds is not rounded by the
/// builder: the encoder rounds it, once, when the packet is built.
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
        MicEBuilder {
            station: self.clone(),
            common: Common::at(latitude, longitude),
            mic_e_message: MicEMessage::OffDuty,
            messaging: false,
        }
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

    /// A telemetry report with this sequence number, sent as three digits; a sequence over 999 is
    /// refused (APRS12c ch. 13).
    pub fn telemetry(&self, sequence: u16) -> TelemetryBuilder {
        TelemetryBuilder { station: self.clone(), sequence, analog: Vec::new(), bits: "00000000".to_string(), comment: String::new() }
    }

    /// The names of telemetry channels, A1-A5 then B1-B8 (`PARM.`, APRS12c ch. 13), by default this
    /// station's own.
    pub fn telemetry_names(&self, names: &[&str]) -> TelemetryMetadataBuilder {
        self.metadata(Data::TelemetryNames(TelemetryNames {
            addressee: self.source.as_str().to_string(),
            names: names.iter().map(|l| l.to_string()).collect(),
            message_id: None,
        }))
    }

    /// The units of analog telemetry channels, then the labels of the digital ones (`UNIT.`, APRS12c
    /// ch. 13), by default this station's own.
    pub fn telemetry_units(&self, units: &[&str]) -> TelemetryMetadataBuilder {
        self.metadata(Data::TelemetryUnits(TelemetryUnits {
            addressee: self.source.as_str().to_string(),
            units: units.iter().map(|l| l.to_string()).collect(),
            message_id: None,
        }))
    }

    /// The scaling for analog telemetry channels: a, b and c for each in turn, giving a x v^2 + b x v + c
    /// (`EQNS.`, APRS12c ch. 13), by default this station's own.
    pub fn telemetry_coefficients(&self, coefficients: &[f64]) -> TelemetryMetadataBuilder {
        self.metadata(Data::TelemetryCoefficients(TelemetryCoefficients {
            addressee: self.source.as_str().to_string(),
            coefficients: coefficients.to_vec(),
            coefficients_text: Vec::new(),
            message_id: None,
        }))
    }

    /// Which state of each digital channel matches its label (eight `0` or `1` characters, B1 first),
    /// and the project title (`BITS.`, APRS12c ch. 13), by default this station's own.
    pub fn telemetry_bits(&self, bits: &str, project: &str) -> TelemetryMetadataBuilder {
        self.metadata(Data::TelemetryBits(TelemetryBits {
            addressee: self.source.as_str().to_string(),
            bits: bits.to_string(),
            project: project.to_string(),
            message_id: None,
        }))
    }

    /// Any data built by hand, sent from this station.
    pub fn data(&self, data: Data) -> DataBuilder {
        DataBuilder { station: self.clone(), data }
    }

    fn metadata(&self, data: Data) -> TelemetryMetadataBuilder {
        TelemetryMetadataBuilder { station: self.clone(), data }
    }

    fn packet(&self, data: Data) -> Result<Packet, EncodeError> {
        Packet::create(self.source.clone(), self.destination.clone(), self.path.clone(), data)
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

/// Telemetry metadata (`PARM.`, `UNIT.`, `EQNS.` or `BITS.`), from [`Station::telemetry_names`] and the
/// methods after it. It describes the sending station's own telemetry unless
/// [`addressee`](Self::addressee) says another, as a gateway does for a sensor station.
#[derive(Clone, Debug, PartialEq)]
pub struct TelemetryMetadataBuilder {
    station: Station,
    data: Data,
}

impl TelemetryMetadataBuilder {
    /// The station the metadata describes, instead of the sending station itself.
    pub fn addressee(mut self, addressee: &str) -> Self {
        let to = addressee.to_string();
        match &mut self.data {
            Data::TelemetryNames(t) => t.addressee = to,
            Data::TelemetryUnits(t) => t.addressee = to,
            Data::TelemetryCoefficients(t) => t.addressee = to,
            Data::TelemetryBits(t) => t.addressee = to,
            _ => {}
        }
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        Ok(self.data.clone())
    }

    /// The packet: the station's header and this metadata, encoded.
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
    tone: Option<(Tone, u16)>,
    offset_khz: Option<i32>,
    /// An altitude given in metres, which a Mic-E report carries in metres and any other in feet.
    altitude_m: Option<f64>,
}

impl Common {
    fn at(latitude: f64, longitude: f64) -> Common {
        Common { position: Some(Position { latitude, longitude, ambiguity: 0 }), ..Common::default() }
    }

    /// The shared fields, checked for what the builder itself needs; the encoder checks the rest.
    fn positioned(&self, what: &str, mic_e: bool) -> Result<Positioned, EncodeError> {
        let position = self
            .position
            .ok_or_else(|| EncodeError::new(RefusalCode::MissingField, format!("{what} needs a position: call at(latitude, longitude)")))?;
        let symbol = self.symbol.ok_or_else(|| {
            EncodeError::new(RefusalCode::MissingField, format!("{what} needs a symbol: call symbol(...), e.g. symbol(Symbol::CAR)"))
        })?;
        let mut fields = self.fields.clone();
        // An ambiguous position reports the box that holds it: its digits cut, not rounded, then
        // blanked, and the box's centre is what a decoder reads (vectors README, Builder rules).
        // A compressed report refuses ambiguity whatever the box, so it keeps the position as given.
        fields.position = if (1..=4).contains(&self.ambiguity) && !fields.compressed {
            let (latitude, longitude) = (box_centre(position.latitude, self.ambiguity), box_centre(position.longitude, self.ambiguity));
            Position { latitude, longitude, ambiguity: self.ambiguity }
        } else {
            Position { ambiguity: self.ambiguity, ..position }
        };
        fields.symbol = symbol;
        if let Some(metres) = self.altitude_m {
            // Mic-E carries its altitude in whole metres (APRS12c ch. 10); a /A= is in feet.
            fields.altitude_feet = Some(if mic_e {
                crate::steps::round(metres) * crate::comment::METRES_TO_FEET
            } else {
                metres * crate::comment::METRES_TO_FEET
            });
        }
        // A course that rounds to 0 is the spec's unknown course, which Mic-E sends as 0 (APRS12c ch. 10).
        if mic_e && fields.course_degrees.is_some_and(|c| crate::steps::round(c) == 0.0) {
            fields.course_degrees = None;
        }
        if self.tone.is_some() || self.offset_khz.is_some() {
            let frequency = fields.frequency.as_mut().ok_or_else(|| {
                EncodeError::new(RefusalCode::MissingField, "a tone or offset needs a frequency: call frequency(mhz) first")
            })?;
            if let Some((tone, value)) = self.tone {
                frequency.tone = Some(tone);
                // `Toff` and the 1750 Hz burst have no value of their own.
                frequency.tone_value = matches!(tone, Tone::Tone | Tone::Ctcss | Tone::Dcs).then_some(value);
            }
            if let Some(khz) = self.offset_khz {
                frequency.offset_khz = Some(khz);
            }
        }
        Ok(fields)
    }
}

impl Common {
    /// A position inside the range whose ambiguity box is centred beyond it has no form. A form that
    /// writes ambiguity (uncompressed and Mic-E) refuses it; a compressed report refuses ambiguity itself.
    ///
    /// A builder names its refusal in the encoder's order, so a fault ranked before this one (a value
    /// out of range, say) is named first: the report is tried with the position as given, whose
    /// ambiguity the encoder judges itself.
    fn box_fault(&self, fields: &Positioned, wrap: impl Fn(Positioned) -> Data) -> Result<(), EncodeError> {
        let Some(position) = self.position else { return Ok(()) };
        if fields.compressed || !(1..=4).contains(&self.ambiguity) {
            return Ok(());
        }
        let (latitude, longitude) = (box_centre(position.latitude, self.ambiguity), box_centre(position.longitude, self.ambiguity));
        if (position.latitude.abs() <= 90.0 && latitude.abs() > 90.0) || (position.longitude.abs() <= 180.0 && longitude.abs() > 180.0) {
            let mut raw = fields.clone();
            raw.position = Position { ambiguity: self.ambiguity, ..position };
            let data = wrap(raw);
            let tried = match &data {
                Data::MicE(r) => crate::mic_e::encode(r).map(|_| ()),
                other => other.encode().map(|_| ()),
            };
            return Err(tried.err().unwrap_or_else(|| {
                EncodeError::new(
                    RefusalCode::ValueHasNoForm,
                    "an ambiguous position at 90 degrees latitude or 180 degrees longitude has a box centred past it",
                )
            }));
        }
        Ok(())
    }
}

/// The centre of the box that holds `degrees` once `ambiguity` (1-4) digits are blanked: the
/// digits are cut to the box, not rounded, so the box is the one the position is in.
fn box_centre(degrees: f64, ambiguity: u8) -> f64 {
    // The box size in hundredths of a minute: a hundredth's digit, a tenth's, a minute's, ten
    // minutes', or a degree.
    let size = [1.0, 10.0, 100.0, 1000.0, 6000.0][usize::from(ambiguity)];
    // A value within 1e-6 of a box's edge, measured in boxes, is on the edge: 80.49 degrees is
    // 80 degrees 29.4 minutes, though in binary it computes as 29.39999...
    let boxes = degrees.abs() * 6000.0 / size;
    let centre = (libm::floor(boxes + 1e-6) * size + size / 2.0) / 6000.0;
    if degrees.is_sign_negative() { -centre } else { centre }
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

        /// Course over ground in degrees clockwise from north, 1-360: 360 is north, and 0 is the
        /// spec's unknown course (APRS12c ch. 7 and 10), which a compressed report cannot send. A
        /// course between the steps the report holds (whole degrees, or 4 degrees compressed) is
        /// rounded to the nearest, once, when the packet is built.
        pub fn course_degrees(mut self, degrees: f64) -> Self {
            self.common.fields.course_degrees = Some(degrees);
            self
        }

        /// Speed over ground in knots.
        pub fn speed_knots(mut self, knots: f64) -> Self {
            self.common.fields.speed_knots = Some(knots);
            self
        }

        /// Speed over ground in kilometres per hour, sent in knots.
        pub fn speed_kmh(self, kilometres_per_hour: f64) -> Self {
            self.speed_knots(kilometres_per_hour / 1.852)
        }

        /// Altitude above mean sea level in feet (`/A=`), sent to the nearest foot.
        pub fn altitude_feet(mut self, feet: f64) -> Self {
            self.common.fields.altitude_feet = Some(feet);
            self.common.altitude_m = None;
            self
        }

        /// Altitude above mean sea level in metres: in a Mic-E report, in its altitude bytes to the
        /// nearest metre; otherwise converted to feet for a `/A=`, to the nearest foot.
        pub fn altitude_m(mut self, metres: f64) -> Self {
            self.common.fields.altitude_feet = None;
            self.common.altitude_m = Some(metres);
            self
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
        pub fn range_miles(mut self, miles: f64) -> Self {
            self.common.fields.range_miles = Some(miles);
            self
        }

        /// A voice frequency at the start of the comment, as radios with a tune button read it
        /// (`145.725MHz`, APRS12c ch. 18), rounded to the nearest kHz. Add [`tone`](Self::tone) and
        /// [`offset_khz`](Self::offset_khz) after it.
        pub fn frequency(mut self, mhz: f64) -> Self {
            self.common.fields.frequency = Some(VoiceFrequency { mhz, ..VoiceFrequency::default() });
            self
        }

        /// The tone field for [`frequency`](Self::frequency): a kind, [`Tone::Tone`], [`Tone::Ctcss`] or
        /// [`Tone::Dcs`], and its value as sent, a whole number of Hz or a DCS code (`Tone::Tone, 118`
        /// sends `T118`; 118.8 Hz is sent as 118). [`Tone::Off`] and [`Tone::ToneBurst`] have no value,
        /// which is ignored.
        pub fn tone(mut self, tone: Tone, value: u16) -> Self {
            self.common.tone = Some((tone, value));
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
        /// (APRS12c ch. 6). The digits are cut, so the report gives the box the position is in, and a
        /// decoder reads its centre. Not with [`compressed`](Self::compressed) or [`dao`](Self::dao).
        pub fn ambiguity(mut self, digits: u8) -> Self {
            self.common.ambiguity = digits;
            self
        }

        /// Adds a `!DAO!` extension carrying extra position precision, in the WGS84 base-91 form (about 0.2 m).
        pub fn dao(mut self) -> Self {
            self.common.fields.dao = Some(Dao { datum: 'W', precision: DaoPrecision::Base91 });
            self
        }

        /// Base-91 telemetry in the comment (`|ss1122..|`, APRS12c ch. 13): a sequence number, 1-5 analog
        /// values, each 0-8280, and optionally the 8 digital bits, which can only follow all 5 analog
        /// values.
        pub fn telemetry(mut self, telemetry: CommentTelemetry) -> Self {
            self.common.fields.telemetry = Some(telemetry);
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
            fields: {
                let fields = self.common.positioned("a position report", false)?;
                self.common.box_fault(&fields, |fields| {
                    Data::Position(PositionReport { timestamp: self.timestamp, messaging: self.messaging, fields })
                })?;
                fields
            },
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
    pub fn killed(mut self) -> Self {
        self.killed = true;
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        let fields = self.common.positioned("an object", false)?;
        self.common.box_fault(&fields, |fields| {
            Data::Object(ObjectReport { name: self.name.clone(), killed: self.killed, timestamp: self.timestamp, fields })
        })?;
        let timestamp = self.timestamp.ok_or_else(|| {
            EncodeError::new(RefusalCode::MissingField, "an object needs a timestamp: call timestamp(...) or permanent()")
        })?;
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
    pub fn killed(mut self) -> Self {
        self.killed = true;
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        let fields = self.common.positioned("an item", false)?;
        self.common.box_fault(&fields, |fields| Data::Item(ItemReport { name: self.name.clone(), killed: self.killed, fields }))?;
        Ok(Data::Item(ItemReport { name: self.name.clone(), killed: self.killed, fields }))
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
    mic_e_message: MicEMessage,
    messaging: bool,
}

impl MicEBuilder {
    positioned_methods!();

    /// The position comment, e.g. [`MicEMessage::EnRoute`].
    pub fn mic_e_message(mut self, message: MicEMessage) -> Self {
        self.mic_e_message = message;
        self
    }

    /// Says the station can receive APRS messages: the `` ` `` type code, where without this it is
    /// `'` (APRS 1.2).
    pub fn messaging(mut self) -> Self {
        self.messaging = true;
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        Ok(Data::MicE(MicEReport {
            mic_e_message: self.mic_e_message,
            // The type code says whether the station can message (APRS 1.2 Mic-E type codes).
            type_code: Some(if self.messaging { '`' } else { '\'' }),
            // An altitude given in feet goes in a `/A=`, one in metres in the Mic-E altitude bytes.
            altitude_in_comment: self.common.altitude_m.is_none() && self.common.fields.altitude_feet.is_some(),
            fields: {
                let fields = self.common.positioned("a Mic-E report", true)?;
                self.common.box_fault(&fields, |fields| {
                    Data::MicE(MicEReport {
                        mic_e_message: self.mic_e_message,
                        type_code: Some(if self.messaging { '`' } else { '\'' }),
                        altitude_in_comment: self.common.altitude_m.is_none() && self.common.fields.altitude_feet.is_some(),
                        fields,
                        ..MicEReport::default()
                    })
                })?;
                fields
            },
            ..MicEReport::default()
        }))
    }

    /// The packet: the station's source and path, the destination this report computes, and the report.
    /// An altitude given in feet goes in a `/A=`, one in metres in the Mic-E altitude bytes.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}

/// A weather report, from [`Station::weather`] (APRS12c ch. 12). With a position it is a position report
/// with the weather station symbol; without, a positionless weather report. Values are in the units APRS
/// sends (mph, Fahrenheit, inches, millibars); the `_c` and `_mm` methods convert.
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

    /// Wind direction in degrees clockwise from north. A direction between the steps the report holds
    /// (whole degrees, or 4 degrees compressed) is rounded to the nearest, once, when the packet is
    /// built. A compressed report needs the speed too.
    pub fn wind_direction_degrees(mut self, degrees: f64) -> Self {
        self.weather.wind_direction_degrees = Some(degrees);
        self
    }

    /// The sustained one-minute wind speed in mph.
    pub fn wind_speed_mph(mut self, mph: f64) -> Self {
        self.weather.wind_speed_mph = Some(mph);
        self
    }

    /// The peak wind gust in the last five minutes, in mph.
    pub fn wind_gust_mph(mut self, mph: f64) -> Self {
        self.weather.wind_gust_mph = Some(mph);
        self
    }

    /// Temperature in degrees Fahrenheit, sent to the nearest degree.
    pub fn temperature_f(mut self, fahrenheit: f64) -> Self {
        self.weather.temperature_f = Some(fahrenheit);
        self
    }

    /// Temperature in degrees Celsius, sent to the nearest degree Fahrenheit.
    pub fn temperature_c(self, celsius: f64) -> Self {
        self.temperature_f(celsius * 9.0 / 5.0 + 32.0)
    }

    /// Relative humidity, 1-100 percent, rounded to the nearest whole percent when the packet is built
    /// (99.7 is sent as 100%; a value that rounds to 0 is refused, since the report's `h00` means 100).
    pub fn humidity_percent(mut self, percent: f64) -> Self {
        self.weather.humidity_percent = Some(percent);
        self
    }

    /// Barometric pressure in millibars (hPa), sent to a tenth.
    pub fn pressure_mbar(mut self, millibars: f64) -> Self {
        self.weather.pressure_mbar = Some(millibars);
        self
    }

    /// Rainfall in the last hour, in inches.
    pub fn rain_1h_in(mut self, inches: f64) -> Self {
        self.weather.rain_1h_in = Some(inches);
        self
    }

    /// Rainfall in the last 24 hours, in inches.
    pub fn rain_24h_in(mut self, inches: f64) -> Self {
        self.weather.rain_24h_in = Some(inches);
        self
    }

    /// Rainfall since local midnight, in inches.
    pub fn rain_midnight_in(mut self, inches: f64) -> Self {
        self.weather.rain_midnight_in = Some(inches);
        self
    }

    /// Rainfall in the last hour, in millimetres, sent in inches.
    pub fn rain_1h_mm(self, millimetres: f64) -> Self {
        self.rain_1h_in(millimetres / 25.4)
    }

    /// Rainfall in the last 24 hours, in millimetres, sent in inches.
    pub fn rain_24h_mm(self, millimetres: f64) -> Self {
        self.rain_24h_in(millimetres / 25.4)
    }

    /// Rainfall since local midnight, in millimetres, sent in inches.
    pub fn rain_midnight_mm(self, millimetres: f64) -> Self {
        self.rain_midnight_in(millimetres / 25.4)
    }

    /// Luminosity in watts per square metre, 0-1999, rounded to the nearest whole number when the
    /// packet is built.
    pub fn luminosity_w_m2(mut self, watts_per_square_metre: f64) -> Self {
        self.weather.luminosity_w_m2 = Some(watts_per_square_metre);
        self
    }

    /// Snowfall in the last 24 hours, in inches.
    pub fn snow_24h_in(mut self, inches: f64) -> Self {
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
        let weather = self.weather.clone();
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
            return Err(EncodeError::new(
                RefusalCode::MissingField,
                "a symbol, messaging, compression and DAO need a position: call at(latitude, longitude)",
            ));
        }
        let timestamp = self.timestamp.ok_or_else(|| {
            EncodeError::new(
                RefusalCode::MissingField,
                "a positionless weather report needs a timestamp: call timestamp(Timestamp::mdhm(...))",
            )
        })?;
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
    pub fn message_id(mut self, id: &str) -> Self {
        self.message.message_id = Some(id.to_string());
        self
    }

    /// Uses the reply-ack format (APRS12c ch. 14), which needs [`message_id`](Self::message_id): acknowledges the other
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

    /// A 4- or 6-character Maidenhead locator at the start, e.g. `IO91SX`; lower case is written in
    /// upper case, as a decoder reads it. It goes with a
    /// [`symbol`](Self::symbol): one without the other is refused when built.
    pub fn locator(mut self, maidenhead_locator: &str) -> Self {
        self.status.locator = Some(maidenhead_locator.to_ascii_uppercase());
        self
    }

    /// The symbol after the locator, e.g. [`Symbol::HOUSE`]. Only with a [`locator`](Self::locator).
    pub fn symbol(mut self, symbol: Symbol) -> Self {
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
    analog: Vec<Option<f64>>,
    bits: String,
    comment: String,
}

impl TelemetryBuilder {
    /// Up to 5 analog values, A1 first, each of which may be `None` to send that channel empty, before a
    /// filled one or after; any not given are sent empty.
    pub fn analog(mut self, values: &[Option<f64>]) -> Self {
        self.analog = values.to_vec();
        self
    }

    /// The 8 digital channels as eight `0` or `1` characters, B1 first; all `0` unless set.
    pub fn bits(mut self, bits: &str) -> Self {
        self.bits = bits.to_string();
        self
    }

    /// Free text after the values.
    pub fn comment(mut self, text: &str) -> Self {
        self.comment = text.to_string();
        self
    }

    /// The data, without a header.
    pub fn to_data(&self) -> Result<Data, EncodeError> {
        if self.sequence > 999 {
            return Err(EncodeError::new(RefusalCode::ValueOutOfRange, "a telemetry sequence is 0-999, three digits (APRS12c ch. 13)"));
        }
        if self.analog.len() > 5 {
            return Err(EncodeError::new(RefusalCode::TooLong, "a telemetry report carries at most 5 analog values (APRS12c ch. 13)"));
        }
        let mut analog = self.analog.clone();
        analog.resize(5, None);
        Ok(Data::Telemetry(Telemetry {
            sequence: format!("{:03}", self.sequence),
            analog,
            analog_text: Vec::new(),
            bits: Some(self.bits.clone()),
            comment: self.comment.clone(),
        }))
    }

    /// The packet: the station's header and this report, encoded.
    pub fn build(&self) -> Result<Packet, EncodeError> {
        self.station.packet(self.to_data()?)
    }
}
