//! Tag file: the outermost container. A tag file is a fixed-width
//! 64-byte [`TagFileHeader`] followed by a mandatory `tag!` stream and
//! two optional streams (`want` dependency list, `info` import info).

use std::error::Error;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::io::*;
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
    /// Read the fixed 64-byte header. Asserts the `BLAM` signature
    /// before returning.
    pub fn read<R: Seek + Read>(reader: &mut std::io::BufReader<R>) -> Result<Self, Box<dyn Error>> {
        let pad = read_u8_array(reader)?;
        let build_version = read_u32_le(reader)? as i32;
        let build_number = read_u32_le(reader)? as i32;
        let version = read_u32_le(reader)?;
        let group_tag = read_u32_le(reader)?;
        let group_version = read_u32_le(reader)?;
        let checksum = read_u32_le(reader)?;
        let signature = read_u32_le(reader)?;
        assert!(signature == u32::from_be_bytes(*b"BLAM"));

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
/// fixed order: `dependency_list_stream` (`want`), then
/// `import_info_stream` (`info`). Missing optional streams simply end
/// the file.
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
}

impl TagFile {
    /// Open `path` and parse a complete tag file. The read asserts that
    /// the file ends exactly at the last consumed stream, so trailing
    /// garbage will panic rather than be silently dropped.
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let mut reader = std::io::BufReader::with_capacity(64 * 1024, std::fs::File::open(path)?);

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

        // Check if there are any 'want' or 'info' chunks
        while reader.stream_position()? != tag_file_size {
            let chunk_header_offset = reader.stream_position()?;
            let chunk_signature = read_u32_le(&mut reader)?;
            reader.seek(SeekFrom::Start(chunk_header_offset))?;

            match &chunk_signature.to_be_bytes() {
                b"want" => {
                    assert!(dependency_list_stream.is_none());
                    dependency_list_stream = Some(TagStream::read(chunk_signature, &mut reader)?);
                }

                b"info" => {
                    assert!(import_info_stream.is_none());
                    import_info_stream = Some(TagStream::read(chunk_signature, &mut reader)?);
                }

                _ => panic!("unhandled chunk signature: '{}' at 0x{:X}", str::from_utf8(&chunk_signature.to_be_bytes()).unwrap(), chunk_header_offset),
            }
        }

        assert!(reader.stream_position()? == tag_file_size);

        Ok(Self {
            header,
            tag_stream,
            dependency_list_stream,
            import_info_stream,
        })
    }

    /// Write this tag file to `path`. Mirrors `TagFile::read`: file header,
    /// then `tag!` stream, then optional `want` and `info` streams in that
    /// order. IMPORTANT: never write to the source tag path — write to a
    /// temp file, then read it back for byte-exact roundtrip verification.
    pub fn write<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);

        self.header.write(&mut writer)?;
        self.tag_stream.write(u32::from_be_bytes(*b"tag!"), &mut writer)?;

        if let Some(dependency_list_stream) = &self.dependency_list_stream {
            dependency_list_stream.write(u32::from_be_bytes(*b"want"), &mut writer)?;
        }
        if let Some(import_info_stream) = &self.import_info_stream {
            import_info_stream.write(u32::from_be_bytes(*b"info"), &mut writer)?;
        }

        Ok(())
    }
}
