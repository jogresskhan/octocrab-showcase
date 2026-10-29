//! `IdeApp` — the eframe application: layout, keyboard shortcuts, command
//! palette, SQLite-backed stale-while-revalidate caching and all promise
//! plumbing. Every GitHub call runs on a background thread; the UI renders
//! instantly from cached data.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use egui::{Color32, Context, Key, RichText, Ui};
use octocrab::Octocrab;
use octocrab_core::cache::{self, TTL_LONG, TTL_MEDIUM, TTL_SHORT};
use octocrab_core::db::{ModuleState, SuiteDb};
use octocrab_ui_egui::panels::{
    editor_tabs, explorer, modules, releases, search, tags, workflows,
    pages as pages_panel, repo as repo_panel, templates as templates_panel,
};
use octocrab_ui_egui::{
    highlight, theme, widgets, CommunityHealth, FileProgress, JobRow, Lang, OpenTab, PagesStatus,
    ReleaseRow, RepoEdits, RepoOverview, RunRow, SearchHit, Span, TagRow, TreeItem,
};
use poll_promise::Promise;

use crate::{fuzzy, net};

// ------------------------------------------------------------- helpers

fn db_lock(db: &Arc<Mutex<SuiteDb>>) -> MutexGuard<'_, SuiteDb> {
    match db.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

fn progress_lock(p: &Arc<Mutex<Vec<FileProgress>>>) -> MutexGuard<'_, Vec<FileProgress>> {
    match p.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

fn progress_snapshot(p: &Arc<Mutex<Vec<FileProgress>>>) -> Vec<FileProgress> {
    progress_lock(p).clone()
}

/// Push a pre-rendered file list through the Contents API on the *current*
/// (background) thread, updating the shared per-file progress rows live.
fn push_files_sync(
    octo: Arc<Octocrab>,
    owner: String,
    repo: String,
    files: Vec<(String, String)>,
    message: String,
    progress: Arc<Mutex<Vec<FileProgress>>>,
) -> Vec<FileProgress> {
    {
        let mut rows = progress_lock(&progress);
        *rows = files
            .iter()
            .map(|(p, _)| FileProgress { path: p.clone(), ..Default::default() })
            .collect();
    }
    let mut out = Vec::new();
    for (i, (path, content)) in files.into_iter().enumerate() {
        let res = net::block_on(net::put_file(&octo, &owner, &repo, &path, &content, &message, None));
        let (ok, detail) = match res {
            Ok(url) => (true, url),
            Err(e) => (false, format!("{e}")),
        };
        out.push(FileProgress { path: path.clone(), done: true, ok, detail: detail.clone() });
        if let Some(row) = progress_lock(&progress).get_mut(i) {
            row.done = true;
            row.ok = ok;
            row.detail = detail;
        }
    }
    out
}

fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn spawn<T, F>(f: F) -> Promise<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    Promise::spawn_thread("octocrab-ide-task", f)
}

const LOCAL_ROOT: &str = "/home/z/octocrab-suite";
const SELFTEST_MARKER: &str = "/tmp/octocrab-ide-selftest-ok";
const RUNS_LIMIT: usize = 20;

/// poll-promise 0.3 `take()` hands back a reference — copy it out so the
/// polling code can work with owned values.
fn take_owned<T: Clone + Send>(slot: &mut Option<Promise<T>>) -> Option<T> {
    let promise = slot.as_ref()?;
    let value = promise.ready()?.clone();
    slot.take();
    Some(value)
}


// -------------------------------------------------------------- models

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Explorer,
    Search,
    Pulls,
    Workflows,
    Releases,
    Templates,
    Pages,
    Repo,
    Account,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Local,
    Github,
}

#[derive(Clone, Copy, PartialEq)]
enum Center {
    Editor,
    RunJobs(u64),
    Modules,
}

#[derive(Clone, Copy)]
enum PagesOp {
    Status,
    Enable,
    Build,
}

#[derive(Clone)]
enum Command {
    ToggleSidebar,
    Goto(Activity),
    ReloadTree,
    ClearCache,
    RefreshRuns,
    DispatchWorkflow,
    OpenModules,
    SwitchLocal,
    SwitchGithub,
    Quit,
}

#[derive(Clone)]
struct TreePayload {
    items: Vec<TreeItem>,
    truncated: bool,
}

// ---------------------------------------------------------------- app

pub struct IdeApp {
    db: Arc<Mutex<SuiteDb>>,
    db_path: String,
    selftest: bool,
    frames: u32,

    // identity / net
    octo: Option<Arc<Octocrab>>,
    login: Option<String>,
    token_kind: Option<String>,
    token_label: Option<String>,
    net_err: Option<String>,
    identity_p: Option<Promise<Result<String, String>>>,
    repo_p: Option<Promise<Result<net::RepoInfo, String>>>,
    repo: Option<net::RepoInfo>,

    // modules
    mods: Vec<ModuleState>,
    orgs: Vec<(String, String, bool, bool, bool)>,
    granted: usize,

    // layout
    activity: Activity,
    sidebar_open: bool,
    center: Center,
    branch: String,

    // explorer
    source: Source,
    tree: Vec<TreeItem>,
    tree_truncated: bool,
    tree_loading: bool,
    tree_err: Option<String>,
    tree_p: Option<Promise<Result<TreePayload, String>>>,
    exp: explorer::ExplorerState,

    // editor
    tabs: Vec<OpenTab>,
    active_tab: Option<usize>,
    file_cache: HashMap<String, String>,
    file_inflight: HashSet<String>,
    file_ps: HashMap<String, Promise<Result<(String, String), String>>>,
    hl_cache: Option<(String, u64, Vec<Vec<Span>>)>,
    file_err: Option<String>,

    // workflows
    runs: Vec<RunRow>,
    runs_loading: bool,
    runs_err: Option<String>,
    runs_p: Option<Promise<Result<Vec<RunRow>, String>>>,
    jobs: Vec<JobRow>,
    jobs_run: Option<u64>,
    jobs_label: String,
    jobs_loading: bool,
    jobs_err: Option<String>,
    jobs_p: Option<Promise<Result<(u64, Vec<JobRow>), String>>>,
    dispatch_p: Option<Promise<Result<String, String>>>,

    // releases / tags
    releases: Vec<ReleaseRow>,
    releases_loading: bool,
    releases_err: Option<String>,
    releases_p: Option<Promise<Result<Vec<ReleaseRow>, String>>>,
    tags: Vec<TagRow>,
    tags_loading: bool,
    tags_err: Option<String>,
    tags_p: Option<Promise<Result<Vec<TagRow>, String>>>,

    // pulls
    pulls: Vec<net::PullRow>,
    pulls_loading: bool,
    pulls_err: Option<String>,
    pulls_p: Option<Promise<Result<Vec<net::PullRow>, String>>>,

    // search
    search_q: String,
    search_last_q: String,
    local_hits: Vec<SearchHit>,
    gh_hits: Vec<SearchHit>,
    gh_busy: bool,
    gh_err: Option<String>,
    code_p: Option<Promise<Result<Vec<SearchHit>, String>>>,

    // templates panel
    tpl_form: templates_panel::TemplatesForm,
    tpl_progress: Arc<Mutex<Vec<FileProgress>>>,
    tpl_installing: bool,
    tpl_err: Option<String>,
    tpl_p: Option<Promise<Result<Vec<FileProgress>, String>>>,

    // pages panel
    pages_form: pages_panel::PagesForm,
    pages_info: Option<PagesStatus>,
    pages_busy: Option<&'static str>,
    pages_err: Option<String>,
    pages_progress: Arc<Mutex<Vec<FileProgress>>>,
    pages_p: Option<Promise<Result<PagesStatus, String>>>,
    pages_gen_p: Option<Promise<Result<Vec<FileProgress>, String>>>,

    // repo panel
    repo_form: repo_panel::RepoForm,
    repo_overview: Option<RepoOverview>,
    repo_edits: RepoEdits,
    repo_community: Option<CommunityHealth>,
    repo_loading: bool,
    repo_busy: Option<&'static str>,
    repo_err: Option<String>,
    rpanel_p: Option<Promise<Result<(RepoOverview, Option<CommunityHealth>), String>>>,
    repo_save_p: Option<Promise<Result<String, String>>>,
    repo_scaffold_progress: Arc<Mutex<Vec<FileProgress>>>,
    repo_scaffold_p: Option<Promise<Result<Vec<FileProgress>, String>>>,

    // palette
    palette_open: bool,
    palette_cmds: bool,
    palette_q: String,
    palette_focus: bool,

    status: String,
    cache_hits: usize,
}

impl IdeApp {
    pub fn new(db: Arc<Mutex<SuiteDb>>, selftest: bool) -> Self {
        let db_path = db_lock(&db).path().display().to_string();

        // Everything in the vault was probed by `octocrab-cli auth check`;
        // read it synchronously (local SQLite, microseconds).
        let (mods, orgs, login_meta, kind_meta, label) = {
            let d = db_lock(&db);
            (
                d.modules(),
                d.orgs(),
                d.get_meta("auth_login"),
                d.get_meta("auth_kind"),
                d.active_label(),
            )
        };
        let granted = mods.iter().filter(|m| m.enabled).count();

        let octo = db_lock(&db).pipe(|d| octocrab_core::client::from_db(d).ok().map(|(c, _)| c));

        let mut app = IdeApp {
            db,
            db_path,
            selftest,
            frames: 0,
            octo,
            login: login_meta,
            token_kind: kind_meta,
            token_label: label,
            net_err: None,
            identity_p: None,
            repo_p: None,
            repo: None,
            mods,
            orgs: orgs
                .into_iter()
                .map(|(login, role, read, write, admin)| (login, role, read, write, admin))
                .collect(),
            granted,
            activity: Activity::Explorer,
            sidebar_open: true,
            center: Center::Editor,
            branch: "main".into(),
            source: Source::Local,
            tree: vec![],
            tree_truncated: false,
            tree_loading: false,
            tree_err: None,
            tree_p: None,
            exp: explorer::ExplorerState::default(),
            tabs: vec![],
            active_tab: None,
            file_cache: HashMap::new(),
            file_inflight: HashSet::new(),
            file_ps: HashMap::new(),
            hl_cache: None,
            file_err: None,
            runs: vec![],
            runs_loading: false,
            runs_err: None,
            runs_p: None,
            jobs: vec![],
            jobs_run: None,
            jobs_label: String::new(),
            jobs_loading: false,
            jobs_err: None,
            jobs_p: None,
            dispatch_p: None,
            releases: vec![],
            releases_loading: false,
            releases_err: None,
            releases_p: None,
            tags: vec![],
            tags_loading: false,
            tags_err: None,
            tags_p: None,
            pulls: vec![],
            pulls_loading: false,
            pulls_err: None,
            pulls_p: None,
            search_q: String::new(),
            search_last_q: String::new(),
            local_hits: vec![],
            gh_hits: vec![],
            gh_busy: false,
            gh_err: None,
            code_p: None,
            tpl_form: templates_panel::TemplatesForm::default(),
            tpl_progress: Arc::new(Mutex::new(vec![])),
            tpl_installing: false,
            tpl_err: None,
            tpl_p: None,
            pages_form: pages_panel::PagesForm::default(),
            pages_info: None,
            pages_busy: None,
            pages_err: None,
            pages_progress: Arc::new(Mutex::new(vec![])),
            pages_p: None,
            pages_gen_p: None,
            repo_form: repo_panel::RepoForm::default(),
            repo_overview: None,
            repo_edits: RepoEdits::default(),
            repo_community: None,
            repo_loading: false,
            repo_busy: None,
            repo_err: None,
            rpanel_p: None,
            repo_save_p: None,
            repo_scaffold_progress: Arc::new(Mutex::new(vec![])),
            repo_scaffold_p: None,
            palette_open: false,
            palette_cmds: false,
            palette_q: String::new(),
            palette_focus: false,
            status: String::new(),
            cache_hits: 0,
        };

        // local workspace tree immediately (background thread)
        app.ensure_tree();
        if !selftest {
            // identity + default repo (background, 10s timeouts inside)
            // skipped in selftest mode: no network I/O at all
            app.spawn_identity();
            app.spawn_repo_fetch();
        }
        app
    }

    // ---------------------------------------------------------- plumbing

    fn module_enabled(&self, key: &str) -> bool {
        self.mods
            .iter()
            .find(|m| m.key == key)
            .map(|m| m.enabled)
            .unwrap_or(false)
    }

    fn spawn_identity(&mut self) {
        let octo = self.octo.clone();
        self.identity_p = Some(spawn(move || match octo {
            None => Err("no active token — run `octocrab-cli auth set`".to_string()),
            Some(octo) => net::block_on(async {
                net::identity(&octo)
                    .await
                    .map_err(|e| format!("identity check failed: {e}"))
            }),
        }));
    }

    fn spawn_repo_fetch(&mut self) {
        let Some(octo) = self.octo.clone() else {
            self.net_err = Some("no active token — GitHub features disabled".into());
            return;
        };
        self.repo_p = Some(spawn(move || {
            net::block_on(async { net::default_repo(&octo).await })
                .map_err(|e| format!("repo lookup failed: {e}"))
        }));
    }

    fn ensure_tree(&mut self) {
        if self.tree_p.is_some() {
            return;
        }
        match self.source {
            Source::Local => {
                self.tree_loading = true;
                self.tree_err = None;
                self.tree_p = Some(spawn(|| {
                    Ok(TreePayload {
                        items: net::walk_local_workspace(LOCAL_ROOT),
                        truncated: false,
                    })
                }));
            }
            Source::Github => {
                if !self.module_enabled("repo:read") {
                    return; // sidebar renders locked_panel
                }
                let Some(repo) = self.repo.clone() else {
                    self.tree_err = Some("waiting for repository…".into());
                    return;
                };
                let Some(octo) = self.octo.clone() else {
                    self.tree_err = Some("offline — no client".into());
                    return;
                };
                let (o, r) = (repo.owner.clone(), repo.name.clone());
                let b = self.branch.clone();
                let key = format!("repo:{o}/{r}:tree:{b}");
                // stale-while-revalidate: render cached tree instantly
                let cached: Option<Vec<TreeItem>> = {
                    let db = db_lock(&self.db);
                    cache::get_json(&db, &key)
                };
                if let Some(items) = cached {
                    self.tree = items;
                    self.cache_hits += 1;
                }
                let db = self.db.clone();
                self.tree_loading = true;
                self.tree_err = None;
                self.tree_p = Some(spawn(move || {
                    match net::block_on(net::fetch_tree(&octo, &o, &r, &b)) {
                        Ok((items, trunc)) => {
                            {
                                let db = db_lock(&db);
                                cache::set_json(&db, &key, &items, TTL_LONG);
                            }
                            Ok(TreePayload { items, truncated: trunc })
                        }
                        Err(e) => Err(format!("{e}")),
                    }
                }));
            }
        }
    }

    fn open_file(&mut self, path: String) {
        let idx = match self.tabs.iter().position(|t| t.path == path) {
            Some(i) => i,
            None => {
                let title = path.rsplit('/').next().unwrap_or(&path).to_string();
                self.tabs.push(OpenTab {
                    path: path.clone(),
                    title,
                    lang: Lang::from_path(&path),
                    loading: false,
                });
                self.tabs.len() - 1
            }
        };
        self.active_tab = Some(idx);
        self.center = Center::Editor;
        self.file_err = None;

        if self.file_cache.contains_key(&path) || self.file_inflight.contains(&path) {
            return;
        }
        match self.source {
            Source::Local => {
                self.file_inflight.insert(path.clone());
                if let Some(t) = self.tabs.iter_mut().find(|t| t.path == path) {
                    t.loading = true;
                }
                self.file_ps.insert(
                    path.clone(),
                    spawn(move || {
                        let p = std::path::Path::new(&path);
                        match std::fs::metadata(p) {
                            Ok(m) if m.len() > 2_000_000 => Err("file larger than 2MB".into()),
                            Ok(_) => std::fs::read_to_string(p)
                                .map(|c| (path.clone(), c))
                                .map_err(|e| format!("read failed: {e}")),
                            Err(e) => Err(format!("stat failed: {e}")),
                        }
                    }),
                );
            }
            Source::Github => {
                let Some(repo) = self.repo.clone() else {
                    self.file_err = Some("no repository selected".into());
                    return;
                };
                if !self.module_enabled("repo:read") {
                    self.file_err = Some("repo:read not granted".into());
                    return;
                }
                let (o, r) = (repo.owner.clone(), repo.name.clone());
                let b = self.branch.clone();
                let key = format!("repo:{o}/{r}:file:{b}:{path}");
                // stale-while-revalidate: instant cached content
                let cached: Option<String> = {
                    let db = db_lock(&self.db);
                    cache::get_json(&db, &key)
                };
                if let Some(content) = cached {
                    self.cache_hits += 1;
                    self.file_cache.insert(path.clone(), content);
                    self.hl_cache = None;
                }
                let Some(octo) = self.octo.clone() else {
                    self.file_err = Some("offline — showing cached content".into());
                    return;
                };
                let db = self.db.clone();
                let p = path.clone();
                self.file_inflight.insert(path.clone());
                if let Some(t) = self.tabs.iter_mut().find(|t| t.path == path) {
                    t.loading = true;
                }
                self.file_ps.insert(
                    path.clone(),
                    spawn(move || match net::block_on(net::fetch_file(&octo, &o, &r, &b, &p)) {
                        Ok(content) => {
                            {
                                let db = db_lock(&db);
                                cache::set_json(&db, &key, &content, TTL_LONG);
                            }
                            Ok((p, content))
                        }
                        Err(e) => Err(format!("{e}")),
                    }),
                );
            }
        }
    }

    fn close_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
        }
        self.active_tab = match self.active_tab {
            Some(a) if a == idx => self.tabs.len().checked_sub(1),
            Some(a) if a > idx => Some(a - 1),
            other => other,
        };
        self.hl_cache = None;
    }

    fn branch_options(&self) -> Vec<String> {
        let mut v = vec![self.branch.clone()];
        for b in ["main", "master"] {
            if !v.iter().any(|x| x == b) {
                v.push(b.to_string());
            }
        }
        v
    }

    fn on_branch_changed(&mut self) {
        self.file_cache.clear();
        self.hl_cache = None;
        self.tree.clear();
        self.exp = explorer::ExplorerState::default();
        self.tree_p = None;
        self.ensure_tree();
    }

    // -------------------------------------------------------------- fetch

    fn ensure_runs(&mut self, force: bool) {
        if self.runs_p.is_some() || !self.module_enabled("workflow:read") {
            return;
        }
        let Some(repo) = self.repo.clone() else { return };
        let (o, r) = (repo.owner.clone(), repo.name.clone());
        let key = format!("runs:{o}/{r}:{RUNS_LIMIT}");
        if !force {
            let cached: Option<Vec<RunRow>> = {
                let db = db_lock(&self.db);
                cache::get_json(&db, &key)
            };
            if let Some(rows) = cached {
                self.runs = rows;
                self.cache_hits += 1;
            }
        }
        let Some(octo) = self.octo.clone() else {
            if self.runs.is_empty() {
                self.runs_err = Some("offline — no client".into());
            }
            return;
        };
        self.runs_loading = true;
        self.runs_err = None;
        let db = self.db.clone();
        self.runs_p = Some(spawn(move || {
            match net::block_on(net::fetch_runs(&octo, &o, &r, RUNS_LIMIT)) {
                Ok(rows) => {
                    {
                        let db = db_lock(&db);
                        cache::set_json(&db, &key, &rows, TTL_SHORT);
                    }
                    Ok(rows)
                }
                Err(e) => Err(format!("{e}")),
            }
        }));
    }

    fn ensure_jobs(&mut self, run_id: u64, force: bool) {
        if self.jobs_p.is_some() {
            return;
        }
        self.jobs_run = Some(run_id);
        self.center = Center::RunJobs(run_id);
        let Some(repo) = self.repo.clone() else { return };
        let (o, r) = (repo.owner.clone(), repo.name.clone());
        let key = format!("jobs:{o}/{r}:{run_id}");
        if !force {
            let cached: Option<Vec<JobRow>> = {
                let db = db_lock(&self.db);
                cache::get_json(&db, &key)
            };
            if let Some(rows) = cached {
                self.jobs = rows;
                self.jobs_loading = false;
                self.cache_hits += 1;
            }
        }
        let Some(octo) = self.octo.clone() else {
            self.jobs_err = Some("offline — no client".into());
            return;
        };
        self.jobs_loading = true;
        self.jobs_err = None;
        let db = self.db.clone();
        self.jobs_p = Some(spawn(move || {
            match net::block_on(net::fetch_jobs(&octo, &o, &r, run_id)) {
                Ok(rows) => {
                    {
                        let db = db_lock(&db);
                        cache::set_json(&db, &key, &rows, TTL_SHORT);
                    }
                    Ok((run_id, rows))
                }
                Err(e) => Err(format!("{e}")),
            }
        }));
    }

    fn ensure_releases(&mut self, force: bool) {
        if self.releases_p.is_some() || !self.module_enabled("repo:read") {
            return;
        }
        let Some(repo) = self.repo.clone() else { return };
        let (o, r) = (repo.owner.clone(), repo.name.clone());
        let key = format!("releases:{o}/{r}:30");
        if !force {
            let cached: Option<Vec<ReleaseRow>> = {
                let db = db_lock(&self.db);
                cache::get_json(&db, &key)
            };
            if let Some(rows) = cached {
                self.releases = rows;
                self.cache_hits += 1;
            }
        }
        let Some(octo) = self.octo.clone() else { return };
        self.releases_loading = true;
        self.releases_err = None;
        let db = self.db.clone();
        self.releases_p = Some(spawn(move || {
            match net::block_on(net::fetch_releases(&octo, &o, &r, 30)) {
                Ok(rows) => {
                    {
                        let db = db_lock(&db);
                        cache::set_json(&db, &key, &rows, TTL_MEDIUM);
                    }
                    Ok(rows)
                }
                Err(e) => Err(format!("{e}")),
            }
        }));
    }

    fn ensure_tags(&mut self, force: bool) {
        if self.tags_p.is_some() || !self.module_enabled("repo:read") {
            return;
        }
        let Some(repo) = self.repo.clone() else { return };
        let (o, r) = (repo.owner.clone(), repo.name.clone());
        let key = format!("tags:{o}/{r}:50");
        if !force {
            let cached: Option<Vec<TagRow>> = {
                let db = db_lock(&self.db);
                cache::get_json(&db, &key)
            };
            if let Some(rows) = cached {
                self.tags = rows;
                self.cache_hits += 1;
            }
        }
        let Some(octo) = self.octo.clone() else { return };
        self.tags_loading = true;
        self.tags_err = None;
        let db = self.db.clone();
        self.tags_p = Some(spawn(move || {
            match net::block_on(net::fetch_tags(&octo, &o, &r, 50)) {
                Ok(rows) => {
                    {
                        let db = db_lock(&db);
                        cache::set_json(&db, &key, &rows, TTL_MEDIUM);
                    }
                    Ok(rows)
                }
                Err(e) => Err(format!("{e}")),
            }
        }));
    }

    fn ensure_pulls(&mut self, force: bool) {
        if self.pulls_p.is_some() || !self.module_enabled("prs:read") {
            return;
        }
        let Some(repo) = self.repo.clone() else { return };
        let (o, r) = (repo.owner.clone(), repo.name.clone());
        let key = format!("pulls:{o}/{r}:30");
        if !force {
            let cached: Option<Vec<net::PullRow>> = {
                let db = db_lock(&self.db);
                cache::get_json(&db, &key)
            };
            if let Some(rows) = cached {
                self.pulls = rows;
                self.pulls_loading = false;
                self.cache_hits += 1;
            }
        }
        let Some(octo) = self.octo.clone() else { return };
        self.pulls_loading = true;
        self.pulls_err = None;
        let db = self.db.clone();
        self.pulls_p = Some(spawn(move || {
            match net::block_on(net::fetch_pulls(&octo, &o, &r)) {
                Ok(rows) => {
                    {
                        let db = db_lock(&db);
                        cache::set_json(&db, &key, &rows, TTL_SHORT);
                    }
                    Ok(rows)
                }
                Err(e) => Err(format!("{e}")),
            }
        }));
    }

    fn start_code_search(&mut self) {
        if self.gh_busy {
            return;
        }
        if !self.module_enabled("repo:read") {
            self.gh_err = Some("repo:read not granted".into());
            return;
        }
        let Some(repo) = self.repo.clone() else {
            self.gh_err = Some("no repository selected".into());
            return;
        };
        let Some(octo) = self.octo.clone() else {
            self.gh_err = Some("offline — no client".into());
            return;
        };
        let q = self.search_q.trim().to_string();
        if q.is_empty() {
            return;
        }
        let (o, r) = (repo.owner.clone(), repo.name.clone());
        self.gh_busy = true;
        self.gh_err = None;
        self.code_p = Some(spawn(move || {
            net::block_on(net::code_search(&octo, &o, &r, &q)).map_err(|e| format!("{e}"))
        }));
    }

    // --------------------------------------------------- templates panel

    /// Prefill panel target fields from the resolved repository (kept
    /// non-destructive: only fills empty fields).
    fn prefill_panel_forms(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        let full = format!("{}/{}", repo.owner, repo.name);
        if self.tpl_form.repo.trim().is_empty() {
            self.tpl_form.repo = full.clone();
        }
        if self.tpl_form.project.trim().is_empty() {
            self.tpl_form.project = repo.name.clone();
        }
        if self.tpl_form.tagline.trim().is_empty() {
            self.tpl_form.tagline = self
                .repo_overview
                .as_ref()
                .map(|ov| ov.description.clone())
                .unwrap_or_default();
        }
        if self.pages_form.repo.trim().is_empty() {
            self.pages_form.repo = full.clone();
        }
        if self.repo_form.repo.trim().is_empty() {
            self.repo_form.repo = full;
        }
    }

    fn on_template_selected(&mut self, idx: usize) {
        self.tpl_form.selected = idx;
        self.tpl_err = None;
        self.prefill_panel_forms();
    }

    /// Render the selected template and push every file (background).
    fn install_template(&mut self) {
        self.tpl_err = None;
        let def = templates_panel::selected_def(&self.tpl_form);
        if !self.module_enabled(def.gate) {
            self.tpl_err =
                Some(format!("module `{}` is not granted — cannot install `{}`", def.gate, def.key));
            return;
        }
        let Some(octo) = self.octo.clone() else {
            self.tpl_err = Some("offline — no GitHub client".into());
            return;
        };
        let Some((o, r)) = widgets::parse_repo(&self.tpl_form.repo) else {
            self.tpl_err = Some("repo must look like OWNER/REPO".into());
            return;
        };
        if self.tpl_installing {
            return;
        }
        let (description, license, contact) = match &self.repo_overview {
            Some(ov) => (ov.description.clone(), ov.license.clone(), String::new()),
            None => (String::new(), "MIT".to_string(), String::new()),
        };
        let params = templates_panel::build_params(
            &self.tpl_form,
            &o,
            &r,
            &description,
            &license,
            &contact,
            &self.branch,
        );
        let key = def.key.to_string();
        let message = format!("octocrab-ide: install template `{key}`");
        let progress = self.tpl_progress.clone();
        self.tpl_installing = true;
        self.status = format!("installing template `{key}` into {o}/{r}…");
        self.tpl_p = Some(spawn(move || {
            let files = match octocrab_core::templates::render(&key, &params) {
                Ok(files) => files,
                Err(e) => return Err(format!("render failed: {e}")),
            };
            Ok(push_files_sync(octo, o, r, files, message, progress))
        }));
    }

    // ------------------------------------------------------ pages panel

    fn pages_target(&self) -> Result<(String, String), String> {
        widgets::parse_repo(&self.pages_form.repo)
            .ok_or_else(|| "repo must look like OWNER/REPO".to_string())
    }

    fn pages_op(&mut self, op: PagesOp) {
        self.pages_err = None;
        let Some(octo) = self.octo.clone() else {
            self.pages_err = Some("offline — no GitHub client".into());
            return;
        };
        let (o, r) = match self.pages_target() {
            Ok(t) => t,
            Err(e) => {
                self.pages_err = Some(e);
                return;
            }
        };
        if self.pages_busy.is_some() {
            return;
        }
        match op {
            PagesOp::Status => {
                self.pages_busy = Some("fetching pages status");
                self.pages_p = Some(spawn(move || {
                    net::block_on(net::pages_status(&octo, &o, &r)).map_err(|e| format!("{e}"))
                }));
            }
            PagesOp::Enable => {
                if !self.module_enabled("repo:admin") {
                    self.pages_err = Some("repo:admin not granted — cannot enable Pages".into());
                    return;
                }
                self.pages_busy = Some("enabling Pages");
                self.pages_p = Some(spawn(move || {
                    net::block_on(net::pages_enable(&octo, &o, &r)).map_err(|e| format!("{e}"))
                }));
            }
            PagesOp::Build => {
                if !self.module_enabled("repo:write") {
                    self.pages_err = Some("repo:write not granted — cannot request builds".into());
                    return;
                }
                self.pages_busy = Some("requesting build");
                self.pages_p = Some(spawn(move || {
                    net::block_on(net::pages_build(&octo, &o, &r)).map_err(|e| format!("{e}"))
                }));
            }
        }
    }

    /// Render the `site` template from live repo data + push site/ files.
    fn generate_site(&mut self) {
        self.pages_err = None;
        let Some(octo) = self.octo.clone() else {
            self.pages_err = Some("offline — no GitHub client".into());
            return;
        };
        let (o, r) = match self.pages_target() {
            Ok(t) => t,
            Err(e) => {
                self.pages_err = Some(e);
                return;
            }
        };
        if !self.module_enabled("repo:write") {
            self.pages_err = Some("repo:write not granted — cannot generate the site".into());
            return;
        }
        if self.pages_busy.is_some() {
            return;
        }
        let progress = self.pages_progress.clone();
        self.pages_busy = Some("generating site");
        self.status = format!("generating Pages site for {o}/{r}…");
        self.pages_gen_p = Some(spawn(move || {
            let files = match net::block_on(net::site_files(&octo, &o, &r)) {
                Ok(files) => files,
                Err(e) => return Err(format!("site render failed: {e}")),
            };
            Ok(push_files_sync(
                octo,
                o.clone(),
                r.clone(),
                files,
                "docs: generate GitHub Pages landing site (octocrab-ide)".into(),
                progress,
            ))
        }));
    }

    // ------------------------------------------------------- repo panel

    fn repo_target(&self) -> Result<(String, String), String> {
        widgets::parse_repo(&self.repo_form.repo)
            .ok_or_else(|| "repo must look like OWNER/REPO".to_string())
    }

    /// Load overview + community health (one background thread, two GETs).
    fn repo_load(&mut self) {
        self.repo_err = None;
        let Some(octo) = self.octo.clone() else {
            self.repo_err = Some("offline — no GitHub client".into());
            return;
        };
        let (o, r) = match self.repo_target() {
            Ok(t) => t,
            Err(e) => {
                self.repo_err = Some(e);
                return;
            }
        };
        if self.repo_loading || self.repo_busy.is_some() {
            return;
        }
        self.repo_loading = true;
        self.status = format!("loading {o}/{r}…");
        self.rpanel_p = Some(spawn(move || {
            let overview = net::block_on(net::repo_overview(&octo, &o, &r))
                .map_err(|e| format!("{e}"))?;
            // community profile is best-effort (404 → None)
            let community = net::block_on(net::community_profile(&octo, &o, &r)).ok();
            Ok((overview, community))
        }));
    }

    /// PATCH /repos + PUT /topics with the edited fields (repo:admin).
    fn repo_save(&mut self) {
        self.repo_err = None;
        let Some(octo) = self.octo.clone() else {
            self.repo_err = Some("offline — no GitHub client".into());
            return;
        };
        if !self.module_enabled("repo:admin") {
            self.repo_err = Some("repo:admin not granted — cannot save settings".into());
            return;
        }
        let (o, r) = match self.repo_target() {
            Ok(t) => t,
            Err(e) => {
                self.repo_err = Some(e);
                return;
            }
        };
        if self.repo_busy.is_some() || self.repo_loading {
            return;
        }
        let description = self.repo_edits.description.trim().to_string();
        let homepage = self.repo_edits.homepage.trim().to_string();
        let topics = widgets::parse_list(&self.repo_edits.topics);
        self.repo_busy = Some("saving settings");
        self.status = format!("saving {o}/{r} settings…");
        self.repo_save_p = Some(spawn(move || {
            net::block_on(net::save_repo_settings(&octo, &o, &r, &description, &homepage, &topics))
                .map_err(|e| format!("{e}"))
        }));
    }

    /// Render the `community` template + push every file (repo:write).
    fn repo_scaffold(&mut self) {
        self.repo_err = None;
        let Some(octo) = self.octo.clone() else {
            self.repo_err = Some("offline — no GitHub client".into());
            return;
        };
        if !self.module_enabled("repo:write") {
            self.repo_err = Some("repo:write not granted — cannot scaffold community files".into());
            return;
        }
        let (o, r) = match self.repo_target() {
            Ok(t) => t,
            Err(e) => {
                self.repo_err = Some(e);
                return;
            }
        };
        if self.repo_busy.is_some() || self.repo_loading {
            return;
        }
        let (description, license) = match &self.repo_overview {
            Some(ov) => (ov.description.clone(), ov.license.clone()),
            None => (String::new(), "MIT".to_string()),
        };
        let params = octocrab_core::templates::TemplateParams {
            project: r.clone(),
            owner: o.clone(),
            repo: r.clone(),
            description,
            license,
            contact: self.login.clone().unwrap_or_default(),
            ..Default::default()
        };
        let progress = self.repo_scaffold_progress.clone();
        self.repo_busy = Some("scaffolding community files");
        self.status = format!("scaffolding community files in {o}/{r}…");
        self.repo_scaffold_p = Some(spawn(move || {
            let files = match octocrab_core::templates::render("community", &params) {
                Ok(files) => files,
                Err(e) => return Err(format!("render failed: {e}")),
            };
            Ok(push_files_sync(
                octo,
                o.clone(),
                r.clone(),
                files,
                "docs: scaffold community health files (octocrab-ide)".into(),
                progress,
            ))
        }));
    }

    fn dispatch_first_workflow(&mut self) {
        if !self.module_enabled("workflow:write") {
            self.status = "workflow:write not granted".into();
            return;
        }
        let Some(repo) = self.repo.clone() else { return };
        let Some(octo) = self.octo.clone() else { return };
        if self.dispatch_p.is_some() {
            return;
        }
        let (o, r) = (repo.owner.clone(), repo.name.clone());
        let git_ref = self.branch.clone();
        self.status = "dispatching workflow…".into();
        self.dispatch_p = Some(spawn(move || {
            let wfs = net::block_on(net::list_workflows(&octo, &o, &r)).map_err(|e| format!("{e}"))?;
            let (path, name) = wfs
                .first()
                .cloned()
                .ok_or_else(|| "no workflows found in repo".to_string())?;
            net::block_on(net::dispatch_workflow(&octo, &o, &r, &path, &git_ref))
                .map_err(|e| format!("{e}"))?;
            Ok(format!("dispatched `{name}` ({path}) on {git_ref}"))
        }));
    }

    fn refresh_all(&mut self) {
        self.tree_p = None;
        self.tree_loading = false;
        self.tree_err = None;
        self.ensure_tree();
        self.runs_p = None;
        self.ensure_runs(true);
        self.releases_p = None;
        self.ensure_releases(true);
        self.tags_p = None;
        self.ensure_tags(true);
        self.pulls_p = None;
        self.ensure_pulls(true);
        if let Some(run) = self.jobs_run {
            self.jobs_p = None;
            self.ensure_jobs(run, true);
        }
        self.status = "refreshed".into();
    }

    fn on_activity_changed(&mut self) {
        match self.activity {
            Activity::Workflows => self.ensure_runs(false),
            Activity::Releases => {
                self.ensure_releases(false);
                self.ensure_tags(false);
            }
            Activity::Pulls => self.ensure_pulls(false),
            Activity::Account => self.center = Center::Modules,
            Activity::Templates | Activity::Pages | Activity::Repo => self.prefill_panel_forms(),
            _ => {}
        }
    }

    // -------------------------------------------------------------- poll

    fn poll_promises(&mut self) {
        // identity
        if let Some(res) = take_owned(&mut self.identity_p) {
                match res {
                Ok(login) => {
                    self.login = Some(login.clone());
                    self.net_err = None;
                    self.status = format!("signed in as {login}");
                }
                Err(e) => self.net_err = Some(e.to_string()),
            }
        }
        // default repo
        if let Some(res) = take_owned(&mut self.repo_p) {
                match res {
                Ok(info) => {
                    let changed = self
                        .repo
                        .as_ref()
                        .map(|r| {
                            r.owner != info.owner
                                || r.name != info.name
                                || r.default_branch != info.default_branch
                        })
                        .unwrap_or(true);
                    if changed {
                        self.branch = info.default_branch.clone();
                        let had_repo = self.repo.is_some();
                        self.repo = Some(info.clone());
                        self.file_cache.clear();
                        self.hl_cache = None;
                        if had_repo || self.source == Source::Github {
                            self.tree.clear();
                            self.exp = explorer::ExplorerState::default();
                            self.tree_p = None;
                            self.ensure_tree();
                        }
                        self.status = "repository resolved — caches warm".into();
                    }
                }
                Err(e) => self.net_err = Some(e.to_string()),
            }
        }
        // tree
        if let Some(res) = take_owned(&mut self.tree_p) {
                match res {
                Ok(payload) => {
                    self.tree_loading = false;
                    self.tree_err = None;
                    self.tree = payload.items.clone();
                    self.tree_truncated = payload.truncated;
                }
                Err(e) => {
                    self.tree_loading = false;
                    self.tree_err = Some(e);
                }
            }
        }
        // files (many in flight)
        let ready: Vec<String> = self
            .file_ps
            .iter()
            .filter(|(_, p)| p.ready().is_some())
            .map(|(k, _)| k.clone())
            .collect();
        for path in ready {
            if let Some(p) = self.file_ps.remove(&path) {
                let res = p.ready().cloned();
                self.file_inflight.remove(&path);
                match res {
                    Some(Ok((p2, content))) => {
                        self.file_cache.insert(p2.clone(), content);
                        if let Some(t) = self.tabs.iter_mut().find(|t| t.path == p2) {
                            t.loading = false;
                        }
                        self.hl_cache = None;
                    }
                    Some(Err(e)) => {
                        self.file_err = Some(format!("{path}: {e}"));
                        if let Some(t) = self.tabs.iter_mut().find(|t| t.path == path) {
                            t.loading = false;
                        }
                    }
                    None => {
                        self.file_ps.insert(path, p);
                    }
                }
            }
        }
        // runs
        if let Some(res) = take_owned(&mut self.runs_p) {
                match res {
                Ok(rows) => {
                    self.runs_loading = false;
                    self.runs_err = None;
                    self.runs = rows;
                }
                Err(e) => {
                    self.runs_loading = false;
                    self.runs_err = Some(e);
                }
            }
        }
        // jobs
        if let Some(res) = take_owned(&mut self.jobs_p) {
                match res {
                Ok((run_id, rows)) => {
                    self.jobs_loading = false;
                    self.jobs_err = None;
                    if self.jobs_run == Some(run_id) {
                        self.jobs = rows;
                    }
                }
                Err(e) => {
                    self.jobs_loading = false;
                    self.jobs_err = Some(e);
                }
            }
        }
        // releases
        if let Some(res) = take_owned(&mut self.releases_p) {
                match res {
                Ok(rows) => {
                    self.releases_loading = false;
                    self.releases_err = None;
                    self.releases = rows;
                }
                Err(e) => {
                    self.releases_loading = false;
                    self.releases_err = Some(e);
                }
            }
        }
        // tags
        if let Some(res) = take_owned(&mut self.tags_p) {
                match res {
                Ok(rows) => {
                    self.tags_loading = false;
                    self.tags_err = None;
                    self.tags = rows;
                }
                Err(e) => {
                    self.tags_loading = false;
                    self.tags_err = Some(e);
                }
            }
        }
        // pulls
        if let Some(res) = take_owned(&mut self.pulls_p) {
                match res {
                Ok(rows) => {
                    self.pulls_loading = false;
                    self.pulls_err = None;
                    self.pulls = rows;
                }
                Err(e) => {
                    self.pulls_loading = false;
                    self.pulls_err = Some(e);
                }
            }
        }
        // code search
        if let Some(res) = take_owned(&mut self.code_p) {
                match res {
                Ok(hits) => {
                    self.gh_busy = false;
                    self.gh_err = None;
                    self.gh_hits = hits;
                }
                Err(e) => {
                    self.gh_busy = false;
                    self.gh_err = Some(e);
                }
            }
        }
        // dispatch
        if let Some(res) = take_owned(&mut self.dispatch_p) {
                match res {
                Ok(msg) => {
                    self.status = msg;
                    self.runs_p = None;
                    self.ensure_runs(true);
                }
                Err(e) => self.status = format!("dispatch failed: {e}"),
            }
        }
        // templates install
        if let Some(res) = take_owned(&mut self.tpl_p) {
            self.tpl_installing = false;
            match res {
                Ok(rows) => {
                    let okc = rows.iter().filter(|r| r.ok).count();
                    self.status = format!("template install finished: {okc}/{} file(s) pushed", rows.len());
                }
                Err(e) => self.tpl_err = Some(e),
            }
        }
        // pages status/enable/build
        if let Some(res) = take_owned(&mut self.pages_p) {
            self.pages_busy = None;
            match res {
                Ok(st) => {
                    self.status = if st.enabled {
                        format!("pages: {} ({})", st.html_url, st.build_status)
                    } else {
                        "pages: not enabled".into()
                    };
                    self.pages_info = Some(st);
                    self.pages_err = None;
                }
                Err(e) => self.pages_err = Some(e),
            }
        }
        // pages site generation
        if let Some(res) = take_owned(&mut self.pages_gen_p) {
            self.pages_busy = None;
            match res {
                Ok(rows) => {
                    let okc = rows.iter().filter(|r| r.ok).count();
                    self.status = format!("site generation finished: {okc}/{} file(s) pushed", rows.len());
                }
                Err(e) => self.pages_err = Some(e),
            }
        }
        // repo overview load
        if let Some(res) = take_owned(&mut self.rpanel_p) {
            self.repo_loading = false;
            match res {
                Ok((ov, community)) => {
                    self.repo_edits.description = ov.description.clone();
                    self.repo_edits.homepage = ov.homepage.clone();
                    self.repo_edits.topics = ov.topics.join(", ");
                    self.repo_overview = Some(ov);
                    self.repo_community = community;
                    self.repo_err = None;
                    self.status = "repository info loaded".into();
                }
                Err(e) => self.repo_err = Some(e),
            }
        }
        // repo settings save
        if let Some(res) = take_owned(&mut self.repo_save_p) {
            self.repo_busy = None;
            match res {
                Ok(msg) => {
                    self.status = msg;
                    if let Some(ov) = &mut self.repo_overview {
                        ov.description = self.repo_edits.description.trim().to_string();
                        ov.homepage = self.repo_edits.homepage.trim().to_string();
                        ov.topics = widgets::parse_list(&self.repo_edits.topics);
                    }
                }
                Err(e) => self.repo_err = Some(format!("save failed: {e}")),
            }
        }
        // repo community scaffold
        if let Some(res) = take_owned(&mut self.repo_scaffold_p) {
            self.repo_busy = None;
            match res {
                Ok(rows) => {
                    let okc = rows.iter().filter(|r| r.ok).count();
                    self.status = format!("community scaffold finished: {okc}/{} file(s) pushed", rows.len());
                }
                Err(e) => self.repo_err = Some(format!("scaffold failed: {e}")),
            }
        }
    }

    fn any_inflight(&self) -> bool {
        self.identity_p.is_some()
            || self.repo_p.is_some()
            || self.tree_p.is_some()
            || !self.file_ps.is_empty()
            || self.runs_p.is_some()
            || self.jobs_p.is_some()
            || self.releases_p.is_some()
            || self.tags_p.is_some()
            || self.pulls_p.is_some()
            || self.code_p.is_some()
            || self.dispatch_p.is_some()
            || self.tpl_p.is_some()
            || self.pages_p.is_some()
            || self.pages_gen_p.is_some()
            || self.repo_save_p.is_some()
            || self.repo_scaffold_p.is_some()
    }

    // -------------------------------------------------------- shortcuts

    fn handle_shortcuts(&mut self, ctx: &Context) {
        ctx.input(|i| {
            let ctrl = i.modifiers.ctrl;
            if ctrl && i.key_pressed(Key::B) {
                self.sidebar_open = !self.sidebar_open;
            }
            if ctrl && i.key_pressed(Key::P) {
                self.palette_open = true;
                self.palette_q.clear();
                self.palette_cmds = i.modifiers.shift;
                self.palette_focus = true;
            }
            if i.key_pressed(Key::Escape) && self.palette_open {
                self.palette_open = false;
            }
            if ctrl && i.key_pressed(Key::W) {
                if let Some(idx) = self.active_tab {
                    self.close_tab(idx);
                }
            }
        });
    }

    // -------------------------------------------------------- top / bottom

    fn top_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("titlebar")
            .frame(
                egui::Frame::none()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("🐙 Octocrab IDE")
                            .color(theme::ACCENT)
                            .strong()
                            .size(14.0),
                    );
                    ui.separator();
                    match &self.login {
                        Some(l) => {
                            widgets::badge(ui, l, theme::GREEN);
                            if let Some(k) = &self.token_kind {
                                widgets::badge(ui, k, theme::YELLOW);
                            }
                        }
                        None => {
                            if self.net_err.is_some() {
                                widgets::badge(ui, "offline", theme::RED);
                            } else {
                                ui.spinner();
                                ui.label(RichText::new("connecting…").color(theme::DIM).size(11.0));
                            }
                        }
                    }
                    ui.separator();
                    ui.label(
                        RichText::new(format!("{}/{} modules", self.granted, self.mods.len()))
                            .color(theme::DIM)
                            .size(11.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("refresh").clicked() {
                            self.refresh_all();
                        }
                        if self.source == Source::Github {
                            let before = self.branch.clone();
                            let cur = self.branch.clone();
                            egui::ComboBox::from_id_salt("branch-sel")
                                .selected_text(cur)
                                .width(140.0)
                                .show_ui(ui, |ui| {
                                    for b in self.branch_options() {
                                        ui.selectable_value(&mut self.branch, b.clone(), b.clone());
                                    }
                                });
                            if self.branch != before {
                                self.on_branch_changed();
                            }
                        }
                        match (&self.repo, self.source) {
                            (Some(r), Source::Github) => {
                                ui.label(
                                    RichText::new(format!("{}/{}", r.owner, r.name))
                                        .color(theme::TEXT)
                                        .size(12.0),
                                );
                            }
                            (_, Source::Local) => {
                                ui.label(
                                    RichText::new("local workspace")
                                        .color(theme::DIM)
                                        .size(11.0),
                                );
                            }
                            (None, _) => {
                                ui.label(
                                    RichText::new("resolving repository…")
                                        .color(theme::DIM)
                                        .size(11.0),
                                );
                            }
                        }
                    });
                });
            });
    }

    fn bottom_bar(&mut self, ctx: &Context) {
        let granted = self.granted;
        let total = self.mods.len();
        let db_path = self.db_path.clone();
        let status = self.status.clone();
        let cache_hits = self.cache_hits;
        let repo_label = match (&self.repo, self.source) {
            (Some(r), Source::Github) => format!("{}/{} @ {}", r.owner, r.name, self.branch),
            _ => "local workspace".to_string(),
        };
        egui::TopBottomPanel::bottom("statusbar")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(0x00, 0x63, 0xa8))
                    .inner_margin(egui::Margin::symmetric(8.0, 2.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("cache hits: {cache_hits}"))
                            .color(Color32::WHITE)
                            .size(11.0),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(repo_label)
                            .color(Color32::WHITE)
                            .size(11.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{granted}/{total} modules"))
                                .color(Color32::WHITE)
                                .size(11.0),
                        );
                        ui.label(
                            RichText::new(db_path)
                                .color(Color32::from_rgb(0xbf, 0xe0, 0xfa))
                                .size(10.0),
                        );
                        if !status.is_empty() {
                            ui.label(
                                RichText::new(status)
                                    .color(Color32::WHITE)
                                    .size(11.0),
                            );
                        }
                    });
                });
            });
    }

    // ---------------------------------------------------- activity + sidebar

    fn activity_bar(&mut self, ctx: &Context) {
        egui::SidePanel::left("activity")
            .exact_width(46.0)
            .frame(egui::Frame::none().fill(theme::SIDEBAR))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let items = [
                        ("🗂", "Explorer", Activity::Explorer),
                        ("🔍", "Search", Activity::Search),
                        ("🌿", "Source Control (open PRs)", Activity::Pulls),
                        ("⚙", "Workflows", Activity::Workflows),
                        ("🏷", "Releases & Tags", Activity::Releases),
                        ("🧩", "Templates", Activity::Templates),
                        ("🌐", "GitHub Pages", Activity::Pages),
                        ("🦀", "Repository & Community", Activity::Repo),
                        ("👤", "Account & Modules", Activity::Account),
                    ];
                    for (icon, tip, act) in items {
                        let sel = self.activity == act;
                        let btn = egui::Button::new(
                            RichText::new(icon)
                                .size(16.0)
                                .color(if sel { Color32::WHITE } else { theme::DIM }),
                        )
                        .fill(if sel {
                            theme::ACCENT.gamma_multiply(0.4)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .min_size(egui::vec2(34.0, 32.0));
                        let mut resp = ui.add(btn);
                        resp = resp.on_hover_text(tip);
                        if resp.clicked() {
                            if self.activity != act {
                                self.activity = act;
                                self.on_activity_changed();
                            }
                        }
                    }
                });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    let b = egui::Button::new(RichText::new("▦").size(15.0).color(theme::DIM))
                        .fill(Color32::TRANSPARENT)
                        .min_size(egui::vec2(30.0, 28.0));
                    if ui.add(b).clicked() {
                        self.sidebar_open = !self.sidebar_open;
                    }
                });
            });
    }

    fn sidebar(&mut self, ctx: &Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(330.0)
            .resizable(true)
            .frame(
                egui::Frame::none()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin::same(6.0)),
            )
            .show(ctx, |ui| match self.activity {
                Activity::Explorer => self.sidebar_explorer(ui),
                Activity::Search => self.sidebar_search(ui),
                Activity::Pulls => self.sidebar_pulls(ui),
                Activity::Workflows => self.sidebar_workflows(ui),
                Activity::Releases => self.sidebar_releases(ui),
                Activity::Templates => self.sidebar_templates(ui),
                Activity::Pages => self.sidebar_pages(ui),
                Activity::Repo => self.sidebar_repo(ui),
                Activity::Account => self.sidebar_account(ui),
            });
    }

    fn sidebar_explorer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let mut switch = None;
            if ui
                .selectable_label(self.source == Source::Local, "Local")
                .clicked()
            {
                switch = Some(Source::Local);
            }
            if ui
                .selectable_label(self.source == Source::Github, "GitHub")
                .clicked()
            {
                switch = Some(Source::Github);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("refresh").clicked() {
                    self.tree_p = None;
                    self.ensure_tree();
                }
            });
            if let Some(s) = switch {
                if self.source != s {
                    self.source = s;
                    self.tree.clear();
                    self.exp = explorer::ExplorerState::default();
                    self.tree_p = None;
                    self.ensure_tree();
                }
            }
        });

        if self.source == Source::Github && !self.module_enabled("repo:read") {
            widgets::locked_panel(ui, "repo:read", "browse remote repository files");
            return;
        }
        if let Some(e) = &self.tree_err {
            ui.label(RichText::new(e.clone()).color(theme::RED).size(11.0));
        }
        if self.tree_loading && self.tree.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("loading tree…").color(theme::DIM).size(11.5));
            });
        }
        if self.tree_truncated {
            ui.label(
                RichText::new("⚠ tree truncated by GitHub API (10k entries)")
                    .color(theme::YELLOW)
                    .size(10.5),
            );
        }
        ui.label(
            RichText::new(format!(
                "{} files · {} rows",
                TreeItem::file_count(&self.tree),
                self.tree.len()
            ))
            .size(10.5)
            .color(theme::DIM),
        );

        let mut opened: Option<String> = None;
        let mut exp = std::mem::take(&mut self.exp);
        explorer::show(ui, "Explorer", &self.tree, &mut exp, &mut |p| {
            opened = Some(p);
        });
        self.exp = exp;
        if let Some(p) = opened {
            self.open_file(p);
        }
    }

    fn sidebar_search(&mut self, ui: &mut Ui) {
        // local fuzzy hits (recomputed only when the query changed)
        if self.search_last_q != self.search_q {
            self.search_last_q = self.search_q.clone();
            let ranked = fuzzy::rank(
                &self.search_q,
                self.tree
                    .iter()
                    .filter(|i| i.kind == octocrab_ui_egui::TreeKind::File)
                    .map(|i| i.path.clone()),
            );
            self.local_hits = ranked
                .into_iter()
                .take(50)
                .map(|(_, path)| SearchHit { path, detail: String::new(), gh: false })
                .collect();
        }
        let mut hits: Vec<SearchHit> = self.local_hits.clone();
        hits.extend(self.gh_hits.iter().cloned());

        let mut picked: Option<usize> = None;
        let mut do_gh = false;
        let gh_allowed = self.module_enabled("repo:read") && self.repo.is_some();
        let gh_busy = self.gh_busy;
        let gh_err = self.gh_err.clone();
        search::show(
            ui,
            &mut self.search_q,
            &hits,
            &mut |i| picked = Some(i),
            gh_allowed,
            gh_busy,
            gh_err.as_deref(),
            &mut || do_gh = true,
        );
        if do_gh {
            self.start_code_search();
        }
        if let Some(i) = picked {
            if let Some(h) = hits.get(i) {
                let p = h.path.clone();
                self.open_file(p);
            }
        }
    }

    fn sidebar_pulls(&mut self, ui: &mut Ui) {
        if !self.module_enabled("prs:read") {
            widgets::locked_panel(ui, "prs:read", "list open pull requests");
            return;
        }
        widgets::section_header(ui, "Open pull requests");
        if let Some(e) = &self.pulls_err {
            ui.label(RichText::new(e.clone()).color(theme::RED).size(11.0));
        }
        if self.pulls_loading && self.pulls.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("loading pulls…").color(theme::DIM).size(11.5));
            });
            return;
        }
        if self.pulls.is_empty() {
            ui.label(RichText::new("(no open PRs)").color(theme::DIM).size(11.5));
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for pr in &self.pulls {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("#{}", pr.number))
                                .color(theme::ACCENT)
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(truncate(&pr.title, 34))
                                .color(theme::TEXT)
                                .size(12.0),
                        );
                        if pr.draft {
                            widgets::badge(ui, "draft", theme::DIM);
                        }
                    });
                    ui.label(
                        RichText::new(format!("  {} · {}", pr.author, pr.branch))
                            .size(10.5)
                            .color(theme::DIM),
                    );
                    ui.add_space(2.0);
                }
            });
    }

    fn sidebar_workflows(&mut self, ui: &mut Ui) {
        if !self.module_enabled("workflow:read") {
            widgets::locked_panel(ui, "workflow:read", "list Actions workflow runs");
            return;
        }
        let mut sel: Option<u64> = None;
        let mut refresh = false;
        let mut dispatch = false;
        workflows::runs_list(
            ui,
            &self.runs,
            self.jobs_run,
            self.runs_loading,
            self.module_enabled("workflow:write"),
            &mut |id| sel = Some(id),
            &mut || refresh = true,
            &mut || dispatch = true,
        );
        if let Some(e) = &self.runs_err {
            ui.label(RichText::new(e.clone()).color(theme::RED).size(11.0));
        }
        if refresh {
            self.runs_p = None;
            self.ensure_runs(true);
        }
        if dispatch {
            self.dispatch_first_workflow();
        }
        if let Some(id) = sel {
            let run = self.runs.iter().find(|r| r.id == id);
            self.jobs_label = run
                .map(|r| format!("#{} · {} · {}", r.id, r.name, r.branch))
                .unwrap_or_else(|| format!("#{id}"));
            self.jobs = vec![];
            self.jobs_p = None;
            self.ensure_jobs(id, false);
        }
    }

    fn sidebar_releases(&mut self, ui: &mut Ui) {
        if !self.module_enabled("repo:read") {
            widgets::locked_panel(ui, "repo:read", "list releases and tags");
            return;
        }
        if let Some(e) = &self.releases_err {
            ui.label(RichText::new(e.clone()).color(theme::RED).size(11.0));
        }
        if let Some(e) = &self.tags_err {
            ui.label(RichText::new(e.clone()).color(theme::RED).size(11.0));
        }
        releases::show(ui, &self.releases, self.releases_loading);
        ui.add_space(4.0);
        tags::show(ui, &self.tags, self.tags_loading);
    }

    fn sidebar_templates(&mut self, ui: &mut Ui) {
        let gate = templates_panel::selected_def(&self.tpl_form).gate;
        let gate_ok = self.module_enabled(gate);
        let has_client = self.octo.is_some();
        let installing = self.tpl_installing;
        let progress = progress_snapshot(&self.tpl_progress);
        let err = self.tpl_err.clone();

        let mut selected: Option<usize> = None;
        let mut do_install = false;
        templates_panel::show(
            ui,
            &mut self.tpl_form,
            &progress,
            installing,
            has_client,
            gate_ok,
            err.as_deref(),
            &mut |i| selected = Some(i),
            &mut || do_install = true,
        );
        if let Some(i) = selected {
            self.on_template_selected(i);
        }
        if do_install {
            self.install_template();
        }
    }

    fn sidebar_pages(&mut self, ui: &mut Ui) {
        let has_client = self.octo.is_some();
        let can_admin = self.module_enabled("repo:admin");
        let can_write = self.module_enabled("repo:write");
        let busy = self.pages_busy;
        let err = self.pages_err.clone();
        let progress = progress_snapshot(&self.pages_progress);

        let mut do_status = false;
        let mut do_enable = false;
        let mut do_build = false;
        let mut do_generate = false;
        pages_panel::show(
            ui,
            &mut self.pages_form,
            self.pages_info.as_ref(),
            busy,
            err.as_deref(),
            &progress,
            can_admin,
            can_write,
            has_client,
            &mut || do_status = true,
            &mut || do_enable = true,
            &mut || do_build = true,
            &mut || do_generate = true,
        );
        if do_status {
            self.pages_op(PagesOp::Status);
        }
        if do_enable {
            self.pages_op(PagesOp::Enable);
        }
        if do_build {
            self.pages_op(PagesOp::Build);
        }
        if do_generate {
            self.generate_site();
        }
    }

    fn sidebar_repo(&mut self, ui: &mut Ui) {
        let has_client = self.octo.is_some();
        let can_admin = self.module_enabled("repo:admin");
        let can_write = self.module_enabled("repo:write");
        let loading = self.repo_loading;
        let busy = self.repo_busy;
        let err = self.repo_err.clone();
        let scaffold_progress = progress_snapshot(&self.repo_scaffold_progress);

        let mut do_load = false;
        let mut do_save = false;
        let mut do_scaffold = false;
        repo_panel::show(
            ui,
            &mut self.repo_form,
            self.repo_overview.as_ref(),
            &mut self.repo_edits,
            self.repo_community.as_ref(),
            loading,
            busy,
            err.as_deref(),
            &scaffold_progress,
            can_admin,
            can_write,
            has_client,
            &mut || do_load = true,
            &mut || do_save = true,
            &mut || do_scaffold = true,
        );
        if do_load {
            self.repo_load();
        }
        if do_save {
            self.repo_save();
        }
        if do_scaffold {
            self.repo_scaffold();
        }
    }

    fn sidebar_account(&mut self, ui: &mut Ui) {
        widgets::section_header(ui, "Account");
        ui.horizontal(|ui| {
            match &self.login {
                Some(l) => {
                    widgets::badge(ui, l, theme::GREEN);
                    if let Some(k) = &self.token_kind {
                        widgets::badge(ui, k, theme::YELLOW);
                    }
                }
                None => {
                    ui.label(RichText::new("not signed in").color(theme::DIM));
                }
            }
        });
        if let Some(label) = &self.token_label {
            ui.label(
                RichText::new(format!("token: {label}"))
                    .size(11.0)
                    .color(theme::DIM),
            );
        }
        if let Some(e) = &self.net_err {
            ui.label(RichText::new(e.clone()).color(theme::RED).size(11.0));
        }
        ui.add_space(6.0);
        widgets::section_header(ui, "Modules");
        ui.label(
            RichText::new(format!("{}/{} granted", self.granted, self.mods.len()))
                .size(12.0)
                .color(if self.granted > 0 { theme::GREEN } else { theme::DIM }),
        );
        if ui.button("open module grid").clicked() {
            self.center = Center::Modules;
        }
        ui.add_space(6.0);
        widgets::section_header(ui, "Recent events");
        let events = {
            let db = db_lock(&self.db);
            db.events(8)
        };
        for (_ts, kind, detail) in events {
            ui.label(
                RichText::new(format!("{kind}: {}", truncate(&detail, 42)))
                    .size(10.5)
                    .color(theme::DIM),
            );
        }
    }

    // ------------------------------------------------------------- center

    fn center(&mut self, ctx: &Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::PANEL))
            .show(ctx, |ui| match self.center {
                Center::Modules => {
                    let mods = self.mods.clone();
                    let orgs = self.orgs.clone();
                    modules::show(ui, &mods, &orgs);
                }
                Center::RunJobs(id) => self.center_jobs(ui, id),
                Center::Editor => self.editor_area(ui),
            });
    }

    fn center_jobs(&mut self, ui: &mut Ui, id: u64) {
        ui.horizontal(|ui| {
            if ui.button("← back to editor").clicked() {
                self.center = Center::Editor;
            }
            ui.label(
                RichText::new(format!("run #{id}"))
                    .color(theme::ACCENT)
                    .size(13.0),
            );
            if ui.button("refresh").clicked() {
                self.jobs_p = None;
                self.ensure_jobs(id, true);
            }
        });
        ui.label(
            RichText::new(self.jobs_label.clone())
                .size(11.0)
                .color(theme::DIM),
        );
        if let Some(e) = &self.jobs_err {
            ui.label(RichText::new(e.clone()).color(theme::RED).size(11.5));
        }
        workflows::jobs_view(ui, &self.jobs_label, &self.jobs, self.jobs_loading);
    }

    fn editor_area(&mut self, ui: &mut Ui) {
        if self.tabs.is_empty() {
            editor_tabs::welcome(ui);
            return;
        }
        let mut sel: Option<usize> = None;
        let mut close: Option<usize> = None;
        editor_tabs::tab_bar(
            ui,
            &self.tabs,
            self.active_tab,
            &mut |i| sel = Some(i),
            &mut |i| close = Some(i),
        );
        if let Some(i) = sel {
            self.active_tab = Some(i);
            self.hl_cache = None;
        }
        if let Some(i) = close {
            self.close_tab(i);
            return;
        }
        let Some(idx) = self.active_tab else { return };
        if idx >= self.tabs.len() {
            self.active_tab = self.tabs.len().checked_sub(1);
            return;
        }
        let tab = self.tabs[idx].clone();

        ui.horizontal(|ui| {
            editor_tabs::breadcrumbs(
                ui,
                self.repo.as_ref().map(|r| r.owner.as_str()).filter(|_| {
                    self.source == Source::Github
                }),
                self.repo.as_ref().map(|r| r.name.as_str()).filter(|_| {
                    self.source == Source::Github
                }),
                &tab.path,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:?}", tab.lang))
                        .size(10.0)
                        .color(theme::DIM),
                );
            });
        });
        ui.separator();

        let content = self.file_cache.get(&tab.path).cloned();
        match content {
            Some(content) => {
                let hash = hash_str(&content);
                let needs = !matches!(
                    &self.hl_cache,
                    Some((p, h, _)) if p == &tab.path && *h == hash
                );
                if needs {
                    let lines = highlight::precompute(tab.lang, &content);
                    self.hl_cache = Some((tab.path.clone(), hash, lines));
                }
                if let Some((_, _, lines)) = &self.hl_cache {
                    editor_tabs::code_view_spans(ui, lines);
                }
            }
            None => {
                ui.add_space(20.0);
                if tab.loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new(format!("loading {}…", tab.path))
                                .color(theme::DIM),
                        );
                    });
                } else {
                    ui.label(
                        RichText::new("content not loaded yet")
                            .color(theme::DIM),
                    );
                    if ui.button("load file").clicked() {
                        self.open_file(tab.path.clone());
                    }
                    if let Some(e) = &self.file_err {
                        ui.label(RichText::new(e.clone()).color(theme::RED).size(11.5));
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ palette

    fn palette_items(&self) -> Vec<(Command, String, String)> {
        if !self.palette_cmds {
            // quick open: fuzzy over tree files
            let ranked = fuzzy::rank(
                &self.palette_q,
                self.tree
                    .iter()
                    .filter(|i| i.kind == octocrab_ui_egui::TreeKind::File)
                    .map(|i| i.path.clone()),
            );
            return ranked
                .into_iter()
                .take(30)
                .map(|(_, path)| (Command::ReloadTree, path, String::new())) // placeholder, replaced below
                .collect();
        }
        let commands: Vec<(Command, String, String)> = vec![
            (Command::ToggleSidebar, "Toggle Sidebar (Ctrl+B)".into(), "show or hide the side panel".into()),
            (Command::Goto(Activity::Explorer), "View: Explorer".into(), "switch the sidebar to the file tree".into()),
            (Command::Goto(Activity::Search), "View: Search".into(), "switch the sidebar to search".into()),
            (Command::Goto(Activity::Pulls), "View: Source Control / PRs".into(), "open pull request list".into()),
            (Command::Goto(Activity::Workflows), "View: Workflows".into(), "switch the sidebar to workflow runs".into()),
            (Command::Goto(Activity::Releases), "View: Releases & Tags".into(), "switch the sidebar to releases".into()),
            (Command::Goto(Activity::Templates), "View: Templates".into(), "install CI / release / pages / site / community templates".into()),
            (Command::Goto(Activity::Pages), "View: GitHub Pages".into(), "status, enable, build, generate landing site".into()),
            (Command::Goto(Activity::Repo), "View: Repository & Community".into(), "repo settings + community health + scaffold".into()),
            (Command::Goto(Activity::Account), "View: Account & Modules".into(), "show identity + module grid".into()),
            (Command::ReloadTree, "Reload File Tree".into(), "re-fetch the tree (stale-while-revalidate)".into()),
            (Command::ClearCache, "Clear Cache & Reload".into(), "invalidate all SQLite TTL caches and refetch".into()),
            (Command::RefreshRuns, "Refresh Workflow Runs".into(), "re-fetch Actions runs".into()),
            (Command::DispatchWorkflow, "Workflow: Dispatch First Workflow".into(), "requires workflow:write".into()),
            (Command::OpenModules, "Open Module Grid".into(), "capability matrix in the editor area".into()),
            (Command::SwitchLocal, "Explorer: Local Workspace Mode".into(), "browse /home/z/octocrab-suite".into()),
            (Command::SwitchGithub, "Explorer: GitHub Repo Mode".into(), "browse the remote git tree".into()),
            (Command::Quit, "Quit".into(), "close the IDE".into()),
        ];
        if self.palette_q.is_empty() {
            return commands;
        }
        let mut v: Vec<(i64, (Command, String, String))> = commands
            .into_iter()
            .filter_map(|c| fuzzy::score(&self.palette_q, &c.1).map(|s| (s, c)))
            .collect();
        v.sort_by(|a, b| b.0.cmp(&a.0));
        v.into_iter().map(|(_, c)| c).collect()
    }

    fn apply_command(&mut self, cmd: Command) {
        match cmd {
            Command::ToggleSidebar => self.sidebar_open = !self.sidebar_open,
            Command::Goto(a) => {
                self.activity = a;
                self.on_activity_changed();
            }
            Command::ReloadTree => {
                self.tree_p = None;
                self.ensure_tree();
            }
            Command::ClearCache => {
                {
                    let db = db_lock(&self.db);
                    for prefix in ["repo:", "runs:", "jobs:", "releases:", "tags:", "pulls:"] {
                        cache::invalidate(&db, prefix);
                    }
                }
                self.cache_hits = 0;
                self.refresh_all();
                self.status = "cache cleared".into();
            }
            Command::RefreshRuns => {
                self.runs_p = None;
                self.ensure_runs(true);
            }
            Command::DispatchWorkflow => self.dispatch_first_workflow(),
            Command::OpenModules => self.center = Center::Modules,
            Command::SwitchLocal | Command::SwitchGithub => {
                let s = if matches!(cmd, Command::SwitchLocal) {
                    Source::Local
                } else {
                    Source::Github
                };
                self.source = s;
                self.activity = Activity::Explorer;
                self.tree.clear();
                self.exp = explorer::ExplorerState::default();
                self.tree_p = None;
                self.ensure_tree();
            }
            Command::Quit => {
                // graceful viewport close
                // (handled by caller via ViewportCommand)
            }
        }
    }

    fn palette(&mut self, ctx: &Context) {
        if !self.palette_open {
            self.palette_focus = false;
            return;
        }
        let cmds_mode = self.palette_cmds;
        let items = self.palette_items();
        // quick-open uses a path picker, not commands
        let quick_paths: Vec<String> = if cmds_mode {
            vec![]
        } else {
            items.iter().map(|(c, label, _)| {
                let _ = c;
                label.clone()
            }).collect()
        };

        let mut open = true;
        let mut picked: Option<usize> = None;
        let mut apply_idx: Option<usize> = None;

        egui::Window::new(if cmds_mode {
            "⌘ Command Palette (Ctrl+Shift+P)"
        } else {
            "Quick Open (Ctrl+P)"
        })
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 40.0])
        .default_width(580.0)
        .frame(
            egui::Frame::none()
                .fill(theme::SIDEBAR)
                .inner_margin(egui::Margin::same(8.0))
                .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                .rounding(6.0),
        )
        .show(ctx, |ui| {
            let resp = ui
                .add(
                    egui::TextEdit::singleline(&mut self.palette_q)
                        .hint_text(if cmds_mode {
                            "type a command…"
                        } else {
                            "type a file path…"
                        })
                        .desired_width(540.0),
                )
                .request_focus();
            let _ = resp;
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(340.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if cmds_mode {
                        for (i, (_cmd, label, detail)) in items.iter().enumerate() {
                            let r = ui.selectable_label(i == 0, RichText::new(label.clone()).size(12.5));
                            if r.clicked() {
                                picked = Some(i);
                            }
                            if !detail.is_empty() {
                                r.on_hover_text(detail.clone());
                            }
                        }
                    } else {
                        for (i, path) in quick_paths.iter().enumerate() {
                            let r = ui.selectable_label(i == 0, RichText::new(path.clone()).size(12.0));
                            if r.clicked() {
                                apply_idx = Some(i);
                            }
                        }
                    }
                });
            ui.label(
                RichText::new(format!(
                    "{} results · Enter picks the first · Esc closes",
                    if cmds_mode { items.len() } else { quick_paths.len() }
                ))
                .size(10.5)
                .color(theme::DIM),
            );
        });

        if !open {
            self.palette_open = false;
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) {
            if !items.is_empty() {
                if cmds_mode {
                    picked = Some(0);
                } else {
                    apply_idx = Some(0);
                }
            }
            self.palette_open = false;
        }
        if let Some(i) = picked {
            if let Some((cmd, _, _)) = items.into_iter().nth(i) {
                self.apply_command(cmd);
            }
        }
        if let Some(i) = apply_idx {
            if let Some(p) = quick_paths.get(i).cloned() {
                self.open_file(p);
            }
        }
    }

    // ------------------------------------------------------------- update

    fn any_inflight_bool(&self) -> bool {
        self.any_inflight()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

trait DbExt {
    fn pipe<T>(&self, f: impl FnOnce(&SuiteDb) -> T) -> T;
}

impl DbExt for MutexGuard<'_, SuiteDb> {
    fn pipe<T>(&self, f: impl FnOnce(&SuiteDb) -> T) -> T {
        f(self)
    }
}

impl eframe::App for IdeApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        theme::apply(ctx);
        self.handle_shortcuts(ctx);
        self.poll_promises();

        self.top_bar(ctx);
        self.bottom_bar(ctx);
        self.activity_bar(ctx);
        if self.sidebar_open {
            self.sidebar(ctx);
        }
        self.center(ctx);
        self.palette(ctx);

        if self.selftest {
            // Scripted, fully offline: canned repository + panel switches.
            // The offline render assertions live in `selftest.rs` (run from
            // main.rs before eframe starts); this loop exercises the real
            // app frame flow end-to-end under xvfb.
            match self.frames {
                3 => {
                    self.repo = Some(net::RepoInfo {
                        owner: "octocrab".into(),
                        name: "showcase".into(),
                        default_branch: "main".into(),
                    });
                    self.repo_overview = Some(RepoOverview {
                        description: "canned showcase repo (selftest)".into(),
                        homepage: "https://example.test/showcase".into(),
                        topics: vec!["rust".into(), "egui".into()],
                        stars: 42,
                        license: "MIT".into(),
                        default_branch: "main".into(),
                        html_url: "https://github.com/octocrab/showcase".into(),
                    });
                    self.prefill_panel_forms();
                    self.status = "selftest: canned repository selected".into();
                }
                10 => self.activity = Activity::Templates,
                15 => {
                    // templates panel has been rendered for ~5 frames: the
                    // canned repo must have prefilled the target field
                    assert!(
                        self.tpl_form.repo == "octocrab/showcase",
                        "selftest: templates repo field not prefilled (got {:?})",
                        self.tpl_form.repo
                    );
                }
                20 => {
                    self.activity = Activity::Pages;
                    self.pages_info = Some(PagesStatus {
                        enabled: true,
                        html_url: "https://octocrab.github.io/showcase/".into(),
                        build_type: "workflow".into(),
                        status: "built".into(),
                        build_status: "built".into(),
                        error: None,
                    });
                }
                25 => {
                    assert!(
                        self.pages_info.as_ref().is_some_and(|p| p.enabled),
                        "selftest: canned pages info missing"
                    );
                }
                30 => {
                    self.activity = Activity::Repo;
                    self.repo_edits = RepoEdits {
                        description: "canned showcase repo (selftest)".into(),
                        homepage: "https://example.test/showcase".into(),
                        topics: "rust, egui".into(),
                    };
                    self.repo_community = Some(CommunityHealth {
                        health_percent: 83,
                        files: vec![
                            ("README".into(), true),
                            ("LICENSE".into(), true),
                            ("CONTRIBUTING".into(), false),
                            ("Code of Conduct".into(), false),
                            ("Issue templates".into(), true),
                            ("PR template".into(), false),
                        ],
                    });
                }
                35 => {
                    assert!(
                        self.repo_form.repo == "octocrab/showcase"
                            && self.repo_community.is_some()
                            && self.repo_edits.topics == "rust, egui",
                        "selftest: repo panel state not prepared"
                    );
                }
                _ => {}
            }
            self.frames += 1;
            ctx.request_repaint();
            if self.frames >= 60 {
                let _ = std::fs::write(
                    SELFTEST_MARKER,
                    format!("ok after {} frames", self.frames),
                );
                std::process::exit(0);
            }
        } else if self.any_inflight_bool() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}
