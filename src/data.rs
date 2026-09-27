//! What an APRS information field says: one type per APRS data type.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::Packet;

/// The decoded content of an information field.
#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    /// A position report: `!`, `=`, `/` or `@` (APRS12c ch. 8 and 9).
    Position(PositionReport),
    /// A Mic-E position report: `` ` `` or `'` (APRS12c ch. 10).
    MicE(MicEReport),
    /// An object report: `;` (APRS12c ch. 11).
    Object(ObjectReport),
    /// An item report: `)` (APRS12c ch. 11).
    Item(ItemReport),
    /// A message to a station (APRS12c ch. 14).
    Message(Message),
    /// A message acknowledgement.
    Ack(Ack),
    /// A message rejection.
    Reject(Reject),
    /// A bulletin or announcement (`BLNn`, `BLNx`, `BLNnGROUP`).
    Bulletin(Bulletin),
    /// A National Weather Service bulletin, addressed `NWS-` or `NWS_`.
    NwsBulletin(Bulletin),
    /// Telemetry channel names, `PARM.` (APRS12c ch. 13).
    TelemetryNames(TelemetryLabels),
    /// Telemetry channel units, `UNIT.`.
    TelemetryUnits(TelemetryLabels),
    /// Telemetry equation coefficients, `EQNS.`.
    TelemetryCoefficients(TelemetryCoefficients),
    /// Telemetry bit sense and project name, `BITS.`.
    TelemetryBits(TelemetryBits),
    /// A query sent as a message: `?APRSx`, `?WX`, ... (APRS12c ch. 15).
    DirectedQuery(DirectedQuery),
    /// A status report: `>` (APRS12c ch. 16).
    Status(Status),
    /// A telemetry report: `T#` (APRS12c ch. 13).
    Telemetry(Telemetry),
    /// A positionless weather report: `_` (APRS12c ch. 12).
    Weather(WeatherReport),
    /// Raw weather station output: `#`, `*`, `$ULTW`, `!!` (APRS12c ch. 12).
    RawWeather(RawWeather),
    /// A raw NMEA sentence: `$GPxxx` (APRS12c ch. 7).
    Nmea(Nmea),
    /// A Maidenhead locator beacon: `[` (obsolete, APRS12c ch. 7).
    MaidenheadBeacon(MaidenheadBeacon),
    /// A general query: `?APRS?`, `?IGATE?`, ... (APRS12c ch. 15).
    Query(Query),
    /// Station capabilities: `<` (APRS12c ch. 15).
    Capabilities(Capabilities),
    /// Third-party traffic: `}` and a whole packet inside (APRS12c ch. 17).
    ThirdParty(Box<Packet>),
    /// User-defined data: `{` (APRS12c ch. 19).
    UserDefined(UserDefined),
    /// Invalid or test data: `,` (APRS12c ch. 20).
    Test(TestData),
    /// An Agrelo DFJr bearing report: `%`.
    AgreloDf(AgreloDf),
    /// Nothing that could be decoded; the diagnostics say why.
    Unrecognized(Unrecognized),
}

/// Why nothing was decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unrecognized {
    /// The information field is empty.
    Empty,
    /// The first byte is not an APRS data type identifier: plain text, a beacon, another protocol.
    NotAprs,
    /// A reserved data type identifier with no defined format.
    ReservedDataType,
    /// It is APRS, but broken; the diagnostics say how.
    Malformed,
}

/// A latitude and longitude.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Position {
    /// Degrees, north positive. For an ambiguous position, the centre of the area it covers.
    pub latitude: f64,
    /// Degrees, east positive.
    pub longitude: f64,
    /// How many trailing digits were blanked with spaces (0-4) (APRS12c ch. 6).
    pub ambiguity: u8,
}

/// A map symbol: a table (or overlay) character and a code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// `/` for the primary table, `\` for the alternate, or an overlay (`0`-`9`, `A`-`Z`) on the alternate.
    pub table: char,
    /// The symbol code, `!` to `~`.
    pub code: char,
}

impl Default for Symbol {
    fn default() -> Self {
        Symbol { table: '/', code: '/' }
    }
}

impl Symbol {
    /// A symbol from its table (or overlay) character and code, e.g. `Symbol::new('/', '>')` for a car.
    /// Every defined symbol also has a name: [`Symbol::CAR`].
    pub const fn new(table: char, code: char) -> Symbol {
        Symbol { table, code }
    }

    /// This alternate-table symbol with an overlay character, `0`-`9` or `A`-`Z`, e.g.
    /// `Symbol::GATEWAY.with_overlay('I')` for an IGate. `None` for a primary-table symbol, which takes no
    /// overlay, or any other character (APRS12c ch. 21).
    pub fn with_overlay(self, overlay: char) -> Option<Symbol> {
        (self.table != '/' && (overlay.is_ascii_digit() || overlay.is_ascii_uppercase()))
            .then_some(Symbol { table: overlay, code: self.code })
    }
}

/// A timestamp as sent. The fields are kept even when out of range, so they can be reported;
/// [`Timestamp::is_valid`] says whether they make sense.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timestamp {
    /// Day of month, hour and minute, UTC (`DDHHMMz`) or local time (`DDHHMM/`).
    DayHourMinute {
        /// Day of the month.
        day: u8,
        /// Hour.
        hour: u8,
        /// Minute.
        minute: u8,
        /// UTC (`z`) rather than local time (`/`).
        utc: bool,
    },
    /// Hour, minute and second, UTC (`HHMMSSh`).
    HourMinuteSecond {
        /// Hour.
        hour: u8,
        /// Minute.
        minute: u8,
        /// Second.
        second: u8,
    },
    /// Month, day, hour and minute, UTC (`MMDDHHMM`), used by positionless weather reports.
    MonthDayHourMinute {
        /// Month.
        month: u8,
        /// Day of the month.
        day: u8,
        /// Hour.
        hour: u8,
        /// Minute.
        minute: u8,
    },
}

/// Compressed position type byte (APRS12c ch. 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompressionType {
    /// Whether the GPS fix is current.
    pub fix: GpsFix,
    /// The NMEA sentence the position came from.
    pub source: NmeaSource,
    /// What compressed the position.
    pub origin: CompressionOrigin,
}

/// GPS fix in a compression type byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpsFix {
    /// Old (last known) fix.
    Old,
    /// Current fix.
    Current,
}

/// NMEA source in a compression type byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NmeaSource {
    /// Other.
    Other,
    /// GLL.
    Gll,
    /// GGA; the cs bytes then carry altitude.
    Gga,
    /// RMC.
    Rmc,
}

/// Compression origin in a compression type byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionOrigin {
    /// Compressed.
    Compressed,
    /// TNC BText.
    TncBeaconText,
    /// Software (DOS, Mac, Win+SA).
    Software,
    /// Reserved (TBD).
    Reserved3,
    /// KPC3.
    Kpc3,
    /// Pico.
    Pico,
    /// Other tracker.
    OtherTracker,
    /// Digipeater conversion.
    DigipeaterConversion,
}

/// Power, height, gain and directivity (APRS12c ch. 7), as the codes sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phg {
    /// Power code 0-9: power = code² watts.
    pub power: u8,
    /// Height code 0-9 (or higher): height = 10 x 2^code feet.
    pub height: u8,
    /// Gain code 0-9: dBi.
    pub gain: u8,
    /// Directivity code 0-9: 0 omni, 1-8 45 x code degrees.
    pub directivity: u8,
    /// PHGR: beacons per hour.
    pub beacons_per_hour: Option<u8>,
}

/// Omni DF signal strength, `DFSshgd` (APRS12c ch. 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DfSignalStrength {
    /// Strength code 0-9 (S-points).
    pub strength: u8,
    /// Height code.
    pub height: u8,
    /// Gain code.
    pub gain: u8,
    /// Directivity code.
    pub directivity: u8,
}

/// An area object's shape (APRS12c ch. 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaShape {
    /// 0: open circle.
    OpenCircle,
    /// 1: line, down and to the right.
    LineDownRight,
    /// 2: open ellipse.
    OpenEllipse,
    /// 3: open triangle.
    OpenTriangle,
    /// 4: open box.
    OpenBox,
    /// 5: filled circle.
    FilledCircle,
    /// 6: line, down and to the left.
    LineDownLeft,
    /// 7: filled ellipse.
    FilledEllipse,
    /// 8: filled triangle.
    FilledTriangle,
    /// 9: filled box.
    FilledBox,
}

/// An area object's colour (APRS12c ch. 11): eight colours, each high or low intensity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaColor {
    /// `/0`: high intensity.
    Black,
    /// `/1`: high intensity.
    Blue,
    /// `/2`: high intensity.
    Green,
    /// `/3`: high intensity.
    Cyan,
    /// `/4`: high intensity.
    Red,
    /// `/5`: high intensity.
    Violet,
    /// `/6`: high intensity.
    Yellow,
    /// `/7`: high intensity.
    Gray,
    /// `/8`: low intensity.
    BlackLow,
    /// `/9`: low intensity.
    BlueLow,
    /// `10`: low intensity.
    GreenLow,
    /// `11`: low intensity.
    CyanLow,
    /// `12`: low intensity.
    RedLow,
    /// `13`: low intensity.
    VioletLow,
    /// `14`: low intensity.
    YellowLow,
    /// `15`: low intensity.
    GrayLow,
}

/// An area object, `Tyy/Cxx` and optionally `{www}` (APRS12c ch. 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AreaObject {
    /// The shape.
    pub shape: AreaShape,
    /// The latitude offset code `yy`: the square root of the offset in 1/100 degree.
    pub lat_offset: u8,
    /// The colour.
    pub color: AreaColor,
    /// The longitude offset code `xx`.
    pub lon_offset: u8,
    /// A line object's corridor width, `{www}`.
    pub corridor_width_miles: Option<u16>,
}

/// A DF bearing and quality, `/BRG/NRQ` (APRS12c ch. 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DfBearing {
    /// Bearing in degrees.
    pub bearing_degrees: u16,
    /// N: number of hits, 0-9 (0: the report is meaningless).
    pub number: u8,
    /// R: range code.
    pub range: u8,
    /// Q: quality code (beamwidth).
    pub quality: u8,
}

/// A storm or hurricane report in an object (APRS12c ch. 12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Storm {
    /// What it is.
    pub kind: StormKind,
    /// Sustained wind speed, knots.
    pub sustained_wind_knots: Option<u16>,
    /// Peak gusts, knots.
    pub gust_knots: Option<u16>,
    /// Central pressure, millibars.
    pub central_pressure_mbar: Option<u16>,
    /// Radius of hurricane-force winds, nautical miles.
    pub hurricane_radius_nm: Option<u16>,
    /// Radius of tropical-storm winds, nautical miles.
    pub tropical_storm_radius_nm: Option<u16>,
    /// Radius of whole gale winds, nautical miles.
    pub whole_gale_radius_nm: Option<u16>,
}

/// The kind of storm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StormKind {
    /// `TS`: tropical storm.
    TropicalStorm,
    /// `HC`: hurricane.
    Hurricane,
    /// `TD`: tropical depression.
    TropicalDepression,
}

/// `!DAO!` extra precision and datum (APRS12c ch. 9 addendum).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dao {
    /// Datum letter: `W` for WGS84 (upper case), others as sent.
    pub datum: char,
    /// How the extra digits are written.
    pub precision: DaoPrecision,
}

/// How a `!DAO!` writes its extra digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaoPrecision {
    /// No extra digits (spaces).
    None,
    /// Upper-case datum: one decimal digit each, thousandths of a minute.
    Thousandths,
    /// Lower-case datum: one base-91 character each, 1/91 of a hundredth of a minute.
    Base91,
}

/// Base-91 comment telemetry, `|ss11223344556677|` (APRS12c ch. 13 addendum).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentTelemetry {
    /// Sequence number, 0-8280.
    pub sequence: u16,
    /// One to five analog values, 0-8280.
    pub analog: Vec<u16>,
    /// The digital bits, if sent (B1 is the least significant bit).
    pub digital: Option<u8>,
}

/// An APRS 1.2 voice frequency and its settings (APRS12c ch. 18 addendum).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoiceFrequency {
    /// The frequency in MHz.
    pub mhz: f64,
    /// The tone field, if one was sent.
    pub tone: Option<Tone>,
    /// The tone frequency (whole Hz) or DCS code, e.g. `100` or `23`.
    pub tone_value: Option<u16>,
    /// Transmit offset in kHz, signed.
    pub offset_khz: Option<i32>,
    /// Range, in miles or (with `range_km`) kilometres.
    pub range: Option<u16>,
    /// The range is in kilometres.
    pub range_km: bool,
    /// Narrow band (`N` after the frequency's `MHz`).
    pub narrow: bool,
    /// Written with 10 kHz resolution (`MHz` form with two decimals).
    pub ten_khz_resolution: bool,
}

/// Tone squelch on a voice frequency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// `Toff`: no tone, no DCS.
    Off,
    /// `Tnnn`: a tone is sent on transmit.
    Tone,
    /// `Cnnn`: CTCSS tone squelch.
    Ctcss,
    /// `Dnnn`: digital-coded squelch.
    Dcs,
    /// `1750`: a 1750 Hz tone burst opens the repeater (APRS12c ch. 18).
    ToneBurst,
}

/// Weather data (APRS12c ch. 12), in the units APRS sends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Weather {
    /// Wind direction, degrees.
    pub wind_direction_degrees: Option<u16>,
    /// Sustained wind speed, mph.
    pub wind_speed_mph: Option<f64>,
    /// Peak gust in the last 5 minutes, mph.
    pub wind_gust_mph: Option<f64>,
    /// Temperature, degrees Fahrenheit.
    pub temperature_f: Option<f64>,
    /// Rain in the last hour, inches.
    pub rain_1h_in: Option<f64>,
    /// Rain in the last 24 hours, inches.
    pub rain_24h_in: Option<f64>,
    /// Rain since local midnight, inches.
    pub rain_midnight_in: Option<f64>,
    /// Raw rain counter (`#`).
    pub rain_raw: Option<u32>,
    /// Relative humidity, percent.
    pub humidity_percent: Option<u8>,
    /// Barometric pressure, millibars (tenths as sent).
    pub pressure_mbar: Option<f64>,
    /// Luminosity, W/m².
    pub luminosity_w_m2: Option<u16>,
    /// Snowfall in the last 24 hours, inches.
    pub snow_24h_in: Option<f64>,
    /// Software type character, e.g. `e` or `W`.
    pub software: Option<char>,
    /// Weather unit type, e.g. `Dvs`.
    pub unit: Option<String>,
    /// Fields in the weather run this decoder does not name (letter and value as sent).
    pub extra: Vec<WeatherField>,
}

/// A weather field with a letter the spec does not define.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeatherField {
    /// The field letter.
    pub letter: char,
    /// The value as sent.
    pub value: String,
}

/// Fields shared by positions, Mic-E reports, objects and items.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Positioned {
    /// Where.
    pub position: Position,
    /// The map symbol.
    pub symbol: Symbol,
    /// Sent in compressed form (APRS12c ch. 9).
    pub compressed: bool,
    /// The compressed type byte, when it says anything (course/speed, range or altitude present).
    pub compression: Option<CompressionType>,
    /// Course, degrees (1-360; 360 is north).
    pub course_degrees: Option<u16>,
    /// Speed, knots.
    pub speed_knots: Option<f64>,
    /// Altitude, feet.
    pub altitude_feet: Option<f64>,
    /// Power, height, gain, directivity.
    pub phg: Option<Phg>,
    /// Radio range, miles (`RNGrrrr` or compressed).
    pub range_miles: Option<f64>,
    /// DF signal strength.
    pub dfs: Option<DfSignalStrength>,
    /// Area object shape and size.
    pub area: Option<AreaObject>,
    /// DF bearing and quality.
    pub df_bearing: Option<DfBearing>,
    /// Storm data.
    pub storm: Option<Storm>,
    /// `!DAO!` datum and precision.
    pub dao: Option<Dao>,
    /// Base-91 comment telemetry.
    pub telemetry: Option<CommentTelemetry>,
    /// Voice frequency.
    pub frequency: Option<VoiceFrequency>,
    /// Weather, for a weather station symbol.
    pub weather: Option<Weather>,
    /// Signpost text, `{...}` on the signpost symbol.
    pub signpost: Option<String>,
    /// What is left of the comment once everything above is lifted out.
    pub comment: String,
}

/// A position report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PositionReport {
    /// When, if timestamped (`/` or `@`).
    pub timestamp: Option<Timestamp>,
    /// The sender can do APRS messaging (`=` or `@`).
    pub messaging: bool,
    /// The shared positioned fields.
    pub fields: Positioned,
}

/// A Mic-E position report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MicEReport {
    /// The Mic-E message (status) code.
    pub message: MicEMessage,
    /// Sent with `'` (old or non-current data) rather than `` ` ``.
    pub old_data: bool,
    /// The device type code after the symbol: `` ` ``, `'`, `>`, `]` or a space.
    pub type_code: Option<char>,
    /// The device suffix after the status text, e.g. `=` or `_3`.
    pub device_suffix: String,
    /// A grid locator in the status text.
    pub locator: Option<String>,
    /// Obsolete Mic-E telemetry.
    pub legacy_telemetry: Vec<u8>,
    /// The destination address's SSID, which selects a digipeat path.
    pub destination_ssid: u8,
    /// The shared positioned fields.
    pub fields: Positioned,
}

/// A Mic-E message code (APRS12c ch. 10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MicEMessage {
    /// M0.
    #[default]
    OffDuty,
    /// M1.
    EnRoute,
    /// M2.
    InService,
    /// M3.
    Returning,
    /// M4.
    Committed,
    /// M5.
    Special,
    /// M6.
    Priority,
    /// C0-C6.
    Custom(u8),
    /// All message bits zero.
    Emergency,
    /// A mix of standard and custom bits, which has no meaning.
    Unknown,
}

/// An object report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectReport {
    /// The object's name, 1-9 characters.
    pub name: String,
    /// Killed (`_`) rather than alive (`*`).
    pub killed: bool,
    /// When.
    pub timestamp: Option<Timestamp>,
    /// The shared positioned fields.
    pub fields: Positioned,
}

/// An item report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemReport {
    /// The item's name, 3-9 characters.
    pub name: String,
    /// Killed (`_`) rather than alive (`!`).
    pub killed: bool,
    /// The shared positioned fields.
    pub fields: Positioned,
}

/// A message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    /// Who it is for.
    pub addressee: String,
    /// The text.
    pub text: String,
    /// The message number, if an acknowledgement is wanted.
    pub message_id: Option<String>,
    /// A reply-ack: `Some("")` says the sender supports reply-acks; otherwise the ID being acked.
    pub reply_ack: Option<String>,
}

/// A message acknowledgement.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ack {
    /// Who it is for.
    pub addressee: String,
    /// The message ID acknowledged.
    pub acked_id: String,
    /// A message ID of its own (not allowed, UAP 5.32).
    pub message_id: Option<String>,
    /// A reply-ack.
    pub reply_ack: Option<String>,
}

/// A message rejection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reject {
    /// Who it is for.
    pub addressee: String,
    /// The message ID rejected.
    pub rejected_id: String,
    /// A message ID of its own (not allowed, UAP 5.32).
    pub message_id: Option<String>,
    /// A reply-ack.
    pub reply_ack: Option<String>,
}

/// A bulletin, announcement or NWS bulletin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bulletin {
    /// The addressee, e.g. `BLN3` or `NWS-WARN`.
    pub addressee: String,
    /// The text.
    pub text: String,
    /// A message ID, if sent.
    pub message_id: Option<String>,
}

/// Telemetry channel names or units.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TelemetryLabels {
    /// The station the metadata describes.
    pub addressee: String,
    /// Up to 13 labels, in channel order (A1-A5, then B1-B8); empty for a channel not named.
    pub labels: Vec<String>,
    /// The message ID, when the metadata was sent as a numbered message.
    pub message_id: Option<String>,
}

/// Telemetry equation coefficients.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TelemetryCoefficients {
    /// The station the metadata describes.
    pub addressee: String,
    /// Up to 15 values, a, b, c for each analog channel in turn, as sent.
    pub coefficients: Vec<String>,
    /// The message ID, when the metadata was sent as a numbered message.
    pub message_id: Option<String>,
}

/// Telemetry bit sense and project name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TelemetryBits {
    /// The station the metadata describes.
    pub addressee: String,
    /// The eight bit-sense characters, B1 first.
    pub bits: String,
    /// The project title.
    pub project: String,
    /// The message ID, when the metadata was sent as a numbered message.
    pub message_id: Option<String>,
}

/// A directed query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectedQuery {
    /// The station asked.
    pub addressee: String,
    /// The query type, e.g. `APRSD`, `PING?` or `WX`.
    pub query_type: String,
    /// The station the query is about, for the types that name one.
    pub target: Option<String>,
}

/// A status report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// When (DDHHMMz only).
    pub timestamp: Option<Timestamp>,
    /// A Maidenhead locator, for the grid format.
    pub locator: Option<String>,
    /// The symbol, for the grid format.
    pub symbol: Option<Symbol>,
    /// A meteor-scatter beam heading and power, `^HP` (APRS12c ch. 16).
    pub beam: Option<BeamHeading>,
    /// The status text.
    pub text: String,
}

/// A beam heading and power in a status report, as the codes sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeamHeading {
    /// Heading code: `0`-`9` or `A`-`Z`, 10 degrees each.
    pub heading_code: char,
    /// Power code: `0`-`9`, the PHG power code.
    pub power_code: char,
}

/// A telemetry report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Telemetry {
    /// The sequence as sent: usually three digits, or `MIC`.
    pub sequence: String,
    /// Five analog values as sent (so `073` and `190.0` write back unchanged); `None` for a
    /// channel sent empty. [`Telemetry::values`] reads them as numbers.
    pub analog: Vec<Option<String>>,
    /// The eight digital bits as on air, B1 first.
    pub bits: Option<String>,
    /// Text after the bits.
    pub comment: String,
}

impl Telemetry {
    /// The analog values as numbers.
    pub fn values(&self) -> Vec<Option<f64>> {
        self.analog.iter().map(|v| v.as_deref().and_then(|t| t.parse().ok())).collect()
    }
}

/// A positionless weather report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeatherReport {
    /// When (MDHM).
    pub timestamp: Option<Timestamp>,
    /// The weather.
    pub weather: Weather,
    /// Text after the weather data (not allowed; see [`crate::Code::WeatherComment`]).
    pub comment: String,
}

/// Raw weather station output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawWeather {
    /// Which station format.
    pub format: RawWeatherFormat,
    /// The data after the identifier.
    pub data: String,
}

/// Raw weather formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawWeatherFormat {
    /// `#`: Peet Bros U-II, hash format.
    PeetBrosHash,
    /// `*`: Peet Bros U-II, star format.
    PeetBrosStar,
    /// `$ULTW`: Ultimeter 2000 packet mode.
    UltimeterPacket,
    /// `!!`: Ultimeter logging mode.
    UltimeterLogging,
}

/// A raw NMEA 0183 sentence (APRS12c ch. 5 and 7).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nmea {
    /// The sentence as sent, without the `$`, up to and including any `*hh` checksum.
    pub sentence: String,
    /// The sentence ends in a `*hh` checksum (which matched).
    pub has_checksum: bool,
    /// Latitude, if the sentence type carries one. The fields below are read from GGA, GLL, RMC,
    /// VTG and WPL sentences only, and each is left out when it is missing or does not parse; a
    /// position needs both coordinates.
    pub latitude: Option<f64>,
    /// Longitude.
    pub longitude: Option<f64>,
    /// Whether the fix is valid: RMC's or GLL's status (`A` or `V`), or GGA's quality digit.
    pub fix_valid: Option<bool>,
    /// Course over ground, degrees.
    pub course_degrees: Option<f64>,
    /// Speed over ground, knots.
    pub speed_knots: Option<f64>,
    /// Altitude, metres.
    pub altitude_m: Option<f64>,
    /// UTC time, `HH:MM:SS`, with any fraction of a second as sent (less trailing zeros).
    pub time: Option<String>,
    /// A waypoint name.
    pub waypoint: Option<String>,
    /// Text after the checksum, kept as sent (TinyTrack and FreeTrak send one). Only a sentence
    /// with a checksum can have one, since the checksum is what ends the sentence.
    pub comment: String,
}

/// A Maidenhead locator beacon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaidenheadBeacon {
    /// The locator.
    pub locator: String,
    /// The comment.
    pub comment: String,
}

/// A general query.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// The query type, e.g. `APRS` or `IGATE`.
    pub query_type: String,
    /// The area it is limited to.
    pub footprint: Option<Footprint>,
}

/// A query's target area. The latitude and longitude are kept as sent, a leading space included, so
/// that ` 34.0` and `-.1715` write back unchanged; [`Footprint::latitude_degrees`] and
/// [`Footprint::longitude_degrees`] read them as numbers, and [`Footprint::new`] writes them from
/// numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Footprint {
    /// Latitude in decimal degrees as sent: an optional `-`, or for a positive value an optional
    /// leading space ("Note the leading space in the latitude, as its value is positive", APRS12c
    /// ch. 15), then digits with an optional decimal point.
    pub latitude: String,
    /// Longitude in decimal degrees as sent, written as the latitude is.
    pub longitude: String,
    /// Radius, miles.
    pub radius_miles: u32,
}

impl Footprint {
    /// A footprint from degrees, north and east positive, written as APRS12c ch. 15 writes them: a
    /// positive value after a space, a negative one with its sign.
    pub fn new(latitude: f64, longitude: f64, radius_miles: u32) -> Footprint {
        let text = |v: f64| if v.is_sign_negative() { alloc::format!("{v}") } else { alloc::format!(" {v}") };
        Footprint { latitude: text(latitude), longitude: text(longitude), radius_miles }
    }

    /// The latitude in degrees, north positive; `None` when the text is not a number as APRS12c
    /// writes one, or is beyond 90 degrees.
    pub fn latitude_degrees(&self) -> Option<f64> {
        crate::other::footprint_degrees(&self.latitude, 90.0)
    }

    /// The longitude in degrees, east positive; `None` when the text is not a number as APRS12c
    /// writes one, or is beyond 180 degrees.
    pub fn longitude_degrees(&self) -> Option<f64> {
        crate::other::footprint_degrees(&self.longitude, 180.0)
    }
}

/// Station capabilities.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// `TOKEN` or `TOKEN=VALUE` items, in order.
    pub capabilities: Vec<(String, Option<String>)>,
}

/// User-defined data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserDefined {
    /// The user ID.
    pub user_id: char,
    /// The packet type.
    pub packet_type: char,
    /// The rest of the information field, as sent.
    pub data: Vec<u8>,
}

/// Invalid or test data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestData {
    /// The text after the `,`.
    pub data: String,
}

/// An Agrelo DFJr bearing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AgreloDf {
    /// Bearing, degrees.
    pub bearing_degrees: u16,
    /// Quality, 0-9.
    pub quality: u8,
}
