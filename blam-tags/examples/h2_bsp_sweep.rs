use std::path::PathBuf;
use blam_tags::classic::{read_classic_tag_file, ClassicHeader};
use blam_tags::{AssFile, AssObjectPayload};
use blam_tags::layout::TagLayout;
fn main(){
    let a:Vec<String>=std::env::args().collect();
    let defs=PathBuf::from(&a[1]); let root=&a[2];
    let (mut tot,mut ok,mut empty,mut err)=(0,0,0,0);
    let (mut v,mut t,mut inst,mut sph)=(0u64,0u64,0u64,0u64);
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
            match AssFile::from_scenario_structure_bsp_h2(&tag){
                Ok(ass)=>{
                    let vv:usize=ass.objects.iter().map(|o|o.vertices_len()).sum();
                    let tt:usize=ass.objects.iter().map(|o|o.triangles_len()).sum();
                    if tt==0 {empty+=1} else {ok+=1}
                    v+=vv as u64; t+=tt as u64; inst+=ass.instances.len() as u64;
                    sph+=ass.objects.iter().filter(|o|matches!(o.payload,AssObjectPayload::Sphere{..})).count() as u64;
                }
                Err(e)=>{err+=1; if samples.len()<8{samples.push(format!("{} :: {e}",p.file_name().unwrap().to_string_lossy()))}}
            }
        }
    }
    println!("=== {tot} sbsp | {ok} ok | {empty} empty | {err} err ===");
    println!("    {v} verts {t} tris {inst} instances {sph} markers");
    for s in &samples{println!("  {s}")}
}
