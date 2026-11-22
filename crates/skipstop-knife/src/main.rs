//! `skipstop-knife`: run the knives of `knives/*.json`.
//!
//! ```text
//! skipstop-knife check [FILE...]             every anchor is in its file exactly once (no test run)
//! skipstop-knife run [--only ID]... [FILE...]  each knife: in, test, out; all must bite
//! ```
//! No FILE: every `knives/*.json`. Run from the repo root.
//! Exit: 0 every knife fine, 1 a knife misses or is invalid, 2 a bad call or a file not read.

use skipstop_knife::{count, line, parse, recover, run_one, Knife, Verdict};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str =
    "usage: skipstop-knife check [FILE...]\n       skipstop-knife run [--only ID]... [FILE...]";

fn files(given: &[String]) -> Result<Vec<PathBuf>, String> {
    if !given.is_empty() {
        return Ok(given.iter().map(PathBuf::from).collect());
    }
    let rd = std::fs::read_dir("knives").map_err(|e| format!("knives/: {e}"))?;
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    v.sort();
    if v.is_empty() {
        return Err("no knives/*.json here (run from the repo root)".into());
    }
    Ok(v)
}

fn load(paths: &[PathBuf]) -> Result<Vec<Knife>, String> {
    let mut all: Vec<Knife> = Vec::new();
    for p in paths {
        let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let knives = parse(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        for k in knives {
            if all.iter().any(|a| a.id == k.id) {
                return Err(format!(
                    "{}: knife id {} is in another file too",
                    p.display(),
                    k.id
                ));
            }
            all.push(k);
        }
    }
    Ok(all)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let (mut only, mut given) = (Vec::new(), Vec::new());
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--only" => match it.next() {
                Some(id) => only.push(id.clone()),
                None => {
                    eprintln!("--only needs an id\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            o if o.starts_with("--") => {
                eprintln!("{o}: not an option\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => given.push(a.clone()),
        }
    }
    let knives = match files(&given).and_then(|f| load(&f)) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("skipstop-knife: {e}");
            return ExitCode::from(2);
        }
    };
    if let Some(missing) = only.iter().find(|id| !knives.iter().any(|k| &k.id == *id)) {
        eprintln!("skipstop-knife: no knife with id {missing}");
        return ExitCode::from(2);
    }
    let root = Path::new(".");
    let back = recover(root);
    if back > 0 {
        println!("KNIFE-RECOVERED {back} file(s) a killed run left with a knife in");
    }
    let chosen: Vec<&Knife> = knives
        .iter()
        .filter(|k| only.is_empty() || only.contains(&k.id))
        .collect();
    let mut bad = 0;
    match cmd.as_str() {
        "check" if only.is_empty() => {
            for k in &chosen {
                let found = match std::fs::read_to_string(root.join(&k.file)) {
                    Ok(t) => count(&t, &k.old),
                    Err(e) => {
                        eprintln!("skipstop-knife: {}: {e}", k.file);
                        return ExitCode::from(2);
                    }
                };
                if found != 1 {
                    bad += 1;
                    println!("{}", line(k, &Verdict::Invalid { found }));
                }
            }
            println!("KNIVES-CHECK knives={} not_once={bad}", chosen.len());
        }
        "run" => {
            for k in &chosen {
                match run_one(root, k) {
                    Ok(v) => {
                        if v != Verdict::Bites {
                            bad += 1;
                        }
                        println!("{}", line(k, &v));
                    }
                    Err(e) => {
                        eprintln!("skipstop-knife: {} {}: {e}", k.id, k.test);
                        return ExitCode::from(2);
                    }
                }
            }
            println!(
                "KNIVES-RUN knives={} bite={} not={bad}",
                chosen.len(),
                chosen.len() - bad
            );
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    if bad > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
