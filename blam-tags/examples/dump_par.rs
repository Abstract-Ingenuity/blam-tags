use std::path::PathBuf;
use blam_tags::TagFile;
use blam_tags::scenario::Scenario;
fn main(){
  let p=std::env::args().nth(1).unwrap();
  let tag=TagFile::read(PathBuf::from(&p)).unwrap();
  let sc=Scenario::from_tag(&tag).unwrap();
  for (i,o) in sc.scenery.iter().enumerate(){
    if let Some(par)=&o.object_data.parent_id {
      let pal = sc.scenery_palette.get(o.palette_index as usize).map(|p| p.tag_path.clone()).unwrap_or_default();
      println!("scen[{i}] pal={} ({}) parent_name_idx={} parent_marker='{}' conn='{}'",
        o.palette_index, pal, par.parent_object_name_index, par.parent_marker, par.connection_marker);
    }
  }
  println!("--- object_names ---");
  for (i,n) in sc.object_names.iter().enumerate(){ println!("  name[{i}]={}", n.name); }
}
