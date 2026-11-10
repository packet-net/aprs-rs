//! The 1.0 API: what each decision about names, numbers and refusals promises.

use pdn_aprs::{
    Address, Code, CommentTelemetry, Data, Device, Footprint, MicEReport, Packet, ParseOptions, Phg, Position, PositionReport, Positioned,
    Query, Severity, Station, Symbol, Telemetry, TelemetryCoefficients, Timestamp, Tone, Weather, WeatherReport,
};

fn me() -> Station {
    Station::new("M0LTE-11").unwrap()
}

fn info(data: Data) -> String {
    String::from_utf8(data.encode().unwrap()).unwrap()
}

#[test]
fn metadata_can_describe_another_station() {
    let line = |packet: Packet| String::from_utf8(packet.to_tnc2()).unwrap();
    assert_eq!(line(me().telemetry_names(&["Battery"]).addressee("G3NRW-5").build().unwrap()), "M0LTE-11>APZ001::G3NRW-5  :PARM.Battery");
    assert_eq!(line(me().telemetry_units(&["V"]).addressee("G3NRW-5").build().unwrap()), "M0LTE-11>APZ001::G3NRW-5  :UNIT.V");
    assert_eq!(
        line(me().telemetry_coefficients(&[0.0, 0.5, 0.0]).addressee("G3NRW-5").build().unwrap()),
        "M0LTE-11>APZ001::G3NRW-5  :EQNS.0,0.5,0"
    );
    assert_eq!(
        line(me().telemetry_bits("10101010", "Garden").addressee("G3NRW-5").build().unwrap()),
        "M0LTE-11>APZ001::G3NRW-5  :BITS.10101010,Garden"
    );
    // By default it is the station's own.
    assert_eq!(line(me().telemetry_names(&["Battery"]).build().unwrap()), "M0LTE-11>APZ001::M0LTE-11 :PARM.Battery");
    // The digital bits are text, and refused when they are not eight 0 or 1 characters.
    assert!(me().telemetry_bits("1010", "Garden").build().is_err());
    assert!(me().telemetry(1).bits("2").build().is_err());
}

#[test]
fn a_tone_has_a_kind_and_a_value_as_sent() {
    let base = || me().position(51.45, -0.98).symbol(Symbol::REPEATER).frequency(145.725);
    let tail = |tone: Tone, value: u16| {
        let line = String::from_utf8(base().tone(tone, value).build().unwrap().to_tnc2()).unwrap();
        line.split_once("MHz").unwrap().1.to_string()
    };
    assert_eq!(tail(Tone::Tone, 118), " T118");
    assert_eq!(tail(Tone::Ctcss, 100), " C100");
    assert_eq!(tail(Tone::Dcs, 23), " D023");
    assert_eq!(tail(Tone::Off, 0), " Toff");
    // A frequency is rounded to its step, not refused for extra decimals.
    let line =
        String::from_utf8(me().position(51.45, -0.98).symbol(Symbol::REPEATER).frequency(145.7251).build().unwrap().to_tnc2()).unwrap();
    assert!(line.ends_with("145.725MHz"), "{line}");
}

#[test]
fn mic_e_is_made_by_create_and_never_loses_its_destination() {
    let mic_e = me().mic_e(42.179, -71.1985).symbol(Symbol::CAR).course_degrees(215.0).speed_knots(9.0).to_data().unwrap();
    // The destination passed in is not used; the one the report computes is.
    let packet = Packet::create(Address::new("M0LTE").unwrap(), Address::new("APZ001").unwrap(), Vec::new(), mic_e.clone()).unwrap();
    assert_ne!(packet.destination.as_str(), "APZ001");
    // Encoding the data alone would drop half the position, so it is refused.
    let refused = mic_e.encode().unwrap_err();
    assert!(refused.message().contains("Packet::create"), "{refused}");
}

#[test]
fn a_mic_e_altitude_in_the_comment_re_encodes_where_it_was() {
    let decode = |line: &[u8]| Packet::decode_tnc2(line, ParseOptions::LENIENT).unwrap();
    let written = |p: &Packet| {
        let again = Packet::create(p.source.clone(), p.destination.clone(), p.path.clone(), p.data.clone()).unwrap();
        (again.information, again.destination)
    };
    // The Mic-E bytes: 9176 metres. The /A= : 100 feet, which no whole metre says.
    let built = |altitude: &dyn Fn(pdn_aprs::MicEBuilder) -> pdn_aprs::MicEBuilder| {
        altitude(me().mic_e(42.179, -71.1985).symbol(Symbol::CAR)).build().unwrap()
    };
    for (packet, in_comment) in [(built(&|b| b.altitude_m(9176.0)), false), (built(&|b| b.altitude_feet(100.0)), true)] {
        let heard = decode(&packet.to_tnc2());
        let Data::MicE(report) = &heard.data else { panic!("a Mic-E report") };
        assert_eq!(report.altitude_in_comment, in_comment);
        assert_eq!(written(&heard), (packet.information.clone(), packet.destination.clone()));
    }
    // Feet that are exactly whole metres still go where the data says.
    let metres = 30.0;
    let feet = metres / 0.3048;
    let in_bytes = built(&|b| b.altitude_m(metres));
    let in_text = built(&|b| b.altitude_feet(feet));
    assert!(!in_bytes.information.ends_with(b"}") || !String::from_utf8_lossy(&in_bytes.information).contains("/A="));
    assert!(String::from_utf8_lossy(&in_text.information).contains("/A="), "{}", String::from_utf8_lossy(&in_text.information));
}

#[test]
fn numbers_the_encoder_rounds_are_fractional_and_rounded_once() {
    let position = |fields: Positioned| {
        let fields = Positioned { position: Position { latitude: 51.5, longitude: -0.1, ambiguity: 0 }, symbol: Symbol::CAR, ..fields };
        info(Data::Position(PositionReport { fields, ..PositionReport::default() }))
    };
    assert!(position(Positioned { course_degrees: Some(88.6), speed_knots: Some(1.0), ..Positioned::default() }).ends_with(">089/001"));
    assert!(position(Positioned { course_degrees: Some(88.4), speed_knots: Some(1.0), ..Positioned::default() }).ends_with(">088/001"));
    // The weather's own four.
    let weather = |w: Weather| {
        info(Data::Weather(WeatherReport { timestamp: Some(Timestamp::mdhm(1, 2, 3, 4)), weather: w, comment: String::new() }))
    };
    let w = Weather {
        wind_direction_degrees: Some(89.5),
        wind_speed_mph: Some(1.0),
        wind_gust_mph: Some(2.0),
        temperature_f: Some(70.0),
        humidity_percent: Some(99.7),
        luminosity_w_m2: Some(1412.4),
        ..Weather::default()
    };
    assert_eq!(weather(w.clone()), "_01020304c090s001g002t070h00l412");
    assert!(
        Data::Weather(WeatherReport { weather: Weather { humidity_percent: Some(0.4), ..w }, ..WeatherReport::default() })
            .encode()
            .is_err()
    );
    // A footprint's radius.
    let query =
        |radius: f64| info(Data::Query(Query { query_type: "APRS".into(), footprint: Some(Footprint::new(34.02, -117.15, radius)) }));
    assert_eq!(query(200.4), "?APRS? 34.02,-117.15,0200");
    assert!(Data::Query(Query { query_type: "APRS".into(), footprint: Some(Footprint::new(34.0, 1.0, 10000.0)) }).encode().is_err());
}

#[test]
fn numbers_kept_as_sent_are_written_back_while_they_still_say_the_number() {
    let heard = Packet::decode_tnc2(b"M0LTE>APZ001:T#005,073,190.0,.5,,-0.50,00000000", ParseOptions::LENIENT).unwrap();
    let Data::Telemetry(t) = &heard.data else { panic!("telemetry") };
    assert_eq!(t.analog, [Some(73.0), Some(190.0), Some(0.5), None, Some(-0.5)]);
    assert_eq!(t.analog_text, ["073", "190.0", ".5", "", "-0.50"]);
    assert_eq!(info(heard.data.clone()), "T#005,073,190.0,.5,,-0.50,00000000");
    // Without text the number is written as APRS12c does: three digits for 0-999, else the shortest decimal.
    let numbers = Telemetry {
        sequence: "005".into(),
        analog: vec![Some(5.0), Some(190.0), Some(0.5), None, Some(-94.0)],
        bits: Some("00000000".into()),
        ..Telemetry::default()
    };
    assert_eq!(info(Data::Telemetry(numbers.clone())), "T#005,005,190,0.5,,-94,00000000");
    // Text that no longer says the number is ignored.
    let stale = Telemetry { analog_text: vec!["073".into(), "190.0".into(), "9".into(), String::new(), "-94".into()], ..numbers };
    assert_eq!(info(Data::Telemetry(stale)), "T#005,005,190.0,0.5,,-94,00000000");

    let coefficients = TelemetryCoefficients {
        addressee: "M0LTE".into(),
        coefficients: vec![0.0, 0.53, 1e-7],
        coefficients_text: vec!["0".into(), ".53".into(), "1e-7".into()],
        message_id: None,
    };
    assert_eq!(info(Data::TelemetryCoefficients(coefficients.clone())), ":M0LTE    :EQNS.0,.53,1e-7");
    assert_eq!(
        info(Data::TelemetryCoefficients(TelemetryCoefficients { coefficients_text: Vec::new(), ..coefficients })),
        ":M0LTE    :EQNS.0,0.53,0.0000001"
    );

    let heard = Packet::decode_tnc2(b"M0LTE>APZ001:?APRS? 34.0,-.1715,0200", ParseOptions::LENIENT).unwrap();
    let Data::Query(query) = &heard.data else { panic!("a query") };
    let footprint = query.footprint.as_ref().unwrap();
    assert_eq!((footprint.latitude, footprint.longitude, footprint.radius_miles), (34.0, -0.1715, 200.0));
    assert_eq!(info(heard.data.clone()), "?APRS? 34.0,-.1715,0200");
}

#[test]
fn a_refusal_has_a_reason_to_read() {
    let refused =
        Data::Message(pdn_aprs::Message { addressee: "N0CALL".into(), text: "x".repeat(68), ..Default::default() }).encode().unwrap_err();
    assert!(refused.message().contains("67 characters"));
    assert_eq!(refused.to_string(), refused.message());
    let invalid = Address::new("not an address!").unwrap_err();
    assert_eq!(invalid.text(), "not an address!");
}

#[test]
fn small_things_a_program_reads() {
    assert_eq!(Timestamp::dhm(9, 23, 45).to_string(), "092345z");
    assert_eq!(Timestamp::mdhm(10, 9, 5, 56).to_string(), "10090556");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert!(Code::ALL.len() >= 64);
    assert_eq!(Code::from_id("invalid-header"), Some(Code::InvalidHeader));
    assert!(Device::DATABASE_VERSION.len() > 8);
    assert!(Device::from_tocall("APRS").is_some());
    assert_eq!(Symbol::CAR.description(), Some("Car"));
    assert_eq!(Symbol::GATEWAY.with_overlay('I').unwrap().description(), Symbol::GATEWAY.description());
    assert_eq!(Symbol::new('/', ' ').description(), None);
    let phg = Phg { power: 5, height: 3, gain: 6, directivity: 2, beacons_per_hour: None };
    assert_eq!((phg.power_watts(), phg.height_feet(), phg.gain_db(), phg.directivity_degrees()), (25, 80, 6, Some(90)));
    assert_eq!(Phg { directivity: 0, ..phg }.directivity_degrees(), None);
}

/// Mic-E data a program fills by hand still needs only `Default`.
#[test]
fn hand_built_data_uses_default() {
    let report = MicEReport {
        fields: Positioned {
            position: Position { latitude: 42.179, longitude: -71.1985, ambiguity: 0 },
            symbol: Symbol::CAR,
            ..Positioned::default()
        },
        type_code: Some('`'),
        ..MicEReport::default()
    };
    let packet = Packet::create(Address::new("M0LTE").unwrap(), Address::new("APZ001").unwrap(), Vec::new(), Data::MicE(report)).unwrap();
    assert_eq!(packet.information[0], b'`');
}

#[test]
fn comment_telemetry_is_one_value() {
    let line = String::from_utf8(
        me().position(51.4543, -0.9781)
            .symbol(Symbol::HOUSE)
            .telemetry(CommentTelemetry { sequence: 1, analog: vec![100, 200, 300, 400, 500], digital: Some(5) })
            .build()
            .unwrap()
            .to_tnc2(),
    )
    .unwrap();
    assert!(line.contains('|'), "{line}");
}
