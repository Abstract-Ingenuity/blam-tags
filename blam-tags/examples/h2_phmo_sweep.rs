use std::path::PathBuf;
use blam_tags::classic::{read_classic_tag_file, ClassicHeader};
use blam_tags::JmsFile;
use blam_tags::layout::TagLayout;
fn main(){
    let a:Vec<String>=std::env::args().collect();
    let defs=PathBuf::from(&a[1]); let root=&a[2];
    let (mut tot,mut ok,mut empty,mut err)=(0,0,0,0);
    let (mut sp,mut bx,mut cp,mut cv,mut rg,mut hg,mut unp)=(0u64,0u64,0u64,0u64,0u64,0u64,0u64);
    let mut samples=Vec::new();
    let mut stack=vec![PathBuf::from(root)];
    while let Some(d)=stack.pop(){
        let Ok(rd)=std::fs::read_dir(&d) else{continue};
        for e in rd.flatten(){ let p=e.path();
            if p.is_dir(){stack.push(p);continue}
            if !p.to_string_lossy().ends_with(".physics_model"){continue}
            let Ok(bytes)=std::fs::read(&p) else{continue};
            if ClassicHeader::parse(&bytes).is_none(){continue}
            let Ok(layout)=TagLayout::from_json(defs.join("physics_model.json")) else{continue};
            let Ok(tag)=read_classic_tag_file(&bytes,layout) else{err+=1;tot+=1;continue};
            tot+=1;
            match JmsFile::from_physics_model_h2(&tag){
                Ok(j)=>{
                    let shapes=j.spheres.len()+j.boxes.len()+j.capsules.len()+j.convex_shapes.len();
                    if shapes==0 {empty+=1} else {ok+=1}
                    sp+=j.spheres.len() as u64; bx+=j.boxes.len() as u64; cp+=j.capsules.len() as u64;
                    cv+=j.convex_shapes.len() as u64; rg+=j.ragdolls.len() as u64; hg+=j.hinges.len() as u64;
                    // count shapes that failed to parent (parent == -1)
                    unp += j.spheres.iter().filter(|s|s.parent<0).count() as u64;
                    unp += j.boxes.iter().filter(|s|s.parent<0).count() as u64;
                    unp += j.capsules.iter().filter(|s|s.parent<0).count() as u64;
                    unp += j.convex_shapes.iter().filter(|s|s.parent<0).count() as u64;
                }
                Err(e)=>{err+=1; if samples.len()<8{samples.push(format!("{} :: {e}",p.file_name().unwrap().to_string_lossy()))}}
            }
        }
    }
    println!("=== {tot} phmo | {ok} ok | {empty} empty | {err} err ===");
    println!("    spheres={sp} boxes={bx} pills={cp} convex={cv} ragdolls={rg} hinges={hg} | unparented shapes={unp}");
    for s in &samples{println!("  {s}")}
}
