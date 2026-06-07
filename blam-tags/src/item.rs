//! `item_struct_definition` substruct — shared parent of `.weapon`
//! (weap) and `.equipment` (eqip). `.projectile` (proj) inherits
//! directly from `obje`, NOT from item.
//!
//! Ares source: `source/items/items.h` (item_struct_definition).
//! Schema: `definitions/halo3_mcc/item.json` →
//! `item_struct_definition` (parent_tag `obje`).
//!
//! Composition mirrors the engine layout: every item tag's root has
//! an `item` substruct which embeds an `object` substruct.

use crate::api::TagStruct;
use crate::math::Bounds;
use crate::object::ObjectDefinition;
use crate::typed_enums::Flags;
use std::sync::Arc;

/// `item_definition_flags` (long_flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug,
         num_derive::FromPrimitive, num_derive::ToPrimitive,
         strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum ItemDefinitionFlags {
    // Full 4-flag H3 layout the .item-bearing tags carry. The MCC JSON
    // pruned "destroyed by explosions" + "unaffected by gravity", leaving 2
    // (and shifting "crate style collision filter" from bit 3 to bit 1).
    // Superset so the historical bits resolve by name. Discriminants follow
    // the tag's real bit positions.
    #[strum(serialize = "always maintains z up")] AlwaysMaintainsZUp = 0,
    #[strum(serialize = "destroyed by explosions")] DestroyedByExplosions = 1,
    #[strum(serialize = "unaffected by gravity")] UnaffectedByGravity = 2,
    #[strum(serialize = "crate style collision filter")] CrateStyleCollisionFilter = 3,
}

/// Walked `item_struct_definition`. Field order matches schema verbatim
/// (`item.json:64-95`).
#[derive(Debug, Clone, Default)]
pub struct ItemDefinition {
    /// Inherited `object_struct_definition` body. `Arc`-shared with
    /// any derived weapon / equipment definition.
    pub object: Arc<ObjectDefinition>,
    /// `flags` (long_flags) — item-specific.
    pub flags: Flags<ItemDefinitionFlags, u32>,

    // -- NEW hud messages --
    pub pickup_message: String,
    pub swap_message: String,
    pub pickup_message_dual: String,
    pub swap_message_dual: String,
    pub picked_up_msg: String,
    pub switch_to_msg: String,
    pub switch_to_from_ai_msg: String,
    pub notify_empty_msg: String,
    /// `private use font icon` (long_integer) — Unicode private-use
    /// codepoint for the item's icon.
    pub private_use_font_icon: i32,
    /// `private use font icon (dual)` — left-hand variant.
    pub private_use_font_icon_dual: i32,

    /// `detonation damage effect` (tag_reference).
    pub detonation_damage_effect: String,
    /// `detonation delay:seconds` (real_bounds).
    pub detonation_delay: Bounds<f32>,
    /// `detonating effect` (tag_reference) — fires while delay counts down.
    pub detonating_effect: String,
    /// `detonation effect` (tag_reference) — fires on detonation.
    pub detonation_effect: String,

    // -- Item scale settings --
    pub single_player_ground: f32,
    pub multiplayer_ground: f32,
    pub small_unit_armed: f32,
    pub small_unit_stowed: f32,
    pub medium_unit_armed: f32,
    pub medium_unit_stowed: f32,
    pub player_unit_armed: f32,
    pub player_unit_stowed: f32,
    pub large_unit_armed: f32,
    pub large_unit_stowed: f32,

    // -- Damping settings --
    /// `~30 == complete damping, 0 == defaults`.
    pub grounded_angular_damping: f32,
    /// `~30 == complete damping, 0 == defaults`.
    pub grounded_linear_damping: f32,
}

impl ItemDefinition {
    /// Parse from a tag's `item` substruct (caller descended to the
    /// right inheritance level). `object` is the already-parsed
    /// `Arc<ObjectDefinition>` from the same tag.
    pub fn from_item_struct(object: Arc<ObjectDefinition>, s: &TagStruct<'_>) -> Self {
        Self {
            object,
            flags: s.try_read_flags("flags").unwrap_or_default(),
            pickup_message: s.read_string_id("pickup message").unwrap_or_default(),
            swap_message: s.read_string_id("swap message").unwrap_or_default(),
            pickup_message_dual: s.read_string_id("pickup message (dual)").unwrap_or_default(),
            swap_message_dual: s.read_string_id("swap message (dual)").unwrap_or_default(),
            picked_up_msg: s.read_string_id("picked up msg").unwrap_or_default(),
            switch_to_msg: s.read_string_id("switch-to msg").unwrap_or_default(),
            switch_to_from_ai_msg: s
                .read_string_id("switch-to from ai msg")
                .unwrap_or_default(),
            notify_empty_msg: s.read_string_id("notify empty msg").unwrap_or_default(),
            private_use_font_icon: s.read_int_any("private use font icon").unwrap_or(0) as i32,
            private_use_font_icon_dual: s
                .read_int_any("private use font icon (dual)")
                .unwrap_or(0) as i32,
            detonation_damage_effect: s
                .read_tag_ref_path("detonation damage effect")
                .unwrap_or_default(),
            detonation_delay: s.read_real_bounds("detonation delay"),
            detonating_effect: s.read_tag_ref_path("detonating effect").unwrap_or_default(),
            detonation_effect: s.read_tag_ref_path("detonation effect").unwrap_or_default(),
            single_player_ground: s.read_real("single player ground").unwrap_or(0.0),
            multiplayer_ground: s.read_real("multiplayer ground").unwrap_or(0.0),
            small_unit_armed: s.read_real("small unit (armed)").unwrap_or(0.0),
            small_unit_stowed: s.read_real("small unit (stowed)").unwrap_or(0.0),
            medium_unit_armed: s.read_real("medium unit (armed)").unwrap_or(0.0),
            medium_unit_stowed: s.read_real("medium unit (stowed)").unwrap_or(0.0),
            player_unit_armed: s.read_real("player unit (armed)").unwrap_or(0.0),
            player_unit_stowed: s.read_real("player unit (stowed)").unwrap_or(0.0),
            large_unit_armed: s.read_real("large unit (armed)").unwrap_or(0.0),
            large_unit_stowed: s.read_real("large unit (stowed)").unwrap_or(0.0),
            grounded_angular_damping: s.read_real("grounded angular damping").unwrap_or(0.0),
            grounded_linear_damping: s.read_real("grounded linear damping").unwrap_or(0.0),
        }
    }
}
