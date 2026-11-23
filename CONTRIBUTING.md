# Contributing

Thank you for looking at this. A few things that make a change easy to take.

## Before you write code

Open an issue first for anything that changes what a run decides (what is reused, what runs,
what stops). Those rules are the product; a small change there needs the reason written down.

## What every change needs

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked          # needs cargo-nextest for the end-to-end test
cargo run -p skipstop-knife --release -- check
```

CI runs these on Linux, macOS and Windows, and on Linux it also runs every knife.

- **Rust only**, and it must work on macOS and Windows alike. A change that uses a Unix-only
  call needs its Windows side in the same change.
- **A rule gets a knife.** A knife (`knives/*.json`) is a small wrong edit of the code and the
  name of the test that must then fail. If your change adds a rule ("a dirty tree is never a
  source"), add the knife that breaks it and see it bite:
  `cargo run -p skipstop-knife --release -- run --only <id>`. Write the knife's `old` text after
  `cargo fmt`: it must be in the file exactly once.
- **When it cannot tell, it runs.** A file that cannot be read, a setting that is not
  understood, a key that cannot be worked out: the answer is to run the test (or stop with an
  error), never to reuse.
- **Numbers say what they are.** Confirmed and estimated savings are never added together; a
  time that is not known is shown as not known, not as 0.
- Formats in `docs/FORMATS.md` are read by other programs. Change one only with its version.

## Licence

Apache-2.0. By sending a change you agree it is under that licence.
