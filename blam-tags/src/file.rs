//! Tag file: the outermost container. A tag file is a fixed-width
//! 64-byte [`TagFileHeader`] followed by a mandatory `tag!` stream
//! and up to three optional streams in fixed order: `want`
//! (dependency list), `info` (import info), `assd` (asset depot
//! storage).

use std::error::Error;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::TagReadError;
use crate::io::*;
use crate::layout::TagLayout;
use crate::stream::TagStream;

/// Fixed 64-byte preamble at the start of every tag file.
///
/// Layout: `pad[36] + build_version + build_number + version +
/// group_tag + group_version + checksum + signature`.
#[derive(Debug)]
pub struct TagFileHeader {
    /// 36 bytes (9 × u32) of zero padding. Preserved verbatim — no
    /// semantic meaning observed.
    pub pad: [u8; 36],
    /// Authoring-toolset build version.
    pub build_version: i32,
    /// Authoring-toolset build number.
    pub build_number: i32,
    /// Tag-file format version (C++: `single_tag_file_version`).
    /// Distinct from `group_version` and from the per-blay layout
    /// version.
    pub version: u32,
    /// 4-byte tag group (e.g. `b"scnr"` for scenario). BE-packed u32
    /// matching the convention used for chunk signatures.
    pub group_tag: u32,
    pub group_version: u32,
    pub checksum: u32,
    /// Always `b"BLAM"`. Used to validate that this is a tag file.
    pub signature: u32,
}

impl TagFileHeader {
    /// Read the fixed 64-byte header. Returns an error (rather than
    /// panicking) when the `BLAM` signature doesn't match, so callers
    /// walking directories full of non-tag files can filter them out
    /// cleanly.
    pub fn read<R: Seek + Read>(reader: &mut std::io::BufReader<R>) -> Result<Self, TagReadError> {
        let pad = read_u8_array(reader)?;
        let build_version = read_u32_le(reader)? as i32;
        let build_number = read_u32_le(reader)? as i32;
        let version = read_u32_le(reader)?;
        let group_tag = read_u32_le(reader)?;
        let group_version = read_u32_le(reader)?;
        let checksum = read_u32_le(reader)?;
        let signature_offset = reader.stream_position()?;
        let signature = read_u32_le(reader)?;
        if signature != u32::from_be_bytes(*b"BLAM") {
            return Err(TagReadError::BadChunkSignature {
                offset: signature_offset,
                expected: *b"BLAM",
                got: signature.to_be_bytes(),
            });
        }

        Ok(Self {
            pad,
            build_version,
            build_number,
            version,
            group_tag,
            group_version,
            checksum,
            signature,
        })
    }

    /// Write this header. Mirrors `TagFileHeader::read`: fixed 64-byte
    /// layout with `pad[36] + build_version + build_number + version +
    /// group_tag + group_version + checksum + signature`.
    pub fn write<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&self.pad)?;
        writer.write_all(&(self.build_version as u32).to_le_bytes())?;
        writer.write_all(&(self.build_number as u32).to_le_bytes())?;
        writer.write_all(&self.version.to_le_bytes())?;
        writer.write_all(&self.group_tag.to_le_bytes())?;
        writer.write_all(&self.group_version.to_le_bytes())?;
        writer.write_all(&self.checksum.to_le_bytes())?;
        writer.write_all(&self.signature.to_le_bytes())?;
        Ok(())
    }
}

/// A fully parsed tag file.
///
/// Structure on disk: `header` (64 bytes), then `tag_stream` (mandatory,
/// `tag!` chunk), then zero-or-one of each optional stream in this
/// fixed order: `dependency_list_stream` (`want`), `import_info_stream`
/// (`info`), `asset_depot_storage_stream` (`assd`). Missing optional
/// streams simply end the file.
#[derive(Debug)]
pub struct TagFile {
    pub header: TagFileHeader,
    /// The `tag!` stream — carries the tag's main payload. Access
    /// the root via [`TagFile::root`] / [`TagFile::root_mut`].
    pub(crate) tag_stream: TagStream,
    /// The optional `want` stream — lists tag dependencies resolved at
    /// build time. Access via [`TagFile::dependency_list`] /
    /// [`TagFile::dependency_list_mut`].
    pub(crate) dependency_list_stream: Option<TagStream>,
    /// The optional `info` stream — import / source metadata. Access
    /// via [`TagFile::import_info`] / [`TagFile::import_info_mut`].
    pub(crate) import_info_stream: Option<TagStream>,
    /// The optional `assd` stream — asset depot storage (tag-editor
    /// icon pixel data). Zero presence in the observed H3/Reach
    /// corpus, but ManagedBlam registers a schema for it so we
    /// round-trip it faithfully when it does appear.
    pub(crate) asset_depot_storage_stream: Option<TagStream>,
}

impl TagFile {
    /// Create a fresh tag file from a JSON schema dumped by
    /// `halo3_dump_tag_definitions_json.py`. The resulting file
    /// has:
    ///
    /// - A header with `group_tag` / `group_version` pulled from the
    ///   schema, `signature = b"BLAM"`, and everything else zeroed
    ///   (pad, build_version, build_number, version, checksum).
    /// - A `tag!` stream whose layout matches the schema and whose
    ///   root block contains one zero-filled default element (nested
    ///   blocks empty, tag_references null, string_ids empty,
    ///   api_interops reset).
    /// - No `want` / `info` streams.
    ///
    /// Fails if the schema JSON doesn't parse or any struct's
    /// computed size doesn't match the schema's stated size
    /// (both surfaced as `blam_tags::TagSchemaError`).
    pub fn new<P: AsRef<Path>>(schema_path: P) -> Result<Self, Box<dyn Error>> {
        let (layout, meta) = TagLayout::from_json_with_meta(schema_path)?;
        let tag_stream = TagStream::new_default(layout);
        let header = TagFileHeader {
            pad: [0u8; 36],
            build_version: 0,
            build_number: 0,
            version: 0,
            group_tag: meta.tag,
            group_version: meta.version,
            checksum: 0,
            signature: u32::from_be_bytes(*b"BLAM"),
        };
        Ok(Self {
            header,
            tag_stream,
            dependency_list_stream: None,
            import_info_stream: None,
            asset_depot_storage_stream: None,
        })
    }

    /// Open `path` and parse a complete tag file. The read asserts that
    /// the file ends exactly at the last consumed stream, so trailing
    /// garbage surfaces as [`TagReadError::ChunkSizeMismatch`].
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self, TagReadError> {
        let reader = std::io::BufReader::with_capacity(64 * 1024, std::fs::File::open(path)?);
        Self::read_from(reader)
    }

    /// Parse a complete tag file from any [`Read`] + [`Seek`] source —
    /// wrapped in a [`std::io::BufReader`] internally if not already
    /// one. Useful for fuzzing, in-memory tag manipulation, and
    /// embedding tag bytes in archives.
    pub fn read_from_bytes(bytes: &[u8]) -> Result<Self, TagReadError> {
        Self::read_from(std::io::BufReader::new(std::io::Cursor::new(bytes)))
    }

    fn read_from<R: Read + Seek>(mut reader: std::io::BufReader<R>) -> Result<Self, TagReadError> {
        // Get the size of the file
        reader.seek(SeekFrom::End(0))?;
        let tag_file_size = reader.stream_position()?;
        reader.seek(SeekFrom::Start(0))?;

        // Read the tag file header
        let header = TagFileHeader::read(&mut reader)?;

        // The 'tag!' chunk contains the tag stream
        let tag_stream = TagStream::read(u32::from_be_bytes(*b"tag!"), &mut reader)?;

        let mut dependency_list_stream = None;
        let mut import_info_stream = None;
        let mut asset_depot_storage_stream = None;

        // Check if there are any 'want' / 'info' / 'assd' chunks
        while reader.stream_position()? != tag_file_size {
            let chunk_header_offset = reader.stream_position()?;
            let chunk_signature = read_u32_le(&mut reader)?;
            reader.seek(SeekFrom::Start(chunk_header_offset))?;

            match &chunk_signature.to_be_bytes() {
                b"want" => {
                    if dependency_list_stream.is_some() {
                        return Err(TagReadError::DuplicateOptionalStream { signature: *b"want" });
                    }
                    dependency_list_stream = Some(TagStream::read(chunk_signature, &mut reader)?);
                }

                b"info" => {
                    if import_info_stream.is_some() {
                        return Err(TagReadError::DuplicateOptionalStream { signature: *b"info" });
                    }
                    import_info_stream = Some(TagStream::read(chunk_signature, &mut reader)?);
                }

                b"assd" => {
                    if asset_depot_storage_stream.is_some() {
                        return Err(TagReadError::DuplicateOptionalStream { signature: *b"assd" });
                    }
                    asset_depot_storage_stream = Some(TagStream::read(chunk_signature, &mut reader)?);
                }

                signature => {
                    return Err(TagReadError::UnknownSubChunkSignature {
                        context: "tag-file top-level",
                        signature: *signature,
                    });
                }
            }
        }

        if reader.stream_position()? != tag_file_size {
            return Err(TagReadError::ChunkSizeMismatch {
                chunk: "tag file",
                started_at: 0,
                ended_at: reader.stream_position()?,
                expected_end: tag_file_size,
            });
        }

        Ok(Self {
            header,
            tag_stream,
            dependency_list_stream,
            import_info_stream,
            asset_depot_storage_stream,
        })
    }

    /// Write this tag file to `path`. Mirrors `TagFile::read`: file
    /// header, then `tag!` stream, then any attached `want`, `info`,
    /// `assd` streams in that fixed order. IMPORTANT: never write to
    /// the source tag path — write to a temp file, then read it back
    /// for byte-exact roundtrip verification.
    pub fn write<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);
        self.write_to(&mut writer)
    }

    /// Serialize this tag to a `Vec<u8>`. Mirrors [`TagFile::write`];
    /// useful for fuzzing roundtrips and in-memory tag pipelines.
    pub fn write_to_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.write_to(&mut buf)?;
        Ok(buf)
    }

    fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.header.write(writer)?;
        self.tag_stream.write(u32::from_be_bytes(*b"tag!"), writer)?;

        if let Some(dependency_list_stream) = &self.dependency_list_stream {
            dependency_list_stream.write(u32::from_be_bytes(*b"want"), writer)?;
        }
        if let Some(import_info_stream) = &self.import_info_stream {
            import_info_stream.write(u32::from_be_bytes(*b"info"), writer)?;
        }
        if let Some(asset_depot_storage_stream) = &self.asset_depot_storage_stream {
            asset_depot_storage_stream.write(u32::from_be_bytes(*b"assd"), writer)?;
        }

        Ok(())
    }

    //
    // Optional-stream attach/detach/rebuild. Stream schemas are
    // loaded from JSON (as dumped by
    // `halo3_dump_tag_definitions_json.py`) — typically
    // `tag_dependency_list.json` for `want` and
    // `tag_import_information.json` for `info`.
    //

    /// Attach an empty `want` (dependency list) stream with one
    /// zero-filled root element. No-op if a dependency list is
    /// already present. `schema_path` is the per-group JSON for
    /// `tag_dependency_list` — required only if no stream exists.
    pub fn add_dependency_list<P: AsRef<Path>>(
        &mut self,
        schema_path: P,
    ) -> Result<(), Box<dyn Error>> {
        if self.dependency_list_stream.is_some() {
            return Ok(());
        }
        let layout = TagLayout::from_json(schema_path)?;
        self.dependency_list_stream = Some(TagStream::new_default(layout));
        Ok(())
    }

    /// Drop the `want` stream if present.
    pub fn remove_dependency_list(&mut self) {
        self.dependency_list_stream = None;
    }

    /// Attach an empty `info` (import info) stream with one
    /// zero-filled root element. No-op if one is already present.
    /// `schema_path` is the per-group JSON for
    /// `tag_import_information`.
    pub fn add_import_info<P: AsRef<Path>>(
        &mut self,
        schema_path: P,
    ) -> Result<(), Box<dyn Error>> {
        if self.import_info_stream.is_some() {
            return Ok(());
        }
        let layout = TagLayout::from_json(schema_path)?;
        self.import_info_stream = Some(TagStream::new_default(layout));
        Ok(())
    }

    /// Drop the `info` stream if present.
    pub fn remove_import_info(&mut self) {
        self.import_info_stream = None;
    }

    /// Attach an empty `assd` (asset depot storage) stream — tag-
    /// editor icon pixel data. One zero-filled root element; icon
    /// data field empty. Callers populate via the facade if needed.
    /// No-op if already present.
    pub fn add_asset_depot_storage<P: AsRef<Path>>(
        &mut self,
        schema_path: P,
    ) -> Result<(), Box<dyn Error>> {
        if self.asset_depot_storage_stream.is_some() {
            return Ok(());
        }
        let layout = TagLayout::from_json(schema_path)?;
        self.asset_depot_storage_stream = Some(TagStream::new_default(layout));
        Ok(())
    }

    /// Drop the `assd` stream if present.
    pub fn remove_asset_depot_storage(&mut self) {
        self.asset_depot_storage_stream = None;
    }

    /// Placeholder for the engine's file-header checksum. Currently a
    /// no-op — writes out whatever value is already in
    /// [`Self::header`]`.checksum` (zero for freshly-created tags).
    ///
    /// We've confirmed from reverse-engineering Reach's Xbox 360
    /// debug build that the algorithm is a reflected CRC32 (poly
    /// `0xEDB88320`, seed `0xFFFFFFFF`, **no final XOR**), captured
    /// during serialization of the `tag!` stream at
    /// `c_single_tag_file_writer::commit_stream`. The primitives
    /// live in `crc_new` / `crc_checksum_buffer` and the wrapper is
    /// `c_checksumed_unbounded_relative_persist_stream::write`.
    ///
    /// What we haven't nailed down is *which bytes* flow through
    /// that stream. Brute-forcing every contiguous file-range span
    /// (whole tag!, blay only, bdat only, payloads vs. headers, the
    /// whole file) against real tags produces zero matches — so the
    /// checksum isn't a raw hash of any single byte span of the
    /// on-disk file. Most likely explanations:
    /// - The engine walks the runtime struct tree and feeds field-
    ///   by-field bytes in a memory-traversal order that differs
    ///   from on-disk serialization order.
    /// - MCC's re-implemented tag-file writer diverged from the
    ///   Xbox 360 build's algorithm.
    ///
    /// BCS itself leaves this field at 0 with a `#TODO` (see
    /// `mandrilllib/filesystem/high_level_tag_file_writer.cpp`).
    /// Freshly-created tags from this library match that behavior.
    /// If the engine ever rejects a zeroed checksum on load, come
    /// back and finish reverse-engineering the tree walk.
    pub fn recompute_checksum(&mut self) {
        // Intentional no-op. See method docs.
    }

    /// Rebuild the `want` stream's dependency list from this tag's
    /// own data. Walks the `tag!` root recursively, collects every
    /// non-null `tag_reference` (duplicates preserved, in encounter
    /// order), filters out `impo`-group references, and writes one
    /// dependency entry per remaining ref (flags=0). Creates the
    /// stream first via `schema_path` if missing.
    ///
    /// Matches 98.8% of observed tag bytes exactly; the rest are
    /// covered by the `impo` filter. See `examples/want_vs_deps.rs`
    /// for the corpus correlation study.
    pub fn rebuild_dependency_list<P: AsRef<Path>>(
        &mut self,
        schema_path: P,
    ) -> Result<(), Box<dyn Error>> {
        // 1. Collect every non-null tag_reference in the tag's data.
        let impo = u32::from_be_bytes(*b"impo");
        let mut refs: Vec<(u32, String)> = Vec::new();
        collect_tag_references(&self.root(), &mut refs);
        refs.retain(|(g, _)| *g != impo);

        // 2. Ensure the stream exists.
        if self.dependency_list_stream.is_none() {
            self.add_dependency_list(schema_path)?;
        }

        // 3. Populate its dependencies block.
        let mut root = self
            .dependency_list_mut()
            .ok_or("dependency_list_stream missing after add")?;
        let mut dep_field = root
            .field_path_mut("dependencies")
            .ok_or("want root missing `dependencies` field")?;
        let mut deps = dep_field
            .as_block_mut()
            .ok_or("`dependencies` is not a block")?;
        deps.clear();
        for (g, p) in refs {
            let i = deps.add_element();
            let mut elem = deps
                .element_mut(i)
                .expect("newly added element should be accessible");
            let mut df = elem
                .field_path_mut("dependency")
                .ok_or("dependency element missing `dependency` field")?;
            df.set(crate::TagFieldData::TagReference(crate::TagReferenceData {
                group_tag_and_name: Some((g, p)),
            }))
            .map_err(|e| format!("set tag_reference failed: {e:?}"))?;
        }
        Ok(())
    }
}

//
// Private walker — collects every non-null tag_reference in a tag's
// data tree. Recurses into struct / block / array fields. Resources
// are intentionally not walked; corpus correlation showed 98.8%
// exact match without them (the rest accounted for by the `impo`
// filter applied by the caller).
//
fn collect_tag_references(st: &crate::TagStruct<'_>, out: &mut Vec<(u32, String)>) {
    for f in st.fields() {
        collect_from_field(&f, out);
    }
}

fn collect_from_field(f: &crate::TagField<'_>, out: &mut Vec<(u32, String)>) {
    if let Some(nested) = f.as_struct() {
        collect_tag_references(&nested, out);
        return;
    }
    if let Some(block) = f.as_block() {
        for elem in block.iter() {
            collect_tag_references(&elem, out);
        }
        return;
    }
    if let Some(arr) = f.as_array() {
        for elem in arr.iter() {
            collect_tag_references(&elem, out);
        }
        return;
    }
    if let Some(crate::TagFieldData::TagReference(r)) = f.value()
        && let Some((g, p)) = r.group_tag_and_name {
            out.push((g, p));
    }
}
