use anyhow::Result;
use clap::{Parser, Subcommand};

use context::CliContext;

mod commands;
mod context;
mod format;
mod repl;
mod walk;

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
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },

    /// Walk a directory for tags; filter + list, or summarize by group
    List {
        /// Directory to walk
        dir: String,
        /// Filter by group tag (e.g. "bipd")
        #[arg(long)]
        group: Option<String>,
        /// Only tags whose filename starts with this prefix
        #[arg(long = "starts-with")]
        starts_with: Option<String>,
        /// Only tags whose path contains this substring
        #[arg(long)]
        contains: Option<String>,
        /// Only tags whose filename ends with this suffix (useful for extensions)
        #[arg(long = "ends-with")]
        ends_with: Option<String>,
        /// Only tags whose full path matches this regex
        #[arg(long)]
        regex: Option<String>,
        /// Read candidate tag paths from this file (one per line) instead of walking
        #[arg(long = "from-file")]
        from_file: Option<String>,
        /// Group/extension tally instead of a path list
        #[arg(long)]
        summary: bool,
        /// Sort summary rows by count (desc) instead of name
        #[arg(long = "sort-by-count")]
        sort_by_count: bool,
        /// Emit JSON
        #[arg(long)]
        json: bool,
        /// Fail on any unreadable / malformed tag encountered (default: skip silently)
        #[arg(long)]
        strict: bool,
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
        /// Only show fields whose name contains any of these comma-separated substrings
        #[arg(long, value_delimiter = ',')]
        filter: Vec<String>,
        /// Skip fields whose name contains any of these comma-separated substrings
        #[arg(long = "filter-not", value_delimiter = ',')]
        filter_not: Vec<String>,
        /// Only show leaf fields whose rendered value contains this substring
        #[arg(long = "filter-value")]
        filter_value: Option<String>,
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
        /// Action to perform on the block
        #[arg(value_enum)]
        action: commands::block::BlockAction,
        /// First index argument (insert/duplicate/delete/swap first/move from)
        index: Option<usize>,
        /// Second index argument (swap second / move to)
        index2: Option<usize>,
        /// Write to a different file
        #[arg(long)]
        output: Option<String>,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
        /// Emit JSON (only meaningful for `count`)
        #[arg(long)]
        json: bool,
    },

    /// List enum/flag options for a field
    Options {
        /// Path to a tag file
        file: String,
        /// Field path to an enum or flags field
        path: String,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },

    /// Diff the layouts of two tag files
    LayoutDiff {
        /// First tag file
        file_a: String,
        /// Second tag file
        file_b: String,
    },

    /// List all tag_reference fields in a tag
    Deps {
        /// Path to a tag file
        file: String,
        /// De-duplicate repeated references
        #[arg(long)]
        unique: bool,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },

    /// Search across a directory of tags for field values matching a query
    Find {
        /// Directory to walk
        dir: String,
        /// Value substring (or regex, if --regex) to search for
        value: String,
        /// Only search tags of this group
        #[arg(long)]
        group: Option<String>,
        /// Only check fields whose name matches this regex
        #[arg(long = "field-name")]
        field_name: Option<String>,
        /// Interpret `value` as a regex instead of a substring
        #[arg(long)]
        regex: bool,
        /// Emit JSON
        #[arg(long)]
        json: bool,
        /// Fail on any unreadable / malformed tag encountered
        #[arg(long)]
        strict: bool,
    },

    /// Dump a tag's state as `set` commands that reproduce it —
    /// diffable between tags and replayable against another
    Export {
        /// Path to a tag file
        file: String,
        /// Optional field path; only export fields under this subtree
        subtree: Option<String>,
        /// Write to a file instead of stdout
        #[arg(long)]
        output: Option<String>,
    },

    /// Compare two tags' *values* at every leaf path (distinct from
    /// `layout-diff`, which compares schemas)
    DataDiff {
        /// First tag file
        file_a: String,
        /// Second tag file
        file_b: String,
        /// Optional subtree to restrict both walks to
        #[arg(long)]
        only: Option<String>,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },

    /// Integrity check — flag enum/flag/real/reference anomalies
    Check {
        /// Path to a tag file
        file: String,
        /// Tags root directory; required for tag-reference existence checks
        #[arg(long = "tags-root")]
        tags_root: Option<String>,
        /// Comma-separated subset: enum,flag,real,reference (default: all)
        #[arg(long)]
        only: Option<String>,
        /// Emit JSON
        #[arg(long)]
        json: bool,
        /// Non-zero exit status on any finding (for CI)
        #[arg(long)]
        strict: bool,
    },

    /// Interactive shell — open a tag and run commands against it
    /// without re-parsing on every invocation
    Repl {
        /// Optional tag to load at startup
        file: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut ctx = CliContext::new();

    match cli.command {
        Commands::Repl { file } => repl::run(&mut ctx, file.as_deref()),
        cmd => dispatch(&mut ctx, cmd, true),
    }
}

/// Execute a parsed command. `reload_tag` controls whether the
/// driver calls [`CliContext::load`] before tag-bound commands:
/// one-shot mode passes `true`, the REPL passes `false` so edits
/// accumulate across commands on a single loaded tag.
pub(crate) fn dispatch(ctx: &mut CliContext, cmd: Commands, reload_tag: bool) -> Result<()> {
    match cmd {
        Commands::Repl { .. } => anyhow::bail!("`repl` is only valid at the top level"),

        Commands::Header { file, json } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::header::run(ctx, json)
        }

        Commands::List {
            dir, group, starts_with, contains, ends_with, regex, from_file, summary, sort_by_count, json, strict,
        } => {
            let filters = commands::list::ListFilters {
                group, starts_with, contains, ends_with, regex, from_file, strict,
            };
            let mode = if json {
                commands::list::OutputMode::Json
            } else if summary {
                commands::list::OutputMode::Summary { sort_by_count }
            } else {
                commands::list::OutputMode::Paths
            };
            commands::list::run(&dir, filters, mode)
        }

        Commands::Inspect { file, path, depth, all, json, filter, filter_not, filter_value } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::inspect::run(
                ctx,
                path.as_deref(),
                depth,
                all,
                json,
                commands::inspect::Filters {
                    names: filter,
                    excludes: filter_not,
                    value: filter_value,
                },
            )
        }

        Commands::Get { file, path, raw, json, hex } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::get::run(ctx, &path, raw, json, hex)
        }

        Commands::Set { file, path, value, output, dry_run } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::set::run(ctx, &path, &value, output.as_deref(), dry_run)
        }

        Commands::Flag { file, path, flag_name, action, output, dry_run } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::flag::run(ctx, &path, &flag_name, action.as_deref(), output.as_deref(), dry_run)
        }

        Commands::Block { file, path, action, index, index2, output, dry_run, json } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::block::run(ctx, &path, action, index, index2, output.as_deref(), dry_run, json)
        }

        Commands::Options { file, path, json } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::options::run(ctx, &path, json)
        }

        Commands::LayoutDiff { file_a, file_b } => commands::layout_diff::run(&file_a, &file_b),

        Commands::Deps { file, unique, json } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::deps::run(ctx, unique, json)
        }

        Commands::Find { dir, value, group, field_name, regex, json, strict } => {
            let filters = commands::find::FindFilters { group, field_name, regex, json, strict };
            commands::find::run(&dir, &value, filters)
        }

        Commands::Export { file, subtree, output } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::export::run(ctx, subtree.as_deref(), output.as_deref())
        }

        Commands::Check { file, tags_root, only, json, strict } => {
            ensure_loaded(ctx, &file, reload_tag)?;
            commands::check::run(ctx, tags_root.as_deref(), only.as_deref(), json, strict)
        }

        Commands::DataDiff { file_a, file_b, only, json } => {
            commands::data_diff::run(&file_a, &file_b, only.as_deref(), json)
        }
    }
}

/// Load `file` into `ctx` (one-shot mode) or verify a tag is already
/// loaded (REPL mode, where `file` is ignored because the REPL's
/// line preprocessor fills it in from [`CliContext::loaded`]).
fn ensure_loaded(ctx: &mut CliContext, file: &str, reload: bool) -> Result<()> {
    if reload {
        ctx.load(file)?;
    } else if ctx.loaded.is_none() {
        anyhow::bail!("no tag loaded (use `open <path>` first)");
    }
    Ok(())
}
