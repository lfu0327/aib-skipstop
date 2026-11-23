//! `aib-skipstop stop`: run the checks, then the command or not.
//!
//! ```text
//! aib-skipstop stop [--config FILE] [--mode observe|real] [--state DIR] -- CMD...
//! aib-skipstop stop [--config FILE] check
//! aib-skipstop stop --no-stop [--state DIR] -- CMD...
//! ```
//! - `-- CMD...`: the checks, then CMD (real mode: not when a `reject` check has a finding).
//! - `check`: the checks only; nothing is recorded.
//! - `--no-stop`: CMD without the checks, for the person who knows better than a check. When
//!   CMD then passes on a commit a check stopped, that stop was wrong: what it had recorded
//!   as saved is taken back.
//!
//! Exit: CMD's own code when it ran; 10 = stopped by a check (CMD was not run); 64 a bad
//! call; 7 the checks file or the state could not be read. No checks file is no checks: CMD
//! runs, and a line says so.

use crate::{observed, parse, saving, says, verdict, Check, Ended, Kind, Observed, Says, Verdict};
use serde_json::Value;
use skipstop_ledger::events::{self, Ctx};
use skipstop_ledger::timesave::{self, Saved, System};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const CONFIG_DEFAULT: &str = "skipstop.stop.json";
pub const EXIT_STOPPED: i32 = 10;
pub const USAGE: &str = "usage: aib-skipstop stop [--config FILE] [--mode observe|real] [--state DIR] -- CMD...\n       aib-skipstop stop [--config FILE] check\n       aib-skipstop stop --no-stop [--state DIR] -- CMD...";

/// Ends a check that ran past its time, with whatever it started: on Unix the check runs in
/// a process group of its own and the group is killed; on Windows `taskkill /T` takes the
/// tree. Then the direct child is killed too, in case the tools above were not there.
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-9", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

/// Runs one check's command in `dir`, with nothing on its stdin and its output kept off the
/// terminal, and ends it and what it started at its time limit.
pub fn run_check(dir: &Path, c: &Check) -> Ended {
    run_argv(dir, &c.cmd, c.timeout_s, false)
}

/// Runs `argv` in `dir` with nothing on its stdin, and ends it and what it started at
/// `timeout_s`. `show`: what it prints goes to this program's standard error, both streams
/// (standard output stays this program's own); otherwise it is kept off the terminal.
/// An empty `argv` did not start.
pub fn run_argv(dir: &Path, argv: &[String], timeout_s: u64, show: bool) -> Ended {
    let Some(program) = argv.first() else {
        return Ended::NoStart("no command".into());
    };
    let mut cmd = Command::new(program);
    cmd.args(&argv[1..]).current_dir(dir).stdin(Stdio::null());
    if show {
        cmd.stdout(Stdio::from(std::io::stderr()))
            .stderr(Stdio::inherit());
    } else {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(ch) => ch,
        Err(e) => return Ended::NoStart(e.to_string()),
    };
    let limit = Duration::from_secs(timeout_s);
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.code().map_or(Ended::Signal, Ended::Code),
            Ok(None) if start.elapsed() >= limit => {
                kill_tree(&mut child);
                // reaped when it is gone; a child that will not die is not waited for
                let gone = Instant::now();
                while gone.elapsed() < Duration::from_secs(5) {
                    if !matches!(child.try_wait(), Ok(None)) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                return Ended::Timeout;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Ended::NoStart(e.to_string()),
        }
    }
}

/// What was asked on the command line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    pub config: Option<PathBuf>,
    pub state: Option<PathBuf>,
    /// `--mode` as given: Some(true) = real.
    pub mode: Option<bool>,
    pub no_stop: bool,
    pub check_only: bool,
    pub cmd: Vec<String>,
}

pub fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--" => {
                a.cmd = it.cloned().collect();
                break;
            }
            "--config" => a.config = Some(it.next().ok_or("--config needs a file")?.into()),
            "--state" => a.state = Some(it.next().ok_or("--state needs a dir")?.into()),
            "--mode" => {
                a.mode = Some(match it.next().map(String::as_str) {
                    Some("observe") => false,
                    Some("real") => true,
                    other => return Err(format!("--mode {other:?}: not observe or real")),
                })
            }
            "--no-stop" => a.no_stop = true,
            "check" if !a.check_only => a.check_only = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    match (a.check_only, a.cmd.is_empty()) {
        (true, false) => Err("check takes no command".into()),
        (false, true) => Err("no command after --".into()),
        (true, true) if a.no_stop => Err("check and --no-stop do not go together".into()),
        _ => Ok(a),
    }
}

/// The mode for a `--mode` flag as given and a state folder: (real, who said so).
pub type ModeOf = dyn Fn(Option<bool>, &Path) -> Result<(bool, &'static str), String>;

/// Where the caller's state lives and which mode it runs in: the binary knows both (the same
/// state root and mode rules as skip), this crate does not.
pub struct Env<'a> {
    /// The state folder for `--state` as given (None = the default one).
    pub state: &'a dyn Fn(Option<&Path>) -> PathBuf,
    pub mode: &'a ModeOf,
    /// The repo's head commit, empty when not known.
    pub head: String,
    /// The repo's root folder, when the caller is inside a git repo: the checks file is looked
    /// for there and the checks run there, wherever in the repo the command is started.
    pub root: Option<PathBuf>,
    /// The caller's folder inside the repo (`git rev-parse --show-prefix`; empty at the root).
    pub prefix: String,
    pub seat: Option<String>,
    pub now_ms: i64,
}

fn line(c: &Check, s: &Says) -> String {
    match s {
        Says::Pass => format!("STOP-CHECK {} pass", c.id),
        Says::Finding => format!("STOP-CHECK {} {}: {}", c.id, c.kind.word(), c.avoids),
        Says::Broken(why) => format!(
            "STOP-CHECK {} BROKEN ({why}): it stops nothing and proves nothing; fix the check",
            c.id
        ),
    }
}

/// The name a stop is recorded under and found again by: the work tree it ran in (its root
/// folder), the folder inside it (empty at the root) and the command's words, as JSON. So
/// `["tool", "a b"]` and `["tool", "a", "b"]`, the same words in two folders, or the same
/// commit checked out in two work trees that share a state folder, are never one name.
pub fn command_text(root: &str, prefix: &str, cmd: &[String]) -> String {
    serde_json::json!({"tree": root, "in": prefix, "cmd": cmd}).to_string()
}

/// The one `time_saved` of a command that the checks stop (or, in observe mode, would stop);
/// None when no `reject` check has a finding. One command is stopped once, however many
/// checks found something: the saving is the largest any of them names, never their sum.
/// When none names a number the stop is still recorded, as 0 with the reason said: the
/// judgment is kept even when its worth is not known. `real` false = a prediction, in no total.
pub fn stop_event(
    ctx: &Ctx,
    checks: &[(&Check, Says)],
    book: &[Value],
    real: bool,
    head: &str,
    command: &str,
    at_ms: i64,
) -> Option<Value> {
    let stops: Vec<&Check> = checks
        .iter()
        .filter(|(c, s)| c.kind == Kind::Reject && *s == Says::Finding)
        .map(|(c, _)| *c)
        .collect();
    let first = stops.first()?;
    let ids: Vec<&str> = stops.iter().map(|c| c.id.as_str()).collect();
    let best = stops
        .iter()
        .filter_map(|c| saving(c, book).map(|(ms, basis, measured)| (ms, basis, measured, *c)))
        .max_by_key(|(ms, _, _, _)| *ms);
    let (ms, basis, measured, by) = match best {
        Some((ms, basis, measured, c)) => (
            ms,
            format!("{basis} (check {}; stopped by {})", c.id, ids.join(", ")),
            measured,
            c,
        ),
        None => (
            0,
            format!("what it saves is not known: no check that stopped it names a number (stopped by {})", ids.join(", ")),
            // nothing is claimed, so there is nothing to be an estimate of
            true,
            *first,
        ),
    };
    let saved = Saved {
        system: System::Precheck,
        rule: &by.id,
        verdict: "reject",
        avoided: &by.avoids,
        saved_ms: ms,
        // the finding is a fact (the check is of the kind that stops). How long the stopped
        // work would have taken is confirmed only when it was measured (the unit's own runs);
        // a number written in the checks file is a person's estimate and is recorded as one
        confirmed: measured,
        basis: &basis,
        real,
        head: Some(head).filter(|h| !h.is_empty()),
        of: Some((command, "stop")),
    };
    timesave::saved(ctx, &saved, at_ms).ok()
}

/// The `time_saved_wrong` events for the stops of `command` recorded at `head`, when that
/// command has now run there and passed.
pub fn wrong_events(
    ctx: &Ctx,
    book: &[Value],
    head: &str,
    command: &str,
    at_ms: i64,
) -> Vec<Value> {
    timesave::standing_stops(book, head, command)
        .into_iter()
        .filter_map(|(id, ms)| {
            let basis =
                format!("the command it stopped was run with --no-stop on {head} and passed");
            timesave::wrong(ctx, &id, ms, &basis, at_ms).ok()
        })
        .collect()
}

fn run_cmd(cmd: &[String]) -> Result<Option<i32>, String> {
    Command::new(&cmd[0])
        .args(&cmd[1..])
        .status()
        .map(|s| s.code())
        .map_err(|e| format!("{}: {e}", cmd[0]))
}

fn code_of(code: Option<i32>) -> i32 {
    // a signal has no code: 1, not a success. The command's own code is passed on whole
    // (Windows codes go past 255)
    code.unwrap_or(1)
}

fn write(dir: &Path, evs: Vec<Value>, at_ms: i64) {
    for ev in evs {
        if let Err(e) = events::append_in(dir, &ev, at_ms) {
            eprintln!("aib-skipstop stop: event not written: {e}");
            break;
        }
    }
}

pub fn cli(args: &[String], env: &Env<'_>) -> i32 {
    let a = match parse_args(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("aib-skipstop stop: {e}\n{USAGE}");
            return 64;
        }
    };
    let state = (env.state)(a.state.as_deref());
    let events_dir = state.join("events");
    let ctx = Ctx {
        host: None,
        seat: env.seat.clone().filter(|s| events::seat_code(s)),
        cards: Vec::new(),
        status_item: None,
    };
    let book = || timesave::read_dirs(std::slice::from_ref(&events_dir)).0;
    // outside a git repo the folder the command starts in stands for the work tree
    let tree = env
        .root
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    if a.no_stop {
        println!("STOP --no-stop: the checks are not run");
        let code = match run_cmd(&a.cmd) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("aib-skipstop stop: {e}");
                return 127;
            }
        };
        if code == Some(0) {
            let command = command_text(&tree, &env.prefix, &a.cmd);
            let back = wrong_events(&ctx, &book(), &env.head, &command, env.now_ms);
            if !back.is_empty() {
                println!(
                    "STOP-WRONG {} stop(s) on this commit are taken back: the command passed",
                    back.len()
                );
            }
            write(&events_dir, back, env.now_ms);
        }
        return code_of(code);
    }
    // the checks belong to the repo: its root, not the folder the command happens to start in
    let dir = env.root.clone().unwrap_or_else(|| PathBuf::from("."));
    let cfg = a.config.clone().unwrap_or_else(|| dir.join(CONFIG_DEFAULT));
    let checks = match std::fs::read_to_string(&cfg) {
        Ok(t) => match parse(&t) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("aib-skipstop stop: {}: {e}", cfg.display());
                return 7;
            }
        },
        // no file named and none in the folder: no checks. A file that was named must be there
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && a.config.is_none() => {
            println!("STOP no {}: nothing is checked", cfg.display());
            Vec::new()
        }
        Err(e) => {
            eprintln!("aib-skipstop stop: {}: {e}", cfg.display());
            return 7;
        }
    };
    let (real, said_by) = if a.check_only {
        (false, "check")
    } else {
        match (env.mode)(a.mode, &state) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("aib-skipstop stop: {e}");
                return 64;
            }
        }
    };
    let results: Vec<(&Check, Says)> = checks
        .iter()
        .map(|c| (c, says(c, &run_check(&dir, c))))
        .collect();
    for (c, s) in &results {
        println!("{}", line(c, s));
    }
    let v: Verdict = verdict(&results);
    println!(
        "STOP checks={} pass={} stop={} warn={} broken={} mode={}",
        results.len(),
        v.passed,
        v.stops.len(),
        v.warns.len(),
        v.broken.len(),
        if a.check_only {
            "check".to_string()
        } else {
            format!("{} (from {said_by})", if real { "real" } else { "observe" })
        }
    );
    if a.check_only {
        return if v.stop() { EXIT_STOPPED } else { 0 };
    }
    if let Some(ev) = stop_event(
        &ctx,
        &results,
        &book(),
        real,
        &env.head,
        &command_text(&tree, &env.prefix, &a.cmd),
        env.now_ms,
    ) {
        if ev["saved_ms"] == 0 {
            println!("STOP no check that stopped it names what it saves: recorded as 0");
        }
        write(&events_dir, vec![ev], env.now_ms);
    }
    if v.stop() && real {
        println!(
            "STOPPED by {}: the command was not run (--no-stop runs it anyway)",
            v.stops.join(", ")
        );
        return EXIT_STOPPED;
    }
    let code = match run_cmd(&a.cmd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aib-skipstop stop: {e}");
            return 127;
        }
    };
    match observed(&v, code == Some(0)) {
        Observed::NoStop => {}
        Observed::Right => println!(
            "STOP-OBSERVED right: {} would have stopped it, and it failed",
            v.stops.join(", ")
        ),
        Observed::WouldBlockGoodWork => println!(
            "STOP-OBSERVED would block good work: {} would have stopped it, and it passed",
            v.stops.join(", ")
        ),
    }
    code_of(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Result<Args, String> {
        parse_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_command_line_is_a_command_or_check_and_never_both_or_neither() {
        let a = args(&[
            "--mode", "real", "--config", "c.json", "--", "cargo", "test", "--", "x",
        ])
        .unwrap();
        assert_eq!(
            (a.mode, a.config, a.cmd.len()),
            (Some(true), Some("c.json".into()), 4)
        );
        // everything after the first -- is the command, another -- included
        assert_eq!(a.cmd[2], "--");
        assert!(args(&["check"]).unwrap().check_only);
        assert!(args(&[]).is_err());
        assert!(args(&["check", "--", "x"]).is_err());
        assert!(args(&["check", "--no-stop"]).is_err());
        assert!(args(&["--mode", "fast", "--", "x"]).is_err());
        assert!(args(&["--frobnicate", "--", "x"]).is_err());
        assert!(args(&["--no-stop", "--", "x"]).unwrap().no_stop);
    }

    fn check(id: &str, kind: Kind, saves_ms: Option<u64>) -> Check {
        Check {
            id: id.into(),
            kind,
            cmd: vec!["x".into()],
            avoids: "an hour of tests".into(),
            saves_ms,
            saves_unit: None,
            timeout_s: 60,
            stop_codes: vec![1],
        }
    }

    fn ctx() -> Ctx {
        Ctx {
            host: None,
            seat: None,
            cards: vec![],
            status_item: None,
        }
    }

    const AT: i64 = 1_795_399_200_000;

    const CMD: &str = "cargo test --workspace";

    #[test]
    fn one_stopped_command_is_one_saving_the_largest_named_and_never_the_sum() {
        let (hour, minute, w, no_number) = (
            check("conflict", Kind::Reject, Some(3_600_000)),
            check("format", Kind::Reject, Some(60_000)),
            check("guess", Kind::Warn, Some(9_999_999)),
            check("no-number", Kind::Reject, None),
        );
        let results = vec![
            (&minute, Says::Finding),
            (&hour, Says::Finding),
            (&w, Says::Finding),
            (&no_number, Says::Finding),
            (&hour, Says::Broken("x".into())),
        ];
        let e = stop_event(&ctx(), &results, &[], true, "abc", CMD, AT).unwrap();
        // not 1:01:00, and not the warn's number
        assert_eq!(e["saved_ms"], 3_600_000);
        assert_eq!(
            (e["system"].as_str(), e["rule"].as_str()),
            (Some("precheck"), Some("conflict"))
        );
        assert_eq!(
            (e["verdict"].as_str(), e["mode"].as_str()),
            (Some("reject"), Some("real"))
        );
        assert_eq!(
            (e["head"].as_str(), e["unit"].as_str()),
            (Some("abc"), Some(CMD))
        );
        let basis = e["basis"].as_str().unwrap();
        assert!(
            basis.contains("format") && basis.contains("no-number"),
            "{basis}"
        );
        // observe mode: the same stop is a prediction
        let o = stop_event(&ctx(), &results, &[], false, "abc", CMD, AT).unwrap();
        assert_eq!(o["mode"], "observe");
        assert_eq!(
            timesave::summarize(&[o], AT, 0).all.total(),
            timesave::Sum::default()
        );
        // a warn finding, a pass and a broken check stop nothing: no event
        let soft = vec![
            (&w, Says::Finding),
            (&hour, Says::Pass),
            (&hour, Says::Broken("x".into())),
        ];
        assert!(stop_event(&ctx(), &soft, &[], true, "abc", CMD, AT).is_none());
        // stopped, and nobody says what that saves: recorded all the same, as 0 with the reason
        let unknown = vec![(&no_number, Says::Finding)];
        let e = stop_event(&ctx(), &unknown, &[], true, "abc", CMD, AT).unwrap();
        assert_eq!(e["saved_ms"], 0);
        assert!(e["basis"].as_str().unwrap().contains("not known"), "{e}");
        assert_eq!(timesave::summarize(&[e], AT, 0).all.precheck.count, 1);
    }

    #[test]
    fn a_typed_saving_is_an_estimate_and_a_measured_one_is_confirmed() {
        // a number a person wrote in the checks file
        let typed = check("typed", Kind::Reject, Some(3_600_000));
        let e = stop_event(
            &ctx(),
            &[(&typed, Says::Finding)],
            &[],
            true,
            "abc",
            CMD,
            AT,
        )
        .unwrap();
        assert_eq!(
            (e["confirmed"].as_bool(), e["saved_ms"].as_u64()),
            (Some(false), Some(3_600_000)),
            "{e}"
        );
        let s = timesave::summarize(std::slice::from_ref(&e), AT, 0);
        assert_eq!(
            (
                s.all.precheck.confirmed_ms,
                s.all.precheck.estimated_ms,
                s.all.precheck.count
            ),
            (0, 3_600_000, 1)
        );
        // the median of the unit's own passing runs in the book
        let mut measured = check("measured", Kind::Reject, None);
        measured.saves_unit = Some("tests".into());
        let book: Vec<serde_json::Value> = [100u64, 300, 200]
            .iter()
            .map(|ms| serde_json::json!({"kind": "unit_time", "unit": "tests", "ms": ms, "outcome": "pass"}))
            .collect();
        let e = stop_event(
            &ctx(),
            &[(&measured, Says::Finding)],
            &book,
            true,
            "abc",
            CMD,
            AT,
        )
        .unwrap();
        assert_eq!(
            (e["confirmed"].as_bool(), e["saved_ms"].as_u64()),
            (Some(true), Some(200)),
            "{e}"
        );
        // both stop it: the larger number is the one recorded, with its own label
        let e = stop_event(
            &ctx(),
            &[(&measured, Says::Finding), (&typed, Says::Finding)],
            &book,
            true,
            "abc",
            CMD,
            AT,
        )
        .unwrap();
        assert_eq!(
            (e["confirmed"].as_bool(), e["saved_ms"].as_u64()),
            (Some(false), Some(3_600_000)),
            "{e}"
        );
    }

    #[test]
    fn a_stop_is_taken_back_once_and_only_for_its_command_on_its_commit() {
        let r = check("conflict", Kind::Reject, Some(1_000));
        let results = vec![(&r, Says::Finding)];
        let stop =
            |real, head, cmd, at| stop_event(&ctx(), &results, &[], real, head, cmd, at).unwrap();
        let mut book = vec![
            stop(true, "abc", CMD, AT),
            stop(true, "other", CMD, AT + 1),
            stop(true, "abc", "cargo build", AT + 2),
            // an observe-mode prediction on the same commit and command stopped nothing
            stop(false, "abc", CMD, AT + 3),
        ];
        assert_eq!(
            timesave::summarize(&book, AT + 9, 0)
                .all
                .precheck
                .estimated_ms,
            3_000
        );
        // another command passing on that commit says nothing about this one
        assert!(wrong_events(&ctx(), &book, "abc", "git status", AT + 4).is_empty());
        let back = wrong_events(&ctx(), &book, "abc", CMD, AT + 4);
        assert_eq!(back.len(), 1, "{back:?}");
        assert_eq!(back[0]["penalty_ms"], 1_000);
        book.extend(back);
        let s = timesave::summarize(&book, AT + 9, 0);
        assert_eq!(
            (
                s.all.precheck.estimated_ms,
                s.all.precheck.penalty_ms,
                s.bad
            ),
            (2_000, 1_000, 0)
        );
        assert!(wrong_events(&ctx(), &book, "abc", CMD, AT + 5).is_empty());
        // a head that is not known matches nothing
        assert!(wrong_events(&ctx(), &book, "", CMD, AT + 6).is_empty());
        // the words, the folder and the work tree, each kept apart
        let c = |tree: &str, prefix: &str, words: &[&str]| {
            let words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
            command_text(tree, prefix, &words)
        };
        assert_ne!(
            c("/r", "", &["tool", "a b"]),
            c("/r", "", &["tool", "a", "b"])
        );
        assert_ne!(
            c("/r", "a/", &["cargo", "test"]),
            c("/r", "b/", &["cargo", "test"])
        );
        assert_ne!(
            c("/r1", "", &["cargo", "test"]),
            c("/r2", "", &["cargo", "test"])
        );
        assert_eq!(
            c("/r", "a/", &["cargo", "test"]),
            c("/r", "a/", &["cargo", "test"])
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_check_command_is_run_with_its_time_limit() {
        let d = tempfile::tempdir().unwrap();
        let mut c = check("c", Kind::Reject, None);
        let sh = |script: &str| vec!["sh".to_string(), "-c".to_string(), script.to_string()];
        c.cmd = sh("exit 0");
        assert_eq!(run_check(d.path(), &c), Ended::Code(0));
        c.cmd = sh("exit 1");
        assert_eq!(run_check(d.path(), &c), Ended::Code(1));
        c.cmd = sh("kill -9 $$");
        assert_eq!(run_check(d.path(), &c), Ended::Signal);
        c.cmd = vec!["no-such-program-anywhere-7c1".into()];
        assert!(matches!(run_check(d.path(), &c), Ended::NoStart(_)));
        c.cmd = sh("sleep 30");
        c.timeout_s = 1;
        let t = Instant::now();
        assert_eq!(run_check(d.path(), &c), Ended::Timeout);
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "killed at its limit, not waited for"
        );
        // what the check started dies with it: nothing is left to run beside the command
        // (first: left alone, the background process does write its file, so the check
        // after it is able to fail)
        c.cmd = sh("(sleep 1; touch early) & exit 0");
        c.timeout_s = 60;
        assert_eq!(run_check(d.path(), &c), Ended::Code(0));
        std::thread::sleep(Duration::from_secs(2));
        assert!(d.path().join("early").exists());
        c.cmd = sh("(sleep 3; touch late) & sleep 30");
        c.timeout_s = 1;
        assert_eq!(run_check(d.path(), &c), Ended::Timeout);
        std::thread::sleep(Duration::from_secs(4));
        assert!(
            !d.path().join("late").exists(),
            "a process the check left behind went on running"
        );
        // it runs in the folder it is given
        std::fs::write(d.path().join("marker"), "").unwrap();
        c.timeout_s = 60;
        c.cmd = sh("test -f marker");
        assert_eq!(run_check(d.path(), &c), Ended::Code(0));
    }

    #[cfg(windows)]
    #[test]
    fn windows_timeout_kills_process_tree() {
        let d = tempfile::tempdir().unwrap();
        let mut c = check("c", Kind::Reject, None);
        c.timeout_s = 1;
        c.cmd = vec![
            "cmd".to_string(),
            "/C".to_string(),
            "start /B cmd /C \"ping -n 4 127.0.0.1 >nul & type nul > late\" & ping -n 20 127.0.0.1 >nul".to_string(),
        ];
        let t = Instant::now();
        assert_eq!(run_check(d.path(), &c), Ended::Timeout);
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "killed at its limit, not waited for"
        );
        std::thread::sleep(Duration::from_secs(5));
        assert!(
            !d.path().join("late").exists(),
            "a child process left behind by taskkill /T went on running"
        );
    }
}
