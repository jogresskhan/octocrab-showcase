/* QA harness mock — defines window.__TAURI__ with a canned core.invoke so the
 * control-center frontend can be exercised in a plain browser (file://),
 * without the native Tauri shell.
 *
 * Every command the app uses has a realistic response; each call is delayed
 * ~100 ms and recorded in window.__MOCK_CALLS__ for assertions.
 *
 * Special targets for error-path QA:
 *   locked/<anything>   → gated commands return {"ok": false, "error": …}
 *   nopages/<anything>  → pages_info returns {enabled: false}
 */
(function () {
  'use strict';

  var DELAY = 100;

  var REPO = 'jogresskhan/octocrab-showcase';
  var OWNER = 'jogresskhan';
  var NAME = 'octocrab-showcase';

  function nowIso(offsetSec) {
    return new Date(Date.now() - (offsetSec || 0) * 1000).toISOString();
  }

  // matches octocrab_core::templates::TEMPLATES
  var TEMPLATES = {
    ci: {
      key: 'ci', name: 'Multi-OS CI', gate: 'workflow:write',
      desc: 'check + test matrix on ubuntu/macos/windows, clippy+fmt gate',
      files: ['.github/workflows/ci.yml'],
    },
    release: {
      key: 'release', name: 'Multi-OS Packaging & Release', gate: 'workflow:write',
      desc: 'builds linux/macos/windows release bundles (tar.gz/zip), attaches them + sha256 checksums to a GitHub Release',
      files: ['.github/workflows/release.yml'],
    },
    pages: {
      key: 'pages', name: 'GitHub Pages Deploy', gate: 'workflow:write',
      desc: 'deploys the site/ directory to GitHub Pages (official deploy-pages flow)',
      files: ['.github/workflows/pages.yml'],
    },
    site: {
      key: 'site', name: 'Pages Landing Site', gate: 'repo:write',
      desc: 'data-driven index.html + style.css under site/ (hero, badges, install cards)',
      files: ['site/index.html', 'site/style.css'],
    },
    community: {
      key: 'community', name: 'Community Health Kit', gate: 'repo:write',
      desc: 'CONTRIBUTING, Code of Conduct, SECURITY, issue & PR templates',
      files: ['CONTRIBUTING.md', 'CODE_OF_CONDUCT.md', 'SECURITY.md', '.github/PULL_REQUEST_TEMPLATE.md', '.github/ISSUE_TEMPLATE/bug_report.yml', '.github/ISSUE_TEMPLATE/feature_request.yml'],
    },
    readme: {
      key: 'readme', name: 'README Scaffold', gate: 'repo:write',
      desc: 'repo landing page: badges, per-OS install table, usage, contributing',
      files: ['README.md'],
    },
  };

  var MODULES = [
    ['repo:read', true, 'granted'], ['repo:write', true, 'granted'], ['repo:admin', true, 'granted'],
    ['workflow:read', true, 'granted'], ['workflow:write', true, 'granted'], ['org:read', true, 'granted'],
    ['org:write', false, 'denied'], ['org:admin', false, 'denied'],
    ['gist:write', true, 'granted'], ['pages:admin', true, 'granted'],
    ['user:read', true, 'granted'], ['secrets:write', false, 'denied'],
  ];

  function splitRepo(args) {
    var spec = String((args && args.repo) || '').trim();
    var parts = spec.split('/').map(function (s) { return s.trim(); }).filter(Boolean);
    return parts.length === 2 ? parts : null;
  }

  function ownerRepo(args) {
    return {
      owner: (args && args.owner) || OWNER,
      repo: (args && args.repo) || NAME,
    };
  }

  function repoJson(o, r) {
    return {
      id: 77001, name: r, full_name: o + '/' + r,
      owner: { login: o }, private: false, fork: false,
      description: 'Showcase repo for the Octocrab Suite — control center demo',
      homepage: 'https://' + o + '.github.io/' + r + '/',
      default_branch: 'main',
      license: { key: 'mit', name: 'MIT License', spdx_id: 'MIT' },
      html_url: 'https://github.com/' + o + '/' + r,
      stargazers_count: 42, open_issues_count: 3, watchers_count: 7,
      has_pages: true, topics: ['rust', 'octocrab', 'github-actions', 'tauri'],
      pushed_at: nowIso(3600), created_at: '2024-11-02T10:00:00Z',
    };
  }

  var COMMUNITY = {
    health_percentage: 83,
    files: {
      readme: { url: 'https://api.github.com/repos/' + REPO + '/contents/README.md', html_url: 'https://github.com/' + REPO + '/blob/main/README.md' },
      license: { url: 'https://api.github.com/repos/' + REPO + '/contents/LICENSE', html_url: 'https://github.com/' + REPO + '/blob/main/LICENSE' },
      contributing: { url: 'https://api.github.com/repos/' + REPO + '/contents/CONTRIBUTING.md', html_url: 'https://github.com/' + REPO + '/blob/main/CONTRIBUTING.md' },
      code_of_conduct: { url: 'https://api.github.com/repos/' + REPO + '/contents/CODE_OF_CONDUCT.md', html_url: 'https://github.com/' + REPO + '/blob/main/CODE_OF_CONDUCT.md' },
      issue_template: { url: 'https://api.github.com/repos/' + REPO + '/contents/.github/ISSUE_TEMPLATE', html_url: 'https://github.com/' + REPO + '/issues/new/choose' },
      pull_request_template: null,
    },
    updated_at: nowIso(86400),
  };

  function installResult(key, o, r) {
    var t = TEMPLATES[key];
    var files = (t ? t.files : []).map(function (p) {
      return {
        path: p, ok: true,
        url: 'https://github.com/' + o + '/' + r + '/blob/main/' + p.replace(/^\//, ''),
      };
    });
    return {
      ok: true, template: key, repo: o + '/' + r, gate: t ? t.gate : '',
      files: files, written: files.length, total: files.length,
    };
  }

  function isLocked(args) {
    var o = splitRepo(args);
    return !!o && o[0] === 'locked';
  }

  function gateError(gate) {
    return {
      ok: false,
      error: gate
        ? gate + ' module is locked — the active token has not been granted it'
        : 'module locked for this session',
    };
  }

  var MOCK = {
    // ------------------------------------------------------------ existing
    ping: function () { return { pong: true }; },

    auth_status: function () {
      return {
        login: 'jogresskhan', kind: 'classic', auth_state: 'authorized',
        scopes: ['repo', 'workflow', 'read:org', 'gist'],
        token_label: 'gh***xy', granted: 9, total_modules: 12,
        orgs: [{ login: 'jogresskhan', role: 'owner', read: true, write: true, admin: true }],
        caps_checked_at: '1767000000',
      };
    },

    modules: function () {
      var mods = MODULES.map(function (m) {
        return { key: m[0], enabled: m[1], state: m[2], source: 'probe', evidence: 'mock evidence for ' + m[0] };
      });
      return {
        modules: mods,
        granted: mods.filter(function (m) { return m.enabled; }).length,
        total: mods.length,
        orgs: [{ login: 'jogresskhan', role: 'owner', read: true, write: true, admin: true }],
      };
    },

    default_target: function () {
      return { owner: OWNER, repo: NAME, full_name: REPO, source: 'preferred' };
    },

    list_workflows: function (args) {
      var t = ownerRepo(args);
      return {
        owner: t.owner, repo: t.repo,
        workflows: [
          { id: 101, name: 'CI', path: '.github/workflows/ci.yml', state: 'active', html_url: 'https://github.com/' + t.owner + '/' + t.repo + '/actions/workflows/ci.yml' },
          { id: 102, name: 'Release', path: '.github/workflows/release.yml', state: 'active', html_url: 'https://github.com/' + t.owner + '/' + t.repo + '/actions/workflows/release.yml' },
          { id: 103, name: 'Deploy Pages', path: '.github/workflows/pages.yml', state: 'active', html_url: 'https://github.com/' + t.owner + '/' + t.repo + '/actions/workflows/pages.yml' },
        ],
        total: 3,
      };
    },

    list_runs: function (args) {
      var t = ownerRepo(args);
      var mk = function (n, status, conclusion, ev, min) {
        return {
          id: 5544000 + n, run_number: n, name: ev === 'push' ? 'CI' : 'Release',
          head_branch: 'main', head_sha: '9f3ac2b77e1d4c0a5511e2b3c4d5e6f708192a3b',
          event: ev, status: status, conclusion: conclusion, workflow_id: 101,
          created_at: nowIso(min * 60), updated_at: nowIso(Math.max(0, min * 60 - 240)),
          html_url: 'https://github.com/' + t.owner + '/' + t.repo + '/actions/runs/' + (5544000 + n),
        };
      };
      return {
        owner: t.owner, repo: t.repo, branch: args.branch || null,
        runs: [mk(21, 'completed', 'success', 'push', 30), mk(20, 'completed', 'failure', 'push', 1500), mk(19, 'in_progress', null, 'workflow_dispatch', 3)],
        total: 3,
      };
    },

    run_details: function (args) {
      var t = ownerRepo(args);
      var id = (args && args.run_id) || 5544000;
      return {
        run: {
          id: id, name: 'CI', head_branch: 'main', head_sha: '9f3ac2b77e1d4c0a5511e2b3c4d5e6f708192a3b',
          event: 'push', status: 'completed', conclusion: 'success', run_number: 21,
          created_at: nowIso(1800), updated_at: nowIso(1560),
          html_url: 'https://github.com/' + t.owner + '/' + t.repo + '/actions/runs/' + id,
        },
        jobs: [{
          id: 991, name: 'check+test (ubuntu-latest)', status: 'completed', conclusion: 'success',
          started_at: nowIso(1740), completed_at: nowIso(1620), runner_name: 'ubuntu-latest',
          steps: [
            { name: 'Checkout', number: 1, status: 'completed', conclusion: 'success', started_at: nowIso(1740), completed_at: nowIso(1739) },
            { name: 'Install Rust toolchain', number: 2, status: 'completed', conclusion: 'success', started_at: nowIso(1739), completed_at: nowIso(1730) },
            { name: 'Check workspace', number: 3, status: 'completed', conclusion: 'success', started_at: nowIso(1730), completed_at: nowIso(1660) },
            { name: 'Test workspace', number: 4, status: 'completed', conclusion: 'success', started_at: nowIso(1660), completed_at: nowIso(1620) },
          ],
        }],
        cached: false,
      };
    },

    dispatch_workflow: function (args) {
      return { ok: true, dispatched: true, workflow_id: (args && args.workflow_id) || 'ci.yml', ref: (args && args.git_ref) || 'main' };
    },

    list_releases: function (args) {
      var t = ownerRepo(args);
      return {
        owner: t.owner, repo: t.repo,
        releases: [{
          id: 9001, tag_name: 'v0.3.0', name: 'v0.3.0 — control center', draft: false, prerelease: false,
          published_at: nowIso(86400 * 3), target_commitish: 'main',
          html_url: 'https://github.com/' + t.owner + '/' + t.repo + '/releases/tag/v0.3.0',
          body: '## Highlights\n- three new tabs (Templates, Pages, Repo)\n- mocked QA harness',
        }],
        total: 1,
      };
    },

    latest_release: function (args) {
      var t = ownerRepo(args);
      return {
        owner: t.owner, repo: t.repo, found: true, id: 9001, tag_name: 'v0.3.0',
        name: 'v0.3.0 — control center', draft: false, prerelease: false,
        published_at: nowIso(86400 * 3), html_url: 'https://github.com/' + t.owner + '/' + t.repo + '/releases/tag/v0.3.0',
        body: '## Highlights\n- three new tabs (Templates, Pages, Repo)',
      };
    },

    list_tags: function (args) {
      var t = ownerRepo(args);
      return {
        owner: t.owner, repo: t.repo,
        tags: [
          { name: 'v0.3.0', sha: '9f3ac2b77e1d4c0a5511e2b3c4d5e6f708192a3b', zipball_url: 'https://api.github.com/repos/' + t.owner + '/' + t.repo + '/zipball/v0.3.0' },
          { name: 'v0.2.1', sha: '1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d', zipball_url: 'https://api.github.com/repos/' + t.owner + '/' + t.repo + '/zipball/v0.2.1' },
        ],
        total: 2,
      };
    },

    list_repos: function () {
      return {
        repos: [
          { full_name: REPO, name: NAME, owner: OWNER, private: false, fork: false, description: 'Showcase repo for the Octocrab Suite', language: 'Rust', stars: 42, pushed_at: nowIso(3600), default_branch: 'main', html_url: 'https://github.com/' + REPO },
          { full_name: 'jogresskhan/dotfiles', name: 'dotfiles', owner: 'jogresskhan', private: true, fork: false, description: null, language: 'Shell', stars: 3, pushed_at: nowIso(86400 * 9), default_branch: 'main', html_url: 'https://github.com/jogresskhan/dotfiles' },
        ],
        total: 2,
      };
    },

    list_orgs: function () {
      return {
        orgs: [{ login: 'jogresskhan', description: 'Octocrab Suite org', avatar_url: '', role: 'owner', read: true, write: true, admin: true }],
        total: 1, known_in_db: 1,
      };
    },

    org_details: function (args) {
      var org = (args && args.org) || 'jogresskhan';
      return {
        login: org, name: 'Octocrab Suite Org', description: 'control center demo org',
        blog: 'https://example.dev', location: 'Europe', is_verified: false,
        public_repos: 2, html_url: 'https://github.com/' + org,
        repos: [{ full_name: REPO, name: NAME, description: 'Showcase repo', language: 'Rust', stars: 42, html_url: 'https://github.com/' + REPO }],
        repos_fetched: 1, members_fetched: 1,
      };
    },

    // ------------------------------------------------------------ new tabs
    templates_list: function () {
      var templates = Object.keys(TEMPLATES).map(function (k) {
        var t = TEMPLATES[k];
        return { key: t.key, name: t.name, desc: t.desc, files: t.files, gate: t.gate };
      });
      return { templates: templates, total: templates.length };
    },

    template_install: function (args) {
      var o = splitRepo(args);
      if (!o) return { error: "target must be owner/repo — got '" + ((args && args.repo) || '') + "'" };
      if (isLocked(args)) return gateError(TEMPLATES[(args && args.key)] ? TEMPLATES[args.key].gate : 'repo:write');
      var key = (args && args.key) || 'ci';
      if (!TEMPLATES[key]) return { error: "unknown template '" + key + "'" };
      return installResult(key, o[0], o[1]);
    },

    pages_info: function (args) {
      var o = splitRepo(args);
      if (!o) return { error: "target must be owner/repo — got '" + ((args && args.repo) || '') + "'" };
      if (o[0] === 'nopages') {
        return { ok: true, enabled: false, owner: o[0], repo: o[1], reason: 'GitHub Pages is not enabled for ' + o[0] + '/' + o[1] };
      }
      return {
        ok: true, enabled: true, owner: o[0], repo: o[1],
        html_url: 'https://' + o[0] + '.github.io/' + o[1] + '/',
        build_type: 'workflow', status: '',
        cname: null, source: { branch: 'main', path: '/' }, protected: true, public: true,
        pages: { html_url: 'https://' + o[0] + '.github.io/' + o[1] + '/', build_type: 'workflow', source: { branch: 'main', path: '/' }, protected: true },
      };
    },

    pages_enable: function (args) {
      var o = splitRepo(args);
      if (!o) return { error: "target must be owner/repo — got '" + ((args && args.repo) || '') + "'" };
      if (isLocked(args)) return gateError('repo:admin');
      return {
        ok: true, owner: o[0], repo: o[1],
        html_url: 'https://' + o[0] + '.github.io/' + o[1] + '/', build_type: 'workflow',
        pages: { html_url: 'https://' + o[0] + '.github.io/' + o[1] + '/', build_type: 'workflow' },
      };
    },

    pages_build: function (args) {
      var o = splitRepo(args);
      if (!o) return { error: "target must be owner/repo — got '" + ((args && args.repo) || '') + "'" };
      if (isLocked(args)) return gateError('repo:admin');
      return {
        ok: true, owner: o[0], repo: o[1], status: 'built',
        created_at: nowIso(120), updated_at: nowIso(60),
        commit: '9f3ac2b77e1d4c0a5511e2b3c4d5e6f708192a3b',
        url: 'https://github.com/' + o[0] + '/' + o[1] + '/commit/9f3ac2b/pages-build',
        error: null,
        build: { status: 'built', created_at: nowIso(120), updated_at: nowIso(60), commit: '9f3ac2b' },
      };
    },

    repo_get: function (args) {
      var o = splitRepo(args);
      if (!o) return { error: "target must be owner/repo — got '" + ((args && args.repo) || '') + "'" };
      var rj = repoJson(o[0], o[1]);
      return {
        owner: o[0], repo: o[1], full_name: rj.full_name,
        description: rj.description, homepage: rj.homepage, topics: rj.topics,
        default_branch: rj.default_branch, license: 'MIT', html_url: rj.html_url,
        stars: rj.stargazers_count, open_issues: rj.open_issues_count,
        private: false, fork: false, has_pages: true,
        community: COMMUNITY, repo_data: rj,
      };
    },

    repo_edit: function (args) {
      var o = splitRepo(args);
      if (!o) return { error: "target must be owner/repo — got '" + ((args && args.repo) || '') + "'" };
      if (isLocked(args)) return gateError('repo:admin');
      var topics = String((args && args.topics) || '')
        .split(',').map(function (s) { return s.trim().toLowerCase(); }).filter(Boolean);
      return {
        ok: true, owner: o[0], repo: o[1],
        description: (args && args.description) || null,
        homepage: (args && args.homepage) || null,
        topics: topics,
      };
    },

    community_scaffold: function (args) {
      var o = splitRepo(args);
      if (!o) return { error: "target must be owner/repo — got '" + ((args && args.repo) || '') + "'" };
      if (isLocked(args)) return gateError('repo:write');
      return installResult('community', o[0], o[1]);
    },

    site_generate: function (args) {
      var o = splitRepo(args);
      if (!o) return { error: "target must be owner/repo — got '" + ((args && args.repo) || '') + "'" };
      if (isLocked(args)) return gateError('repo:write');
      return installResult('site', o[0], o[1]);
    },
  };

  window.__MOCK_CALLS__ = [];

  window.__TAURI__ = {
    core: {
      invoke: function (cmd, args) {
        return new Promise(function (resolve) {
          setTimeout(function () {
            window.__MOCK_CALLS__.push({ cmd: cmd, args: args || {}, at: Date.now() });
            var handler = Object.prototype.hasOwnProperty.call(MOCK, cmd) ? MOCK[cmd] : null;
            if (!handler) {
              resolve({ error: 'mock: no canned response for command "' + cmd + '"' });
              return;
            }
            var out;
            try {
              out = handler(args || {});
            } catch (e) {
              out = { error: 'mock handler threw: ' + (e && e.message) };
            }
            resolve(out);
          }, DELAY);
        });
      },
    },
  };

  // QA helpers
  window.__QA__ = {
    calls: function () { return window.__MOCK_CALLS__.slice(); },
    callsFor: function (cmd) {
      return window.__MOCK_CALLS__.filter(function (c) { return c.cmd === cmd; });
    },
    reset: function () { window.__MOCK_CALLS__.length = 0; },
  };
})();
