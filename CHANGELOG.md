# Changelog

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
