# Changelog

## Unreleased

Decoding now agrees with Packet.Aprs on every one of 6.9 million APRS-IS packets, found by comparing the two implementations packet by packet (packet-net/aprs-vectors `tools/compare.py`); each rule that differed is pinned by a new case in the vectors.

- **Breaking:** `TelemetryLabels`, `TelemetryCoefficients` and `TelemetryBits` gain `message_id`, and `Tone` gains `ToneBurst` (`1750`, APRS12c ch. 18).
- q-constructs with a lower-case third letter (`qAr`, `qAo`) are recognised.
- Telemetry reports: a sequence of any length, trailing empty values counted, a malformed sequence an error; `EQNS.` tolerates trailing commas and spaces and reads exponents.
- Weather: the field run, extra fields and software/unit follow the vectors' rules; a `DDD/SSS` wind extension after a compressed position replaces the cs wind, and c/s wind fields after one with blank cs are read.
- Comments: structured elements are lifted out in the vectors' order (telemetry and DAO, altitude, braces, a late data extension, frequency), which changes the comment text left in some reports.
- Messages: an empty addressee, metadata checked before its text, and upper-case query types the spec does not define.
- Objects and items: short objects are truncated, garbled timestamps are recognised by the vectors' rule, and item names may contain `!` or `_` after the third character.
- NMEA: positions with fewer degree digits, and sentences with empty fields, decode.
- Capabilities: spaces around tokens are trimmed, and free text is split at commas.
- Positions: checks run in field order, so a bad latitude is reported before a bad longitude; a latitude past 90 degrees with its ambiguity centre is refused; a Mic-E latitude past 90 degrees is refused.
- Encoding: a compressed altitude that the cs bytes cannot carry exactly is also written as `/A=`.

## 0.1.0

First release: an APRS encoder and decoder for every APRS 1.2 data type, `no_std` with `alloc`, built and tested against the conformance vectors in [packet-net/aprs-vectors](https://github.com/packet-net/aprs-vectors). The device identification data is aprs-deviceid commit 845e3f8 (2026-09-18).
