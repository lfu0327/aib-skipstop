//! Knife tests: break one line on purpose, run the test that guards it, put the line back.
//!
//! A test that still passes with its rule taken away guards nothing. Each knife names a file,
//! the text to take out (`old`, which must be in the file exactly once), what to put in its
//! place (`new`), the command to run and the test that has to fail.
//!
//! The knives are data, `knives/*.json`: `{"knives": [{"id", "why", "file", "old", "new",
//! "test", "cmd": [...]}]}`. No script and no other language runs them.
//!
//! - **bites**: the command failed and its output names `test` as failed (`test <name> ...
//!   FAILED`). Everything going red does not count: the knife must hit its own test.
//! - **misses**: anything else with the knife in: the command passed, or it failed without
//!   naming the test (a knife that does not compile fails every test and proves nothing).
//! - **invalid**: `old` is not in the file exactly once; nothing was changed.
//!
//! The file is put back by writing the original text again, so its modification time is new
//! and the next build does not reuse the knifed one. The original is also kept beside the
//! build folder first, and put back by the next run if this one was killed.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One knife as its file gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knife {
    pub id: String,
    /// What rule it takes away, for a person.
    pub why: String,
    pub file: String,
    pub old: String,
    pub new: String,
    /// The test that has to fail (a part of its name is enough, as with a test filter).
    pub test: String,
    pub cmd: Vec<String>,
}

/// Reads a knife file. Every field is needed and of its kind; an id comes once.
pub fn parse(text: &str) -> Result<Vec<Knife>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let list = v
        .get("knives")
        .and_then(Value::as_array)
        .ok_or("no \"knives\" list")?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (i, k) in list.iter().enumerate() {
        let s = |f: &str| -> Result<String, String> {
            k.get(f)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("knife {}: no \"{f}\"", i + 1))
        };
        let cmd: Vec<String> = k
            .get("cmd")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if cmd.is_empty() || cmd.len() != k["cmd"].as_array().map_or(0, Vec::len) {
            return Err(format!("knife {}: \"cmd\" is not a list of words", i + 1));
        }
        let knife = Knife {
            id: s("id")?,
            why: s("why")?,
            file: s("file")?,
            old: s("old")?,
            // taking text out without putting anything in is a knife too
            new: k
                .get("new")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("knife {}: no \"new\"", i + 1))?
                .to_string(),
            test: s("test")?,
            cmd,
        };
        if knife.old == knife.new {
            return Err(format!(
                "knife {}: \"old\" and \"new\" are the same",
                knife.id
            ));
        }
        if !seen.insert(knife.id.clone()) {
            return Err(format!("knife id {} comes twice", knife.id));
        }
        out.push(knife);
    }
    if out.is_empty() {
        return Err("the \"knives\" list is empty".into());
    }
    Ok(out)
}

/// How a knife came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Bites,
    /// The command's exit code (None: ended by a signal) and the tests it named as failed.
    Misses {
        rc: Option<i32>,
        failed: Vec<String>,
    },
    /// How many times `old` is in the file.
    Invalid {
        found: usize,
    },
}

/// The names in `test <name> ... FAILED` lines of a test run's output.
pub fn failed_tests(output: &str) -> Vec<String> {
    let mut v: BTreeSet<String> = BTreeSet::new();
    for line in output.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("test ") {
            if rest.ends_with("FAILED") {
                if let Some(name) = rest.split_whitespace().next() {
                    v.insert(name.to_string());
                }
            }
        }
    }
    v.into_iter().collect()
}

/// The verdict of a run with the knife in: `ok` = the command's exit was success.
pub fn judge(test: &str, ok: bool, rc: Option<i32>, output: &str) -> Verdict {
    let failed = failed_tests(output);
    if !ok && failed.iter().any(|t| t.contains(test)) {
        Verdict::Bites
    } else {
        Verdict::Misses { rc, failed }
    }
}

/// How often `old` is in `text` (not overlapping, as the replacement reads it).
pub fn count(text: &str, old: &str) -> usize {
    text.matches(old).count()
}

const BACKUP: &str = "knife-backup";

fn backup_dir(root: &Path) -> PathBuf {
    root.join("target").join(BACKUP)
}

/// Puts back what a killed run left knifed: each kept original is written over its file and
/// the copy removed. Returns how many files.
pub fn recover(root: &Path) -> usize {
    let dir = backup_dir(root);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let mut n = 0;
    for e in rd.flatten() {
        let p = e.path();
        let (Some(name), Ok(text)) = (
            p.file_name().and_then(|n| n.to_str()),
            std::fs::read_to_string(&p),
        ) else {
            continue;
        };
        // the copy's name is the file's path with `/` written as `%`
        let rel = name.replace('%', "/");
        if std::fs::write(root.join(&rel), text).is_ok() && std::fs::remove_file(&p).is_ok() {
            n += 1;
        }
    }
    n
}

/// The original of one file, kept until it is put back (also when the run panics).
struct Held {
    file: PathBuf,
    copy: PathBuf,
    orig: String,
}

impl Drop for Held {
    fn drop(&mut self) {
        // a new modification time on purpose: the next build must not reuse the knifed one
        if std::fs::write(&self.file, &self.orig).is_ok() {
            let _ = std::fs::remove_file(&self.copy);
        }
    }
}

/// Runs one knife under `root`: the anchor is checked, the knife written in, the command run
/// with the test name as its last word, and the file put back whatever happened.
pub fn run_one(root: &Path, k: &Knife) -> Result<Verdict, String> {
    let file = root.join(&k.file);
    let orig = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", k.file))?;
    let found = count(&orig, &k.old);
    if found != 1 {
        return Ok(Verdict::Invalid { found });
    }
    let dir = backup_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let copy = dir.join(k.file.replace(['/', '\\'], "%"));
    std::fs::write(&copy, &orig).map_err(|e| format!("{}: {e}", copy.display()))?;
    let held = Held {
        file: file.clone(),
        copy,
        orig: orig.clone(),
    };
    std::fs::write(&file, orig.replacen(&k.old, &k.new, 1))
        .map_err(|e| format!("{}: {e}", k.file))?;
    let out = Command::new(&k.cmd[0])
        .args(&k.cmd[1..])
        .arg(&k.test)
        .current_dir(root)
        .output();
    drop(held);
    let out = out.map_err(|e| format!("{}: {e}", k.cmd[0]))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(judge(
        &k.test,
        out.status.success(),
        out.status.code(),
        &text,
    ))
}

/// One line for a knife's verdict.
pub fn line(k: &Knife, v: &Verdict) -> String {
    match v {
        Verdict::Bites => format!("KNIFE {} {}: BITES", k.id, k.test),
        Verdict::Misses { rc, failed } => format!(
            "KNIFE {} {}: MISSES rc={} failed={failed:?} ({})",
            k.id,
            k.test,
            rc.map_or("signal".to_string(), |c| c.to_string()),
            k.why
        ),
        Verdict::Invalid { found } => format!(
            "KNIFE {} {}: INVALID \"old\" is {found} times in {}, not once; nothing changed",
            k.id, k.test, k.file
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"{"knives": [{"id": "K1", "why": "the sum skips a number",
        "file": "src/lib.rs", "old": "a + b", "new": "a", "test": "adds", "cmd": ["cargo", "test"]}]}"#;

    #[test]
    fn a_knife_file_needs_every_field_and_an_id_once() {
        let k = parse(ONE).unwrap();
        assert_eq!(k.len(), 1);
        assert_eq!(
            (k[0].id.as_str(), k[0].old.as_str(), k[0].new.as_str()),
            ("K1", "a + b", "a")
        );
        assert_eq!(k[0].cmd, vec!["cargo", "test"]);
        for field in ["id", "why", "file", "old", "new", "test", "cmd"] {
            let mut v: Value = serde_json::from_str(ONE).unwrap();
            v["knives"][0].as_object_mut().unwrap().remove(field);
            let e = parse(&v.to_string()).unwrap_err();
            assert!(e.contains(field), "{field}: {e}");
        }
        // nothing put in the place is a knife; nothing taken out is not
        let mut v: Value = serde_json::from_str(ONE).unwrap();
        v["knives"][0]["new"] = Value::from("");
        assert_eq!(parse(&v.to_string()).unwrap()[0].new, "");
        v["knives"][0]["new"] = Value::from("a + b");
        assert!(parse(&v.to_string()).unwrap_err().contains("the same"));
        // the same id twice, a word of the command that is not text, no knife at all
        let mut v: Value = serde_json::from_str(ONE).unwrap();
        let first = v["knives"][0].clone();
        v["knives"].as_array_mut().unwrap().push(first);
        assert!(parse(&v.to_string()).unwrap_err().contains("twice"));
        let mut v: Value = serde_json::from_str(ONE).unwrap();
        v["knives"][0]["cmd"] = serde_json::json!(["cargo", 1]);
        assert!(parse(&v.to_string()).is_err());
        assert!(parse(r#"{"knives": []}"#).unwrap_err().contains("empty"));
        assert!(parse("[]").is_err());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn a_knife_bites_only_when_the_command_fails_and_names_its_own_test() {
        let named = "test tests::adds ... FAILED\ntest tests::other ... ok\n";
        assert_eq!(judge("adds", false, Some(101), named), Verdict::Bites);
        // the command passed with the knife in: the test guards nothing
        assert!(matches!(
            judge("adds", true, Some(0), named),
            Verdict::Misses { .. }
        ));
        // it failed, but another test did: not this knife's doing
        let other = "test tests::other ... FAILED\n";
        assert_eq!(
            judge("adds", false, Some(101), other),
            Verdict::Misses {
                rc: Some(101),
                failed: vec!["tests::other".into()]
            }
        );
        // it did not compile: every test "fails" and none is named
        let broken = "error[E0425]: cannot find value `b`\nerror: could not compile `x`\n";
        assert_eq!(
            judge("adds", false, Some(101), broken),
            Verdict::Misses {
                rc: Some(101),
                failed: vec![]
            }
        );
        assert_eq!(
            failed_tests("test a::b ... FAILED\r\ntest a::b ... FAILED\ntest c ... ok\n  test d ... FAILED\n"),
            vec!["a::b".to_string()]
        );
    }

    fn knife(file: &str, old: &str, new: &str, cmd: &[&str]) -> Knife {
        Knife {
            id: "K".into(),
            why: "why".into(),
            file: file.into(),
            old: old.into(),
            new: new.into(),
            test: "adds".into(),
            cmd: cmd.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_knife_is_in_while_the_command_runs_and_the_file_is_put_back_after() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("f.txt"), "let s = a + b;\n").unwrap();
        let mut k = knife("f.txt", "a + b", "a", &[]);
        // the command reads the file while the knife is in
        k.cmd = vec![
            "sh".into(),
            "-c".into(),
            "grep -q 'let s = a;' f.txt && echo 'test tests::adds ... FAILED'; exit 1".into(),
            "sh".into(),
        ];
        assert_eq!(run_one(d.path(), &k).unwrap(), Verdict::Bites);
        assert_eq!(
            std::fs::read_to_string(d.path().join("f.txt")).unwrap(),
            "let s = a + b;\n"
        );
        // nothing is left in the backup folder
        assert_eq!(
            std::fs::read_dir(d.path().join("target/knife-backup"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn an_anchor_that_is_not_there_once_changes_nothing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("f.txt"), "a + b; a + b;\n").unwrap();
        let k = knife("f.txt", "a + b", "a", &["no-such-program-is-run"]);
        assert_eq!(
            run_one(d.path(), &k).unwrap(),
            Verdict::Invalid { found: 2 }
        );
        let k0 = knife("f.txt", "x + y", "x", &["no-such-program-is-run"]);
        assert_eq!(
            run_one(d.path(), &k0).unwrap(),
            Verdict::Invalid { found: 0 }
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join("f.txt")).unwrap(),
            "a + b; a + b;\n"
        );
        assert!(run_one(d.path(), &knife("gone.txt", "a", "b", &["x"])).is_err());
    }

    #[test]
    fn a_command_that_cannot_start_still_leaves_the_file_as_it_was() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("f.txt"), "a + b\n").unwrap();
        let k = knife("f.txt", "a + b", "a", &["no-such-program-anywhere-9f3"]);
        assert!(run_one(d.path(), &k).is_err());
        assert_eq!(
            std::fs::read_to_string(d.path().join("f.txt")).unwrap(),
            "a + b\n"
        );
    }

    #[test]
    fn a_killed_runs_knifed_file_is_put_back_by_the_next_run() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("crates/x/src");
        std::fs::create_dir_all(&sub).unwrap();
        // what a killed run leaves: the knife in the file, the original in the backup folder
        std::fs::write(sub.join("lib.rs"), "knifed\n").unwrap();
        let b = d.path().join("target/knife-backup");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("crates%x%src%lib.rs"), "original\n").unwrap();
        assert_eq!(recover(d.path()), 1);
        assert_eq!(
            std::fs::read_to_string(sub.join("lib.rs")).unwrap(),
            "original\n"
        );
        assert_eq!(std::fs::read_dir(&b).unwrap().count(), 0);
        assert_eq!(recover(d.path()), 0);
    }

    #[test]
    fn a_verdict_line_names_the_knife_its_test_and_what_went_wrong() {
        let k = knife("f.txt", "a", "b", &["x"]);
        assert_eq!(line(&k, &Verdict::Bites), "KNIFE K adds: BITES");
        let m = line(
            &k,
            &Verdict::Misses {
                rc: Some(0),
                failed: vec![],
            },
        );
        assert!(
            m.starts_with("KNIFE K adds: MISSES rc=0") && m.ends_with("(why)"),
            "{m}"
        );
        let i = line(&k, &Verdict::Invalid { found: 3 });
        assert!(
            i.contains("INVALID") && i.contains("3 times in f.txt"),
            "{i}"
        );
    }
}
