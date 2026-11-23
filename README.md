# aib-skipstop

Two ways to stop a test run from wasting time, as an independent open-source AIB plugin.

- **Skip** (shadow rerun): a unit whose inputs did not change is not run again; the result of its
  last real run is reused, and the run says which run that was.
- **Stop** (precheck): work that is sure to fail is stopped before the expensive part starts.

Both write what they saved into one book, so a page can show it: confirmed and estimated apart,
a wrong block or a wrong reuse taken back out.

## Where it stands (0.1.0)

Said plainly, because a tool that reuses results has to earn trust:

| | state |
|---|---|
| Skip: the key of a unit from the files on disk, the run ledger, receipts, exit codes | here, tested |
| Skip: audits (random, forced, the defense round), revoking a source | here, tested (pure functions) |
| Skip: `aib-skipstop` for cargo workspaces (cargo-nextest as the runner) | here. **Observe mode is the default**: every unit runs for real and the ledger records which earlier run it would have reused and whether the result came out the same |
| Skip: really not running a unit (`--mode real`) | here, behind the flag, with random and forced audits and a weekly defense round. It becomes the default only after the observe ledger has shown no disagreement over a trial period |
| Stop: checks before an expensive command (`aib-skipstop stop`) | here. Checks are data; observe mode is the default, `--mode real` stops |
| The time-saved events and their sum (today / week / month / year / all) | here, tested. Each run writes its units' times and, in observe mode, the reuses it would have made; `aib-skipstop saved` prints the sum |
| Other languages than Rust | not yet |

Rules that will not change:

- What cannot be measured runs for real. A key is never weakened to make a reuse possible.
- A reused pass never covers a failure that ran this time.
- An observe-mode saving is a prediction and is in no total. Confirmed and estimated savings are never added together.
- Whatever cannot be read is counted as unread, never guessed.

## Install

As an AIB plugin, by hand for now: `docs/PLUGIN.md` has the commands for macOS, Linux and
Windows (download, check the sum, then run). Or build it here, as below.

## Try it

Needs Rust (the toolchain in `rust-toolchain.toml`), git, and
[cargo-nextest](https://nexte.st) (`cargo install cargo-nextest --locked`).

```sh
cargo build --release
cd /path/to/a/cargo/workspace            # a git checkout
/path/to/aib-skipstop/target/release/aib-skipstop --why
/path/to/aib-skipstop/target/release/aib-skipstop --why   # a second run, same tree
```

On a clean tree the second run says, per package, which earlier run it would have reused. A tree
with uncommitted changes is not taken as a source.

```sh
aib-skipstop saved --utc-offset 480      # the five boxes, days on a UTC+08:00 clock; --json for a program
```

In observe mode every box stays 0 and the predictions are counted beside: a unit that ran anyway
saved nothing.

What the numbers are: for a skipped unit, the time its own tests took in the run it was reused
from, added up. Tests run side by side, so this is more than the time saved on the clock (on a
14-package run here: 35 s on the clock, about three times that as unit time). The output says
so in its first line, and the JSON as `"measure": "unit_time_sum"`.

The time on the clock is given beside it (`on the clock`, `clock_ms`): for a run that reused
something, the wall time of the last full run of the same units minus its own, never under 0;
a run with no full run to compare with is counted as not known. A stop's saving is clock time
already (what the stopped command would have taken).

### Real mode

```sh
aib-skipstop --mode real --why     # this run only
aib-skipstop --mode real --fresh   # run everything this time, whatever the ledger says
aib-skipstop mode real             # from now on, for this state folder; `mode observe` turns it back
aib-skipstop mode                  # which mode, and who said so
aib-skipstop --mode real --plan    # what a run would do with each unit; runs nothing, writes nothing
```

A unit is **not run** only when all of this holds: its key could be worked out; an earlier run of
that same key passed, ran to its end on a tree without uncommitted changes, and was never revoked;
no audit picked the unit this time. Otherwise it runs:

- the first real-mode run, and one run a week after, is the **defense round**: everything runs.
  An input a unit never listed changes no key, so only running everything finds it;
- **audits**: one unit in five at random for the first two weeks, one in ten after; and always
  after ten reuses in a row or seven days without a real run;
- a real run that disagrees with the source it would have been reused from **revokes** that
  source for good, and the run is not green.

A failure is never a source: a red unit runs again until it is green.

A run's ledger file is kept for 30 days and then removed, except one that holds a revocation,
which is kept for good (it is what makes "never a source again" outlast the days). The events
book is not trimmed: its savings are what "this year" and "all" are added up from.

Two more things run a unit whatever its key: a ledger file that cannot be read whole (it may
hold a newer failure or a revocation: nothing is reused until the file named in the output is
dealt with), and a unit whose same key has both passed and failed before on this kind of machine.

The mode is the first of these that says: `--mode`, the variable `AIB_SKIPSTOP_MODE`, the state
folder's mode (`aib-skipstop mode real`), else observe. A word that is not a mode is an error
wherever it stands. When a source is revoked, what earlier runs had saved by reusing it is taken
back out of the total and shown as taken back.

The ledger goes under the AIB state root (`--state <dir>`, else `EZ2_AIB_STATE_ROOT`, else
`EZ2_AIB_HOME`, else `~/.ez2-aib`). Inside AIB the seat sets `AIB_SEAT` and the ledger names it;
outside AIB nothing needs to be set and the ledger says `local`.

## Stop

Before a command that costs a lot, run cheap checks, and do not start it when one shows it is sure
to fail. The checks are a file at the repo's root, `skipstop.stop.json` (found there and run
there, wherever in the repo the command is started):

```json
{"checks": [{
  "id": "merge-conflict",
  "kind": "reject",
  "cmd": ["git", "merge-tree", "--write-tree", "--quiet", "HEAD", "origin/main"],
  "avoids": "a CI round on a branch that cannot be merged",
  "saves_ms": 3600000
}]}
```

```sh
aib-skipstop stop -- cargo test --workspace          # observe: checks run, the command runs anyway
aib-skipstop stop --mode real -- cargo test --workspace
aib-skipstop stop check                              # the checks only
aib-skipstop stop --no-stop -- cargo test --workspace   # you know better than a check
```

- A check **passes** on exit 0 and has a **finding** on one of its `stop_codes` (default `[1]`).
- `kind: reject`: a finding stops the command in real mode (exit 10; the command is not run).
  `kind: warn`: a finding is printed and nothing is stopped. A guess never stops work.
- Anything else is a **broken** check: it could not start, ran past `timeout_s` (default 60; it
  and what it started are killed), was ended by a signal, or ended with another code. A broken
  check stops nothing and is said out loud: it must not be able to block all work, and it must
  not read as a pass.
- What a stop saves: `saves_ms`, or `saves_unit` (the median of that unit's last passing runs in
  the events book). One stopped command is one saving, the largest any stopping check names,
  never their sum; when none names a number it is recorded as 0 with the reason.
- In observe mode the stop is recorded as a prediction and the command runs anyway; the output
  then says whether the stop would have been right (it failed) or would have blocked good work
  (it passed).
- `--no-stop` and the very command a check stopped (the same words, started in the same folder)
  passes on that commit: that stop was wrong, and what it claimed is taken back. Another command
  passing says nothing about it.

The mode is set apart from skip's: `--mode`, `AIB_SKIPSTOP_STOP_MODE`, the state folder's
`stop-mode` file, else observe.

## Layout

| crate | what |
|---|---|
| `skipstop-ledger` | append-only JSONL ledger, month-file events, the time-saved events and their sum |
| `skipstop-core` | keys, skip reasons, exit codes, receipts, the run ledger |
| `skipstop-audit` | audits and revocations |
| `skipstop-stop` | checks before an expensive command |
| `skipstop-cargo` | the cargo adapter and the `aib-skipstop` command |
| `skipstop-knife` | knife tests (a developer tool) |

Rust only. macOS, Windows and Linux are checked on every change.

## Knives

A test that still passes with its rule taken away guards nothing. `knives/*.json` lists, for each
rule, the line to break and the test that then has to fail; `skipstop-knife` (a crate here, no
script and no other language) puts the break in, runs the test, puts the line back, and is red
unless that very test failed.

```sh
cargo run --release -p skipstop-knife -- check     # every anchor is in its file exactly once
cargo run --release -p skipstop-knife -- run       # all of them, about a minute
```

## 中文

**aib-skipstop** 是 AIB 的獨立開源外掛,做兩件省時間的事:

- **Skip(影子續跑)**:輸入沒變的測試單位不重跑,沿用上次真跑的結果,並說得出沿用自哪一次。
- **Stop(預檢省時)**:一定會失敗的工作,在花大錢之前先擋下。

**現況(0.1.0)要講清楚**:預設是「觀察模式」——每個單位都真跑,只在帳上記下「這次本來可以沿用哪一次、結果是否相同」。真沿用已經做好,要加 `--mode real` 才會啟用(有隨機與強制抽查、每週一次全跑的防守輪、`--fresh` 強制全跑);等觀察帳在試用期內沒有不一致,才會改成預設。Stop(`aib-skipstop stop`)也做好了:檢查寫在 `skipstop.stop.json`,`reject` 有發現才擋、`warn` 只印、檢查自己壞掉不擋也不算過;同樣預設觀察、`--mode real` 才真的擋。Rust 以外的語言還沒做。

不會變的規矩:量不到就真跑;沿用的通過蓋不掉這次真跑出來的失敗;觀察模式的省時不進任何合計;已確認與預估永不相加;讀不懂的照實數出來,不猜。

## A project's settings

`skipstop.json` at the repo's root, when the defaults are not right for a project:

```json
{
  "never_reuse": {"deploy-tests": "side_effect"},
  "defense_period_days": 7,
  "keep_days": 30
}
```

A key is the hash of the files a unit reads. A test that reads the network, deploys something
or depends on the date has an input no file holds: name it in `never_reuse` and it runs every
time. A file that is not understood stops the run; it never runs as the defaults instead.

## Formats

`docs/FORMATS.md`: exit codes, the run's JSON and every `plan` word, `saved --json`, the events
book, the ledger, and the lines `stop` prints.

## Releases

`CHANGELOG.md`. A tag `v<version>` builds the three executables and a `SHA256SUMS` into a draft
release; a person publishes it.

## License

Apache-2.0. See `LICENSE` and `NOTICE`. This began as the `aib test` 2.0 engine of
[ez2app/ez2-aib](https://github.com/ez2app/ez2-aib).
