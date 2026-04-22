use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod format;
mod resolve;

#[derive(Parser)]
#[command(name = "blam-tag-shell", about = "Halo tag file inspector and editor")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show tag/cache file header metadata
    Header {
        /// Path to a tag or cache file
        file: String,
    },
    
    /// Scan a directory for tag types
    Scan {
        /// Directory to scan
        dir: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Sort by: name or count
        #[arg(long, default_value = "name")]
        sort: String,
    },

    /// Show field tree
    Inspect {
        /// Path to a tag file
        file: String,
        /// Field path to start from
        path: Option<String>,
        /// Maximum depth to display
        #[arg(long, default_value_t = 1)]
        depth: usize,
        /// Show all fields including hidden
        #[arg(long)]
        all: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Read a field value
    Get {
        /// Path to a tag file
        file: String,
        /// Field path (e.g. "jump velocity" or "unit/seats\[0\]/flags")
        path: String,
        /// Output raw value only (no label)
        #[arg(long)]
        raw: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Output numeric values in hex
        #[arg(long)]
        hex: bool,
    },

    /// Write a field value
    Set {
        /// Path to a tag file
        file: String,
        /// Field path
        path: String,
        /// Value to set
        value: String,
        /// Write to a different file
        #[arg(long)]
        output: Option<String>,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Get or set flag bits
    Flag {
        /// Path to a tag file
        file: String,
        /// Field path to a flags field
        path: String,
        /// Flag name
        flag_name: String,
        /// Action: on, off, toggle (omit to read)
        action: Option<String>,
        /// Write to a different file
        #[arg(long)]
        output: Option<String>,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Block element operations
    Block {
        /// Path to a tag file
        file: String,
        /// Field path to a block
        path: String,
        /// Action: count, add, insert N, duplicate N, delete N, clear
        action: String,
        /// Index argument for insert/duplicate/delete
        index: Option<usize>,
        /// Write to a different file
        #[arg(long)]
        output: Option<String>,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// List enum/flag options for a field
    Options {
        /// Path to a tag file
        file: String,
        /// Field path to an enum or flags field
        path: String,
    },

    /// Diff the layouts of two tag files
    LayoutDiff {
        /// First tag file
        file_a: String,
        /// Second tag file
        file_b: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Header { file } => commands::header::run(&file),

        Commands::Scan { dir, json, sort } => commands::scan::run(&dir, json, &sort),

        Commands::Inspect {
            file,
            path,
            depth,
            all,
            json,
        } => commands::inspect::run(&file, path.as_deref(), depth, all, json),

        Commands::Get {
            file,
            path,
            raw,
            json,
            hex,
        } => commands::get::run(&file, &path, raw, json, hex),

        Commands::Set {
            file,
            path,
            value,
            output,
            dry_run,
        } => commands::set::run(&file, &path, &value, output.as_deref(), dry_run),

        Commands::Flag {
            file,
            path,
            flag_name,
            action,
            output,
            dry_run,
        } => commands::flag::run(
            &file,
            &path,
            &flag_name,
            action.as_deref(),
            output.as_deref(),
            dry_run,
        ),

        Commands::Block {
            file,
            path,
            action,
            index,
            output,
            dry_run,
        } => commands::block::run(&file, &path, &action, index, output.as_deref(), dry_run),

        Commands::Options { file, path } => commands::options::run(&file, &path),

        Commands::LayoutDiff { file_a, file_b } => {
            commands::layout_diff::run(&file_a, &file_b)
        }
    }
}
