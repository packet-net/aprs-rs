# Changelog

## Unreleased

## 0.1.0

First release: an APRS encoder and decoder for every APRS 1.2 data type, `no_std` with `alloc`, built and tested against the conformance vectors in [packet-net/aprs-vectors](https://github.com/packet-net/aprs-vectors). The device identification data is aprs-deviceid commit 845e3f8 (2026-09-18).

- Passes every vectors case (7,175 checks), with no known differences.
- Agrees with Packet.Aprs (C#) and pdn-aprs for Python on every one of 6,879,893 APRS-IS packets: lenient and strict decoding, and re-encoding. The comparison tooling is in packet-net/aprs-vectors (`tools/compare.py`); `examples/diff_dump.rs` writes this crate's side of it.
