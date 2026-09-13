//! The folder the model is allowed to write to.
//!
//! Writing is enabled without a per-call prompt, so the boundary has to be
//! structural rather than advisory: every path the model supplies is resolved
//! to an absolute location and checked to be inside the chosen project folder
//! before the tool ever sees it. A model that asks to write outside the folder
//! gets an error back, not a prompt, and not a file.
//!
//! Relative paths are also rewritten to absolute here. The file tools resolve
//! relative paths against the process working directory, which for an installed
//! desktop app is wherever the launcher happened to start it — so `src/main.rs`
//! would otherwise land somewhere arbitrary rather than in the project.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

/// Arguments that name a file, and therefore have to be confined.
///
/// Covers the tools the desktop offers; a tool added later with a differently
/// named path argument must be added here or it will not be rewritten.
const PATH_ARGS: &[&str] = &["path", "file_path"];

/// A project folder, already verified to exist.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Accepts a folder as the project root.
    ///
    /// Fails rather than defaulting, because a silently wrong root would put
    /// the model's writes somewhere the user is not looking.
    pub fn open(raw: &str) -> Result<Self, String> {
        let path = PathBuf::from(raw);
        if !path.is_dir() {
            return Err(format!("{raw} is not a folder"));
        }
        let root = path
            .canonicalize()
            .map_err(|error| format!("Could not open {raw}: {error}"))?;
        Ok(Self { root })
    }

    /// The confined root, used by the tests that assert containment and by the
    /// file tools, which need the boundary stated rather than inferred.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Displays the root without the `\\?\` prefix Windows canonicalization adds.
    #[must_use]
    pub fn label(&self) -> String {
        let text = self.root.display().to_string();
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
    }

    /// Resolves a model-supplied path, refusing anything outside the project.
    pub fn confine(&self, raw: &str) -> Result<PathBuf, String> {
        if raw.trim().is_empty() {
            return Err("No path was given.".to_string());
        }

        let candidate = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.root.join(raw)
        };

        let resolved = resolve(&candidate);
        if resolved.starts_with(&self.root) {
            Ok(resolved)
        } else {
            Err(format!(
                "Refused: {raw} is outside the project folder ({}). Only files inside it can be \
                 changed.",
                self.label()
            ))
        }
    }

    /// Rewrites a tool's arguments so every path it names is confined.
    pub fn rewrite(&self, input: &Value) -> Result<Value, String> {
        let Some(object) = input.as_object() else {
            return Ok(input.clone());
        };

        let mut out = object.clone();
        for key in PATH_ARGS {
            if let Some(Value::String(raw)) = object.get(*key) {
                let confined = self.confine(raw)?;
                out.insert((*key).to_string(), Value::String(path_text(&confined)));
            }
        }
        Ok(Value::Object(out))
    }
}

/// Renders a path for a tool, dropping the Windows verbatim prefix.
///
/// `canonicalize` yields `\\?\C:\...`, which some path handling treats as a
/// literal name rather than a prefix.
fn path_text(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

/// Resolves a path that need not exist yet.
///
/// `canonicalize` fails on a file that has not been created, which is the
/// normal case for a write. So the deepest existing ancestor is canonicalized —
/// that is what defeats a symlink pointing out of the project — and the
/// remaining names are appended to it.
fn resolve(candidate: &Path) -> PathBuf {
    let lexical = flatten(candidate);

    let mut existing = lexical.as_path();
    let mut trailing: Vec<&OsStr> = Vec::new();
    while !existing.exists() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                trailing.push(name);
                existing = parent;
            }
            _ => break,
        }
    }

    let mut resolved = existing
        .canonicalize()
        .unwrap_or_else(|_| existing.to_path_buf());
    for name in trailing.into_iter().rev() {
        resolved.push(name);
    }
    resolved
}

/// Removes `.` and applies `..` textually.
///
/// Done before touching the filesystem so that `../../etc/passwd` cannot
/// survive as a component and be re-introduced after the ancestor walk.
fn flatten(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("disco-ws-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_path_inside_the_project_is_accepted_and_made_absolute() {
        let dir = scratch("inside");
        let workspace = Workspace::open(dir.to_str().unwrap()).unwrap();

        let resolved = workspace.confine("src/main.rs").unwrap();

        assert!(resolved.is_absolute(), "tools resolve relative paths against \
            the launcher's working directory, so they must be made absolute");
        assert!(resolved.starts_with(workspace.root()));
        assert!(resolved.ends_with("main.rs"));
    }

    #[test]
    fn a_file_that_does_not_exist_yet_can_still_be_written() {
        let dir = scratch("new-file");
        let workspace = Workspace::open(dir.to_str().unwrap()).unwrap();

        assert!(
            workspace.confine("does/not/exist/yet.txt").is_ok(),
            "creating a file is the common case and must not require it to exist"
        );
    }

    #[test]
    fn walking_up_out_of_the_project_is_refused() {
        let dir = scratch("escape");
        let workspace = Workspace::open(dir.to_str().unwrap()).unwrap();

        for attempt in [
            "../outside.txt",
            "../../outside.txt",
            "src/../../outside.txt",
            "./././../outside.txt",
        ] {
            let error = workspace
                .confine(attempt)
                .expect_err("{attempt} leaves the project folder");
            assert!(error.contains("outside the project folder"), "{attempt}");
        }
    }

    #[test]
    fn an_absolute_path_elsewhere_on_the_machine_is_refused() {
        let dir = scratch("absolute");
        let workspace = Workspace::open(dir.to_str().unwrap()).unwrap();

        let elsewhere = std::env::temp_dir().join("disco-ws-absolute-sibling.txt");
        assert!(workspace.confine(elsewhere.to_str().unwrap()).is_err());
    }

    /// A sibling folder whose name merely starts with the root's name is not
    /// inside the root. Comparing the paths as text would wrongly allow it.
    #[test]
    fn a_sibling_with_a_shared_name_prefix_is_not_inside_the_project() {
        let root = scratch("prefix");
        let sibling = scratch("prefix-evil");
        std::fs::write(sibling.join("f.txt"), b"x").unwrap();

        let workspace = Workspace::open(root.to_str().unwrap()).unwrap();

        assert!(workspace
            .confine(sibling.join("f.txt").to_str().unwrap())
            .is_err());
    }

    #[test]
    fn rewriting_replaces_every_path_argument_and_leaves_the_rest_alone() {
        let dir = scratch("rewrite");
        let workspace = Workspace::open(dir.to_str().unwrap()).unwrap();

        let input = serde_json::json!({
            "path": "notes.md",
            "content": "hello",
        });
        let out = workspace.rewrite(&input).unwrap();

        assert_eq!(out["content"], "hello");
        let rewritten = out["path"].as_str().unwrap();
        assert!(Path::new(rewritten).is_absolute());
        assert!(!rewritten.starts_with(r"\\?\"), "the verbatim prefix confuses \
            path handling in the tools");
    }

    #[test]
    fn rewriting_fails_closed_when_a_path_escapes() {
        let dir = scratch("rewrite-escape");
        let workspace = Workspace::open(dir.to_str().unwrap()).unwrap();

        let input = serde_json::json!({ "path": "../escape.txt", "content": "x" });

        assert!(
            workspace.rewrite(&input).is_err(),
            "the tool must never run at all, rather than run on a clamped path"
        );
    }

    #[test]
    fn a_missing_folder_is_not_accepted_as_a_project() {
        let missing = std::env::temp_dir().join("disco-ws-definitely-absent");
        let _ = std::fs::remove_dir_all(&missing);

        assert!(Workspace::open(missing.to_str().unwrap()).is_err());
    }
}
