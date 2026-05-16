//! Debug probe: print one tag's cache block + xsync state.

use std::error::Error;

use blam_tags::monolithic::{MonolithicCache, XSyncState};

fn main() -> Result<(), Box<dyn Error>> {
    let cache_dir = std::env::args().nth(1).ok_or("usage: <cache_dir> <group> <name>")?;
    let group_arg = std::env::args().nth(2).ok_or("missing group arg")?;
    let name = std::env::args().nth(3).ok_or("missing name arg")?;

    let cache = MonolithicCache::open(&cache_dir)?;
    let group_tag = blam_tags::parse_group_tag(&group_arg).ok_or("bad group")?;

    let entry = cache.find_tag(group_tag, &name).ok_or("tag not found")?;
    println!("tag: {}:{}", group_arg, entry.name);

    let tag_block = cache.resolve_tag_block(entry);
    let cash_block = cache.resolve_cache_block(entry);
    println!("tag_block:   {:?}", tag_block);
    println!("cache_block: {:?}", cash_block);

    // Read the tag fresh (un-hydrated) so we can see the original
    // xsync state bytes. Use read_tag_bytes which gives us the raw
    // tag stream.
    let bytes = cache.read_tag_bytes(entry)?;
    println!("tag bytes: {} (size of tag stream from tags_N)", bytes.len());

    // Search the tag bytes for tgxc signatures and parse each.
    let mut i = 0;
    while i + 12 <= bytes.len() {
        if &bytes[i..i + 4] == b"tgxc" {
            let version = u32::from_be_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]);
            let size = u32::from_be_bytes([bytes[i + 8], bytes[i + 9], bytes[i + 10], bytes[i + 11]]) as usize;
            println!("\ntgxc at offset 0x{i:x}: version={version}, size={size}");
            if i + 12 + size <= bytes.len() {
                let payload = &bytes[i + 12..i + 12 + size];
                println!("  first 64 bytes hex:");
                for chunk in payload[..64.min(payload.len())].chunks(16) {
                    print!("    ");
                    for b in chunk { print!("{:02x} ", b); }
                    println!();
                }
                match XSyncState::parse(payload, version) {
                    Ok(state) => {
                        println!("  cache_location_offset: 0x{:x} = {}",
                            state.header.cache_location_offset, state.header.cache_location_offset);
                        println!("  cache_location_size:   0x{:x} = {}",
                            state.header.cache_location_size, state.header.cache_location_size);
                        println!("  optional_offset:       0x{:x} = {}",
                            state.header.optional_location_offset, state.header.optional_location_offset);
                        println!("  optional_size:         0x{:x} = {}",
                            state.header.optional_location_size, state.header.optional_location_size);
                        println!("  control_data_size:     {}", state.header.control_data_size);
                        println!("  control_fixup_count:   {}", state.header.control_fixup_count);
                        println!("  interop_usage_count:   {}", state.header.interop_usage_count);
                        println!("  root_address:          0x{:08x}", state.header.root_address);
                        println!("  --- parsed body ---");
                        println!("  control_data:          {} bytes", state.control_data.len());
                        println!("  control_fixups:        {} entries", state.control_fixups.len());
                        for (i, fx) in state.control_fixups.iter().enumerate() {
                            println!("    [{i}] at 0x{:04x}: tier={:?} offset=0x{:x}",
                                fx.block_offset, fx.address.tier(), fx.address.offset());
                        }
                        println!("  pageable_fixups:       {} entries", state.pageable_fixups.len());
                        println!("  optional_fixups:       {} entries", state.optional_fixups.len());
                        println!("  interop_guids:         {} entries", state.interop_guids.len());
                        if !state.control_data.is_empty() {
                            let fixed = state.apply_control_fixups();
                            println!("  control_data (fixed-up), {} bytes:", fixed.len());
                            for (j, chunk) in fixed.chunks(16).enumerate() {
                                print!("    {:04x}  ", j * 16);
                                for b in chunk { print!("{:02x} ", b); }
                                println!();
                            }
                        }
                    }
                    Err(e) => println!("  parse error: {e:?}"),
                }
            }
            i += 12 + size;
        } else {
            i += 1;
        }
    }

    Ok(())
}
