use blam_tags::TagFile;
fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "/Users/camden/Halo/halo3_mcc/tags/fx/particles/models/cinematics/sentinel/sentinel.particle_model".into()
    });
    let t = TagFile::read(&path).unwrap();
    let root = t.root();
    println!("ROOT fields:");
    for f in root.fields() {
        println!("  type={:?}  name={:?}", f.field_type(), f.name());
    }
    if let Some(rg) = root.field("render geometry").and_then(|f| f.as_struct()) {
        println!("\nrender geometry:");
        for f in rg.fields() {
            println!("  type={:?}  name={:?}", f.field_type(), f.name());
        }
    }
    if let Some(gpu) = root.field("m_gpu_data").and_then(|f| f.as_struct()) {
        println!("\nm_gpu_data:");
        for f in gpu.fields() {
            println!("  type={:?}  name={:?}  value={:?}", f.field_type(), f.name(), f.value());
        }
        if let Some(vb) = gpu.field("m_variants").and_then(|f| f.as_block()) {
            println!("  m_variants[{}]:", vb.len());
            for i in 0..vb.len().min(4) {
                if let Some(v) = vb.element(i) {
                    for f in v.fields() {
                        println!("    variant[{i}].{} = {:?}", f.name(), f.value());
                    }
                }
            }
        }
    }
}
