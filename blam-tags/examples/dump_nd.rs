use std::path::PathBuf;
use blam_tags::TagFile;
use blam_tags::render_model::RenderModel;
fn main(){
  let p=std::env::args().nth(1).unwrap();
  let tag=TagFile::read(PathBuf::from(&p)).unwrap();
  let rm=RenderModel::from_tag(&tag).unwrap();
  println!("nodes[{}]:", rm.nodes.len());
  for (i,n) in rm.nodes.iter().enumerate(){
    let r=n.default_rotation; let t=n.default_translation;
    let (x,y,z,w)=(r.i,r.j,r.k,r.w);
    let fx=1.0-2.0*(y*y+z*z); let fy=2.0*(x*y+z*w); let fz=2.0*(x*z-y*w);
    let ux=2.0*(x*z+y*w); let uy=2.0*(y*z-x*w); let uz=1.0-2.0*(x*x+y*y);
    println!("  [{i}] '{}' parent={} pos=({:.3},{:.3},{:.3}) quat=({:.3},{:.3},{:.3},{:.3}) fwd=({:.2},{:.2},{:.2}) up=({:.2},{:.2},{:.2})",
      n.name,n.parent_node,t.x,t.y,t.z,x,y,z,w,fx,fy,fz,ux,uy,uz);
  }
}
