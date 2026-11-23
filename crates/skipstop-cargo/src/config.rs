//! A project's own settings: `skipstop.json` at the repo's root. No file: the defaults.
//!
//! ```json
//! {
//!   "never_reuse": {"deploy-tests": "side_effect", "fmt-check": "writes_worktree"},
//!   "defense_period_days": 7,
//!   "keep_days": 30
//! }
//! ```
//!
//! - `never_reuse`: units that run every time whatever their key, each with the reason: it
//!   `writes_worktree`, `reads_outside` (the network, state outside the repo), has a
//!   `side_effect` (deploys, sends, pushes), or reads `time_random_home`. A key is the hash of
//!   the files a unit reads; none of these four is in a file.
//! - `defense_period_days` (1 to 30, default 7): how often every unit runs whatever its key.
//!   There is no "never": that round is what finds an input no key holds.
//! - `keep_days` (8 to 3650, default 30): how long a run's ledger file is kept. Not under 8:
//!   the weekly questions need a week of history.
//!
//! A key the file does not know, a value out of its range, a key twice: errors. A setting a
//! person wrote and the program did not understand must not run as the default instead.

use skipstop_audit::NeverReuse;
use std::collections::BTreeMap;

pub const FILE: &str = "skipstop.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub never_reuse: BTreeMap<String, NeverReuse>,
    pub defense_period_days: i64,
    pub keep_days: i64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            never_reuse: BTreeMap::new(),
            defense_period_days: crate::real::DEFENSE_PERIOD_DAYS,
            keep_days: crate::real::KEEP_DAYS,
        }
    }
}

fn days(v: &serde_json::Value, name: &str, min: i64, max: i64) -> Result<i64, String> {
    v.as_i64()
        .filter(|d| (min..=max).contains(d))
        .ok_or_else(|| format!("\"{name}\" is not a whole number from {min} to {max}"))
}

pub fn parse(text: &str) -> Result<Config, String> {
    let v = skipstop_ledger::strict::json(text)?;
    let o = v.as_object().ok_or("not an object")?;
    let mut c = Config::default();
    for (k, v) in o {
        match k.as_str() {
            "never_reuse" => {
                let m = v.as_object().ok_or("\"never_reuse\" is not an object")?;
                for (unit, why) in m {
                    let class = why
                        .as_str()
                        .and_then(NeverReuse::from_word)
                        // flaky is found from the ledger, not declared
                        .filter(|n| *n != NeverReuse::Flaky)
                        .ok_or_else(|| {
                            format!(
                                "never_reuse {unit:?}: {why} is not writes_worktree, reads_outside, \
                                 side_effect or time_random_home"
                            )
                        })?;
                    if unit.trim().is_empty() {
                        return Err("never_reuse has a unit with no name".into());
                    }
                    c.never_reuse.insert(unit.clone(), class);
                }
            }
            "defense_period_days" => c.defense_period_days = days(v, k, 1, 30)?,
            "keep_days" => c.keep_days = days(v, k, 8, 3650)?,
            other => return Err(format!("\"{other}\" is not a setting")),
        }
    }
    Ok(c)
}

/// The settings of the repo at `root`. No file there: the defaults. Anything else that is not
/// a file read whole and understood is an error: a link to nothing, a folder, a file that
/// may not be read. (Reading a link to nothing says "not found" as a missing file does, so
/// the name is looked at first, without following it.)
pub fn load(root: &std::path::Path) -> Result<Config, String> {
    let path = root.join(FILE);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(format!("{FILE}: {e}")),
        Ok(_) => {}
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{FILE}: {e}"))?;
    parse(&text).map_err(|e| format!("{FILE}: {e}"))
}

/// The units `never_reuse` names that are not units of this workspace: a typo there would
/// leave the unit it meant to be reused like any other.
pub fn strangers<'a>(c: &'a Config, units: &[String]) -> Vec<&'a str> {
    c.never_reuse
        .keys()
        .filter(|u| !units.contains(u))
        .map(String::as_str)
        .collect()
}

/// The word of a never-reuse class as a unit's reason to run.
pub fn why_word(n: NeverReuse) -> &'static str {
    match n {
        NeverReuse::WritesWorktree => "never_reuse:writes_worktree",
        NeverReuse::ReadsOutside => "never_reuse:reads_outside",
        NeverReuse::SideEffect => "never_reuse:side_effect",
        NeverReuse::TimeRandomHome => "never_reuse:time_random_home",
        NeverReuse::Flaky => "flaky",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_setting_is_the_default_and_a_setting_not_understood_is_an_error() {
        assert_eq!(parse("{}").unwrap(), Config::default());
        let c = parse(
            r#"{"never_reuse": {"deploy": "side_effect", "fmt": "writes_worktree"},
                "defense_period_days": 3, "keep_days": 90}"#,
        )
        .unwrap();
        assert_eq!(c.never_reuse["deploy"], NeverReuse::SideEffect);
        assert_eq!((c.defense_period_days, c.keep_days), (3, 90));
        for bad in [
            r#"{"never_resue": {}}"#,
            r#"{"never_reuse": {"deploy": "sometimes"}}"#,
            // flaky is read from the ledger; declaring it would hide what it means
            r#"{"never_reuse": {"deploy": "flaky"}}"#,
            r#"{"never_reuse": {"deploy": true}}"#,
            r#"{"never_reuse": ["deploy"]}"#,
            r#"{"never_reuse": {" ": "side_effect"}}"#,
            // no round at all, or one a year
            r#"{"defense_period_days": 0}"#,
            r#"{"defense_period_days": 31}"#,
            r#"{"defense_period_days": "7"}"#,
            r#"{"keep_days": 7}"#,
            r#"{"keep_days": 1.5}"#,
            // off then on in one file
            r#"{"keep_days": 30, "keep_days": 90}"#,
            r#"{"never_reuse": {"a": "side_effect", "a": "reads_outside"}}"#,
            "[]",
            "",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(parse(r#"{"never_resue": {}}"#)
            .unwrap_err()
            .contains("not a setting"));
    }

    #[test]
    fn only_a_missing_file_is_the_defaults() {
        let d = std::env::temp_dir().join(format!("skipstop-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(load(&d).unwrap(), Config::default());
        let f = d.join(FILE);
        std::fs::write(&f, r#"{"keep_days": 90}"#).unwrap();
        assert_eq!(load(&d).unwrap().keep_days, 90);
        std::fs::write(&f, r#"{"keep_days": 1}"#).unwrap();
        assert!(load(&d).unwrap_err().starts_with("skipstop.json: "));
        // a folder of that name
        std::fs::remove_file(&f).unwrap();
        std::fs::create_dir(&f).unwrap();
        assert!(load(&d).is_err());
        std::fs::remove_dir(&f).unwrap();
        // a link to nothing: reading it says "not found", as no file does
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(d.join("gone.json"), &f).unwrap();
            assert!(
                load(&d).is_err(),
                "a link to nothing is not \"no settings\""
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_never_reuse_name_that_is_no_unit_is_found() {
        let c = parse(r#"{"never_reuse": {"deploy": "side_effect", "dpeloy-two": "side_effect"}}"#)
            .unwrap();
        let units = vec!["deploy".to_string(), "deploy-two".to_string()];
        assert_eq!(strangers(&c, &units), vec!["dpeloy-two"]);
        assert!(strangers(&Config::default(), &units).is_empty());
        assert_eq!(why_word(NeverReuse::SideEffect), "never_reuse:side_effect");
    }
}
