//! Typed, name-resolved render-method category choices.
//!
//! An `rm**` selects one option per category its `rmdf` declares:
//! `rmsh.options[i]` indexes `rmdf.categories[i].options[]`. Both the
//! category position AND the option position vary per rmdf — verified
//! against the shipped H3 tags:
//!
//! - `blend_mode` is category **1** in `particle.rmdf`, **7** in
//!   `shader.rmdf`, **2** in `halogram.rmdf`.
//! - within `blend_mode`, `pre_multiplied_alpha` is option **5** in
//!   `shader.rmdf` but option **10** in `particle.rmdf` (same blend
//!   mode, different index), and neither matches the runtime
//!   [`AlphaBlendMode`] value order.
//!
//! So positional decode is wrong by construction; only the category and
//! option **names** are stable. This resolver records the
//! `(category_name, option_name)` pairs once so consumers never index by
//! position. It is the canonical home for what protomorph previously did
//! locally in its `CategoryChoices`.

use std::str::FromStr;

use super::types::{AlphaBlendMode, RenderMethod, RenderMethodDefinition};

/// One resolved category choice.
#[derive(Debug, Clone)]
pub struct RenderMethodCategoryChoice {
    pub category_name: String,
    pub option_name: String,
    /// The raw option index into `rmdf.categories[c].options`. Kept for
    /// diagnostics ONLY — it is the authored category position, which
    /// differs per rmdf and is NOT the runtime enum value. Never feed it
    /// to a runtime table.
    pub option_index: u16,
}

/// All of an `rm**`'s category→option choices, resolved by name.
#[derive(Debug, Clone, Default)]
pub struct RenderMethodChoices {
    choices: Vec<RenderMethodCategoryChoice>,
}

impl RenderMethodChoices {
    /// Resolve from a parsed `rm**` + its `rmdf`. Needs only the rmdf
    /// categories and `rm.options` (no rmop loading). Missing
    /// `options[i]` — `rm.options` shorter than `rmdf.categories`, common
    /// when a newer rmdf adds categories — defaults to option index 0,
    /// matching the runtime's `find_parameter` behavior. Categories with
    /// an empty name are skipped.
    pub fn resolve(rm: &RenderMethod, rmdf: &RenderMethodDefinition) -> Self {
        let mut choices = Vec::with_capacity(rmdf.categories.len());
        for (cat_idx, category) in rmdf.categories.iter().enumerate() {
            if category.category_name.is_empty() {
                continue;
            }
            let opt_idx = rm.options.get(cat_idx).copied().unwrap_or(0).max(0) as usize;
            let option_name = category
                .options
                .get(opt_idx)
                .map(|o| o.option_name.clone())
                .unwrap_or_default();
            choices.push(RenderMethodCategoryChoice {
                category_name: category.category_name.clone(),
                option_name,
                option_index: opt_idx as u16,
            });
        }
        Self { choices }
    }

    /// All resolved choices, in rmdf category order.
    pub fn choices(&self) -> &[RenderMethodCategoryChoice] {
        &self.choices
    }

    /// The chosen option name for a category, or `None` when the rmdf
    /// doesn't declare that category. Caller decides default vs. error.
    pub fn get(&self, category: &str) -> Option<&str> {
        self.choices
            .iter()
            .find(|c| c.category_name == category)
            .map(|c| c.option_name.as_str())
    }

    /// The chosen option name with a fallback default. Use when "category
    /// absent" should behave as "category set to its first option" (e.g.
    /// `"none"` / `"off"` / `"opaque"`).
    pub fn get_or<'a>(&'a self, category: &str, default: &'a str) -> &'a str {
        self.get(category).unwrap_or(default)
    }

    /// Resolve the `blend_mode` category to the runtime [`AlphaBlendMode`]
    /// **by name** (order- and drift-proof). Returns `None` when the rmdf
    /// has no `blend_mode` category OR the option name is unrecognized;
    /// the caller picks the appropriate default (transparent subclasses
    /// typically fall back to `alpha_blend`, opaque ones to `opaque`).
    pub fn blend_mode(&self) -> Option<AlphaBlendMode> {
        AlphaBlendMode::from_str(self.get("blend_mode")?).ok()
    }

    pub fn is_empty(&self) -> bool {
        self.choices.is_empty()
    }
}
