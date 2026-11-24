# What aib-skipstop prints and writes

Everything a program may read is here. A field that is not listed may change; a listed one
changes only with the version number of its format.

## Exit codes

| code | from | meaning |
|---|---|---|
| 0 | a run | every unit passed or was reused |
| 1 | a run | a unit failed (also: a pass that ran no test) |
| 3 | a run | no unit at all |
| 4 | a run | nothing ran and nothing was reused |
| 5 | a run | a unit ran past its time |
| 6 | a run | cancelled |
| 7 | a run, `mode`, `stop` | the engine could not work: cargo-nextest missing, the ledger or a file not written or read |
| 8 | a run | a real run disagreed with the source a unit was being reused from |
| 10 | `stop` | a check stopped the command; it was not run |
| 64 | any | a bad call (an unknown word, a mode that is no mode) |
| other | `stop` | the protected command's own code, passed on whole |

When several hold, the worst wins: 1, then 8, 5, 6, 7, 4, 3.

## A run: `aib-skipstop [--mode observe|real] [--fresh] [--plan] [-p <package>]... --json`

One JSON object on standard output.

| field | |
|---|---|
| `invocation_id` | this run's name; its ledger file is `<state>/ledger/<repo>/<invocation_id>.jsonl` |
| `head`, `dirty` | the commit, and whether the tree had uncommitted changes (a dirty run is never a source) |
| `mode` | `observe` or `real` |
| `defense_round` | a unit's defense round ran in this run |
| `cases_run` | how many test cases the runner ran (0 when every unit was reused: it was not called) |
| `exit` | the exit code |
| `ledger_problems` | ledger files that could not be read; any one of them makes a real-mode run run everything |
| `ledger_files_expired` | old ledger files removed at the end of this run |
| `units[]` | below |

Each unit:

| field | |
|---|---|
| `unit` | the package |
| `plan` | what the run did with it, one of the words below |
| `result` | `pass`, `fail`, `invalid` (it ran no test), `none` |
| `tests` | tests it ran now (0 when it was reused) |
| `key` | the unit's key; null when it could not be worked out (`unmeasurable` then says why) |
| `would_reuse` | `{from, key, tree, at}`: the source run, when there is one |
| `same` | the real run now equals the source's result (null: nothing to compare) |
| `audit` | `{compare, revoke}` when the real run was compared with a source: `same`, `mismatch`, `incomparable` (`behavior_changed` is a word of the format that this version does not write); `revoke` true = that source is never a source again. Compared: the outcome, the number of tests, the names of the failed ones, and which cases ran and how each ended |

With `--plan` the object is `{plan_only: true, mode, units: [{unit, plan, key, unmeasurable}],
would_run, decided_in_ms, ledger_problems}`; nothing ran and nothing was written.

### The `plan` words (also in `--why`, in brackets)

| word | the unit … |
|---|---|
| `run:observe` | ran: observe mode runs everything |
| `run:fresh` | ran: `--fresh` |
| `run:never_reuse:<class>` | ran: the project's `skipstop.json` says it is never reused; the class is `writes_worktree`, `reads_outside`, `side_effect`, `time_random_home` or `security` |
| `run:ledger_unread` | ran: a ledger file could not be read, so nothing is reused |
| `run:defense_round` | ran: its weekly defense round was due (or it is new to real mode) |
| `run:no_source` | ran: no key, or no earlier green run of this key on a clean tree, or that source was revoked |
| `run:flaky` | ran: this key has both passed and failed before on this kind of machine |
| `audit:run_audit_random` | has a source and ran anyway, drawn at random (then `+forced:…` when a forced reason held too) |
| `audit:run_audit_forced+forced:streak10` | … after ten reuses in a row |
| `audit:run_audit_forced+forced:age7d` | … no real run for seven days |
| `audit:run_audit_forced+forced:incomparable` | … its last audit did not finish |
| `audit:run_audit_forced+forced:kind_undersampled` | … its kind of unit was audited too rarely |
| `reuse` | was not run; its `would_reuse` is where the result comes from |

## `aib-skipstop saved --json` (`"v": 1`, format `aib-skipstop.saved/v1`)

| field | |
|---|---|
| `measure` | `unit_time_sum`: what `total` / `skip` / `stop` milliseconds are |
| `utc_offset_min` | the clock the days start on |
| `today`, `week`, `month`, `year`, `all` | each a box, below; a week starts on Monday |
| `since` | the time of the earliest saving in `all`; null with none |
| `observed` | observe-mode predictions: in no box |
| `unread` | lines and events that could not be read: in no box |
| `recent` | up to 20 newest real savings (not taken back), each `{at, system, confirmed, saved_ms, avoided, basis}` |

A box:

| field | |
|---|---|
| `total`, `skip`, `stop` | each `{confirmed_ms, estimated_ms, penalty_ms, count}`. Confirmed and estimated are never added together; `penalty_ms` is what was taken back in this box |
| `clock_ms` | saved on the clock: `skip_clock_ms` plus the stops' confirmed time |
| `skip_clock_ms` | over real runs that reused something: the last full run's wall time minus the run's own, never under 0 |
| `skip_clock_unknown_runs` | such runs with no full run to compare with: saved, by a time that is not known |

Show `clock_ms` as "time saved". The unit-time numbers add up work that ran side by side and are
larger than the clock.

## `aib-skipstop trial --json` (`"v": 1`)

What this repo's ledger says of the trial so far. It reads; it changes no mode.

| field | |
|---|---|
| `first_at`, `days` | the first finished run, and whole days since |
| `runs_real`, `runs_observe` | finished runs (a killed run is not counted) |
| `reused` | units not run |
| `compared`, `same`, `differed` | a real run set beside the source it was (or would have been) reused from: audits in real mode, every run in observe mode. `differed` = a reuse that would have been wrong |
| `incomparable`, `revoked`, `reds` | audits that could not be compared; sources revoked; units that ran and failed |
| `ledger_problems` | ledger files that could not be read |
| `bar` | `{days, compared, reds, differed, met}`: the bar for making real mode the default (1 day / 24 hours, 50 comparisons, 3 real failures seen, 0 that differed, no ledger problem) and whether every part is met |

## The project's settings: `skipstop.json` at the repo's root

No file: the defaults. A key that is not below, a value out of range, a key written twice, or a
`never_reuse` name that is no package of the workspace stops the run with exit 7.

| key | |
|---|---|
| `never_reuse` | `{"<package>": "<class>"}`: packages that run every time whatever their key. Class: `writes_worktree`, `reads_outside` (the network, state outside the repo), `side_effect` (deploys, sends, pushes), `time_random_home`, `security` (auth, permissions, signatures) |
| `extra_inputs` | `{"<package>": ["path", "folder/"]}`: extra repo files or folders hashed into a cargo package's key (e.g. files read via `include_str!` outside its crate folder) |
| `defense_period_days` | 1 to 30, default 7: how often every unit runs whatever its key |
| `units` | `{"<name>": {...}}`: units that are any command, below. A name is letters, digits, `.`, `_`, `-`, and not the name of a package |
| `keep_days` | 8 to 3650, default 30: how long a run's ledger file is kept |

A unit of `units`:

| key | |
|---|---|
| `run` | the command as a list of words (no shell); it passes with exit code 0 |
| `run_windows` | the list used on Windows instead, when the program has another name there (`npm.cmd`) |
| `inputs` | needed, not empty: files, or folders ending in `/`, of the repo. The key is their hash, with the settings file's. A name that matches no file leaves the unit with no key: it runs every time and says why |
| `env` | variables whose value is part of the key |
| `tools` | commands whose whole output is part of the key (`["node", "--version"]`); one that cannot be run leaves the unit with no key |
| `timeout_s` | 1 to 86400, default 1800; past it the command and what it started are ended (exit 5) |

In a run's JSON such a unit has `tests: 1`. What it prints goes to standard error. Its audit
compares the outcome only: there are no cases to compare.

## The events book: `<state>/events/<YYYY-MM>.jsonl`

One JSON object a line, appended under a lock. Every event has `v` (1), `id`, `kind`, `at`
(RFC 3339, UTC, milliseconds), `seat` (the AIB seat, or null), `cards`, `status_item`, `host`,
`outcome`. Then, by `kind`:

| kind | own fields |
|---|---|
| `unit_time` | `unit`, `ms` (the unit's own run time), `runner` (the kind of machine), `size` (tests run); `outcome` is how it ended |
| `time_saved` | `system` (`shadow` = skip, `precheck` = stop), `rule`, `verdict` (`reuse` / `reject`), `avoided`, `saved_ms`, `confirmed`, `basis`, `mode` (`observe` / `real`), `head`; `unit` and `from` (skip: the unit and the source run; stop: the command's name and `stop`) |
| `time_saved_confirm` | `ref`: the id of an estimated `time_saved` that was checked |
| `time_saved_wrong` | `ref`, `penalty_ms`, `basis`: that saving is taken back |
| `run_wall` | `wall_ms`, `baseline_wall_ms` (absent when there is no full run of the same units), `units`, `reused`, `mode` |

An event that lacks a common field, has a field of the wrong kind, is dated more than five
minutes after the reader's clock, or names a `ref` that is not there is not counted and is in
`unread`.

The book is for people and for measuring. A program of the same user can edit it, so nothing in
it is a gate or evidence for one.

## The ledger: `<state>/ledger/<repo>/<invocation_id>.jsonl`

`<repo>` names the clone: its folder name, `-`, and eight hex digits of the sha256 of the path
of its git folder (`git rev-parse --git-common-dir`, links resolved), so every working tree of
one clone writes to the same folder and two clones of one name do not. Before 0.4 it was the
name of the working tree's own folder; a folder of that name is no longer read. When git does
not tell the git folder, `<repo>` is `<folder name>-clone-unknown`, the run says so in
`ledger_problems`, and nothing is reused.

One file per run: a `plan` line, a line per unit, an `end` line. A unit that ran has `output_digest`:
`cases-v1:` and the sha256 of which cases it ran and how each ended (order and times left out). A run without its `end` line
was killed or is still going, and is not a source. A file that cannot be read whole is a
problem: it is named in `ledger_problems` and nothing is reused until it is dealt with (remove
the file, or let the run that is writing it finish). Files older than 30 days are removed,
except one that holds a revocation.

## `aib-skipstop stop` lines

| line | |
|---|---|
| `STOP-CHECK <id> pass` | |
| `STOP-CHECK <id> reject: <avoids>` / `warn: <avoids>` | a finding |
| `STOP-CHECK <id> BROKEN (<why>)…` | the check did not work; it stopped nothing |
| `STOP checks=<n> pass=<n> stop=<n> warn=<n> broken=<n> mode=<mode> (from <who said so>)` | the summary |
| `STOPPED by <ids>…` | real mode: the command was not run (exit 10) |
| `STOP-OBSERVED right…` / `would block good work…` | observe mode, once the command has run |
| `STOP-WRONG <n> stop(s)…` | `--no-stop`: stops of this very command on this commit are taken back |
