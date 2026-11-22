//! Stop: before a command that costs a lot (an hour of tests, a place in a queue), run cheap
//! checks, and do not start it when one of them shows it is sure to fail.
//!
//! The checks are data, a JSON file (`skipstop.stop.json` in the repo root unless named):
//!
//! ```json
//! {"checks": [{
//!   "id": "merge-conflict",
//!   "kind": "reject",
//!   "cmd": ["git", "merge-tree", "--write-tree", "--quiet", "HEAD", "origin/main"],
//!   "avoids": "a CI round on a branch that cannot be merged",
//!   "saves_ms": 3600000
//! }]}
//! ```
//!
//! - `kind`: `reject` (a finding stops the command) or `warn` (a finding is printed, nothing
//!   is stopped: a guess is never a reason to stop work).
//! - A check **finds** something when its command ends with one of `stop_codes` (default `[1]`).
//!   Exit 0 is a pass.
//! - Anything else is a **broken** check: it could not start, ran past `timeout_s` (default
//!   60), was ended by a signal, or ended with a code that is neither 0 nor a stop code. A
//!   broken check stops nothing and is said out loud. A check that breaks must never be able
//!   to block all work, and it must never read as a pass either.
//! - What a stop saves: `saves_ms` (a number someone measured and wrote down) or `saves_unit`
//!   (the median of that unit's last runs in the events book). Neither, or a unit with no
//!   runs: the stop is recorded without a number being made up.
//!
//! Modes, as for skip: in **observe** the checks run and what they would have stopped is
//! recorded, and the command runs anyway, so the two can be compared (would stop, and the
//! command did fail: right; would stop, and it passed: that check would have blocked good
//! work). In **real** a `reject` finding stops the command.

pub mod run;

use serde_json::Value;
use std::collections::BTreeSet;

/// What a finding of the check does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Reject,
    Warn,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Reject => "reject",
            Kind::Warn => "warn",
        }
    }
}

/// One check as its file gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub id: String,
    pub kind: Kind,
    pub cmd: Vec<String>,
    /// What is not done when this check stops the command, for a person.
    pub avoids: String,
    pub saves_ms: Option<u64>,
    pub saves_unit: Option<String>,
    pub timeout_s: u64,
    pub stop_codes: Vec<i32>,
}

pub const TIMEOUT_DEFAULT_S: u64 = 60;
pub const TIMEOUT_MAX_S: u64 = 3600;

fn id_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Reads a checks file. A field that is there and not of its kind is an error naming the
/// check; an id comes once; a key that is not known is an error (a typo in `kind` or
/// `stop_codes` must not silently turn a check into another one).
pub fn parse(text: &str) -> Result<Vec<Check>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let list = v
        .get("checks")
        .and_then(Value::as_array)
        .ok_or("no \"checks\" list")?;
    const KEYS: [&str; 8] = [
        "id",
        "kind",
        "cmd",
        "avoids",
        "saves_ms",
        "saves_unit",
        "timeout_s",
        "stop_codes",
    ];
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (i, c) in list.iter().enumerate() {
        let n = i + 1;
        let o = c.as_object().ok_or(format!("check {n}: not an object"))?;
        if let Some(k) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(format!("check {n}: \"{k}\" is not a field of a check"));
        }
        let text = |f: &str| -> Result<String, String> {
            o.get(f)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("check {n}: no \"{f}\""))
        };
        let id = text("id")?;
        if !id_ok(&id) {
            return Err(format!("check {n}: id {id:?} is not a-z 0-9 - _ (1 to 64)"));
        }
        let kind = match text("kind")?.as_str() {
            "reject" => Kind::Reject,
            "warn" => Kind::Warn,
            other => return Err(format!("check {id}: kind {other:?} is not reject or warn")),
        };
        let words = o.get("cmd").and_then(Value::as_array);
        let cmd: Vec<String> = words
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if cmd.is_empty() || cmd.len() != words.map_or(0, Vec::len) || cmd[0].is_empty() {
            return Err(format!("check {id}: \"cmd\" is not a list of words"));
        }
        let number = |f: &str| -> Result<Option<u64>, String> {
            match o.get(f) {
                None => Ok(None),
                Some(v) => v
                    .as_u64()
                    .map(Some)
                    .ok_or_else(|| format!("check {id}: \"{f}\" is not a whole number from 0")),
            }
        };
        let saves_ms = number("saves_ms")?;
        let saves_unit = match o.get("saves_unit") {
            None => None,
            Some(_) => Some(text("saves_unit")?),
        };
        if saves_ms.is_some() && saves_unit.is_some() {
            return Err(format!(
                "check {id}: saves_ms and saves_unit are both given; one says it"
            ));
        }
        let timeout_s = number("timeout_s")?.unwrap_or(TIMEOUT_DEFAULT_S);
        if !(1..=TIMEOUT_MAX_S).contains(&timeout_s) {
            return Err(format!(
                "check {id}: timeout_s is not from 1 to {TIMEOUT_MAX_S}"
            ));
        }
        let stop_codes: Vec<i32> = match o.get("stop_codes") {
            None => vec![1],
            Some(v) => {
                let a = v
                    .as_array()
                    .ok_or(format!("check {id}: \"stop_codes\" is not a list"))?;
                let codes: Vec<i32> = a
                    .iter()
                    .filter_map(Value::as_i64)
                    .filter_map(|c| i32::try_from(c).ok())
                    .collect();
                // 0 is the pass: a check that stops on 0 would stop every time it works
                if codes.is_empty() || codes.len() != a.len() || codes.contains(&0) {
                    return Err(format!(
                        "check {id}: \"stop_codes\" is not a list of exit codes other than 0"
                    ));
                }
                codes
            }
        };
        if !seen.insert(id.clone()) {
            return Err(format!("check id {id} comes twice"));
        }
        out.push(Check {
            id,
            kind,
            cmd,
            avoids: text("avoids")?,
            saves_ms,
            saves_unit,
            timeout_s,
            stop_codes,
        });
    }
    if out.is_empty() {
        return Err("the \"checks\" list is empty".into());
    }
    Ok(out)
}

/// How a check's command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    Code(i32),
    /// Ended by a signal (no exit code).
    Signal,
    /// Ran past its time and was killed.
    Timeout,
    /// Could not be started.
    NoStart(String),
}

/// What a check says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Says {
    Pass,
    /// It found what it looks for.
    Finding,
    /// It did not work: neither a pass nor a finding.
    Broken(String),
}

pub fn says(c: &Check, ended: &Ended) -> Says {
    match ended {
        Ended::Code(0) => Says::Pass,
        Ended::Code(code) if c.stop_codes.contains(code) => Says::Finding,
        Ended::Code(code) => Says::Broken(format!(
            "exit {code} is neither 0 nor one of its stop codes {:?}",
            c.stop_codes
        )),
        Ended::Signal => Says::Broken("ended by a signal".into()),
        Ended::Timeout => Says::Broken(format!("ran past its {} s", c.timeout_s)),
        Ended::NoStart(e) => Says::Broken(format!("could not start: {e}")),
    }
}

/// What the checks together say about the command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    /// `reject` checks with a finding: these stop the command (in real mode).
    pub stops: Vec<String>,
    /// `warn` checks with a finding: printed, nothing stopped.
    pub warns: Vec<String>,
    /// Checks that did not work, with why: said out loud, nothing stopped.
    pub broken: Vec<(String, String)>,
    pub passed: usize,
}

impl Verdict {
    /// Would the command be stopped? Only a `reject` finding does that.
    pub fn stop(&self) -> bool {
        !self.stops.is_empty()
    }
}

pub fn verdict(results: &[(&Check, Says)]) -> Verdict {
    let mut v = Verdict::default();
    for (c, s) in results {
        match (s, c.kind) {
            (Says::Pass, _) => v.passed += 1,
            (Says::Finding, Kind::Reject) => v.stops.push(c.id.clone()),
            (Says::Finding, Kind::Warn) => v.warns.push(c.id.clone()),
            (Says::Broken(why), _) => v.broken.push((c.id.clone(), why.clone())),
        }
    }
    v
}

/// What observe mode learns once the command has run anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// Nothing would have stopped it.
    NoStop,
    /// A check would have stopped it, and it did fail: the stop would have been right.
    Right,
    /// A check would have stopped it, and it passed: that check would have blocked good work.
    WouldBlockGoodWork,
}

pub fn observed(v: &Verdict, command_ok: bool) -> Observed {
    match (v.stop(), command_ok) {
        (false, _) => Observed::NoStop,
        (true, false) => Observed::Right,
        (true, true) => Observed::WouldBlockGoodWork,
    }
}

/// The lower median of the last `last` `unit_time` events of `unit` that passed, in the order
/// the events are given; None with none (no number is made up).
pub fn unit_median_ms(evs: &[Value], unit: &str, last: usize) -> Option<u64> {
    let mut ms: Vec<u64> = evs
        .iter()
        .filter(|v| v["kind"] == "unit_time" && v["unit"] == unit && v["outcome"] == "pass")
        .filter_map(|v| v["ms"].as_u64())
        .collect();
    if ms.len() > last {
        ms.drain(..ms.len() - last);
    }
    if ms.is_empty() {
        return None;
    }
    ms.sort_unstable();
    Some(ms[(ms.len() - 1) / 2])
}

/// How many of a unit's last runs a `saves_unit` median is taken over.
pub const MEDIAN_OVER: usize = 7;

/// What stopping on `c` saves, and where the number comes from; None = not known.
pub fn saving(c: &Check, evs: &[Value]) -> Option<(u64, String)> {
    if let Some(ms) = c.saves_ms {
        return Some((ms, "saves_ms in the checks file".into()));
    }
    let unit = c.saves_unit.as_deref()?;
    let ms = unit_median_ms(evs, unit, MEDIAN_OVER)?;
    Some((
        ms,
        format!("median of the last runs of {unit} in the events book (up to {MEDIAN_OVER})"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ONE: &str = r#"{"checks": [{"id": "merge-conflict", "kind": "reject",
        "cmd": ["git", "merge-tree", "HEAD", "origin/main"], "avoids": "a CI round"}]}"#;

    fn check(kind: Kind) -> Check {
        Check {
            id: "c".into(),
            kind,
            cmd: vec!["x".into()],
            avoids: "a run".into(),
            saves_ms: None,
            saves_unit: None,
            timeout_s: 60,
            stop_codes: vec![1],
        }
    }

    #[test]
    fn a_checks_file_is_read_whole_or_not_at_all() {
        let c = &parse(ONE).unwrap()[0];
        assert_eq!(
            (c.id.as_str(), c.kind, c.timeout_s),
            ("merge-conflict", Kind::Reject, 60)
        );
        assert_eq!(
            (c.stop_codes.clone(), c.saves_ms, c.saves_unit.clone()),
            (vec![1], None, None)
        );
        let with = |f: &str, v: Value| {
            let mut j: Value = serde_json::from_str(ONE).unwrap();
            j["checks"][0][f] = v;
            parse(&j.to_string())
        };
        assert_eq!(with("kind", json!("warn")).unwrap()[0].kind, Kind::Warn);
        assert_eq!(with("saves_ms", json!(5)).unwrap()[0].saves_ms, Some(5));
        assert_eq!(
            with("stop_codes", json!([1, 3])).unwrap()[0].stop_codes,
            vec![1, 3]
        );
        // a word that is not a kind is not read as one
        assert!(with("kind", json!("block"))
            .unwrap_err()
            .contains("reject or warn"));
        // a key nobody knows: a typo must not turn the check into another one
        assert!(with("stop_code", json!([2]))
            .unwrap_err()
            .contains("not a field"));
        // 0 as a stop code would stop every time the check works
        assert!(with("stop_codes", json!([0])).is_err());
        assert!(with("stop_codes", json!([])).is_err());
        assert!(with("stop_codes", json!(["1"])).is_err());
        assert!(with("timeout_s", json!(0)).is_err());
        assert!(with("timeout_s", json!(99999)).is_err());
        assert!(with("saves_ms", json!(-1)).is_err());
        assert!(with("cmd", json!([])).is_err());
        assert!(with("cmd", json!(["git", 1])).is_err());
        assert!(with("id", json!("Merge Conflict")).is_err());
        assert!(with("avoids", json!("  ")).is_err());
        // two ways to say what it saves: which one is meant?
        let mut j: Value = serde_json::from_str(ONE).unwrap();
        j["checks"][0]["saves_ms"] = json!(1);
        j["checks"][0]["saves_unit"] = json!("u");
        assert!(parse(&j.to_string()).unwrap_err().contains("both"));
        // the same id twice, no check, not a list
        let mut j: Value = serde_json::from_str(ONE).unwrap();
        let first = j["checks"][0].clone();
        j["checks"].as_array_mut().unwrap().push(first);
        assert!(parse(&j.to_string()).unwrap_err().contains("twice"));
        assert!(parse(r#"{"checks": []}"#).unwrap_err().contains("empty"));
        assert!(parse("{}").is_err() && parse("nope").is_err());
    }

    #[test]
    fn a_check_passes_finds_or_is_broken_and_broken_is_neither() {
        let c = check(Kind::Reject);
        assert_eq!(says(&c, &Ended::Code(0)), Says::Pass);
        assert_eq!(says(&c, &Ended::Code(1)), Says::Finding);
        // a code that is neither: the tool itself failed (git's 128, a missing file's 2)
        assert!(matches!(says(&c, &Ended::Code(128)), Says::Broken(w) if w.contains("128")));
        assert!(matches!(says(&c, &Ended::Signal), Says::Broken(_)));
        assert!(matches!(says(&c, &Ended::Timeout), Says::Broken(w) if w.contains("60 s")));
        assert!(
            matches!(says(&c, &Ended::NoStart("no such file".into())), Says::Broken(w) if w.contains("no such file"))
        );
        // its own stop codes
        let mut c3 = check(Kind::Reject);
        c3.stop_codes = vec![3];
        assert_eq!(says(&c3, &Ended::Code(3)), Says::Finding);
        assert!(matches!(says(&c3, &Ended::Code(1)), Says::Broken(_)));
    }

    #[test]
    fn only_a_reject_finding_stops_and_a_broken_check_stops_nothing() {
        let (r, w) = (check(Kind::Reject), check(Kind::Warn));
        let broken = Says::Broken("ran past its 60 s".into());
        // a warn finding and a broken reject: nothing is stopped, both are said
        let v = verdict(&[(&w, Says::Finding), (&r, broken.clone()), (&r, Says::Pass)]);
        assert!(!v.stop());
        assert_eq!((v.warns.len(), v.broken.len(), v.passed), (1, 1, 1));
        // one reject finding stops, whatever else passed
        let v = verdict(&[(&r, Says::Pass), (&r, Says::Finding), (&w, Says::Pass)]);
        assert!(v.stop());
        assert_eq!(v.stops, vec!["c".to_string()]);
        // nothing ran: nothing stops
        assert!(!verdict(&[]).stop());
        // a broken warn is broken too, not a warning
        let v = verdict(&[(&w, broken)]);
        assert_eq!((v.warns.len(), v.broken.len()), (0, 1));
    }

    #[test]
    fn observe_mode_tells_a_right_stop_from_one_that_would_block_good_work() {
        let r = check(Kind::Reject);
        let stop = verdict(&[(&r, Says::Finding)]);
        let go = verdict(&[(&r, Says::Pass)]);
        assert_eq!(observed(&stop, false), Observed::Right);
        assert_eq!(observed(&stop, true), Observed::WouldBlockGoodWork);
        assert_eq!(observed(&go, false), Observed::NoStop);
        assert_eq!(observed(&go, true), Observed::NoStop);
    }

    #[test]
    fn what_a_stop_saves_is_written_down_or_measured_and_never_made_up() {
        let unit = |u: &str, ms: u64, outcome: &str| json!({"kind": "unit_time", "unit": u, "ms": ms, "outcome": outcome});
        let evs = vec![
            unit("tests", 100, "pass"),
            // two failed runs, short ones: counted in, they would pull the median to 100
            unit("tests", 10, "fail"),
            unit("tests", 20, "fail"),
            unit("other", 5, "pass"),
            unit("tests", 300, "pass"),
            unit("tests", 200, "pass"),
            json!({"kind": "time_saved", "unit": "tests", "ms": 1, "outcome": "pass"}),
        ];
        // the passes of that unit only: 100 300 200 ⇒ 200
        assert_eq!(unit_median_ms(&evs, "tests", 7), Some(200));
        // the last two: 300 200 ⇒ the lower one
        assert_eq!(unit_median_ms(&evs, "tests", 2), Some(200));
        assert_eq!(unit_median_ms(&evs, "tests", 1), Some(200));
        assert_eq!(unit_median_ms(&evs, "never-ran", 7), None);
        let mut c = check(Kind::Reject);
        assert_eq!(saving(&c, &evs), None);
        c.saves_unit = Some("tests".into());
        let (ms, basis) = saving(&c, &evs).unwrap();
        assert_eq!(ms, 200);
        assert!(
            basis.contains("median") && basis.contains("tests"),
            "{basis}"
        );
        c.saves_unit = Some("never-ran".into());
        assert_eq!(saving(&c, &evs), None);
        c.saves_unit = None;
        c.saves_ms = Some(42);
        assert_eq!(saving(&c, &evs).unwrap().0, 42);
    }
}
