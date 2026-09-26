# Changelog

## Unreleased

- `Station`: a fluent builder for the packets an application sends, as the C#, Python and TypeScript implementations have. `Station::new("M0LTE-9")?.position(lat, lon).symbol(Symbol::CAR).speed(36.0).build()?`, and the same for objects, items, Mic-E, weather, messages, acks and rejects, bulletins, status and telemetry.
- Every defined symbol by name (`Symbol::CAR`), the same names the other implementations use, with `Symbol::new` and `Symbol::with_overlay`.
- `Timestamp::dhm`, `Timestamp::hms` and `Timestamp::mdhm`; `VoiceFrequency` implements `Default`.
- Fixed: a position with ambiguity was written one box low unless it was already the centre of its box: 51.4543 with two digits of ambiguity became `5126.  N`, not `5127.  N`. Decoded positions, which are centres, were not affected. Found by the builder tests, and pinned by a new vectors case.

## 0.1.0

First release: an APRS encoder and decoder for every APRS 1.2 data type, `no_std` with `alloc`, built and tested against the conformance vectors in [packet-net/aprs-vectors](https://github.com/packet-net/aprs-vectors). The device identification data is aprs-deviceid commit 845e3f8 (2026-09-18).

- Passes every vectors case (7,175 checks), with no known differences.
- Agrees with Packet.Aprs (C#) and pdn-aprs for Python on every one of 6,879,893 APRS-IS packets: lenient and strict decoding, and re-encoding. The comparison tooling is in packet-net/aprs-vectors (`tools/compare.py`); `examples/diff_dump.rs` writes this crate's side of it.
