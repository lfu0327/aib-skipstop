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
