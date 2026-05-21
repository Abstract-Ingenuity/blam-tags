//! Dump a particle_model (pmdf) tag.

use blam_tags::file::TagFile;
use blam_tags::particle_model::ParticleModel;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_particle_model <path>");
    let tag = TagFile::read(&path).expect("read");
    let pm = ParticleModel::from_tag(&tag).expect("walk");
    println!("=== {} ===", path);
    println!("variant_count:               {}", pm.variant_count);
    println!("mesh_count:                  {}", pm.mesh_count);
    println!("render_geometry runtime flags: 0x{:08x}", pm.render_geometry_runtime_flags);
}
