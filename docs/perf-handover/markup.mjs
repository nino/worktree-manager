// Static markup approximating the real app tree, for CSS paint benchmarking.
// Structure mirrors App.tsx > RepoNode > WorktreeRow.
export function page(css, { repos = 6, worktrees = 8 } = {}) {
  const row = (i) => `
    <div class="wt-row${i === 0 ? " wt-main" : ""}">
      <div class="wt-info">
        <div class="wt-line1">
          <button class="branch-select" type="button">feature/some-branch-${i}</button>
          <span class="badges">
            <span class="badge badge-staged">3</span>
            <span class="badge badge-unstaged">1</span>
            <span class="badge badge-ahead">↑2</span>
            <span class="badge badge-behind">↓4</span>
          </span>
        </div>
        <div class="wt-path-row">
          <span class="wt-path">~/worktrees/repo/feature-some-branch-${i}</span>
          <button class="copy-btn"><svg width="13" height="13"></svg></button>
        </div>
      </div>
      <div class="wt-actions">
        <span class="btn-group">
          <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
          <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
          <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
        </span>
        <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
        <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
      </div>
    </div>`;

  const repo = (r) => `
    <section class="repo">
      <div class="repo-head">
        <button class="disclosure">▼</button>
        <div class="repo-title">
          <span class="repo-name">repository-number-${r}</span>
          <span class="repo-path">~/code/repository-number-${r}</span>
          <button class="copy-btn"><svg width="13" height="13"></svg></button>
        </div>
        <button class="btn btn-sm">+ New worktree</button>
        <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
      </div>
      <div class="wt-list">${Array.from({ length: worktrees }, (_, i) => row(i)).join("")}</div>
    </section>`;

  return `<!doctype html><html><head><meta charset="utf-8"><style>${css}</style></head>
<body><div id="root"><div class="app">
  <header class="topbar">
    <div class="topbar-title">
      <span class="gems">
        <button class="gem gem-close"></button>
        <button class="gem gem-min"></button>
        <button class="gem gem-zoom"></button>
      </span>
      <span class="app-name">Worktree Manager</span>
    </div>
    <div class="topbar-actions">
      <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
      <button class="btn btn-sm btn-primary">+ Add repo</button>
      <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
      <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
      <button class="btn btn-sm btn-icon"><svg width="13" height="13"></svg></button>
    </div>
  </header>
  <main class="tree">${Array.from({ length: repos }, (_, r) => repo(r)).join("")}</main>
  <div class="grow-box"></div>
</div></div></body></html>`;
}
