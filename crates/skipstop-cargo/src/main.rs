//! `aib-skipstop`: the first runnable `aib test` 2.0, observation mode (519d0 11-17 03:35).
//! Every unit runs for real; the ledger also records, per unit, the earlier run it would
//! have reused and whether the result came out the same. Nothing here is evidence for a
//! push, a review or a merge (519d0 rule 5). It does not replace 1.x `aib test`.
//!
//! usage: aib-skipstop [-p <package>]... [--json] [--why] [--state <dir>]
//! exit: the codes of skipstop_core::exit (0 ok, 1 failed, 3 zero units, 4 nothing ran,
//!       7 engine error); 64 bad arguments.

use skipstop_audit::{engine, Compare, Outcome as AuditOutcome, Verdict};
use skipstop_cargo::real::{self, Decision};
use skipstop_cargo::repo::repo_id;
use skipstop_cargo::units::{self, Workspace};
use skipstop_cargo::{list_args, parse_junit, parse_list, run_args, run_files, tool_config};
use skipstop_core::context::{self, Context, CARGO_VARS};
use skipstop_core::exit::{exit_for, Exit, Outcome as RunOutcome};
use skipstop_core::key::{self, FileCache};
use skipstop_core::ledger::{
    self, Audit, AuditCompare, How, Origin, Outcome, Plan, Reuse, Shadow, Unit,
};
use skipstop_core::skip::Use;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "usage: aib-skipstop [-p <package>]... [--mode observe|real] [--fresh] [--plan] [--json] [--why] [--state <dir>]\n       aib-skipstop mode [observe|real] [--state <dir>]\n       aib-skipstop saved [--json] [--state <dir>] [--utc-offset <minutes>]\n       aib-skipstop trial [--json] [--state <dir>]\n       aib-skipstop stop [--config <file>] [--mode observe|real] [--no-stop] [--state <dir>] (check | -- <command>...)";

struct Args {
    packages: Vec<String>,
    json: bool,
    why: bool,
    state: Option<PathBuf>,
    help: bool,
    /// `saved`: print the time-saved boxes instead of running
    saved: bool,
    /// `trial`: what the ledger says of the trial so far
    trial: bool,
    /// real mode: a unit with a source that no audit picked is not run
    real: bool,
    /// `--mode` as given on the command line, which wins over everything else
    mode_flag: Option<bool>,
    /// `mode [observe|real]`: show or set the mode this state folder runs in
    mode_cmd: Option<Option<String>>,
    /// run every unit this time, whatever the ledger says
    fresh: bool,
    /// work out the keys and say what a run would do with each unit; run nothing, write nothing
    plan_only: bool,
    /// minutes the caller's clock is ahead of UTC (days, weeks, months start on it)
    utc_offset: i32,
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        packages: Vec::new(),
        json: false,
        why: false,
        state: None,
        help: false,
        saved: false,
        trial: false,
        real: false,
        mode_flag: None,
        mode_cmd: None,
        fresh: false,
        plan_only: false,
        utc_offset: 0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "-p" | "--package" => a.packages.push(it.next().ok_or("-p needs a package")?),
            "--json" => a.json = true,
            "--why" => a.why = true,
            "--state" => a.state = Some(PathBuf::from(it.next().ok_or("--state needs a dir")?)),
            "-h" | "--help" => a.help = true,
            "--mode" => {
                a.mode_flag = Some(mode_word(
                    &it.next().ok_or("--mode needs observe or real")?,
                )?)
            }
            "mode" if !a.saved && a.mode_cmd.is_none() && a.packages.is_empty() => {
                a.mode_cmd = Some(None)
            }
            w @ ("observe" | "real") if a.mode_cmd == Some(None) => {
                a.mode_cmd = Some(Some(w.to_string()))
            }
            "--fresh" => a.fresh = true,
            "--plan" => a.plan_only = true,
            "saved" if !a.saved && a.packages.is_empty() => a.saved = true,
            "trial" if !a.trial && !a.saved && a.packages.is_empty() => a.trial = true,
            "--utc-offset" => {
                let v = it.next().ok_or("--utc-offset needs minutes")?;
                a.utc_offset = v
                    .parse::<i32>()
                    .ok()
                    .filter(|m| (-14 * 60..=14 * 60).contains(m))
                    .ok_or_else(|| format!("--utc-offset {v:?}: not minutes from -840 to 840"))?;
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(a)
}

/// `observe` ⇒ false, `real` ⇒ true; any other word is an error, never a guess.
fn mode_word(word: &str) -> Result<bool, String> {
    match word.trim() {
        "observe" => Ok(false),
        "real" => Ok(true),
        other => Err(format!("mode {other:?}: not observe or real")),
    }
}

/// The mode a run is in, the first of these that says: `--mode`, `AIB_SKIPSTOP_MODE`, the
/// state folder's `mode` file (`aib-skipstop mode real` writes it), else observe. An empty
/// variable or a missing file says nothing; a word that is not a mode is an error wherever
/// it stands, so a typo never runs as something else. Returns the mode and who said it.
fn mode_of(
    flag: Option<bool>,
    env: Option<&str>,
    file: Option<&str>,
) -> Result<(bool, &'static str), String> {
    // every word that is given is read first: a typo in the file is an error even when the
    // flag would win over it
    fn said(w: Option<&str>) -> Option<&str> {
        w.map(str::trim).filter(|w| !w.is_empty())
    }
    let env = said(env)
        .map(|w| mode_word(w).map_err(|e| format!("AIB_SKIPSTOP_MODE: {e}")))
        .transpose()?;
    let file = said(file)
        .map(|w| mode_word(w).map_err(|e| format!("the mode file: {e}")))
        .transpose()?;
    if let Some(r) = flag {
        return Ok((r, "--mode"));
    }
    if let Some(r) = env {
        return Ok((r, "AIB_SKIPSTOP_MODE"));
    }
    if let Some(r) = file {
        return Ok((r, "the mode file"));
    }
    Ok((false, "the default"))
}

/// A number no earlier run had, for this run's random draws. `AIB_SKIPSTOP_SEED=<number>`
/// fixes it, so a run can be repeated draw for draw (a test, or looking into one audit).
fn fresh_seed() -> u64 {
    if let Some(s) = std::env::var("AIB_SKIPSTOP_SEED")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        return s;
    }
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_i64(now_ms());
    h.write_u32(std::process::id());
    h.finish()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn git(root: &Path, a: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(a)
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn cargo(root: &Path, a: &[String]) -> Result<std::process::Output, String> {
    Command::new("cargo")
        .current_dir(root)
        .args(a)
        .output()
        .map_err(|e| format!("cargo: {e}"))
}

/// `aib-skipstop trial`: the repo's ledger as a trial report. It reads; it changes no mode.
fn trial_report(a: &Args) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = key::repo_root(&cwd).map_err(|e| format!("{e:?}"))?;
    let (_, streams, problems) = repo_streams(&state_root(a).join("ledger"), &root);
    let t = real::trial(&streams, now_ms() / 1000);
    let bar = t.meets_the_bar() && problems.is_empty();
    if a.json {
        return Ok(format!(
            "{}\n",
            serde_json::json!({
                "v": 1,
                "first_at": t.first_at_ms.map(skipstop_ledger::envelope::rfc3339_ms),
                "days": t.days, "runs_real": t.runs_real, "runs_observe": t.runs_observe,
                "reused": t.reused, "compared": t.compared(), "same": t.same,
                "differed": t.differed, "incomparable": t.incomparable,
                "revoked": t.revoked, "reds": t.reds,
                "ledger_problems": problems,
                "bar": {"days": real::TRIAL_DAYS, "compared": real::TRIAL_COMPARED,
                        "reds": real::TRIAL_REDS, "differed": 0, "met": bar},
            })
        ));
    }
    let mark = |ok: bool| if ok { "met" } else { "not yet" };
    let mut s = format!(
        "the trial in this repo's ledger: {} day(s), {} real run(s), {} observe run(s)\n",
        t.days, t.runs_real, t.runs_observe
    );
    s.push_str(&format!(
        "  reused {} time(s); compared with a real run {} time(s): {} the same, {} differed ({} could not be compared); {} source(s) revoked; {} failure(s) found by a run\n",
        t.reused, t.compared(), t.same, t.differed, t.incomparable, t.revoked, t.reds
    ));
    s.push_str("the bar for making real mode the default (a person's decision; nothing here changes the mode):\n");
    s.push_str(&format!(
        "  {:<8} a week of history: {} of {} days\n",
        mark(t.days >= real::TRIAL_DAYS),
        t.days,
        real::TRIAL_DAYS
    ));
    s.push_str(&format!(
        "  {:<8} comparisons: {} of {}\n",
        mark(t.compared() >= real::TRIAL_COMPARED),
        t.compared(),
        real::TRIAL_COMPARED
    ));
    s.push_str(&format!(
        "  {:<8} real failures seen: {} of {}\n",
        mark(t.reds >= real::TRIAL_REDS),
        t.reds,
        real::TRIAL_REDS
    ));
    s.push_str(&format!(
        "  {:<8} comparisons that differed: {} (must be 0)\n",
        mark(t.differed == 0),
        t.differed
    ));
    for p in &problems {
        s.push_str(&format!("  not yet  a ledger file cannot be read: {p}\n"));
    }
    s.push_str(if bar {
        "every part is met\n"
    } else {
        "not every part is met\n"
    });
    Ok(s)
}

/// Seconds a tool has to say its version.
const TOOL_LIMIT_S: u64 = 20;

/// What a tool says of itself (`node --version`), as one line for the key: the command and
/// the sha256 of each of its two streams, bytes as they came. A tool that cannot be run, does
/// not end well or is not done in [`TOOL_LIMIT_S`] gives no line: the unit then has no key
/// and runs.
fn tool_text(root: &Path, argv: &[String]) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    use std::process::Stdio;
    let name = &argv[0];
    let mut child = Command::new(name)
        .current_dir(root)
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("tool {name}: {e}"))?;
    // read both streams as they come, so a tool that prints much does not wait on a full pipe
    let read = |s: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut h = Sha256::new();
            let mut buf = [0u8; 8192];
            if let Some(mut s) = s {
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                }
            }
            h.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if start.elapsed().as_secs() >= TOOL_LIMIT_S => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("tool {name}: not done in {TOOL_LIMIT_S} s"));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(e) => return Err(format!("tool {name}: {e}")),
        }
    };
    if !status.success() {
        return Err(format!("tool {name}: {status}"));
    }
    let (out, err) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    let said = serde_json::to_string(argv).map_err(|e| e.to_string())?;
    Ok(format!("{said} {out} {err}"))
}

fn state_root(a: &Args) -> PathBuf {
    root_from(
        a.state.as_deref(),
        std::env::var("EZ2_AIB_STATE_ROOT").ok().as_deref(),
        std::env::var("EZ2_AIB_HOME").ok().as_deref(),
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .as_deref(),
    )
}

/// Where the ledger lives: `--state` as given; else `aib-skipstop` under the aib state root,
/// found the way aib-drive finds it (`EZ2_AIB_STATE_ROOT`, then `EZ2_AIB_HOME`, then
/// `~/.ez2-aib`; an empty value counts as unset), so a cabin that moved its state root has
/// one ledger, not a second one under the home folder (043dd S69-u1, after #833).
fn root_from(
    flag: Option<&Path>,
    env_root: Option<&str>,
    aib_home: Option<&str>,
    home: Option<&Path>,
) -> PathBuf {
    if let Some(s) = flag {
        return s.to_path_buf();
    }
    let aib = if let Some(r) = env_root.filter(|r| !r.is_empty()) {
        PathBuf::from(r)
    } else if let Some(h) = aib_home.filter(|h| !h.is_empty()) {
        PathBuf::from(h)
    } else {
        home.unwrap_or_else(|| Path::new(".")).join(".ez2-aib")
    };
    aib.join("state").join("aib-skipstop")
}

/// Every run of this repo in the ledger (see `skipstop_cargo::repo`), and its id. A clone
/// that could not be told is a problem like a file that could not be read: nothing is reused.
fn repo_streams(ledger_root: &Path, root: &Path) -> (String, Vec<ledger::Stream>, Vec<String>) {
    let (id, unknown) = repo_id(root);
    let (streams, mut problems) = ledger::streams(ledger_root, &id);
    problems.extend(unknown);
    (id, streams, problems)
}

struct Row {
    unit: String,
    key: Option<String>,
    unmeasurable: Option<String>,
    would: Option<Reuse>,
    /// the source's result, to say whether the real run agreed
    source_result: Option<Outcome>,
    /// the source line as a verdict, to check this real run against (1ced7 engine link)
    source: Option<Verdict>,
    failed: Vec<String>,
    /// which cases ran and how each ended, as one line ([`skipstop_cargo::cases_digest`])
    cases: String,
    /// the unit's own time in the source run, when its line has one
    source_ms: Option<u64>,
    audit: Option<Audit>,
    result: Outcome,
    ran: u32,
    ms: u64,
    /// what this run does with the unit
    plan: Decision,
}

/// `aib-skipstop stop …`: the stop half, with this binary's state root and mode rules (its
/// own variable and mode file, so skip and stop are switched apart).
fn stop_main(rest: &[String]) -> ! {
    let state = |flag: Option<&Path>| {
        root_from(
            flag,
            std::env::var("EZ2_AIB_STATE_ROOT").ok().as_deref(),
            std::env::var("EZ2_AIB_HOME").ok().as_deref(),
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from)
                .as_deref(),
        )
    };
    let mode = |flag: Option<bool>, state: &Path| {
        let file = match std::fs::read_to_string(state.join("stop-mode")) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}", state.join("stop-mode").display())),
        };
        let env = std::env::var("AIB_SKIPSTOP_STOP_MODE").ok();
        mode_of(flag, env.as_deref(), file.as_deref()).map(|(real, by)| {
            let by = match by {
                "AIB_SKIPSTOP_MODE" => "AIB_SKIPSTOP_STOP_MODE",
                "the mode file" => "the stop-mode file",
                other => other,
            };
            (real, by)
        })
    };
    let cwd = std::env::current_dir().ok();
    let ask = |what: &[&str]| cwd.as_deref().and_then(|d| git(d, what));
    let head = ask(&["rev-parse", "HEAD"]).unwrap_or_default();
    let root = ask(&["rev-parse", "--show-toplevel"])
        .filter(|r| !r.is_empty())
        .map(PathBuf::from);
    let prefix = ask(&["rev-parse", "--show-prefix"]).unwrap_or_default();
    let env = skipstop_stop::run::Env {
        state: &state,
        mode: &mode,
        head,
        root,
        prefix,
        seat: std::env::var("AIB_SEAT").ok(),
        now_ms: now_ms(),
    };
    // the protected command's own exit code is passed on whole (Windows codes go past 255)
    std::process::exit(skipstop_stop::run::cli(rest, &env))
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if matches!(raw.as_slice(), [v] if v == "--version" || v == "-V") {
        println!("aib-skipstop {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if raw.first().map(String::as_str) == Some("stop") {
        stop_main(&raw[1..]);
    }
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("aib-skipstop: {e}");
            return ExitCode::from(64);
        }
    };
    if a.help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let mode_file = state_root(&a).join("mode");
    if let Some(Some(word)) = &a.mode_cmd {
        if let Err(e) = std::fs::create_dir_all(state_root(&a))
            .and_then(|()| std::fs::write(&mode_file, format!("{word}\n")))
        {
            eprintln!("aib-skipstop: {}: {e}", mode_file.display());
            return ExitCode::from(Exit::EngineError.code() as u8);
        }
    }
    // no file says nothing; a file that is there and cannot be read is not "nothing"
    let file = match std::fs::read_to_string(&mode_file) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            eprintln!("aib-skipstop: {}: {e}", mode_file.display());
            return ExitCode::from(Exit::EngineError.code() as u8);
        }
    };
    let env = std::env::var("AIB_SKIPSTOP_MODE").ok();
    let (real, said_by) = match mode_of(a.mode_flag, env.as_deref(), file.as_deref()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("aib-skipstop: {e}");
            return ExitCode::from(64);
        }
    };
    if a.mode_cmd.is_some() {
        println!("{} (from {said_by})", if real { "real" } else { "observe" });
        return ExitCode::SUCCESS;
    }
    let a = Args { real, ..a };
    if a.saved {
        let dir = state_root(&a).join("events");
        let (evs, unread) = skipstop_ledger::timesave::read_dirs(std::slice::from_ref(&dir));
        let mut s = skipstop_ledger::timesave::summarize(&evs, now_ms(), a.utc_offset);
        s.bad += unread;
        if a.json {
            println!(
                "{}",
                skipstop_cargo::run_events::saved_json(&s, a.utc_offset)
            );
        } else {
            print!(
                "{}",
                skipstop_cargo::run_events::saved_text(&s, a.utc_offset)
            );
        }
        return ExitCode::SUCCESS;
    }
    if a.trial {
        return match trial_report(&a) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("aib-skipstop: {e}");
                ExitCode::from(Exit::EngineError.code() as u8)
            }
        };
    }
    match run(&a) {
        Ok(e) => ExitCode::from(e.code() as u8),
        Err(e) => {
            eprintln!("aib-skipstop: engine error: {e}");
            ExitCode::from(Exit::EngineError.code() as u8)
        }
    }
}

fn run(a: &Args) -> Result<Exit, String> {
    // the ledger names who ran it: the AIB seat when there is one (the seat sets AIB_SEAT),
    // `local` for a run outside AIB. Never "unknown", never a guess at a seat
    let seat = std::env::var("AIB_SEAT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "local".to_string());
    let plan_started = now_ms();
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = key::repo_root(&cwd).map_err(|e| format!("{e:?}"))?;
    // a cargo workspace when there is one; a repo with none has only the units its settings name
    let ws: Workspace = if root.join("Cargo.toml").exists() {
        let meta = cargo(
            &root,
            &["metadata", "--no-deps", "--format-version", "1", "--locked"].map(String::from),
        )?;
        if !meta.status.success() {
            return Err(format!(
                "cargo metadata: {}",
                String::from_utf8_lossy(&meta.stderr).trim()
            ));
        }
        units::parse_metadata(&String::from_utf8_lossy(&meta.stdout))?
    } else {
        Workspace {
            root: root.to_string_lossy().into_owned(),
            target_dir: String::new(),
            packages: Vec::new(),
        }
    };
    // the project's own settings; a file that is there and not understood stops the run: it
    // must not run as the defaults instead (a unit said never to be reused would be reused)
    let config = skipstop_cargo::config::load(&root)?;
    let file = skipstop_cargo::config::FILE;
    let mut all: Vec<String> = ws.packages.iter().map(|p| p.name.clone()).collect();
    if let Some(twice) = config.units.keys().find(|u| all.contains(u)) {
        return Err(format!(
            "{file}: unit {twice:?} has the name of a package of this workspace"
        ));
    }
    all.extend(config.units.keys().cloned());
    let strangers = skipstop_cargo::config::strangers(&config, &all);
    if !strangers.is_empty() {
        return Err(format!(
            "{file}: never_reuse names {} which is no package of this workspace and no unit of this file",
            strangers.join(", ")
        ));
    }
    let chosen: Vec<String> = if a.packages.is_empty() {
        all.clone()
    } else {
        a.packages.clone()
    };
    if chosen.is_empty() {
        return Ok(Exit::ZeroUnits);
    }

    // the key of each unit, from the files on disk
    let env: Vec<(String, String)> = std::env::vars().collect();
    let ctx = Context {
        machine_kind: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        toolchain: context::cargo_toolchain(),
        vars: CARGO_VARS.iter().map(|s| s.to_string()).collect(),
        target_dir: ws.target_dir.clone(),
        target_shared: false,
    };
    let all = key::wide_paths(&root);
    let chash = ctx.hash(&env);
    let mut cache = FileCache::default();
    let present = |f: &str| root.join(f).exists();
    let state = state_root(a);
    let (rid, streams, problems) = repo_streams(&state.join("ledger"), &root);
    let mut rows: Vec<Row> = Vec::new();
    for name in &chosen {
        let k = (|| -> Result<String, String> {
            let all = all.as_ref().map_err(|e| format!("{e:?}"))?;
            if let Some(u) = config.units.get(name) {
                // a command: its key is the command, what its tools say, the variables and
                // files its settings name, and the settings file itself
                let mut tools = String::from("no tools named");
                for t in &u.tools {
                    tools.push('\n');
                    tools.push_str(&tool_text(&root, t)?);
                }
                let ctx = Context {
                    machine_kind: ctx.machine_kind.clone(),
                    toolchain: tools,
                    vars: u.env.clone(),
                    target_dir: String::new(),
                    target_shared: false,
                };
                let ch = ctx.hash(&env).map_err(|e| format!("{e:?}"))?;
                let mut pre = u.inputs.clone();
                pre.push(file.to_string());
                let paths = key::declared(all, &pre).map_err(|e| format!("{e:?}"))?;
                let inputs =
                    key::input_list(&root, &paths, &mut cache).map_err(|e| format!("{e:?}"))?;
                let def = serde_json::to_string(&u.run).map_err(|e| e.to_string())?;
                return Ok(key::key(&format!("command {def}"), &ch, &inputs));
            }
            let ch = chash.as_ref().map_err(|e| format!("{e:?}"))?;
            let pre = ws.inputs(name, &present)?;
            let paths = key::declared(all, &pre).map_err(|e| format!("{e:?}"))?;
            let inputs =
                key::input_list(&root, &paths, &mut cache).map_err(|e| format!("{e:?}"))?;
            Ok(key::key(
                &format!("cargo nextest run -p {name}"),
                ch,
                &inputs,
            ))
        })();
        let (key, unmeasurable) = match k {
            Ok(k) => (Some(k), None),
            Err(e) => (None, Some(e)),
        };
        let src = key
            .as_deref()
            .and_then(|k| ledger::local_source(&streams, name, k));
        let would = src.map(|(st, _, at)| Reuse {
            origin: Origin::Local,
            from: format!("{rid}/{}", st.invocation_id),
            key: key.clone().unwrap_or_default(),
            tree: st.plan.tree.clone(),
            at: skipstop_ledger::envelope::rfc3339_ms(at),
        });
        let source_result = src.map(|(_, u, _)| u.result);
        let source_ms = src.and_then(|(_, u, _)| u.dur_ms);
        let source = src.map(|(st, u, _)| Verdict {
            outcome: audit_outcome(u.result),
            tests_ran: u.tests_ran,
            failed: u.failed.clone(),
            // only a line of the kind this version takes is compared with
            output_digest: u
                .output_digest
                .clone()
                .filter(|d| d.starts_with(skipstop_cargo::CASES_DIGEST))
                .unwrap_or_default(),
            censored: !st.complete(),
        });
        rows.push(Row {
            unit: name.clone(),
            key,
            unmeasurable,
            would,
            source_result,
            source,
            source_ms,
            failed: Vec::new(),
            cases: String::new(),
            audit: None,
            result: Outcome::None,
            ran: 0,
            ms: 0,
            plan: Decision::Run { why: "observe" },
        });
    }

    // real mode: which units are not run this time
    if a.real {
        let asked = real::Asked {
            streams: &streams,
            fresh: a.fresh,
            ledger_ok: problems.is_empty(),
            seed: fresh_seed(),
            now_s: now_ms() / 1000,
            config: &config,
        };
        for r in &mut rows {
            r.plan = real::decide(&asked, &r.unit, r.would.is_some());
        }
    }
    // a run in which a unit's defense round ran is a round (for the units that ran in it)
    let defense_round = rows.iter().any(|r| {
        r.plan
            == Decision::Run {
                why: "defense_round",
            }
    });

    if a.plan_only {
        // what a run would do, and how long it took to know: nothing runs, nothing is written
        let took_ms = now_ms() - plan_started;
        let would_run = rows.iter().filter(|r| r.plan.runs()).count();
        if a.json {
            let units: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({"unit": r.unit, "plan": r.plan.word(), "key": r.key,
                        "unmeasurable": r.unmeasurable})
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({"plan_only": true, "mode": if a.real { "real" } else { "observe" },
                    "units": units, "would_run": would_run, "decided_in_ms": took_ms,
                    "ledger_problems": problems})
            );
        } else {
            for r in &rows {
                println!("  {:<28} [{}]", r.unit, r.plan.word());
            }
            for p in &problems {
                println!("  ledger: {p}");
            }
            println!(
                "aib-skipstop --plan ({}): {} units, {} would run, {} would be reused; decided in {} ms; nothing was run",
                if a.real { "real" } else { "observe" },
                rows.len(),
                would_run,
                rows.len() - would_run,
                took_ms
            );
        }
        return Ok(Exit::Ok);
    }

    // one nextest for every unit that runs (observe mode: all of them)
    std::fs::create_dir_all(&state).map_err(|e| e.to_string())?;
    // absolute, whatever the caller gave: nextest reads the JUnit path in the config as it stands
    let state_abs = std::path::absolute(&state).map_err(|e| format!("{}: {e}", state.display()))?;
    let (cfg, jp) = run_files(&state_abs, std::process::id());
    std::fs::write(&cfg, tool_config(&jp)?).map_err(|e| e.to_string())?;
    let mut select = Vec::new();
    for r in rows
        .iter()
        .filter(|r| r.plan.runs() && !config.units.contains_key(&r.unit))
    {
        select.push("-p".to_string());
        select.push(r.unit.clone());
    }
    let any_runs = !select.is_empty();
    let started = now_ms();
    // nothing to run (every unit reused): nextest is not called at all
    let (listed, cases, run_failed) = if any_runs {
        run_nextest(&root, &cfg, &jp, &select)?
    } else {
        let _ = std::fs::remove_file(&cfg);
        (skipstop_cargo::List::default(), Vec::new(), false)
    };

    // the units that are a command, one after another: what each prints goes to standard
    // error (standard output is this program's own)
    let mut commands: BTreeMap<String, (skipstop_stop::Ended, u64)> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.plan.runs()) {
        if let Some(u) = config.units.get(&r.unit) {
            let t0 = now_ms();
            let ended = skipstop_stop::run::run_argv(&root, &u.run, u.timeout_s, true);
            commands.insert(
                r.unit.clone(),
                (ended, u64::try_from(now_ms() - t0).unwrap_or(0)),
            );
        }
    }

    // read the result back per package
    let mut per: BTreeMap<String, (Vec<_>, Vec<_>)> = BTreeMap::new();
    for l in &listed.tests {
        per.entry(units::package_of_binary(&l.binary).to_string())
            .or_default()
            .0
            .push(l.clone());
    }
    for c in &cases {
        per.entry(units::package_of_binary(&c.binary).to_string())
            .or_default()
            .1
            .push(c.clone());
    }
    let mut outcomes = Vec::new();
    for r in &mut rows {
        if !r.plan.runs() {
            // not run: the source's result stands (a source is a pass on a clean tree)
            r.result = Outcome::Pass;
            outcomes.push(RunOutcome::Reused);
            continue;
        }
        if let Some((ended, ms)) = commands.remove(&r.unit) {
            // a command is one test: it passes with exit code 0. There are no cases to
            // compare; an audit of it compares the outcome
            use skipstop_stop::Ended;
            r.ms = ms;
            r.ran = 1;
            let met = matches!(ended, Ended::Code(0));
            r.result = if met { Outcome::Pass } else { Outcome::Fail };
            match &ended {
                Ended::Code(0) => {}
                Ended::Code(n) => eprintln!("aib-skipstop: {}: exit code {n}", r.unit),
                Ended::Signal => eprintln!("aib-skipstop: {}: ended by a signal", r.unit),
                Ended::Timeout => {
                    eprintln!("aib-skipstop: {}: ran past its time and was ended", r.unit)
                }
                Ended::NoStart(e) => eprintln!("aib-skipstop: {}: did not start: {e}", r.unit),
            }
            if let Some(src) = &r.source {
                let audit = Verdict {
                    outcome: audit_outcome(r.result),
                    tests_ran: Some(1),
                    failed: Vec::new(),
                    output_digest: String::new(),
                    censored: false,
                };
                r.audit = Some(audit_of(&audit, src, met));
            }
            outcomes.push(if ended == Ended::Timeout {
                RunOutcome::TimedOut
            } else {
                RunOutcome::Ran {
                    met,
                    tests_ran: Some(1),
                }
            });
            if a.real && r.audit.as_ref().is_some_and(|x| x.revoke) {
                outcomes.push(RunOutcome::SpotCheckDiffers);
            }
            continue;
        }
        let (l, c) = per.remove(&r.unit).unwrap_or_default();
        // the unit's own time: the sum of its cases' times (5d33a 0cddcc0c N1; not the whole run's)
        r.ms = c.iter().filter_map(|x| x.ms).sum();
        let reading = skipstop_cargo::read(&l, &c);
        let RunOutcome::Ran { met, tests_ran } = reading.outcome.clone() else {
            unreachable!("read always gives Ran")
        };
        r.ran = tests_ran.unwrap_or(0);
        r.result = if r.ran == 0 {
            Outcome::Invalid
        } else if met {
            Outcome::Pass
        } else {
            Outcome::Fail
        };
        r.failed = c
            .iter()
            .filter(|x| x.status == skipstop_cargo::CaseStatus::Failed)
            .map(|x| format!("{} {}", x.binary, x.name))
            .collect();
        r.failed.sort();
        r.cases = skipstop_cargo::cases_digest(&c);
        if let Some(src) = &r.source {
            // this real run is an audit of the source it would have reused: the two sides
            // compare on outcome, count, failed names, and on which cases ran and how each
            // ended. A source written before that line was taken has none: then neither side
            // brings one, and the older three decide
            let audit = Verdict {
                outcome: audit_outcome(r.result),
                tests_ran: Some(r.ran),
                failed: r.failed.clone(),
                output_digest: cases_to_compare(&src.output_digest, &r.cases),
                censored: false,
            };
            r.audit = Some(audit_of(&audit, src, met));
        }
        outcomes.push(reading.outcome);
        // real mode: a real run that disagrees with the source this unit was being reused
        // from is not a clean run, whatever the unit itself says now
        if a.real && r.audit.as_ref().is_some_and(|x| x.revoke) {
            outcomes.push(RunOutcome::SpotCheckDiffers);
        }
    }
    let code = exit_for(
        &outcomes,
        Use::Local,
        Some(std::env::consts::OS),
        false,
        run_failed && cases.is_empty(),
        &|_| false,
    );

    // the ledger: one stream for this run
    let head = git(&root, &["rev-parse", "HEAD"]).unwrap_or_default();
    let tree = git(&root, &["rev-parse", "HEAD^{tree}"]).unwrap_or_default();
    let dirty = git(&root, &["status", "--porcelain"]).is_none_or(|s| !s.is_empty());
    let inv = format!("tr-{}-{}", started, std::process::id());
    let plan = Plan {
        head: head.clone(),
        tree: if dirty {
            format!("{tree}+dirty")
        } else {
            tree.clone()
        },
        host: ctx.machine_kind.clone(),
        run_os: std::env::consts::OS.to_string(),
        cells: vec![if !a.real {
            real::CELL_OBSERVE
        } else if defense_round {
            real::CELL_DEFENSE
        } else {
            real::CELL_REAL
        }
        .into()],
        units: chosen.clone(),
    };
    let mut w = ledger::Writer::start(&state.join("ledger"), &rid, &inv, &seat, started, &plan)?;
    for r in &rows {
        let reused = !r.plan.runs();
        let u = Unit {
            unit: r.unit.clone(),
            krate: Some(r.unit.clone()),
            how: if reused { How::Reused } else { How::Ran },
            result: r.result,
            // the four words a reuse must name: where from, the key, the tree, when
            reuse: if reused { r.would.clone() } else { None },
            why_not: None,
            why_detail: r.unmeasurable.clone(),
            skip: None,
            dur_ms: if reused { None } else { Some(r.ms) },
            seat: None,
            key: r.key.clone(),
            tests_ran: if reused { None } else { Some(r.ran) },
            failed: r.failed.clone(),
            output_digest: if reused || r.cases.is_empty() {
                None
            } else {
                Some(r.cases.clone())
            },
            audit: r.audit.clone(),
            shadow: if reused {
                None
            } else {
                Some(Shadow {
                    would_reuse: r.would.clone(),
                    same: r.source_result.map(|s| s == r.result),
                })
            },
        };
        w.unit(now_ms(), &u)?;
    }
    w.end(now_ms(), true)?;

    // the events book beside the ledger: each unit's time, and what observe mode would have saved
    let at = now_ms();
    let ctx = skipstop_ledger::events::Ctx {
        host: None,
        seat: Some(seat.clone()).filter(|s| skipstop_ledger::events::seat_code(s)),
        cards: Vec::new(),
        status_item: None,
    };
    let ran: Vec<skipstop_cargo::run_events::Ran<'_>> = rows
        .iter()
        .map(|r| skipstop_cargo::run_events::Ran {
            reused: !r.plan.runs(),
            unit: &r.unit,
            ms: r.ms,
            tests: r.ran,
            outcome: r.result.word(),
            would: r.would.as_ref().map(|w| skipstop_cargo::run_events::Would {
                from: &w.from,
                source_ms: r.source_ms,
                agreed: r.source_result == Some(r.result)
                    && !r.audit.as_ref().is_some_and(|a| a.revoke),
            }),
        })
        .collect();
    let events_dir = state.join("events");
    // what earlier runs saved by reusing a source that has since been revoked is taken back:
    // the sources this run revoked and every one the ledger already holds, so a take-back
    // that could not be written once is written now
    let mut sources = real::revoked_sources(&streams);
    for r in rows
        .iter()
        .filter(|r| r.audit.as_ref().is_some_and(|x| x.revoke))
    {
        if let Some(w) = &r.would {
            sources.push((r.unit.clone(), w.from.clone()));
        }
    }
    sources.sort();
    sources.dedup();
    let mut taken_back = Vec::new();
    if !sources.is_empty() {
        let (book, _) = skipstop_ledger::timesave::read_dirs(std::slice::from_ref(&events_dir));
        for (unit, from) in &sources {
            let back = skipstop_cargo::run_events::take_back(&ctx, &book, unit, from, at);
            taken_back.extend(back);
        }
    }
    let evs = if a.real {
        skipstop_cargo::run_events::real(&ctx, &plan.host, &head, &ran, at)
    } else {
        skipstop_cargo::run_events::observe(&ctx, &plan.host, &head, &ran, at)
    };
    // the run on the clock, beside what a full run of the same units took: the only measure
    // of a saving that is time on the clock (unit times add up to more: units run side by side)
    let wall = skipstop_ledger::timesave::RunWall {
        wall_ms: u64::try_from(at - plan_started).unwrap_or(0),
        baseline_wall_ms: real::baseline_wall_ms(&streams, &chosen),
        units: rows.len() as u64,
        reused: rows.iter().filter(|r| !r.plan.runs()).count() as u64,
        real: a.real,
    };
    let wall_ev = skipstop_ledger::timesave::run_wall(&ctx, &wall, at).ok();
    for ev in evs.into_iter().chain(taken_back).chain(wall_ev) {
        // for people to read, never a gate: a write that fails is said and changes nothing
        if let Err(e) = skipstop_ledger::events::append_in(&events_dir, &ev, at) {
            eprintln!("aib-skipstop: event not written: {e}");
            break;
        }
    }

    // old ledger files go (never one that holds a revocation, never one that could not be
    // read: that one is a problem to be looked at, and it is not in `streams`)
    let mut expired_files = 0;
    for s in real::expired(&streams, config.keep_days, now_ms() / 1000) {
        let f = ledger::stream_path(&state.join("ledger"), &rid, &s.invocation_id);
        if std::fs::remove_file(f).is_ok() {
            expired_files += 1;
        }
    }

    // what it says
    let would = rows.iter().filter(|r| r.would.is_some()).count();
    let same = rows
        .iter()
        .filter(|r| r.source_result == Some(r.result))
        .count();
    if a.json {
        let v: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "unit": r.unit, "result": r.result.word(), "tests": r.ran,
                    "plan": r.plan.word(),
                    "key": r.key, "unmeasurable": r.unmeasurable,
                    "would_reuse": r.would.as_ref().map(|w| serde_json::json!({
                        "from": w.from, "key": w.key, "tree": w.tree, "at": w.at })),
                    "same": r.source_result.map(|s| s == r.result),
                    "audit": r.audit.as_ref().map(|a| serde_json::json!({
                        "compare": a.compare.word(), "revoke": a.revoke })),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({"invocation_id": inv, "head": head, "dirty": dirty,
                "mode": if a.real { "real" } else { "observe" }, "defense_round": defense_round,
                "ledger_files_expired": expired_files,
                "cases_run": cases.len(),
                "exit": code.code(), "units": v, "ledger_problems": problems})
        );
    } else {
        if a.why {
            for r in &rows {
                let src = r
                    .would
                    .as_ref()
                    .map(|w| {
                        format!(
                            "{} {} (key {} tree {} at {})",
                            if !r.plan.runs() {
                                "reused from"
                            } else if a.real {
                                "ran though it has a source"
                            } else {
                                "would reuse"
                            },
                            w.from,
                            w.key.get(..12).unwrap_or(&w.key),
                            w.tree.get(..12).unwrap_or(&w.tree),
                            w.at
                        )
                    })
                    .unwrap_or_else(|| {
                        r.unmeasurable
                            .clone()
                            .map(|e| format!("not measured: {e}"))
                            .unwrap_or_else(|| "no earlier passing run with this key".into())
                    });
                let src = match &r.audit {
                    Some(a) if a.revoke => {
                        format!("{src}; audit {} ⇒ source revoked", a.compare.word())
                    }
                    Some(a) => format!("{src}; audit {}", a.compare.word()),
                    None => src,
                };
                let tests = if r.plan.runs() {
                    format!("{:>4} tests", r.ran)
                } else {
                    "   not run".to_string()
                };
                println!(
                    "  {:<24} {:<7} {tests}  [{}] {src}",
                    r.unit,
                    r.result.word(),
                    r.plan.word()
                );
            }
            for p in &problems {
                println!("  ledger: {p}");
            }
        }
        let ledger_file = ledger::stream_path(&state.join("ledger"), &rid, &inv);
        if a.real {
            let reused = rows.iter().filter(|r| !r.plan.runs()).count();
            let audits = rows
                .iter()
                .filter(|r| matches!(r.plan, Decision::Audit(_)))
                .count();
            let revoked = rows
                .iter()
                .filter(|r| r.audit.as_ref().is_some_and(|x| x.revoke))
                .count();
            println!(
                "aib-skipstop: {} units, {} reused, {} ran ({} as audits{}){}, exit {}, ledger {}",
                rows.len(),
                reused,
                rows.len() - reused,
                audits,
                if revoked > 0 {
                    format!(", {revoked} source(s) revoked")
                } else {
                    String::new()
                },
                if defense_round {
                    "; a defense round: units whose round was due ran whatever their key"
                } else if a.fresh {
                    "; --fresh: everything ran"
                } else {
                    ""
                },
                code.code(),
                ledger_file.display()
            );
        } else {
            println!(
                "aib-skipstop (observe): {} units ran, {} would have been reused ({} same result), exit {}, ledger {}",
                rows.len(),
                would,
                same,
                code.code(),
                ledger_file.display()
            );
        }
    }
    Ok(code)
}

/// What one nextest call gave: the tests it listed, the cases it ran, and whether it ended
/// in failure.
type NextestRun = (skipstop_cargo::List, Vec<skipstop_cargo::Case>, bool);

/// Lists and runs the selected packages with the tool config, reads the JUnit file back and
/// removes this run's two files.
fn run_nextest(
    root: &Path,
    cfg: &Path,
    jp: &Path,
    select: &[String],
) -> Result<NextestRun, String> {
    // no silent fallback to `cargo test` (519d0 11-17 ruling 2): say what is missing
    let ver = cargo(root, &["nextest".to_string(), "--version".to_string()]);
    if !ver.as_ref().is_ok_and(|o| o.status.success()) {
        return Err(
            "cargo-nextest is not installed (cargo nextest --version failed); install it: \
             cargo install cargo-nextest --locked, or the prebuilt binary from get.nexte.st"
                .into(),
        );
    }
    let list = cargo(root, &list_args(cfg, select))?;
    if !list.status.success() {
        return Err(format!(
            "cargo nextest list failed: {}",
            String::from_utf8_lossy(&list.stderr).trim()
        ));
    }
    let listed = parse_list(&String::from_utf8_lossy(&list.stdout))?;
    // a junit.xml left by an earlier run must never be read as this one's (5d33a L1)
    match std::fs::remove_file(jp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "{}: cannot clear the last run's result: {e}",
                jp.display()
            ))
        }
    }
    let runout = cargo(root, &run_args(cfg, select))?;
    let xml = std::fs::read_to_string(jp).map_err(|e| format!("{}: {e}", jp.display()));
    // this run's two files are read once; left behind they would only pile up
    let _ = std::fs::remove_file(jp);
    let _ = std::fs::remove_file(cfg);
    let xml = xml?;
    let cases = parse_junit(&xml)?;
    Ok((listed, cases, !runout.status.success()))
}

/// The ledger's result in the audit's words (a line with no result never finished).
fn audit_outcome(o: Outcome) -> AuditOutcome {
    match o {
        Outcome::Pass => AuditOutcome::Pass,
        Outcome::Fail => AuditOutcome::Fail,
        Outcome::Invalid => AuditOutcome::Invalid,
        Outcome::None => AuditOutcome::Incomplete,
    }
}

/// The cases line this run brings to its audit: its own when the source has one, none when
/// the source has none (a ledger written before the line was taken). An old source is then
/// compared on outcome, count and failed names as it always was, not revoked for lacking a
/// line nobody took.
fn cases_to_compare(source: &str, now: &str) -> String {
    if source.is_empty() {
        String::new()
    } else {
        now.to_string()
    }
}

/// The audit line for one unit. A source line written before the ledger had `tests_ran`
/// (None) cannot be compared: Incomparable and never revoked, or the first run after an
/// upgrade would revoke every old source as a false mismatch (5d33a 598ce962 L1).
fn audit_of(audit: &Verdict, source: &Verdict, met: bool) -> Audit {
    if source.tests_ran.is_none() {
        return Audit {
            compare: AuditCompare::Incomparable,
            revoke: false,
        };
    }
    let l = engine::link(audit, source, met);
    // the only line taken is of which cases ran and how each ended. Under one key that is
    // the same every time; a difference means something the key does not hold decided which
    // tests there are, so the source is not one to reuse from: a mismatch, and revoked
    if l.compare == Compare::BehaviorChanged {
        return Audit {
            compare: AuditCompare::Mismatch,
            revoke: true,
        };
    }
    Audit {
        compare: match l.compare {
            Compare::Same => AuditCompare::Same,
            Compare::BehaviorChanged => AuditCompare::BehaviorChanged,
            Compare::Mismatch => AuditCompare::Mismatch,
            Compare::Incomparable => AuditCompare::Incomparable,
        },
        revoke: l.revoke,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_git_does_not_know_is_a_ledger_problem_so_nothing_is_reused() {
        let d = std::env::temp_dir().join(format!("skipstop-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(std::fs::create_dir_all(d.join("ledger")).is_ok());
        let (id, streams, problems) = repo_streams(&d.join("ledger"), &d);
        let _ = std::fs::remove_dir_all(&d);
        assert!(id.ends_with("-clone-unknown"), "{id}");
        assert!(streams.is_empty());
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("its ledger is not read"),
            "{problems:?}"
        );
    }

    fn v(tests_ran: Option<u32>, failed: &[&str]) -> Verdict {
        Verdict {
            outcome: AuditOutcome::Pass,
            tests_ran,
            failed: failed.iter().map(|s| s.to_string()).collect(),
            output_digest: String::new(),
            censored: false,
        }
    }

    #[test]
    fn an_old_source_line_without_tests_ran_is_incomparable_and_never_revoked() {
        // the shape of a unit line written before af917a7d: no tests_ran, no failed
        let a = audit_of(&v(Some(56), &[]), &v(None, &[]), true);
        assert_eq!(a.compare, AuditCompare::Incomparable);
        assert!(!a.revoke);
    }

    #[test]
    fn the_formats_document_names_every_plan_word_and_exit_code_there_is() {
        let doc = include_str!("../../../docs/FORMATS.md");
        // every reason a unit runs for, as the code spells it
        for why in [
            "observe",
            "fresh",
            "ledger_unread",
            "defense_round",
            "no_source",
            "flaky",
        ] {
            assert!(
                doc.contains(&format!("| `run:{why}` |")),
                "run:{why} is not in the document"
            );
        }
        assert!(doc.contains("| `reuse` |"));
        assert!(doc.contains("| `run:never_reuse:<class>` |"));
        for class in [
            "writes_worktree",
            "reads_outside",
            "side_effect",
            "time_random_home",
        ] {
            let n = skipstop_audit::NeverReuse::from_word(class).expect("a class");
            assert_eq!(
                skipstop_cargo::config::why_word(n),
                format!("never_reuse:{class}")
            );
            assert!(
                doc.contains(&format!("`{class}`")),
                "{class} is not in the document"
            );
        }
        for code in [
            Exit::Ok,
            Exit::Failed,
            Exit::ZeroUnits,
            Exit::NothingRan,
            Exit::TimedOut,
            Exit::Cancelled,
            Exit::EngineError,
            Exit::SpotCheckDiffers,
        ] {
            assert!(
                doc.contains(&format!("\n| {} | ", code.code())),
                "exit {} is not listed",
                code.code()
            );
        }
        assert!(doc.contains(&format!(
            "\n| {} | `stop` |",
            skipstop_stop::run::EXIT_STOPPED
        )));
        for kind in [
            "unit_time",
            "time_saved",
            "time_saved_confirm",
            "time_saved_wrong",
            "run_wall",
        ] {
            assert!(
                doc.contains(&format!("| `{kind}` |")),
                "event kind {kind} is not in the document"
            );
        }
    }

    #[test]
    fn the_plugin_file_names_this_version_and_the_command_this_crate_builds() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../../aib-plugin.json")).expect("JSON");
        assert_eq!(v["schema"], "aib-plugin/v1");
        assert_eq!(v["name"], "aib-skipstop");
        // one version: the workspace's, the plugin file's and the change log's newest heading
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
        let log = include_str!("../../../CHANGELOG.md");
        let newest = log
            .lines()
            .find_map(|l| l.strip_prefix("## "))
            .expect("a heading");
        assert!(
            newest.starts_with(concat!(env!("CARGO_PKG_VERSION"), " ")),
            "CHANGELOG.md starts with {newest:?}"
        );
        assert_eq!(v["commands"][0]["name"], "aib-skipstop");
        assert_eq!(v["commands"][0]["bin"], "bin/aib-skipstop");
        assert_eq!(v["reads"][0]["run"][0], "aib-skipstop");
        assert!(matches!(v["default"].as_str(), Some("on" | "off")));
        assert!(v["paid"].is_boolean());
    }

    #[test]
    fn the_mode_is_the_first_that_says_and_a_word_that_is_no_mode_is_an_error() {
        // nothing says: observe
        assert_eq!(mode_of(None, None, None), Ok((false, "the default")));
        assert_eq!(
            mode_of(None, Some(""), Some("\n")),
            Ok((false, "the default"))
        );
        // the file, then the variable over it, then the flag over both
        assert_eq!(
            mode_of(None, None, Some("real\n")),
            Ok((true, "the mode file"))
        );
        assert_eq!(
            mode_of(None, Some("observe"), Some("real\n")),
            Ok((false, "AIB_SKIPSTOP_MODE"))
        );
        assert_eq!(
            mode_of(Some(true), Some("observe"), Some("observe")),
            Ok((true, "--mode"))
        );
        // a typo is never run as something else
        assert!(mode_of(None, Some("fast"), None).is_err());
        // also where something stronger would have won: every word given is read
        assert!(mode_of(Some(true), Some("ra el"), None)
            .unwrap_err()
            .contains("AIB_SKIPSTOP_MODE"));
        assert!(mode_of(Some(true), None, Some("reel"))
            .unwrap_err()
            .contains("mode file"));
        assert!(mode_of(None, Some("real"), Some("reel")).is_err());
        assert!(mode_of(None, None, Some("on"))
            .unwrap_err()
            .contains("mode file"));
        assert!(mode_word("Real").is_err());
    }

    #[test]
    fn the_ledger_follows_the_aib_state_root_like_aib_drive() {
        let led = |p: &str| Path::new(p).join("state").join("aib-skipstop");
        let home = Some(Path::new("/h"));
        assert_eq!(
            root_from(Some(Path::new("/f")), Some("/r"), Some("/a"), home),
            Path::new("/f")
        );
        assert_eq!(root_from(None, Some("/r"), Some("/a"), home), led("/r"));
        assert_eq!(root_from(None, Some(""), Some("/a"), home), led("/a"));
        assert_eq!(root_from(None, None, Some(""), home), led("/h/.ez2-aib"));
        assert_eq!(root_from(None, None, None, None), led("./.ez2-aib"));
    }

    #[test]
    fn a_tool_s_line_is_of_its_command_and_of_each_stream_apart() {
        let git = |args: &[&str]| {
            let mut v = vec!["git".to_string()];
            v.extend(args.iter().map(|s| s.to_string()));
            tool_text(Path::new("."), &v)
        };
        let a = git(&["--version"]).expect("git is here");
        assert_eq!(a, git(&["--version"]).expect("git is here"));
        // the command, then two sha256: standard output, standard error (nothing: the empty hash)
        let parts: Vec<&str> = a.rsplitn(3, ' ').collect();
        assert_eq!((parts[0].len(), parts[1].len()), (64, 64), "{a}");
        assert_eq!(
            parts[0],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_ne!(parts[1], parts[0]);
        assert_eq!(parts[2], r#"["git","--version"]"#);
        // a tool that does not end well, and one that is not there, give no line
        assert!(git(&["no-such-git-command-skipstop"]).is_err());
        assert!(tool_text(Path::new("."), &["no-such-program-skipstop".to_string()]).is_err());
    }

    #[test]
    fn same_count_and_names_is_same_and_a_different_count_revokes() {
        let a = audit_of(&v(Some(56), &[]), &v(Some(56), &[]), true);
        assert_eq!((a.compare, a.revoke), (AuditCompare::Same, false));
        let b = audit_of(&v(Some(57), &[]), &v(Some(56), &[]), true);
        assert_eq!((b.compare, b.revoke), (AuditCompare::Mismatch, true));
    }

    #[test]
    fn the_same_count_of_other_cases_revokes_and_a_source_with_no_cases_line_does_not() {
        let with = |d: &str| Verdict {
            output_digest: d.into(),
            ..v(Some(56), &[])
        };
        let same = audit_of(&with("cases-v1:aa"), &with("cases-v1:aa"), true);
        assert_eq!((same.compare, same.revoke), (AuditCompare::Same, false));
        let other = audit_of(&with("cases-v1:bb"), &with("cases-v1:aa"), true);
        assert_eq!(
            (other.compare, other.revoke),
            (AuditCompare::Mismatch, true)
        );
        // an old source has no line: this run brings none either, and the two are the same
        assert_eq!(cases_to_compare("", "cases-v1:bb"), "");
        assert_eq!(
            cases_to_compare("cases-v1:aa", "cases-v1:bb"),
            "cases-v1:bb"
        );
        let now = with(&cases_to_compare("", "cases-v1:bb"));
        let old = audit_of(&now, &with(""), true);
        assert_eq!((old.compare, old.revoke), (AuditCompare::Same, false));
    }
}
