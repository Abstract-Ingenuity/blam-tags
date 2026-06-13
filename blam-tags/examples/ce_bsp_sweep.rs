use std::path::PathBuf;
use blam_tags::classic::{read_classic_tag_file, ClassicHeader};
use blam_tags::jms::JmsFile;
use blam_tags::layout::TagLayout;
fn main(){
    let a:Vec<String>=std::env::args().collect();
    let defs=PathBuf::from(&a[1]); let root=&a[2];
    let (mut tot,mut rok,mut cok,mut empty,mut err)=(0,0,0,0,0);
    let (mut rv,mut rt,mut cv,mut ct)=(0u64,0u64,0u64,0u64);
    let mut samples=Vec::new();
    let mut stack=vec![PathBuf::from(root)];
    while let Some(d)=stack.pop(){
        let Ok(rd)=std::fs::read_dir(&d) else{continue};
        for e in rd.flatten(){ let p=e.path();
            if p.is_dir(){stack.push(p);continue}
            if !p.to_string_lossy().ends_with(".scenario_structure_bsp"){continue}
            let Ok(bytes)=std::fs::read(&p) else{continue};
            if ClassicHeader::parse(&bytes).is_none(){continue}
            let Ok(layout)=TagLayout::from_json(defs.join("scenario_structure_bsp.json")) else{continue};
            let Ok(tag)=read_classic_tag_file(&bytes,layout) else{err+=1;tot+=1;continue};
            tot+=1;
            match JmsFile::from_scenario_structure_bsp_ce(&tag){
                Ok(j) if j.triangles.is_empty()=>empty+=1,
                Ok(j)=>{rok+=1; rv+=j.vertices.len() as u64; rt+=j.triangles.len() as u64;}
                Err(e)=>{err+=1; if samples.len()<8{samples.push(format!("{} R:: {e}",p.file_name().unwrap().to_string_lossy()))}}
            }
            match JmsFile::from_scenario_structure_bsp_ce_collision(&tag){
                Ok(j)=>{cok+=1; cv+=j.vertices.len() as u64; ct+=j.triangles.len() as u64;}
                Err(e)=>{if samples.len()<8{samples.push(format!("{} C:: {e}",p.file_name().unwrap().to_string_lossy()))}}
            }
        }
    }
    println!("=== {tot} sbsp | render {rok} ok ({empty} empty) | coll {cok} ok | {err} err ===");
    println!("    render {rv} verts {rt} tris | coll {cv} verts {ct} tris");
    for s in &samples{println!("  {s}")}
}
