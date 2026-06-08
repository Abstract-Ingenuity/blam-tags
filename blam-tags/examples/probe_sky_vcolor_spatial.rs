//! Bin construct skydome vert_color by vertex height (z) to see whether
//! the visible upper dome carries the bright-blue gradient or the dark part.
use blam_tags::{RenderModel, TagFile};

fn main() {
    let path = "/Users/camden/Halo/halo3_mcc/tags/levels/multi/construct/sky/sky.render_model";
    let tag = TagFile::read(path).expect("read");
    let meshes = RenderModel::derive_render_meshes(&tag).expect("derive");
    let m = &meshes[0]; // skydome
    // z range
    let (mut zmin, mut zmax) = (f32::MAX, f32::MIN);
    for v in &m.vertices {
        zmin = zmin.min(v.position.z);
        zmax = zmax.max(v.position.z);
    }
    println!("skydome verts={} z=[{:.1}, {:.1}]", m.vertices.len(), zmin, zmax);
    // 8 z-bins, avg vert_color per bin
    const N: usize = 8;
    let mut sum = [[0f64; 3]; N];
    let mut cnt = [0usize; N];
    for v in &m.vertices {
        let t = ((v.position.z - zmin) / (zmax - zmin)).clamp(0.0, 0.9999);
        let b = (t * N as f32) as usize;
        sum[b][0] += v.vert_color.i as f64;
        sum[b][1] += v.vert_color.j as f64;
        sum[b][2] += v.vert_color.k as f64;
        cnt[b] += 1;
    }
    println!("z-bin (low→high)   count   avg vert_color (r,g,b)");
    for i in 0..N {
        let c = cnt[i].max(1) as f64;
        let z0 = zmin + (zmax - zmin) * (i as f32 / N as f32);
        let z1 = zmin + (zmax - zmin) * ((i + 1) as f32 / N as f32);
        println!(
            "  [{:7.0}..{:7.0}]  {:5}   ({:.3}, {:.3}, {:.3})",
            z0, z1, cnt[i],
            sum[i][0] / c, sum[i][1] / c, sum[i][2] / c,
        );
    }
}
