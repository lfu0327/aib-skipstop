# Security

## Reporting

Please do not open a public issue for a security problem. Use GitHub's private report:
**Security → Report a vulnerability** on this repository. You will get an answer within a week.

## What counts

aib-skipstop decides that a test need not run, or that a command must not start. A way to make
it decide wrongly is a security problem when it lets a failing change through:

- a result reused for inputs it was not made for (two different trees with one key);
- a failed or killed run taken as a source;
- a revoked source used again;
- a settings or ledger file that is not understood being read as "nothing to say";
- a plugin binary that is not the one its `aib-plugin.json` names being run.

## What it does not defend against

The ledger, the events book and the mode file are plain files of the user who runs it. Someone
who can write those files as that user can make it reuse anything, and can equally edit the
tests. Nothing in those files is evidence for a gate on another machine. Use a full run
(`--fresh`) wherever a result must be trusted by someone else: a merge to main, a release.
