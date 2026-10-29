//! `xtask` — build automation for the Octocrab Suite (mirrors the
//! craft-launcher pattern of `apps/*` + `crates/*` + `xtask`).
//!
//! Tasks:
//! * `xtask sysroot` — provision a local webkit2gtk sysroot from Debian
//!   packages (no root required) so the Tauri binary can build & run
//!   without system-wide installs
//! * `xtask ui-check` — run a GUI binary under Xvfb, screenshot it, exit
//! * `xtask paths` — print the vault / sysroot paths

use anyhow::{bail, Result};
use std::process::Command;

fn main() -> Result<()> {
    let task = std::env::args().nth(1).unwrap_or_else(|| "help".into());
    match task.as_str() {
        "sysroot" => sysroot(),
        "ui-check" => ui_check(
            &std::env::args()
                .nth(2)
                .unwrap_or_else(|| "octocrab-ide".into()),
        ),
        "paths" => paths(),
        _ => {
            println!("usage: cargo run -p xtask -- <sysroot|ui-check|paths>");
            println!("  sysroot          download+extract webkit2gtk dev debs into ~/sysroot");
            println!("  ui-check <bin>   run a GUI binary under Xvfb and screenshot it");
            println!("  paths            print vault + sysroot paths");
            Ok(())
        }
    }
}

fn paths() -> Result<()> {
    println!("vault db : {}", octocrab_data_dir().display());
    println!("sysroot  : {}", sysroot_dir().display());
    Ok(())
}

fn octocrab_data_dir() -> std::path::PathBuf {
    dirs_data().join("com.octocrab.suite")
}

fn dirs_data() -> std::path::PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            std::path::PathBuf::from(home).join(".local/share")
        })
}

fn sysroot_dir() -> std::path::PathBuf {
    home().join("sysroot")
}

fn home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn sysroot() -> Result<()> {
    let debs = home().join("sysroot-debs");
    std::fs::create_dir_all(&debs)?;
    std::fs::create_dir_all(sysroot_dir())?;
    std::env::set_current_dir(&debs)?;

    let pkgs = "libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
                x11-apps imagemagick libgl1-mesa-dri libglx-mesa0 libegl-mesa0 libegl1 \
                libxkbcommon-x11-0 fonts-dejavu-core dbus dbus-daemon dbus-session-bus-common \
                shared-mime-info";
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "apt-cache depends --recurse --no-recommends --no-suggests --no-conflicts \
             --no-breaks --no-replaces --no-enhances --no-pre-depends {pkgs} \
             | awk '/^[a-zA-Z0-9]/ {{print $1}} /^ *Depends: [a-zA-Z0-9]/ {{print $2}}' \
             | tr -d '<>,' | sed 's/:any$//;s/:amd64$//' | sort -u | grep -v '^$'"
        ))
        .output()?;
    let closure = String::from_utf8_lossy(&output.stdout).to_string();
    if closure.trim().is_empty() {
        bail!("cannot compute dependency closure (apt lists missing?)");
    }
    println!("downloading {} packages…", closure.lines().count());
    run("bash", &["-c", &format!("apt-get download {closure}")])?;

    let mut count = 0;
    for entry in std::fs::read_dir(&debs)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "deb").unwrap_or(false) {
            run(
                "dpkg-deb",
                &[
                    "-x",
                    path.to_string_lossy().as_ref(),
                    sysroot_dir().to_string_lossy().as_ref(),
                ],
            )?;
            count += 1;
        }
    }
    println!(
        "extracted {count} packages into {}",
        sysroot_dir().display()
    );

    let env_sh = sysroot_dir().join("env.sh");
    std::fs::write(
        &env_sh,
        format!(
            r#"export PKG_CONFIG_SYSROOT_DIR={sysroot}
export PKG_CONFIG_PATH={sysroot}/usr/lib/x86_64-linux-gnu/pkgconfig:{sysroot}/usr/lib/pkgconfig:{sysroot}/usr/share/pkgconfig
export LD_LIBRARY_PATH={sysroot}/usr/lib/x86_64-linux-gnu:{sysroot}/usr/lib:$LD_LIBRARY_PATH
export PATH={sysroot}/usr/bin:$PATH
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LIBGL_ALWAYS_SOFTWARE=1
"#,
            sysroot = sysroot_dir().display()
        ),
    )?;
    println!("wrote {}", env_sh.display());
    println!("verify: source {env_sh:?} && pkg-config --modversion webkit2gtk-4.1");
    Ok(())
}

fn ui_check(binary: &str) -> Result<()> {
    let target = home().join("octocrab-suite/target/debug").join(binary);
    let target = if target.exists() {
        target
    } else {
        bail!("binary not found: {} (build it first)", target.display());
    };
    let shot = home().join(format!("{binary}-xvfb.png"));
    run(
        "bash",
        &[
            "-c",
            &format!(
                "xvfb-run -a --server-args='-screen 0 1440x900x24' bash -c 'export LIBGL_ALWAYS_SOFTWARE=1; sleep 4; import -display :99 -window root {shot} & sleep 2' {bin} & sleep 10; kill %1 2>/dev/null; true",
                shot = shot.display(),
                bin = target.display()
            ),
        ],
    )?;
    println!("screenshot: {}", shot.display());
    Ok(())
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program).args(args).status()?;
    if !status.success() {
        bail!("{program} {:?} failed with {status}", args);
    }
    Ok(())
}
