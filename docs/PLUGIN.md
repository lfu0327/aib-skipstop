# The AIB plugin area, and this plugin in it

This is what `aib-skipstop` ships to be an AIB plugin, and the rules it follows. The rules are
the plugin area's (agreed 2026-11-23); AIB's own side of them (finding, listing, switching and
calling a plugin) lives in AIB.

## Where a plugin lives

```
<AIB home>/plugins/<name>/
    aib-plugin.json        what the plugin is (below)
    bin/<command>          its executable(s); `.exe` on Windows
```

`<AIB home>` is `~/.ez2-aib` (`%USERPROFILE%\.ez2-aib` on Windows) unless `EZ2_AIB_HOME` says
otherwise. One folder per plugin, named as the plugin is. A plugin's state is not in this folder:
`aib-skipstop` keeps its ledger, events and modes under `<AIB home>/state/aib-skipstop/`, so
replacing the plugin's folder loses nothing.

## What a plugin is

- **A program of its own.** AIB runs the plugin's executable with fixed words and reads what it
  prints; nothing of the plugin is loaded into AIB. A plugin that breaks cannot take AIB down,
  and the same rule holds on macOS, Windows and Linux.
- **One repository per plugin**, with its own version and releases.
- **On by default, and it can be turned off.** A plugin that is installed is used unless its
  switch says off; the switch is AIB's (web page and command line). Being installed and being
  allowed to run are two questions: a paid plugin also needs its name in the licence.

## aib-plugin.json

| field | meaning |
|---|---|
| `schema` | `aib-plugin/v1`. Another value: the file is not read as a plugin |
| `name` | the plugin's name; equals its folder's name; `a-z 0-9 -` |
| `version` | the plugin's own version |
| `summary` | one line for the list |
| `license`, `homepage` | where it comes from |
| `min_aib` | the oldest AIB it works with |
| `paid` | `false`: free. `true`: its name must be in the licence's add-ons to run |
| `default` | `on` or `off`: the switch when the user has not set it |
| `commands` | each `{name, bin}`: a command the plugin gives, and its executable inside the folder (`.exe` is added on Windows) |
| `reads` | each `{what, run, format}`: something AIB may ask the plugin for by running `run` and reading its standard output, which is `format` |
| `sha256` | written at install time, per executable: `{"bin/aib-skipstop": "<64 hex>"}`. AIB checks it before it runs the file; a file that does not match is not run |

A field AIB does not know is left alone, so a newer plugin still lists in an older AIB.

## What AIB may ask this plugin

| run | prints | format |
|---|---|---|
| `aib-skipstop saved --json [--utc-offset <minutes>]` | the five boxes (today, week, month, year, all), skip and stop apart, in milliseconds of **unit time added up** (`"measure": "unit_time_sum"`: more than the clock would show, since units run side by side); `observed` and `unread` beside them | `aib-skipstop.saved/v1` (the object's `"v": 1`) |
| `aib-skipstop mode` | `observe` or `real`, and who said so | one line |
| `aib-skipstop --version` | `aib-skipstop <version>` | one line |

Running a test run or a stop is the user's (or a seat's) doing, with the commands in the README;
AIB does not start those by itself.

## Installing by hand

A release has one executable per system and `SHA256SUMS`. Check the sum before the file is
ever run.

macOS / Linux:

```sh
V=0.3.0
T=aarch64-apple-darwin        # or x86_64-unknown-linux-gnu
B=https://github.com/lfu0327/aib-skipstop/releases/download/v$V
D=~/.ez2-aib/plugins/aib-skipstop
mkdir -p "$D/bin" && cd "$D" \
  && curl -fLO "$B/aib-skipstop-$V-$T" && curl -fLO "$B/SHA256SUMS" && curl -fLO "$B/aib-plugin.json" \
  && grep " aib-skipstop-$V-$T\$" SHA256SUMS | shasum -a 256 -c - \
  && chmod +x "aib-skipstop-$V-$T" && mv "aib-skipstop-$V-$T" bin/aib-skipstop \
  && bin/aib-skipstop --version
```

Windows (cmd):

```bat
set V=0.3.0
set F=aib-skipstop-%V%-x86_64-pc-windows-msvc.exe
set B=https://github.com/lfu0327/aib-skipstop/releases/download/v%V%
set D=%USERPROFILE%\.ez2-aib\plugins\aib-skipstop
if not exist "%D%\bin" mkdir "%D%\bin"
curl.exe -fL -o "%D%\bin\aib-skipstop.exe" "%B%/%F%" && curl.exe -fL -o "%D%\SHA256SUMS" "%B%/SHA256SUMS" && curl.exe -fL -o "%D%\aib-plugin.json" "%B%/aib-plugin.json"
set GOT=none
for /f "delims=" %h in ('certutil -hashfile "%D%\bin\aib-skipstop.exe" SHA256 ^| findstr /v ":"') do @set GOT=%h
findstr /b /i /c:"%GOT%  %F%" "%D%\SHA256SUMS" >nul || (del "%D%\bin\aib-skipstop.exe" & echo checksum mismatch, download deleted)
if exist "%D%\bin\aib-skipstop.exe" "%D%\bin\aib-skipstop.exe" --version
```

A sum that does not match stops there; nothing is run.

It also needs [cargo-nextest](https://nexte.st) for the skip half on a Rust workspace, and git.
