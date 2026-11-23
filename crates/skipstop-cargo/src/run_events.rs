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
            of: Some((u.unit, w.from)),
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
            of: Some((u.unit, from)),
        };
        if let Ok(v) = timesave::saved(ctx, &saved, at_ms) {
            out.push(v);
        }
    }
    out
}

/// The `time_saved_wrong` events for a source that a real run has just disagreed with: one for
/// each real saving that stands on `unit` reused from `from` and was not taken back before.
/// Each takes back exactly what that saving claimed.
pub fn take_back(ctx: &Ctx, evs: &[Value], unit: &str, from: &str, at_ms: i64) -> Vec<Value> {
    timesave::standing_on(evs, unit, from)
        .into_iter()
        .filter_map(|(id, ms)| {
            let basis =
                format!("its source {from} was revoked: a real run of {unit} disagreed with it");
            timesave::wrong(ctx, &id, ms, &basis, at_ms).ok()
        })
        .collect()
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
    // said first, so the numbers are not read as time on the clock
    let mut out = format!(
        "time saved, as the skipped work's own run time added up (units run side by side, so it is \
         more than the clock would show); days on a UTC{} clock\n",
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
            "  {name:<10} {}  (skip {} · stop {})  estimated {}  taken back {}  savings {}  | on the clock {}{}\n",
            clock(t.confirmed_ms),
            clock(p.shadow.confirmed_ms),
            clock(p.precheck.confirmed_ms),
            clock(t.estimated_ms),
            clock(t.penalty_ms),
            t.count,
            // skip: baseline wall minus the run's wall; stop: what the stopped command would
            // have taken is a time on the clock already
            clock(p.clock_ms.saturating_add(p.precheck.confirmed_ms)),
            if p.clock_unknown > 0 {
                format!(" (+{} run(s) with no full run to compare with)", p.clock_unknown)
            } else {
                String::new()
            }
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
    let period = |p: &timesave::Period| {
        serde_json::json!({
            "total": sum(&p.total()), "skip": sum(&p.shadow), "stop": sum(&p.precheck),
            // on the clock: skip as baseline wall minus the run's wall, stop as what the
            // stopped command would have taken
            "clock_ms": p.clock_ms.saturating_add(p.precheck.confirmed_ms),
            "skip_clock_ms": p.clock_ms,
            "skip_clock_unknown_runs": p.clock_unknown,
        })
    };
    let recent: Vec<Value> = s
        .recent
        .iter()
        .map(|e| {
            serde_json::json!({
                "at": skipstop_ledger::envelope::rfc3339_ms(e.at_ms),
                "system": e.system.as_str(),
                "confirmed": e.confirmed,
                "saved_ms": e.saved_ms,
                "avoided": e.avoided,
                "basis": e.basis,
            })
        })
        .collect();
    serde_json::json!({
        "v": 1,
        // what total / skip / stop milliseconds are: not time on the clock (clock_ms is)
        "measure": "unit_time_sum",
        "utc_offset_min": offset_min,
        "today": period(&s.today), "week": period(&s.week), "month": period(&s.month),
        "year": period(&s.year), "all": period(&s.all),
        "since": s.since_ms.map(skipstop_ledger::envelope::rfc3339_ms),
        "observed": s.observed,
        "unread": s.bad,
        "recent": recent,
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
                // the saving names its unit and source too, so it can be found again
                ("time_saved", "reused"),
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
            of: None,
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
            text.starts_with("time saved, as the skipped work's own run time added up")
                && text
                    .lines()
                    .next()
                    .is_some_and(|l| l.ends_with("days on a UTC+08:00 clock")),
            "{text}"
        );
        // 1:02:05 confirmed; the estimate (0:01:00) and the prediction are not in it
        assert!(
            text.contains("  today      1:02:05  (skip 0:00:00 · stop 1:02:05)  estimated 0:01:00  taken back 0:00:00  savings 2  | on the clock 1:02:05\n"),
            "{text}"
        );
        assert!(text.contains("1 observe-mode prediction(s)"), "{text}");
        assert!(text.contains("1 more line(s) could not be read"), "{text}");
        let j = saved_json(&s, 480);
        assert_eq!(j["today"]["total"]["confirmed_ms"], 3_725_000);
        assert_eq!(j["measure"], "unit_time_sum");
        // on the clock: no run reused anything here, and the stop's hour is clock time
        assert_eq!(j["today"]["clock_ms"], 3_725_000);
        assert_eq!(j["today"]["skip_clock_ms"], 0);
        assert!(text.contains("| on the clock 1:02:05\n"), "{text}");
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
    fn a_revoked_source_takes_back_every_saving_that_stood_on_it_once() {
        let reused = |unit, from| Ran {
            reused: true,
            would: Some(Would {
                from,
                source_ms: Some(900),
                agreed: true,
            }),
            ..ran(unit, None)
        };
        // two runs reused b from tr-1, one reused b from tr-2, one reused a from tr-1
        let mut evs = real(
            &ctx(),
            "m",
            "h",
            &[reused("b", "repo/tr-1"), reused("a", "repo/tr-1")],
            AT,
        );
        evs.extend(real(&ctx(), "m", "h", &[reused("b", "repo/tr-1")], AT + 1));
        evs.extend(real(&ctx(), "m", "h", &[reused("b", "repo/tr-2")], AT + 2));
        // an observe-mode prediction about the same unit and source saved nothing to take back
        let seen = ran(
            "b",
            Some(Would {
                from: "repo/tr-1",
                source_ms: Some(900),
                agreed: true,
            }),
        );
        evs.extend(observe(&ctx(), "m", "h", &[seen], AT + 3));
        assert_eq!(
            timesave::summarize(&evs, AT + 9, 0).all.shadow.confirmed_ms,
            3_600
        );

        let back = take_back(&ctx(), &evs, "b", "repo/tr-1", AT + 4);
        assert_eq!(back.len(), 2, "{back:?}");
        assert!(back
            .iter()
            .all(|w| w["kind"] == "time_saved_wrong" && w["penalty_ms"] == 900));
        assert!(back[0]["basis"].as_str().unwrap().contains("repo/tr-1"));
        evs.extend(back);
        let s = timesave::summarize(&evs, AT + 9, 0);
        // b from tr-1 twice is gone; a from tr-1 and b from tr-2 stand
        assert_eq!(
            (s.all.shadow.confirmed_ms, s.all.shadow.penalty_ms),
            (1_800, 1_800)
        );
        assert_eq!((s.all.shadow.count, s.bad), (2, 0));
        // asked again: nothing is taken back twice
        assert!(take_back(&ctx(), &evs, "b", "repo/tr-1", AT + 5).is_empty());
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
        assert!(saved_json(&s, 0)["recent"].as_array().unwrap().is_empty());
    }

    #[test]
    fn saved_json_lists_newest_real_savings_only() {
        let mut evs = Vec::new();
        // observe event: excluded from recent
        evs.extend(observe(
            &ctx(),
            "linux-x86_64",
            "",
            &[ran(
                "obs",
                Some(Would {
                    from: "repo/tr-0",
                    source_ms: Some(9_999),
                    agreed: true,
                }),
            )],
            AT + 50_000,
        ));
        // revoked real event: excluded from recent
        let mut revoked_u = ran(
            "rev",
            Some(Would {
                from: "repo/tr-rev",
                source_ms: Some(8_888),
                agreed: true,
            }),
        );
        revoked_u.reused = true;
        evs.extend(real(
            &ctx(),
            "linux-x86_64",
            "abc",
            &[revoked_u],
            AT + 40_000,
        ));
        evs.extend(take_back(&ctx(), &evs, "rev", "repo/tr-rev", AT + 41_000));
        // precheck (stop) estimated -> confirmed
        let is_real = true;
        let stop_est = timesave::saved(
            &ctx(),
            &timesave::Saved {
                system: timesave::System::Precheck,
                rule: "check",
                verdict: "reject",
                avoided: "cargo test",
                saved_ms: 4_000,
                confirmed: false,
                basis: "guess",
                real: is_real,
                head: Some("abc"),
                of: Some(("check1", "stop")),
            },
            AT + 26_000,
        )
        .unwrap();
        let stop_id = stop_est["id"].as_str().unwrap().to_string();
        evs.push(stop_est);
        evs.push(timesave::confirm(&ctx(), &stop_id, AT + 27_000).unwrap());

        for i in 0..25 {
            let unit = format!("u{i}");
            let mut u = ran(
                &unit,
                Some(Would {
                    from: "repo/tr-1",
                    source_ms: Some(1_000 + i as u64),
                    agreed: true,
                }),
            );
            u.reused = true;
            evs.extend(real(&ctx(), "linux-x86_64", "abc", &[u], AT + i * 1_000));
        }
        let s = timesave::summarize(&evs, AT + 60_000, 0);
        let j = saved_json(&s, 0);
        let rec = j["recent"].as_array().unwrap();
        assert_eq!(rec.len(), timesave::RECENT_CAP);
        assert_eq!(rec[0]["system"], "precheck");
        assert_eq!(rec[0]["confirmed"], true);
        assert_eq!(rec[0]["saved_ms"], 4_000);
        assert_eq!(rec[0]["avoided"], "cargo test");
        assert_eq!(rec[0]["basis"], "guess");
        assert!(rec[0]["at"].as_str().is_some_and(|s| !s.is_empty()));
        assert_eq!(rec[1]["system"], "shadow");
        assert_eq!(rec[1]["saved_ms"], 1_024);
        assert_eq!(rec[19]["saved_ms"], 1_006);
    }
}
