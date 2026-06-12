//! Dump a `model_animation_graph` (jmad) tag's content/modes tree.

use std::path::PathBuf;

use blam_tags::TagFile;
use blam_tags::animation::AnimationGraph;

fn main() {
    let Some(path_str) = std::env::args().nth(1) else {
        eprintln!("usage: dump_animation_graph <path/to/foo.model_animation_graph>");
        std::process::exit(2);
    };
    let path = PathBuf::from(&path_str);
    let tag = TagFile::read(&path).expect("read jmad");
    let graph = AnimationGraph::from_tag(&tag);

    println!("animation_graph: {}", path.display());
    println!("  modes: [{}]", graph.modes.len());
    for mode in &graph.modes {
        println!("    mode {:?}", mode.label);
        for wc in &mode.weapon_classes {
            println!("      weapon_class {:?}", wc.label);
            for wt in &wc.weapon_types {
                println!("        weapon_type {:?}", wt.label);
                for action in &wt.actions {
                    println!(
                        "          action {:?} -> graph={} anim={}",
                        action.label,
                        action.animation.graph_index,
                        action.animation.animation_index,
                    );
                }
                if !wt.overlays.is_empty() {
                    println!("          overlays[{}]", wt.overlays.len());
                }
                if !wt.transitions.is_empty() {
                    println!("          transitions[{}]", wt.transitions.len());
                }
            }
        }
    }
    println!();
    if let Some(first) = graph.first_action() {
        println!("first_action: graph={} anim={}", first.graph_index, first.animation_index);
    }
    if let Some(idle) = graph.find_action("any", "any", "any", "any", "idle") {
        println!("find any/any/any/idle: graph={} anim={}", idle.graph_index, idle.animation_index);
    }
}
