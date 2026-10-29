
// surface any JS error in the status bar so xvfb screenshots are debuggable
window.onerror = function (msg, src, line, col) {
  try {
    var el = document.getElementById('status-left');
    if (el) el.textContent = 'JS ERROR: ' + msg + ' @' + (src || '') + ':' + line + ':' + col;
  } catch (e) {}
};
window.addEventListener('unhandledrejection', function (ev) {
  try {
    var el = document.getElementById('status-left');
    if (el) el.textContent = 'PROMISE REJECTION: ' + ((ev.reason && ev.reason.message) || ev.reason);
  } catch (e) {}
});
/* Octocrab Control Center — static, dependency-free frontend.
 * All GitHub data arrives via Tauri IPC commands (window.__TAURI__.core.invoke).
 * No CDN, no external fonts, no build step. */
(function () {
  'use strict';

  var T = window.__TAURI__;

  var state = {
    target: null,          // {owner, repo, source}
    section: 'overview',
    auth: null,
    modules: null,
    branch: '',
    limit: 25,
    currentRunId: null,
  };

  var SECTIONS = ['overview', 'workflows', 'runs', 'releases', 'tags', 'repos', 'orgs', 'modules'];

  // ------------------------------------------------------------- plumbing

  function hasTauri() {
    return !!(T && T.core && typeof T.core.invoke === 'function');
  }

  function inv(cmd, args) {
    if (!hasTauri()) {
      return Promise.reject(new Error('Tauri IPC unavailable (window.__TAURI__ missing — run inside the octocrab-tauri window)'));
    }
    return T.core.invoke(cmd, args || {}).then(function (data) {
      if (data && data.error) throw new Error(data.error);
      return data;
    });
  }

  function esc(x) {
    if (x === null || x === undefined) return '';
    return String(x)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }

  function shortSha(sha) { return esc((sha || '').slice(0, 7)); }
  function shortPath(p) { return esc(String(p || '').replace(/^\.github\/workflows\//, '')); }

  function fmtDate(iso) {
    if (!iso) return '—';
    var d = new Date(iso);
    if (isNaN(d.getTime())) return esc(iso);
    return d.toLocaleString();
  }

  function rel(iso) {
    if (!iso) return '';
    var t = new Date(iso).getTime();
    if (isNaN(t)) return '';
    var s = Math.max(0, (Date.now() - t) / 1000);
    if (s < 60) return Math.round(s) + 's ago';
    if (s < 3600) return Math.round(s / 60) + 'm ago';
    if (s < 86400) return Math.round(s / 3600) + 'h ago';
    return Math.round(s / 86400) + 'd ago';
  }

  function durBetween(a, b) {
    if (!a) return '';
    var t0 = new Date(a).getTime();
    if (isNaN(t0)) return '';
    var t1 = b ? new Date(b).getTime() : Date.now();
    if (isNaN(t1)) return '';
    var d = Math.max(0, (t1 - t0) / 1000);
    if (d < 1) return '<1s';
    if (d < 60) return Math.round(d) + 's';
    var m = Math.floor(d / 60);
    var rs = Math.round(d % 60);
    if (m < 60) return m + 'm ' + (rs < 10 ? '0' : '') + rs + 's';
    var h = Math.floor(m / 60);
    return h + 'h ' + (m % 60) + 'm';
  }

  function targetArgs(extra) {
    var a = extra || {};
    if (state.target) {
      a.owner = state.target.owner;
      a.repo = state.target.repo;
    }
    return a;
  }

  function toast(msg, kind) {
    var box = document.getElementById('toasts');
    var el = document.createElement('div');
    el.className = 'toast ' + (kind || '');
    el.textContent = msg;
    box.appendChild(el);
    setTimeout(function () { el.remove(); }, 5200);
  }

  function setLoading(html) {
    return '<div class="loading"><span class="spinner"></span><span>' + esc(html || 'Loading…') + '</span></div>';
  }

  function errorBox(err, retryFnName) {
    return '<div class="error-box"><span>⚠ ' + esc(err && err.message ? err.message : String(err)) + '</span>' +
      (retryFnName ? '<button class="btn small ghost" onclick="' + retryFnName + '()">Retry</button>' : '') +
      '</div>';
  }

  function empty(icon, text, hint) {
    return '<div class="empty"><div class="empty-icon">' + icon + '</div><div class="empty-text">' + esc(text) + '</div>' +
      (hint ? '<div class="empty-hint">' + esc(hint) + '</div>' : '') + '</div>';
  }

  function setContent(html) {
    document.getElementById('content').innerHTML = html;
  }

  // --------------------------------------------------------------- badges

  function modBadge(st) {
    var cls = st === 'granted' ? 'granted' : st === 'denied' ? 'denied' : 'unknown';
    return '<span class="badge ' + cls + '">' + esc(st) + '</span>';
  }

  function runStatusBadge(status, conclusion) {
    var label = conclusion || status || 'unknown';
    var cls = 'unknown';
    if (conclusion === 'success') cls = 'success';
    else if (conclusion === 'failure' || conclusion === 'timed_out' || conclusion === 'action_required') cls = 'failure';
    else if (conclusion === 'cancelled') cls = 'neutral';
    else if (conclusion === 'skipped' || conclusion === 'neutral') cls = 'unknown';
    else if (status === 'in_progress') cls = 'in_progress';
    else if (status === 'queued' || status === 'pending' || status === 'waiting') cls = 'queued';
    return '<span class="badge ' + cls + '">' + esc(label) + '</span>';
  }

  function boolBadge(v, onText, offText) {
    return v ? '<span class="badge granted">' + esc(onText || 'yes') + '</span>'
             : '<span class="badge unknown">' + esc(offText || 'no') + '</span>';
  }

  function stepGlyph(step) {
    var c = step.conclusion, st = step.status;
    if (c === 'success') return ['✓', 'ok'];
    if (c === 'failure' || c === 'timed_out' || c === 'action_required') return ['✗', 'bad'];
    if (c === 'cancelled') return ['⊘', 'warn'];
    if (c === 'skipped' || c === 'neutral') return ['⊘', 'dim'];
    if (st === 'in_progress') return ['●', 'run'];
    if (st === 'queued' || st === 'pending' || st === 'waiting') return ['○', 'wait'];
    if (st === 'completed') return ['·', 'dim'];
    return ['●', 'dim'];
  }

  // -------------------------------------------------------------- sections

  function renderOverview(auth) {
    var a = auth || {};
    var granted = a.granted || 0;
    var total = a.total_modules || 0;
    var pct = total > 0 ? Math.round((granted / total) * 100) : 0;
    var orgs = a.orgs || [];
    var scopes = a.scopes || [];
    var t = state.target;

    var html = '<h2 class="section-title">Overview</h2>' +
      '<p class="section-sub">Control center for the Octocrab Suite — workflows, releases and tags, powered by the encrypted token vault.</p>' +

      '<div class="stat-grid">' +
        '<div class="stat"><div class="stat-value">' + esc(a.login || '—') + '</div><div class="stat-label">identity' + (a.kind ? ' · ' + esc(a.kind) : '') + '</div></div>' +
        '<div class="stat"><div class="stat-value">' + granted + '<span class="dim" style="font-size:14px">/' + total + '</span></div><div class="stat-label">modules granted</div></div>' +
        '<div class="stat"><div class="stat-value">' + orgs.length + '</div><div class="stat-label">organizations</div></div>' +
        '<div class="stat"><div class="stat-value">' + (t && t.owner ? esc(t.owner + '/' + t.repo) : '—') + '</div><div class="stat-label">target repo' + (t && t.source ? ' · ' + esc(t.source) : '') + '</div></div>' +
      '</div>' +

      '<div class="cards">' +
        '<div class="card"><h3>Session</h3><div class="kv">' +
          '<div class="k">login</div><div>' + esc(a.login || 'unknown') + '</div>' +
          '<div class="k">token kind</div><div>' + esc(a.kind || 'unknown') + '</div>' +
          '<div class="k">auth state</div><div>' + esc(a.auth_state || 'unknown') + '</div>' +
          '<div class="k">token label</div><div class="mono">' + esc(a.token_label || '—') + '</div>' +
          '<div class="k">caps checked</div><div>' + (a.caps_checked_at ? esc(fmtDate(new Date(Number(a.caps_checked_at) * 1000).toISOString())) : '—') + '</div>' +
        '</div>' +
        '<div class="progress"><div style="width:' + pct + '%"></div></div>' +
        '<div class="dim" style="font-size:11px">' + pct + '% of capability modules granted</div></div>' +

        '<div class="card"><h3>Scopes</h3>' +
          (scopes.length
            ? '<div class="chips">' + scopes.map(function (sx) { return '<span class="chip on">' + esc(sx) + '</span>'; }).join('') + '</div>'
            : '<div class="dim">No scope header recorded (fine-grained token).</div>') +
        '</div>' +

        '<div class="card"><h3>Organizations</h3>' +
          (orgs.length
            ? '<div class="chips">' + orgs.map(function (o) {
                return '<span class="chip on" title="' + esc(o.role) + '">' + esc(o.login) +
                  (o.admin ? ' ·admin' : o.write ? ' ·write' : '') + '</span>';
              }).join('') + '</div>'
            : '<div class="dim">No organizations on record — the vault org map is empty.</div>') +
        '</div>' +

        '<div class="card"><h3>Quick actions</h3><div class="chips" style="gap:8px">' +
          '<button class="btn" onclick="OC.show(\'workflows\')">Workflows</button>' +
          '<button class="btn" onclick="OC.show(\'runs\')">Runs</button>' +
          '<button class="btn" onclick="OC.show(\'releases\')">Releases</button>' +
          '<button class="btn" onclick="OC.show(\'tags\')">Tags</button>' +
          '<button class="btn ghost" onclick="OC.show(\'modules\')">Modules</button>' +
        '</div><p class="dim" style="margin-top:10px;font-size:12px">Set the target repository in the header bar to scope workflows, runs, releases and tags.</p></div>' +
      '</div>';
    setContent(html);
  }

  function renderWorkflows(data) {
    var rows = (data && data.workflows) || [];
    var head = '<h2 class="section-title">Workflows — ' + esc(data.owner + '/' + data.repo) + '</h2>' +
      '<p class="section-sub">' + rows.length + ' workflow(s) · dispatch requires the <span class="mono">workflow:write</span> module</p>' +
      '<div class="toolbar"><button class="btn small ghost" onclick="OC.loadWorkflows()">⟳ Refresh</button><span class="count">' + rows.length + ' total</span></div>';
    if (!rows.length) {
      setContent(head + empty('⚙', 'No workflows found', 'The repository has no GitHub Actions workflows, or Actions is disabled.'));
      return;
    }
    var body = rows.map(function (w) {
      var state = w.state || 'unknown';
      var cls = state === 'active' ? 'granted' : 'unknown';
      return '<tr>' +
        '<td class="mono dim">' + esc(w.id) + '</td>' +
        '<td><b>' + esc(w.name) + '</b></td>' +
        '<td class="mono dim">' + shortPath(w.path) + '</td>' +
        '<td><span class="badge ' + cls + '">' + esc(state) + '</span></td>' +
        '<td class="nowrap"><button class="btn small" onclick="OC.dispatch(' + esc(JSON.stringify(String(w.id))) + ',\'' + esc(String(w.name).replace(/'/g, '')) + '\')">▶ Run</button></td>' +
      '</tr>';
    }).join('');
    setContent(head + '<table class="data"><thead><tr><th>ID</th><th>Name</th><th>Path</th><th>State</th><th></th></tr></thead><tbody>' + body + '</tbody></table>');
  }

  function renderRuns(data) {
    var rows = (data && data.runs) || [];
    var head = '<h2 class="section-title">Workflow Runs — ' + esc(data.owner + '/' + data.repo) + '</h2>' +
      '<p class="section-sub">Click a run for job &amp; step details</p>' +
      '<div class="toolbar">' +
        '<label>branch</label><input type="text" id="branch-input" value="' + esc(state.branch) + '" placeholder="all branches" style="width:160px" />' +
        '<label>limit</label><select id="limit-sel">' + [10, 25, 50, 100].map(function (n) {
          return '<option' + (state.limit === n ? ' selected' : '') + '>' + n + '</option>';
        }).join('') + '</select>' +
        '<button class="btn small" onclick="OC.applyRunFilters()">Apply</button>' +
        '<button class="btn small ghost" onclick="OC.loadRuns()">⟳ Refresh</button>' +
        '<span class="count">' + rows.length + ' run(s)' + (data.branch ? ' · branch ' + esc(data.branch) : '') + '</span>' +
      '</div>';
    if (!rows.length) {
      setContent(head + empty('▶', 'No workflow runs', 'Trigger a workflow from the Workflows tab or push to the repository.'));
      return;
    }
    var body = rows.map(function (r) {
      return '<tr class="rowlink" onclick="OC.openRun(' + esc(JSON.stringify(r.id)) + ')" title="show jobs & steps">' +
        '<td class="mono">#' + esc(r.run_number) + '</td>' +
        '<td><b>' + esc(r.name) + '</b><div class="dim mono" style="font-size:11px">' + shortSha(r.head_sha) + '</div></td>' +
        '<td class="mono">' + esc(r.head_branch) + '</td>' +
        '<td>' + runStatusBadge(r.status, r.conclusion) + '</td>' +
        '<td>' + esc(r.event) + '</td>' +
        '<td class="dim nowrap">' + fmtDate(r.created_at) + '<div style="font-size:11px">' + esc(rel(r.created_at)) + '</div></td>' +
      '</tr>';
    }).join('');
    setContent(head + '<table class="data"><thead><tr><th>#</th><th>Workflow</th><th>Branch</th><th>Status</th><th>Event</th><th>Started</th></tr></thead><tbody>' + body + '</tbody></table>');
  }

  function renderReleaseCard(r) {
    if (!r) return '';
    return '<div class="card" style="margin-bottom:12px"><h3>' + esc(r.tag_name) + ' — ' + esc(r.name || '(no name)') + '</h3>' +
      '<div class="chips" style="margin-bottom:8px">' +
        '<span class="badge ' + (r.prerelease ? 'queued' : 'success') + '">' + (r.prerelease ? 'prerelease' : 'stable') + '</span>' +
        (r.draft ? '<span class="badge unknown">draft</span>' : '') +
        '<span class="badge info">' + fmtDate(r.published_at) + '</span>' +
      '</div>' +
      (r.body ? '<pre class="body">' + esc(String(r.body).slice(0, 1200)) + '</pre>' : '<div class="dim">No release notes.</div>') +
    '</div>';
  }

  function renderReleases(data) {
    var rows = (data && data.releases) || [];
    var head = '<h2 class="section-title">Releases — ' + esc(data.owner + '/' + data.repo) + '</h2>' +
      '<p class="section-sub">' + rows.length + ' release(s)</p>' +
      '<div class="toolbar"><button class="btn small ghost" onclick="OC.loadReleases()">⟳ Refresh</button>' +
      '<button class="btn small" onclick="OC.loadLatestRelease()">★ Latest</button><span class="count">' + rows.length + ' total</span></div>';
    if (!rows.length) {
      setContent(head + empty('🏷', 'No releases published', 'Create one with: octocrab-cli releases create'));
      return;
    }
    var body = rows.map(function (r) {
      return '<tr>' +
        '<td class="mono"><b>' + esc(r.tag_name) + '</b></td>' +
        '<td>' + esc(r.name || '—') + '</td>' +
        '<td>' + boolBadge(r.draft, 'draft') + '</td>' +
        '<td>' + boolBadge(r.prerelease, 'pre') + '</td>' +
        '<td class="dim nowrap">' + fmtDate(r.published_at) + '</td>' +
        '<td class="mono dim">' + esc(r.target_commitish) + '</td>' +
      '</tr>';
    }).join('');
    setContent(head + '<table class="data"><thead><tr><th>Tag</th><th>Name</th><th>Draft</th><th>Pre</th><th>Published</th><th>Target</th></tr></thead><tbody>' + body + '</tbody></table>');
  }

  function renderLatestRelease(r) {
    var head = '<h2 class="section-title">Latest Release — ' + esc(r.owner + '/' + r.repo) + '</h2>' +
      '<p class="section-sub">GET /repos/…/releases/latest</p>' +
      '<div class="toolbar"><button class="btn small ghost" onclick="OC.show(\'releases\')">← All releases</button></div>';
    setContent(head + renderReleaseCard(r));
  }

  function renderTags(data) {
    var rows = (data && data.tags) || [];
    var head = '<h2 class="section-title">Tags — ' + esc(data.owner + '/' + data.repo) + '</h2>' +
      '<p class="section-sub">' + rows.length + ' tag(s)</p>' +
      '<div class="toolbar"><button class="btn small ghost" onclick="OC.loadTags()">⟳ Refresh</button><span class="count">' + rows.length + ' total</span></div>';
    if (!rows.length) {
      setContent(head + empty('#', 'No tags', 'Tags appear here once the repository has at least one git tag.'));
      return;
    }
    var body = rows.map(function (t, i) {
      return '<tr>' +
        '<td class="dim mono">' + (i + 1) + '</td>' +
        '<td class="mono"><b>' + esc(t.name) + '</b></td>' +
        '<td class="mono dim">' + shortSha(t.sha) + '</td>' +
      '</tr>';
    }).join('');
    setContent(head + '<table class="data"><thead><tr><th></th><th>Tag</th><th>Commit</th></tr></thead><tbody>' + body + '</tbody></table>');
  }

  function renderRepos(data) {
    var rows = (data && data.repos) || [];
    var head = '<h2 class="section-title">Repositories</h2>' +
      '<p class="section-sub">GET /user/repos · sorted by last push</p>' +
      '<div class="toolbar"><button class="btn small ghost" onclick="OC.loadRepos()">⟳ Refresh</button><span class="count">' + rows.length + ' repo(s)</span></div>';
    if (!rows.length) {
      setContent(head + empty('📚', 'No repositories accessible', 'The active token grants no repository access, or the account has no repos.'));
      return;
    }
    var cards = rows.map(function (r) {
      return '<div class="card">' +
        '<h3>' + esc(r.full_name) + ' ' +
          (r.private ? '<span class="badge queued">private</span>' : '') +
          (r.fork ? '<span class="badge unknown">fork</span>' : '') +
        '</h3>' +
        '<div class="dim" style="min-height:16px;margin-bottom:6px">' + esc(r.description || 'No description') + '</div>' +
        '<div class="chips" style="margin-bottom:10px">' +
          (r.language ? '<span class="chip on">' + esc(r.language) + '</span>' : '') +
          '<span class="chip">★ ' + esc(r.stars || 0) + '</span>' +
          (r.default_branch ? '<span class="chip">' + esc(r.default_branch) + '</span>' : '') +
          '<span class="chip">' + esc(rel(r.pushed_at) || '—') + '</span>' +
        '</div>' +
        '<button class="btn small" onclick="OC.setTarget(\'' + esc(String(r.full_name).replace(/'/g, '')) + '\')">Set as target</button>' +
      '</div>';
    }).join('');
    setContent(head + '<div class="cards">' + cards + '</div>');
  }

  function renderOrgs(data) {
    var rows = (data && data.orgs) || [];
    var head = '<h2 class="section-title">Organizations</h2>' +
      '<p class="section-sub">GET /user/orgs · roles from the capability check</p>' +
      '<div class="toolbar"><button class="btn small ghost" onclick="OC.loadOrgs()">⟳ Refresh</button><span class="count">' + rows.length + ' org(s)</span></div>';
    if (!rows.length) {
      setContent(head + empty('🏢', 'No organizations', 'The authenticated user does not belong to any organization (or org:read is denied).'));
      return;
    }
    var cards = rows.map(function (o) {
      return '<div class="card">' +
        '<h3>' + esc(o.login) + ' <span class="badge info">' + esc(o.role || 'member') + '</span></h3>' +
        '<div class="dim" style="min-height:16px;margin-bottom:8px">' + esc(o.description || 'No description') + '</div>' +
        '<div class="chips">' +
          boolBadge(o.read, 'read') +
          boolBadge(o.write, 'write') +
          boolBadge(o.admin, 'admin') +
        '</div>' +
        '<div style="margin-top:10px"><button class="btn small" onclick="OC.orgDetails(\'' + esc(String(o.login).replace(/'/g, '')) + '\')">Details</button></div>' +
      '</div>';
    }).join('');
    setContent(head + '<div class="cards">' + cards + '</div>');
  }

  function renderOrgDetails(d) {
    var head = '<h2 class="section-title">Org ' + esc(d.login || '') + '</h2>' +
      '<p class="section-sub">profile · repos · members (first page)</p>' +
      '<div class="toolbar"><button class="btn small ghost" onclick="OC.show(\'orgs\')">← All orgs</button></div>';
    var reposHtml = (d.repos || []).map(function (r) {
      return '<tr><td class="mono"><b>' + esc(r.name) + '</b></td>' +
        '<td class="dim">' + esc(r.description || '') + '</td>' +
        '<td>' + (r.language ? '<span class="chip on">' + esc(r.language) + '</span>' : '—') + '</td>' +
        '<td class="mono dim">★ ' + esc(r.stars || 0) + '</td></tr>';
    }).join('');
    setContent(head +
      '<div class="card" style="margin-bottom:12px"><h3>' + esc(d.name || d.login) +
        (d.is_verified ? ' <span class="badge granted">verified</span>' : '') + '</h3>' +
      '<div class="kv">' +
        '<div class="k">description</div><div>' + esc(d.description || '—') + '</div>' +
        '<div class="k">location</div><div>' + esc(d.location || '—') + '</div>' +
        '<div class="k">blog</div><div class="mono">' + esc(d.blog || '—') + '</div>' +
        '<div class="k">public repos</div><div>' + esc(d.public_repos || 0) + ' <span class="dim">(fetched ' + esc(d.repos_fetched || 0) + ')</span></div>' +
        '<div class="k">members</div><div>' + esc(d.members_fetched || 0) + ' <span class="dim">(first page)</span></div>' +
      '</div></div>' +
      (reposHtml
        ? '<table class="data"><thead><tr><th>Repo</th><th>Description</th><th>Language</th><th>Stars</th></tr></thead><tbody>' + reposHtml + '</tbody></table>'
        : empty('📚', 'No org repos visible', 'org repos are private or the token lacks access')));
  }

  function renderModules(data) {
    state.modules = data;
    var rows = (data && data.modules) || [];
    var granted = data.granted || 0, total = data.total || 0;
    var pct = total ? Math.round((granted / total) * 100) : 0;
    var head = '<h2 class="section-title">Capability Modules</h2>' +
      '<p class="section-sub">' + granted + ' of ' + total + ' granted (' + pct + '%) · stored in the vault DB, probed by <span class="mono">octocrab-cli caps check</span></p>' +
      '<div class="toolbar"><button class="btn small ghost" onclick="OC.loadModules()">⟳ Refresh</button>' +
      '<span class="badge granted">granted ' + granted + '</span><span class="badge denied">denied ' +
      rows.filter(function (m) { return m.state === 'denied'; }).length + '</span><span class="badge unknown">unknown ' +
      rows.filter(function (m) { return m.state === 'unknown'; }).length + '</span></div>';
    if (!rows.length) {
      setContent(head + empty('🔐', 'No modules recorded', 'Run: octocrab-cli caps check'));
      return;
    }
    var body = rows.map(function (m) {
      return '<tr>' +
        '<td class="mono"><b>' + esc(m.key) + '</b></td>' +
        '<td>' + modBadge(m.state) + '</td>' +
        '<td class="mono dim">' + esc(m.source) + '</td>' +
        '<td class="dim clip" title="' + esc(m.evidence) + '">' + esc(m.evidence) + '</td>' +
      '</tr>';
    }).join('');
    setContent(head + '<table class="data"><thead><tr><th>Module</th><th>State</th><th>Source</th><th>Evidence</th></tr></thead><tbody>' + body + '</tbody></table>');
  }

  // ------------------------------------------------------------ data loads

  function loadAuthFooter() {
    return inv('auth_status').then(function (a) {
      state.auth = a;
      var left = (a.login || 'no identity') + (a.kind ? ' · ' + a.kind : '') +
        ' · ' + (a.granted || 0) + '/' + (a.total_modules || 0) + ' modules' +
        (a.token_label ? ' · token ' + a.token_label : '');
      document.getElementById('status-left').textContent = left;
      document.getElementById('status-right').textContent =
        (state.target && state.target.owner ? 'target ' + state.target.owner + '/' + state.target.repo : 'no target') +
        (a.auth_state ? ' · ' + a.auth_state : '');
      return a;
    });
  }

  function loadDefaultTarget() {
    return inv('default_target').then(function (t) {
      if (t && t.owner && t.repo) {
        state.target = t;
        document.getElementById('target-input').value = t.owner + '/' + t.repo;
      }
      return t;
    }).catch(function (e) {
      toast('default_target failed: ' + (e.message || e), 'err');
      return null;
    });
  }

  var loaders = {
    overview: function () {
      setContent(setLoading('Loading identity & capabilities…'));
      return inv('auth_status')
        .then(renderOverview)
        .catch(function (e) { setContent(errorBox(e, 'OC.loadOverview')); });
    },
    workflows: function () {
      if (!state.target) { setContent(empty('⚙', 'No target repository', 'Set owner/repo in the header bar first.')); return Promise.resolve(); }
      setContent(setLoading('Listing workflows…'));
      return inv('list_workflows', targetArgs())
        .then(renderWorkflows)
        .catch(function (e) { setContent(errorBox(e, 'OC.loadWorkflows')); });
    },
    runs: function () {
      if (!state.target) { setContent(empty('▶', 'No target repository', 'Set owner/repo in the header bar first.')); return Promise.resolve(); }
      setContent(setLoading('Listing workflow runs…'));
      var args = targetArgs({ limit: state.limit });
      if (state.branch) args.branch = state.branch;
      return inv('list_runs', args)
        .then(renderRuns)
        .catch(function (e) { setContent(errorBox(e, 'OC.loadRuns')); });
    },
    releases: function () {
      if (!state.target) { setContent(empty('🏷', 'No target repository', 'Set owner/repo in the header bar first.')); return Promise.resolve(); }
      setContent(setLoading('Listing releases…'));
      return inv('list_releases', targetArgs({ limit: 30 }))
        .then(renderReleases)
        .catch(function (e) { setContent(errorBox(e, 'OC.loadReleases')); });
    },
    tags: function () {
      if (!state.target) { setContent(empty('#', 'No target repository', 'Set owner/repo in the header bar first.')); return Promise.resolve(); }
      setContent(setLoading('Listing tags…'));
      return inv('list_tags', targetArgs({ limit: 50 }))
        .then(renderTags)
        .catch(function (e) { setContent(errorBox(e, 'OC.loadTags')); });
    },
    repos: function () {
      setContent(setLoading('Listing repositories…'));
      return inv('list_repos', { limit: 50 })
        .then(renderRepos)
        .catch(function (e) { setContent(errorBox(e, 'OC.loadRepos')); });
    },
    orgs: function () {
      setContent(setLoading('Listing organizations…'));
      return inv('list_orgs')
        .then(renderOrgs)
        .catch(function (e) { setContent(errorBox(e, 'OC.loadOrgs')); });
    },
    modules: function () {
      setContent(setLoading('Reading capability modules…'));
      return inv('modules')
        .then(renderModules)
        .catch(function (e) { setContent(errorBox(e, 'OC.loadModules')); });
    },
  };

  function show(section) {
    state.section = section;
    var items = document.querySelectorAll('.nav-item');
    for (var i = 0; i < items.length; i++) {
      items[i].classList.toggle('active', items[i].getAttribute('data-section') === section);
    }
    closeDrawer();
    var fn = loaders[section] || loaders.overview;
    fn();
  }

  // --------------------------------------------------------------- actions

  function applyRunFilters() {
    var b = document.getElementById('branch-input');
    var l = document.getElementById('limit-sel');
    state.branch = b ? b.value.trim() : '';
    state.limit = l ? parseInt(l.value, 10) || 25 : 25;
    loadRuns();
  }

  function setTarget(fullName) {
    var parts = String(fullName || '').split('/');
    if (parts.length !== 2 || !parts[0] || !parts[1]) { toast('target must be owner/repo', 'err'); return; }
    state.target = { owner: parts[0], repo: parts[1], source: 'manual' };
    document.getElementById('target-input').value = fullName;
    document.getElementById('status-right').textContent = 'target ' + fullName + ' · ' + (state.auth && state.auth.auth_state || '');
    toast('target set to ' + fullName, 'ok');
    show(state.section);
  }

  function applyTargetInput() {
    setTarget(document.getElementById('target-input').value.trim());
  }

  function dispatch(workflowId, name) {
    if (!state.target) { toast('no target repository', 'err'); return; }
    var ref = 'main';
    var btns = document.querySelectorAll('.btn');
    for (var i = 0; i < btns.length; i++) btns[i].disabled = true;
    inv('dispatch_workflow', { owner: state.target.owner, repo: state.target.repo, workflow_id: String(workflowId), git_ref: ref })
      .then(function (r) {
        toast('Dispatched ' + (name || workflowId) + ' on ' + ref + ' — run queued', 'ok');
        setTimeout(function () { show('runs'); }, 900);
      })
      .catch(function (e) {
        toast('Dispatch failed: ' + (e.message || e), 'err');
      })
      .then(function () {
        for (var i = 0; i < btns.length; i++) btns[i].disabled = false;
      });
  }

  function loadLatestRelease() {
    if (!state.target) { toast('no target repository', 'err'); return; }
    setContent(setLoading('Fetching latest release…'));
    inv('latest_release', targetArgs())
      .then(renderLatestRelease)
      .catch(function (e) { setContent(errorBox(e, 'OC.loadReleases')); });
  }

  function orgDetails(login) {
    setContent(setLoading('Fetching org ' + login + '…'));
    inv('org_details', { org: login })
      .then(renderOrgDetails)
      .catch(function (e) { setContent(errorBox(e, 'OC.loadOrgs')); });
  }

  // --------------------------------------------------------------- drawer

  function openRun(runId) {
    state.currentRunId = runId;
    var drawer = document.getElementById('drawer');
    var body = document.getElementById('drawer-body');
    var title = document.getElementById('drawer-title');
    drawer.classList.remove('hidden');
    title.textContent = 'Run ' + runId;
    body.innerHTML = setLoading('Fetching jobs & steps…');
    inv('run_details', targetArgs({ run_id: runId }))
      .then(function (d) {
        if (state.currentRunId !== runId) return;
        renderRunDrawer(d);
      })
      .catch(function (e) {
        if (state.currentRunId !== runId) return;
        body.innerHTML = errorBox(e, 'OC.reopenRun');
      });
  }

  function reopenRun() {
    if (state.currentRunId) openRun(state.currentRunId);
  }

  function renderRunDrawer(d) {
    var run = d.run || {};
    var jobs = d.jobs || [];
    document.getElementById('drawer-title').innerHTML =
      '#' + esc(run.run_number) + ' · ' + esc(run.name || 'run') + ' ' +
      runStatusBadge(run.status, run.conclusion) + (d.cached ? ' <span class="badge unknown">cached</span>' : '');
    var html =
      '<div class="card" style="margin-bottom:12px"><div class="kv">' +
        '<div class="k">branch</div><div class="mono">' + esc(run.head_branch) + '</div>' +
        '<div class="k">commit</div><div class="mono">' + shortSha(run.head_sha) + '</div>' +
        '<div class="k">event</div><div>' + esc(run.event) + '</div>' +
        '<div class="k">created</div><div>' + fmtDate(run.created_at) + ' <span class="dim">(' + esc(rel(run.created_at)) + ')</span></div>' +
        '<div class="k">updated</div><div>' + fmtDate(run.updated_at) + '</div>' +
        '<div class="k">duration</div><div>' + esc(durBetween(run.created_at, run.updated_at)) + '</div>' +
      '</div></div>';
    if (!jobs.length) {
      html += empty('🛠', 'No jobs reported', 'Jobs may still be scheduling.');
    }
    jobs.forEach(function (j) {
      var steps = j.steps || [];
      var stepsHtml = steps.map(function (st) {
        var g = stepGlyph(st);
        var pulse = (g[1] === 'run') ? ' pulse' : '';
        return '<div class="step' + pulse + '">' +
          '<span class="glyph ' + g[1] + '">' + g[0] + '</span>' +
          '<span class="step-name">' + esc(st.number) + '. ' + esc(st.name) + '</span>' +
          '<span class="step-dur">' + esc(durBetween(st.started_at, st.completed_at)) + '</span>' +
        '</div>';
      }).join('');
      html += '<div class="job">' +
        '<div class="job-head">' +
          '<span class="job-name">' + esc(j.name) + '</span>' +
          runStatusBadge(j.status, j.conclusion) +
          '<span class="job-meta">' + (j.runner_name ? 'runner: ' + esc(j.runner_name) + ' · ' : '') +
            esc(durBetween(j.started_at, j.completed_at)) + '</span>' +
        '</div>' +
        (stepsHtml ? '<div class="steps">' + stepsHtml + '</div>' : '<div class="dim" style="padding:8px 12px">no steps</div>') +
      '</div>';
    });
    document.getElementById('drawer-body').innerHTML = html;
  }

  function closeDrawer() {
    state.currentRunId = null;
    document.getElementById('drawer').classList.add('hidden');
  }

  // ------------------------------------------------------------------ boot

  function boot() {
    var items = document.querySelectorAll('.nav-item');
    for (var i = 0; i < items.length; i++) {
      items[i].addEventListener('click', function () { show(this.getAttribute('data-section')); });
    }
    document.getElementById('target-set').addEventListener('click', applyTargetInput);
    document.getElementById('target-input').addEventListener('keydown', function (ev) {
      if (ev.key === 'Enter') applyTargetInput();
    });
    document.getElementById('refresh-btn').addEventListener('click', function () { show(state.section); });
    document.getElementById('drawer-close').addEventListener('click', closeDrawer);

    if (!hasTauri()) {
      setContent(errorBox(new Error('This dashboard must run inside the octocrab-tauri window (window.__TAURI__ is missing).')));
      document.getElementById('status-left').textContent = 'IPC unavailable';
      return;
    }

    inv('ping').then(function (r) {
      document.getElementById('status-left').textContent = 'ping ok: ' + JSON.stringify(r);
      return r;
    }).catch(function (e) {
      document.getElementById('status-left').textContent = 'ping FAILED: ' + (e.message || e);
    });
    loadDefaultTarget()
      .then(loadAuthFooter)
      .then(function () { show('overview'); })
      .catch(function (e) {
        setContent(errorBox(e));
        document.getElementById('status-left').textContent = 'startup failed: ' + (e.message || e);
      });
  }

  // export minimal API for inline onclick handlers
  window.OC = {
    show: show,
    loadOverview: function () { return loaders.overview(); },
    loadWorkflows: function () { return loaders.workflows(); },
    loadRuns: function () { return loaders.runs(); },
    loadReleases: function () { return loaders.releases(); },
    loadLatestRelease: loadLatestRelease,
    loadTags: function () { return loaders.tags(); },
    loadRepos: function () { return loaders.repos(); },
    loadOrgs: function () { return loaders.orgs(); },
    loadModules: function () { return loaders.modules(); },
    applyRunFilters: applyRunFilters,
    setTarget: setTarget,
    dispatch: dispatch,
    openRun: openRun,
    reopenRun: reopenRun,
    orgDetails: orgDetails,
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
})();
