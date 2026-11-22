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
| Skip: `aib-skipstop` for cargo workspaces (cargo-nextest as the runner) | **observe mode only**: every unit still runs for real; the ledger records which earlier run it would have reused and whether the result came out the same |
| Skip: really not running a unit | not yet. It is switched on only after the observe ledger shows no disagreement |
| Stop: the generic precheck frame | not yet |
| The time-saved events and their sum (today / week / month / year / all) | here, tested. Each run writes its units' times and, in observe mode, the reuses it would have made; `aib-skipstop saved` prints the sum |
| Other languages than Rust | not yet |

Rules that will not change:

- What cannot be measured runs for real. A key is never weakened to make a reuse possible.
- A reused pass never covers a failure that ran this time.
- An observe-mode saving is a prediction and is in no total. Confirmed and estimated savings are never added together.
- Whatever cannot be read is counted as unread, never guessed.

## Try it

Needs Rust (the toolchain in `rust-toolchain.toml`), git, and
[cargo-nextest](https://nexte.st) (`cargo install cargo-nextest --locked`).

```sh
cargo build --release
cd /path/to/a/cargo/workspace            # a git checkout
AIB_SEAT=me /path/to/aib-skipstop/target/release/aib-skipstop --why
AIB_SEAT=me /path/to/aib-skipstop/target/release/aib-skipstop --why   # a second run, same tree
```

On a clean tree the second run says, per package, which earlier run it would have reused. A tree
with uncommitted changes is not taken as a source.

```sh
aib-skipstop saved --utc-offset 480      # the five boxes, days on a UTC+08:00 clock; --json for a program
```

While only observe mode exists every box is 0 and the predictions are counted beside: a unit that
ran anyway saved nothing.

The ledger goes under the AIB state root (`--state <dir>`, else `EZ2_AIB_STATE_ROOT`, else
`EZ2_AIB_HOME`, else `~/.ez2-aib`). `AIB_SEAT` names who ran it; inside AIB the seat sets it.

## Layout

| crate | what |
|---|---|
| `skipstop-ledger` | append-only JSONL ledger, month-file events, the time-saved events and their sum |
| `skipstop-core` | keys, skip reasons, exit codes, receipts, the run ledger |
| `skipstop-audit` | audits and revocations |
| `skipstop-cargo` | the cargo adapter and the `aib-skipstop` command |

Rust only. macOS, Windows and Linux are checked on every change.

## 中文

**aib-skipstop** 是 AIB 的獨立開源外掛,做兩件省時間的事:

- **Skip(影子續跑)**:輸入沒變的測試單位不重跑,沿用上次真跑的結果,並說得出沿用自哪一次。
- **Stop(預檢省時)**:一定會失敗的工作,在花大錢之前先擋下。

**現況(0.1.0)要講清楚**:目前只有「觀察模式」——每個單位都還是真跑,只在帳上記下「這次本來可以沿用哪一次、結果是否相同」。真的不跑(真沿用)要等觀察帳顯示沒有不一致才會打開;Stop 的通用框架、Rust 以外的語言都還沒做。

不會變的規矩:量不到就真跑;沿用的通過蓋不掉這次真跑出來的失敗;觀察模式的省時不進任何合計;已確認與預估永不相加;讀不懂的照實數出來,不猜。

## License

Apache-2.0. See `LICENSE` and `NOTICE`. This began as the `aib test` 2.0 engine of
[ez2app/ez2-aib](https://github.com/ez2app/ez2-aib).
