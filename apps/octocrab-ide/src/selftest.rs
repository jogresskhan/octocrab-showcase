//! Offline self-test harness for the new panels (templates / pages / repo).
//!
//! Runs headless — no db, no network, no eframe — in two layers:
//!
//! 1. *Render checks*: each panel is drawn into a fresh `egui::Context`
//!    with canned data and the painted output (widget text) is asserted
//!    for the expected controls, values, lock states and errors.
//! 2. *Interaction checks*: a pointer press+release is synthesized at the
//!    painted position of every button / template row; the panel callback
//!    must fire (and must *not* fire while the module gate is locked).
//!
//! The full-app frame loop (panel switching with a canned repository) is
//! scripted in `app.rs`; `main.rs` runs this harness before eframe starts
//! and exits non-zero if it returns `Err` (or panics).

use std::cell::Cell;

use egui::{Context, Pos2, RawInput};
use octocrab_ui_egui::panels::{
    pages as pages_panel, repo as repo_panel, templates as templates_panel,
};
use octocrab_ui_egui::{
    theme, widgets, CommunityHealth, FileProgress, PagesStatus, RepoOverview,
};

const SCREEN: (f32, f32) = (360.0, 940.0);

// ------------------------------------------------------------- egui runner

/// One full egui pass. `click_at` optionally synthesizes a pointer
/// press+release at that screen position (hit-tested against the widgets
/// laid out by the previous pass).
fn run_pass(
    ctx: &Context,
    click_at: Option<Pos2>,
    mut render: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut events = Vec::new();
    if let Some(pos) = click_at {
        events.push(egui::Event::PointerMoved(pos));
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
    }
    let input = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(SCREEN.0, SCREEN.1),
        )),
        time: Some(1.0),
        events,
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| render(ui));
    })
}

/// Every painted text string, in paint order.
fn painted_text(out: &egui::FullOutput) -> String {
    let mut s = String::new();
    for clipped in &out.shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            s.push_str(&t.galley.job.text);
            s.push('\n');
        }
    }
    s
}

/// Screen-space center of the first painted text equal to `needle`
/// (exact galley match preferred, substring fallback).
fn text_center(out: &egui::FullOutput, needle: &str) -> Option<Pos2> {
    let lower = needle.to_lowercase();
    let mut fallback = None;
    for clipped in &out.shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            let text = &t.galley.job.text;
            let hit = if text == needle {
                true
            } else {
                fallback.is_none() && text.to_lowercase().contains(&lower)
            };
            if hit {
                let center = t.pos + t.galley.mesh_bounds.center().to_vec2();
                if text == needle {
                    return Some(center);
                }
                fallback = Some(center);
            }
        }
    }
    fallback
}

/// Render a pass to lay out the panel, then click the painted control
/// named `needle` and render the click pass. Errors if nothing painted it.
fn click_control(
    ctx: &Context,
    render: &mut dyn FnMut(&mut egui::Ui),
    needle: &str,
) -> Result<(), String> {
    let out = run_pass(ctx, None, &mut *render);
    let pos = text_center(&out, needle)
        .ok_or_else(|| format!("cannot click `{needle}` — not painted by the panel"))?;
    run_pass(ctx, Some(pos), &mut *render);
    Ok(())
}

fn check(cond: bool, what: &str) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(format!("selftest assertion failed: {what}"))
    }
}

fn expect_text(text: &str, needle: &str) -> Result<(), String> {
    check(
        text.to_lowercase().contains(&needle.to_lowercase()),
        &format!("painted output does not contain `{needle}`"),
    )
}

fn expect_no_text(text: &str, needle: &str) -> Result<(), String> {
    check(
        !text.to_lowercase().contains(&needle.to_lowercase()),
        &format!("painted output unexpectedly contains `{needle}`"),
    )
}

// ------------------------------------------------------------------ canned

fn canned_progress() -> Vec<FileProgress> {
    vec![
        FileProgress {
            path: "site/index.html".into(),
            done: true,
            ok: true,
            detail: "https://example.test/site/index.html".into(),
        },
        FileProgress {
            path: "site/style.css".into(),
            done: true,
            ok: false,
            detail: "HTTP 422 — canned failure".into(),
        },
        FileProgress {
            path: "site/pending.txt".into(),
            ..Default::default()
        },
    ]
}

// -------------------------------------------------------------------- main

/// Run all offline panel checks. Returns a friendly error on failure.
pub fn run_offline_panel_checks() -> Result<(), String> {
    let ctx = Context::default();
    theme::apply(&ctx);

    // sanity: shared parse helpers the panels + app rely on
    check(
        widgets::parse_repo("octocrab/showcase") == Some(("octocrab".into(), "showcase".into())),
        "parse_repo ok",
    )?;
    check(
        widgets::parse_repo("  owner / repo ") == Some(("owner".into(), "repo".into())),
        "parse_repo trims",
    )?;
    check(
        widgets::parse_repo("no-slash").is_none(),
        "parse_repo rejects missing slash",
    )?;
    check(
        widgets::parse_list("a, b,, c ") == vec!["a", "b", "c"],
        "parse_list splits + trims",
    )?;
    // the panel is data-driven over the core registry — verify the source
    check(
        octocrab_core::templates::TEMPLATES.len() == 6,
        "core registers exactly six templates",
    )?;
    check(
        octocrab_core::templates::template_def("site").is_some()
            && octocrab_core::templates::template_def("community").is_some(),
        "site + community templates exist",
    )?;

    // -------------------------------------------------------- templates
    let mut form = templates_panel::TemplatesForm {
        selected: 0, // Multi-OS CI (gate workflow:write)
        repo: "octocrab/showcase".into(),
        project: "Octocrab Showcase".into(),
        tagline: "a demo showcase".into(),
        bins: "octocrab-ide, octocrab-cli".into(),
        exclude: "octocrab-tauri".into(),
    };
    let progress = canned_progress();

    let selected = Cell::new(None::<usize>);
    let install = Cell::new(false);
    let mut render = |ui: &mut egui::Ui| {
        templates_panel::show(
            ui,
            &mut form,
            &progress,
            false,
            true,
            true,
            None,
            &mut |i| selected.set(Some(i)),
            &mut || install.set(true),
        );
    };

    let out = run_pass(&ctx, None, &mut render);
    let text = painted_text(&out);
    expect_text(&text, "Repository templates")?;
    expect_text(&text, "Multi-OS CI")?;
    expect_text(&text, "workflow:write")?;
    expect_text(&text, "Install Multi-OS CI")?;
    // form field values must be painted (proves the TextEdits rendered)
    expect_text(&text, "octocrab/showcase")?;
    expect_text(&text, "Octocrab Showcase")?;
    expect_text(&text, "a demo showcase")?;
    expect_text(&text, "octocrab-ide, octocrab-cli")?;
    expect_text(&text, "octocrab-tauri")?;
    expect_text(&text, "Install progress")?;
    expect_text(&text, "site/index.html")?;
    expect_text(&text, "HTTP 422 — canned failure")?;

    // interaction: clicking a template row fires on_select with its index
    click_control(&ctx, &mut render, "Multi-OS Packaging & Release")?;
    check(
        selected.get() == Some(1),
        "clicking a template row must fire on_select(1)",
    )?;
    // interaction: the Install button fires on_install
    click_control(&ctx, &mut render, "Install Multi-OS CI")?;
    check(install.get(), "clicking Install must fire on_install")?;

    // installing state renders the spinner line
    let mut render = |ui: &mut egui::Ui| {
        templates_panel::show(
            ui,
            &mut form,
            &progress,
            true, // installing
            true,
            true,
            None,
            &mut |_| {},
            &mut || {},
        );
    };
    let out = run_pass(&ctx, None, &mut render);
    expect_text(&painted_text(&out), "installing")?;

    // gated render → locked indicator instead of the install button
    let mut render = |ui: &mut egui::Ui| {
        templates_panel::show(
            ui,
            &mut form,
            &[],
            false,
            true,
            false, // gate not granted
            None,
            &mut |_| {},
            &mut || {},
        );
    };
    let out = run_pass(&ctx, None, &mut render);
    let text = painted_text(&out);
    expect_text(&text, "locked")?;
    expect_text(&text, "workflow:write")?;
    expect_no_text(&text, "Install Multi-OS CI")?;

    // offline / error line
    let mut render = |ui: &mut egui::Ui| {
        templates_panel::show(
            ui,
            &mut form,
            &[],
            false,
            false, // no client
            true,
            Some("canned error line"),
            &mut |_| {},
            &mut || {},
        );
    };
    let out = run_pass(&ctx, None, &mut render);
    let text = painted_text(&out);
    expect_text(&text, "offline")?;
    expect_text(&text, "canned error line")?;

    // ------------------------------------------------------------ pages
    let mut form = pages_panel::PagesForm {
        repo: "octocrab/showcase".into(),
    };
    let info_enabled = PagesStatus {
        enabled: true,
        html_url: "https://octocrab.github.io/showcase/".into(),
        build_type: "workflow".into(),
        status: "built".into(),
        build_status: "built".into(),
        error: None,
    };

    let status = Cell::new(false);
    let enable = Cell::new(false);
    let build = Cell::new(false);
    let generate = Cell::new(false);
    let mut render = |ui: &mut egui::Ui| {
        pages_panel::show(
            ui,
            &mut form,
            Some(&info_enabled),
            None,
            None,
            &canned_progress(),
            true,
            true,
            true,
            &mut || status.set(true),
            &mut || enable.set(true),
            &mut || build.set(true),
            &mut || generate.set(true),
        );
    };

    let out = run_pass(&ctx, None, &mut render);
    let text = painted_text(&out);
    expect_text(&text, "GitHub Pages")?;
    expect_text(&text, "octocrab/showcase")?;
    expect_text(&text, "Enable Pages")?;
    expect_text(&text, "Build now")?;
    expect_text(&text, "Generate site")?;
    expect_text(&text, "https://octocrab.github.io/showcase/")?;
    expect_text(&text, "latest build: built")?;
    expect_text(&text, "Site generation")?;

    // interaction: every wired button fires its callback
    click_control(&ctx, &mut render, "Status")?;
    check(status.get(), "Status button must fire on_status")?;
    click_control(&ctx, &mut render, "Enable Pages")?;
    check(enable.get(), "Enable Pages button must fire on_enable")?;
    click_control(&ctx, &mut render, "Build now")?;
    check(build.get(), "Build now button must fire on_build")?;
    click_control(&ctx, &mut render, "Generate site")?;
    check(generate.get(), "Generate site button must fire on_generate")?;

    // disabled state must be explicit; locked buttons must NOT fire
    let disabled = PagesStatus {
        enabled: false,
        error: Some("GitHub Pages is not enabled for octocrab/showcase".into()),
        ..Default::default()
    };
    let enable = Cell::new(false);
    let mut render = |ui: &mut egui::Ui| {
        pages_panel::show(
            ui,
            &mut form,
            Some(&disabled),
            Some("fetching pages status"),
            Some("canned pages error"),
            &[],
            false, // no repo:admin → 🔒 labels
            false, // no repo:write
            true,
            &mut || {},
            &mut || enable.set(true),
            &mut || {},
            &mut || {},
        );
    };
    let out = run_pass(&ctx, None, &mut render);
    let text = painted_text(&out);
    expect_text(&text, "Pages is NOT enabled")?;
    expect_text(&text, "canned pages error")?;
    expect_text(&text, "🔒")?;
    click_control(&ctx, &mut render, "Enable Pages")?;
    check(
        !enable.get(),
        "locked Enable Pages button must not fire its callback",
    )?;

    // ------------------------------------------------------------- repo
    let mut form = repo_panel::RepoForm {
        repo: "octocrab/showcase".into(),
    };
    let mut edits = repo_panel::RepoEdits {
        description: "canned description".into(),
        homepage: "https://example.test".into(),
        topics: "rust, egui".into(),
    };
    let overview = RepoOverview {
        description: "canned showcase repo".into(),
        homepage: "https://example.test".into(),
        topics: vec!["rust".into(), "egui".into()],
        stars: 42,
        license: "MIT".into(),
        default_branch: "main".into(),
        html_url: "https://github.com/octocrab/showcase".into(),
    };
    let community = CommunityHealth {
        health_percent: 83,
        files: vec![
            ("README".into(), true),
            ("LICENSE".into(), true),
            ("CONTRIBUTING".into(), false),
            ("Code of Conduct".into(), false),
            ("Issue templates".into(), true),
            ("PR template".into(), false),
        ],
    };

    let load = Cell::new(false);
    let save = Cell::new(false);
    let scaffold = Cell::new(false);
    let mut render = |ui: &mut egui::Ui| {
        repo_panel::show(
            ui,
            &mut form,
            Some(&overview),
            &mut edits,
            Some(&community),
            false,
            None,
            None,
            &canned_progress(),
            true,
            true,
            true,
            &mut || load.set(true),
            &mut || save.set(true),
            &mut || scaffold.set(true),
        );
    };

    let out = run_pass(&ctx, None, &mut render);
    let text = painted_text(&out);
    expect_text(&text, "Repository")?;
    expect_text(&text, "octocrab/showcase")?;
    expect_text(&text, "Save settings")?;
    expect_text(&text, "Scaffold community files")?;
    expect_text(&text, "canned showcase repo")?;
    expect_text(&text, "⭐ 42")?;
    expect_text(&text, "https://example.test")?;
    expect_text(&text, "rust, egui")?;
    expect_text(&text, "health 83/100")?;
    expect_text(&text, "Code of Conduct")?;
    expect_text(&text, "Scaffold progress")?;

    // interaction: Load / Save settings / Scaffold all fire
    click_control(&ctx, &mut render, "Load")?;
    check(load.get(), "Load button must fire on_load")?;
    click_control(&ctx, &mut render, "Save settings")?;
    check(save.get(), "Save settings button must fire on_save")?;
    click_control(&ctx, &mut render, "Scaffold community files")?;
    check(scaffold.get(), "Scaffold button must fire on_scaffold")?;

    // gated variant: lock labels painted, disabled buttons must not fire
    let save = Cell::new(false);
    let scaffold = Cell::new(false);
    let mut render = |ui: &mut egui::Ui| {
        repo_panel::show(
            ui,
            &mut form,
            None,
            &mut edits,
            None,
            false,
            None,
            Some("canned repo error"),
            &[],
            false, // no repo:admin
            false, // no repo:write
            true,
            &mut || {},
            &mut || save.set(true),
            &mut || scaffold.set(true),
        );
    };
    let out = run_pass(&ctx, None, &mut render);
    let text = painted_text(&out);
    expect_text(&text, "🔒 Save settings")?;
    expect_text(&text, "🔒 Scaffold community files")?;
    expect_text(&text, "canned repo error")?;
    click_control(&ctx, &mut render, "Save settings")?;
    check(
        !save.get(),
        "locked Save settings button must not fire its callback",
    )?;
    click_control(&ctx, &mut render, "Scaffold community files")?;
    check(
        !scaffold.get(),
        "locked Scaffold button must not fire its callback",
    )?;

    Ok(())
}
