//! Top-level tag stream: the `tag!` / `want` / `info` chunks that sit
//! directly under the tag file header. Each stream wraps a `blay` block
//! layout and a `bdat` root block data chunk — same structure, different
//! content (main data, dependency list, import info).

use std::error::Error;
use std::io::{Read, Seek, Write};

use crate::data::TagBlockData;
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
    /// (`b"tag!"`, `b"want"`, or `b"info"`); the function asserts it and
    /// parses the `blay` + `bdat` body.
    pub(crate) fn read<R: Seek + Read>(
        chunk_signature: u32,
        reader: &mut std::io::BufReader<R>,
    ) -> Result<Self, Box<dyn Error>> {
        // Read the chunk header
        let chunk_header = read_tag_chunk_header(reader)?;
        assert!(chunk_header.signature == chunk_signature);
        // HYPOTHESIS: outer tag! / want / info chunk version is always 0.
        assert_eq!(
            chunk_header.version, 0,
            "outer stream chunk version ({}) != 0",
            chunk_header.version,
        );
        let chunk_offset = reader.stream_position()?;

        //
        // Now we're inside the chunk, read the 'blay' chunk
        //

        let layout = TagLayout::read(reader)?;
        let root_block_layout = &layout.block_layouts[layout.header.tag_group_block_index as usize];
        // layout.display_block(layout.header.tag_group_block_index as usize, 0);

        //
        // Read the 'bdat' chunk
        //

        let block_data_header = read_tag_chunk_header(reader)?;
        assert!(block_data_header.signature == u32::from_be_bytes(*b"bdat"));
        // HYPOTHESIS: bdat version is always 1.
        assert_eq!(
            block_data_header.version, 1,
            "bdat version ({}) != 1", block_data_header.version,
        );
        let block_data_offset = reader.stream_position()?;

        let tag_block_data = TagBlockData::read(&layout, root_block_layout, reader)?;

        let end_offset = reader.stream_position()?;
        let expected_offset = block_data_offset + block_data_header.size as u64;
        if end_offset != expected_offset {
            panic!("failed to read 'bdat': started at 0x{block_data_offset:X}, ended at 0x{end_offset:X}, expected 0x{expected_offset:X}");
        }

        let end_offset = reader.stream_position()?;
        let expected_offset = chunk_offset + chunk_header.size as u64;
        if end_offset != expected_offset {
            panic!(
                "failed to read '{}': started at 0x{chunk_offset:X}, ended at 0x{end_offset:X}, expected 0x{expected_offset:X}",
                str::from_utf8(&chunk_signature.to_be_bytes()).unwrap(),
            );
        }

        Ok(Self {
            layout,
            data: tag_block_data,
        })
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
