//! `model_definition` (`hlmt`) — the mid-level tag between an object
//! definition and its `render_model`. An object's `model` field points
//! here; this tag selects the `render_model` plus a list of **variants**.
//!
//! A variant names a set of region permutations to show AND a set of
//! **child objects** to attach (engine `model_variant_object_block`).
//! The latter is how a vehicle gets its turret: e.g. the `warthog`
//! model's `default` variant carries a child object
//! `objects\vehicles\warthog\turrets\chaingun\chaingun.vehicle`
//! attached at a marker. The engine spawns each child during
//! `object_new` and binds it via `object_attach_to_marker`.
//!
//! Schema reference: `definitions/halo3_mcc/model.json` →
//! `model_definition` → `variants` (`model_variant_block`) →
//! `objects` (`model_variant_object_block`). Field names below match
//! the schema verbatim (see the self-describing field strings in any
//! `.model` tag: `variants` / `objects` / `parent marker` /
//! `child marker` / `child variant name` / `child object`).

use crate::api::TagStruct;
use crate::file::TagFile;

/// One child object a variant attaches to the parent
/// (`model_variant_object_block`). The engine binds `child object`'s
/// `child marker` to the parent model's `parent marker`.
#[derive(Debug, Clone, Default)]
pub struct ModelVariantObject {
    /// `parent marker` (string_id) — marker on THIS model the child
    /// attaches to. Empty = parent origin.
    pub parent_marker: String,
    /// `child marker` (string_id) — marker on the child object that
    /// mates with `parent_marker`. Empty = child origin.
    pub child_marker: String,
    /// `child variant name` (string_id) — which variant of the child
    /// object to spawn (recurses through the child's own variants).
    pub child_variant_name: String,
    /// `child object` (tag_reference path) — the attached object tag
    /// (e.g. the turret `.vehicle`). Empty when the reference is null.
    pub child_object: String,
    /// 4-byte big-endian group fourcc of [`Self::child_object`]
    /// (`b"vehi"` / `b"weap"` / …). `[0;4]` when null.
    pub child_object_group: [u8; 4],
}

impl ModelVariantObject {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let (group_u32, child_object) = s
            .read_tag_ref_with_group("child object")
            .unwrap_or((0, String::new()));
        Self {
            parent_marker: s.read_string_id("parent marker").unwrap_or_default(),
            child_marker: s.read_string_id("child marker").unwrap_or_default(),
            child_variant_name: s.read_string_id("child variant name").unwrap_or_default(),
            child_object,
            child_object_group: group_u32.to_be_bytes(),
        }
    }
}

/// One region of a model variant (`model_variant_region_block`). Maps a
/// render_model region (by NAME) to the permutation(s) this variant
/// shows for it. The engine resolves the active permutation by NAME via
/// `render_model_definition_find_region_permutation_by_name` — the
/// render_model's authored permutation ORDER is irrelevant (e.g. the
/// wraith's `cockpit` region lists `medium` before `base`, but the
/// `default` variant designates `base`).
#[derive(Debug, Clone, Default)]
pub struct ModelVariantRegion {
    /// `region name` (string_id) — matches a render_model region name.
    pub name: String,
    /// `permutations[].permutation name` (string_id) in state order.
    /// `[0]` is the base / undamaged permutation; later entries are
    /// damage states. Undamaged rendering uses `[0]`.
    pub permutation_names: Vec<String>,
}

impl ModelVariantRegion {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        let permutation_names = read_block_vec(s, "permutations", |p| {
            p.read_string_id("permutation name").unwrap_or_default()
        });
        Self {
            name: s.read_string_id("region name").unwrap_or_default(),
            permutation_names,
        }
    }
}

/// One model variant (`model_variant_block`). Walks the fields needed to
/// drive child-object attachment (`objects`) and region/permutation
/// selection (`regions`).
#[derive(Debug, Clone, Default)]
pub struct ModelVariant {
    /// `name` (string_id) — the variant name (e.g. `"default"`).
    pub name: String,
    /// `regions` (`model_variant_region_block`) — per-region permutation
    /// designation. Drives which render_model permutation to draw.
    pub regions: Vec<ModelVariantRegion>,
    /// `objects` (`model_variant_object_block`) — child objects this
    /// variant attaches (turrets, etc.).
    pub objects: Vec<ModelVariantObject>,
}

impl ModelVariant {
    fn from_struct(s: &TagStruct<'_>) -> Self {
        Self {
            name: s.read_string_id("name").unwrap_or_default(),
            regions: read_block_vec(s, "regions", ModelVariantRegion::from_struct),
            objects: read_block_vec(s, "objects", ModelVariantObject::from_struct),
        }
    }

    /// The base (undamaged) permutation name this variant designates for
    /// `region_name`, or `None` if the variant doesn't list that region.
    /// `render_model` permutation lookup is then by this NAME, not by
    /// authored order.
    pub fn region_base_permutation(&self, region_name: &str) -> Option<&str> {
        self.regions
            .iter()
            .find(|r| r.name == region_name)
            .and_then(|r| r.permutation_names.first())
            .map(|s| s.as_str())
    }
}

/// Walked subset of the engine `model_definition` (`hlmt`). Carries the
/// `render_model` reference and the variant list.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// `render model` (tag_reference path → `mode`).
    pub render_model: String,
    /// `variants` (`model_variant_block`).
    pub variants: Vec<ModelVariant>,
}

/// Error decoding a [`Model`].
#[derive(Debug)]
pub enum ModelError {
    /// The tag's group fourcc is not `hlmt`.
    WrongGroup { actual: [u8; 4] },
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::WrongGroup { actual } => {
                write!(f, "not a model tag (group = {:?})", std::str::from_utf8(actual))
            }
        }
    }
}

impl std::error::Error for ModelError {}

impl Model {
    pub fn from_tag(tag: &TagFile) -> Result<Self, ModelError> {
        let actual = tag.group().tag.to_be_bytes();
        if &actual != b"hlmt" {
            return Err(ModelError::WrongGroup { actual });
        }
        let root = tag.root();
        Ok(Self {
            render_model: root.read_tag_ref_path("render model").unwrap_or_default(),
            variants: read_block_vec(&root, "variants", ModelVariant::from_struct),
        })
    }

    /// Resolve the variant matching `name`, falling back to a variant
    /// literally named `"default"`, then the first variant. Returns
    /// `None` only when the model has no variants at all. Mirrors the
    /// engine's `model_get_variant_index` (name match → default → 0).
    pub fn variant<'a>(&'a self, name: &str) -> Option<&'a ModelVariant> {
        if !name.is_empty() {
            if let Some(v) = self.variants.iter().find(|v| v.name == name) {
                return Some(v);
            }
        }
        self.variants
            .iter()
            .find(|v| v.name == "default")
            .or_else(|| self.variants.first())
    }
}

fn read_block_vec<T, F>(s: &TagStruct<'_>, name: &str, mut f: F) -> Vec<T>
where
    F: FnMut(&TagStruct<'_>) -> T,
{
    s.field(name)
        .and_then(|f| f.as_block())
        .map(|block| block.iter().map(|e| f(&e)).collect::<Vec<_>>())
        .unwrap_or_default()
}
