use blam_tags::effect::EffectDefinition;
use blam_tags::file::TagFile;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let tag = TagFile::read(&path).unwrap();
    let eff = EffectDefinition::from_tag(&tag).unwrap();
    println!("=== {} ===", path);
    for ev in &eff.events {
        for ps in &ev.particle_systems {
            for em in &ps.emitters {
                let rd = em.relative_direction.starting_interpolant;
                let to = em.translational_offset.starting_interpolant;
                println!("emitter {:?}: rel_dir={:?} trans_off={:?}", em.name, rd, to);
            }
        }
    }
}
