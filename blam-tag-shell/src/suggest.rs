//! Fuzzy "did you mean ...?" hints for the CLI.
//!
//! When the user mistypes a field path, we walk the parent struct's
//! field names and pick the closest by Levenshtein distance —
//! tolerant of small typos but rejecting matches that diverge too far
//! (`distance > typed.len() / 2 + 1`). This is a UX heuristic, not a
//! schema operation, so it lives in the shell rather than the lib.

use blam_tags::TagStruct;

/// If `typed` doesn't resolve to an actual field on `parent`, return
/// the closest existing field name when it's near enough to be worth
/// suggesting. Returns `None` when no field is within tolerance —
/// callers fall back to a plain "not found" error in that case.
pub fn suggest_field_name<'a>(parent: &TagStruct<'a>, typed: &str) -> Option<&'a str> {
    let typed_lower = typed.to_lowercase();
    let mut best: Option<(usize, &'a str)> = None;
    for candidate in parent.field_names() {
        let distance = edit_distance(&typed_lower, &candidate.to_lowercase());
        match best {
            Some((d, _)) if distance >= d => {}
            _ => best = Some((distance, candidate)),
        }
    }
    best.filter(|(d, _)| *d <= typed.len() / 2 + 1).map(|(_, s)| s)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m { dp[i][0] = i; }
    for j in 0..=n { dp[0][j] = j; }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1).min(dp[i][j - 1] + 1).min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}
