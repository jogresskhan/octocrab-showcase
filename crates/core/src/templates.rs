//! Data-driven repository templates — GitHub Actions workflows, a GitHub
//! Pages site, community-health files and a README scaffold.
//!
//! Templates are *rendered from [`TemplateParams`]* via `@PLACEHOLDER@`
//! substitution (no format!-brace fights with `${{ … }}` expressions), so
//! every binary in the suite can install fully-formed, repo-tailored files
//! through the Contents API.

use anyhow::{bail, Result};

// ------------------------------------------------------------------ registry

pub struct TemplateDef {
    pub key: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    /// destination files this template writes
    pub files: &'static [&'static str],
    /// capability module required to install it
    pub gate: &'static str,
}

pub const TEMPLATES: &[TemplateDef] = &[
    TemplateDef {
        key: "ci",
        name: "Multi-OS CI",
        desc: "check + test matrix on ubuntu/macos/windows, clippy+fmt gate",
        files: &[".github/workflows/ci.yml"],
        gate: "workflow:write",
    },
    TemplateDef {
        key: "release",
        name: "Multi-OS Packaging & Release",
        desc: "builds linux/macos/windows release bundles (tar.gz/zip), attaches them + sha256 checksums to a GitHub Release",
        files: &[".github/workflows/release.yml"],
        gate: "workflow:write",
    },
    TemplateDef {
        key: "pages",
        name: "GitHub Pages Deploy",
        desc: "deploys the site/ directory to GitHub Pages (official deploy-pages flow)",
        files: &[".github/workflows/pages.yml"],
        gate: "workflow:write",
    },
    TemplateDef {
        key: "site",
        name: "Pages Landing Site",
        desc: "data-driven index.html + style.css under site/ (hero, badges, install cards)",
        files: &["site/index.html", "site/style.css"],
        gate: "repo:write",
    },
    TemplateDef {
        key: "community",
        name: "Community Health Kit",
        desc: "CONTRIBUTING, Code of Conduct, SECURITY, issue & PR templates",
        files: &[
            "CONTRIBUTING.md",
            "CODE_OF_CONDUCT.md",
            "SECURITY.md",
            ".github/PULL_REQUEST_TEMPLATE.md",
            ".github/ISSUE_TEMPLATE/bug_report.yml",
            ".github/ISSUE_TEMPLATE/feature_request.yml",
        ],
        gate: "repo:write",
    },
    TemplateDef {
        key: "readme",
        name: "README Scaffold",
        desc: "repo landing page: badges, per-OS install table, usage, contributing",
        files: &["README.md"],
        gate: "repo:write",
    },
];

pub fn template_def(key: &str) -> Option<&'static TemplateDef> {
    TEMPLATES.iter().find(|t| t.key == key)
}

// ------------------------------------------------------------------ params

#[derive(Debug, Clone)]
pub struct TemplateParams {
    pub project: String,
    pub owner: String,
    pub repo: String,
    pub description: String,
    pub tagline: String,
    /// binaries to build & package in the release workflow
    pub bins: Vec<String>,
    /// branches that trigger CI / pages deploy
    pub branches: Vec<String>,
    /// workspace members to exclude from CI (e.g. GUI crates needing system libs)
    pub exclude: Vec<String>,
    pub license: String,
    pub contact: String,
}

impl Default for TemplateParams {
    fn default() -> Self {
        Self {
            project: "My Project".into(),
            owner: "octocrab".into(),
            repo: "repo".into(),
            description: String::new(),
            tagline: String::new(),
            bins: vec![],
            branches: vec!["main".into()],
            exclude: vec![],
            license: "MIT".into(),
            contact: String::new(),
        }
    }
}

impl TemplateParams {
    fn slug(&self) -> String {
        self.project.to_lowercase().replace([' ', '_'], "-")
    }
    fn bins_space(&self) -> String {
        if self.bins.is_empty() {
            String::new()
        } else {
            self.bins.join(" ")
        }
    }
    fn exclude_flags(&self) -> String {
        self.exclude
            .iter()
            .map(|c| format!(" --exclude {c}"))
            .collect::<String>()
    }
    fn branches_list(&self) -> String {
        self.branches.join(", ")
    }
    fn substitute(&self, text: &str) -> String {
        let year = chrono::Utc::now().format("%Y").to_string();
        text.replace("@PROJECT@", &self.project)
            .replace("@OWNER@", &self.owner)
            .replace("@REPO@", &self.repo)
            .replace("@DESC@", &self.description)
            .replace("@TAGLINE@", &self.tagline)
            .replace("@BINS_SPACE@", &self.bins_space())
            .replace("@EXCLUDE_FLAGS@", &self.exclude_flags())
            .replace("@BRANCHES@", &self.branches_list())
            .replace("@LICENSE@", &self.license)
            .replace("@CONTACT@", &self.contact)
            .replace("@SLUG@", &self.slug())
            .replace("@YEAR@", &year)
    }
}

/// Render a template into its `(destination path, file content)` pairs.
pub fn render(key: &str, p: &TemplateParams) -> Result<Vec<(String, String)>> {
    let (files, body): (&[&str], String) = match key {
        "ci" => (template_def("ci").unwrap().files, CI.to_string()),
        "release" => (template_def("release").unwrap().files, RELEASE.to_string()),
        "pages" => (template_def("pages").unwrap().files, PAGES.to_string()),
        "site" => (template_def("site").unwrap().files, format!("{SITE_CSS}\n<<<SPLIT>>>\n{SITE_HTML}")),
        "community" => (template_def("community").unwrap().files, COMMUNITY.to_string()),
        "readme" => (template_def("readme").unwrap().files, README.to_string()),
        other => bail!("unknown template '{other}' — see `templates list`"),
    };

    // multi-file bodies are delimited by lines of `<<<SPLIT>>>`
    let parts: Vec<&str> = body.split("\n<<<SPLIT>>>\n").collect();
    if parts.len() != files.len() {
        bail!("internal: template '{key}' has {} parts for {} files", parts.len(), files.len());
    }
    Ok(files
        .iter()
        .zip(parts)
        .map(|(path, content)| (path.to_string(), p.substitute(content)))
        .collect())
}

// ------------------------------------------------------------------ ci

const CI: &str = r#"name: CI (@PROJECT@)

on:
  push:
    branches: [@BRANCHES@]
  pull_request:
  workflow_dispatch:

jobs:
  build:
    name: check+test (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - name: Checkout
        uses: actions/checkout@v4
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
      - name: Cache cargo
        uses: Swatinem/rust-cache@v2
      - name: Check workspace
        run: cargo check --workspace --all-targets@EXCLUDE_FLAGS@
      - name: Test workspace
        run: cargo test --workspace@EXCLUDE_FLAGS@

  lint:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - name: Cache cargo
        uses: Swatinem/rust-cache@v2
      - name: Format
        run: cargo fmt --all -- --check
      - name: Clippy
        run: cargo clippy --workspace@EXCLUDE_FLAGS@ -- -D warnings
"#;

// ------------------------------------------------------------------ release

const RELEASE: &str = r#"name: Release (@PROJECT@)

on:
  push:
    tags: ["v*"]
  workflow_dispatch:

permissions:
  contents: write

jobs:
  build:
    name: package (${{ matrix.target }})
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            ext: tar.gz
          - os: macos-latest
            target: aarch64-apple-darwin
            ext: tar.gz
          - os: macos-latest
            target: x86_64-apple-darwin
            ext: tar.gz
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            ext: zip
    runs-on: ${{ matrix.os }}
    steps:
      - name: Checkout
        uses: actions/checkout@v4
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - name: Cache cargo
        uses: Swatinem/rust-cache@v2
      - name: Build release binaries
        run: cargo build --release --target ${{ matrix.target }}@EXCLUDE_FLAGS@
      - name: Package bundle
        shell: bash
        run: |
          mkdir -p dist
          for bin in @BINS_SPACE@; do
            for cand in "target/${{ matrix.target }}/release/$bin" "target/${{ matrix.target }}/release/$bin.exe"; do
              if [ -f "$cand" ]; then cp "$cand" dist/; fi
            done
          done
          NAME="@SLUG@-${{ matrix.target }}"
          if [ "${{ matrix.ext }}" = "zip" ]; then
            tar -a -cf "$NAME.zip" -C dist .
            echo "ASSET=$NAME.zip" >> "$GITHUB_ENV"
          else
            tar -czf "$NAME.tar.gz" -C dist .
            echo "ASSET=$NAME.tar.gz" >> "$GITHUB_ENV"
          fi
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: bundle-${{ matrix.target }}
          path: ${{ env.ASSET }}

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - name: Collect bundles
        uses: actions/download-artifact@v4
        with:
          path: dist
          merge-multiple: true
      - name: Checksums
        run: cd dist && sha256sum * > checksums.sha256
      - name: Create / update release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh release create "${{ github.ref_name }}" dist/* \
            --title "@PROJECT@ ${{ github.ref_name }}" \
            --generate-notes \
            || gh release upload "${{ github.ref_name }}" dist/* --clobber
"#;

// ------------------------------------------------------------------ pages

const PAGES: &str = r#"name: Deploy Pages (@PROJECT@)

on:
  push:
    branches: [@BRANCHES@]
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  cancel-in-progress: true

jobs:
  deploy:
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4
      - name: Configure Pages
        uses: actions/configure-pages@v5
      - name: Upload site artifact
        uses: actions/upload-pages-artifact@v3
        with:
          path: site
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
"#;

// ------------------------------------------------------------------ site

const SITE_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>@PROJECT@</title>
  <meta name="description" content="@DESC@">
  <link rel="stylesheet" href="style.css">
</head>
<body>
  <header class="topbar">
    <span class="brand">@PROJECT@</span>
    <nav>
      <a href="https://github.com/@OWNER@/@REPO@">repo</a>
      <a href="https://github.com/@OWNER@/@REPO@/releases">releases</a>
      <a href="https://github.com/@OWNER@/@REPO@/actions">ci</a>
      <a href="https://github.com/@OWNER@/@REPO@/blob/main/CONTRIBUTING.md">contributing</a>
    </nav>
  </header>

  <main>
    <section class="hero">
      <h1>@PROJECT@</h1>
      <p class="tagline">@TAGLINE@</p>
      <p class="desc">@DESC@</p>
      <div class="cta">
        <a class="btn primary" href="https://github.com/@OWNER@/@REPO@/releases/latest">Download latest</a>
        <a class="btn" href="https://github.com/@OWNER@/@REPO@">View source</a>
      </div>
      <div class="badges">
        <img alt="ci" src="https://github.com/@OWNER@/@REPO@/actions/workflows/ci.yml/badge.svg">
        <img alt="release" src="https://github.com/@OWNER@/@REPO@/actions/workflows/release.yml/badge.svg">
        <img alt="pages" src="https://github.com/@OWNER@/@REPO@/actions/workflows/pages.yml/badge.svg">
      </div>
    </section>

    <section class="grid">
      <article class="card">
        <h3>&#128683; linux</h3>
        <p>x86_64 bundle from the latest release (tar.gz + sha256).</p>
        <a class="btn" href="https://github.com/@OWNER@/@REPO@/releases/latest">Get it &rarr;</a>
      </article>
      <article class="card">
        <h3>&#63743; macos</h3>
        <p>arm64 &amp; intel bundles built by the packaging workflow.</p>
        <a class="btn" href="https://github.com/@OWNER@/@REPO@/releases/latest">Get it &rarr;</a>
      </article>
      <article class="card">
        <h3>&#9000; windows</h3>
        <p>x86_64 zip bundle with checksum verification.</p>
        <a class="btn" href="https://github.com/@OWNER@/@REPO@/releases/latest">Get it &rarr;</a>
      </article>
      <article class="card">
        <h3>&#129302; contribute</h3>
        <p>PRs welcome — see the contributing guide &amp; code of conduct.</p>
        <a class="btn" href="https://github.com/@OWNER@/@REPO@/blob/main/CONTRIBUTING.md">Start &rarr;</a>
      </article>
    </section>
  </main>

  <footer>
    <span>@LICENSE@ &copy; @YEAR@ @OWNER@ &middot; generated by the Octocrab Suite</span>
  </footer>
</body>
</html>
"#;

const SITE_CSS: &str = r#":root {
  --bg: #0d1117;
  --panel: #161b22;
  --border: #30363d;
  --text: #e6edf3;
  --muted: #8b949e;
  --accent: #3fb950;
  --accent-2: #f78166;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  background: var(--bg);
  color: var(--text);
  font: 16px/1.6 ui-sans-serif, system-ui, "Segoe UI", sans-serif;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 24px;
  border-bottom: 1px solid var(--border);
  background: var(--panel);
  position: sticky;
  top: 0;
}
.brand { font-weight: 700; letter-spacing: 0.5px; }
.topbar nav { display: flex; gap: 18px; flex-wrap: wrap; }
.topbar a { color: var(--muted); text-decoration: none; }
.topbar a:hover { color: var(--accent); }
main { flex: 1; width: min(1080px, 100% - 32px); margin: 0 auto; padding: 48px 0 64px; }
.hero h1 { font-size: clamp(2rem, 6vw, 3.2rem); line-height: 1.15; }
.tagline { color: var(--accent); margin-top: 10px; font-size: 1.15rem; }
.desc { color: var(--muted); margin-top: 12px; max-width: 640px; }
.cta { display: flex; gap: 12px; margin: 26px 0 18px; flex-wrap: wrap; }
.btn {
  display: inline-block;
  padding: 10px 18px;
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--text);
  text-decoration: none;
  background: var(--panel);
  transition: border-color .15s, transform .15s;
}
.btn:hover { border-color: var(--accent); transform: translateY(-1px); }
.btn.primary { background: var(--accent); border-color: var(--accent); color: #05260f; font-weight: 600; }
.badges { display: flex; gap: 8px; flex-wrap: wrap; }
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(230px, 1fr));
  gap: 16px;
  margin-top: 48px;
}
.card {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.card h3 { font-size: 1.05rem; }
.card p { color: var(--muted); flex: 1; }
.card .btn { align-self: flex-start; padding: 7px 14px; font-size: .9rem; }
footer {
  border-top: 1px solid var(--border);
  padding: 18px 24px;
  color: var(--muted);
  font-size: .85rem;
  text-align: center;
  margin-top: auto;
}
"#;

// ------------------------------------------------------------------ community

const COMMUNITY: &str = r#"# Contributing to @PROJECT@

Thanks for your interest in improving **@PROJECT@**! :tada:

## How to contribute

1. **Open an issue** first for anything non-trivial — bugs, features, questions.
2. Fork the repo and create a branch from `main`.
3. Make your change with focused commits; add tests when behaviour changes.
4. Run the checks locally: `cargo check --workspace@EXCLUDE_FLAGS@ && cargo fmt --all -- --check`.
5. Open a pull request and link the issue. CI (ubuntu / macos / windows) must pass.

## Ground rules

* Be kind and constructive — see the [Code of Conduct](CODE_OF_CONDUCT.md).
* Keep PRs reviewable: one concern per PR where possible.
* Security-sensitive findings: follow [SECURITY.md](SECURITY.md), do not open public issues.
<<<SPLIT>>>
# Contributor Covenant Code of Conduct

## Our pledge

We pledge to make participation in the **@PROJECT@** project a
harassment-free experience for everyone, regardless of background, identity or
level of experience.

## Our standards

Positive behaviour includes using welcoming language, respecting differing
viewpoints and gracefully accepting constructive criticism.
Unacceptable behaviour includes trolling, insulting or derogatory comments,
personal or political attacks, and publishing private information without
consent.

## Enforcement

Instances of abusive behaviour may be reported to the project team at
@CONTACT@. All complaints are reviewed and investigated and result in a
response deemed necessary to the circumstances. The project team is obligated
to maintain confidentiality with regard to the reporter.

Adapted from the [Contributor Covenant](https://www.contributor-covenant.org), version 2.1.
<<<SPLIT>>>
# Security Policy

## Supported versions

The latest tagged release of @PROJECT@ receives security fixes.

## Reporting a vulnerability

Please report vulnerabilities privately via
[GitHub security advisories](https://github.com/@OWNER@/@REPO@/security/advisories/new)
or contact @CONTACT@ directly. Please do not open public issues for
security problems. Expect an initial response within 7 days.
<<<SPLIT>>>
## Summary

<!--
Describe the change and the motivation. Link related issues with
"Fixes #123" so they close on merge.
-->

## Checklist

- [ ] `cargo check --workspace@EXCLUDE_FLAGS@` passes
- [ ] `cargo fmt --all -- --check` passes
- [ ] Tests added / updated where behaviour changed
- [ ] Docs / README updated if needed
<<<SPLIT>>>
name: Bug report
description: Something in @PROJECT@ is broken or surprising
labels: [bug]
body:
  - type: textarea
    id: what-happened
    attributes:
      label: What happened?
      description: Also tell us what you expected to happen.
    validations:
      required: true
  - type: textarea
    id: repro
    attributes:
      label: Steps to reproduce
      placeholder: |
        1. run `...`
        2. see error
    validations:
      required: true
  - type: input
    id: version
    attributes:
      label: Version / platform
      placeholder: e.g. v0.1.0 on ubuntu 24.04
    validations:
      required: true
  - type: textarea
    id: logs
    attributes:
      label: Logs or screenshots
<<<SPLIT>>>
name: Feature request
description: Suggest an idea for @PROJECT@
labels: [enhancement]
body:
  - type: textarea
    id: problem
    attributes:
      label: Problem to solve
      description: What are you trying to do, and what is missing?
    validations:
      required: true
  - type: textarea
    id: proposal
    attributes:
      label: Proposed solution
    validations:
      required: true
  - type: textarea
    id: alternatives
    attributes:
      label: Alternatives considered
"#;

// ------------------------------------------------------------------ readme

const README: &str = r#"# @PROJECT@

> @TAGLINE@

@DESC@

[![CI](https://github.com/@OWNER@/@REPO@/actions/workflows/ci.yml/badge.svg)](https://github.com/@OWNER@/@REPO@/actions/workflows/ci.yml)
[![Release](https://github.com/@OWNER@/@REPO@/actions/workflows/release.yml/badge.svg)](https://github.com/@OWNER@/@REPO@/actions/workflows/release.yml)
[![Pages](https://github.com/@OWNER@/@REPO@/actions/workflows/pages.yml/badge.svg)](https://github.com/@OWNER@/@REPO@/actions/workflows/pages.yml)

## Install

Grab a prebuilt bundle from the [latest release](https://github.com/@OWNER@/@REPO@/releases/latest):

| Platform | Bundle |
| --- | --- |
| linux x86_64 | `@SLUG@-x86_64-unknown-linux-gnu.tar.gz` |
| macos (apple silicon) | `@SLUG@-aarch64-apple-darwin.tar.gz` |
| macos (intel) | `@SLUG@-x86_64-apple-darwin.tar.gz` |
| windows x86_64 | `@SLUG@-x86_64-pc-windows-msvc.zip` |

Every bundle ships with a `checksums.sha256` — verify before running:

```bash
sha256sum -c checksums.sha256
```

## Build from source

```bash
git clone https://github.com/@OWNER@/@REPO@.git
cd @REPO@
cargo build --release@EXCLUDE_FLAGS@
```

## Contributing

PRs are welcome — read [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow.
Please note the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Distributed under the @LICENSE@ license — see `LICENSE` for details.
"#;
