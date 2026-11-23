# Hub

A launcher and updater for four apps: **mdedit**, **mdview**, **Sprawl** and **Inbound**. One
window shows which are installed, which have an update, what the update brings, and installs,
updates, rolls back and opens them. Windows first. Not released; nothing here has been published.

```
crates/hub-core   headless and tested: catalog, hub.txt parser, detection, checking,
                  downloading, installing, Hub's own files
bins/hub          the window (winit + softbuffer, every control drawn) and `hub --shot`
tools/            write-hub-txt.ps1 (what each app's release workflow calls), samples/
tests/            shots.sh, shots-check.sh
docs/             STATE, DECISIONS, DESIGN, CODE, COMPLIANCE, APP-CHANGES, INTERFACES, RELEASE
```

How it works, in four lines: each app's release carries one `hub.txt`; Hub reads
`github.com/Kaan88/<repo>/releases/latest/download/hub.txt`, nothing else (no API, no token);
every download is checked against the SHA-256 in that file before anything is placed or run; a
running app is asked to close and waited for, never ended.

Build and try it (never on real settings; see `docs/CODE.md`):

```
cargo build --release -p hub
hub --shot out.png --demo main --dark        # a picture, nothing read from this PC
bash tests/shots.sh target/shots             # every state x view x theme x two sizes
cargo test --release --workspace
```

Start here: `CLAUDE.md`, then `docs/STATE.md`.

**Working on another app for Hub?** Read [docs/COMPLIANCE.md](docs/COMPLIANCE.md): what a release, the
installer and the exe must do so Hub can detect, check and update it.
