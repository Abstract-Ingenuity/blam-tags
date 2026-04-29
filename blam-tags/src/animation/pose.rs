//! Skeleton + Pose composition.
//!
//! [`Skeleton::from_tag`] walks the jmad's `definitions/skeleton
//! nodes` (or `resources/skeleton nodes` for older inline-layout
//! tags) into a flat list of [`SkeletonNode`]s. [`super::AnimationClip`]
//! exposes a [`pose`](super::AnimationClip::pose) method that takes a
//! `Skeleton` and resolves every (frame, bone) pair through the
//! per-component flag bitarrays — bones flagged static read from
//! `static_tracks`, bones flagged animated read the right frame from
//! `animated_tracks`, unflagged bones fall back to identity.
//!
//! The output [`Pose`] is the unit the JMA writer
//! ([`super::jma`]) consumes.

use crate::file::TagFile;
use crate::math::{RealPoint3d, RealQuaternion};

use super::{AnimationClip, BitArray, NodeFlags, TOP_LEVEL_NAMES};

//================================================================================
// Skeleton + Pose
//================================================================================

/// One node in the jmad's `definitions/skeleton nodes` block.
#[derive(Debug, Clone)]
pub struct SkeletonNode {
    pub name: String,
    /// Index into `Skeleton::nodes` of the first child, or `-1`.
    pub first_child: i16,
    /// Index of the next sibling under the same parent, or `-1`.
    pub next_sibling: i16,
    /// Index of the parent node, or `-1` for root.
    pub parent: i16,
}

/// jmad skeleton — the bone hierarchy that animations target.
#[derive(Debug, Clone)]
pub struct Skeleton {
    pub nodes: Vec<SkeletonNode>,
}

impl Skeleton {
    /// Walk `definitions/skeleton nodes` (or `resources/skeleton nodes`
    /// for older inline-layout tags) into a flat list of nodes.
    /// Returns an empty skeleton if the block is missing.
    pub fn from_tag(tag: &TagFile) -> Self {
        let root = tag.root();
        for prefix in TOP_LEVEL_NAMES {
            if let Some(block) = root
                .field_path(&format!("{prefix}/skeleton nodes"))
                .and_then(|f| f.as_block())
            {
                let mut nodes = Vec::with_capacity(block.len());
                for i in 0..block.len() {
                    let Some(elem) = block.element(i) else { continue };
                    let name = elem.read_string_id("name").unwrap_or_default();
                    let first_child = elem.read_block_index("first child node index");
                    let next_sibling = elem.read_block_index("next sibling node index");
                    let parent = elem.read_block_index("parent node index");
                    nodes.push(SkeletonNode { name, first_child, next_sibling, parent });
                }
                return Self { nodes };
            }
        }
        Self { nodes: Vec::new() }
    }

    /// Number of skeleton nodes (bones).
    pub fn len(&self) -> usize { self.nodes.len() }

    /// `true` when the tag has no skeleton nodes (e.g. inheriting jmads).
    pub fn is_empty(&self) -> bool { self.nodes.is_empty() }
}

/// One bone's transform at one frame — the unit JMA writes per
/// `(frame, node)` cell.
#[derive(Debug, Clone, Copy, Default)]
pub struct NodeTransform {
    pub rotation: RealQuaternion,
    pub translation: RealPoint3d,
    pub scale: f32,
}

impl NodeTransform {
    /// Identity transform: rotation `(0,0,0,1)`, translation
    /// `(0,0,0)`, scale `1.0`. Useful as a fallback when no rest
    /// pose is available — note `Default` is all-zeros (rotation
    /// included) which is *not* identity.
    pub const IDENTITY: Self = Self {
        rotation: RealQuaternion::IDENTITY,
        translation: RealPoint3d { x: 0.0, y: 0.0, z: 0.0 },
        scale: 1.0,
    };
}

/// Per-frame, per-bone transform table. `frames[frame_index][bone_index]`.
#[derive(Debug, Clone)]
pub struct Pose {
    pub frames: Vec<Vec<NodeTransform>>,
}

impl AnimationClip {
    /// Compose `static_tracks` + `animated_tracks` against the skeleton
    /// using the per-component `node_flags` bitarrays. Result has one
    /// `NodeTransform` per (frame, skeleton bone).
    ///
    /// `defaults` supplies the per-bone rest pose used when a bone is
    /// flagged neither static nor animated. Pass `None` for identity
    /// (legacy behavior — wrong for FP weapons and other tags whose
    /// unflagged bones rely on the render_model's `default
    /// translation` / `default rotation`); pass `Some(&[..])` with one
    /// entry per skeleton bone for the canonical render_model defaults
    /// (matches TagTool's `AnimationDefaultNodeHelper` behavior).
    pub fn pose(&self, skeleton: &Skeleton, defaults: Option<&[NodeTransform]>) -> Pose {
        let bones = skeleton.len();
        let frames_n = self.frame_count.max(1) as usize;
        let mut frames = Vec::with_capacity(frames_n);

        // Pre-resolve every bone's source: which codec stream owns each
        // component, and what the codec_node_index is.
        let resolutions: Vec<BoneResolution> = (0..bones)
            .map(|b| BoneResolution::for_bone(b, self.node_flags.as_ref()))
            .collect();

        for f in 0..frames_n {
            let mut row = Vec::with_capacity(bones);
            for (b, res) in resolutions.iter().enumerate() {
                let default = defaults
                    .and_then(|d| d.get(b))
                    .copied()
                    .unwrap_or(NodeTransform::IDENTITY);
                let rotation = pick_rotation(self, res, f).unwrap_or(default.rotation);
                let translation = pick_translation(self, res, f).unwrap_or(default.translation);
                let scale = pick_scale(self, res, f).unwrap_or(default.scale);
                row.push(NodeTransform { rotation, translation, scale });
            }
            frames.push(row);
        }
        Pose { frames }
    }
}

#[derive(Debug, Clone, Copy)]
struct BoneResolution {
    rotation: TrackSource,
    translation: TrackSource,
    scale: TrackSource,
}

#[derive(Debug, Clone, Copy)]
enum TrackSource {
    Static(usize),
    Animated(usize),
    Identity,
}

impl BoneResolution {
    fn for_bone(bone: usize, flags: Option<&NodeFlags>) -> Self {
        match flags {
            None => {
                // No flags available — fall back to "all bones use the
                // static track in skeleton order". Right for the most
                // common static-only case (mongoose-class inline
                // layouts), wrong for tagged bone subsets — but those
                // tags carry node_flags so we won't take this path.
                Self {
                    rotation: TrackSource::Static(bone),
                    translation: TrackSource::Static(bone),
                    scale: TrackSource::Static(bone),
                }
            }
            Some(f) => Self {
                rotation: pick_source(bone, &f.static_rotation, &f.animated_rotation),
                translation: pick_source(bone, &f.static_translation, &f.animated_translation),
                scale: pick_source(bone, &f.static_scale, &f.animated_scale),
            }
        }
    }
}

fn pick_source(bone: usize, static_flags: &BitArray, animated_flags: &BitArray) -> TrackSource {
    if static_flags.bit(bone) { TrackSource::Static(static_flags.popcount_below(bone)) }
    else if animated_flags.bit(bone) { TrackSource::Animated(animated_flags.popcount_below(bone)) }
    else { TrackSource::Identity }
}

fn pick_rotation(clip: &AnimationClip, res: &BoneResolution, frame: usize) -> Option<RealQuaternion> {
    match res.rotation {
        TrackSource::Static(i) => clip.static_tracks.rotations.get(i).and_then(|f| f.first()).copied(),
        TrackSource::Animated(i) => clip.animated_tracks.as_ref()
            .and_then(|t| t.rotations.get(i)).and_then(|f| f.get(frame.min(f.len() - 1))).copied(),
        TrackSource::Identity => None,
    }
}
fn pick_translation(clip: &AnimationClip, res: &BoneResolution, frame: usize) -> Option<RealPoint3d> {
    match res.translation {
        TrackSource::Static(i) => clip.static_tracks.translations.get(i).and_then(|f| f.first()).copied(),
        TrackSource::Animated(i) => clip.animated_tracks.as_ref()
            .and_then(|t| t.translations.get(i)).and_then(|f| f.get(frame.min(f.len() - 1))).copied(),
        TrackSource::Identity => None,
    }
}
fn pick_scale(clip: &AnimationClip, res: &BoneResolution, frame: usize) -> Option<f32> {
    match res.scale {
        TrackSource::Static(i) => clip.static_tracks.scales.get(i).and_then(|f| f.first()).copied(),
        TrackSource::Animated(i) => clip.animated_tracks.as_ref()
            .and_then(|t| t.scales.get(i)).and_then(|f| f.get(frame.min(f.len() - 1))).copied(),
        TrackSource::Identity => None,
    }
}
