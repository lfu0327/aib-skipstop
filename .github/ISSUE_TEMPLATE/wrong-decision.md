---
name: It reused (or stopped) something it should not have
about: A test was skipped that would have failed, or a command was stopped that was fine
labels: wrong-decision
---

**What it decided** (paste the unit's line from `aib-skipstop --why`, or the `STOP-CHECK` line):

**What was true** (the test fails when run / the command was fine):

**The run's JSON** (`aib-skipstop --mode real --json`, the unit's object is enough):

**What changed between the source run and this one** (a file outside the package, an
environment variable, the date, the network …):

**Version and system** (`aib-skipstop --version`, macOS / Windows / Linux):
