use blam_tags::TagFile;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap();
    let tag = TagFile::read(&path)?;
    let pmt = tag.root().field_path("render geometry/per mesh temporary").unwrap()
        .as_block().unwrap();
    let mesh0 = pmt.element(0).unwrap();
    let raw_v = mesh0.field("raw vertices").unwrap().as_block().unwrap();
    let v = raw_v.element(0).unwrap();
    println!("vertex 0 fields:");
    for f in v.fields() {
        println!("  {} : {} = {:?}", f.name(), f.type_name(), f.value());
    }
    println!();
    println!("node indices array:");
    if let Some(arr) = v.field("node indices").and_then(|f| f.as_array()) {
        println!("  len={}", arr.len());
        for k in 0..arr.len() {
            let e = arr.element(k).unwrap();
            for fld in e.fields() {
                println!("  [{k}] {} : {} = {:?}", fld.name(), fld.type_name(), fld.value());
            }
        }
    } else {
        println!("  could not get array");
    }
    println!();
    println!("node weights array:");
    if let Some(arr) = v.field("node weights").and_then(|f| f.as_array()) {
        println!("  len={}", arr.len());
        for k in 0..arr.len() {
            let e = arr.element(k).unwrap();
            for fld in e.fields() {
                println!("  [{k}] {} : {} = {:?}", fld.name(), fld.type_name(), fld.value());
            }
        }
    }
    Ok(())
}
