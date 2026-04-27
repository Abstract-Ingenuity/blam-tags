//! JMA-family text export.
//!
//! [`Pose::write_jma`] serializes a composed pose plus optional
//! movement data into one of the JMA-family text formats — JMM
//! (base), JMA (dx/dy), JMT (dx/dy/dyaw), JMZ (dx/dy/dz/dyaw),
//! JMO (overlay), JMR (replacement), or JMW (world-relative).
//! [`JmaKind::from_metadata`] picks the right kind from the
//! animation's `animation type` × `frame info type` schema fields.
//!
//! Halo→JMA conventions applied here:
//! - **Translation `× 100`** (Halo world-units → JMA centimeter).
//! - **Quaternion conjugate** serialization: JMA writes
//!   `(-i, -j, -k, w)`.
//! - **World-space movement composition** for JMA/JMT/JMZ — the tag
//!   stores per-frame deltas in local space; we rotate `dx/dy` by
//!   the accumulated yaw before applying this frame's `dyaw`. Per
//!   Foundry commit `850d680d`.

use super::{MovementData, Pose, Skeleton};

/// JMA-family file extension — picked from the animation's
/// `animation type` × `frame info type`.
#[derive(Debug, Clone, Copy)]
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
    /// World-relative (no movement).
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
    /// Defaults to JMM when no movement data and base-type.
    pub fn from_metadata(animation_type: Option<&str>, frame_info_type: Option<&str>) -> Self {
        match animation_type.unwrap_or("base") {
            "overlay" => return Self::Jmo,
            "replacement" => return Self::Jmr,
            "world" => return Self::Jmw,
            _ => {}
        }
        match frame_info_type.unwrap_or("none") {
            "dx,dy" => Self::Jma,
            "dx,dy,dyaw" => Self::Jmt,
            "dx,dy,dz,dyaw" | "dx,dy,dz,dangle_axis" => Self::Jmz,
            _ => Self::Jmm,
        }
    }

    /// Whether this kind needs per-frame movement-data lines after
    /// the per-bone transforms. JMM/JMW/JMO/JMR don't; JMA/JMT/JMZ do.
    pub fn has_movement_data(self) -> bool {
        matches!(self, Self::Jma | Self::Jmt | Self::Jmz)
    }

    /// Number of float components written per movement-data frame
    /// (`0` when [`Self::has_movement_data`] is false).
    pub fn movement_components(self) -> usize {
        match self {
            Self::Jma => 2, // dx, dy
            Self::Jmt => 3, // dx, dy, dyaw
            Self::Jmz => 4, // dx, dy, dz, dyaw
            _ => 0,
        }
    }
}

impl Pose {
    /// Write this pose as a JMA-family text file (.JMM/.JMA/.JMT/etc).
    /// Applies the JMA-side conventions — translation `× 100` (Halo
    /// world-units → JMA centimeter convention) and quaternion
    /// **conjugate** serialization (JMA writes `(-i, -j, -k, w)`).
    ///
    /// `movement` carries per-frame root deltas in **local space** as
    /// stored in the tag. For movement-bearing kinds (JMA/JMT/JMZ)
    /// this writer composes them into **world space** at write time
    /// per Foundry commit `850d680d`: dx/dy rotate by accumulated yaw
    /// before the dyaw delta is applied. JMM/JMW/JMO/JMR ignore
    /// `movement` entirely.
    pub fn write_jma<W: std::io::Write>(
        &self,
        writer: &mut W,
        skeleton: &Skeleton,
        node_list_checksum: i32,
        kind: JmaKind,
        actor_name: &str,
        movement: Option<&MovementData>,
    ) -> std::io::Result<()> {
        // Header.
        writeln!(writer, "16392")?;
        writeln!(writer, "{}", self.frames.len())?;
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

        // Frames + per-frame movement (composed local→world per Foundry).
        let mut accumulated_yaw = 0.0f32;
        for (frame_idx, frame) in self.frames.iter().enumerate() {
            for transform in frame {
                let t = transform.translation;
                // Halo world-units → JMA "centimeter" convention.
                write_floats(writer, &[t.x * 100.0, t.y * 100.0, t.z * 100.0])?;
                let q = transform.rotation;
                // JMA wants the conjugate: negate i,j,k, keep w.
                write_floats(writer, &[-q.i, -q.j, -q.k, q.w])?;
                write_floats(writer, &[transform.scale])?;
            }
            if kind.has_movement_data() {
                let local = movement
                    .and_then(|m| m.frames.get(frame_idx))
                    .copied()
                    .unwrap_or_default();
                // Rotate dx/dy from local space into world by the
                // yaw accumulated up through the previous frame
                // (Foundry's order: rotate first, then accumulate
                // this frame's dyaw).
                let cos_y = accumulated_yaw.cos();
                let sin_y = accumulated_yaw.sin();
                let world_dx = local.dx * cos_y - local.dy * sin_y;
                let world_dy = local.dx * sin_y + local.dy * cos_y;
                // JMA-side translation scale is ×100 (cm convention).
                let row: Vec<f32> = match kind {
                    JmaKind::Jma => vec![world_dx * 100.0, world_dy * 100.0],
                    JmaKind::Jmt => vec![world_dx * 100.0, world_dy * 100.0, local.dyaw],
                    JmaKind::Jmz => vec![world_dx * 100.0, world_dy * 100.0, local.dz * 100.0, local.dyaw],
                    _ => Vec::new(),
                };
                write_floats(writer, &row)?;
                accumulated_yaw += local.dyaw;
            }
        }
        writer.flush()?;
        Ok(())
    }
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
