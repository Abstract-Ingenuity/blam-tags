//! JMA-family text export.
//!
//! [`Pose::write_jma`] serializes a composed pose into one of the
//! JMA-family text formats — JMM (base), JMA (dx/dy), JMT
//! (dx/dy/dyaw), JMZ (dx/dy/dz/dyaw), JMO (overlay), JMR
//! (replacement), or JMW (world-relative). [`JmaKind::from_metadata`]
//! picks the right kind from the animation's `animation type` ×
//! `frame info type` × `internal flags / world relative` schema fields.
//!
//! Halo→JMA conventions applied here:
//! - **Translation `× 100`** (Halo world-units → JMA centimeter).
//! - **Quaternion conjugate** serialization: JMA writes
//!   `(-i, -j, -k, w)`.
//! - **No separate movement section**. The H3 JMA spec (version
//!   16392) is just `header + nodes + per-frame per-bone transforms`
//!   — no trailing per-frame movement table. Movement deltas are
//!   instead **folded into the root bone (index 0)** at write time:
//!   `dx/dy` rotate from local to world space by the accumulated
//!   yaw (per Foundry commit `850d680d` — fixes TagTool's
//!   actor-slides-backwards bug on yawed-during-walk anims), then
//!   the running translation+yaw is added/multiplied onto the root
//!   bone's pose. Verified against `General-101/Halo-Asset-Blender-
//!   Development-Toolset` (HABT) `process_file_retail.py` and
//!   TagTool's `Animation.Process()`.
//! - **Type-specific frame layout**:
//!     - Base (JMM/JMA/JMT/JMZ) and JMW: codec frames + a duplicated
//!       trailing frame (Tool expects a held terminal pose for
//!       blending into the next anim).
//!     - Replacement (JMR): a leading rest-pose frame, then codec
//!       frames. Tool subtracts the leading frame at re-build time
//!       to derive deltas.
//!     - Overlay (JMO): a leading rest-pose frame, then per-frame
//!       composed `(rest_rotation × codec_delta_rotation,
//!       rest_translation + codec_delta_translation)`.
//!
//!   In all cases the final on-disk frame count is `codec_count + 1`.

use crate::math::{RealPoint3d, RealQuaternion};

use super::{MovementData, MovementFrame, NodeTransform, Pose, Skeleton};

/// JMA-family file extension — picked from the animation's
/// `animation type` × `frame info type` × world-relative flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JmaKind {
    /// Base animation, no movement data.
    Jmm,
    /// Base + dx/dy.
    Jma,
    /// Base + dx/dy/dyaw.
    Jmt,
    /// Base + dx/dy/dz/dyaw.
    Jmz,
    /// Overlay animation.
    Jmo,
    /// Replacement animation.
    Jmr,
    /// World-relative (no movement, but selected via
    /// `internal_flags / world relative` rather than `animation type`).
    Jmw,
}

impl JmaKind {
    /// Uppercase JMA-family file extension (no leading dot).
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jmm => "JMM", Self::Jma => "JMA", Self::Jmt => "JMT", Self::Jmz => "JMZ",
            Self::Jmo => "JMO", Self::Jmr => "JMR", Self::Jmw => "JMW",
        }
    }

    /// Pick the right JMA-family kind from the per-animation metadata.
    ///
    /// `world_relative` is the `internal flags / world relative` bit
    /// from the jmad's `animations[i]` block — JMW is base + this
    /// bit, NOT a separate `animation_type` enum value (the schema
    /// only has `base / overlay / replacement`). Mirrors TagTool's
    /// `GetAnimationExtension(type, frame_info, worldRelative)` and
    /// Foundry's `internal_flags.TestBit("world relative")`.
    pub fn from_metadata(
        animation_type: Option<&str>,
        frame_info_type: Option<&str>,
        world_relative: bool,
    ) -> Self {
        match animation_type.unwrap_or("base") {
            "overlay" => return Self::Jmo,
            "replacement" => return Self::Jmr,
            _ => {}
        }
        if world_relative {
            return Self::Jmw;
        }
        match frame_info_type.unwrap_or("none") {
            "dx,dy" => Self::Jma,
            "dx,dy,dyaw" => Self::Jmt,
            "dx,dy,dz,dyaw" | "dx,dy,dz,dangle_axis" => Self::Jmz,
            _ => Self::Jmm,
        }
    }

    /// Whether this kind accumulates per-frame movement deltas into
    /// the root bone at write time. Only the base kinds with movement
    /// data (`Jma / Jmt / Jmz`) do; the rest emit per-bone transforms
    /// without any movement folding.
    pub fn folds_movement(self) -> bool {
        matches!(self, Self::Jma | Self::Jmt | Self::Jmz)
    }

    /// `JMR / JMO` prepend a leading rest-pose frame so Tool's
    /// importer can derive deltas/composition cleanly during re-build.
    pub fn prepends_rest_pose(self) -> bool {
        matches!(self, Self::Jmo | Self::Jmr)
    }

    /// Base kinds (`JMM / JMA / JMT / JMZ`) and `JMW` append a
    /// duplicated trailing frame as the held terminal pose.
    pub fn appends_held_frame(self) -> bool {
        matches!(self, Self::Jmm | Self::Jma | Self::Jmt | Self::Jmz | Self::Jmw)
    }

    /// `JMO` composes per-frame `(rest × codec_delta)` rotations and
    /// `(rest + codec_delta)` translations. The codec values are
    /// deltas-from-rest; the writer combines them with the rest pose
    /// so the on-disk JMA carries world poses Tool can re-import.
    pub fn composes_overlay(self) -> bool {
        matches!(self, Self::Jmo)
    }
}

impl Pose {
    /// Write this pose as a JMA-family text file (`.JMM/.JMA/.JMT/...`).
    /// See the [module docs](self) for the full layout convention.
    ///
    /// `defaults` is the per-skeleton-bone rest pose (typically built
    /// from the render_model's `nodes[]` defaults plus the jmad's
    /// `additional node data` fallback). It supplies the rest-pose
    /// values for the leading frame (JMR/JMO), the base pose for
    /// overlay composition (JMO), and is the source of identity-
    /// vs-rest ambiguity-resolution callers should configure when
    /// building the input `Pose` (overlay anims should be built with
    /// identity defaults so unflagged-bone composition produces the
    /// rest pose, not double-rest).
    ///
    /// `movement` carries per-frame root deltas in **local space**.
    /// For movement-bearing kinds (JMA/JMT/JMZ) the writer rotates
    /// `dx/dy` by the accumulated yaw before adding it to the root
    /// bone's pose — Foundry-style local→world fix per commit
    /// `850d680d`. JMM/JMW/JMO/JMR ignore `movement` entirely.
    #[allow(clippy::too_many_arguments)] // each arg is load-bearing; bundling adds a builder type for one call site
    pub fn write_jma<W: std::io::Write>(
        &self,
        writer: &mut W,
        skeleton: &Skeleton,
        defaults: &[NodeTransform],
        node_list_checksum: i32,
        kind: JmaKind,
        actor_name: &str,
        movement: Option<&MovementData>,
    ) -> std::io::Result<()> {
        let codec_count = self.frames.len();
        // Tool re-importers expect codec_count + 1 frames: a leading
        // rest pose for JMR/JMO, or a held trailing frame for the
        // base kinds and JMW.
        let total_frames = if codec_count == 0 { 0 } else { codec_count + 1 };

        // Header.
        writeln!(writer, "16392")?;
        writeln!(writer, "{total_frames}")?;
        writeln!(writer, "30")?;
        writeln!(writer, "1")?;
        writeln!(writer, "{actor_name}")?;
        writeln!(writer, "{}", skeleton.len())?;
        writeln!(writer, "{node_list_checksum}")?;

        // Skeleton.
        for node in &skeleton.nodes {
            writeln!(writer, "{}", node.name)?;
            writeln!(writer, "{}", node.first_child)?;
            writeln!(writer, "{}", node.next_sibling)?;
        }

        if codec_count == 0 {
            return writer.flush();
        }

        // Optional leading rest-pose frame for JMR/JMO. Movement
        // accumulation hasn't started yet, so the rest pose is
        // emitted unmodified.
        if kind.prepends_rest_pose() {
            for transform in defaults {
                write_transform(writer, *transform)?;
            }
        }

        // Movement folding state — accumulated through every codec
        // frame. The trailing held frame (when present) inherits the
        // final state with no further accumulation.
        let mut accumulated_translation = RealPoint3d::default();
        let mut accumulated_yaw = 0.0f32;

        for (frame_idx, frame) in self.frames.iter().enumerate() {
            // Advance movement accumulators FIRST so this frame's
            // root bone reflects the new position. (Order matters:
            // frame 0 already shows the first delta, not zero.)
            if kind.folds_movement() {
                let local = movement
                    .and_then(|m| m.frames.get(frame_idx))
                    .copied()
                    .unwrap_or_default();
                advance_movement(&mut accumulated_translation, &mut accumulated_yaw, &local);
            }

            for (bone_idx, transform) in frame.iter().enumerate() {
                let composed = compose_frame_bone(
                    *transform,
                    bone_idx,
                    defaults,
                    accumulated_translation,
                    accumulated_yaw,
                    kind,
                );
                write_transform(writer, composed)?;
            }
        }

        // Trailing held frame — duplicate of the last codec frame's
        // (already-composed-where-relevant) values. Movement state
        // freezes at the final accumulated translation/yaw.
        if kind.appends_held_frame() {
            let last_idx = codec_count - 1;
            let last_frame = &self.frames[last_idx];
            for (bone_idx, transform) in last_frame.iter().enumerate() {
                let composed = compose_frame_bone(
                    *transform,
                    bone_idx,
                    defaults,
                    accumulated_translation,
                    accumulated_yaw,
                    kind,
                );
                write_transform(writer, composed)?;
            }
        }

        writer.flush()?;
        Ok(())
    }
}

/// Apply this frame's local movement delta — translation rotated
/// into world space by the previously-accumulated yaw, then yaw
/// itself accumulated. Foundry commit `850d680d` order: rotate
/// first, accumulate after.
fn advance_movement(
    translation: &mut RealPoint3d,
    accumulated_yaw: &mut f32,
    local: &MovementFrame,
) {
    let (cos_y, sin_y) = (accumulated_yaw.cos(), accumulated_yaw.sin());
    let world_dx = local.dx * cos_y - local.dy * sin_y;
    let world_dy = local.dx * sin_y + local.dy * cos_y;
    translation.x += world_dx;
    translation.y += world_dy;
    translation.z += local.dz;
    *accumulated_yaw += local.dyaw;
}

/// Apply per-bone composition for the given JMA kind. Returns the
/// transform that should be written to disk (post-conjugate, post-
/// scale-by-100 are still applied by [`write_transform`]).
fn compose_frame_bone(
    transform: NodeTransform,
    bone_idx: usize,
    defaults: &[NodeTransform],
    accumulated_translation: RealPoint3d,
    accumulated_yaw: f32,
    kind: JmaKind,
) -> NodeTransform {
    let mut t = transform.translation;
    let mut q = transform.rotation;
    let mut s = transform.scale;

    if kind.composes_overlay() {
        // Codec values are deltas from the rest pose; combine with
        // the bone's rest pose to produce the world pose Tool wants.
        if let Some(base) = defaults.get(bone_idx).copied() {
            t = RealPoint3d {
                x: base.translation.x + transform.translation.x,
                y: base.translation.y + transform.translation.y,
                z: base.translation.z + transform.translation.z,
            };
            q = base.rotation * transform.rotation;
            // Overlay scale is additive: TagTool's `Animation.Overlay`
            // and Foundry's `compose_overlay_animation` agree.
            s = base.scale + transform.scale;
        }
    }

    // Movement folding lives on the root bone (index 0) and runs
    // after any per-bone composition above.
    if kind.folds_movement() && bone_idx == 0 {
        t = RealPoint3d {
            x: t.x + accumulated_translation.x,
            y: t.y + accumulated_translation.y,
            z: t.z + accumulated_translation.z,
        };
        q = yaw_quat(accumulated_yaw) * q;
    }

    NodeTransform { translation: t, rotation: q, scale: s }
}

/// Rotation around +Z axis (Halo's up) by `yaw` radians, encoded as
/// a quaternion. Used to fold accumulated movement yaw into the
/// root bone at write time.
fn yaw_quat(yaw: f32) -> RealQuaternion {
    let half = yaw * 0.5;
    RealQuaternion { i: 0.0, j: 0.0, k: half.sin(), w: half.cos() }
}

/// Write one (translation, rotation, scale) bone-frame triple in
/// JMA-on-disk format: translation `× 100` (cm convention),
/// quaternion **conjugate** (`(-i, -j, -k, w)`), scale unchanged.
fn write_transform<W: std::io::Write>(writer: &mut W, t: NodeTransform) -> std::io::Result<()> {
    let p = t.translation;
    write_floats(writer, &[p.x * 100.0, p.y * 100.0, p.z * 100.0])?;
    let q = t.rotation;
    write_floats(writer, &[-q.i, -q.j, -q.k, q.w])?;
    write_floats(writer, &[t.scale])?;
    Ok(())
}

fn write_floats<W: std::io::Write>(writer: &mut W, values: &[f32]) -> std::io::Result<()> {
    for (i, v) in values.iter().enumerate() {
        let v = if *v == -0.0 { 0.0 } else { *v };
        if i + 1 < values.len() {
            write!(writer, "{:.10}\t", v)?;
        } else {
            writeln!(writer, "{:.10}", v)?;
        }
    }
    Ok(())
}
