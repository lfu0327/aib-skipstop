//! Real mode: which units of this run are not run. The rule, in the order it is asked:
//!
//! 1. `--fresh` ⇒ every unit runs.
//! 2. The ledger could not be read whole ⇒ every unit runs: the part that was not read may
//!    hold a newer failure or a revocation, and an older green run must not stand in for it.
//! 3. The unit's defense round is due ⇒ the unit runs.
//! 4. A unit with no source (no key, no earlier green run of that key on a clean tree, or a
//!    revoked one) runs. What cannot be measured is never reused.
//! 5. A unit whose same key has both passed and failed before (flaky) runs: its green proves
//!    nothing about the next run.
//! 6. A unit with a source is asked for an audit ([`skipstop_audit::choose`]: the random
//!    draw, then the forced reasons: ten reuses in a row, no real run for seven days, …).
//!    Chosen ⇒ it runs for real and the run is compared with the source.
//! 7. Otherwise it is reused: not run, and its line names the source.
//!
//! The defense round ([`skipstop_audit::defense_due`]) is the one check an audit cannot
//! replace: an input the unit never listed changes no key, so only running everything finds
//! it. It is kept per unit: a unit's round is done only by a finished run that was a round and
//! really ran that unit, so a run of one package is no round for the others. It is due the
//! first time a unit is seen in real mode and then once a period; the run that finds it due
//! is the round.
//!
//! Everything here is worked out from the ledger and the clock the caller gives; nothing is
//! run and nothing is written.

use skipstop_audit::{choose, defense_due, AuditKind, Choice, Defense, History, KindCounts};
use skipstop_core::ledger::{AuditCompare, How, Stream};

/// The cell name on a real-mode run's plan.
pub const CELL_REAL: &str = "aib-skipstop";
/// The cell name on a plan whose run was the defense round.
pub const CELL_DEFENSE: &str = "aib-skipstop-defense";
/// The cell name on an observe-mode run's plan.
pub const CELL_OBSERVE: &str = "aib-skipstop-observe";
/// Days between defense rounds on a user's machine.
pub const DEFENSE_PERIOD_DAYS: i64 = 7;
/// Every unit here is a package's tests.
pub const KIND: &str = "test";

/// What happens to one unit in this run.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Runs for real. `why` is a fixed word: `fresh`, `ledger_unread`, `defense_round`,
    /// `no_source`, `flaky`.
    Run { why: &'static str },
    /// Has a source, runs anyway, and is compared with it.
    Audit(Choice),
    /// Not run: the source's result stands.
    Reuse,
}

impl Decision {
    pub fn runs(&self) -> bool {
        !matches!(self, Decision::Reuse)
    }
    /// The word for the `--why` line and the JSON.
    pub fn word(&self) -> String {
        match self {
            Decision::Run { why } => format!("run:{why}"),
            Decision::Audit(c) => format!("audit:{}", c.reason_codes.join("+")),
            Decision::Reuse => "reuse".into(),
        }
    }
}

/// Every line of `unit` in the ledger, oldest first.
fn lines<'a>(streams: &'a [Stream], unit: &str) -> Vec<(i64, &'a skipstop_core::ledger::Unit)> {
    let mut v: Vec<(i64, &skipstop_core::ledger::Unit)> = streams
        .iter()
        .flat_map(|s| s.units.iter())
        .filter(|(_, u)| u.unit == unit)
        .map(|(at, u)| (*at, u))
        .collect();
    v.sort_by_key(|(at, _)| *at);
    v
}

/// What the ledger says about one unit, for the audit's choice. Times in seconds.
pub fn history(streams: &[Stream], unit: &str) -> History {
    let mut h = History {
        kind: KIND.into(),
        ..History::default()
    };
    for (at, u) in lines(streams, unit) {
        match u.how {
            How::Ran => {
                h.reuse_streak = 0;
                h.last_real_run_at = Some(at / 1000);
                if let Some(a) = &u.audit {
                    h.ever_audited = true;
                    h.last_audit_incomparable = a.compare == AuditCompare::Incomparable;
                }
            }
            How::Reused => h.reuse_streak += 1,
            How::Skipped | How::NotRun => {}
        }
    }
    h
}

/// When anything was first reused (seconds): the audit rate is higher for the two weeks after.
pub fn first_reuse_at(streams: &[Stream]) -> Option<i64> {
    streams
        .iter()
        .flat_map(|s| s.units.iter())
        .filter(|(_, u)| u.how == How::Reused)
        .map(|(at, _)| at / 1000)
        .min()
}

/// When `unit` last ran for real in a defense round that ran to its end (seconds). A round
/// the unit was not part of, or was only reused in, is not its round.
pub fn last_defense_at(streams: &[Stream], unit: &str) -> Option<i64> {
    streams
        .iter()
        .filter(|s| s.complete() && s.plan.cells.iter().any(|c| c == CELL_DEFENSE))
        .filter(|s| {
            s.units
                .iter()
                .any(|(_, u)| u.unit == unit && u.how == How::Ran)
        })
        .map(|s| s.at_ms / 1000)
        .max()
}

/// Every (unit, source run) the ledger says was revoked: a line that ran, was compared with the
/// source it would have been reused from, and disagreed. Sorted, each once. What earlier runs
/// saved on such a source is taken back; asking on every run means a take-back that could
/// not be written once is written by a later run.
pub fn revoked_sources(streams: &[Stream]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = streams
        .iter()
        .flat_map(|s| s.units.iter())
        .filter(|(_, u)| u.audit.as_ref().is_some_and(|a| a.revoke))
        .filter_map(|(_, u)| {
            let from = &u.shadow.as_ref()?.would_reuse.as_ref()?.from;
            Some((u.unit.clone(), from.clone()))
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

/// (audited, candidates) of the one kind there is: a candidate is a line that ran with a
/// source to compare against or was reused.
pub fn kind_counts(streams: &[Stream]) -> KindCounts {
    let (mut audited, mut candidates) = (0u64, 0u64);
    for (_, u) in streams.iter().flat_map(|s| s.units.iter()) {
        match u.how {
            How::Reused => candidates += 1,
            How::Ran if u.audit.is_some() => {
                audited += 1;
                candidates += 1;
            }
            _ => {}
        }
    }
    let mut k = KindCounts::new();
    k.insert(KIND.into(), (audited, candidates));
    k
}

/// Is `unit`'s defense round due?
pub fn defense(streams: &[Stream], unit: &str, now_s: i64) -> Defense {
    defense_due(
        last_defense_at(streams, unit),
        DEFENSE_PERIOD_DAYS,
        false,
        now_s,
    )
}

/// The same key on the same kind of machine has both passed and failed in this unit's real
/// runs ([`skipstop_audit::flaky`]). A run that could not be read as pass or fail says nothing.
pub fn flaky_unit(streams: &[Stream], unit: &str) -> bool {
    let runs: Vec<skipstop_audit::RealRun> = streams
        .iter()
        .flat_map(|s| s.units.iter().map(move |(_, u)| (s, u)))
        .filter(|(_, u)| u.unit == unit && u.how == How::Ran)
        .filter_map(|(s, u)| {
            Some(skipstop_audit::RealRun {
                key: u.key.clone()?,
                env_id: s.plan.host.clone(),
                outcome: match u.result {
                    skipstop_core::ledger::Outcome::Pass => skipstop_audit::Outcome::Pass,
                    skipstop_core::ledger::Outcome::Fail => skipstop_audit::Outcome::Fail,
                    _ => return None,
                },
                // which runs were reruns after a red is not kept: every run counts, which
                // can only call more units flaky, never fewer
                retry_after_red: false,
            })
        })
        .collect();
    skipstop_audit::flaky(&runs)
}

/// What one run needs to decide for its units.
pub struct Asked<'a> {
    pub streams: &'a [Stream],
    pub fresh: bool,
    /// Every stream of the ledger was read (no problem reported).
    pub ledger_ok: bool,
    /// A fresh number for this run's random draws.
    pub seed: u64,
    pub now_s: i64,
}

/// The decision for one unit; `has_source` = [`skipstop_core::ledger::local_source`] found one.
pub fn decide(a: &Asked<'_>, unit: &str, has_source: bool) -> Decision {
    if a.fresh {
        return Decision::Run { why: "fresh" };
    }
    if !a.ledger_ok {
        return Decision::Run {
            why: "ledger_unread",
        };
    }
    if matches!(defense(a.streams, unit, a.now_s), Defense::Due { .. }) {
        return Decision::Run {
            why: "defense_round",
        };
    }
    if !has_source {
        return Decision::Run { why: "no_source" };
    }
    if flaky_unit(a.streams, unit) {
        return Decision::Run { why: "flaky" };
    }
    let c = choose(
        &history(a.streams, unit),
        &kind_counts(a.streams),
        false,
        a.seed,
        unit,
        first_reuse_at(a.streams),
        a.now_s,
    );
    if c.audit_kind == AuditKind::None {
        Decision::Reuse
    } else {
        Decision::Audit(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skipstop_core::ledger::{Audit, Outcome, Plan, Unit};

    const DAY_MS: i64 = 86_400_000;
    const NOW_S: i64 = 1_795_399_200;

    fn now_ms() -> i64 {
        NOW_S * 1000
    }

    fn unit(name: &str, how: How, audit: Option<AuditCompare>) -> Unit {
        Unit {
            unit: name.into(),
            krate: Some(name.into()),
            how,
            result: Outcome::Pass,
            reuse: None,
            why_not: None,
            why_detail: None,
            skip: None,
            dur_ms: Some(1_000),
            seat: None,
            key: Some("k".into()),
            shadow: None,
            tests_ran: Some(3),
            failed: vec![],
            output_digest: None,
            audit: audit.map(|compare| Audit {
                compare,
                revoke: compare == AuditCompare::Mismatch,
            }),
        }
    }

    fn stream(at_ms: i64, cell: &str, ended: Option<bool>, units: Vec<Unit>) -> Stream {
        Stream {
            invocation_id: format!("tr-{at_ms}"),
            seat: "c5d88".into(),
            plan: Plan {
                head: "h".into(),
                tree: "t".into(),
                host: "macos-aarch64".into(),
                run_os: "macos".into(),
                cells: vec![cell.into()],
                units: units.iter().map(|u| u.unit.clone()).collect(),
            },
            at_ms,
            units: units.into_iter().map(|u| (at_ms + 1, u)).collect(),
            ended,
        }
    }

    /// A finished defense round that ran `units` for real, `ago_ms` ago.
    fn round(ago_ms: i64, units: &[&str]) -> Stream {
        let ran = units.iter().map(|u| unit(u, How::Ran, None)).collect();
        stream(now_ms() - ago_ms, CELL_DEFENSE, Some(true), ran)
    }

    /// A finished real-mode run that was no round, `ago_ms` ago.
    fn plain(ago_ms: i64, units: Vec<Unit>) -> Stream {
        stream(now_ms() - ago_ms, CELL_REAL, Some(true), units)
    }

    /// A seed whose draw for `unit` is at or above the early rate: no random audit.
    fn quiet_seed(unit: &str) -> u64 {
        (0u64..)
            .find(|s| skipstop_audit::draw(*s, unit) >= skipstop_audit::EARLY_RATE)
            .unwrap()
    }

    /// A seed whose draw for `unit` is under the later rate: a random audit at either rate.
    fn loud_seed(unit: &str) -> u64 {
        (0u64..)
            .find(|s| skipstop_audit::draw(*s, unit) < skipstop_audit::LATER_RATE)
            .unwrap()
    }

    fn ask(streams: &[Stream], fresh: bool, seed: u64) -> Asked<'_> {
        Asked {
            streams,
            fresh,
            ledger_ok: true,
            seed,
            now_s: NOW_S,
        }
    }

    fn run(why: &'static str) -> Decision {
        Decision::Run { why }
    }

    #[test]
    fn fresh_the_defense_round_and_no_source_each_run_the_unit_whatever_else_holds() {
        let done = [round(DAY_MS, &["a"])];
        let q = quiet_seed("a");
        assert_eq!(decide(&ask(&done, true, q), "a", true), run("fresh"));
        // never in a round yet: its round is due, source or not
        assert_eq!(decide(&ask(&[], false, q), "a", true), run("defense_round"));
        assert_eq!(decide(&ask(&done, false, q), "a", false), run("no_source"));
        // fresh is said first when the others hold too
        assert_eq!(decide(&ask(&[], true, q), "a", false), run("fresh"));
        // none of the three: its round is done, a recent real run, not drawn ⇒ reused
        assert_eq!(decide(&ask(&done, false, q), "a", true), Decision::Reuse);
        assert!(!Decision::Reuse.runs() && run("fresh").runs());
    }

    #[test]
    fn a_unit_with_a_source_is_still_run_when_the_draw_or_a_forced_reason_picks_it() {
        let recent = [round(DAY_MS, &["a"])];
        // the random draw
        let d = decide(&ask(&recent, false, loud_seed("a")), "a", true);
        assert!(
            matches!(&d, Decision::Audit(c) if c.audit_kind == AuditKind::Random),
            "{d:?}"
        );
        assert!(
            d.runs() && d.word().starts_with("audit:run_audit_random"),
            "{}",
            d.word()
        );
        // ten reuses in a row since the last real run
        let mut many = vec![round(DAY_MS, &["a"])];
        for i in 0..10 {
            many.push(plain(
                DAY_MS - 1000 * (i + 1),
                vec![unit("a", How::Reused, None)],
            ));
        }
        assert_eq!(history(&many, "a").reuse_streak, 10);
        let d = decide(&ask(&many, false, quiet_seed("a")), "a", true);
        assert!(
            matches!(&d, Decision::Audit(c) if c.audit_kind == AuditKind::Forced
                && c.reason_codes.contains(&"forced:streak10".to_string())),
            "{d:?}"
        );
        // nine is not ten
        many.pop();
        assert_eq!(
            decide(&ask(&many, false, quiet_seed("a")), "a", true),
            Decision::Reuse
        );
        // its last audit did not finish: asked again
        let cut = [
            round(2 * DAY_MS, &["a"]),
            plain(
                DAY_MS,
                vec![unit("a", How::Ran, Some(AuditCompare::Incomparable))],
            ),
        ];
        let d = decide(&ask(&cut, false, quiet_seed("a")), "a", true);
        assert!(
            matches!(&d, Decision::Audit(c) if c.reason_codes.contains(&"forced:incomparable".to_string())),
            "{d:?}"
        );
    }

    #[test]
    fn the_history_is_read_from_the_units_own_lines_in_time_order() {
        let st = [
            // given newest first: the order of the files is not the order of time
            plain(
                DAY_MS,
                vec![unit("a", How::Reused, None), unit("b", How::Reused, None)],
            ),
            plain(
                2 * DAY_MS,
                vec![unit("a", How::Ran, Some(AuditCompare::Incomparable))],
            ),
            plain(
                3 * DAY_MS,
                vec![unit("a", How::Reused, None), unit("a", How::Reused, None)],
            ),
        ];
        let h = history(&st, "a");
        // the two reuses before the real run do not count; the one after does
        assert_eq!(h.reuse_streak, 1);
        assert_eq!(h.last_real_run_at, Some((now_ms() - 2 * DAY_MS + 1) / 1000));
        assert!(h.ever_audited && h.last_audit_incomparable);
        assert_eq!(h.kind, KIND);
        // another unit's lines are not this unit's
        let b = history(&st, "b");
        assert_eq!(
            (b.reuse_streak, b.last_real_run_at, b.ever_audited),
            (1, None, false)
        );
        assert_eq!(
            first_reuse_at(&st),
            Some((now_ms() - 3 * DAY_MS + 1) / 1000)
        );
        assert_eq!(first_reuse_at(&st[1..2]), None);
        // 1 audited line; candidates = it and the four reused lines
        assert_eq!(kind_counts(&st)[KIND], (1, 5));
    }

    #[test]
    fn the_defense_round_is_due_first_and_then_once_a_period_counted_from_a_finished_round() {
        assert!(matches!(
            defense(&[], "a", NOW_S),
            Defense::Due { why: "first", .. }
        ));
        // real-mode runs that were not the round do not count as one
        let no_round = [plain(DAY_MS, vec![unit("a", How::Ran, None)])];
        assert!(matches!(
            defense(&no_round, "a", NOW_S),
            Defense::Due { why: "first", .. }
        ));
        // nor does a round that was killed
        let killed = [stream(
            now_ms() - DAY_MS,
            CELL_DEFENSE,
            None,
            vec![unit("a", How::Ran, None)],
        )];
        assert_eq!(last_defense_at(&killed, "a"), None);
        let done = [round(DAY_MS, &["a"])];
        assert_eq!(defense(&done, "a", NOW_S), Defense::NotDue);
        let old = [round(8 * DAY_MS, &["a"])];
        assert!(matches!(
            defense(&old, "a", NOW_S),
            Defense::Due { why: "overdue", .. }
        ));
        // due again: the unit runs before any audit is asked
        assert_eq!(
            decide(&ask(&old, false, quiet_seed("a")), "a", true),
            run("defense_round")
        );
        // the newest finished round is the one that counts
        let both = [old[0].clone(), done[0].clone()];
        assert_eq!(defense(&both, "a", NOW_S), Defense::NotDue);
    }

    #[test]
    fn a_ledger_not_read_whole_runs_everything_and_only_fresh_is_said_before_it() {
        // the unread part may hold a newer failure or a revocation
        let done = [round(DAY_MS, &["a"])];
        let q = quiet_seed("a");
        let unread = |fresh| Asked {
            ledger_ok: false,
            ..ask(&done, fresh, q)
        };
        assert_eq!(decide(&unread(false), "a", true), run("ledger_unread"));
        assert_eq!(decide(&unread(false), "a", false), run("ledger_unread"));
        assert_eq!(decide(&unread(true), "a", true), run("fresh"));
        // read whole, the same unit is reused
        assert_eq!(decide(&ask(&done, false, q), "a", true), Decision::Reuse);
    }

    #[test]
    fn a_unit_whose_same_key_passed_and_failed_is_never_reused() {
        let mut red = unit("a", How::Ran, None);
        red.result = Outcome::Fail;
        let mut other_key = red.clone();
        other_key.key = Some("k2".into());
        let mut unknown = unit("a", How::Ran, None);
        unknown.result = Outcome::Invalid;
        // green and red on the same key
        let flaky = [round(2 * DAY_MS, &["a"]), plain(DAY_MS, vec![red.clone()])];
        assert!(flaky_unit(&flaky, "a"));
        assert_eq!(
            decide(&ask(&flaky, false, quiet_seed("a")), "a", true),
            run("flaky")
        );
        // red on another key (the code changed and broke: not flaky), or a run that was
        // neither pass nor fail
        let changed = [
            round(2 * DAY_MS, &["a"]),
            plain(DAY_MS, vec![other_key, unknown]),
        ];
        assert!(!flaky_unit(&changed, "a"));
        assert_eq!(
            decide(&ask(&changed, false, quiet_seed("a")), "a", true),
            Decision::Reuse
        );
        // another unit's red is not this unit's
        let mut b_red = red.clone();
        b_red.unit = "b".into();
        let other = [round(2 * DAY_MS, &["a"]), plain(DAY_MS, vec![b_red])];
        assert!(!flaky_unit(&other, "a") && !flaky_unit(&other, "b"));
        // the same key on another kind of machine is another environment
        let mut elsewhere = plain(DAY_MS, vec![red]);
        elsewhere.plan.host = "linux-x86_64".into();
        assert!(!flaky_unit(&[round(2 * DAY_MS, &["a"]), elsewhere], "a"));
    }

    #[test]
    fn the_revoked_sources_are_read_back_from_the_ledger_each_once() {
        use skipstop_core::ledger::{Origin, Reuse, Shadow};
        let would = |from: &str| {
            Some(Shadow {
                would_reuse: Some(Reuse {
                    origin: Origin::Local,
                    from: from.into(),
                    key: "k".into(),
                    tree: "t".into(),
                    at: "2026-11-22T00:00:00.000Z".into(),
                }),
                same: Some(false),
            })
        };
        let mut bad = unit("b", How::Ran, Some(AuditCompare::Mismatch));
        bad.shadow = would("repo/tr-1");
        // compared and the same: not revoked
        let mut fine = unit("a", How::Ran, Some(AuditCompare::Same));
        fine.shadow = would("repo/tr-1");
        // revoked, but the line does not say from which run: nothing to take back by
        let lost = unit("c", How::Ran, Some(AuditCompare::Mismatch));
        let st = [
            plain(2 * DAY_MS, vec![bad.clone(), fine, lost]),
            plain(DAY_MS, vec![bad]),
        ];
        assert_eq!(
            revoked_sources(&st),
            vec![("b".to_string(), "repo/tr-1".to_string())]
        );
        assert!(revoked_sources(&[]).is_empty());
    }

    #[test]
    fn a_round_of_one_package_is_no_round_for_the_others() {
        // `--mode real -p a` yesterday: a's round is done, b was never in one
        let only_a = [round(DAY_MS, &["a"])];
        assert_eq!(defense(&only_a, "a", NOW_S), Defense::NotDue);
        assert!(matches!(
            defense(&only_a, "b", NOW_S),
            Defense::Due { why: "first", .. }
        ));
        assert_eq!(
            decide(&ask(&only_a, false, quiet_seed("b")), "b", true),
            run("defense_round")
        );
        // a round b was listed in but only reused in (a was the one due) did not run b
        let mixed = [stream(
            now_ms() - DAY_MS,
            CELL_DEFENSE,
            Some(true),
            vec![unit("a", How::Ran, None), unit("b", How::Reused, None)],
        )];
        assert_eq!(
            last_defense_at(&mixed, "a"),
            Some((now_ms() - DAY_MS) / 1000)
        );
        assert_eq!(last_defense_at(&mixed, "b"), None);
    }
}
