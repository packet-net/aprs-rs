#![doc = include_str!("../README.md")]
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

mod base91;
mod builder;
mod comment;
mod context;
mod data;
mod decode;
mod deviceid;
mod deviceid_data;
mod diagnostics;
mod encode;
mod message;
mod mic_e;
mod object;
mod options;
mod other;
mod packet;
mod position;
mod status;
mod symbols;
mod telemetry;
mod text;
mod timestamp;
mod weather;

pub use builder::{
    AckBuilder, DEFAULT_DESTINATION, DataBuilder, ItemBuilder, MessageBuilder, MicEBuilder, ObjectBuilder, PositionBuilder, Station,
    StatusBuilder, TelemetryBuilder, WeatherBuilder,
};
pub use data::*;
pub use deviceid::{Device, database_version, tocall};
pub use diagnostics::{Code, Diagnostic, Severity};
pub use options::ParseOptions;
pub use packet::{Address, EncodeError, HeaderError, InvalidAddress, Packet, PathEntry, QConstruct};
