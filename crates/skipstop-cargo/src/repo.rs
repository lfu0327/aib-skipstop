//! Which ledger folder a repo's runs are kept in.
//!
//! The folder is named from the clone, not from the folder the run was started in: every
//! working tree of one clone (`git worktree add`) keeps its runs in the same ledger, and two
//! clones that happen to sit in folders of the same name do not. Before 0.4 the folder name
//! of the working tree was the id, so a trial's history was split between working trees and
//! ended when one was removed.
//!
//! A ledger kept under the older name is not read: its runs' names were only unique inside
//! that folder, a revocation in one working tree's folder would be unseen from another, and
//! any folder of the same name would be taken for it. History starts again under the new
//! name, and with no history real mode runs everything.
//!
//! Sharing a ledger does not widen what may be reused: a run is reused only under the same
//! key, and the key already holds the unit, its inputs and the build folder.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A folder's name as a plain id (letters, digits, `-`, `_`; anything else is `-`).
pub fn folder_id(dir: &Path) -> String {
    let n = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let s: String = n
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() {
        "repo".into()
    } else {
        s
    }
}

/// The id of a clone from its common git folder (`<clone>/.git`, or a bare repo's own
/// folder): the clone's folder name, then eight hex digits of the folder's whole path, so two
/// clones of one name are two ledgers.
pub fn id_from_common(common: &Path) -> String {
    let named = if common.file_name().is_some_and(|n| n == ".git") {
        common.parent().unwrap_or(common)
    } else {
        common
    };
    let mut h = Sha256::new();
    h.update(common.to_string_lossy().as_bytes());
    let d = h.finalize();
    let hex: String = d.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{}-{hex}", folder_id(named))
}

/// The path git's `rev-parse --git-common-dir` names, started in `root`: one line, taken
/// from `root` when it is not absolute (git answers `.git` in the main working tree).
/// Anything else is no answer.
pub fn common_from_git(root: &Path, stdout: &str) -> Option<PathBuf> {
    let mut lines = stdout.lines().filter(|l| !l.trim().is_empty());
    let one = lines.next()?.trim();
    if lines.next().is_some() {
        return None;
    }
    let p = PathBuf::from(one);
    Some(if p.is_absolute() { p } else { root.join(p) })
}

/// The clone's common git folder with links resolved, so every working tree names it the
/// same way. An error says why it is not known.
pub fn common_dir(root: &Path) -> Result<PathBuf, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err("git rev-parse --git-common-dir failed".into());
    }
    let p = common_from_git(root, &String::from_utf8_lossy(&out.stdout))
        .ok_or("git rev-parse --git-common-dir gave no path")?;
    std::fs::canonicalize(&p).map_err(|e| format!("{}: {e}", p.display()))
}

/// The ledger folder of the repo whose working tree is at `root`, and the problem when the
/// clone could not be told. With a problem the id is a folder of its own that no other run
/// reads as the clone's, and the caller treats the ledger as not readable: nothing is reused.
pub fn repo_id(root: &Path) -> (String, Option<String>) {
    id_or_problem(root, common_dir(root))
}

/// `repo_id`, given what was found of the clone's git folder.
pub fn id_or_problem(root: &Path, common: Result<PathBuf, String>) -> (String, Option<String>) {
    match common {
        Ok(c) => (id_from_common(&c), None),
        Err(e) => (
            format!("{}-clone-unknown", folder_id(root)),
            Some(format!(
                "which clone this is could not be told ({e}): its ledger is not read"
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn every_working_tree_of_a_clone_has_the_clones_id() {
        let common = PathBuf::from("/w/proj/.git");
        let (main, p1) = id_or_problem(Path::new("/w/proj"), Ok(common.clone()));
        let (tree, p2) = id_or_problem(Path::new("/elsewhere/feature-x"), Ok(common));
        assert_eq!(main, tree);
        assert!(main.starts_with("proj-"), "{main}");
        assert_eq!(main.len(), "proj-".len() + 8);
        assert_eq!((p1, p2), (None, None));
    }

    #[test]
    fn two_clones_in_folders_of_one_name_are_two_ledgers() {
        let a = id_from_common(Path::new("/a/proj/.git"));
        let b = id_from_common(Path::new("/b/proj/.git"));
        assert_ne!(a, b);
        assert!(a.starts_with("proj-") && b.starts_with("proj-"));
    }

    #[test]
    fn a_bare_repo_is_named_by_its_own_folder() {
        let id = id_from_common(Path::new("/srv/git/proj.git"));
        assert!(id.starts_with("proj-git-"), "{id}");
    }

    #[test]
    fn a_clone_that_cannot_be_told_is_a_problem_and_a_folder_of_its_own() {
        let (id, problem) = id_or_problem(Path::new("/w/my proj"), Err("no git".into()));
        assert_eq!(id, "my-proj-clone-unknown");
        assert!(problem.is_some_and(|p| p.contains("no git") && p.contains("not read")));
        // never the id a clone in that folder has
        assert_ne!(id, id_from_common(Path::new("/w/my proj/.git")));
    }

    #[test]
    fn gits_answer_is_one_line_and_a_relative_one_is_under_the_root() {
        let root = Path::new("/w/proj");
        assert_eq!(
            common_from_git(root, ".git\n"),
            Some(PathBuf::from("/w/proj/.git"))
        );
        let abs = if cfg!(windows) {
            "C:/w/proj/.git"
        } else {
            "/w/proj/.git"
        };
        assert_eq!(
            common_from_git(root, &format!("{abs}\n")),
            Some(PathBuf::from(abs))
        );
        assert_eq!(common_from_git(root, "--some-option\n.git\n"), None);
        assert_eq!(common_from_git(root, ""), None);
    }
}
