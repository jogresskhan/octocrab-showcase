//! `workflows` — full Actions control: list, runs, **per-job step trees**,
//! dispatch, watch, re-run and cancel. Everything comes from octocrab.

use crate::cmd::auth::open_client;
use crate::cmd::repos::split_repo;
use crate::util::*;
use anyhow::Result;
use octocrab::models::workflows::{Job, Run};

/// Render octocrab enum values through their snake_case serde names.
pub fn enum_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|x| x.as_str().map(String::from))
        .unwrap_or_default()
}

pub fn run_status(run: &Run) -> String {
    enum_str(&run.status)
}

pub fn run_conclusion(run: &Run) -> Option<String> {
    serde_json::to_value(&run.conclusion)
        .ok()
        .and_then(|x| match x {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => Some(s),
            _ => None,
        })
}

pub fn job_status(job: &Job) -> String {
    enum_str(&job.status)
}

pub async fn list(full_name: String, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let page = octo
        .workflows(&owner, &repo)
        .list()
        .per_page(50)
        .send()
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&page.items)?);
        return Ok(());
    }
    let t = Table::new(&[12, 30, 34, 12]);
    t.row(&["ID", "NAME", "PATH", "STATE"]);
    for w in &page.items {
        t.row(&[
            w.id.to_string(),
            trunc(&w.name, 29),
            trunc(&w.path, 33),
            match w.state.as_str() {
                "active" => ok(&w.state),
                _ => warn(&w.state),
            },
        ]);
    }
    Ok(())
}

pub async fn runs(full_name: String, branch: Option<String>, limit: usize, json: bool) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let wf_handler = octo.workflows(&owner, &repo);
    let mut builder = wf_handler
        .list_all_runs()
        .per_page(limit.min(100) as u8);
    if let Some(b) = branch {
        builder = builder.branch(b);
    }
    let page = builder.send().await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&page.items)?);
        return Ok(());
    }
    let t = Table::new(&[11, 5, 26, 9, 14, 12, 12]);
    t.row(&["RUN", "#", "WORKFLOW", "BRANCH", "STATUS", "CONCLUSION", "CREATED"]);
    for r in &page.items {
        let icon = run_icon(&run_status(r), run_conclusion(r));
        t.row(&[
            r.id.to_string(),
            r.run_number.to_string(),
            trunc(&r.name, 25),
            trunc(&r.head_branch, 8),
            format!("{icon} {}", run_status(r)),
            run_conclusion(r).unwrap_or_else(|| "—".into()),
            r.created_at.format("%m-%d %H:%M").to_string(),
        ]);
    }
    Ok(())
}

/// Full run view: run summary + every job with every step (the "checking
/// workflow + their steps" view).
pub async fn run_view(full_name: String, run_id: u64, json: bool) -> Result<()> {
    let (db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;

    // completed runs are immutable → cacheable (data-driven UIs reuse this)
    let cache_key = format!("run:{owner}/{repo}:{run_id}");
    let is_completed = move |payload: &serde_json::Value| {
        payload["run"]["status"].as_str() == Some("completed")
    };

    let payload = match octocrab_core::cache::get_json::<serde_json::Value>(&db, &cache_key) {
        Some(v) if is_completed(&v) => v,
        _ => {
            let run = octo.workflows(&owner, &repo).get(octocrab::models::RunId(run_id)).await?;
            let jobs_page = octo
                .workflows(&owner, &repo)
                .list_jobs(octocrab::models::RunId(run_id))
                .per_page(100)
                .send()
                .await?;
            let payload = serde_json::json!({ "run": run, "jobs": jobs_page.items });
            if is_completed(&payload) {
                octocrab_core::cache::set_json(&db, &cache_key, &payload, octocrab_core::cache::TTL_LONG);
            }
            payload
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_run_json(&payload);
    }
    Ok(())
}

pub fn print_run_json(payload: &serde_json::Value) {
    let run = &payload["run"];
    let status = run.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let conclusion = run.get("conclusion").and_then(|v| v.as_str()).map(String::from);
    println!(
        "{} Run #{} (id {}) — {}",
        run_icon(status, conclusion.clone()),
        run.get("run_number").and_then(|v| v.as_i64()).unwrap_or(0),
        run.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        bold(run.get("name").and_then(|v| v.as_str()).unwrap_or("workflow"))
    );
    println!(
        "branch: {} • event: {} • attempt: {}",
        run.get("head_branch").and_then(|v| v.as_str()).unwrap_or("—"),
        run.get("event").and_then(|v| v.as_str()).unwrap_or("—"),
        run.get("run_attempt").and_then(|v| v.as_i64()).unwrap_or(1)
    );
    println!();
    if let Some(jobs) = payload.get("jobs").and_then(|j| j.as_array()) {
        for job in jobs {
            let name = job.get("name").and_then(|v| v.as_str()).unwrap_or("job");
            let status = job.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let conclusion = job
                .get("conclusion")
                .and_then(|v| v.as_str())
                .map(String::from);
            let secs = diff_secs(
                job.get("started_at").and_then(|v| v.as_str()),
                job.get("completed_at").and_then(|v| v.as_str()),
            );
            println!(
                "{} {} {}",
                run_icon(status, conclusion),
                bold(&format!("job: {name}")),
                dim(&fmt_duration(secs))
            );
            if let Some(steps) = job.get("steps").and_then(|s| s.as_array()) {
                let last = steps.len().saturating_sub(1);
                for (i, step) in steps.iter().enumerate() {
                    let sname = step.get("name").and_then(|v| v.as_str()).unwrap_or("step");
                    let sstatus = step.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    let sconcl = step.get("conclusion").and_then(|v| v.as_str()).map(String::from);
                    let branch = if i == last { "└─" } else { "├─" };
                    let secs = diff_secs(
                        step.get("started_at").and_then(|v| v.as_str()),
                        step.get("completed_at").and_then(|v| v.as_str()),
                    );
                    println!(
                        "   {branch} {} {} {}",
                        run_icon(sstatus, sconcl),
                        trunc(sname, 58),
                        dim(&fmt_duration(secs))
                    );
                }
            }
            println!();
        }
    }
}

fn diff_secs(start: Option<&str>, end: Option<&str>) -> i64 {
    match (start, end) {
        (Some(s), Some(e)) => {
            let s = chrono::DateTime::parse_from_rfc3339(s).ok();
            let e = chrono::DateTime::parse_from_rfc3339(e).ok();
            match (s, e) {
                (Some(s), Some(e)) => (e - s).num_seconds(),
                _ => -1,
            }
        }
        _ => -1,
    }
}

pub async fn trigger(
    full_name: String,
    workflow: String,
    reference: String,
    inputs: Vec<String>,
    json: bool,
) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let actions = octo.actions();
    let mut builder = actions.create_workflow_dispatch(&owner, &repo, &workflow, &reference);
    if !inputs.is_empty() {
        let mut map = serde_json::Map::new();
        for kv in inputs {
            let (k, v) = kv
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("--input expects KEY=VALUE, got '{kv}'"))?;
            map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
        }
        builder = builder.inputs(serde_json::Value::Object(map));
    }
    builder.send().await?;
    let msg = serde_json::json!({"dispatched": true, "workflow": workflow, "ref": reference});
    if json {
        println!("{}", serde_json::to_string_pretty(&msg)?);
    } else {
        println!(
            "{} workflow '{workflow}' dispatched on ref '{}'",
            ok("✓"),
            cyan(&reference)
        );
        println!(
            "{}",
            dim(&format!("check runs: octocrab-cli workflows runs {full_name} --limit 5"))
        );
    }
    Ok(())
}

/// Deep one-shot trace of a run — every job and every step with
/// started→completed timestamps, durations and conclusions. Serves the
/// payload from the vault cache when available, like `run` does.
pub async fn trace(full_name: String, run_id: u64, json: bool) -> Result<()> {
    let (db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    let cache_key = format!("run:{owner}/{repo}:{run_id}");
    let payload: serde_json::Value =
        match octocrab_core::cache::get_json::<serde_json::Value>(&db, &cache_key) {
            Some(v) => v,
            None => {
                let run = octo
                    .workflows(&owner, &repo)
                    .get(octocrab::models::RunId(run_id))
                    .await?;
                let jobs = octo
                    .workflows(&owner, &repo)
                    .list_jobs(octocrab::models::RunId(run_id))
                    .per_page(100)
                    .send()
                    .await?;
                let payload = serde_json::json!({ "run": run, "jobs": jobs.items });
                octocrab_core::cache::set_json(&db, &cache_key, &payload, octocrab_core::cache::TTL_LONG);
                payload
            }
        };

    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let run = &payload["run"];
    println!(
        "{} trace {}#{} — {}",
        run_icon(
            run.get("status").and_then(|v| v.as_str()).unwrap_or(""),
            run.get("conclusion").and_then(|v| v.as_str()).map(String::from)
        ),
        bold(&full_name),
        run.get("run_number").and_then(|v| v.as_i64()).unwrap_or(0),
        bold(run.get("name").and_then(|v| v.as_str()).unwrap_or("workflow"))
    );
    println!(
        "event: {} • attempt: {} • sha: {}",
        run.get("event").and_then(|v| v.as_str()).unwrap_or("—"),
        run.get("run_attempt").and_then(|v| v.as_i64()).unwrap_or(1),
        dim(&trunc(run.get("head_sha").and_then(|v| v.as_str()).unwrap_or("—"), 10))
    );
    println!();
    if let Some(jobs) = payload.get("jobs").and_then(|j| j.as_array()) {
        for job in jobs {
            let name = job.get("name").and_then(|v| v.as_str()).unwrap_or("job");
            let status = job.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let conclusion = job.get("conclusion").and_then(|v| v.as_str()).map(String::from);
            println!(
                "{} {} — started {} • ended {} • {}",
                run_icon(status, conclusion.clone()),
                bold(&format!("job: {name}")),
                dim(&fmt_clock(job.get("started_at").and_then(|v| v.as_str()))),
                dim(&fmt_clock(job.get("completed_at").and_then(|v| v.as_str()))),
                dim(&fmt_duration(diff_secs(
                    job.get("started_at").and_then(|v| v.as_str()),
                    job.get("completed_at").and_then(|v| v.as_str()),
                )))
            );
            if let Some(steps) = job.get("steps").and_then(|s| s.as_array()) {
                let last = steps.len().saturating_sub(1);
                for (i, step) in steps.iter().enumerate() {
                    let sname = step.get("name").and_then(|v| v.as_str()).unwrap_or("step");
                    let sstatus = step.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    let sconcl = step.get("conclusion").and_then(|v| v.as_str()).map(String::from);
                    let branch = if i == last { "└─" } else { "├─" };
                    println!(
                        "   {branch} {} {} {} → {} {}",
                        run_icon(sstatus, sconcl),
                        trunc(sname, 44),
                        dim(&fmt_clock(step.get("started_at").and_then(|v| v.as_str()))),
                        dim(&fmt_clock(step.get("completed_at").and_then(|v| v.as_str()))),
                        dim(&fmt_duration(diff_secs(
                            step.get("started_at").and_then(|v| v.as_str()),
                            step.get("completed_at").and_then(|v| v.as_str()),
                        )))
                    );
                }
            }
            println!();
        }
    }
    Ok(())
}

/// RFC3339 → HH:MM:SS for trace timelines.
fn fmt_clock(ts: Option<&str>) -> String {
    ts.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "—".into())
}

pub async fn watch(full_name: String, run_id: u64, interval: u64) -> Result<()> {
    let (db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    loop {
        let run = octo.workflows(&owner, &repo).get(octocrab::models::RunId(run_id)).await?;
        let jobs = octo
            .workflows(&owner, &repo)
            .list_jobs(octocrab::models::RunId(run_id))
            .per_page(100)
            .send()
            .await?;
        let active = jobs
            .items
            .iter()
            .filter(|j| job_status(j) != "completed")
            .count();
        println!(
            "{} run #{}/attempt-{} — {} • {active} job(s) active • {}",
            run_icon(&run_status(&run), run_conclusion(&run)),
            run.run_number,
            run_status(&run),
            run_status(&run),
            chrono::Utc::now().format("%H:%M:%S")
        );
        if run_status(&run) == "completed" {
            let payload = serde_json::json!({ "run": run, "jobs": jobs.items });
            let cache_key = format!("run:{owner}/{repo}:{run_id}");
            octocrab_core::cache::set_json(&db, &cache_key, &payload, octocrab_core::cache::TTL_LONG);
            println!();
            print_run_json(&payload);
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval.max(3))).await;
    }
}

pub async fn rerun(full_name: String, run_id: u64) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    octo.workflows(&owner, &repo).rerun_run(octocrab::models::RunId(run_id)).await?;
    println!("{}", ok(&format!("run {run_id} re-triggered")));
    Ok(())
}

pub async fn cancel(full_name: String, run_id: u64) -> Result<()> {
    let (_db, octo) = open_client()?;
    let (owner, repo) = split_repo(&full_name)?;
    octo.workflows(&owner, &repo).force_cancel_run(octocrab::models::RunId(run_id)).await?;
    println!("{}", warn(&format!("cancellation requested for run {run_id}")));
    Ok(())
}
