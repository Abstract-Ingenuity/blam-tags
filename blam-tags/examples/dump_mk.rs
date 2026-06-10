use std::path::PathBuf;
use blam_tags::TagFile;
use blam_tags::render_model::RenderModel;
fn main(){
  for p in std::env::args().skip(1){
    let path=PathBuf::from(&p);
    let Ok(tag)=TagFile::read(&path) else { println!("{p}: read fail"); continue };
    let Ok(rm)=RenderModel::from_tag(&tag) else { println!("{p}: walk fail"); continue };
    println!("==== {p} ====");
    for g in &rm.marker_groups {
      for m in &g.markers {
        let r=m.rotation; let t=m.translation;
        let (i,j,k,w)=(r.i,r.j,r.k,r.w);
        // forward = q*+X, up = q*+Z
        let fx=1.0-2.0*(j*j+k*k); let fy=2.0*(i*j+k*w); let fz=2.0*(i*k-j*w);
        let ux=2.0*(i*k+j*w); let uy=2.0*(j*k-i*w); let uz=1.0-2.0*(i*i+j*j);
        println!("  '{}' node={} pos=({:.3},{:.3},{:.3}) quat=({:.3},{:.3},{:.3},{:.3}) fwd=({:.3},{:.3},{:.3}) up=({:.3},{:.3},{:.3})",
          g.name,m.node_index,t.x,t.y,t.z,i,j,k,w,fx,fy,fz,ux,uy,uz);
      }
    }
  }
}
