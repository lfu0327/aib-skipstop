//! Real mode end to end, with the built `aib-skipstop` on a small cargo workspace made here:
//! what is not run, what runs again after a change, and that a failure is never reused or
//! hidden. Needs git and cargo-nextest; without nextest the test fails and says so, unless
//! `SKIPSTOP_ALLOW_NO_NEXTEST=1` (a test that did not run is not a test that passed).
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

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
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
        .unwrap();
    assert!(
        ok.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
}

const LIB_A: &str = "pub fn a() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn a_is_one() {\n        assert_eq!(super::a(), 1);\n    }\n}\n";
// b's test also reads something no key holds: an environment variable outside the context's list
const LIB_B: &str = "pub fn b() -> u32 {\n    2\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn b_is_two() {\n        assert_eq!(super::b(), 2);\n        assert!(std::env::var(\"SKIPSTOP_TEST_HIDDEN\").is_err());\n    }\n}\n";

fn workspace() -> Tmp {
    let d = std::env::temp_dir().join(format!("skipstop-real-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    for p in ["ws/crates/a/src", "ws/crates/b/src", "state"] {
        fs::create_dir_all(d.join(p)).unwrap();
    }
    let ws = d.join("ws");
    let pkg =
        |n: &str| format!("[package]\nname = \"{n}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n");
    for (p, c) in [
        (
            "Cargo.toml",
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/a\", \"crates/b\"]\n".to_string(),
        ),
        ("crates/a/Cargo.toml", pkg("a")),
        ("crates/a/src/lib.rs", LIB_A.to_string()),
        ("crates/b/Cargo.toml", pkg("b")),
        ("crates/b/src/lib.rs", LIB_B.to_string()),
        (".gitignore", "target/\n".to_string()),
    ] {
        fs::write(ws.join(p), c).unwrap();
    }
    let lock = Command::new("cargo")
        .current_dir(&ws)
        .args(["generate-lockfile", "--offline"])
        .output()
        .unwrap();
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    git(&ws, &["init", "-q", "-b", "main"]);
    git(&ws, &["add", "-A"]);
    git(&ws, &["commit", "-q", "-m", "start"]);
    Tmp(d)
}

/// A seed under which neither unit is drawn for a random audit at the early rate.
fn quiet_seed() -> String {
    (0u64..)
        .find(|s| {
            ["a", "b"]
                .iter()
                .all(|u| skipstop_audit::draw(*s, u) >= skipstop_audit::EARLY_RATE)
        })
        .unwrap()
        .to_string()
}

/// The one repo's folder in the ledger of the default state folder.
fn repo_ledger(t: &Tmp) -> PathBuf {
    let dir = t.0.join("state/ledger");
    fs::read_dir(dir).unwrap().flatten().next().unwrap().path()
}

struct Out {
    rc: i32,
    json: serde_json::Value,
}

impl Out {
    fn plan(&self, unit: &str) -> String {
        self.unit(unit)["plan"].as_str().unwrap().to_string()
    }
    fn result(&self, unit: &str) -> String {
        self.unit(unit)["result"].as_str().unwrap().to_string()
    }
    fn unit(&self, unit: &str) -> &serde_json::Value {
        self.json["units"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["unit"] == unit)
            .unwrap()
    }
}

fn run(t: &Tmp, args: &[&str]) -> Out {
    run_env(t, args, None)
}

/// `run` with the hidden input of b's test set or not.
fn run_env(t: &Tmp, args: &[&str], hidden: Option<&str>) -> Out {
    run_state(t, "state", args, hidden)
}

/// `run_env` with the ledger in another state folder of the same workspace (a fresh history).
fn run_state(t: &Tmp, state: &str, args: &[&str], hidden: Option<&str>) -> Out {
    let mut c = Command::new(env!("CARGO_BIN_EXE_aib-skipstop"));
    match hidden {
        Some(v) => c.env("SKIPSTOP_TEST_HIDDEN", v),
        None => c.env_remove("SKIPSTOP_TEST_HIDDEN"),
    };
    let o = c
        .current_dir(t.0.join("ws"))
        .args(args)
        .args(["--json", "--state"])
        .arg(t.0.join(state))
        .env_remove("AIB_SEAT")
        .env("AIB_SKIPSTOP_SEED", quiet_seed())
        .env_remove("AIB_SKIPSTOP_MODE")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    let json = serde_json::from_str(text.trim()).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}): {text}\n{}",
            String::from_utf8_lossy(&o.stderr)
        )
    });
    Out {
        rc: o.status.code().unwrap_or(-1),
        json,
    }
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
fn real_mode_skips_what_did_not_change_and_never_reuses_or_hides_a_failure() {
    let have = Command::new("cargo")
        .args(["nextest", "--version"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !have {
        // not quiet: a test that did not run is not a test that passed
        assert!(
            std::env::var("SKIPSTOP_ALLOW_NO_NEXTEST").as_deref() == Ok("1"),
            "cargo-nextest is not installed: install it (cargo install cargo-nextest --locked), \
             or set SKIPSTOP_ALLOW_NO_NEXTEST=1 to let this test pass without running"
        );
        eprintln!("cargo-nextest is not installed: the end-to-end test did not run");
        return;
    }
    let t = workspace();
    let lib_a = t.0.join("ws/crates/a/src/lib.rs");

    // the first real-mode run is the defense round: everything runs, nothing is saved
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(r.rc, 0, "{}", r.json);
    assert_eq!(r.json["defense_round"], true);
    assert_eq!(
        (r.plan("a"), r.plan("b")),
        ("run:defense_round".into(), "run:defense_round".into())
    );
    assert_eq!(saved(&t)["all"]["total"]["count"], 0);
    // run outside AIB (no seat set): the ledger says `local`, not a made-up seat
    let ledger_dir = t.0.join("state/ledger");
    let mut lines = String::new();
    for repo in fs::read_dir(&ledger_dir).unwrap().flatten() {
        for f in fs::read_dir(repo.path()).unwrap().flatten() {
            lines += &fs::read_to_string(f.path()).unwrap();
        }
    }
    assert!(lines.contains("\"seat\":\"local\""), "{lines}");
    assert!(!lines.contains("\"seat\":\"unknown\""));
    // a mode file that is there and is not a mode stops the run instead of running as observe
    fs::write(t.0.join("state/mode"), "reel\n").unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_aib-skipstop"))
        .current_dir(t.0.join("ws"))
        .args(["--mode", "real", "--state"])
        .arg(t.0.join("state"))
        .env_remove("AIB_SKIPSTOP_MODE")
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(64),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    fs::remove_file(t.0.join("state/mode")).unwrap();

    // nothing changed: neither unit runs, and each names the run it is reused from
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(r.rc, 0, "{}", r.json);
    assert_eq!(r.json["defense_round"], false);
    assert_eq!((r.plan("a"), r.plan("b")), ("reuse".into(), "reuse".into()));
    assert_eq!(
        (r.result("a"), r.result("b")),
        ("pass".into(), "pass".into())
    );
    assert!(r.unit("a")["would_reuse"]["from"]
        .as_str()
        .is_some_and(|f| !f.is_empty()));
    assert_eq!(saved(&t)["all"]["skip"]["count"], 2);
    // nextest was not called for them: no test case ran at all
    assert_eq!(r.json["cases_run"], 0, "{}", r.json);

    // a round of one package is no round for the others: a third crate, added and committed,
    // is new to real mode and runs as its own round while a and b are reused
    let ws = t.0.join("ws");
    fs::create_dir_all(ws.join("crates/c/src")).unwrap();
    let c_toml = "[package]\nname = \"c\"\nversion = \"0.0.0\"\nedition = \"2021\"\n";
    fs::write(ws.join("crates/c/Cargo.toml"), c_toml).unwrap();
    let lib_c = LIB_A.replace("a()", "c()").replace("a_is_one", "c_is_one");
    fs::write(ws.join("crates/c/src/lib.rs"), lib_c).unwrap();
    let members =
        "[workspace]\nresolver = \"2\"\nmembers = [\"crates/a\", \"crates/b\", \"crates/c\"]\n";
    fs::write(ws.join("Cargo.toml"), members).unwrap();
    let lock = Command::new("cargo")
        .current_dir(&ws)
        .args(["generate-lockfile", "--offline"])
        .output()
        .unwrap();
    assert!(lock.status.success());
    git(&ws, &["add", "-A"]);
    git(&ws, &["commit", "-q", "-m", "c"]);
    // (the workspace files changed, so every key did: one run to give a and b sources again)
    let r = run(&t, &["--mode", "real", "-p", "a", "-p", "b"]);
    assert_eq!(r.rc, 0, "{}", r.json);
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(r.rc, 0, "{}", r.json);
    assert_eq!((r.plan("a"), r.plan("b")), ("reuse".into(), "reuse".into()));
    assert_eq!(r.plan("c"), "run:defense_round", "{}", r.json);

    // --plan says what a run would do and does none of it: no test case, no ledger line
    let streams_before = fs::read_dir(repo_ledger(&t)).unwrap().count();
    let r = run(&t, &["--mode", "real", "--plan"]);
    assert_eq!(r.rc, 0, "{}", r.json);
    assert_eq!(r.json["plan_only"], true);
    assert_eq!(r.json["would_run"], 0, "nothing changed: {}", r.json);
    assert!(r.json["decided_in_ms"].as_i64().is_some_and(|ms| ms >= 0));
    assert_eq!(
        fs::read_dir(repo_ledger(&t)).unwrap().count(),
        streams_before
    );

    // --fresh runs everything whatever the ledger says
    let r = run(&t, &["--mode", "real", "--fresh"]);
    assert_eq!(
        (r.rc, r.plan("a"), r.plan("b")),
        (0, "run:fresh".into(), "run:fresh".into())
    );

    // observe mode never skips
    let r = run(&t, &[]);
    assert_eq!(
        (r.rc, r.plan("a"), r.plan("b")),
        (0, "run:observe".into(), "run:observe".into())
    );

    // a breaks: it runs and fails, b is still reused, and the run is not green
    fs::write(&lib_a, LIB_A.replace("    1\n", "    7\n")).unwrap();
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(r.rc, 1, "{}", r.json);
    assert_eq!(
        (r.plan("a"), r.result("a")),
        ("run:no_source".into(), "fail".into())
    );
    assert_eq!(
        (r.plan("b"), r.result("b")),
        ("reuse".into(), "pass".into())
    );
    assert_eq!(r.json["cases_run"], 1, "only a's test ran: {}", r.json);

    // the same broken tree again: the failure is not a source, so a runs and fails again
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(r.rc, 1, "{}", r.json);
    assert_eq!(
        (r.plan("a"), r.result("a")),
        ("run:no_source".into(), "fail".into())
    );

    // put back: the key is the old one again and the old green run is its source
    fs::write(&lib_a, LIB_A).unwrap();
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(
        (r.rc, r.plan("a"), r.result("a")),
        (0, "reuse".into(), "pass".into())
    );

    // an input no key holds changes (what a defense round is for): b's key is the same, so it
    // would be reused. Run for real, it disagrees with its source: the run is not green and
    // the source is revoked
    let before = saved(&t);
    let r = run_env(&t, &["--mode", "real", "--fresh"], Some("1"));
    assert_eq!(r.rc, 1, "{}", r.json);
    assert_eq!(r.result("b"), "fail");
    assert_eq!(r.unit("b")["audit"]["revoke"], true, "{}", r.json);
    assert_eq!(r.unit("a")["audit"]["revoke"], false, "{}", r.json);
    // what earlier runs saved by reusing b from that source is taken back, and only that
    let after = saved(&t);
    let n = |v: &serde_json::Value, k: &str| v["all"]["skip"][k].as_u64().unwrap();
    assert!(n(&after, "penalty_ms") > 0, "{after}");
    assert!(n(&after, "count") < n(&before, "count"), "{before} {after}");
    assert_eq!(
        n(&before, "confirmed_ms") - n(&after, "confirmed_ms"),
        n(&after, "penalty_ms"),
        "{before} {after}"
    );
    assert_eq!(after["unread"], 0);
    // the hidden input is gone again, but a revoked source is never a source again: b runs
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(
        (r.rc, r.plan("b"), r.result("b")),
        (0, "run:no_source".into(), "pass".into())
    );
    assert_eq!(r.plan("a"), "reuse");

    // and again: a key a real run disagreed on is never a source again, however green it is now
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(
        (r.rc, r.plan("b"), r.result("b")),
        (0, "run:no_source".into(), "pass".into()),
        "{}",
        r.json
    );
    assert_eq!(r.plan("a"), "reuse");

    // flaky without any revocation, in a history of its own: b's first run fails (the hidden
    // input is set; there is no source yet to disagree with), then it passes on the very same
    // key. It has a green source now, and its green proves nothing: it runs every time
    let r = run_state(&t, "state2", &["--mode", "real"], Some("1"));
    assert_eq!((r.rc, r.result("b")), (1, "fail".into()), "{}", r.json);
    let r = run_state(&t, "state2", &["--mode", "real"], None);
    assert_eq!(
        (r.rc, r.plan("b"), r.result("b")),
        (0, "run:no_source".into(), "pass".into())
    );
    for _ in 0..2 {
        let r = run_state(&t, "state2", &["--mode", "real"], None);
        assert_eq!(
            (r.rc, r.plan("b"), r.result("b")),
            (0, "run:flaky".into(), "pass".into()),
            "{}",
            r.json
        );
        assert_eq!(r.plan("a"), "reuse");
    }

    // a ledger file that cannot be read whole: it may hold a newer failure or a revocation,
    // so nothing is reused until it is dealt with, and the run says which file
    let repo_dir = fs::read_dir(&ledger_dir)
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    let junk = repo_dir.join("tr-0-junk.jsonl");
    fs::write(&junk, "{\"not\": \"a ledger line\"}\n").unwrap();
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(r.rc, 0, "{}", r.json);
    assert_eq!(
        (r.plan("a"), r.plan("c")),
        ("run:ledger_unread".into(), "run:ledger_unread".into())
    );
    assert!(
        r.json["ledger_problems"]
            .as_array()
            .is_some_and(|p| p.len() == 1),
        "{}",
        r.json
    );
    assert!(
        r.json["ledger_problems"][0]
            .as_str()
            .is_some_and(|p| p.contains("tr-0-junk.jsonl")),
        "the problem names the file: {}",
        r.json
    );
    fs::remove_file(&junk).unwrap();
    assert_eq!(run(&t, &["--mode", "real"]).plan("a"), "reuse");

    // the mode can be set for the state folder: a plain run is then a real-mode run, and
    // --mode on the command line still wins
    let mode = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_aib-skipstop"))
            .args(args)
            .arg("--state")
            .arg(t.0.join("state"))
            .env_remove("AIB_SKIPSTOP_MODE")
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
        )
    };
    assert_eq!(
        mode(&["mode"]),
        (Some(0), "observe (from the default)".into())
    );
    assert_eq!(
        mode(&["mode", "real"]),
        (Some(0), "real (from the mode file)".into())
    );
    let r = run(&t, &[]);
    assert_eq!(
        (r.rc, r.json["mode"].as_str()),
        (0, Some("real")),
        "{}",
        r.json
    );
    assert_eq!(r.plan("a"), "reuse");
    let r = run(&t, &["--mode", "observe"]);
    assert_eq!(r.json["mode"], "observe");
    assert_eq!(mode(&["mode", "observe"]).1, "observe (from the mode file)");

    // a word that is not a mode is refused, not read as one
    let o = Command::new(env!("CARGO_BIN_EXE_aib-skipstop"))
        .current_dir(t.0.join("ws"))
        .args(["--mode", "fast"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(64));
}
