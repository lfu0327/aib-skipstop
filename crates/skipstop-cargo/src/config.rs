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
//! - `units`: units that are not cargo packages: any command, with the files it reads said
//!   by a person.
//!
//!   ```json
//!   "units": {
//!     "web": {
//!       "run": ["npm", "test"],
//!       "run_windows": ["npm.cmd", "test"],
//!       "inputs": ["web/", "package.json", "package-lock.json"],
//!       "env": ["NODE_ENV"],
//!       "tools": [["node", "--version"]],
//!       "timeout_s": 900
//!     }
//!   }
//!   ```
//!
//!   `run`: the command, as a list (no shell). `run_windows`: the list used on Windows instead,
//!   when the program has another name there. `inputs`: files, or folders ending in `/`, of
//!   the repo; the key is their hash, so **a file the command reads and this list does not
//!   name is a change that will not be seen**. `env`: variables whose value changes the result.
//!   `tools`: commands whose whole output is part of the key (a version). `timeout_s`: 1 to
//!   86400, default 1800. The command passes with exit code 0; any other code is a failure.
//!
//! A key the file does not know, a value out of its range, a key twice: errors. A setting a
//! person wrote and the program did not understand must not run as the default instead.

use skipstop_audit::NeverReuse;
use std::collections::BTreeMap;

pub const FILE: &str = "skipstop.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandUnit {
    /// the command on this system (`run_windows` on Windows when it is given)
    pub run: Vec<String>,
    pub inputs: Vec<String>,
    pub env: Vec<String>,
    pub tools: Vec<Vec<String>>,
    pub timeout_s: u64,
}

pub const UNIT_TIMEOUT_DEFAULT_S: u64 = 1800;
pub const UNIT_TIMEOUT_MAX_S: u64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub units: BTreeMap<String, CommandUnit>,
    pub never_reuse: BTreeMap<String, NeverReuse>,
    pub defense_period_days: i64,
    pub keep_days: i64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            units: BTreeMap::new(),
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

fn argv(v: &serde_json::Value, what: &str) -> Result<Vec<String>, String> {
    let list: Option<Vec<String>> = v.as_array().and_then(|a| {
        a.iter()
            .map(|x| x.as_str().map(str::to_owned))
            .collect::<Option<Vec<String>>>()
    });
    list.filter(|l| l.first().is_some_and(|p| !p.trim().is_empty()))
        .ok_or_else(|| format!("{what} is not a list of words that starts with a program"))
}

/// A name a unit, a variable or a file of the ledger can have: letters, digits, `.`, `_`, `-`.
fn plain_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(['.', '-'])
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// An input: a path of the repo, `/` between its parts, not leaving the repo.
fn input_ok(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && p.trim_end_matches('/')
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn unit(name: &str, v: &serde_json::Value, windows: bool) -> Result<CommandUnit, String> {
    let o = v
        .as_object()
        .ok_or_else(|| format!("unit {name:?} is not an object"))?;
    let mut run = None;
    let mut run_windows = None;
    let mut u = CommandUnit {
        run: Vec::new(),
        inputs: Vec::new(),
        env: Vec::new(),
        tools: Vec::new(),
        timeout_s: UNIT_TIMEOUT_DEFAULT_S,
    };
    let words = |v: &serde_json::Value, key: &str, ok: &dyn Fn(&str) -> bool| {
        v.as_array()
            .and_then(|a| {
                a.iter()
                    .map(|x| x.as_str().filter(|s| ok(s)).map(str::to_owned))
                    .collect::<Option<Vec<String>>>()
            })
            .ok_or_else(|| format!("unit {name:?}: \"{key}\" has a word that cannot be one"))
    };
    for (k, v) in o {
        match k.as_str() {
            "run" => run = Some(argv(v, &format!("unit {name:?}: \"run\""))?),
            "run_windows" => {
                run_windows = Some(argv(v, &format!("unit {name:?}: \"run_windows\""))?)
            }
            "inputs" => u.inputs = words(v, k, &input_ok)?,
            "env" => u.env = words(v, k, &|s| plain_name(s) && !s.contains(['.', '-']))?,
            "tools" => {
                let list = v
                    .as_array()
                    .ok_or_else(|| format!("unit {name:?}: \"tools\" is not a list"))?;
                for t in list {
                    u.tools.push(argv(t, &format!("unit {name:?}: a tool"))?);
                }
            }
            "timeout_s" => {
                u.timeout_s = v
                    .as_u64()
                    .filter(|t| (1..=UNIT_TIMEOUT_MAX_S).contains(t))
                    .ok_or_else(|| {
                        format!("unit {name:?}: \"timeout_s\" is not a whole number from 1 to {UNIT_TIMEOUT_MAX_S}")
                    })?
            }
            other => return Err(format!("unit {name:?}: \"{other}\" is not a setting")),
        }
    }
    let run = run.ok_or_else(|| format!("unit {name:?} has no \"run\""))?;
    u.run = match run_windows {
        Some(w) if windows => w,
        _ => run,
    };
    // no inputs: every run would have one key, and the first green would be reused for ever
    if u.inputs.is_empty() {
        return Err(format!(
            "unit {name:?} has no \"inputs\": name the files its result depends on"
        ));
    }
    Ok(u)
}

pub fn parse(text: &str) -> Result<Config, String> {
    parse_for(text, cfg!(windows))
}

/// [`parse`] for a system said by the caller (`windows`: `run_windows` is the command).
pub fn parse_for(text: &str, windows: bool) -> Result<Config, String> {
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
            "units" => {
                let m = v.as_object().ok_or("\"units\" is not an object")?;
                for (name, v) in m {
                    if !plain_name(name) {
                        return Err(format!(
                            "unit {name:?}: a name is letters, digits, '.', '_' and '-'"
                        ));
                    }
                    c.units.insert(name.clone(), unit(name, v, windows)?);
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
    fn a_command_unit_says_its_command_and_its_inputs_or_it_is_an_error() {
        let text = r#"{"units": {"web": {
            "run": ["npm", "test"], "run_windows": ["npm.cmd", "test"],
            "inputs": ["web/", "package.json"], "env": ["NODE_ENV"],
            "tools": [["node", "--version"]], "timeout_s": 900}}}"#;
        let u = &parse_for(text, false).unwrap().units["web"];
        assert_eq!(u.run, vec!["npm", "test"]);
        assert_eq!(u.inputs, vec!["web/", "package.json"]);
        assert_eq!((u.env.len(), u.tools.len(), u.timeout_s), (1, 1, 900));
        // on Windows the other list is the command
        assert_eq!(
            parse_for(text, true).unwrap().units["web"].run,
            vec!["npm.cmd", "test"]
        );
        // without run_windows, run is the command there too; the time limit has a default
        let plain = r#"{"units": {"web": {"run": ["make", "check"], "inputs": ["src/"]}}}"#;
        let u = &parse_for(plain, true).unwrap().units["web"];
        assert_eq!(
            (u.run.clone(), u.timeout_s),
            (
                vec!["make".to_string(), "check".into()],
                UNIT_TIMEOUT_DEFAULT_S
            )
        );
        let with = |body: &str| format!(r#"{{"units": {{"web": {body}}}}}"#);
        for bad in [
            // no inputs: one key for ever
            r#"{"run": ["make"]}"#,
            r#"{"run": ["make"], "inputs": []}"#,
            // no command, a command that is a line for a shell, an empty program
            r#"{"inputs": ["src/"]}"#,
            r#"{"run": "make check", "inputs": ["src/"]}"#,
            r#"{"run": [], "inputs": ["src/"]}"#,
            r#"{"run": [" "], "inputs": ["src/"]}"#,
            r#"{"run": ["make", 1], "inputs": ["src/"]}"#,
            r#"{"run": ["make"], "run_windows": [], "inputs": ["src/"]}"#,
            // inputs that leave the repo or are no path of it
            r#"{"run": ["make"], "inputs": ["../other/"]}"#,
            r#"{"run": ["make"], "inputs": ["/etc/hosts"]}"#,
            r#"{"run": ["make"], "inputs": ["src//x"]}"#,
            r#"{"run": ["make"], "inputs": ["C:\\x"]}"#,
            r#"{"run": ["make"], "inputs": [""]}"#,
            r#"{"run": ["make"], "inputs": ["src/"], "env": ["A B"]}"#,
            r#"{"run": ["make"], "inputs": ["src/"], "tools": [[]]}"#,
            r#"{"run": ["make"], "inputs": ["src/"], "timeout_s": 0}"#,
            r#"{"run": ["make"], "inputs": ["src/"], "timeout_s": 86401}"#,
            r#"{"run": ["make"], "inputs": ["src/"], "inptus": ["x"]}"#,
            r#"["make"]"#,
        ] {
            assert!(parse_for(&with(bad), false).is_err(), "{bad}");
        }
        // a name that could not be a file of the ledger
        for name in ["", "a b", "../x", "-p", ".hidden", "a/b"] {
            let text =
                format!(r#"{{"units": {{{name:?}: {{"run": ["make"], "inputs": ["src/"]}}}}}}"#);
            assert!(parse_for(&text, false).is_err(), "{name}");
        }
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
