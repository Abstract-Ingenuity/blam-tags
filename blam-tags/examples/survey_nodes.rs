use blam_tags::render_model::RenderModel;
use blam_tags::TagFile;

type M3 = [[f32; 3]; 3];
fn mul3(a: M3, b: M3) -> M3 {
    let mut o = [[0f32; 3]; 3];
    for i in 0..3 { for j in 0..3 { for k in 0..3 { o[i][j] += a[i][k] * b[k][j]; } } }
    o
}
// quaternion -> column-convention rotation (R*v)
fn quat_to_m(i: f32, j: f32, k: f32, w: f32) -> M3 {
    [
        [1.0 - 2.0 * (j * j + k * k), 2.0 * (i * j - k * w), 2.0 * (i * k + j * w)],
        [2.0 * (i * j + k * w), 1.0 - 2.0 * (i * i + k * k), 2.0 * (j * k - i * w)],
        [2.0 * (i * k - j * w), 2.0 * (j * k + i * w), 1.0 - 2.0 * (i * i + j * j)],
    ]
}
// columns = fwd,left,up (engine matrix4x3 rows -> glam-col equivalent), scaled
fn cols(f: [f32; 3], l: [f32; 3], u: [f32; 3], s: f32) -> M3 {
    [[f[0]*s, l[0]*s, u[0]*s], [f[1]*s, l[1]*s, u[1]*s], [f[2]*s, l[2]*s, u[2]*s]]
}
fn near_ident(m: M3) -> f32 {
    let id = [[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]];
    let mut d = 0f32;
    for i in 0..3 { for j in 0..3 { d += (m[i][j]-id[i][j]).abs(); } }
    d
}
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let tag = TagFile::read(&path).unwrap();
    let m = RenderModel::from_tag(&tag).unwrap();
    println!("{}  nodes={}", path.rsplit('/').next().unwrap(), m.nodes.len());
    for (idx, n) in m.nodes.iter().enumerate() {
        if n.parent_node >= 0 { continue; } // root nodes only (no compose)
        let rw = quat_to_m(n.default_rotation.i, n.default_rotation.j, n.default_rotation.k, n.default_rotation.w);
        let rinv = cols(
            [n.inverse_forward.i, n.inverse_forward.j, n.inverse_forward.k],
            [n.inverse_left.i, n.inverse_left.j, n.inverse_left.k],
            [n.inverse_up.i, n.inverse_up.j, n.inverse_up.k],
            n.inverse_scale,
        );
        let skin = mul3(rw, rinv);
        let d = near_ident(skin);
        println!("  root node[{idx}] {:<14} |node_world*inv_bind - I|={:.3} {}  (inv_scale={:.2})",
            n.name, d, if d < 0.05 { "= IDENTITY" } else { "= node_world (applies transform)" }, n.inverse_scale);
        println!("    skin row0=({:.2},{:.2},{:.2})", skin[0][0], skin[0][1], skin[0][2]);
    }
}
