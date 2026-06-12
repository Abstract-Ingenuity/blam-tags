//! Animation graph traversal — `definitions/animations[]` provides
//! per-animation metadata (already exposed via [`super::Animation`]),
//! and `content/modes[]` is a tree that names which animation plays
//! for which (mode, weapon_class, weapon_type, state) tuple.
//!
//! ## Structure (MCC, jmad)
//!
//! ```text
//! content
//!   modes[]                 // e.g. "any", "stand", "crouch"
//!     label: string_id
//!     weapon_class[]        // "any" or specific weapons
//!       label: string_id
//!       weapon_type[]
//!         label: string_id
//!         actions[]         // states like "idle", "aim", "fire"
//!           label: string_id
//!           animation       // graph_index + animation block index
//!         overlays[]        // additive layered animations
//!         death_and_damage[]
//!         transitions[]     // cross-state blend animations
//!       weapon_ik[]
//!     mode_ik[]
//!     foot_defaults[]
//! ```
//!
//! For static scenery and most non-combat objects, only the `actions`
//! block matters — the runtime picks an action by name (e.g. "idle")
//! to drive the c_animation_manager's state channel.
//!
//! Reference: `Ares/source/animations/animation_definitions.h:448-462`
//! (older — newer MCC has the same shape with renamed fields).

use crate::api::{TagBlock, TagStruct};
use crate::file::TagFile;

/// Result of walking a jmad's `content/modes[]` tree. Names are
/// resolved string-ids; -1 indices are kept as `None`.
#[derive(Debug, Clone, Default)]
pub struct AnimationGraph {
    pub modes: Vec<GraphMode>,
}

impl AnimationGraph {
    /// Build the graph from a `model_animation_graph` tag.
    pub fn from_tag(tag: &TagFile) -> Self {
        Self::from_struct(&tag.root())
    }

    /// Build the graph from the tag's root struct.
    pub fn from_struct(root: &TagStruct<'_>) -> Self {
        let modes = root
            .field("content")
            .and_then(|f| f.as_struct())
            .and_then(|content| content.field("modes").and_then(|f| f.as_block()))
            .map(|b| read_block_vec(&b, GraphMode::from_struct))
            .unwrap_or_default();
        Self { modes }
    }

    /// Look up an action animation by walking the (mode, weapon_class,
    /// weapon_type, set, action) tuple. Each scope component falls back
    /// to "any" if the exact name doesn't match (mirrors Halo's wildcard
    /// resolution). Returns `None` if no action matches at all.
    ///
    /// The `set` level only exists in Reach+ (mode→class→type→**set**→
    /// actions); for H3 a single implicit `"any"` set is synthesized
    /// from the weapon type's direct `actions`, so passing `set = "any"`
    /// works for both engines. After the exact/`any` set, this also
    /// falls back to *any* set that contains the action — Reach sets are
    /// usually labeled `"any"` but this keeps resolution robust.
    ///
    /// Use `mode/weapon_class/weapon_type/set = "any"` for non-combat
    /// objects (scenery, machines, etc.).
    pub fn find_action(
        &self,
        mode: &str,
        weapon_class: &str,
        weapon_type: &str,
        set: &str,
        action: &str,
    ) -> Option<&GraphActionAnimation> {
        let m = self
            .modes
            .iter()
            .find(|m| m.label == mode)
            .or_else(|| self.modes.iter().find(|m| m.label == "any"))?;
        let wc = m
            .weapon_classes
            .iter()
            .find(|w| w.label == weapon_class)
            .or_else(|| m.weapon_classes.iter().find(|w| w.label == "any"))?;
        let wt = wc
            .weapon_types
            .iter()
            .find(|w| w.label == weapon_type)
            .or_else(|| wc.weapon_types.iter().find(|w| w.label == "any"))?;

        fn pick<'g>(s: &'g GraphSet, action: &str) -> Option<&'g GraphActionAnimation> {
            s.actions.iter().find(|a| a.label == action).map(|a| &a.animation)
        }
        // Exact set, then the "any" set, then any set carrying the action.
        wt.sets.iter().find(|s| s.label == set).and_then(|s| pick(s, action))
            .or_else(|| wt.sets.iter().find(|s| s.label == "any").and_then(|s| pick(s, action)))
            .or_else(|| wt.sets.iter().find_map(|s| pick(s, action)))
    }

    /// Find the first action available anywhere in the tree. Useful for
    /// "just play SOMETHING" — most scenery has exactly one action and
    /// we don't care which.
    pub fn first_action(&self) -> Option<&GraphActionAnimation> {
        for mode in &self.modes {
            for wc in &mode.weapon_classes {
                for wt in &wc.weapon_types {
                    for set in &wt.sets {
                        if let Some(action) = set.actions.first() {
                            return Some(&action.animation);
                        }
                    }
                }
            }
        }
        None
    }
}

/// One entry in `content/modes[]`. e.g. mode "any" or "stand".
#[derive(Debug, Clone, Default)]
pub struct GraphMode {
    pub label: String,
    pub weapon_classes: Vec<GraphWeaponClass>,
}

impl GraphMode {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            label: s.read_string_id("label").unwrap_or_default(),
            weapon_classes: s
                .field("weapon class")
                .and_then(|f| f.as_block())
                .map(|b| read_block_vec(&b, GraphWeaponClass::from_struct))
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GraphWeaponClass {
    pub label: String,
    pub weapon_types: Vec<GraphWeaponType>,
}

impl GraphWeaponClass {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            label: s.read_string_id("label").unwrap_or_default(),
            weapon_types: s
                .field("weapon type")
                .and_then(|f| f.as_block())
                .map(|b| read_block_vec(&b, GraphWeaponType::from_struct))
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GraphWeaponType {
    pub label: String,
    /// Animation **sets**. Reach+ has an explicit `sets[]` level
    /// (mode→class→type→set→actions); for H3 — which stores `actions`
    /// directly on the weapon type — a single implicit set labeled
    /// `"any"` is synthesized so both engines walk the same shape.
    pub sets: Vec<GraphSet>,
}

impl GraphWeaponType {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let label = s.read_string_id("label").unwrap_or_default();
        let sets = match s.field("sets").and_then(|f| f.as_block()) {
            // Reach+: actions/overlays/transitions live under sets[].
            Some(b) => read_block_vec(&b, GraphSet::from_struct),
            // H3: synthesize one implicit "any" set from direct fields.
            None => vec![GraphSet::from_weapon_type(s)],
        };
        Self { label, sets }
    }
}

/// One animation **set** under a weapon type. Carries the actual
/// state→animation bindings. Real block in Reach+; synthesized
/// (label `"any"`) from the weapon type's direct fields in H3.
#[derive(Debug, Clone, Default)]
pub struct GraphSet {
    pub label: String,
    pub actions: Vec<GraphAction>,
    pub overlays: Vec<GraphAction>,
    pub transitions: Vec<GraphTransition>,
}

impl GraphSet {
    /// Reach+ `animation_set_block` element.
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            label: s.read_string_id("label").unwrap_or_default(),
            actions: action_block(s, "actions"),
            // Reach names the block "overlay animations"; H3 "overlays".
            overlays: {
                let o = action_block(s, "overlay animations");
                if o.is_empty() { action_block(s, "overlays") } else { o }
            },
            transitions: s
                .field("transitions")
                .and_then(|f| f.as_block())
                .map(|b| read_block_vec(&b, GraphTransition::from_struct))
                .unwrap_or_default(),
        }
    }

    /// H3: the weapon type itself carries the action blocks; wrap them
    /// in a single implicit `"any"` set.
    fn from_weapon_type(s: &TagStruct<'_>) -> Self {
        Self {
            label: "any".to_string(),
            actions: action_block(s, "actions"),
            overlays: action_block(s, "overlays"),
            transitions: s
                .field("transitions")
                .and_then(|f| f.as_block())
                .map(|b| read_block_vec(&b, GraphTransition::from_struct))
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GraphAction {
    pub label: String,
    pub animation: GraphActionAnimation,
}

impl GraphAction {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let animation = s
            .field("animation")
            .and_then(|f| f.as_struct())
            .map(|st| GraphActionAnimation::from_struct(&st))
            .unwrap_or_default();
        Self {
            label: s.read_string_id("label").unwrap_or_default(),
            animation,
        }
    }
}

/// Reference to an animation entry — either local
/// (`graph_index = -1`, `animation` indexes into our own
/// [`super::Animation::iter`]) or inherited (positive `graph_index`
/// references a parent animation_graph chain entry).
#[derive(Debug, Clone, Copy, Default)]
pub struct GraphActionAnimation {
    /// `graph_index` — -1 = local. Otherwise an index into the parent
    /// graph chain (Halo's animation inheritance system).
    pub graph_index: i16,
    /// `animation` — block index into `definitions/animations[]`.
    /// `-1` if no animation is bound.
    pub animation_index: i16,
}

impl GraphActionAnimation {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            graph_index: s.read_int_any("graph index").unwrap_or(-1) as i16,
            animation_index: s.read_block_index("animation"),
        }
    }

    /// `true` when the reference is local (resolves via this jmad's
    /// own animations block).
    pub fn is_local(&self) -> bool {
        self.graph_index < 0
    }
}

/// One transition between two states — a transition animation
/// (e.g. "idle_to_run") referenced by source + destination state names.
#[derive(Debug, Clone, Default)]
pub struct GraphTransition {
    /// `state name` — destination state.
    pub destination_state: String,
    /// `animation` block index into `definitions/animations[]`.
    pub animation: GraphActionAnimation,
}

impl GraphTransition {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            destination_state: s.read_string_id("state name").unwrap_or_default(),
            animation: s
                .field("animation")
                .and_then(|f| f.as_struct())
                .map(|st| GraphActionAnimation::from_struct(&st))
                .unwrap_or_default(),
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Read a named child block of `s` as a `Vec<GraphAction>`, or empty if
/// the field is missing.
fn action_block(s: &TagStruct<'_>, name: &str) -> Vec<GraphAction> {
    s.field(name)
        .and_then(|f| f.as_block())
        .map(|b| read_block_vec(&b, GraphAction::from_struct))
        .unwrap_or_default()
}

fn read_block_vec<T, F>(block: &TagBlock<'_>, f: F) -> Vec<T>
where
    F: Fn(&TagStruct<'_>) -> T,
{
    let mut out = Vec::with_capacity(block.len());
    for i in 0..block.len() {
        if let Some(elem) = block.element(i) {
            out.push(f(&elem));
        }
    }
    out
}
