# gentooplz

gentoo please, when you will compile it?

*A live terminal dashboard for what Portage is building right now*

[![Rust](https://img.shields.io/badge/Rust-2021-b7410e?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Gentoo](https://img.shields.io/badge/Gentoo-Portage-54487a?style=flat-square&logo=gentoo&logoColor=white)](https://wiki.gentoo.org/wiki/Portage)
[![License](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE)

[Features](#features) • [Requirements](#requirements) • [Installation](#installation) • [Usage](#usage) • [How it works](#how-it-works)

---

`gentooplz` is a `ratatui`-based TUI that watches `/var/tmp/portage` and shows, in real time, exactly what emerge is compiling: which package, which build system, how far along it is, and how much CPU/RAM it's actually using no more guessing from a wall of scrolling `emerge.log` output.

## Features

- **Live build list** - every package currently under `/var/tmp/portage/<category>/<pkg-version>/temp/` is detected automatically, no configuration needed.
- **Real progress, not guesses** - parses `ninja`/`meson`/`cmake` (`[123/456]`), `make` (`[ 45%]`), and `cargo` (`Compiling foo v1.2`) output directly from each build's log tail. For plain autotools/make builds that print nothing structured, it falls back to counting source vs. object files on disk.
- **Per-package resource usage** - matches the process tree behind each build (including short-lived `cc1`, `ld`, etc. children) and sums their live CPU% and RSS.
- **Queue position** - reads `/var/log/emerge.log` to show progress through the whole merge list (e.g. "3 of 15"), not just what's on disk right now.
- **Three focused views** - `Build` (now-building card + CPU history), `Log` (live tail of the selected package's build log), and `Resources` (system CPU/RAM/load + full build queue).
- **Cheap to run** - all disk and `/proc` scanning happens on a background task; the render loop never blocks on I/O.

## Requirements

- Linux with a Gentoo/Portage install.
- Rust 2021 edition (stable toolchain) to build.
- Root privileges to run (see below).

> [!IMPORTANT]
> Portage builds run as `portage:portage`, and inspecting other users' processes under `/proc` needs elevated rights too. Because of this, `gentooplz` **requires root** to actually see anything.
>
> If you start it without root, it won't silently fail or show an empty screen - it detects whether `doas` or `sudo` is installed on your system and prints the exact command to re-run.

## Installation

```sh
## Manual
git clone https://github.com/JustRoccat/gentooplz
cd gentooplz
cargo build --release

## Cargo 
cargo install gentooplz
```

The binary is then available at `target/release/gentooplz`.

## Usage

```sh
sudo ./target/release/whatamicompiling
# or, on systems using doas:
doas ./target/release/whatamicompiling
```

### CLI flags

| Flag | Description | Default |
| --- | --- | --- |
| `-i, --interval <MS>` | Refresh interval for the disk + process scan | `1000` |
| `-p, --portage-tmp <PATH>` | Path to Portage's working directory | `/var/tmp/portage` |
| `-e, --emerge-log <PATH>` | Path to Portage's global emerge log, used for queue position | `/var/log/emerge.log` |
| `-h, --help` | Print help | |
| `-V, --version` | Print version | |

### Keybinds

| Key | Action |
| --- | --- |
| `↑`/`k`, `↓`/`j` | Move selection between active builds |
| `←`/`h`, `→`/`l`, `Tab`/`Shift+Tab` | Switch between `Build`, `Log`, and `Resources` views |
| `q`, `Esc` | Quit |


## How it works

```
Portage scanner (scanner.rs)  --ScanEvent (mpsc)-->  App (main.rs)  -->  ratatui UI (ui/)
        |
        +-- parser.rs: tail-reads build.log (last 4 KB) + ninja/make/cargo regexes
```

Each tick, the scanner:

1. Walks `/var/tmp/portage` for package directories with a `temp/` subfolder the marker that emerge is actively working on them.
2. Reads the tail of each build's `build.log` to detect the build system and progress.
3. Matches running processes (via `sysinfo`) whose working directory or command line points inside the build directory, then sums CPU/RAM across their whole process trees.
4. Reads the tail of `/var/log/emerge.log` to resolve the current position in the merge queue.

All of this runs on `tokio::task::spawn_blocking`, so disk and `/proc` I/O never stalls the render loop, and updates are pushed to the UI over an `mpsc` channel.
