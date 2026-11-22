//! What one run writes into the events book beside its ledger: each unit's time and size, and
//! in observe mode each reuse it would have made.
//!
//! - `unit_time`: every unit that ran: its own milliseconds (the sum of its cases, not the
//!   run's wall clock), how many tests it ran, the kind of machine, how it ended.
//! - `time_saved` with `mode = observe`: a unit that had a source it would have reused, whose
//!   real run now agreed with that source, and whose source says how long it took. The saving is
//!   the source run's own unit time, a fact, so `confirmed` is true; observe mode keeps it out
//!   of every total all the same (nothing was skipped). A unit whose real run disagreed, or
//!   whose source has no time, writes none: no number is made up.
//!
//! Pure: the events are built here and written by the caller, and a write that fails never
//! changes the run's exit code.

use serde_json::Value;
use skipstop_ledger::events::Ctx;
use skipstop_ledger::timesave::{self, Saved, System, Unit};

/// The run a unit would have been reused from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Would<'a> {
    /// `<repo id>/<invocation id>` of the source run.
    pub from: &'a str,
    /// The unit's time in that run; None when its line has none.
    pub source_ms: Option<u64>,
    /// The real run now came out the same as the source.
    pub agreed: bool,
}

/// One unit of this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran<'a> {
    pub unit: &'a str,
    /// Real mode: the unit was not run; its source's result stands.
    pub reused: bool,
    pub ms: u64,
    pub tests: u32,
    /// `pass` / `fail` / `invalid` / `none`, the ledger's word.
    pub outcome: &'a str,
    pub would: Option<Would<'a>>,
}

/// The events of one observe-mode run, in unit order: a `unit_time` each, then the unit's
/// `time_saved` when it has one. A unit whose fields do not make an event (an empty name) is
/// left out of the book, not written wrong.
pub fn observe(ctx: &Ctx, runner: &str, head: &str, units: &[Ran<'_>], at_ms: i64) -> Vec<Value> {
    let mut out = Vec::new();
    for u in units {
        let time = Unit {
            unit: u.unit,
            ms: u.ms,
            runner,
            size: Some(u64::from(u.tests)),
            load1: None,
            outcome: u.outcome,
        };
        if let Ok(v) = timesave::unit_time(ctx, &time, at_ms) {
            out.push(v);
        }
        let Some(w) = &u.would else { continue };
        let (true, Some(ms)) = (w.agreed, w.source_ms) else {
            continue;
        };
        let saved = Saved {
            system: System::Shadow,
            rule: "reuse",
            verdict: "reuse",
            avoided: &format!("cargo nextest run -p {}", u.unit),
            saved_ms: ms,
            confirmed: true,
            basis: &format!(
                "the unit's own time in the run it would have reused ({}); observe mode: it ran anyway",
                w.from
            ),
            real: false,
            head: Some(head).filter(|h| !h.is_empty()),
        };
        if let Ok(v) = timesave::saved(ctx, &saved, at_ms) {
            out.push(v);
        }
    }
    out
}

/// The events of one real-mode run, in unit order: a unit that ran writes its `unit_time`; a
/// unit that was reused writes a `time_saved` with `mode = real` when its source says how long
/// it took (the source's own unit time, a fact, so `confirmed`). A reused unit whose source has
/// no time writes nothing: it was saved, by a number nobody knows.
pub fn real(ctx: &Ctx, runner: &str, head: &str, units: &[Ran<'_>], at_ms: i64) -> Vec<Value> {
    let mut out = Vec::new();
    for u in units {
        if !u.reused {
            let time = Unit {
                unit: u.unit,
                ms: u.ms,
                runner,
                size: Some(u64::from(u.tests)),
                load1: None,
                outcome: u.outcome,
            };
            if let Ok(v) = timesave::unit_time(ctx, &time, at_ms) {
                out.push(v);
            }
            continue;
        }
        let Some((from, ms)) = u.would.as_ref().and_then(|w| Some((w.from, w.source_ms?))) else {
            continue;
        };
        let saved = Saved {
            system: System::Shadow,
            rule: "reuse",
            verdict: "reuse",
            avoided: &format!("cargo nextest run -p {}", u.unit),
            saved_ms: ms,
            confirmed: true,
            basis: &format!("the unit's own time in the run it was reused from ({from})"),
            real: true,
            head: Some(head).filter(|h| !h.is_empty()),
        };
        if let Ok(v) = timesave::saved(ctx, &saved, at_ms) {
            out.push(v);
        }
    }
    out
}

fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// `+08:00` for 480.
pub fn offset_text(min: i32) -> String {
    let sign = if min < 0 { '-' } else { '+' };
    format!("{sign}{:02}:{:02}", min.abs() / 60, min.abs() % 60)
}

/// The five boxes as lines for a person: confirmed large, estimated and penalty beside it and
/// never added in, the two systems apart, and what stands outside every box said last.
pub fn saved_text(s: &timesave::Summary, offset_min: i32) -> String {
    let mut out = format!(
        "time saved (days on a UTC{} clock)\n",
        offset_text(offset_min)
    );
    for (name, p) in [
        ("today", &s.today),
        ("this week", &s.week),
        ("this month", &s.month),
        ("this year", &s.year),
        ("all", &s.all),
    ] {
        let t = p.total();
        out += &format!(
            "  {name:<10} {}  (skip {} · stop {})  estimated {}  taken back {}  savings {}\n",
            clock(t.confirmed_ms),
            clock(p.shadow.confirmed_ms),
            clock(p.precheck.confirmed_ms),
            clock(t.estimated_ms),
            clock(t.penalty_ms),
            t.count
        );
    }
    match s.since_ms {
        Some(ms) => out += &format!("  since {}\n", skipstop_ledger::envelope::rfc3339_ms(ms)),
        None => out += "  nothing saved for real yet\n",
    }
    if s.observed > 0 {
        out += &format!(
            "  {} observe-mode prediction(s): the unit ran anyway, in no total\n",
            s.observed
        );
    }
    if s.bad > 0 {
        out += &format!(
            "  {} more line(s) could not be read and are not counted\n",
            s.bad
        );
    }
    out
}

/// The same numbers for a program (the page): milliseconds, nothing formatted.
pub fn saved_json(s: &timesave::Summary, offset_min: i32) -> Value {
    let sum = |x: &timesave::Sum| {
        serde_json::json!({
            "confirmed_ms": x.confirmed_ms, "estimated_ms": x.estimated_ms,
            "penalty_ms": x.penalty_ms, "count": x.count,
        })
    };
    let period = |p: &timesave::Period| serde_json::json!({ "total": sum(&p.total()), "skip": sum(&p.shadow), "stop": sum(&p.precheck) });
    serde_json::json!({
        "v": 1,
        "utc_offset_min": offset_min,
        "today": period(&s.today), "week": period(&s.week), "month": period(&s.month),
        "year": period(&s.year), "all": period(&s.all),
        "since": s.since_ms.map(skipstop_ledger::envelope::rfc3339_ms),
        "observed": s.observed,
        "unread": s.bad,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: i64 = 1_795_399_200_000;

    fn ctx() -> Ctx {
        Ctx {
            host: None,
            seat: Some("c5d88".into()),
            cards: vec![],
            status_item: None,
        }
    }
    fn ran<'a>(unit: &'a str, would: Option<Would<'a>>) -> Ran<'a> {
        Ran {
            unit,
            reused: false,
            ms: 1_500,
            tests: 28,
            outcome: "pass",
            would,
        }
    }

    #[test]
    fn every_unit_writes_its_time_and_only_an_agreed_reuse_with_a_known_time_writes_a_saving() {
        let from = "repo/tr-1";
        let units = [
            ran("plain", None),
            ran(
                "reused",
                Some(Would {
                    from,
                    source_ms: Some(900),
                    agreed: true,
                }),
            ),
            ran(
                "differs",
                Some(Would {
                    from,
                    source_ms: Some(900),
                    agreed: false,
                }),
            ),
            ran(
                "no-time",
                Some(Would {
                    from,
                    source_ms: None,
                    agreed: true,
                }),
            ),
        ];
        let evs = observe(&ctx(), "macos-aarch64", "abc123", &units, AT);
        let kinds: Vec<(&str, &str)> = evs
            .iter()
            .map(|v| {
                (
                    v["kind"].as_str().unwrap(),
                    v.get("unit").and_then(Value::as_str).unwrap_or(""),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("unit_time", "plain"),
                ("unit_time", "reused"),
                ("time_saved", ""),
                ("unit_time", "differs"),
                ("unit_time", "no-time"),
            ]
        );
        let t = &evs[1];
        assert_eq!(
            (t["ms"].as_u64(), t["size"].as_u64()),
            (Some(1_500), Some(28))
        );
        assert_eq!(t["runner"], "macos-aarch64");
        assert_eq!(t["outcome"], "pass");
        let s = &evs[2];
        assert_eq!(s["system"], "shadow");
        assert_eq!(s["verdict"], "reuse");
        // the source's time, not this run's
        assert_eq!(s["saved_ms"], 900);
        assert_eq!(s["mode"], "observe");
        assert_eq!(s["confirmed"], true);
        assert_eq!(s["avoided"], "cargo nextest run -p reused");
        assert!(s["basis"].as_str().unwrap().contains(from), "{s}");
        assert_eq!(s["head"], "abc123");
    }

    #[test]
    fn the_boxes_are_printed_with_estimates_and_predictions_beside_and_never_added_in() {
        let gate = |ms: u64, confirmed: bool, real: bool| Saved {
            system: System::Precheck,
            rule: "knife-precheck",
            verdict: "reject",
            avoided: "a run",
            saved_ms: ms,
            confirmed,
            basis: "measured",
            real,
            head: None,
        };
        let evs = vec![
            timesave::saved(&ctx(), &gate(3_725_000, true, true), AT).unwrap(),
            timesave::saved(&ctx(), &gate(60_000, false, true), AT).unwrap(),
            timesave::saved(&ctx(), &gate(9_999_000, true, false), AT).unwrap(),
            Value::from("not an event"),
        ];
        let s = timesave::summarize(&evs, AT, 480);
        let text = saved_text(&s, 480);
        assert!(
            text.starts_with("time saved (days on a UTC+08:00 clock)\n"),
            "{text}"
        );
        // 1:02:05 confirmed; the estimate (0:01:00) and the prediction are not in it
        assert!(
            text.contains("  today      1:02:05  (skip 0:00:00 · stop 1:02:05)  estimated 0:01:00  taken back 0:00:00  savings 2\n"),
            "{text}"
        );
        assert!(text.contains("1 observe-mode prediction(s)"), "{text}");
        assert!(text.contains("1 more line(s) could not be read"), "{text}");
        let j = saved_json(&s, 480);
        assert_eq!(j["today"]["total"]["confirmed_ms"], 3_725_000);
        assert_eq!(j["today"]["total"]["estimated_ms"], 60_000);
        assert_eq!(j["all"]["stop"]["count"], 2);
        assert_eq!(j["all"]["skip"]["confirmed_ms"], 0);
        assert_eq!(
            (j["observed"].as_u64(), j["unread"].as_u64()),
            (Some(1), Some(1))
        );
        assert_eq!(offset_text(-330), "-05:30");
        // nothing real yet is said, not shown as a row of zeros alone
        let none = timesave::summarize(&[], AT, 0);
        assert!(saved_text(&none, 0).contains("nothing saved for real yet"));
        assert!(saved_json(&none, 0)["since"].is_null());
    }

    #[test]
    fn a_real_run_saves_what_it_did_not_run_and_times_what_it_ran() {
        let from = "repo/tr-1";
        let reused = |unit, source_ms| Ran {
            reused: true,
            would: Some(Would {
                from,
                source_ms,
                agreed: true,
            }),
            ..ran(unit, None)
        };
        let units = [
            ran("ran", None),
            reused("skipped", Some(900)),
            reused("no-time", None),
        ];
        let evs = real(&ctx(), "macos-aarch64", "abc123", &units, AT);
        // the unit that ran: its time; the reused one: a saving and no time of its own;
        // the one whose source has no time: nothing, not a zero
        assert_eq!(evs.len(), 2, "{evs:?}");
        assert_eq!(
            (evs[0]["kind"].as_str(), evs[0]["unit"].as_str()),
            (Some("unit_time"), Some("ran"))
        );
        let s = &evs[1];
        assert_eq!(s["kind"], "time_saved");
        assert_eq!(s["mode"], "real");
        assert_eq!(s["saved_ms"], 900);
        assert_eq!(s["confirmed"], true);
        assert_eq!(s["avoided"], "cargo nextest run -p skipped");
        assert!(s["basis"].as_str().unwrap().contains(from), "{s}");
        // and it is in the total, where an observe-mode prediction is not
        let sum = timesave::summarize(&evs, AT, 0);
        assert_eq!(sum.all.shadow.confirmed_ms, 900);
        assert_eq!((sum.all.shadow.count, sum.observed, sum.bad), (1, 0, 0));
    }

    #[test]
    fn an_observe_run_adds_nothing_to_the_totals() {
        let units = [ran(
            "reused",
            Some(Would {
                from: "repo/tr-1",
                source_ms: Some(900),
                agreed: true,
            }),
        )];
        let evs = observe(&ctx(), "linux-x86_64", "", &units, AT);
        assert!(
            evs[1].get("head").is_none(),
            "no head known: no head written"
        );
        let s = timesave::summarize(&evs, AT, 0);
        assert_eq!(s.all.total(), timesave::Sum::default());
        assert_eq!((s.observed, s.bad), (1, 0));
    }
}
