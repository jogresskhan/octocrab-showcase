# Octocrab Suite

A three-binary GitHub control suite built on [octocrab](https://github.com/XAMPPRocky/octocrab),
structured like [storytold/craft-launcher](https://github.com/storytold/craft-launcher)
(`crates/*` libraries + `apps/*` binaries + `xtask` automation).

```
octocrab-suite/
├── apps/
│   ├── octocrab-cli/      # binary 1 — GitHub control CLI
│   ├── octocrab-tauri/    # binary 2 — Tauri desktop control center (webkit2gtk)
│   └── octocrab-ide/      # binary 3 — data-driven VSCode-style IDE (egui/eframe)
├── crates/
│   ├── core/              # octocrab-core: vault db, capability modules, client builder
│   └── ui-egui/           # octocrab-ui-egui: IDE UI kit (theme, widgets, panels)
└── xtask/                 # automation (sysroot provisioning, ui checks)
```

## The token vault (security model)

The GitHub token lives **only** in an AES-256-GCM encrypted SQLite database at
the Tauri-style app-data dir:

```
~/.local/share/com.octocrab.suite/octocrab.db      (mode 0600)
~/.local/share/com.octocrab.suite/.secret.key      (mode 0600)
```

* No `--token` flag, no env var, no `curl`, no `gh` — nothing that could leak
  into shell history or the process list. `auth set` reads stdin or a file
  (the file is shredded after reading).
* `auth show`/`status` always mask; the raw token never leaves the vault.
* **git integration** — `auth git-credential` is a git credential-helper
  bridge: git pushes/pulls authenticate straight from the vault, so the token
  never lands in argv, env, the remote URL or `.git/config`:

```bash
git -c credential.helper= \
    -c credential.helper='!octocrab-cli auth git-credential' \
    push origin main
```

```bash
printf '%s' '<token>' | octocrab-cli auth set --stdin
octocrab-cli auth check          # map capabilities (classic scopes or fine-grained probes)
```

## Capability modules (the unlock system)

`auth check` probes the token once and stores **capability modules** that every
binary consults to unlock features:

* **classic tokens** — scopes read from the `X-OAuth-Scopes` header
  (`repo`, `workflow`, `read:org`, `write:org`, `admin:org`, `user`, …).
* **fine-grained tokens** — zero-side-effect probes: requests are crafted so
  GitHub answers `422` when a permission is granted and `403` when not —
  nothing is ever created or modified.
* Org access is mapped per-organization (read / write / admin).

17 of 18 modules granted for the current session (only `gist` denied).

## CLI highlights

```bash
octocrab-cli me                                   # identity
octocrab-cli modules list | can repo:write        # session unlock state
octocrab-cli repos list | get | create | upload   # repositories (+ file write w/ sha)
octocrab-cli issues list | create | comment | close
octocrab-cli pulls list
octocrab-cli workflows list | runs                # Actions
octocrab-cli workflows run <o/r> <run_id>         # jobs + every step (✓/✗/● + durations)
octocrab-cli workflows trigger <o/r> ci.yml --ref main --input k=v
octocrab-cli workflows watch <o/r> <run_id>       # poll until done, print step tree
octocrab-cli workflows rerun | cancel
octocrab-cli releases list | latest | create
octocrab-cli tags list
octocrab-cli orgs list | get | members | repos | create
octocrab-cli achievements show | boost --yes      # profile enrichment, first repo,
                                                  # quickdraw issue, merged PR (Pull Shark
                                                  # + YOLO), stars, gist
octocrab-cli setup --yes --org <login>            # one-shot wizard
octocrab-cli api <route>                          # raw GET passthrough
octocrab-cli api --method PUT <route>             # module-gated write verbs
                                                  # (--data '{"k":"v"}', gate:
                                                  # /orgs→org:write,
                                                  # /actions→workflow:write,
                                                  # else repo:write)
octocrab-cli status --json                        # machine summary (web control center)
```

## IDE (`octocrab-ide`)

* Local workspace + GitHub repo modes; virtualized explorer; tabbed editor with
  line-based syntax highlighting (Rust/TOML/MD/JS/YAML).
* **Fast, data-driven loading**: every tree/file/run is cached in the vault DB
  (stale-while-revalidate) + background threads via `poll_promise` — the UI
  thread never touches the network.
* Panels are **gated by capability modules** (e.g. the Run panel needs
  `workflow:read`).
* `--selftest` renders 60 frames under Xvfb and exits 0 (used by CI).

## Tauri control center (`octocrab-tauri`)

* Static dependency-free dashboard (`dist/`) embedded at compile time
  (`withGlobalTauri`), dark VSCode theme.
* Sections: Overview, Workflows, Runs (with per-job step trees), Releases,
  Tags, Repos, Orgs, Modules. Dispatch gated on `workflow:write`.
* All GitHub access through IPC commands that read the encrypted vault.

## Building on this sandbox (no root)

System webkit2gtk/GTK dev libraries are NOT installed and apt needs root, so
the suite provisions a **local sysroot** from Debian packages:

```bash
cargo run -p xtask -- sysroot          # downloads + extracts webkit2gtk-4.1-dev … into ~/sysroot
source ~/sysroot/env.sh                # PKG_CONFIG_SYSROOT_DIR / LD_LIBRARY_PATH / …
cargo build -p octocrab-tauri          # now webkit2gtk-4.1 resolves
```

The sysroot's `libwebkit2gtk` is prefix-patched (same-length string swap
`/usr/lib/x86_64-linux-gnu/` → `/home/z/sysroot/usr/lib64/` + symlink) so
WebKit's helper processes (WebKitNetworkProcess/WebProcess) spawn from the
sysroot at runtime. `~/sysroot/env.sh` also sets
`WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1` etc. — required for Xvfb runs.

## Running the GUIs under Xvfb

```bash
source ~/sysroot/env.sh
Xvfb :86 -screen 0 1440x900x24 &
DISPLAY=:86 LIBGL_ALWAYS_SOFTWARE=1 target/debug/octocrab-ide --selftest
DISPLAY=:86 WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 target/debug/octocrab-tauri
```

## Database schema (shared by all three binaries)

`tokens` (AES-GCM blobs) · `modules` (capability state) · `accounts` · `orgs` ·
`cache` (TTL, data-driven UIs) · `meta` · `events` (audit) · `achievements`.
