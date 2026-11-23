//! Units that are any command, end to end, with the built `aib-skipstop` on a repo that has
//! no cargo workspace at all. The commands are `git grep`, which every system here has: it
//! ends with 0 when the word is in the file and 1 when it is not. Needs git only.
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

// `strict` names the file it reads; `loose` reads other.txt and does not say so
const SETTINGS: &str = r#"{
  "units": {
    "strict": {"run": ["git", "grep", "-q", "good", "--", "data.txt"], "inputs": ["data.txt"]},
    "loose": {"run": ["git", "grep", "-q", "good", "--", "other.txt"], "inputs": ["data.txt"]}
  }
}"#;

struct Out {
    rc: i32,
    json: serde_json::Value,
    err: String,
}

impl Out {
    fn unit(&self, unit: &str) -> &serde_json::Value {
        self.json["units"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["unit"] == unit)
            .unwrap()
    }
    fn plan(&self, unit: &str) -> String {
        self.unit(unit)["plan"].as_str().unwrap().to_string()
    }
    fn result(&self, unit: &str) -> String {
        self.unit(unit)["result"].as_str().unwrap().to_string()
    }
}

fn quiet_seed() -> String {
    (0u64..)
        .find(|s| {
            ["strict", "loose"]
                .iter()
                .all(|u| skipstop_audit::draw(*s, u) >= skipstop_audit::EARLY_RATE)
        })
        .unwrap()
        .to_string()
}

fn run(t: &Tmp, args: &[&str]) -> Out {
    let o = Command::new(env!("CARGO_BIN_EXE_aib-skipstop"))
        .current_dir(t.0.join("repo"))
        .args(args)
        .args(["--json", "--state"])
        .arg(t.0.join("state"))
        .env_remove("AIB_SEAT")
        .env_remove("AIB_SKIPSTOP_MODE")
        .env("AIB_SKIPSTOP_SEED", quiet_seed())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    Out {
        rc: o.status.code().unwrap_or(-1),
        json: serde_json::from_str(text.trim()).unwrap_or(serde_json::Value::Null),
        err,
    }
}

#[test]
fn a_command_is_a_unit_reused_while_its_named_files_stand_and_run_when_they_change() {
    let t = Tmp(std::env::temp_dir().join(format!("skipstop-cmd-{}", std::process::id())));
    let _ = fs::remove_dir_all(&t.0);
    let repo = t.0.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let write = |name: &str, text: &str| fs::write(repo.join(name), text).unwrap();
    let commit = |what: &str| {
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", what]);
    };
    git(&repo, &["init", "-q"]);
    write("data.txt", "good\n");
    write("other.txt", "good\n");
    write("skipstop.json", SETTINGS);
    commit("first");

    // no Cargo.toml anywhere: the units are the two the settings name. The first real-mode
    // run is their defense round; both commands run and pass
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(r.rc, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["units"].as_array().unwrap().len(), 2, "{}", r.json);
    for u in ["strict", "loose"] {
        assert_eq!(
            (r.plan(u), r.result(u), r.unit(u)["tests"].as_u64()),
            ("run:defense_round".into(), "pass".into(), Some(1)),
            "{}",
            r.json
        );
        assert!(
            r.unit(u)["key"].as_str().is_some_and(|k| k.len() == 64),
            "{}",
            r.json
        );
    }
    // nothing changed: neither runs
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(
        (r.rc, r.plan("strict"), r.plan("loose")),
        (0, "reuse".into(), "reuse".into()),
        "{}",
        r.json
    );
    // --plan says the same and runs nothing
    let p = run(&t, &["--mode", "real", "--plan"]);
    assert_eq!(p.json["would_run"], 0, "{}", p.json);

    // a named file changes for the worse: both keys change (both name it), both run, and
    // `strict`, which reads it, fails: the run is red
    write("data.txt", "bad\n");
    commit("data goes bad");
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(
        (r.rc, r.plan("strict"), r.result("strict")),
        (1, "run:no_source".into(), "fail".into()),
        "{} {}",
        r.json,
        r.err
    );
    assert!(r.err.contains("strict: exit code 1"), "{}", r.err);
    assert_eq!(
        (r.plan("loose"), r.result("loose")),
        ("run:no_source".into(), "pass".into())
    );
    // a failure is never a source: it runs again, and fails again
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(
        (r.rc, r.plan("strict"), r.plan("loose")),
        (1, "run:no_source".into(), "reuse".into())
    );
    // mended: back to the first key, whose green source is still there
    write("data.txt", "good\n");
    commit("data mended");
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(
        (r.rc, r.plan("strict"), r.plan("loose")),
        (0, "reuse".into(), "reuse".into()),
        "{}",
        r.json
    );

    // the hazard the settings carry: `loose` reads other.txt and does not name it. It goes
    // bad, the key does not move, and the old green is reused
    write("other.txt", "bad\n");
    commit("other goes bad, and nobody named it");
    let r = run(&t, &["--mode", "real"]);
    assert_eq!((r.rc, r.plan("loose")), (0, "reuse".into()), "{}", r.json);
    // what finds it is a real run: here --fresh, in daily use an audit or the defense round.
    // The run is red, the source is revoked, and it is never reused from again
    let r = run(&t, &["--mode", "real", "--fresh"]);
    assert_eq!(r.rc, 1, "{}", r.json);
    assert_eq!(
        (
            r.result("loose"),
            r.unit("loose")["audit"]["revoke"].as_bool()
        ),
        ("fail".into(), Some(true)),
        "{}",
        r.json
    );
    assert_eq!(r.unit("strict")["audit"]["compare"], "same", "{}", r.json);
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(
        (r.plan("loose"), r.plan("strict")),
        ("run:no_source".into(), "reuse".into()),
        "{}",
        r.json
    );

    // one unit alone; and a unit said never to be reused
    let r = run(&t, &["--mode", "real", "-p", "strict"]);
    assert_eq!(
        (
            r.rc,
            r.json["units"].as_array().unwrap().len(),
            r.plan("strict")
        ),
        (0, 1, "reuse".into())
    );
    write("other.txt", "good\n");
    write(
        "skipstop.json",
        &SETTINGS.replacen(
            "{\n  \"units\"",
            "{\n  \"never_reuse\": {\"strict\": \"reads_outside\"},\n  \"units\"",
            1,
        ),
    );
    commit("strict reads the network");
    for _ in 0..2 {
        let r = run(&t, &["--mode", "real"]);
        assert_eq!(
            (r.rc, r.plan("strict")),
            (0, "run:never_reuse:reads_outside".into()),
            "{} {}",
            r.json,
            r.err
        );
    }

    // a named input that is no file of the repo: the unit has no key, says why, and runs
    write(
        "skipstop.json",
        &SETTINGS.replace(
            "\"inputs\": [\"data.txt\"]},\n",
            "\"inputs\": [\"gone.txt\"]},\n",
        ),
    );
    commit("strict names a file that is not there");
    let r = run(&t, &["--mode", "real", "--plan"]);
    let strict = r.unit("strict");
    assert_eq!(strict["plan"], "run:no_source", "{}", r.json);
    assert!(
        strict["key"].is_null()
            && strict["unmeasurable"]
                .as_str()
                .is_some_and(|w| w.contains("gone.txt")),
        "{}",
        r.json
    );

    // a program that is not there: the unit fails and says so; nothing is reused for it
    write(
        "skipstop.json",
        &SETTINGS.replace(
            "[\"git\", \"grep\", \"-q\", \"good\", \"--\", \"data.txt\"]",
            "[\"no-such-program-skipstop\"]",
        ),
    );
    commit("strict's program is gone");
    let r = run(&t, &["--mode", "real"]);
    assert_eq!((r.rc, r.result("strict")), (1, "fail".into()), "{}", r.json);
    assert!(r.err.contains("strict: did not start"), "{}", r.err);

    // a unit with the name of... nothing here to collide with; but a settings file that is
    // wrong stops the run with 7, as for cargo units
    write(
        "skipstop.json",
        "{\"units\": {\"x\": {\"run\": [\"git\"]}}}",
    );
    commit("no inputs");
    let r = run(&t, &["--mode", "real"]);
    assert_eq!(r.rc, 7, "{}", r.err);
    assert!(r.err.contains("has no \"inputs\""), "{}", r.err);
}

#[test]
fn a_command_unit_that_times_out_fails_and_is_never_reused() {
    let d = std::env::temp_dir().join(format!("skipstop-cmd-timeout-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("repo")).unwrap();
    fs::create_dir_all(d.join("state")).unwrap();
    let t = Tmp(d);
    let repo = t.0.join("repo");
    fs::write(repo.join("data.txt"), "ok\n").unwrap();
    let cmd = if cfg!(windows) {
        r#"["cmd", "/C", "ping -n 15 127.0.0.1 >nul"]"#
    } else {
        r#"["sh", "-c", "sleep 10"]"#
    };
    fs::write(
        repo.join("skipstop.json"),
        format!(
            "{{\"units\": {{\"slow\": {{\"run\": {cmd}, \"inputs\": [\"data.txt\"], \"timeout_s\": 1}}}}}}"
        ),
    )
    .unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "start"]);

    for i in 0..2 {
        let r = run(&t, &["--mode", "real"]);
        assert_eq!(r.rc, 5, "run {i}: {}", r.json);
        assert_eq!(r.result("slow"), "fail", "run {i}: {}", r.json);
        assert_ne!(r.plan("slow"), "reuse", "run {i}: {}", r.json);
    }
}
