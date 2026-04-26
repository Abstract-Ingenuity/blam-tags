//! `list-animations` — enumerate the animations in a `model_animation_graph`
//! tag with header metadata only (no codec decode). Pairs with
//! [`crate::commands::extract_animation`] for the per-animation decode +
//! JMA / JSON export.
//!
//! Inheritance is a normal case: a jmad with zero local animations and a
//! non-null `parent animation graph` prints a one-line "(inherits from
//! …)" notice instead of failing.

use anyhow::{Context, Result};
use serde_json::json;

use blam_tags::Animation;

use crate::context::CliContext;

pub fn run(ctx: &mut CliContext, json_output: bool) -> Result<()> {
    let loaded = ctx.loaded("list-animations")?;
    let animation = Animation::new(&loaded.tag)
        .with_context(|| format!("failed to walk animations in {}", loaded.path.display()))?;

    if json_output {
        let rows: Vec<_> = animation.iter().map(|g| {
            json!({
                "index": g.index,
                "name": g.name,
                "type": g.animation_type,
                "frame_info_type": g.frame_info_type,
                "frame_count": g.frame_count,
                "node_count": g.node_count,
                "node_list_checksum": g.node_list_checksum,
                "resource_group": g.resource_group,
                "resource_group_member": g.resource_group_member,
                "checksum": g.checksum,
                "movement_type": g.movement_type,
                "codec": g.codec_byte,
                "blob_size": g.blob.len(),
                "data_sizes": g.data_sizes.as_ref().map(|d| {
                    let m: serde_json::Map<String, serde_json::Value> = d.fields.iter()
                        .map(|(k, v)| (k.clone(), serde_json::Value::from(*v)))
                        .collect();
                    json!({ "total": d.total(), "fields": m })
                }),
                "movement_type_mismatch": g.movement_type_mismatch(),
            })
        }).collect();
        let out = json!({
            "count": animation.len(),
            "unresolved": animation.unresolved_count(),
            "parent": animation.parent(),
            "animations": rows,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if animation.is_empty() {
        match animation.parent() {
            Some(p) => println!("(no animations — inherits from {p})"),
            None => println!("(no animations)"),
        }
        return Ok(());
    }

    println!(
        "{:>5}  {:>3}  {:>5} {:>4}  {:<10}  {:<14}  {:>9}  {}",
        "idx", "cdc", "frame", "node", "type", "movement", "blob", "name",
    );
    for g in animation.iter() {
        let codec = g.codec_byte.map(|c| c.to_string()).unwrap_or_else(|| "-".into());
        let typ = g.animation_type.as_deref().unwrap_or("");
        let mvmt = g.movement_type.as_deref().or(g.frame_info_type.as_deref()).unwrap_or("");
        let warn = if g.movement_type_mismatch() { " !" } else { "" };
        println!(
            "{:>5}  {:>3}  {:>5} {:>4}  {:<10}  {:<14}{:>2}  {:>9}  {}",
            g.index,
            codec,
            g.frame_count,
            g.node_count,
            typ,
            mvmt,
            warn,
            g.blob.len(),
            g.name.as_deref().unwrap_or("(unnamed)"),
        );
    }

    let unresolved = animation.unresolved_count();
    if unresolved > 0 {
        println!();
        println!("{unresolved} animation(s) have no resolved group_member (likely inherited)");
        if let Some(p) = animation.parent() {
            println!("parent: {p}");
        }
    }

    Ok(())
}
