//! CLI-level session state.
//!
//! [`CliContext`] is threaded through every command's `run`. For
//! tag-bound commands the driver loads the target tag into
//! [`CliContext::loaded`] before dispatching; commands mutate through
//! the facade and set [`LoadedTag::dirty`]; the driver or the command
//! decides when to persist.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blam_tags::TagFile;

use crate::tag_index::TagIndex;

/// Per-invocation session state — the loaded tag (if any), the
/// REPL navigation prefix, and the loaded `_meta.json` tag index.
/// Threaded through every command's `run`.
pub struct CliContext {
    /// The tag the next command will operate on. `None` when no tag
    /// is loaded (one-shot mode pre-load, or REPL `close` state).
    pub loaded: Option<LoadedTag>,
    /// REPL navigation prefix — each segment is a path fragment the
    /// user entered via `edit-block`. Empty in one-shot mode and at
    /// the Tag context in the REPL. Commands concatenate this with
    /// the user-supplied path to produce the actual field-path.
    pub nav: Vec<String>,
    /// Game identifier (e.g. `"haloreach_mcc"`). Set once at startup
    /// from the global `--game` flag and used to scope schema lookups
    /// and the [`TagIndex`].
    pub game: String,
    /// group_tag ↔ group-name index loaded from
    /// `definitions/<game>/_meta.json`. Used by every command that
    /// renders or parses tag references.
    pub tag_index: TagIndex,
}

/// A parsed tag plus the path it came from, with a dirty flag for
/// REPL save-prompt logic.
pub struct LoadedTag {
    /// Path the tag was read from. Used as the default save target.
    pub path: PathBuf,
    /// Parsed tag tree.
    pub tag: TagFile,
    /// `true` once a command has mutated the tag without persisting.
    pub dirty: bool,
}

impl CliContext {
    /// Build a context for the given game. Eagerly loads
    /// `definitions/<game>/_meta.json` — errors if that file is
    /// missing or malformed.
    pub fn new(game: impl Into<String>) -> Result<Self> {
        let game = game.into();
        let tag_index = TagIndex::load(Path::new("definitions"), &game)?;
        Ok(Self { loaded: None, nav: Vec::new(), game, tag_index })
    }

    /// Load `path` into [`Self::loaded`] and reset [`Self::nav`] to
    /// the root. Replaces any currently-loaded tag without prompting
    /// — dirty-state handling is the caller's job.
    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let tag = TagFile::read(path).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;
        self.loaded = Some(LoadedTag { path: path.to_path_buf(), tag, dirty: false });
        self.nav.clear();
        Ok(())
    }

    /// Immutable borrow of the loaded tag. Context is the command
    /// name for the error message when nothing is loaded.
    pub fn loaded(&self, cmd: &str) -> Result<&LoadedTag> {
        self.loaded
            .as_ref()
            .with_context(|| format!("`{cmd}` needs a loaded tag"))
    }

    /// Mutable borrow of the loaded tag.
    pub fn loaded_mut(&mut self, cmd: &str) -> Result<&mut LoadedTag> {
        self.loaded
            .as_mut()
            .with_context(|| format!("`{cmd}` needs a loaded tag"))
    }

    /// Resolve a user-supplied path against the current navigation
    /// prefix. Paths starting with `/` are absolute (nav stripped);
    /// everything else is concatenated onto the nav prefix.
    pub fn resolve_path(&self, user_path: &str) -> String {
        if let Some(rest) = user_path.strip_prefix('/') {
            rest.to_string()
        } else if self.nav.is_empty() {
            user_path.to_string()
        } else if user_path.is_empty() {
            self.nav.join("/")
        } else {
            format!("{}/{}", self.nav.join("/"), user_path)
        }
    }
}

impl LoadedTag {
    /// Write the tag to `dest` (or back to [`Self::path`] if `dest` is
    /// `None`). Clears the dirty flag on success.
    pub fn save(&mut self, dest: Option<&Path>) -> Result<PathBuf> {
        let target = dest.map(Path::to_path_buf).unwrap_or_else(|| self.path.clone());
        self.tag
            .write(&target)
            .map_err(|e| anyhow::anyhow!("failed to save tag file: {e}"))?;
        self.dirty = false;
        Ok(target)
    }

    /// Save and report where it went. Convenience for mutating
    /// commands — returns `(target_path, was_redirected)` so callers
    /// can print a "saved to <path>" line only when output differs
    /// from the source.
    pub fn commit(&mut self, dest: Option<&Path>) -> Result<Commit> {
        let source = self.path.clone();
        let target = self.save(dest)?;
        let redirected = target != source;
        Ok(Commit { target, redirected })
    }
}

/// Result of [`LoadedTag::commit`] — the path written to and a flag
/// indicating whether it differs from the source path. Lets mutating
/// commands print a "saved to …" line only when the user redirected
/// output via `--output`.
pub struct Commit {
    /// Path the tag was written to.
    pub target: PathBuf,
    /// `true` if `target` differs from the source path (i.e. the
    /// user passed `--output`).
    pub redirected: bool,
}
