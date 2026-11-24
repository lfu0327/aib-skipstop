# Changelog

## 0.4.5 — 2026-11-25

- **Default concurrent-safe directory creation (`ensure_dir`).** Tolerates `AlreadyExists` / Windows `os error 183` races in `skipstop-ledger` and `skipstop-core` by default, backed by a 5-round two-process concurrent mkdir test.
- **Cross-platform CI guidance (`README.md`).** Documents concurrent mkdir defaults and splitting OS-specific vs Linux-runnable tests.

## 0.4.4 — 2026-11-24

- **Align tier schema with DESIGN r4 (`§2`, `§4`, `§12.12–13`).** Uses exact 8 tier fields (`reuse`, `spot`, `knives`, `retry_miss`, `platforms`, `overflow`, `cell_max_min`, `cells_max`), removes non-design options (`off`/`observe`/`linux_only`/`fail`/`stop`), and moves `script_max_knives` (140, `20..=300`) and `script_max_min` (15, `5..=30`) into `RepoSettings`.

## 0.4.3 — 2026-11-24

- **Tiered skip/stop policies (`T1` + `T2`).** Adds three built-in tiers (`strict`, `normal`, `fast`) plus custom overrides (`overrides`, `paths`), repo-wide settings (`repo_settings`: `linux_knives`, `knife_length_check`, `merge_batch_tz`/`merge_batch_hours`), and `aib-skipstop tier [--json]`. In Phase 1, `run()` enforces `observe`/`off` mode for unsigned relaxations and falls back to `strict` (`fell_back_to_strict`) while exposing `tier` and `verdict_label` in JSON output.
- **24-hour trial window (`TRIAL_DAYS = 1`).** `aib-skipstop trial` (`S11 -> S17`) now requires 24 hours of history instead of 7 calendar days.

## 0.4.2 — 2026-11-24

- **`trial` counts predicted-to-run real failures (`reds`).** Failures on audited or would-reuse units no longer count against the trial gate; consecutive identical `(unit, key)` failure runs count as one red.
- **`skipstop.json` `extra_inputs` for cargo packages.** Packages that read files outside their crate folder (`include_str!`, `include_bytes!`) can declare those repo-relative paths in `extra_inputs.<package>` so edits invalidate their key.
- **`never_reuse` adds `"security"` class.** Security-sensitive or credential-touching units can opt out of reuse explicitly (`never_reuse:security`).

## 0.4.1 — 2026-11-24

- **`saved --json` includes `recent`.** Up to 20 newest real-mode time-saving events (`at`, `system`, `confirmed`, `saved_ms`, `avoided`, `basis`), net of revoked or wrong-stop entries, for AIB's chat bar and filter.

## 0.4.0 — 2026-11-23

- **The ledger is kept per clone, not per folder name.** Until now a repo's runs were kept under
  the name of the folder the run started in, so every working tree of one clone
  (`git worktree add`) had its own ledger: `aib-skipstop trial` counted only that tree's runs, and
  the history ended when the tree was removed; two different repos in folders of one name
  shared a ledger. Now the folder is `<clone's folder name>-<8 hex of where its git folder is>`,
  the same for every working tree of a clone.
- **History starts again.** A ledger kept under the older name is not read (it stays on disk
  under `<state>/ledger/<folder name>/` and can be removed): its runs were named per folder,
  and a revocation in one working tree's folder would not be seen from another. The first
  real-mode run after the update runs everything, and `trial` counts from there. The
  time-saved book is not touched.
- When git does not tell which clone a folder belongs to, that is a ledger problem like a file
  that cannot be read: it is named in `ledger_problems` and nothing is reused.
- Sharing a ledger does not widen what is reused: a run is reused only under the same key,
  and the key holds the unit, its inputs and the build folder.

## 0.3.0 — 2026-11-23

- `aib-skipstop trial`: what the repo's ledger says of the trial so far (runs, reuses,
  comparisons that agreed and that differed, real failures found) and the bar for making real
  mode the default, part by part. It reads; it changes no mode.

## 0.2.1 — 2026-11-23

- **Fix: a stop's typed saving is an estimate.** A check's `saves_ms` (a number a person wrote in
  the checks file) was recorded as confirmed, so one stop put that whole number into the
  confirmed sum and the time on the clock. It is now recorded as an estimate; only a
  `saves_unit` saving (the median of the unit's own runs) is confirmed. Found on the first real
  use: one stopped command showed 1 h 20 min "saved" that nobody had measured.

## 0.2.0 — 2026-11-23

0.1.0 was built as a draft and never published; this is the first version to publish.

- **Saved on the clock.** Each run writes its wall time beside the wall time of the last full
  run of the same units. `saved` shows it per box (`on the clock`, `clock_ms`): the number to
  show as "time saved". The unit-time sums stay, said to be what they are.
- **A project's settings: `skipstop.json`.** `never_reuse` (units that run every time: they
  deploy, read the network, depend on the date), `defense_period_days`, `keep_days`. A file
  that is not understood stops the run; it never runs as the defaults.
- **Units that are any command.** `units` in `skipstop.json`: a command, the files its result
  depends on, the tools and variables that matter. A repo with no `Cargo.toml` works.
- **Audits compare which cases ran**, not only how many: the same number of other tests, all
  green, under one key is a mismatch and revokes the source.
- A run's ledger file is removed after 30 days; one that holds a revocation never is.
- `docs/FORMATS.md`: exit codes, JSON, every `plan` word, the events book, the ledger. A test
  holds it to the code.
- `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, issue forms.
- 183 tests, 168 knives.

## 0.1.0 — 2026-11-23

The first version. It began as the `aib test` 2.0 engine of ez2app/ez2-aib.

- **Skip**: a unit's key from the files on disk, a run ledger, receipts, exit codes; audits
  (random, forced), the defense round kept per unit, revoking a source. For cargo workspaces,
  with cargo-nextest as the runner. Observe mode is the default: every unit runs and the ledger
  records what would have been reused. `--mode real` does not run a unit that did not change;
  `--fresh` runs everything. A failure, a revoked key, a flaky unit and a unit behind an unread
  ledger are never reused.
- **Stop**: `aib-skipstop stop -- <command>`: checks from `skipstop.stop.json` before an
  expensive command. A `reject` finding stops it in real mode; a `warn` is printed; a broken
  check stops nothing and is said. `--no-stop` passing on the very command takes the stop back.
- **The book**: each run writes its units' times and what it saved; `aib-skipstop saved` prints
  today / week / month / year / all, skip and stop apart, estimates and what was taken back
  beside and never added in.
- Runs outside AIB too (the ledger says `local`). macOS, Windows, Linux.
- Knife tests in Rust (`skipstop-knife`), 138 knives.
- The plugin's `aib-plugin.json` and the plugin area's rules (`docs/PLUGIN.md`).
