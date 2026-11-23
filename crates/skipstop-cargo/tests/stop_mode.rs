//! `aib-skipstop stop` end to end with the built binary, on a git repo made here. The checks
//! and the protected command are git commands, so the test runs the same on every system:
//! `git diff --quiet HEAD -- <file>` is 0 when the file is as committed and 1 when it changed.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap()
        .status
        .success()
}

/// The checks: `changed` is a finding when `flag.txt` is not as committed; `guess` warns when
/// `hint.txt` is not; `broken` asks git for a ref that is not there (exit 128).
fn config(with_broken: bool) -> String {
    let mut checks = vec![
        r#"{"id": "changed", "kind": "reject", "cmd": ["git", "diff", "--quiet", "HEAD", "--", "flag.txt"],
            "avoids": "an hour of tests on a tree that cannot pass", "saves_ms": 3600000}"#
            .to_string(),
        r#"{"id": "guess", "kind": "warn", "cmd": ["git", "diff", "--quiet", "HEAD", "--", "hint.txt"],
            "avoids": "maybe a round", "saves_ms": 1}"#
            .to_string(),
    ];
    if with_broken {
        checks.push(
            r#"{"id": "broken", "kind": "reject", "cmd": ["git", "rev-parse", "--verify", "--quiet", "refs/heads/no-such-branch^{commit}"],
                "avoids": "nothing", "stop_codes": [77]}"#
                .to_string(),
        );
    }
    format!("{{\"checks\": [{}]}}", checks.join(","))
}

fn repo(name: &str) -> Tmp {
    let d = std::env::temp_dir().join(format!("skipstop-stop-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("ws")).unwrap();
    let ws = d.join("ws");
    for f in ["flag.txt", "hint.txt", "work.txt"] {
        fs::write(ws.join(f), "as committed\n").unwrap();
    }
    assert!(git(&ws, &["init", "-q", "-b", "main"]));
    assert!(git(&ws, &["add", "-A"]));
    assert!(git(&ws, &["commit", "-q", "-m", "start"]));
    fs::write(ws.join("skipstop.stop.json"), config(false)).unwrap();
    Tmp(d)
}

struct Out {
    rc: i32,
    text: String,
}

/// `aib-skipstop stop <args>`; the protected command tags the commit, so a tag that is
/// there says the command ran.
fn stop(t: &Tmp, args: &[&str], tag: Option<&str>) -> Out {
    stop_in(t, "", args, tag)
}

/// `stop` started in a folder inside the repo.
fn stop_in(t: &Tmp, sub: &str, args: &[&str], tag: Option<&str>) -> Out {
    let mut c = Command::new(env!("CARGO_BIN_EXE_aib-skipstop"));
    c.current_dir(t.0.join("ws").join(sub))
        .arg("stop")
        .args(args)
        .arg("--state")
        .arg(t.0.join("state"))
        .env_remove("AIB_SKIPSTOP_STOP_MODE")
        .env_remove("AIB_SEAT");
    if let Some(tag) = tag {
        c.args(["--", "git", "tag", tag]);
    }
    let o = c.output().unwrap();
    Out {
        rc: o.status.code().unwrap_or(-1),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
    }
}

fn ran(t: &Tmp, tag: &str) -> bool {
    git(
        &t.0.join("ws"),
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/tags/{tag}"),
        ],
    )
}

fn saved(t: &Tmp) -> serde_json::Value {
    let o = Command::new(env!("CARGO_BIN_EXE_aib-skipstop"))
        .args(["saved", "--json", "--state"])
        .arg(t.0.join("state"))
        .output()
        .unwrap();
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn a_reject_finding_stops_the_command_in_real_mode_and_only_there() {
    let t = repo("real");
    let ws = t.0.join("ws");

    // nothing to find: the command runs, in either mode
    let o = stop(&t, &["--mode", "real"], Some("t1"));
    assert_eq!(o.rc, 0, "{}", o.text);
    assert!(
        ran(&t, "t1") && o.text.contains("STOP checks=2 pass=2 stop=0"),
        "{}",
        o.text
    );

    // the reject check has a finding. Observe mode (the default): said, recorded as a
    // prediction, and the command runs anyway; it passes, so that stop would have been wrong
    fs::write(ws.join("flag.txt"), "changed\n").unwrap();
    let o = stop(&t, &[], Some("t2"));
    assert_eq!(o.rc, 0, "{}", o.text);
    assert!(ran(&t, "t2"), "{}", o.text);
    assert!(
        o.text
            .contains("STOP-CHECK changed reject: an hour of tests"),
        "{}",
        o.text
    );
    assert!(
        o.text.contains("mode=observe (from the default)"),
        "{}",
        o.text
    );
    assert!(
        o.text
            .contains("STOP-OBSERVED would block good work: changed"),
        "{}",
        o.text
    );
    let s = saved(&t);
    assert_eq!(
        (s["all"]["total"]["count"].as_u64(), s["observed"].as_u64()),
        (Some(0), Some(1)),
        "{s}"
    );

    // real mode: the command is not run, the exit says so, and the stop is in the total
    let o = stop(&t, &["--mode", "real"], Some("t3"));
    assert_eq!(o.rc, 10, "{}", o.text);
    assert!(!ran(&t, "t3"), "{}", o.text);
    assert!(o.text.contains("STOPPED by changed"), "{}", o.text);
    let s = saved(&t);
    // the hour is a number a person wrote in the checks file: an estimate, recorded as one,
    // and so not in the confirmed sum (nor in the time on the clock)
    assert_eq!(s["all"]["stop"]["estimated_ms"], 3_600_000, "{s}");
    assert_eq!(s["all"]["stop"]["confirmed_ms"], 0, "{s}");
    assert_eq!(s["all"]["clock_ms"], 0, "{s}");
    assert_eq!(s["all"]["skip"]["confirmed_ms"], 0);

    // `check` runs the checks only and records nothing
    let o = stop(&t, &["check"], None);
    assert_eq!(o.rc, 10, "{}", o.text);
    assert_eq!(saved(&t)["all"]["stop"]["count"], 1);

    // started in a folder inside the repo, the repo's checks still apply (they are the repo's,
    // found at its root and run there)
    fs::create_dir_all(ws.join("sub")).unwrap();
    let o = stop_in(&t, "sub", &["--mode", "real"], Some("t3"));
    assert_eq!(o.rc, 10, "{}", o.text);
    assert!(
        !ran(&t, "t3") && o.text.contains("STOPPED by changed"),
        "{}",
        o.text
    );
    assert_eq!(saved(&t)["all"]["stop"]["estimated_ms"], 7_200_000);
    // the same words started in another folder are another command: passing in sub/ with
    // --no-stop takes back sub/'s stop and leaves the one started at the root standing
    let o = stop_in(&t, "sub", &["--no-stop"], Some("t3"));
    assert_eq!(o.rc, 0, "{}", o.text);
    assert!(o.text.contains("STOP-WRONG 1 stop(s)"), "{}", o.text);
    assert_eq!(saved(&t)["all"]["stop"]["estimated_ms"], 3_600_000);
    assert!(git(&ws, &["tag", "-d", "t3"]));

    // --no-stop with another command passing says nothing about the one that was stopped
    let o = stop(&t, &["--no-stop"], Some("unrelated"));
    assert_eq!(o.rc, 0, "{}", o.text);
    assert!(!o.text.contains("STOP-WRONG"), "{}", o.text);
    assert_eq!(saved(&t)["all"]["stop"]["estimated_ms"], 3_600_000);

    // --no-stop: the person knows better. The command runs and passes, so the stop on this
    // commit was wrong and what it claimed is taken back, once
    // (the very command that was stopped: git tag t3)
    let o = stop(&t, &["--no-stop"], Some("t3"));
    assert_eq!(o.rc, 0, "{}", o.text);
    assert!(
        ran(&t, "t3") && o.text.contains("STOP-WRONG 1 stop(s)"),
        "{}",
        o.text
    );
    let s = saved(&t);
    assert_eq!(
        (
            s["all"]["stop"]["estimated_ms"].as_u64(),
            s["all"]["stop"]["penalty_ms"].as_u64()
        ),
        (Some(0), Some(7_200_000)),
        "{s}"
    );
    let o = stop(&t, &["--no-stop"], Some("t5"));
    assert!(!o.text.contains("STOP-WRONG"), "{}", o.text);
    assert_eq!(saved(&t)["all"]["stop"]["penalty_ms"], 7_200_000);

    // the mode can be set for the state folder, apart from skip's
    fs::write(t.0.join("state/stop-mode"), "real\n").unwrap();
    let o = stop(&t, &[], Some("t6"));
    assert_eq!(o.rc, 10, "{}", o.text);
    assert!(
        o.text.contains("mode=real (from the stop-mode file)") && !ran(&t, "t6"),
        "{}",
        o.text
    );
    fs::write(t.0.join("state/stop-mode"), "reel\n").unwrap();
    assert_eq!(stop(&t, &[], Some("t7")).rc, 64);
    assert!(!ran(&t, "t7"));
}

#[test]
fn a_warning_and_a_broken_check_stop_nothing_and_are_said() {
    let t = repo("soft");
    let ws = t.0.join("ws");
    fs::write(ws.join("skipstop.stop.json"), config(true)).unwrap();
    fs::write(ws.join("hint.txt"), "changed\n").unwrap();
    let o = stop(&t, &["--mode", "real"], Some("t1"));
    assert_eq!(o.rc, 0, "{}", o.text);
    assert!(ran(&t, "t1"), "{}", o.text);
    assert!(
        o.text.contains("STOP-CHECK guess warn: maybe a round"),
        "{}",
        o.text
    );
    assert!(o.text.contains("STOP-CHECK broken BROKEN"), "{}", o.text);
    assert!(o.text.contains("stop=0 warn=1 broken=1"), "{}", o.text);
    // nothing was stopped, so nothing was saved
    assert_eq!(saved(&t)["all"]["total"]["count"], 0);
    // the protected command's own failure is the exit code, not a stop
    let o = stop(&t, &["--mode", "real"], Some("t1"));
    assert!(
        o.rc != 0 && o.rc != 10,
        "tagging t1 twice fails with git's code: {}",
        o.text
    );
}

#[test]
fn no_checks_file_checks_nothing_and_a_named_or_bad_one_is_an_error() {
    let t = repo("files");
    let ws = t.0.join("ws");
    fs::remove_file(ws.join("skipstop.stop.json")).unwrap();
    let o = stop(&t, &["--mode", "real"], Some("t1"));
    assert_eq!(o.rc, 0, "{}", o.text);
    assert!(
        ran(&t, "t1") && o.text.contains("nothing is checked"),
        "{}",
        o.text
    );
    // a file that was named must be there
    let o = stop(&t, &["--config", "no-such.json"], Some("t2"));
    assert_eq!(o.rc, 7, "{}", o.text);
    assert!(!ran(&t, "t2"));
    // a checks file that is not understood stops the run: it is not read as "no checks"
    fs::write(
        ws.join("skipstop.stop.json"),
        r#"{"checks": [{"id": "x", "kind": "block"}]}"#,
    )
    .unwrap();
    let o = stop(&t, &[], Some("t3"));
    assert_eq!(o.rc, 7, "{}", o.text);
    assert!(!ran(&t, "t3"));
    // no command, or an option nobody knows
    assert_eq!(stop(&t, &[], None).rc, 64);
    assert_eq!(stop(&t, &["--frobnicate"], Some("t4")).rc, 64);
}
