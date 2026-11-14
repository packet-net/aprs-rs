# Changelog

## Unreleased

- `Packet::create` (and so every builder's `build`) refuses with `invalid-address` a header that a TNC2 line cannot carry: an address that is not 1-9 letters, digits or `-` (a third-party source such as `PY2SP_R-R` used as an outer source, or an empty address). Before, it built the packet and `to_tnc2()` wrote a line that `decode_tnc2` rejected. This matches the C library, which refuses the same header with the same code. A Mic-E destination is worked out from the report, so it is not checked.
- README: the conformance statement names the two encode cases this crate skips, and no longer quotes case counts or "five implementations".

Round 4 of the conformance vectors (pinned at fe7d451). The two encode cases listed in `tests/known-differences.txt` are still the only exceptions.

- An encoder or builder refuses an information field over 256 bytes as `too-long`, whatever the output (TNC2, AX.25, KISS). Bytes are counted (UTF-8), number text included; a third-party packet counts its outermost field. The size is the last check, on the bytes actually written, after every other check (the read-back included): data that any other rule refuses gets that rule's code, and only otherwise sound data over 256 bytes is `too-long` for its size. A field over its own limit (message text over 67 characters, more than five analog values) keeps `too-long` at its place in the order. A decoder sets no limit: a decoded packet over 256 bytes re-encodes `refused`, `too-long`.
- New diagnostic `Code::ControlCharacterInText` (`control-character-in-text`, tolerable): text with a byte below 0x20 or 0x7F (a TAB, or a CR or LF inside the text, included) decodes with one warning per text, and the text is kept as sent. A strict decoder rejects it. It is judged last for its text, after the text's structure and `non-utf8-text`, and not for user-defined, raw weather, test or undecoded data, object and item names, capability tokens, addressees, headers or NMEA sentences. A capabilities report checks `free-text-capabilities` first.
- Encoders and builders refuse text with a control character (not only CR and LF) as `invalid-character`, and a Mic-E status text starting with 0x1D is now refused instead of being written after a `/`.
- A fixed-width weather field whose digits run on by two or more, followed by the end of the information field or by a defined field the run goes on to read, is `out-of-range-value`: the value is dropped and the fields carry on after the run (`t2147483647r000` has a rain of 0 and no temperature). Anything else after the run keeps the 1.1.0 reading.
- Builder defaults are written down and changed where they differed: a position, object, item or Mic-E report built with no symbol is refused as `invalid-symbol` (it was `missing-field`); a builder never reads the clock (an object or positionless weather report with no timestamp was already `missing-field` here). The telemetry builder takes its sequence as an argument, so a missing sequence cannot be built; the dump tool reports it `refused`, `missing-field`. A builder given no symbol, no timestamp or no sequence names a fault ranked earlier in the refusal order first (an object name that is too long is `invalid-name` before `invalid-symbol` and `missing-field`), because the report is tried with a placeholder for what is left out. A builder given no analog values writes five empty channels.
- A Mic-E altitude goes in the Mic-E altitude bytes, rounded once to the nearest metre from the value given (1000 feet is 305 m), whether the builder was given feet or metres; outside -10000 to 743570 m is `value-out-of-range`, and no `/A=` is written. For neutral data, `MicEReport::altitude_in_comment` false means the value decides: a `/A=` only when the value is a whole number of feet within six digits that the Mic-E bytes do not hold exactly (not a whole number of metres, or outside their range), otherwise the Mic-E bytes, rounded to the metre (100.5 ft is 31 m, -50000 ft is `/A=-50000`). An altitude read from a `/A=` is still written back as one.
- Neutral telemetry data with no sequence is `missing-field` (it was `malformed`); a sequence that is there but is not letters and digits stays `malformed`. A status symbol with no locator is `missing-field`, as a locator with no symbol is.
- A strict decoder rejects a weather value that runs on by two or more digits with `out-of-range-value` even when a later field has a non-standard width: whether a defined field follows the run no longer depends on how strictly that field would be read.
- The dump tool passes only the keys a recipe gives and no longer fills in a missing telemetry sequence with 0. The builder recipe code is shared with the vectors runner (`tests/vectors/build.rs`), which now runs the `build` cases (`cases/builder.json`) through the real builder API and compares the TNC2 line or the refusal code. The test dependency `serde_json` has the `float_roundtrip` feature, so a number such as `-5.40694e+75` in a recipe or data line reads as the exact double (without it the last digit was off and the number text was written with a stray digit).
- The licence is MIT.

### Breaking changes for callers

- An encoder or builder refuses an information field over 256 bytes (`too-long`); text with a control character is refused (`invalid-character`); a decoder gives `warning:control-character-in-text`, so a strict decoder now rejects such packets; a run-on weather value before a defined field is dropped with `out-of-range-value`.
- A position, object, item or Mic-E report built with no symbol is refused as `invalid-symbol` instead of `missing-field`.
- A Mic-E report built with an altitude in feet puts it in the Mic-E altitude bytes, rounded to the metre, instead of a `/A=` in the status text; an altitude outside the Mic-E range is refused instead of moved to a `/A=`.
- Neutral Mic-E data of -50000 ft is written as `/A=-50000` instead of refused; neutral Mic-E altitudes that are neither whole feet nor whole metres (100.5 ft) go in the Mic-E bytes instead of a rounded `/A=`.
- This crate never read the clock and has no "now" helper: the builders already take an explicit time.

## 1.1.0 (2026-11-14)

Round 3 of the conformance vectors (pinned at 10d4e25). Two encode cases (no symbol, an undecoded inner packet) are records this crate's data cannot hold; they are listed in `tests/known-differences.txt`.

- A decoder reads at most four levels of third-party header; a fifth `}` makes the packet holding it `unrecognized` with `error:invalid-third-party`. Decoding a line of thousands of nested `}` no longer overflows the stack.
- An AX.25 address holding anything but upper-case letters and digits (a lower-case letter is still tolerated) is `invalid-address`, and a call of spaces only is an empty source (`invalid-address`), destination (`empty-destination`) or digipeater (`empty-path-entry`).
- A `!DAO!` on an ambiguous position reads with `dao-with-ambiguity` whatever its precision, `!W  !` included.
- An ambiguous position is cut to its box, not rounded and then cut again, so a latitude or longitude just under the edge is written in the box below it, in an uncompressed report and in Mic-E.
- A wind direction in a compressed weather report is judged in whole degrees before it is rounded to the 4-degree step: 360.5 and -0.5 are `value-out-of-range`.
- Refusal order follows the vectors: more than five analog values in a telemetry report or comment telemetry are `too-long`; a status grid locator that is not upper case is `malformed`; a field that is wanted twice (a PHG, RNG or DFS that loses the one data extension, or a course, speed or wind beside a range or a GGA altitude in the cs bytes) keeps its own range checks, so its refusal can outrank `extension-conflict`; a reply-ack that is wrong on its own is `invalid-message-id` even with no message ID.
- `StatusBuilder::locator` writes a lower-case locator in upper case; the encoder, given a lower-case locator in data, refuses it.
- A lower-case grid locator in Mic-E or Maidenhead beacon data is `malformed`, as in a status.
- Base-91 telemetry on a weather report has no place, so it is `field-not-allowed` and its own checks (too many values, a value out of range) do not run.
- A course or speed beside a compressed weather report's wind is judged in the cs-byte checks before `extension-conflict`, so a course of 0 is `value-has-no-form`.
- The telemetry builder names a fault ranked before too-long (a line break in the comment) ahead of too many analog values; builders in general refuse in the encoder's order: a value out of range (a course of 360.5, a latitude past 90, a Mic-E speed of 999.5) is `value-out-of-range` before an ambiguous position at 90 or 180 degrees is `value-has-no-form`.
- The encoder refuses third-party data nested more than four deep (`invalid-third-party-header`), counting the levels in a loop, so a chain a caller built thousands deep is refused without recursion. Dropping, cloning, comparing or printing such a chain still recurses (plain boxed data); the limit is documented on `Data::ThirdParty`.
- A compressed weather report whose GGA altitude is 1 foot or less is written with the nearest cs value (1 foot, `!!`) alone, as it has no comment to carry a `/A=`; it used to be refused as `text-reads-as-field`.
- A value that is not a number (NaN, an infinity) is `not-a-number` in its place in the order everywhere: a speed or other whole-number field, the coordinates of a compressed position, and the numeric fields of an NMEA sentence; before, some were named `value-out-of-range` or reached a later fault first.
- The dump tool reads the `NaN`, `Infinity` and `-Infinity` tokens the generator's `--nonfinite` writes, and a recipe giving one quantity in two units is built once with each unit and refused with the first code in the order among both results and `value-given-twice`; it is never built from one unit and never `unsupported`.
- The vectors runner compares the refusal code a case names, not only that there was a refusal.

### Behaviour changes

For callers of 1.x, in a minor release:

- A lower-case locator in `Status`, `MicE` or `MaidenheadBeacon` data is now refused as `malformed` (a status used to write it in upper case). Only `StatusBuilder::locator` still upper-cases it.
- An AX.25 address with punctuation or a space inside is now a hard `invalid-address`, even with `ParseOptions::LENIENT`.
- More than five telemetry analog values are now `too-long` (they were `malformed`).
- A path entry of spaces only is dropped on decode (with `empty-path-entry`) rather than refused.
- Third-party packets nest at most four deep on decode, and the encoder refuses deeper data.

## 1.0.1 (2026-11-12)

Refusals carry a reason code.

- New `RefusalCode` (non-exhaustive, with `id()`, `from_id()` and `ALL`) and `EncodeError::code()`. Every refusal from `Data::encode`, `Packet::create`, `Packet::to_ax25` and the builders says which of the 26 shared reasons it is, with the same ids as the `refusals` list in the conformance vectors. The message text and every existing signature are unchanged.
- `diff_dump` writes the code as `refusal` on refused encode and build records, `reencode_refusal` on decode records whose re-encode is refused, and `ax25_refusal` in the API view.

## 1.0.0 (2026-11-10)

This is the first stable release. The public API is now frozen under semantic versioning. From 1.0 the major and minor version are shared with aprs-vectors: a ruling round is a minor version for all five implementations, this library's own fixes are patch versions, and an API break is a major version.

The 1.0 API cleanup: one naming rule, fractional numbers where the encoder rounds, one way to say each thing. Nothing about what the encoder writes or the decoder reads changes: the vectors suite, and a differential run over 100,000 generated encode cases, 100,000 builder recipes and 1,831 captured packets, give the same bytes and results as 0.4.0.

### Breaking changes

Migrating from 0.4.0:

**Numbers.**
- `Positioned::course_degrees`, `Weather::wind_direction_degrees`, `Weather::humidity_percent`, `Weather::luminosity_w_m2` and `Footprint::radius_miles` are `f64`. They are whole when decoded and rounded once, by the encoder, when written (to 4 degrees for a compressed course or wind direction). `Some(88)` becomes `Some(88.0)`.
- `Telemetry::analog` is `Vec<Option<f64>>`, with the text as sent in the new `analog_text` (one string per value, empty for an empty channel). `Telemetry::values()` is gone: read `analog`.
- `TelemetryCoefficients::coefficients` is `Vec<f64>`, with the text as sent in the new `coefficients_text`.
- `Footprint::latitude` and `longitude` are `f64`, with the text as sent in the new `latitude_text` and `longitude_text`. `Footprint::latitude_degrees()` and `longitude_degrees()` are gone. `Footprint::new` takes the radius as `f64`. `Footprint` and `TelemetryCoefficients` are no longer `Eq`.
- The text fields are optional when encoding, and `==` ignores them. The encoder writes a text that is a number of the form the spec allows and still says the value, and otherwise the number: three digits for a whole telemetry value from 0 to 999, else the shortest decimal that reads back the same, with no exponent.

**Names.** A builder setter has the name of the field it fills, and a converting setter changes only the unit.
- `PositionBuilder` (and the object, item and Mic-E builders): `course` is `course_degrees`, `speed` is `speed_knots`, `altitude` is `altitude_feet`, `altitude_metres` is `altitude_m`, `range` is `range_miles`, `kill()` is `killed()`. `telemetry(sequence, &[u16])` and `telemetry_with_bits` are one `telemetry(CommentTelemetry)`.
- `WeatherBuilder`: `wind(direction, speed)` is removed, use `wind_direction_degrees` and the new `wind_speed_mph`. `gust` is `wind_gust_mph`, `temperature` is `temperature_f`, `temperature_celsius` is `temperature_c`, `humidity` is `humidity_percent`, `pressure` is `pressure_mbar`, `rain_last_hour` is `rain_1h_in`, `rain_last_24_hours` is `rain_24h_in`, `rain_since_midnight` is `rain_midnight_in`, and the `_mm` ones are `rain_1h_mm`, `rain_24h_mm`, `rain_midnight_mm`. `luminosity` is `luminosity_w_m2`, `snowfall` is `snow_24h_in`.
- `MessageBuilder::id` is `message_id`. `MicEBuilder::message` is `mic_e_message`, and so is the field `MicEReport::message`.
- `TelemetryBuilder`: `analog(&[f64])` is removed and `channels(&[Option<f64>])` is now `analog(&[Option<f64>])`. `digital(u8)` is `bits(&str)`, eight `0` or `1` characters, B1 first.
- `StatusBuilder::locator(locator, symbol)` is two setters, `locator(locator)` and `symbol(symbol)`; one without the other is refused when built.
- `Station::telemetry_bits(bits: u8, project)` takes the bits as `&str`.
- `GpsFix` and `NmeaSource` are `CompressionFix` and `CompressionSource`.
- `TelemetryLabels` is two types, `TelemetryNames` (field `names`) and `TelemetryUnits` (field `units`), as `Data::TelemetryNames` and `Data::TelemetryUnits` carry.
- `pdn_aprs::tocall(callsign)` is `Device::from_tocall(callsign)`, and `pdn_aprs::database_version()` is `Device::DATABASE_VERSION`.

**Voice frequency.** `tone(hz: f64)` is `tone(kind: Tone, value: u16)`: the kind (`Tone::Tone`, `Tone::Ctcss`, `Tone::Dcs`) and the value as sent, so CTCSS and DCS no longer need the whole `voice_frequency(VoiceFrequency { .. })`. 118.8 Hz is `tone(Tone::Tone, 118)`, as it was cut to before.

**Mic-E.** `Packet::create_mic_e` is gone: `Packet::create` works out the destination of a `Data::MicE` (the destination you pass is not used). `Data::encode` on Mic-E data now refuses it, since it used to return the information field and silently drop the computed destination, half the position. `MicEReport` has a new field, `altitude_in_comment`: the decoder sets it when the altitude came from a `/A=`, and the encoder honours it, so encoding a copy of a report writes the bytes it was built from (the builder sets it when the altitude is given in feet). Code that builds a `MicEReport` with a struct literal needs the field or `..Default::default()`.

**Refusals.** `EncodeError` is `#[non_exhaustive]` and its `message` field is private: read `message()` or `Display`. The wording of a reason is not part of the stable API, so that a machine-readable reason can be added without a major version.

**What can be built by hand.** `Data`, `Packet`, `Diagnostic`, `Device`, `HeaderError` and `Unrecognized` are `#[non_exhaustive]`: a `match` on `Data` or `Unrecognized` needs a `_` arm, and a `Packet` is made with `Packet::create` or `Packet::decode`, not a struct literal. The data structs a program fills in (`Positioned`, `Weather`, the reports and the rest) stay exhaustive with `Default`. `InvalidAddress`'s field is private: read `text()`. `Timestamp::to_bytes` is removed, use `Display` (`timestamp.to_string()`).

**Other.** `Code::ALL` is `&'static [Code]` (not `[Code; 64]`), and `ParseOptions` holds its tolerated codes in a `u128`, so a 65th code fits without another break.

### Added

- `TelemetryMetadataBuilder`: `telemetry_names`, `telemetry_units`, `telemetry_coefficients` and `telemetry_bits` return it, with `addressee(&str)` for metadata about another station (the sending station itself by default), `to_data()` and `build()`.
- `WeatherBuilder::wind_speed_mph`.
- `Symbol::description()`: what the symbol stands for ("Car"), ignoring any overlay.
- `Phg::power_watts()`, `height_feet()`, `gain_db()` and `directivity_degrees()` (`None` for omni).
- `impl Display` for `Severity` and `Timestamp`.
- `EncodeError::message()` and `InvalidAddress::text()`.
- `examples/diff_dump.rs` writes the `symbol` and `phg` keys of the API view, and maps every recipe key it did before; telemetry metadata for another station and the CTCSS and DCS tones are now buildable, so the recipe keys for them are no longer reported unsupported.

### Fixed

- The `course_degrees` doc: 0 is the spec's unknown course, 360 is north, 1-360 is a known course (it said 1-360).

## 0.4.0

The encoder writes exactly the bytes the vectors' Encoding rule gives, which the vectors now check byte for byte (packet-net/aprs-vectors, exact bytes batch 1, rulings E1-E12).

**Breaking:** `Footprint`'s `latitude` and `longitude` are now the text as sent (`String`), a leading space included, so that ` 34.0` and `-.1715` write back unchanged, as telemetry values and coefficients already do. `Footprint::latitude_degrees` and `Footprint::longitude_degrees` read them as numbers, and `Footprint::new` writes them from numbers as APRS12c does (a space before a positive value). `Footprint` is no longer `Copy`.

- A position, object or item report's comment is written in one order: the voice frequency and its fields, the signpost or corridor braces, the `/A=` altitude, the free text (after a space when a frequency comes before it), base-91 telemetry and the `!DAO!`. The frequency is where radios read it, in the first bytes of the comment (APRS12c ch. 18); it was written after the altitude. Straight after a seven-byte data extension it follows a `/`, and after a PHGR, which ends in one, it follows straight on (`PHG33403/145.225MHz`, not `PHG33403//145.225MHz`); the same in Mic-E status text, where a `/A=` between them takes the `/`'s place.
- A `!DAO!` on a compressed position carries the digits of the position reported (the compressed one), as it would on that position written uncompressed (`!sCp!`); it had `!!`.
- Mic-E: a speed of 190-199 knots is written with `/`, the printable one of the two characters APRS12c ch. 10 gives, not DEL. Status text that would start with 0x1D (or a type code character) is written after a `/`, even when it is too short to read as Rev 0 telemetry. A course of 0, which Mic-E sends for an unknown course, is refused: due north is 360.
- Mic-E with ambiguity: the longitude digits a decoder ignores hold the centre it reports (`34.50` for 34.494 minutes at ambiguity 2), and the latitude is the box the position is in, rounded then blanked as an uncompressed position's is; a position not at the centre of its box could land in the box below. A latitude whose box is centred past 90 degrees is refused.
- A Mic-E `/A=` altitude in feet the Mic-E bytes cannot hold is written rounded to the foot, as a position's is; it was refused.
- An NMEA sentence's latitude, longitude, course, speed and altitude, when given, agree with the sentence within a part in 10^9, as the vectors compare numbers; a value computed by other arithmetic was refused.
- Snowfall under an inch is written `.` and two digits (`.50`); 0.5 was `0.5`.
- The vectors test runner compares the bytes written with each case's `canonical_info`, and knows the new `rounded` re-encoding.

Builder gaps (the builder can say everything the neutral form does):

**Breaking:** `course`, `WeatherBuilder::wind`, `humidity` and `luminosity` take `impl Into<f64>` instead of `u16`/`u8`, so a fractional value can be given. Integer and float literals, `u8`, `u16`, `i32` and `f32` values still compile; what breaks is coercing the method to a function pointer, or an argument whose type was inferred as `u16` (e.g. `course(s.parse()?)`). Whether a course is unknown (0) and whether it is in range (1-360) is decided in whole degrees; a known course is then rounded once, halves away from zero, to the step its field holds (whole degrees, or 4 degrees compressed), so 301.7 compressed is 300, not 304. A compressed 1.2 is north (360), 0.3 is unknown and refused, -0.7 and 360.6 are refused. Humidity and luminosity are rounded to whole numbers and range-checked after. Needs a minor version bump.

- `WeatherBuilder::wind_direction` gives a wind direction alone. `TelemetryBuilder::channels` takes `&[Option<f64>]`, so any analog channel can be empty, not only the last ones.
- `examples/diff_dump.rs` and the vectors runner read a fractional compressed course or wind direction by rounding once to the 4-degree step, instead of reporting it unsupported. The data types (`Positioned::course_degrees`, `Weather::wind_direction_degrees`, ...) still hold whole numbers: widening them to `f64` would change what every decoder user reads, so the rounding is done by the builder and the runner, which are the only places a fractional value arrives.

Exact bytes batch 2 (rulings F1-F17, B1-B8, A1-A2); the vectors submodule moves to aprs-vectors exact-bytes `e27a79f`:

- A value between the steps its field holds is rounded to the nearest step, halves away from zero, and refused only when the rounded value is outside the field: weather values (a gust of 5.4 mph is `g005`, a pressure of 986.95 mbar `b09870`), `/A=` feet (-0.6 is `-00001`), `RNG` miles (1.5 is `RNG0002`), speeds (a Mic-E speed of -0.3 is 0), DAO digits, latitude and longitude minutes, and the cs bytes. A value within 1e-6 of a step's half, measured in steps, is the half, so a latitude whose minutes compute as 58.584999999999996 is written `58.59`. Weather values between steps, `/A=` values and RNG values were refused, the last two with a message blaming the comment text. A refusal for a report with no comment now names the field that does not read back.
- The cs bytes' steps follow their formula for every whole s, and a value rounds to the nearest; one whose s falls below the byte is refused: a compressed speed of -0.03 knots is written as 0 and -0.3 refused, a compressed range of 1.95 miles is written as 2 and 1.9 refused (G3). A range under 2 miles was refused.
- Mic-E: a strict decoder reads a later altitude as text and lifts the `!DAO!` as usual, and rejects the report (`mic-e-altitude-not-first`) only when the text left once base-91 telemetry and the `!DAO!` are both lifted holds an altitude across a place one was taken from. Rev 0 telemetry whose last byte is a CR or LF, which would read back as a trailing line break, is refused.
- A position found after other text by the obsolete TNC beacon rule is noted (`position-not-at-start`) before the position's own diagnostics, in reading order.
- Snowfall is rounded to hundredths if that leaves it under 1, else to tenths if under 10, else to a whole number, and a whole result is three digits (0.995 is `001`, 9.96 `010`).
- Compressed: a course with no speed, a speed with no course, and a course of 0 (which reads back as 360) are refused; one alone was written with the other as 0. A compressed weather report whose GGA altitude the cs bytes cannot hold exactly writes the nearest cs value; it was refused.
- A `!DAO!` on an ambiguous position is refused whatever its precision; `!W  !` was written.
- `T#MIC` with an empty first channel is refused with a reason of its own.
- Mic-E: a locator followed by base-91 telemetry or a `!DAO!` alone is written with them straight after it (`IO91/G|!:&0'p|`), and decodes with no `missing-space-after-locator`, since telemetry and a DAO are lifted out before the locator is read. Status text that would join the digits before it into a Mic-E altitude (`/A=000101{5}`) is written after a space, the only clean form; it was refused.
- Builder: a course of 0 is the spec's unknown course (`000` uncompressed, no course in Mic-E, refused compressed); a Mic-E report's type code is `` ` `` with `messaging()` and `'` without (it had none); a Mic-E altitude from `altitude_metres` goes in the Mic-E altitude bytes rounded to the metre, and one from `altitude` in a `/A=`; an ambiguous position reports the box that holds it, its digits cut rather than rounded (a value within 1e-6 of a box's edge is on it); a telemetry sequence over 999 is refused. Temperatures are rounded once, by the encoder.
- The vectors runner and `examples/diff_dump.rs` read numbers given without their text as the vectors say: a telemetry value or sequence that is a whole number from 0 to 999 is three digits (`005`, 190.0 as `190`), any other number the shortest decimal with no exponent (`-94`, 1e61 in full), and a footprint's latitude and longitude 0 take the leading space. A fractional course, humidity, wind direction, luminosity, raw rain count or footprint radius, which this crate holds as whole numbers, is rounded on the way in, except a compressed course or wind direction, whose 4-degree steps would round it twice: that is left unsupported.
- `examples/diff_dump.rs` writes the bytes it re-encodes and the API view (what the packet's accessors say), and has `--encode` and `--build` modes for data and builder recipes from the vectors' `tools/generate.py`.

## 0.3.0

Brought into line with the rulings from differential fuzzing of all five implementations (packet-net/aprs-vectors, 159 new cases in seven rounds, the rules in its README and interpretations.md). The vectors submodule moves to them.

**Breaking:** `Nmea` has a new field, `comment`: the text after a sentence's checksum, kept as sent (TinyTrack and FreeTrak send one). `sentence` now ends at the checksum. Code that builds an `Nmea` with a struct literal must set `comment`, or use `..Nmea::default()`. `Packet` has a new field, `third_party`, set on the packet inside a third-party packet, whose header is kept as sent: `Packet::q_construct` finds no q-construct there, since one is read only in the outer header. Code that builds a `Packet` with a struct literal must set it.

- `Address::third_party_source`: the source of a packet inside a third-party packet, which APRS12c ch. 17 lets be any 1-9 printable ASCII characters other than `>` and `:`. A third-party packet with such a source (`PY2SP_R-R`) now decodes and re-encodes.
- NMEA: `$` text must be an NMEA 0183 sentence, or it is `invalid-nmea`: printable ASCII (not DEL), an address field of five upper-case letters or digits (or `P` and three or more), at least one field, no `$`, and `*` only to start the checksum. The checksum ends the sentence and is verified even when a comment follows. Its fields are read one by one, so a short sentence keeps what it has; a coordinate needs a degree digit and minutes under 60; GGA's quality is one digit; a proprietary sentence such as `$PGRMC` is not read as RMC.
- Mic-E: the Rev 0 data type identifiers 0x1C and 0x1D get an `ObsoleteFormat` info. A PHG straight after the type code is lifted before an altitude is looked for later in the status text, and a `!DAO!` is never joined across a removed altitude.
- Positions: a latitude's or longitude's range is checked before its hemisphere letter. A DF bearing over 360 is `OutOfRangeValue` (tolerated) and the whole `/BRG/NRQ` is dropped, as an out-of-range course is; it was left in the comment. A late PHG, RNG or DFS is the first well-formed one, so `PHG12` no longer hides a real PHG after it. Signpost and corridor braces are the first well-formed ones wherever they are: `{`, 1-3 characters that are not braces (printable ASCII for a signpost, digits for a corridor), and `}`. Braces that do not qualify are comment text and do not stop the search, so `{5Wm{55}` and `{{5}` hold the signposts `55` and `5`. A garbled timestamp, in an object or a `/` or `@` report, is judged on the position after it, not on the whole report.
- Weather: the wind is decided where the extension belongs, before any field. A `c` with a value is the wind direction wherever it comes before the wind is known; a bare `c` is not; a direction without a speed is incomplete wind; `L` and `l` are one field, so a second luminosity ends the fields. After a position's fields, base-91 telemetry and a `!DAO!` are lifted out before the rest is read as the software type and unit. Snowfall keeps its width of three characters: `s.050` is 0.05 inches and a `0` of text, not a short run of dots, and a short run of dots followed by a digit (`s..6`) is not a field at all.
- Messages and queries: a query type is upper-case letters (`?IGAT7?` is `invalid-general-query`, `?APRS000` a plain message); a directed query's target is 1-9 letters, digits or `-`, one space before it is a separator and spaces after it are padding. A general query footprint beyond 90 or 180 degrees is `invalid-general-query`, and so is a space before a negative latitude or longitude: the leading space is for a positive value (APRS12c ch. 15), and the encoder writes none before a negative one.
- Telemetry: a value or an `EQNS.` coefficient has no `+`, a coefficient must be a finite number (`0eN` is 0), and only spaces around one are padding.
- Capabilities: only spaces (U+0020) are trimmed, so a CR is a control character and makes the report free text.
- Agrelo DF: exactly `%bbb/q`, with a bearing of 000 to 360.
- The encoder never writes bytes that read back as different data: it writes an equivalent form or refuses. New refusals: course and speed together with a range in compressed cs bytes, a compression type with nothing for the cs bytes to carry, a GGA type with anything but the altitude, a `{` in a telemetry project title, a capability value that starts or ends with a space, a third-party packet whose inner header had a tolerated defect, message data that would read back as something else (an ack, metadata, a query or a bulletin), a telemetry sequence that would not read back, a weather report with a course, speed or data extension other than its wind, a report with the weather station symbol but no weather, a weather software type and unit that would read back as fields (`h` and `89b1`), a compressed weather report with a wind speed but no direction (or the reverse), since the cs bytes carry both, and raw weather data that is not printable ASCII.
- The encoder now writes what it used to refuse: PHG and DFS height codes above 9, a GGA altitude under 1 foot (as cs `!!` and a `/A=`), a snowfall with a fraction (`s1.5`, `s.25`), NWS bulletins longer than 67 characters, a digit `!DAO!` datum with no added precision, and user-defined data with any bytes. A directed query of a type the spec does not define has its target after a space (`?FOO N0QBF`), and an APRSH target is padded to 9 characters. In Mic-E status text, anything after a grid locator's `/G`, a data extension included, comes after a space, and an altitude the whole-metre Mic-E bytes cannot hold exactly (`/A=030105`) stays a `/A=`.
- Fixed: a compressed course or wind direction was cut down to the 4-degree step below (103 degrees written as 100); it is now rounded to the nearest (104).

## 0.2.1

- Mic-E Rev 0 binary telemetry (0x1D and five bytes after the symbol) is read into `MicEReport::legacy_telemetry`, with an `ObsoleteFormat` info, and written back; it was left in the comment. A value of 255 is refused on encoding. The ruling is shared by all five implementations (packet-net/aprs-vectors, "Mic-E Rev 0 binary telemetry").

## 0.2.0

- `Station`: a fluent builder for the packets an application sends, as the C#, Python and TypeScript implementations have. `Station::new("M0LTE-9")?.position(lat, lon).symbol(Symbol::CAR).speed(36.0).build()?`, and the same for objects, items, Mic-E, weather, messages, acks and rejects, bulletins, status and telemetry.
- Every defined symbol by name (`Symbol::CAR`), the same names the other implementations use, with `Symbol::new` and `Symbol::with_overlay`.
- `Timestamp::dhm`, `Timestamp::hms` and `Timestamp::mdhm`; `VoiceFrequency` implements `Default`.
- Fixed: a position with ambiguity was written one box low unless it was already the centre of its box: 51.4543 with two digits of ambiguity became `5126.  N`, not `5127.  N`. Decoded positions, which are centres, were not affected. Found by the builder tests, and pinned by a new vectors case.

## 0.1.0

First release: an APRS encoder and decoder for every APRS 1.2 data type, `no_std` with `alloc`, built and tested against the conformance vectors in [packet-net/aprs-vectors](https://github.com/packet-net/aprs-vectors). The device identification data is aprs-deviceid commit 845e3f8 (2026-09-18).

- Passes every vectors case (7,175 checks), with no known differences.
- Agrees with Packet.Aprs (C#) and pdn-aprs for Python on every one of 6,879,893 APRS-IS packets: lenient and strict decoding, and re-encoding. The comparison tooling is in packet-net/aprs-vectors (`tools/compare.py`); `examples/diff_dump.rs` writes this crate's side of it.
