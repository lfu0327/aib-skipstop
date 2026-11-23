# Changelog

## Unreleased

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
