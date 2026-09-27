# pdn-aprs

An APRS (Automatic Packet Reporting System) encoder and decoder in Rust. It covers every APRS 1.2 data type in [APRS12c](https://github.com/wb2osz/aprsspec), from TNC2 / APRS-IS text lines or AX.25 UI frames. It is `no_std` with `alloc`, and has no `unsafe`.

- **Decoding never fails because of the information field.** Every deviation from the spec becomes a [`Diagnostic`] with a stable [`Code`], and the rest of the packet is still read where it can be.
- **Strict or lenient, one defect at a time.** [`ParseOptions`] says which of the 32 tolerable defects to accept: lower-case hemispheres, unpadded object names, text after weather data, and so on. [`ParseOptions::LENIENT`] (the default) accepts them all, each with a warning. [`ParseOptions::STRICT`] accepts none. Any single one can be switched off.
- **The encoder writes only what the spec allows.** It checks free text by decoding what it wrote: a comment that would read back as something else, such as an altitude, a data extension or a `!DAO!`, is escaped or refused rather than silently changed.

It is built and tested against the language-neutral conformance vectors in [packet-net/aprs-vectors](https://github.com/packet-net/aprs-vectors), the same cases the C# implementation, [Packet.Aprs](https://www.nuget.org/packages/Packet.Aprs), runs.

## Install

```sh
cargo add pdn-aprs
```

## Decoding

```rust
use pdn_aprs::{Data, Packet, ParseOptions};

let packet = Packet::decode_tnc2(b"N0CALL-9>APZ001,WIDE1-1,qAR,M0LTE-10:=5130.00N/00007.00W>088/036Mobile", ParseOptions::default())?;

assert_eq!(packet.source.as_str(), "N0CALL-9");
assert_eq!(packet.q_construct().unwrap().construct, "qAR");
let Data::Position(report) = &packet.data else { panic!("a position") };
assert!(report.messaging);
assert!((report.fields.position.latitude - 51.5).abs() < 1e-9);
assert_eq!(report.fields.course_degrees, Some(88));
assert_eq!(report.fields.speed_knots, Some(36.0));
assert_eq!(report.fields.comment, "Mobile");
assert!(packet.diagnostics.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

Only an unusable header is an error, a [`HeaderError`]. Anything wrong in the information field comes back as diagnostics, and [`Data::Unrecognized`] when nothing could be read.

An AX.25 frame in KISS form (no flags, no FCS) decodes the same way:

```rust
use pdn_aprs::{Packet, ParseOptions};

let frame = [
    0x82, 0xA0, 0xB4, 0x60, 0x60, 0x62, 0xE0, 0x9A, 0x60, 0x98, 0xA8, 0x8A, 0x40, 0x72, 0x9A, 0x60, 0x98, 0xA8, 0x8A, 0x40,
    0xE2, 0xAE, 0x92, 0x88, 0x8A, 0x64, 0x40, 0x63, 0x03, 0xF0, b'>', b'h', b'e', b'l', b'l', b'o',
];
let packet = Packet::decode_ax25(&frame, ParseOptions::default())?;
assert_eq!(packet.to_tnc2(), b"M0LTE-9>APZ001,M0LTE-1*,WIDE2-1:>hello");
assert_eq!(packet.to_ax25()?, frame);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Strict and lenient

```rust
use pdn_aprs::{Code, Data, Packet, ParseOptions, Severity};

let line = b"N1EOE>APN391:!4216.95n/07243.20w#phg6230/ Easthampton MA";

// Lenient: the lower-case hemispheres are read, and each is reported.
let lenient = Packet::decode_tnc2(line, ParseOptions::LENIENT)?;
assert!(matches!(lenient.data, Data::Position(_)));
assert!(lenient.diagnostics.iter().all(|d| d.severity == Severity::Warning && d.code == Code::LowercaseHemisphere));

// Strict: the same defect is an error, and the data is not decoded.
let strict = Packet::decode_tnc2(line, ParseOptions::STRICT)?;
assert!(matches!(strict.data, Data::Unrecognized(_)));

// Or tolerate everything except this one defect.
let options = ParseOptions::LENIENT.without(Code::LowercaseHemisphere);
assert!(Packet::decode_tnc2(line, options)?.has_errors());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Building packets

Start from your station and say what the packet is. `build()` gives the encoded [`Packet`], and `to_data()` the [`Data`] alone.

```rust
use pdn_aprs::{Station, Symbol, Timestamp};

let me = Station::new("M0LTE-9")?.via(&["WIDE1-1", "WIDE2-1"])?;

let beacon = me.position(51.4543, -0.9781).symbol(Symbol::CAR).course(88).speed(36.0).altitude(120.0).comment("Mobile").build()?;
assert_eq!(beacon.to_tnc2(), b"M0LTE-9>APZ001,WIDE1-1,WIDE2-1:!5127.26N/00058.69W>088/036/A=000120Mobile");

let message = me.message("G3NRW", "Hi Ian").id("01").build()?;
assert_eq!(message.to_tnc2(), b"M0LTE-9>APZ001,WIDE1-1,WIDE2-1::G3NRW    :Hi Ian{01");

let repeater = me
    .object("MYRPTR")
    .at(51.45, -0.98)
    .symbol(Symbol::REPEATER)
    .timestamp(Timestamp::dhm(25, 18, 30))
    .frequency(145.725)
    .tone(118.8)
    .offset_khz(-600)
    .build()?;
assert_eq!(repeater.to_tnc2(), b"M0LTE-9>APZ001,WIDE1-1,WIDE2-1:;MYRPTR   *251830z5127.00N/00058.80Wr145.725MHz T118 -060");

let weather = me.weather().at(51.45, -0.98).wind(220, 4.0).gust(5.0).temperature(77.0).build()?;
assert_eq!(weather.to_tnc2(), b"M0LTE-9>APZ001,WIDE1-1,WIDE2-1:!5127.00N/00058.80W_220/004g005t077");
# Ok::<(), Box<dyn std::error::Error>>(())
```

[`Station`] also starts items, Mic-E reports, acks and rejects, bulletins, status reports, telemetry and its `PARM.`/`UNIT.`/`EQNS.`/`BITS.` metadata. A station is a value, so keep one and build every packet from it. The destination is [`DEFAULT_DESTINATION`], `APZ001` from the experimental range, until you set your own with [`Station::to`]. Values are in the units APRS sends (knots, feet, Fahrenheit, mph), with `speed_kmh`, `altitude_metres`, `temperature_celsius` and the `_mm` rain methods to convert. The crate has no clock, so an object or a positionless weather report needs its timestamp given. Every defined symbol has a name ([`Symbol::CAR`], [`Symbol::GATEWAY`]`.with_overlay('I')`), the same names the other implementations use.

## Encoding

To build the data by hand, fill in the types, then make a packet. The fields are public, and every type has a sensible `Default`.

```rust
use pdn_aprs::{Address, Data, Packet, PathEntry, Position, PositionReport, Positioned, Symbol};

let report = PositionReport {
    messaging: true,
    fields: Positioned {
        position: Position { latitude: 51.5, longitude: -0.116667, ambiguity: 0 },
        symbol: Symbol::CAR,
        course_degrees: Some(88),
        speed_knots: Some(36.0),
        altitude_feet: Some(120.0),
        comment: "Mobile".into(),
        ..Positioned::default()
    },
    ..PositionReport::default()
};
let packet = Packet::create(
    Address::new("N0CALL-9")?,
    Address::new("APZ001")?,
    vec![PathEntry::new(Address::new("WIDE1-1")?)],
    Data::Position(report),
)?;
assert_eq!(packet.to_tnc2(), b"N0CALL-9>APZ001,WIDE1-1:=5130.00N/00007.00W>088/036/A=000120Mobile");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Messages, acks, bulletins and the rest encode with [`Data::encode`]:

```rust
use pdn_aprs::{Data, Message};

let message = Data::Message(Message {
    addressee: "G3NRW".into(),
    text: "Hi Ian".into(),
    message_id: Some("01".into()),
    reply_ack: Some(String::new()), // says this station supports reply-acks
});
assert_eq!(message.encode()?, b":G3NRW    :Hi Ian{01}");

// What the spec forbids is refused, with the reason.
let too_long = Data::Message(Message { addressee: "N0CALL".into(), text: "x".repeat(68), ..Message::default() });
assert!(too_long.encode().unwrap_err().message.contains("67 characters"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Mic-E and device identification

Mic-E carries half its position in the destination address, so it has its own constructor, [`Packet::create_mic_e`], which works out that address. [`Packet::device`] names the sending device or program from the [APRS device identification database](https://github.com/aprsorg/aprs-deviceid): from the Mic-E type code and suffix, or from the destination address for everything else.

```rust
use pdn_aprs::{Data, Packet, ParseOptions};

let packet = Packet::decode_tnc2(b"N1JCM-9>TRQP7T,WA1PLE-4*:`c'wl|+>/`\"4-}_%", ParseOptions::default())?;
let Data::MicE(report) = &packet.data else { panic!("Mic-E") };
assert_eq!(report.fields.course_degrees, Some(215));
assert_eq!(report.fields.speed_knots, Some(9.0));
assert_eq!(report.device_suffix, "_%");

let device = packet.device().unwrap();
assert_eq!((device.vendor, device.model), (Some("Yaesu"), Some("FTM-400DR")));

// And back: the same information field and destination.
let again = Packet::create_mic_e(packet.source.clone(), report.clone(), packet.path.clone())?;
assert_eq!(again.destination, packet.destination);
assert_eq!(again.information, packet.information);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What it covers

| Data type | Decode | Encode |
|---|---|---|
| Position reports, uncompressed and compressed, with ambiguity, `!DAO!`, PHG / RNG / DFS / course and speed / DF bearing / area / storm extensions, altitude, base-91 telemetry, voice frequency, signpost | yes | yes |
| Mic-E, with type codes, altitude, grid locator and device suffixes | yes | yes |
| Objects and items | yes | yes |
| Weather: with a position, positionless, compressed (wind in the cs bytes), raw station formats | yes | yes |
| Messages, acks and rejects (with reply-acks), bulletins, NWS bulletins | yes | yes |
| Telemetry reports and metadata (`PARM.`, `UNIT.`, `EQNS.`, `BITS.`) | yes | yes |
| Status reports, with grid locator and beam heading | yes | yes |
| Queries (general and directed), station capabilities | yes | yes |
| Third-party traffic, user-defined data, raw NMEA, Maidenhead beacons, test data, Agrelo DF | yes | yes |

Values are in the units APRS sends (knots, feet, mph, degrees Fahrenheit), with the unit in the field name.

## Conformance

`cargo test` runs the whole [packet-net/aprs-vectors](https://github.com/packet-net/aprs-vectors) suite (a git submodule at `vectors/`): every example in APRS12c and *Understanding APRS Packets*, every tolerable defect, the encoder's rules, 1,395 real APRS-IS packets, 40 more that settled a disagreement between this crate and the C# implementation, and 157 that settle what differential fuzzing of all five implementations found. Each case is checked lenient, strict, with only its own tolerance switched off, re-encoded, and read back: 7,819 checks, all passing. A deliberate difference from a recorded expectation would be listed, with its reason, in [`tests/known-differences.txt`](https://github.com/packet-net/aprs-rs/blob/main/tests/known-differences.txt); there are none.

This crate was written from the spec, the vectors and their [interpretations](https://github.com/packet-net/aprs-vectors/blob/main/interpretations.md), not by porting the C# implementation. Doing so found three places where the vectors had recorded a C# quirk rather than a rule; the C# was fixed and the cases refreshed.

The two implementations have also been run over the same 6.9 million APRS-IS packets and compared packet by packet, with [`examples/diff_dump.rs`](https://github.com/packet-net/aprs-rs/blob/main/examples/diff_dump.rs) and the vectors' `tools/compare.py`. They now decode every one of them the same way, leniently and strictly. Where they differ is only in how some data is re-encoded: both write something that decodes back to the same data, but only one writes the original bytes.

## `no_std`

The crate is `#![no_std]` and needs only `alloc`; its one dependency, [`libm`](https://crates.io/crates/libm), is `no_std` too. CI builds it for `thumbv6m-none-eabi` (the RP2040).

## Licence

AGPL-3.0-or-later ([`LICENSE`](https://github.com/packet-net/aprs-rs/blob/main/LICENSE)). It includes data from the APRS device identification database, CC BY-SA 2.0; see [`THIRD-PARTY-NOTICES.md`](https://github.com/packet-net/aprs-rs/blob/main/THIRD-PARTY-NOTICES.md).
