//! `octocrab-cli` — GitHub control CLI powered by [octocrab].
//!
//! Design rules:
//! * the token lives **only** in the encrypted SQLite vault
//!   (`$XDG_DATA_HOME/com.octocrab.suite/octocrab.db`);
//! * no `--token` flag, no env var, no `curl`, no `gh` — nothing that could
//!   leak into shell history or the process list;
//! * every feature is gated by the data-driven capability modules detected by
//!   `auth check` (classic scopes or fine-grained probes).

mod cmd;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "octocrab-cli",
    version,
    about = "GitHub control CLI powered by octocrab — token comes from the encrypted local vault"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Token vault: store, inspect, clear (token never passes through argv)
    Auth {
        #[command(subcommand)]
        cmd: AuthCmd,
    },
    /// Authenticated user identity
    Me {
        /// output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Capability modules unlocked for this session
    Modules {
        #[command(subcommand)]
        cmd: ModulesCmd,
    },
    /// Repositories: list, get, create, read files
    Repos {
        #[command(subcommand)]
        cmd: ReposCmd,
    },
    /// Issues: list, create, comment, close
    Issues {
        #[command(subcommand)]
        cmd: IssuesCmd,
    },
    /// Pull requests: list
    Pulls {
        #[command(subcommand)]
        cmd: PullsCmd,
    },
    /// Actions: workflows, runs, jobs, steps, dispatch, watch
    Workflows {
        #[command(subcommand)]
        cmd: WorkflowsCmd,
    },
    /// Releases: list, latest, create
    Releases {
        #[command(subcommand)]
        cmd: ReleasesCmd,
    },
    /// Tags: list
    Tags {
        /// OWNER/REPO
        repo: String,
        #[arg(long, default_value = "30")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Organizations: list, get, members, repos, create
    Orgs {
        #[command(subcommand)]
        cmd: OrgsCmd,
    },
    /// Achievement farming: profile enrichment, first repo, org, badges
    Achievements {
        #[command(subcommand)]
        cmd: AchCmd,
    },
    /// One-shot wizard: capability check → profile → first repo → org
    Setup {
        /// confirm all write actions
        #[arg(long)]
        yes: bool,
        /// create this organization
        #[arg(long)]
        org: Option<String>,
    },
    /// Vault database inspection
    Db {
        #[command(subcommand)]
        cmd: DbCmd,
    },
    /// Raw GET against any GitHub API route (module-gated by the token)
    Api {
        /// route, e.g. "repos/owner/repo" (leading slash optional)
        route: String,
        #[arg(long)]
        json: bool,
    },
    /// Machine-readable suite summary (consumed by the web control center)
    Status {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum AuthCmd {
    /// Store a token (reads stdin with --stdin, or a one-shot file with --file;
    /// the file is shredded after reading)
    Set {
        /// vault label for the token
        #[arg(long, default_value = "default")]
        label: String,
        /// read the token from stdin
        #[arg(long)]
        stdin: bool,
        /// read the token from a file (file is deleted afterwards)
        #[arg(long)]
        file: Option<String>,
    },
    /// Show vault status (token always masked)
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Full capability probe: classic scopes or fine-grained permission map
    Check {
        #[arg(long)]
        json: bool,
    },
    /// List stored tokens (masked)
    Tokens,
    /// Remove all tokens and capability data
    Clear,
    /// git credential-helper bridge — lets git push/pull authenticate
    /// straight from the encrypted vault (token never hits argv/env/URL)
    GitCredential,
}

#[derive(Subcommand)]
enum ModulesCmd {
    /// List all modules and their unlock state
    List {
        #[arg(long)]
        json: bool,
    },
    /// Exit 0 if module is unlocked, 2 otherwise
    Can {
        /// e.g. repo:write, org:read, workflow:write
        key: String,
    },
}

#[derive(Subcommand)]
enum ReposCmd {
    /// List your repositories
    List {
        #[arg(long, default_value = "30")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Show repository details
    Get {
        /// OWNER/REPO
        repo: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a repository
    Create {
        name: String,
        #[arg(long)]
        desc: Option<String>,
        /// make it public (default: private)
        #[arg(long)]
        public: bool,
        #[arg(long)]
        json: bool,
    },
    /// Create or update a file (contents write)
    Upload {
        /// OWNER/REPO
        repo: String,
        /// destination path in the repo
        path: String,
        /// local file to upload
        #[arg(long)]
        file: String,
        #[arg(long, default_value = "update via octocrab-cli")]
        message: String,
        #[arg(long)]
        branch: Option<String>,
    },
    /// Print a file's contents
    File {
        /// OWNER/REPO
        repo: String,
        path: String,
        #[arg(long)]
        ref_: Option<String>,
    },
}

#[derive(Subcommand)]
enum IssuesCmd {
    List {
        /// OWNER/REPO
        repo: String,
        #[arg(long, default_value = "open")]
        state: String,
        #[arg(long, default_value = "30")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Create {
        /// OWNER/REPO
        repo: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: Option<String>,
    },
    Comment {
        /// OWNER/REPO
        repo: String,
        number: u64,
        #[arg(long)]
        body: String,
    },
    Close {
        /// OWNER/REPO
        repo: String,
        number: u64,
    },
}

#[derive(Subcommand)]
enum PullsCmd {
    List {
        /// OWNER/REPO
        repo: String,
        #[arg(long, default_value = "open")]
        state: String,
        #[arg(long, default_value = "30")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum WorkflowsCmd {
    /// List workflow definitions
    List {
        /// OWNER/REPO
        repo: String,
        #[arg(long)]
        json: bool,
    },
    /// List workflow runs
    Runs {
        /// OWNER/REPO
        repo: String,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long, default_value = "20")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Show one run with every job and every step
    Run {
        /// OWNER/REPO
        repo: String,
        run_id: u64,
        #[arg(long)]
        json: bool,
    },
    /// Dispatch (trigger) a workflow
    Trigger {
        /// OWNER/REPO
        repo: String,
        /// workflow file name (ci.yml) or workflow id
        workflow: String,
        #[arg(long, default_value = "main")]
        ref_: String,
        #[arg(long = "input")]
        inputs: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Poll a run until completion, then print the full step tree
    Watch {
        /// OWNER/REPO
        repo: String,
        run_id: u64,
        #[arg(long, default_value = "5")]
        interval: u64,
    },
    /// Re-run a workflow run
    Rerun {
        /// OWNER/REPO
        repo: String,
        run_id: u64,
    },
    /// Cancel a workflow run
    Cancel {
        /// OWNER/REPO
        repo: String,
        run_id: u64,
    },
}

#[derive(Subcommand)]
enum ReleasesCmd {
    List {
        /// OWNER/REPO
        repo: String,
        #[arg(long, default_value = "20")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Latest {
        /// OWNER/REPO
        repo: String,
        #[arg(long)]
        json: bool,
    },
    Get {
        /// OWNER/REPO
        repo: String,
        tag: String,
        #[arg(long)]
        json: bool,
    },
    Create {
        /// OWNER/REPO
        repo: String,
        tag: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        prerelease: bool,
        #[arg(long)]
        draft: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum OrgsCmd {
    List {
        #[arg(long)]
        json: bool,
    },
    Get {
        org: String,
        #[arg(long)]
        json: bool,
    },
    Members {
        org: String,
        #[arg(long, default_value = "30")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Repos {
        org: String,
        #[arg(long, default_value = "30")]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Create a new organization
    Create {
        login: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum AchCmd {
    /// Show profile completeness + achievement action log
    Show,
    /// Enrich profile, ensure showcase repo, quickdraw issue, merged PR, stars, gist
    Boost {
        /// confirm all write actions
        #[arg(long)]
        yes: bool,
        /// also create this organization
        #[arg(long)]
        org: Option<String>,
    },
}

#[derive(Subcommand)]
enum DbCmd {
    /// Print the vault database path
    Path,
    /// Human-readable vault info + recent events
    Info,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    use cmd::{achievements as ach, auth, issues, misc, modules, orgs, repos, releases, workflows};
    match cli.cmd {
        Cmd::Auth { cmd } => match cmd {
            AuthCmd::Set { label, stdin, file } => auth::set(label, stdin, file).await,
            AuthCmd::Status { json } => auth::status(json).await,
            AuthCmd::Check { json } => auth::check(json).await,
            AuthCmd::Tokens => auth::tokens().await,
            AuthCmd::Clear => auth::clear().await,
            AuthCmd::GitCredential => auth::git_credential(),
        },
        Cmd::Me { json } => misc::me(json).await,
        Cmd::Modules { cmd } => match cmd {
            ModulesCmd::List { json } => modules::list(json).await,
            ModulesCmd::Can { key } => modules::can(key).await,
        },
        Cmd::Repos { cmd } => match cmd {
            ReposCmd::List { limit, json } => repos::list(limit, json).await,
            ReposCmd::Get { repo, json } => repos::get(repo, json).await,
            ReposCmd::Create { name, desc, public, json } => {
                repos::create(name, desc, public, json).await
            }
            ReposCmd::Upload { repo, path, file, message, branch } => {
                repos::upload(repo, path, file, message, branch).await
            }
            ReposCmd::File { repo, path, ref_ } => repos::file(repo, path, ref_).await,
        },
        Cmd::Issues { cmd } => match cmd {
            IssuesCmd::List { repo, state, limit, json } => {
                issues::issues_list(repo, state, limit, json).await
            }
            IssuesCmd::Create { repo, title, body } => issues::issues_create(repo, title, body).await,
            IssuesCmd::Comment { repo, number, body } => issues::issues_comment(repo, number, body).await,
            IssuesCmd::Close { repo, number } => issues::issues_close(repo, number).await,
        },
        Cmd::Pulls { cmd } => match cmd {
            PullsCmd::List { repo, state, limit, json } => {
                issues::pulls_list(repo, state, limit, json).await
            }
        },
        Cmd::Workflows { cmd } => match cmd {
            WorkflowsCmd::List { repo, json } => workflows::list(repo, json).await,
            WorkflowsCmd::Runs { repo, branch, limit, json } => {
                workflows::runs(repo, branch, limit, json).await
            }
            WorkflowsCmd::Run { repo, run_id, json } => {
                workflows::run_view(repo, run_id, json).await
            }
            WorkflowsCmd::Trigger { repo, workflow, ref_, inputs, json } => {
                workflows::trigger(repo, workflow, ref_, inputs, json).await
            }
            WorkflowsCmd::Watch { repo, run_id, interval } => {
                workflows::watch(repo, run_id, interval).await
            }
            WorkflowsCmd::Rerun { repo, run_id } => workflows::rerun(repo, run_id).await,
            WorkflowsCmd::Cancel { repo, run_id } => workflows::cancel(repo, run_id).await,
        },
        Cmd::Releases { cmd } => match cmd {
            ReleasesCmd::List { repo, limit, json } => releases::list(repo, limit, json).await,
            ReleasesCmd::Latest { repo, json } => releases::latest(repo, json).await,
            ReleasesCmd::Get { repo, tag, json } => releases::get_by_tag(repo, tag, json).await,
            ReleasesCmd::Create { repo, tag, name, notes, prerelease, draft, json } => {
                releases::create(repo, tag, name, notes, prerelease, draft, json).await
            }
        },
        Cmd::Tags { repo, limit, json } => repos::tags(repo, limit, json).await,
        Cmd::Orgs { cmd } => match cmd {
            OrgsCmd::List { json } => orgs::list(json).await,
            OrgsCmd::Get { org, json } => orgs::get(org, json).await,
            OrgsCmd::Members { org, limit, json } => orgs::members(org, limit, json).await,
            OrgsCmd::Repos { org, limit, json } => orgs::repos(org, limit, json).await,
            OrgsCmd::Create { login, name, json } => orgs::create_org(login, name, json).await,
        },
        Cmd::Achievements { cmd } => match cmd {
            AchCmd::Show => ach::show().await,
            AchCmd::Boost { yes, org } => ach::boost(yes, org).await,
        },
        Cmd::Setup { yes, org } => {
            // wizard: check → boost (which covers profile/repo/org writes)
            println!("{}", util::bold("octocrab suite setup"));
            auth::check(false).await?;
            println!();
            ach::boost(yes, org).await
        }
        Cmd::Db { cmd } => match cmd {
            DbCmd::Path => misc::db_info(true).await,
            DbCmd::Info => misc::db_info(false).await,
        },
        Cmd::Api { route, json } => misc::api_raw(route, json).await,
        Cmd::Status { json } => misc::status(json).await,
    }
}
