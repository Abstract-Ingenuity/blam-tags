use std::path::PathBuf;
use blam_tags::classic::read_classic_tag_file;
use blam_tags::{AssFile, AssObjectPayload};
use blam_tags::layout::TagLayout;
fn main(){
    let a:Vec<String>=std::env::args().collect();
    let bytes=std::fs::read(&a[2]).unwrap();
    let layout=TagLayout::from_json(PathBuf::from(&a[1]).join("scenario_structure_bsp.json")).unwrap();
    let tag=read_classic_tag_file(&bytes,layout).unwrap();
    let ass=AssFile::from_scenario_structure_bsp_h2(&tag).unwrap();
    let v:usize=ass.objects.iter().map(|o|o.vertices_len()).sum();
    let t:usize=ass.objects.iter().map(|o|o.triangles_len()).sum();
    let lights=ass.objects.iter().filter(|o|matches!(o.payload,AssObjectPayload::GenericLight(_))).count();
    let spheres=ass.objects.iter().filter(|o|matches!(o.payload,AssObjectPayload::Sphere{..})).count();
    println!("{} mats, {} objects ({} spheres, {} lights), {} instances, {} verts, {} tris",
        ass.materials.len(), ass.objects.len(), spheres, lights, ass.instances.len(), v, t);
}
