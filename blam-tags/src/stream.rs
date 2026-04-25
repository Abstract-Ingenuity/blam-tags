//! Top-level tag stream: the `tag!` / `want` / `info` chunks that sit
//! directly under the tag file header. Each stream wraps a `blay` block
//! layout and a `bdat` root block data chunk — same structure, different
//! content (main data, dependency list, import info).

use std::io::{Read, Seek, Write};

use crate::data::TagBlockData;
use crate::error::TagReadError;
use crate::io::*;
use crate::layout::TagLayout;

/// One of the three top-level chunks in a tag file: `tag!` (main
/// payload), `want` (dependency list), or `info` (import info). All three
/// share the same structure: a `blay` block layout followed by a `bdat`
/// block data. The chunk signature is passed in by the caller since it
/// determines *which* stream is being read, not *how* it's shaped.
#[derive(Debug)]
pub(crate) struct TagStream {
    /// The `blay` chunk — the schema (structs, blocks, fields, types)
    /// used to interpret `data`.
    pub(crate) layout: TagLayout,
    /// The `bdat` chunk — the root block whose elements are the actual
    /// tag values, shaped by `layout`.
    pub(crate) data: TagBlockData,
}

impl TagStream {
    /// Read a stream chunk. Caller supplies the expected outer signature
    /// (`b"tag!"`, `b"want"`, `b"info"`, or `b"assd"`); the function
    /// validates it and parses the `blay` + `bdat` body.
    pub(crate) fn read<R: Seek + Read>(
        chunk_signature: u32,
        reader: &mut std::io::BufReader<R>,
    ) -> Result<Self, TagReadError> {
        // Outer chunk: signature is dynamic (one of tag!/want/info/
        // assd), so we can't use the validating helper — inline the
        // signature/version checks. Stream-level error context uses
        // the static name "tag stream" since we can't materialise the
        // dynamic name as a `&'static str`.
        let chunk_header_offset = reader.stream_position()?;
        let chunk_header = read_chunk_header(reader)?;
        if chunk_header.signature != chunk_signature {
            return Err(TagReadError::BadChunkSignature {
                offset: chunk_header_offset,
                expected: chunk_signature.to_be_bytes(),
                got: chunk_header.signature.to_be_bytes(),
            });
        }
        if chunk_header.version != 0 {
            return Err(TagReadError::BadChunkVersion {
                chunk: "tag stream",
                version: chunk_header.version,
            });
        }
        let chunk_offset = reader.stream_position()?;

        //
        // Now we're inside the chunk, read the 'blay' chunk
        //

        let layout = TagLayout::read(reader)?;
        let root_block_layout = &layout.block_layouts[layout.header.tag_group_block_index as usize];

        //
        // Read the 'bdat' chunk — version is 1 (not 0), so we use
        // the unvalidated header reader and check the version
        // ourselves.
        //

        let bdat_offset = reader.stream_position()?;
        let block_data_header = read_chunk_header(reader)?;
        if block_data_header.signature != u32::from_be_bytes(*b"bdat") {
            return Err(TagReadError::BadChunkSignature {
                offset: bdat_offset,
                expected: *b"bdat",
                got: block_data_header.signature.to_be_bytes(),
            });
        }
        if block_data_header.version != 1 {
            return Err(TagReadError::BadChunkVersion {
                chunk: "bdat",
                version: block_data_header.version,
            });
        }
        let block_data_offset = reader.stream_position()?;

        let tag_block_data = TagBlockData::read(&layout, root_block_layout, reader)?;

        check_chunk_end(reader, "bdat", block_data_offset, block_data_header.size)?;
        check_chunk_end(reader, "tag stream", chunk_offset, chunk_header.size)?;

        Ok(Self {
            layout,
            data: tag_block_data,
        })
    }

    /// Build a new stream containing the given layout and a root
    /// block with exactly one zero-filled default element. Used by
    /// `TagFile::new` when creating a tag from a schema.
    pub(crate) fn new_default(layout: TagLayout) -> Self {
        let root_block_index = layout.header.tag_group_block_index;
        let data = TagBlockData::new_root_default(&layout, root_block_index);
        Self { layout, data }
    }

    /// Write this stream as a `tag!` / `want` / `info` chunk. The payload is
    /// a `blay` chunk (block layout) followed by a `bdat` chunk (block
    /// data). Both the outer stream chunk and `blay` have version 0;
    /// `bdat` has version 1 (hypothesis-verified on read).
    pub(crate) fn write<W: Write>(
        &self,
        chunk_signature: u32,
        writer: &mut W,
    ) -> std::io::Result<()> {
        let mut stream_body = Vec::new();

        // blay chunk
        self.layout.write(&mut stream_body)?;

        // bdat chunk — wraps the root TagBlockData chunk (tgbl).
        let mut bdat_body = Vec::new();
        self.data.write(&self.layout, &mut bdat_body)?;
        write_tag_chunk_content(&mut stream_body, u32::from_be_bytes(*b"bdat"), 1, &bdat_body)?;

        write_tag_chunk_content(writer, chunk_signature, 0, &stream_body)?;
        Ok(())
    }
}
