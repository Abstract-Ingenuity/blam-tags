use blam_tags::render_method::RenderMethodTemplate;
use blam_tags::TagFile;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let tag = TagFile::read(&path).unwrap();
    let t = RenderMethodTemplate::from_tag(&tag).unwrap();
    println!("vertex_shader: {}", t.vertex_shader_path);
    println!("pixel_shader:  {}", t.pixel_shader_path);
    println!("entry_points:  {}", t.entry_points.len());
    println!("passes:        {}", t.passes.len());
    println!("routing_info:  {}", t.routing_info.len());
    println!("float_consts:  {}  e.g. {:?}", t.float_constants.len(), &t.float_constants[..t.float_constants.len().min(5)]);
    println!("textures:      {}  e.g. {:?}", t.textures.len(), &t.textures[..t.textures.len().min(5)]);
    if let Some(p0) = t.passes.first() {
        println!("\npass[0].pixel_real_constants: start={} count={}", p0.pixel_real_constants.start(), p0.pixel_real_constants.count());
    }
    println!("\nfirst 8 routing entries:");
    for (i, r) in t.routing_info.iter().take(8).enumerate() {
        println!("  [{i}] dest={} src={}", r.destination_index, r.source_index);
    }
}
