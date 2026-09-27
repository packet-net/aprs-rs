# Changelog

## Unreleased

The encoder writes exactly the bytes the vectors' Encoding rule gives, which the vectors now check byte for byte (packet-net/aprs-vectors, exact bytes batch 1, rulings E1-E12).

**Breaking:** `Footprint`'s `latitude` and `longitude` are now the text as sent (`String`), a leading space included, so that ` 34.0` and `-.1715` write back unchanged, as telemetry values and coefficients already do. `Footprint::latitude_degrees` and `Footprint::longitude_degrees` read them as numbers, and `Footprint::new` writes them from numbers as APRS12c does (a space before a positive value). `Footprint` is no longer `Copy`.

- A position, object or item report's comment is written in one order: the voice frequency and its fields, the signpost or corridor braces, the `/A=` altitude, the free text (after a space when a frequency comes before it), base-91 telemetry and the `!DAO!`. The frequency is where radios read it, in the first bytes of the comment (APRS12c ch. 18); it was written after the altitude. Straight after a seven-byte data extension it follows a `/`, and after a PHGR, which ends in one, it follows straight on (`PHG33403/145.225MHz`, not `PHG33403//145.225MHz`); the same in Mic-E status text, where a `/A=` between them takes the `/`'s place.
- A `!DAO!` on a compressed position carries the digits of the position reported, as it would on the position written uncompressed (`!sCp!`); it had `!!`.
- Mic-E: a speed of 190-199 knots is written with `/`, the printable one of the two characters APRS12c ch. 10 gives, not DEL. Status text that would start with 0x1D (or a type code character) is written after a `/`, even when it is too short to read as Rev 0 telemetry. A course of 0, which Mic-E sends for an unknown course, is refused: due north is 360.
- Snowfall under an inch is written `.` and two digits (`.50`); 0.5 was `0.5`.
- The vectors test runner compares the bytes written with each case's `canonical_info`, and knows the new `rounded` re-encoding.
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
