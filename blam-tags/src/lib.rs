//! # blam-tags
//!
//! A standalone Rust library for reading, writing, and manipulating
//! Halo 3 / Reach tag files. No ManagedBlam.dll, no .NET, no engine
//! required. Built around a byte-exact roundtrip read/write path (see
//! [`file::TagFile`]) with a typed per-field API layered on top
//! (see [`fields::TagFieldData`], [`data::TagStruct::parse_field`],
//! and [`data::TagStruct::set_field`]).
//!
//! ## Quick start
//!
//! ```no_run
//! use blam_tags::file::TagFile;
//!
//! let tag = TagFile::read("path/to/masterchief.biped").unwrap();
//! // `tag.tag_stream.layout` is the schema; `tag.tag_stream.data` is
//! // the root block whose elements are the actual tag data.
//! ```
//!
//! ## Module tour
//!
//! - [`io`] — primitive readers/writers + 12-byte chunk header helpers.
//! - [`math`] — bounds, colors, vectors, points, euler angles.
//! - [`fields`] — `TagFieldType` dispatch enum, `TagFieldData`
//!   per-field value enum, plus `deserialize_field` / `serialize_field`.
//! - [`layout`] — `TagLayout` (the schema) and all its definition
//!   records.
//! - [`data`] — `TagStruct` / `TagBlockData` / `TagSubChunkContent`
//!   (per-tag instance data) plus `parse_field` / `set_field` and
//!   `TagBlockData` add/insert/duplicate/delete/clear operations.
//! - [`path`] — `/`-separated path navigation into the data tree.
//! - [`stream`] — the `tag!` / `want` / `info` outer stream chunks.
//! - [`crate::file`] — [`file::TagFile`], the fully parsed tag file.

pub mod math;
pub mod io;
pub mod fields;
pub mod layout;
pub mod data;
pub mod path;
pub mod stream;
pub mod file;
