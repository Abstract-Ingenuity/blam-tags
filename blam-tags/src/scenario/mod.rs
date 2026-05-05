//! Halo scenario tag (`scnr`) walker.
//!
//! Mirrors the rendering-relevant subset of Bungie's `struct scenario`
//! (Ares `source/scenario/scenario_definitions.h`). Field names follow
//! the **MCC tag schema** (with spaces) since that's the authoritative
//! source for parsing — Ares is older and field offsets/names have
//! drifted across MCC builds.
//!
//! See [`Scenario`] for the entry point.

mod types;

pub use types::{
    CubemapEntry, DecoratorPlacementBlock, ObjectPlacement, PlacementMultiplayerData,
    PlacementObjectData, PlacementPermutationData, Scenario, ScenarioError, SkyReference,
    StructureBspReference, TagReferencePalette, ZoneSet,
};
