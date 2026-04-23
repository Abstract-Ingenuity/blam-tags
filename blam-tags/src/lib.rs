//! # blam-tags
//!
//! A standalone Rust library for reading, writing, and manipulating
//! Halo 3 / Reach tag files. No ManagedBlam.dll, no .NET, no engine
//! required. Built around a byte-exact roundtrip read/write path
//! with a concept-oriented façade layered on top.
//!
//! ## Quick start
//!
//! ```no_run
//! use blam_tags::TagFile;
//!
//! let mut tag = TagFile::read("masterchief.biped").unwrap();
//!
//! // Read a field by `/`-separated path.
//! let jump = tag.root().field_path("jump velocity").unwrap();
//! println!("{}: {} = {}", "jump velocity", jump.type_name(), jump.value().unwrap());
//!
//! // Toggle a flag by name.
//! tag.root_mut()
//!     .field_path_mut("unit/flags").unwrap()
//!     .flag_mut("has_hull").unwrap()
//!     .toggle();
//!
//! tag.write("masterchief.biped.edited").unwrap();
//! ```
//!
//! ## Module tour
//!
//! **High-level façade (start here):**
//!
//! - [`api`] — data-side façade: [`TagStruct`], [`TagField`],
//!   [`TagBlock`], [`TagArray`], [`TagFlag`], [`TagResource`], and
//!   their mutable counterparts. All reachable from [`TagFile`].
//! - [`definition`] — schema-side façade: [`TagStructDefinition`],
//!   [`TagFieldDefinition`], [`TagBlockDefinition`],
//!   [`TagArrayDefinition`], reachable from [`TagFile::definitions`].
//! - [`file::TagFile`] — the fully parsed tag file (re-exported as
//!   [`TagFile`]).
//!
//! **Lower-level, used by the façade:**
//!
//! - [`fields`] — [`TagFieldType`] dispatch enum, [`TagFieldData`]
//!   per-field value enum (with [`std::fmt::Display`]), plus
//!   `deserialize_field` / `serialize_field`.
//! - [`layout`] — [`layout::TagLayout`] (the schema chunk) and all
//!   its record types.
//! - [`data`] — per-tag instance data storage (opaque; driven through
//!   the [`api`] façade).
//! - [`path`] — `/`-separated path navigation (crate-internal).
//! - [`stream`] — the `tag!` / `want` / `info` outer stream chunks.
//! - [`io`] — primitive readers/writers + 12-byte chunk header helpers.
//! - [`math`] — bounds, colors, vectors, points, euler angles.

pub mod math;
pub mod io;
pub mod fields;
pub mod layout;
pub mod data;
pub mod path;
pub mod stream;
pub mod file;
pub mod api;
pub mod definition;

// Façade re-exports — the recommended surface for editing tags.
pub use api::{
    TagArray, TagArrayMut, TagBlock, TagBlockMut, TagField, TagFieldMut, TagFlag, TagFlagMut,
    TagFlagOption, TagGroup, TagIndexError, TagOptions, TagResource, TagResourceKind, TagSetError,
    TagStruct, TagStructMut,
};
pub use definition::{
    TagApiInteropDefinition, TagArrayDefinition, TagBlockDefinition, TagDefinitions,
    TagFieldDefinition, TagResourceDefinition, TagStructDefinition,
};
pub use fields::{
    format_group_tag, ApiInteropData, StringIdData, TagFieldData, TagFieldType, TagReferenceData,
};
pub use file::TagFile;
