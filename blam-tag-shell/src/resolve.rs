//! CLI-level helpers layered on top of [`blam_tags::path::lookup`]:
//! a Levenshtein "did you mean?" suggester for unresolved field
//! names. Library-level operations (field-name listing, by-name
//! lookup) live on [`blam_tags::data::TagStruct`].

use blam_tags::data::TagStruct;
use blam_tags::layout::TagLayout;

/// Collect the user-addressable field names in `struct_data`'s
/// struct. Thin wrapper over [`TagStruct::field_names`]; exists to
/// keep `suggest_field` ergonomic without imposing the iterator on
/// every caller.
pub fn available_field_names(layout: &TagLayout, struct_data: &TagStruct) -> Vec<String> {
    struct_data
        .field_names(layout)
        .map(str::to_string)
        .collect()
}

/// Suggest the closest field name to `typed` by case-insensitive edit
/// distance. Returns `None` when no candidate is reasonably close
/// (distance > half the typed name's length + 1).
pub fn suggest_field(typed: &str, available: &[String]) -> Option<String> {
    let typed_lower = typed.to_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for candidate in available {
        let distance = edit_distance(&typed_lower, &candidate.to_lowercase());
        match &best {
            Some((d, _)) if distance < *d => best = Some((distance, candidate)),
            None => best = Some((distance, candidate)),
            _ => {}
        }
    }
    best.filter(|(d, _)| *d <= typed.len() / 2 + 1)
        .map(|(_, s)| s.to_string())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

