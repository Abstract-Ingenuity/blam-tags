//! JMS (Bungie Joint Model Skeleton) export from `render_model` tags.
//!
//! Reconstructs a JMS-format static-geometry asset from a parsed
//! `render_model`. Targets the H3 / Reach MCC source-style tag
//! pipeline where every render mesh stores its vertex/index buffers
//! inline under `render geometry/per mesh temporary[i]` (no `tgrc`
//! resource indirection). Cache-built map files would need a
//! different code path — see `reference_tagtool_jms_extraction.md`
//! for the contrast.
//!
//! Two-stage decompression on positions and texcoords: raw values
//! live in `[0,1]` quantized space and must be linear-decompressed
//! against `render geometry/compression info[0]` *before* the
//! world-units → JMS-cm ×100 scale. The 6 position-bounds floats are
//! packed across two `real_point_3d` fields as the sequential tuple
//! `[xmin, xmax, ymin, ymax, zmin, zmax]` (NOT min/max corners).
//!
//! Materials are walked region × permutation × mesh.parts, mirroring
//! the H3 Blender exporter (`build_asset.py:write_materials_8205`):
//! one entry per `(shader, "{perm} {region}")` cell, with
//! `material_name` formatted as `(<slot>) <perm> <region>`. The
//! `slot` value is a deterministic 1-based counter; the artist's
//! original `(N)` is `bpy.data.materials.find()` from their Blender
//! scene and unrecoverable from the tag, but it's round-trip
//! metadata only and the H3 importer's parser doesn't act on it.
//!
//! Markers flatten `marker_groups[i].markers[j]` keeping every
//! variant — same shape TagTool emits.
//!
//! Triangle strips are split on the `0xFFFF` restart sentinel and
//! converted per-segment with parity-correct winding plus
//! degenerate-triangle filtering (any window with two equal indices
//! is dropped). Transparent parts (`part_type = 4`) typically
//! contain double-sided geometry baked in by the importer (each
//! triangle once per winding); JMS export keeps both copies, same
//! as TagTool — dedupe is the caller's choice.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

use crate::api::TagStruct;
use crate::fields::TagFieldData;
use crate::file::TagFile;
use crate::geometry::{
    quat_from_basis_columns, quat_mul, quat_negate, quat_rotate,
    read_compression_bounds, read_int_any, read_point3d, read_quat, read_real,
    read_string_id, read_tag_ref_path, read_vec3, scale_point, strip_to_list,
    vec3_add, vec3_cross, vec3_len, vec3_scale, vec3_sub, walk_surface_ring,
    CompressionBounds, EdgeRow, SCALE,
};

/// JMS export errors. Most failures during a corpus sweep land in
/// `MissingField` (schema-shape variation) or `Io` (write-out).
#[derive(Debug)]
pub enum JmsError {
    /// A required field couldn't be located on the parsed tag —
    /// either the schema doesn't have it or the tag instance left it
    /// empty. Carries the dotted field path for diagnosis.
    MissingField(&'static str),
    /// Io error from the JMS writer.
    Io(io::Error),
}

impl std::fmt::Display for JmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(p) => write!(f, "render_model is missing required field: {p}"),
            Self::Io(e) => write!(f, "JMS write failed: {e}"),
        }
    }
}

impl std::error::Error for JmsError {}

impl From<io::Error> for JmsError {
    fn from(e: io::Error) -> Self { Self::Io(e) }
}

/// JMS skeletal node (bone). `parent` is `-1` for roots.
#[derive(Debug, Clone)]
pub struct JmsNode {
    pub name: String,
    pub parent: i16,
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
}

/// JMS material entry. `name` is the shader basename (with attribute
/// symbols where applicable); `material_name` is the
/// `(slot) [lod] perm region` cell label.
#[derive(Debug, Clone)]
pub struct JmsMaterial {
    pub name: String,
    pub material_name: String,
}

/// JMS marker (one per marker_group variant). `radius = -1.0`
/// matches the embedded-source convention for "unset radius".
#[derive(Debug, Clone)]
pub struct JmsMarker {
    pub name: String,
    pub node_index: i16,
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub radius: f32,
}

/// JMS vertex entry. JMS doesn't share vertices across triangles —
/// each triangle owns a fresh 3-tuple of vertex entries.
#[derive(Debug, Clone)]
pub struct JmsVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub node_sets: Vec<(i16, f32)>,
    pub uvs: Vec<[f32; 2]>,
}

/// JMS triangle: material slot + 3 vertex indices into [`JmsFile::vertices`].
#[derive(Debug, Clone)]
pub struct JmsTriangle {
    pub material: i32,
    pub v: [u32; 3],
}

/// JMS sphere collision primitive. `parent` is a node index, `material`
/// indexes into [`JmsFile::materials`].
#[derive(Debug, Clone)]
pub struct JmsSphere {
    pub name: String,
    pub parent: i32,
    pub material: i32,
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub radius: f32,
}

/// JMS axis-aligned-in-local-space box. `width`/`length`/`height` are
/// FULL extents (twice the half-extents the tag stores).
#[derive(Debug, Clone)]
pub struct JmsBox {
    pub name: String,
    pub parent: i32,
    pub material: i32,
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub width: f32,
    pub length: f32,
    pub height: f32,
}

/// JMS capsule (Halo "pill"). Anchored at the bottom-cap center.
#[derive(Debug, Clone)]
pub struct JmsCapsule {
    pub name: String,
    pub parent: i32,
    pub material: i32,
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub height: f32,
    pub radius: f32,
}

/// JMS convex shape — explicit per-vertex polyhedron.
#[derive(Debug, Clone)]
pub struct JmsConvex {
    pub name: String,
    pub parent: i32,
    pub material: i32,
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub vertices: Vec<[f32; 3]>,
}

/// JMS ragdoll constraint between two bodies.
#[derive(Debug, Clone)]
pub struct JmsRagdoll {
    pub name: String,
    pub attached: i32,
    pub referenced: i32,
    pub attached_rotation: [f32; 4],
    pub attached_translation: [f32; 3],
    pub referenced_rotation: [f32; 4],
    pub referenced_translation: [f32; 3],
    pub min_twist: f32, pub max_twist: f32,
    pub min_cone: f32, pub max_cone: f32,
    pub min_plane: f32, pub max_plane: f32,
    pub friction_limit: f32,
}

/// JMS hinge constraint (covers `hinge_constraints` and
/// `limited_hinge_constraints` variants — `is_limited` distinguishes).
#[derive(Debug, Clone)]
pub struct JmsHinge {
    pub name: String,
    pub body_a: i32,
    pub body_b: i32,
    pub a_rotation: [f32; 4],
    pub a_translation: [f32; 3],
    pub b_rotation: [f32; 4],
    pub b_translation: [f32; 3],
    pub is_limited: i32,
    pub friction_limit: f32,
    pub min_angle: f32,
    pub max_angle: f32,
}

/// A reconstructed JMS file in memory — the full set of sections
/// JMS export emits, ready for [`Self::write`] or for inspection by
/// validators. Render-model fields (`nodes`/`materials`/`markers`/
/// `vertices`/`triangles`) populate from `from_render_model`;
/// collision/physics fields populate from `from_collision_model` and
/// `from_physics_model`. Any can be combined into a single JmsFile
/// for an `.hlmt` (model) export.
#[derive(Debug, Clone, Default)]
pub struct JmsFile {
    pub nodes: Vec<JmsNode>,
    pub materials: Vec<JmsMaterial>,
    pub markers: Vec<JmsMarker>,
    pub vertices: Vec<JmsVertex>,
    pub triangles: Vec<JmsTriangle>,
    pub spheres: Vec<JmsSphere>,
    pub boxes: Vec<JmsBox>,
    pub capsules: Vec<JmsCapsule>,
    pub convex_shapes: Vec<JmsConvex>,
    pub ragdolls: Vec<JmsRagdoll>,
    pub hinges: Vec<JmsHinge>,
}

impl JmsFile {
    /// Walk a parsed `render_model` tag and reconstruct the JMS
    /// scene from its inline geometry (`per mesh temporary[*]`),
    /// nodes, marker_groups, and region/permutation/material walk.
    pub fn from_render_model(tag: &TagFile) -> Result<Self, JmsError> {
        let root = tag.root();
        // The tag stores `default rotation/translation` LOCAL to each
        // node's parent. JMS expects nodes in WORLD-space bind pose,
        // so chain locals forward through parent pointers. Forward
        // chaining works because the tag stores nodes parent-before-
        // child. Markers, by contrast, stay local-to-their-attached-
        // node in JMS — the importer composes them via the bone
        // parent at scene-construction time. Same convention TagTool
        // / Foundry / the H3 Blender exporter all use.
        let local_nodes = read_nodes(&root)?;
        let world_nodes = chain_local_to_world(&local_nodes);
        let bounds = read_compression_bounds(&root);
        let (materials, part_material_map, mesh_emit_order) = build_materials(&root)?;
        let markers = read_markers(&root)?;
        let (vertices, triangles) = build_geometry(
            &root, &part_material_map, &mesh_emit_order, &bounds,
        )?;
        Ok(Self { nodes: world_nodes, materials, markers, vertices, triangles, ..Default::default() })
    }

    /// Walk a parsed `collision_model` tag and reconstruct the JMS
    /// scene from its BSP geometry. Vertices stay in their BSP's
    /// local space — pass a `render_model`-derived skeleton via
    /// [`Self::from_collision_model_with_skeleton`] if you want
    /// world-space placement (which is what embedded source JMSes
    /// carry).
    pub fn from_collision_model(tag: &TagFile) -> Result<Self, JmsError> {
        Self::build_collision_model(tag, None)
    }

    /// Same as [`Self::from_collision_model`] but composes each
    /// BSP's vertices through the skeleton's world-space transforms
    /// (chained from the render_model's local-space `default
    /// rotation`/`translation`). The lookup matches BSP node names
    /// against the supplied skeleton's node names — bones not found
    /// in the skeleton stay in BSP-local space.
    pub fn from_collision_model_with_skeleton(
        tag: &TagFile,
        skeleton: &[JmsNode],
    ) -> Result<Self, JmsError> {
        Self::build_collision_model(tag, Some(skeleton))
    }

    fn build_collision_model(tag: &TagFile, skeleton: Option<&[JmsNode]>) -> Result<Self, JmsError> {
        let root = tag.root();
        let nodes = read_phmo_nodes(&root)?;
        // Build name → world-transform map from the skeleton (if
        // provided). The skeleton is expected to be in world space
        // (e.g. the result of `from_render_model`).
        let bone_xform: Option<std::collections::HashMap<String, ([f32; 4], [f32; 3])>> =
            skeleton.map(|nodes| {
                nodes.iter().map(|n| (n.name.clone(), (n.rotation, n.translation))).collect()
            });
        let materials_block = root.field_path("materials").and_then(|f| f.as_block())
            .ok_or(JmsError::MissingField("materials"))?;
        let regions_block = root.field_path("regions").and_then(|f| f.as_block())
            .ok_or(JmsError::MissingField("regions"))?;

        let mut materials: Vec<JmsMaterial> = Vec::new();
        let mut vertices: Vec<JmsVertex> = Vec::new();
        let mut triangles: Vec<JmsTriangle> = Vec::new();

        for ri in 0..regions_block.len() {
            let region = regions_block.element(ri).unwrap();
            let region_name = read_string_id(&region, "name").unwrap_or_default();
            let perms = match region.field("permutations").and_then(|f| f.as_block()) {
                Some(b) => b, None => continue,
            };
            for pi in 0..perms.len() {
                let perm = perms.element(pi).unwrap();
                let perm_name = read_string_id(&perm, "name").unwrap_or_default();
                let bsps = match perm.field("bsps").and_then(|f| f.as_block()) {
                    Some(b) => b, None => continue,
                };
                for bi in 0..bsps.len() {
                    let bsp_elem = bsps.element(bi).unwrap();
                    let node_idx = read_int_any(&bsp_elem, "node index").map(|v| v as i16).unwrap_or(-1);
                    let bsp = match bsp_elem.field("bsp").and_then(|f| f.as_struct()) { Some(s) => s, None => continue };
                    let surfaces = match bsp.field("surfaces").and_then(|f| f.as_block()) { Some(b) => b, None => continue };
                    let edges = match bsp.field("edges").and_then(|f| f.as_block()) { Some(b) => b, None => continue };
                    let bsp_verts = match bsp.field("vertices").and_then(|f| f.as_block()) { Some(b) => b, None => continue };

                    // World transform for this BSP — looked up by
                    // the BSP's bone NAME in the supplied skeleton
                    // (collision_model nodes carry no transforms).
                    // None means we leave vertices in BSP-local
                    // space; matches what `from_collision_model`
                    // gives without a skeleton.
                    let bone_world = if let (Some(map), Some(node_block)) = (
                        bone_xform.as_ref(),
                        Some(&nodes),
                    ) {
                        node_block.get(node_idx as usize)
                            .map(|n| n.name.as_str())
                            .and_then(|name| map.get(name))
                            .copied()
                    } else { None };

                    // Build a (start_vertex, end_vertex, forward,
                    // reverse, left_surface, right_surface) cache to
                    // avoid hammering the as_struct API in the hot
                    // edge-walk loop.
                    let edge_cache: Vec<EdgeRow> = (0..edges.len()).map(|k| {
                        let e = edges.element(k).unwrap();
                        EdgeRow {
                            start_vertex: read_int_any(&e, "start vertex").unwrap_or(-1) as i32,
                            end_vertex: read_int_any(&e, "end vertex").unwrap_or(-1) as i32,
                            forward_edge: read_int_any(&e, "forward edge").unwrap_or(-1) as i32,
                            reverse_edge: read_int_any(&e, "reverse edge").unwrap_or(-1) as i32,
                            left_surface: read_int_any(&e, "left surface").unwrap_or(-1) as i32,
                            right_surface: read_int_any(&e, "right surface").unwrap_or(-1) as i32,
                        }
                    }).collect();

                    let vert_points: Vec<[f32; 3]> = (0..bsp_verts.len()).map(|k| {
                        let local = scale_point(read_point3d(&bsp_verts.element(k).unwrap(), "point"));
                        if let Some((rot, trans)) = bone_world {
                            // World = bone_translation + bone_rotation.rotate(local)
                            vec3_add(trans, quat_rotate(rot, local))
                        } else {
                            local
                        }
                    }).collect();

                    for si in 0..surfaces.len() {
                        let surface = surfaces.element(si).unwrap();
                        let first_edge = read_int_any(&surface, "first edge").unwrap_or(-1) as i32;
                        if first_edge < 0 { continue; }
                        let surface_material = read_int_any(&surface, "material").unwrap_or(-1) as i32;

                        // Edge-ring walk.
                        let polygon = walk_surface_ring(si as i32, first_edge, &edge_cache);
                        if polygon.len() < 3 { continue; }

                        // Look up shader name for this surface's material.
                        let shader_name = if surface_material >= 0 && (surface_material as usize) < materials_block.len() {
                            let m = materials_block.element(surface_material as usize).unwrap();
                            read_string_id(&m, "name").unwrap_or_default()
                        } else {
                            "default".to_owned()
                        };
                        let cell_label = format!("{} {}", perm_name, region_name);
                        let jms_idx = match materials.iter().position(|m|
                            m.name == shader_name && m.material_name.ends_with(&cell_label)
                        ) {
                            Some(i) => i as i32,
                            None => {
                                let slot = materials.len() + 1;
                                materials.push(JmsMaterial {
                                    name: shader_name,
                                    material_name: format!("({}) {}", slot, cell_label),
                                });
                                (materials.len() - 1) as i32
                            }
                        };

                        // Triangle-fan the convex polygon.
                        for k in 1..polygon.len() - 1 {
                            let a = polygon[0];
                            let b = polygon[k];
                            let c = polygon[k + 1];
                            let base = vertices.len() as u32;
                            for &vi in &[a, b, c] {
                                let pos = vert_points.get(vi as usize).copied().unwrap_or([0.0; 3]);
                                vertices.push(JmsVertex {
                                    position: pos,
                                    normal: [0.0, 0.0, 1.0],
                                    node_sets: vec![(node_idx, 1.0)],
                                    uvs: vec![[0.0, 0.0]],
                                });
                            }
                            triangles.push(JmsTriangle {
                                material: jms_idx,
                                v: [base, base + 1, base + 2],
                            });
                        }
                    }
                }
            }
        }

        Ok(Self { nodes, materials, vertices, triangles, ..Default::default() })
    }

    /// Walk a parsed `physics_model` tag and reconstruct the JMS
    /// scene from its Havok shape primitives + ragdoll/hinge
    /// constraints. Without a skeleton, the emitted nodes carry
    /// only names + tree links (identity transforms) — pass a
    /// render_model-derived skeleton via
    /// [`Self::from_physics_model_with_skeleton`] to populate
    /// world-space bind-pose transforms for the JMS importer.
    pub fn from_physics_model(tag: &TagFile) -> Result<Self, JmsError> {
        Self::build_physics_model(tag, None)
    }

    /// Same as [`Self::from_physics_model`] but layers the supplied
    /// skeleton's world-space transforms onto the phmo's nodes,
    /// matched by name. Bones not found in the skeleton stay at
    /// identity. Use the skeleton from a sibling `render_model`
    /// (via `JmsFile::from_render_model`).
    pub fn from_physics_model_with_skeleton(
        tag: &TagFile,
        skeleton: &[JmsNode],
    ) -> Result<Self, JmsError> {
        Self::build_physics_model(tag, Some(skeleton))
    }

    fn build_physics_model(tag: &TagFile, skeleton: Option<&[JmsNode]>) -> Result<Self, JmsError> {
        let root = tag.root();
        let mut nodes = read_phmo_nodes(&root)?;
        if let Some(skel) = skeleton {
            let by_name: std::collections::HashMap<&str, &JmsNode> =
                skel.iter().map(|n| (n.name.as_str(), n)).collect();
            for n in nodes.iter_mut() {
                if let Some(src) = by_name.get(n.name.as_str()) {
                    n.rotation = src.rotation;
                    n.translation = src.translation;
                }
            }
        }
        let materials = read_phmo_materials(&root)?;
        let parent_lookup = build_phmo_parent_lookup(&root);
        let spheres = read_phmo_spheres(&root, &parent_lookup);
        let boxes = read_phmo_boxes(&root, &parent_lookup);
        let capsules = read_phmo_pills(&root, &parent_lookup);
        let convex_shapes = read_phmo_polyhedra(&root, &parent_lookup);
        let ragdolls = read_phmo_ragdolls(&root);
        let mut hinges = read_phmo_hinges(&root, false);
        hinges.extend(read_phmo_hinges(&root, true));
        Ok(Self {
            nodes,
            materials,
            spheres,
            boxes,
            capsules,
            convex_shapes,
            ragdolls,
            hinges,
            ..Default::default()
        })
    }

    /// Write the JMS as version 8213 text format (the H3 source
    /// convention) into `w`. Layout matches the embedded-source
    /// section ordering exactly so byte-diffs against artist
    /// originals stay focused on the data, not boilerplate.
    pub fn write<W: Write>(&self, w: &mut W) -> Result<(), JmsError> {
        writeln!(w, ";### VERSION ###")?;
        writeln!(w, "8213")?;
        writeln!(w)?;

        writeln!(w, ";### NODES ###")?;
        writeln!(w, "{}", self.nodes.len())?;
        writeln!(w, ";\t<name>")?;
        writeln!(w, ";\t<parent node index>")?;
        writeln!(w, ";\t<default rotation <i,j,k,w>>")?;
        writeln!(w, ";\t<default translation <x,y,z>>")?;
        writeln!(w)?;
        for (i, n) in self.nodes.iter().enumerate() {
            writeln!(w, ";NODE {i}")?;
            writeln!(w, "{}", n.name)?;
            writeln!(w, "{}", n.parent)?;
            write_floats(w, &n.rotation)?;
            write_floats(w, &n.translation)?;
            writeln!(w)?;
        }

        writeln!(w, ";### MATERIALS ###")?;
        writeln!(w, "{}", self.materials.len())?;
        writeln!(w, ";\t<name>")?;
        writeln!(w, ";\t<material name>")?;
        writeln!(w)?;
        for (i, m) in self.materials.iter().enumerate() {
            writeln!(w, ";MATERIAL {i}")?;
            writeln!(w, "{}", m.name)?;
            writeln!(w, "{}", m.material_name)?;
            writeln!(w)?;
        }

        writeln!(w, ";### MARKERS ###")?;
        writeln!(w, "{}", self.markers.len())?;
        writeln!(w, ";\t<name>")?;
        writeln!(w, ";\t<node index>")?;
        writeln!(w, ";\t<rotation <i,j,k,w>>")?;
        writeln!(w, ";\t<translation <x,y,z>>")?;
        writeln!(w, ";\t<radius>")?;
        writeln!(w)?;
        for (i, m) in self.markers.iter().enumerate() {
            writeln!(w, ";MARKER {i}")?;
            writeln!(w, "{}", m.name)?;
            writeln!(w, "{}", m.node_index)?;
            write_floats(w, &m.rotation)?;
            write_floats(w, &m.translation)?;
            write_floats(w, &[m.radius])?;
            writeln!(w)?;
        }

        writeln!(w, ";### INSTANCE XREF PATHS ###")?;
        writeln!(w, "0")?;
        writeln!(w, ";\t<path>")?;
        writeln!(w, ";\t<name>")?;
        writeln!(w)?;

        writeln!(w, ";### INSTANCE MARKERS ###")?;
        writeln!(w, "0")?;
        writeln!(w, ";\t<name>")?;
        writeln!(w, ";\t<unique identifier>")?;
        writeln!(w, ";\t<path index>")?;
        writeln!(w, ";\t<rotation <i,j,k,w>>")?;
        writeln!(w, ";\t<translation <x,y,z>>")?;
        writeln!(w)?;

        writeln!(w, ";### VERTICES ###")?;
        writeln!(w, "{}", self.vertices.len())?;
        writeln!(w, ";\t<position>")?;
        writeln!(w, ";\t<normal>")?;
        writeln!(w, ";\t<node influences count>")?;
        writeln!(w, ";\t\t<node influences <index, weight>>")?;
        writeln!(w, ";\t\t<...>")?;
        writeln!(w, ";\t<texture coordinate count>")?;
        writeln!(w, ";\t\t<texture coordinates <u,v>>")?;
        writeln!(w, ";\t\t<...>")?;
        writeln!(w, ";\t\t<vertex color <r,g,b>>")?;
        writeln!(w, ";\t\t<...>")?;
        writeln!(w)?;
        for (i, v) in self.vertices.iter().enumerate() {
            writeln!(w, ";VERTEX {i}")?;
            write_floats(w, &v.position)?;
            write_floats(w, &v.normal)?;
            writeln!(w, "{}", v.node_sets.len())?;
            for (idx, wt) in &v.node_sets {
                writeln!(w, "{}", idx)?;
                write_floats(w, &[*wt])?;
            }
            writeln!(w, "{}", v.uvs.len())?;
            for uv in &v.uvs {
                write_floats(w, uv)?;
            }
            write_floats(w, &[0.0, 0.0, 0.0])?; // vertex color always zero per TagTool
            writeln!(w)?;
        }

        writeln!(w, ";### TRIANGLES ###")?;
        writeln!(w, "{}", self.triangles.len())?;
        writeln!(w, ";\t<material index>")?;
        writeln!(w, ";\t<vertex indices <v0,v1,v2>>")?;
        writeln!(w)?;
        for (i, t) in self.triangles.iter().enumerate() {
            writeln!(w, ";TRIANGLE {i}")?;
            writeln!(w, "{}", t.material)?;
            writeln!(w, "{}\t{}\t{}", t.v[0], t.v[1], t.v[2])?;
            writeln!(w)?;
        }

        // Phmo / coll trailing sections. Section headers + helper
        // comment lines mirror the embedded source JMS layout
        // exactly so byte diffs stay focused on data. Sections that
        // we don't currently populate (CAR_WHEEL, POINT_TO_POINT,
        // PRISMATIC, BOUNDING SPHERE, SKYLIGHT) emit empty.

        writeln!(w, ";### SPHERES ###")?;
        writeln!(w, "{}", self.spheres.len())?;
        for h in ["<name>", "<parent>", "<material>", "<rotation <i,j,k,w>>", "<translation <x,y,z>>", "<radius>"] {
            writeln!(w, ";\t{h}")?;
        }
        writeln!(w)?;
        for (i, s) in self.spheres.iter().enumerate() {
            writeln!(w, ";SPHERE {i}")?;
            writeln!(w, "{}", s.name)?;
            writeln!(w, "{}", s.parent)?;
            writeln!(w, "{}", s.material)?;
            write_floats(w, &s.rotation)?;
            write_floats(w, &s.translation)?;
            write_floats(w, &[s.radius])?;
            writeln!(w)?;
        }

        writeln!(w, ";### BOXES ###")?;
        writeln!(w, "{}", self.boxes.len())?;
        for h in ["<name>", "<parent>", "<material>", "<rotation <i,j,k,w>>", "<translation <x,y,z>>", "<width (x)>", "<length (y)>", "<height (z)>"] {
            writeln!(w, ";\t{h}")?;
        }
        writeln!(w)?;
        for (i, b) in self.boxes.iter().enumerate() {
            writeln!(w, ";BOX {i}")?;
            writeln!(w, "{}", b.name)?;
            writeln!(w, "{}", b.parent)?;
            writeln!(w, "{}", b.material)?;
            write_floats(w, &b.rotation)?;
            write_floats(w, &b.translation)?;
            write_floats(w, &[b.width])?;
            write_floats(w, &[b.length])?;
            write_floats(w, &[b.height])?;
            writeln!(w)?;
        }

        writeln!(w, ";### CAPSULES ###")?;
        writeln!(w, "{}", self.capsules.len())?;
        for h in ["<name>", "<parent>", "<material>", "<rotation <i,j,k,w>>", "<translation <x,y,z>>", "<height>", "<radius>"] {
            writeln!(w, ";\t{h}")?;
        }
        writeln!(w)?;
        for (i, c) in self.capsules.iter().enumerate() {
            writeln!(w, ";CAPSULE {i}")?;
            writeln!(w, "{}", c.name)?;
            writeln!(w, "{}", c.parent)?;
            writeln!(w, "{}", c.material)?;
            write_floats(w, &c.rotation)?;
            write_floats(w, &c.translation)?;
            write_floats(w, &[c.height])?;
            write_floats(w, &[c.radius])?;
            writeln!(w)?;
        }

        writeln!(w, ";### CONVEX SHAPES ###")?;
        writeln!(w, "{}", self.convex_shapes.len())?;
        // The 8213 source variant we observed (masterchief_ragdoll.jms)
        // omits the "height" field that 8207 carried — emit
        // name/parent/material/rotation/translation/vertex_count then
        // the vertex list directly.
        for h in ["<name>", "<parent>", "<material>", "<rotation <i,j,k,w>>", "<translation <x,y,z>>", "<vertex count>", "<...vertices>"] {
            writeln!(w, ";\t{h}")?;
        }
        writeln!(w)?;
        for (i, c) in self.convex_shapes.iter().enumerate() {
            writeln!(w, ";CONVEX SHAPE {i}")?;
            writeln!(w, "{}", c.name)?;
            writeln!(w, "{}", c.parent)?;
            writeln!(w, "{}", c.material)?;
            write_floats(w, &c.rotation)?;
            write_floats(w, &c.translation)?;
            writeln!(w, "{}", c.vertices.len())?;
            for v in &c.vertices {
                write_floats(w, v)?;
            }
            writeln!(w)?;
        }

        writeln!(w, ";### RAGDOLLS ###")?;
        writeln!(w, "{}", self.ragdolls.len())?;
        for h in ["<name>", "<attached index>", "<referenced index>", "<attached transform>", "<reference transform>", "<min twist>", "<max twist>", "<min cone>", "<max cone>", "<min plane>", "<max plane>", "<friction limit>"] {
            writeln!(w, ";\t{h}")?;
        }
        writeln!(w)?;
        for (i, r) in self.ragdolls.iter().enumerate() {
            writeln!(w, ";RAGDOLL {i}")?;
            writeln!(w, "{}", r.name)?;
            writeln!(w, "{}", r.attached)?;
            writeln!(w, "{}", r.referenced)?;
            write_floats(w, &r.attached_rotation)?;
            write_floats(w, &r.attached_translation)?;
            write_floats(w, &r.referenced_rotation)?;
            write_floats(w, &r.referenced_translation)?;
            write_floats(w, &[r.min_twist])?;
            write_floats(w, &[r.max_twist])?;
            write_floats(w, &[r.min_cone])?;
            write_floats(w, &[r.max_cone])?;
            write_floats(w, &[r.min_plane])?;
            write_floats(w, &[r.max_plane])?;
            write_floats(w, &[r.friction_limit])?;
            writeln!(w)?;
        }

        writeln!(w, ";### HINGES ###")?;
        writeln!(w, "{}", self.hinges.len())?;
        for h in ["<name>", "<body A index>", "<body B index>", "<body A transform>", "<body B transform>", "<is limited>", "<friction limit>", "<min angle>", "<max angle"] {
            writeln!(w, ";\t{h}")?;
        }
        writeln!(w)?;
        for (i, h) in self.hinges.iter().enumerate() {
            writeln!(w, ";HINGE {i}")?;
            writeln!(w, "{}", h.name)?;
            writeln!(w, "{}", h.body_a)?;
            writeln!(w, "{}", h.body_b)?;
            write_floats(w, &h.a_rotation)?;
            write_floats(w, &h.a_translation)?;
            write_floats(w, &h.b_rotation)?;
            write_floats(w, &h.b_translation)?;
            writeln!(w, "{}", h.is_limited)?;
            write_floats(w, &[h.friction_limit])?;
            write_floats(w, &[h.min_angle])?;
            write_floats(w, &[h.max_angle])?;
            writeln!(w)?;
        }

        // Sections we don't currently populate stay empty.
        for (name, helps) in EMPTY_SECTIONS_TRAILING {
            writeln!(w, ";### {name} ###")?;
            writeln!(w, "0")?;
            for h in *helps { writeln!(w, ";\t{h}")?; }
            writeln!(w)?;
        }
        writeln!(w)?;
        Ok(())
    }
}

// ---- node / material / marker / geometry walkers ----

fn read_nodes(root: &TagStruct<'_>) -> Result<Vec<JmsNode>, JmsError> {
    let block = root.field_path("nodes").and_then(|f| f.as_block())
        .ok_or(JmsError::MissingField("nodes"))?;
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        let n = block.element(i).unwrap();
        out.push(JmsNode {
            name: read_string_id(&n, "name").unwrap_or_default(),
            parent: read_block_index(&n, "parent node"),
            rotation: read_quat(&n, "default rotation"),
            translation: scale_point(read_point3d(&n, "default translation")),
        });
    }
    Ok(out)
}

/// Convert per-node local transforms (parent-relative, as the tag
/// stores them) to world transforms (root-relative, as JMS expects).
/// Forward-iteration works because the tag stores nodes
/// parent-before-child. Mirrors Foundry's
/// `RenderArmature.{create_bone, parent_bone}` matrix chain in
/// `connected_geometry.py:621-645`, just expressed with quaternions
/// directly instead of via 4×4 matrices: same composition rule
/// `world = parent_world * local`.
fn chain_local_to_world(local: &[JmsNode]) -> Vec<JmsNode> {
    let mut out: Vec<JmsNode> = Vec::with_capacity(local.len());
    for (i, n) in local.iter().enumerate() {
        let world = if n.parent < 0 || (n.parent as usize) >= i {
            // Root or forward reference (shouldn't happen in
            // well-formed tags) — treat as already-world.
            n.clone()
        } else {
            let parent = &out[n.parent as usize];
            JmsNode {
                name: n.name.clone(),
                parent: n.parent,
                rotation: quat_mul(parent.rotation, n.rotation),
                translation: vec3_add(
                    parent.translation,
                    quat_rotate(parent.rotation, n.translation),
                ),
            }
        };
        out.push(world);
    }
    out
}

// ---- collision_model walkers ----

// ---- physics_model walkers ----

/// Read the physics_model nodes block (parallel structure to
/// render_model nodes — same `name`/`parent`/`sibling`/`child` shape).
/// JMS stores nodes as world-space bind pose, but the physics_model
/// nodes block has only names + tree links (no transforms), so we
/// emit them with identity transforms; bones are placed by the
/// caller's render_model when combining into a model.
fn read_phmo_nodes(root: &TagStruct<'_>) -> Result<Vec<JmsNode>, JmsError> {
    let block = root.field_path("nodes").and_then(|f| f.as_block())
        .ok_or(JmsError::MissingField("nodes"))?;
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        let n = block.element(i).unwrap();
        out.push(JmsNode {
            name: read_string_id(&n, "name").unwrap_or_default(),
            parent: read_block_index(&n, "parent"),
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation: [0.0, 0.0, 0.0],
        });
    }
    Ok(out)
}

fn read_phmo_materials(root: &TagStruct<'_>) -> Result<Vec<JmsMaterial>, JmsError> {
    let block = root.field_path("materials").and_then(|f| f.as_block())
        .ok_or(JmsError::MissingField("materials"))?;
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        let m = block.element(i).unwrap();
        let name = read_string_id(&m, "name").unwrap_or_default();
        // physics_model materials carry a separate `global material
        // name` but the JMS material_name slot is the same name,
        // matching TagTool's 1:1 copy.
        out.push(JmsMaterial {
            name: name.clone(),
            material_name: name,
        });
    }
    Ok(out)
}

/// Each rigid_body references one shape via `(shape_type, shape)`.
/// Build a map keyed by (shape_type_value, shape_index) → node_index
/// so the per-shape walks can attach each shape to the right node.
/// Shape-type enum values verified by inspecting H3 phmo tags:
/// 0=sphere, 1=pill (=capsule), 2=box, 3=triangle, 4=polyhedron,
/// (higher values for multi-sphere/list/mopp not yet seen). See
/// `SHAPE_TYPE_*` constants below.
fn build_phmo_parent_lookup(root: &TagStruct<'_>) -> std::collections::HashMap<(i64, i64), i32> {
    let mut out = std::collections::HashMap::new();
    let Some(rbs) = root.field_path("rigid bodies").and_then(|f| f.as_block()) else { return out; };
    for i in 0..rbs.len() {
        let rb = rbs.element(i).unwrap();
        let node_idx = read_int_any(&rb, "node").map(|v| v as i32).unwrap_or(-1);
        let Some(sr) = rb.field("shape reference").and_then(|f| f.as_struct()) else { continue; };
        let Some(shape_type) = read_int_any(&sr, "shape type") else { continue; };
        let Some(shape_idx) = read_int_any(&sr, "shape") else { continue; };
        out.insert((shape_type, shape_idx), node_idx);
    }
    out
}

fn parent_for(parent_lookup: &std::collections::HashMap<(i64, i64), i32>, shape_type: i64, idx: usize) -> i32 {
    parent_lookup.get(&(shape_type, idx as i64)).copied().unwrap_or(-1)
}

const SHAPE_TYPE_SPHERE: i64 = 0;
const SHAPE_TYPE_PILL: i64 = 1;
const SHAPE_TYPE_BOX: i64 = 2;
const SHAPE_TYPE_POLYHEDRON: i64 = 4;

fn read_phmo_spheres(root: &TagStruct<'_>, parents: &std::collections::HashMap<(i64, i64), i32>) -> Vec<JmsSphere> {
    let Some(block) = root.field_path("spheres").and_then(|f| f.as_block()) else { return Vec::new(); };
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        let s = block.element(i).unwrap();
        let base = match s.field("base").and_then(|f| f.as_struct()) { Some(b) => b, None => continue };
        // Sphere has no per-shape rotation/translation — TagTool
        // outputs identity. Radius lives at `sphere/radius` (sibling
        // to `base`).
        out.push(JmsSphere {
            name: read_string_id(&base, "name").unwrap_or_default(),
            parent: parent_for(parents, SHAPE_TYPE_SPHERE, i),
            material: read_int_any(&base, "material").map(|v| v as i32).unwrap_or(0),
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation: [0.0, 0.0, 0.0],
            radius: read_real(&s, "radius").unwrap_or(0.0) * SCALE,
        });
    }
    out
}

fn read_phmo_boxes(root: &TagStruct<'_>, parents: &std::collections::HashMap<(i64, i64), i32>) -> Vec<JmsBox> {
    let Some(block) = root.field_path("boxes").and_then(|f| f.as_block()) else { return Vec::new(); };
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        let b = block.element(i).unwrap();
        let base = match b.field("base").and_then(|f| f.as_struct()) { Some(s) => s, None => continue };
        let cts = match b.field("convex transform shape").and_then(|f| f.as_struct()) { Some(c) => c, None => continue };
        // The box-specific half_extents lives at top-level on the
        // box block (sibling to `box shape`), as `half extents` —
        // 3-vec3 in world units. The Havok convex skin-width radius
        // is at `box shape/radius` and the source JMS adds it to
        // every half-extent before doubling: each face of the box
        // grows by one radius (typically 0.0164 wu = 1.64cm = the
        // standard Halo convex radius). JMS dimension formula:
        //   side = (half_extent + radius) × 2 × 100
        let half = read_vec3(&b, "half extents");
        let convex_radius = b.field("box shape").and_then(|f| f.as_struct())
            .and_then(|bs| read_real(&bs, "radius"))
            .unwrap_or(0.0);
        out.push(JmsBox {
            name: read_string_id(&base, "name").unwrap_or_default(),
            parent: parent_for(parents, SHAPE_TYPE_BOX, i),
            material: read_int_any(&base, "material").map(|v| v as i32).unwrap_or(0),
            rotation: rotation_from_basis(&cts),
            translation: scale_point(read_vec3(&cts, "translation")),
            width:  (half[0] + convex_radius) * 2.0 * SCALE,
            length: (half[1] + convex_radius) * 2.0 * SCALE,
            height: (half[2] + convex_radius) * 2.0 * SCALE,
        });
    }
    out
}

fn read_phmo_pills(root: &TagStruct<'_>, parents: &std::collections::HashMap<(i64, i64), i32>) -> Vec<JmsCapsule> {
    let Some(block) = root.field_path("pills").and_then(|f| f.as_block()) else { return Vec::new(); };
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        let p = block.element(i).unwrap();
        let base = match p.field("base").and_then(|f| f.as_struct()) { Some(s) => s, None => continue };
        // Pill radius lives at `capsule shape/radius` (sibling to
        // `capsule shape/base`, which is a generic Havok shape base).
        let radius = p.field("capsule shape").and_then(|f| f.as_struct())
            .and_then(|cs| read_real(&cs, "radius"))
            .unwrap_or(0.0);
        let bottom = read_vec3(&p, "bottom");
        let top = read_vec3(&p, "top");
        // TagTool pill anchor: translation = bottom + normalized(bottom - top) * radius
        let dir = vec3_sub(bottom, top);
        let dlen = vec3_len(dir);
        let unit = if dlen > 1e-9 { vec3_scale(dir, 1.0 / dlen) } else { [0.0, 0.0, 0.0] };
        let anchor = vec3_add(bottom, vec3_scale(unit, radius));
        let height = vec3_len(vec3_sub(top, bottom)) * SCALE;
        // Orientation from the `top - bottom` axis (custom quat;
        // TagTool's QuaternionFromVector with reference up = (0, 0, -1))
        let axis = vec3_sub(top, bottom);
        let rot = quat_from_axis_to_ref(axis);
        out.push(JmsCapsule {
            name: read_string_id(&base, "name").unwrap_or_default(),
            parent: parent_for(parents, SHAPE_TYPE_PILL, i),
            material: read_int_any(&base, "material").map(|v| v as i32).unwrap_or(0),
            rotation: rot,
            translation: scale_point(anchor),
            height,
            radius: radius * SCALE,
        });
    }
    out
}

fn read_phmo_polyhedra(root: &TagStruct<'_>, parents: &std::collections::HashMap<(i64, i64), i32>) -> Vec<JmsConvex> {
    let Some(block) = root.field_path("polyhedra").and_then(|f| f.as_block()) else { return Vec::new(); };
    let four_vectors = root.field_path("polyhedron four vectors").and_then(|f| f.as_block());
    let mut out = Vec::with_capacity(block.len());
    let mut fv_offset: usize = 0;
    for i in 0..block.len() {
        let p = block.element(i).unwrap();
        let base = match p.field("base").and_then(|f| f.as_struct()) { Some(s) => s, None => continue };
        // `four vectors size` is at the polyhedron top level, not
        // inside `polyhedron shape` (which only carries base + radius).
        let fv_size = read_int_any(&p, "four vectors size").unwrap_or(0) as usize;
        let mut verts: Vec<[f32; 3]> = Vec::new();
        if let Some(fvb) = &four_vectors {
            for k in 0..fv_size {
                let Some(fv) = fvb.element(fv_offset + k) else { continue };
                let xv = read_vec3(&fv, "four vectors x");
                let yv = read_vec3(&fv, "four vectors y");
                let zv = read_vec3(&fv, "four vectors z");
                let xw = read_real(&fv, "havok w four vectors x").unwrap_or(0.0);
                let yw = read_real(&fv, "havok w four vectors y").unwrap_or(0.0);
                let zw = read_real(&fv, "havok w four vectors z").unwrap_or(0.0);
                // 4 vertices packed: (x.i, y.i, z.i), (x.j, y.j, z.j),
                // (x.k, y.k, z.k), (x_w, y_w, z_w)
                verts.push(scale_point([xv[0], yv[0], zv[0]]));
                verts.push(scale_point([xv[1], yv[1], zv[1]]));
                verts.push(scale_point([xv[2], yv[2], zv[2]]));
                verts.push(scale_point([xw, yw, zw]));
            }
        }
        // Dedupe duplicates (the 4-vector packing left padding when
        // the actual vertex count isn't a multiple of 4).
        let mut seen = std::collections::HashSet::new();
        verts.retain(|v| {
            let key = (v[0].to_bits(), v[1].to_bits(), v[2].to_bits());
            seen.insert(key)
        });
        // Polyhedron transform is identity — vertices are absolute.
        out.push(JmsConvex {
            name: read_string_id(&base, "name").unwrap_or_default(),
            parent: parent_for(parents, SHAPE_TYPE_POLYHEDRON, i),
            material: read_int_any(&base, "material").map(|v| v as i32).unwrap_or(0),
            rotation: [0.0, 0.0, 0.0, 1.0],
            translation: [0.0, 0.0, 0.0],
            vertices: verts,
        });
        fv_offset += fv_size;
    }
    out
}

fn read_phmo_ragdolls(root: &TagStruct<'_>) -> Vec<JmsRagdoll> {
    let Some(block) = root.field_path("ragdoll constraints").and_then(|f| f.as_block()) else { return Vec::new(); };
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        let r = block.element(i).unwrap();
        let bodies = match r.field("constraint bodies").and_then(|f| f.as_struct()) { Some(b) => b, None => continue };
        let (a_rot, a_trans) = constraint_frame(&bodies, "a");
        let (b_rot, b_trans) = constraint_frame(&bodies, "b");
        out.push(JmsRagdoll {
            name: read_string_id(&bodies, "name").unwrap_or_default(),
            attached: read_int_any(&bodies, "node a").map(|v| v as i32).unwrap_or(-1),
            referenced: read_int_any(&bodies, "node b").map(|v| v as i32).unwrap_or(-1),
            // TagTool negates the ragdoll-derived quat — verified
            // against the masterchief embedded source: e.g. b_head's
            // tag matrix gives q=(0.6995, 0.1043, 0.1043, 0.6995),
            // source has (-0.6995, -0.1043, -0.1043, -0.6995).
            attached_rotation: quat_negate(a_rot),
            attached_translation: a_trans,
            referenced_rotation: quat_negate(b_rot),
            referenced_translation: b_trans,
            min_twist: read_real(&r, "min twist").unwrap_or(0.0),
            max_twist: read_real(&r, "max twist").unwrap_or(0.0),
            min_cone: read_real(&r, "min cone").unwrap_or(0.0),
            max_cone: read_real(&r, "max cone").unwrap_or(0.0),
            min_plane: read_real(&r, "min plane").unwrap_or(0.0),
            max_plane: read_real(&r, "max plane").unwrap_or(0.0),
            // The schema field carries a typo in MCC — `max friciton torque`.
            friction_limit: read_real(&r, "max friciton torque")
                .or_else(|| read_real(&r, "max friction torque"))
                .unwrap_or(0.0),
        });
    }
    out
}

fn read_phmo_hinges(root: &TagStruct<'_>, limited: bool) -> Vec<JmsHinge> {
    let block_name = if limited { "limited hinge constraints" } else { "hinge constraints" };
    let Some(block) = root.field_path(block_name).and_then(|f| f.as_block()) else { return Vec::new(); };
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        let h = block.element(i).unwrap();
        let bodies = match h.field("constraint bodies").and_then(|f| f.as_struct()) { Some(b) => b, None => continue };
        let (a_rot, a_trans) = constraint_frame(&bodies, "a");
        let (b_rot, b_trans) = constraint_frame(&bodies, "b");
        out.push(JmsHinge {
            name: read_string_id(&bodies, "name").unwrap_or_default(),
            body_a: read_int_any(&bodies, "node a").map(|v| v as i32).unwrap_or(-1),
            body_b: read_int_any(&bodies, "node b").map(|v| v as i32).unwrap_or(-1),
            // Hinges (per TagTool) are NOT negated — only ragdolls.
            a_rotation: a_rot,
            a_translation: a_trans,
            b_rotation: b_rot,
            b_translation: b_trans,
            is_limited: if limited { 1 } else { 0 },
            friction_limit: read_real(&h, "limit friction").unwrap_or(0.0),
            min_angle: read_real(&h, "limit min angle").unwrap_or(0.0),
            max_angle: read_real(&h, "limit max angle").unwrap_or(0.0),
        });
    }
    out
}

/// Build (rotation_quat, translation) from a constraint frame's
/// `<side> forward / left / up / position` vectors. Side is `"a"` or
/// `"b"`. Matches Foundry's column-major construction
/// (connected_geometry.py:689-694): forward in column 0, left in
/// column 1, up in column 2.
fn constraint_frame(bodies: &TagStruct<'_>, side: &str) -> ([f32; 4], [f32; 3]) {
    let f = read_vec3(bodies, &format!("{side} forward"));
    let l = read_vec3(bodies, &format!("{side} left"));
    let u = read_vec3(bodies, &format!("{side} up"));
    let p = read_vec3(bodies, &format!("{side} position"));
    let rot = quat_from_basis_columns(f, l, u);
    (rot, scale_point(p))
}

/// Build a quaternion from a `convex transform shape` struct's
/// rotation_i/j/k row vectors (Havok stores rotation as 3 vec3 rows).
fn rotation_from_basis(cts: &TagStruct<'_>) -> [f32; 4] {
    let row_i = read_vec3(cts, "rotation i");
    let row_j = read_vec3(cts, "rotation j");
    let row_k = read_vec3(cts, "rotation k");
    // Rows form the rotation matrix; columns are forward/left/up.
    quat_from_basis_columns(
        [row_i[0], row_j[0], row_k[0]],
        [row_i[1], row_j[1], row_k[1]],
        [row_i[2], row_j[2], row_k[2]],
    )
}

/// Pill orientation: TagTool's `QuaternionFromVector` aligns a
/// reference up vector `(0, 0, -1)` to the supplied axis. Returns
/// the shortest-arc rotation between them; degenerate cases (axis
/// parallel or anti-parallel to the reference) get an explicit
/// 180° rotation around an arbitrary perpendicular axis.
fn quat_from_axis_to_ref(axis: [f32; 3]) -> [f32; 4] {
    let len = vec3_len(axis);
    if len < 1e-9 { return [0.0, 0.0, 0.0, 1.0]; }
    let to = vec3_scale(axis, 1.0 / len);
    let from: [f32; 3] = [0.0, 0.0, -1.0];
    let dot = from[0]*to[0] + from[1]*to[1] + from[2]*to[2];
    if dot > 0.999999 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    if dot < -0.999999 {
        // 180° around any perpendicular axis.
        let perp = if from[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let axis = vec3_cross(from, perp);
        let n = vec3_scale(axis, 1.0 / vec3_len(axis));
        return [n[0], n[1], n[2], 0.0];
    }
    let cross = vec3_cross(from, to);
    let s = ((1.0 + dot) * 2.0).sqrt();
    let inv_s = 1.0 / s;
    [cross[0] * inv_s, cross[1] * inv_s, cross[2] * inv_s, s * 0.5]
}

// ---- vector + quaternion helpers ----

// (vector + quaternion math + generic field readers moved to crate::geometry)

fn read_markers(root: &TagStruct<'_>) -> Result<Vec<JmsMarker>, JmsError> {
    let block = root.field_path("marker groups").and_then(|f| f.as_block())
        .ok_or(JmsError::MissingField("marker groups"))?;
    let mut out = Vec::new();
    for i in 0..block.len() {
        let g = block.element(i).unwrap();
        let group_name = read_string_id(&g, "name").unwrap_or_default();
        let inner = match g.field("markers").and_then(|f| f.as_block()) {
            Some(b) => b, None => continue,
        };
        for j in 0..inner.len() {
            let m = inner.element(j).unwrap();
            out.push(JmsMarker {
                name: group_name.clone(),
                node_index: read_int_any(&m, "node index").unwrap_or(-1) as i16,
                rotation: read_quat(&m, "rotation"),
                translation: scale_point(read_point3d(&m, "translation")),
                radius: -1.0,
            });
        }
    }
    Ok(out)
}

/// Region × permutation walker that builds:
/// - the JMS material list (one per unique `(shader, perm-region)` cell)
/// - a `(mesh_index, part_index) → jms_material_index` lookup
/// - the mesh-emit order (only meshes referenced by some `(region, perm)`)
fn build_materials(root: &TagStruct<'_>)
    -> Result<(Vec<JmsMaterial>, HashMap<(usize, usize), i32>, Vec<usize>), JmsError>
{
    let mats_block = root.field_path("materials").and_then(|f| f.as_block())
        .ok_or(JmsError::MissingField("materials"))?;
    let regions_block = root.field_path("regions").and_then(|f| f.as_block())
        .ok_or(JmsError::MissingField("regions"))?;
    let meshes = root.field_path("render geometry/meshes").and_then(|f| f.as_block())
        .ok_or(JmsError::MissingField("render geometry/meshes"))?;

    let mut materials: Vec<JmsMaterial> = Vec::new();
    let mut part_material_map: HashMap<(usize, usize), i32> = HashMap::new();
    let mut mesh_emit_order: Vec<usize> = Vec::new();

    for ri in 0..regions_block.len() {
        let region = regions_block.element(ri).unwrap();
        let region_name = read_string_id(&region, "name").unwrap_or_default();
        let perms = match region.field("permutations").and_then(|f| f.as_block()) {
            Some(b) => b, None => continue,
        };
        for pi in 0..perms.len() {
            let perm = perms.element(pi).unwrap();
            let perm_name = read_string_id(&perm, "name").unwrap_or_default();
            let mesh_idx = read_int_any(&perm, "mesh index").unwrap_or(-1);
            let mesh_count = read_int_any(&perm, "mesh count").unwrap_or(0);
            if mesh_idx < 0 || mesh_count <= 0 { continue; }
            for mi_off in 0..mesh_count as usize {
                let mi = mesh_idx as usize + mi_off;
                if mi >= meshes.len() { continue; }
                if !mesh_emit_order.contains(&mi) {
                    mesh_emit_order.push(mi);
                }
                let mesh = meshes.element(mi).unwrap();
                let parts = match mesh.field("parts").and_then(|f| f.as_block()) {
                    Some(b) => b, None => continue,
                };
                for part_i in 0..parts.len() {
                    let part = parts.element(part_i).unwrap();
                    let shader_idx = read_int_any(&part, "render method index").unwrap_or(0);
                    let shader_name = if shader_idx >= 0 && (shader_idx as usize) < mats_block.len() {
                        let m = mats_block.element(shader_idx as usize).unwrap();
                        let path = read_tag_ref_path(&m, "render method").unwrap_or_default();
                        Path::new(&path.replace('\\', "/"))
                            .file_stem().and_then(|s| s.to_str()).unwrap_or("default").to_owned()
                    } else {
                        "default".to_owned()
                    };
                    let cell_label = format!("{} {}", perm_name, region_name);
                    let jms_idx = match materials.iter().position(|m|
                        m.name == shader_name && m.material_name.ends_with(&cell_label)
                    ) {
                        Some(idx) => idx as i32,
                        None => {
                            let slot = materials.len() + 1;
                            materials.push(JmsMaterial {
                                name: shader_name,
                                material_name: format!("({}) {}", slot, cell_label),
                            });
                            (materials.len() - 1) as i32
                        }
                    };
                    part_material_map.insert((mi, part_i), jms_idx);
                }
            }
        }
    }
    Ok((materials, part_material_map, mesh_emit_order))
}

fn build_geometry(
    root: &TagStruct<'_>,
    part_material_map: &HashMap<(usize, usize), i32>,
    mesh_emit_order: &[usize],
    bounds: &CompressionBounds,
) -> Result<(Vec<JmsVertex>, Vec<JmsTriangle>), JmsError> {
    let pmt_block = root.field_path("render geometry/per mesh temporary")
        .and_then(|f| f.as_block())
        .ok_or(JmsError::MissingField("render geometry/per mesh temporary"))?;
    let meshes_block = root.field_path("render geometry/meshes")
        .and_then(|f| f.as_block())
        .ok_or(JmsError::MissingField("render geometry/meshes"))?;

    let mut vertices: Vec<JmsVertex> = Vec::new();
    let mut triangles: Vec<JmsTriangle> = Vec::new();

    for &mi in mesh_emit_order {
        if mi >= pmt_block.len() { continue; }
        let pmt = pmt_block.element(mi).unwrap();
        let mesh = meshes_block.element(mi).unwrap();

        // Defensive rigid fallback (see crate-level doc note).
        let vt = mesh.field("vertex type").and_then(|f| f.value()).map(|v| match v {
            TagFieldData::CharEnum { value, .. } => value as i32, _ => -1,
        }).unwrap_or(-1);
        let rigid_fallback_node = if matches!(vt, 1 | 5) {
            read_int_any(&mesh, "rigid node index").map(|v| v as i16).filter(|&v| v >= 0)
        } else { None };

        let raw_v = pmt.field("raw vertices").and_then(|f| f.as_block())
            .ok_or(JmsError::MissingField("per mesh temporary[i]/raw vertices"))?;
        let raw_i = pmt.field("raw indices").and_then(|f| f.as_block())
            .ok_or(JmsError::MissingField("per mesh temporary[i]/raw indices"))?;
        let indices: Vec<u16> = (0..raw_i.len())
            .filter_map(|k| raw_i.element(k))
            .map(|e| read_int_any(&e, "word").unwrap_or(0) as u16)
            .collect();

        // Default to "triangle strip" — what every MCC render mesh
        // observed uses. The schema enum value 5 = triangle strip.
        let is_strip = mesh.field("index buffer type")
            .and_then(|f| f.value())
            .map(|v| matches!(v, TagFieldData::CharEnum { name: Some(n), .. } if n == "triangle strip"))
            .unwrap_or(true);

        let parts = mesh.field("parts").and_then(|f| f.as_block())
            .ok_or(JmsError::MissingField("meshes[i]/parts"))?;
        for pi in 0..parts.len() {
            let part = parts.element(pi).unwrap();
            let material_index = part_material_map.get(&(mi, pi)).copied().unwrap_or(0);
            // `index start` / `index count` are schema-typed as
            // `short integer` (i16) but functionally unsigned u16 —
            // strips spanning more than 32767 indices wrap into
            // negative i16 territory. Reinterpret the low 16 bits as
            // u16 to recover the real offset. A genuine "no
            // geometry" sentinel would be -1 (u16 0xFFFF), which is
            // rejected by the bounds check below since 0xFFFF would
            // exceed any plausible strip length.
            let start_i = read_int_any(&part, "index start").unwrap_or(0);
            let count_i = read_int_any(&part, "index count").unwrap_or(0);
            if count_i <= 0 { continue; }
            let start = (start_i as i16 as u16) as usize;
            let count = count_i as usize;
            if start >= indices.len() { continue; }
            let end = (start + count).min(indices.len());
            let part_indices = &indices[start..end];

            let tris: Vec<(u16, u16, u16)> = if is_strip {
                strip_to_list(part_indices)
            } else {
                part_indices.chunks_exact(3).map(|c| (c[0], c[1], c[2])).collect()
            };

            for (a, b, c) in tris {
                let base = vertices.len() as u32;
                for vi in [a, b, c] {
                    let Some(v) = raw_v.element(vi as usize) else { continue; };
                    let mut jv = read_vertex(&v, bounds);
                    if jv.node_sets.is_empty() {
                        if let Some(node) = rigid_fallback_node {
                            jv.node_sets.push((node, 1.0));
                        }
                    }
                    vertices.push(jv);
                }
                triangles.push(JmsTriangle {
                    material: material_index,
                    v: [base, base + 1, base + 2],
                });
            }
        }
    }
    Ok((vertices, triangles))
}

// ---- raw_vertex_block reader (CompressionBounds + readers in crate::geometry) ----

fn read_vertex(v: &TagStruct<'_>, bounds: &CompressionBounds) -> JmsVertex {
    let raw_pos = read_point3d(v, "position");
    let position = scale_point(bounds.decompress_position(raw_pos));
    let normal = read_point3d(v, "normal");
    let raw_uv = match v.field("texcoord").and_then(|f| f.value()) {
        Some(TagFieldData::RealPoint2d(p)) => [p.x, p.y], _ => [0.0, 0.0],
    };
    let texcoord = bounds.decompress_texcoord(raw_uv);
    let mut node_sets = Vec::with_capacity(4);
    if let (Some(idx_arr), Some(wt_arr)) = (
        v.field("node indices").and_then(|f| f.as_array()),
        v.field("node weights").and_then(|f| f.as_array()),
    ) {
        for k in 0..idx_arr.len().min(wt_arr.len()) {
            let idx_e = idx_arr.element(k).unwrap();
            let wt_e = wt_arr.element(k).unwrap();
            let idx = idx_e.fields().next().and_then(|f| f.value())
                .and_then(|v| if let TagFieldData::CharInteger(c) = v { Some(c as i16) } else { None })
                .unwrap_or(-1);
            let wt = wt_e.fields().next().and_then(|f| f.value())
                .and_then(|v| if let TagFieldData::Real(r) = v { Some(r) } else { None })
                .unwrap_or(0.0);
            if wt > 0.0 { node_sets.push((idx, wt)); }
        }
    }
    JmsVertex {
        position, normal, node_sets,
        uvs: vec![[texcoord[0], 1.0 - texcoord[1]]],
    }
}

// ---- jms-specific helpers ----

/// Local field reader: maps every `*BlockIndex` variant to a clamped
/// i16 (with -1 for null/unset). Used for JMS's `parent` index where
/// the JMS format only supports 16-bit signed.
fn read_block_index(s: &TagStruct<'_>, name: &str) -> i16 {
    match s.field(name).and_then(|f| f.value()) {
        Some(TagFieldData::CharBlockIndex(v)) => v as i16,
        Some(TagFieldData::ShortBlockIndex(v)) => v,
        Some(TagFieldData::LongBlockIndex(v)) => v as i16,
        _ => -1,
    }
}

// ---- writer helpers ----

fn write_floats<W: Write>(w: &mut W, values: &[f32]) -> io::Result<()> {
    for (i, v) in values.iter().enumerate() {
        let v = if *v == -0.0 { 0.0 } else { *v };
        if i + 1 < values.len() { write!(w, "{:.10}\t", v)?; }
        else                    { writeln!(w, "{:.10}", v)?; }
    }
    Ok(())
}

const EMPTY_SECTIONS_TRAILING: &[(&str, &[&str])] = &[
    ("CAR_WHEEL", &["<name>", "<chassis index>", "<wheel index>", "<chassis transform>", "<wheel transform>", "<suspension transform>", "<suspension min limit>", "<suspension max limit>"]),
    ("POINT_TO_POINT", &["<name>", "<body A index>", "<body B index>", "<body A transform>", "<body B transform>", "<constraint type>", "<x min>", "<x max>", "<y min>", "<y max>", "<z min>", "<z max>", "<spring length>"]),
    ("PRISMATIC", &["<name>", "<body A index>", "<body B index>", "<body A transform>", "<body B transform>", "<is limited>", "<friction limit>", "<min limit>", "<max limit>"]),
    ("BOUNDING SPHERE", &["<translation <x,y,z>>", "<radius>"]),
    ("SKYLIGHT", &["<direction <x,y,z>>", "<radiant intensity <x,y,z>>", "<solid angle>"]),
];
