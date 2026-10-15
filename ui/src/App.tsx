import { createMemo, createSignal, For, onCleanup, onMount, Show, type JSX } from "solid-js";
import { createStore, reconcile } from "solid-js/store";

import type { Backend } from "./backend";
import { badgesFor } from "./badges";
import { splitAgentPrefix } from "./branch";
import * as I from "./icons";
import { noticeIsLong, noticeSummary } from "./notice";
import { children, normalise, repoMeta, showsRepo } from "./search";
import type { Model } from "./wire/Model";
import type { Pending } from "./wire/Pending";
import type { Repo } from "./wire/Repo";
import type { Worktree } from "./wire/Worktree";

const BUSY_LABEL = {
  pushing: "Pushing…",
  pulling: "Pulling…",
  merging: "Merging…",
  switching: "Switching…",
  deleting: "Deleting…",
} as const;

export function App(props: { backend: Backend; initial: Model }) {
  const be = props.backend;
  // One store for the core's model, patched in place on every push so that
  // only the rows whose data changed are touched.
  const [model, setModel] = createStore<Model>(structuredClone(props.initial));
  const apply = (m: Model) => setModel(reconcile(m, { key: "id" }));
  be.onModel(apply);
  // The DOM now holds the boot model, so the window can be shown with
  // content in its first frame; the answer is the model as it is by now.
  onMount(() => be.ready().then(apply));

  const [query, setQuery] = createSignal("");
  const q = createMemo(() => normalise(query()));
  const [closed, setClosed] = createSignal<ReadonlySet<string>>(new Set());
  const toggle = (id: string) =>
    setClosed((s) => {
      const next = new Set(s);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  let search!: HTMLInputElement;
  const onKey = (e: KeyboardEvent) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key === "f") {
      e.preventDefault();
      search.focus();
      search.select();
    } else if (mod && e.key === "r") {
      e.preventDefault();
      be.refresh();
    }
  };
  document.addEventListener("keydown", onKey);
  onCleanup(() => document.removeEventListener("keydown", onKey));

  const shown = createMemo(() => model.repos.filter((r) => showsRepo(r, q())));

  return (
    <div class="window">
      <header class="toolbar">
        <button
          class="tool"
          classList={{ spinning: model.refreshing }}
          title="Refresh all (⌘R)"
          aria-label="Refresh all"
          onClick={() => be.refresh()}
        >
          <I.Refresh />
        </button>
        <h1>Worktree Manager</h1>
        <label class="search">
          <I.Search />
          <input
            ref={search}
            type="search"
            placeholder="Search worktrees"
            aria-label="Search worktrees"
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setQuery("");
                e.currentTarget.blur();
              }
            }}
          />
        </label>
      </header>

      <Show when={model.notice}>
        {(n) => <NoticeBar text={n().text} tone={n().tone} onClose={() => be.clearNotice()} />}
      </Show>

      <main class="list">
        <For each={shown()}>
          {(repo) => (
            <RepoCard
              repo={repo}
              query={q()}
              open={!closed().has(repo.id)}
              onToggle={() => toggle(repo.id)}
              backend={be}
            />
          )}
        </For>
        <Show when={model.repos.length === 0}>
          <p class="empty">No repositories yet.</p>
        </Show>
        <Show when={model.repos.length > 0 && shown().length === 0}>
          <p class="empty">No worktree matches “{query().trim()}”.</p>
        </Show>
      </main>
    </div>
  );
}

function NoticeBar(props: { text: string; tone: "info" | "error"; onClose: () => void }) {
  const [details, setDetails] = createSignal(false);
  return (
    <div class={`notice ${props.tone}`} role={props.tone === "error" ? "alert" : "status"}>
      <pre class="notice-text">{details() ? props.text : noticeSummary(props.text)}</pre>
      <Show when={noticeIsLong(props.text)}>
        <button class="link" onClick={() => setDetails(!details())}>
          {details() ? "Less" : "Details…"}
        </button>
      </Show>
      <button class="icon-button" title="Dismiss" aria-label="Dismiss" onClick={props.onClose}>
        <I.Close />
      </button>
    </div>
  );
}

function RepoCard(props: {
  repo: Repo;
  query: string;
  open: boolean;
  onToggle: () => void;
  backend: Backend;
}) {
  const rows = createMemo(() => children(props.repo, props.query));
  return (
    <section class="card" classList={{ closed: !props.open }}>
      <header class="card-header" onClick={props.onToggle}>
        <button
          class="disclosure"
          aria-expanded={props.open}
          aria-label={props.open ? `Collapse ${props.repo.name}` : `Expand ${props.repo.name}`}
        >
          <I.Chevron />
        </button>
        <div class="card-title">
          <div class="line">
            <span class="repo-name">{props.repo.name}</span>
            <span class="meta">{repoMeta(props.repo, props.query)}</span>
            <Show when={!props.repo.loaded}>
              <span class="meta loading">loading…</span>
            </Show>
          </div>
          <div class="path" title={props.repo.path}>
            {props.repo.displayPath}
          </div>
          <Show when={props.repo.error}>{(e) => <div class="repo-error">{e()}</div>}</Show>
        </div>
      </header>
      <Show when={props.open}>
        <div class="rows">
          <For each={rows()}>
            {(c) =>
              c.kind === "worktree" ? (
                <WorktreeRow repo={props.repo} w={c.worktree} backend={props.backend} />
              ) : (
                <PendingRow p={c.pending} />
              )
            }
          </For>
        </div>
      </Show>
    </section>
  );
}

function BranchLabel(props: { branch: string | null }) {
  const split = () => (props.branch ? splitAgentPrefix(props.branch) : null);
  return (
    <span class="branch" aria-label={props.branch ?? "detached HEAD"}>
      <Show when={split()} fallback={props.branch ?? "(detached)"}>
        {(s) => (
          <>
            {s()[0] === "claude" ? <I.ClaudeMark /> : <I.CursorMark />}
            {s()[1]}
          </>
        )}
      </Show>
    </span>
  );
}

function WorktreeRow(props: { repo: Repo; w: Worktree; backend: Backend }) {
  const be = props.backend;
  const missing = () => props.w.prunable;
  const idle = () => props.w.busy === null && !missing();
  const act = (f: (repoId: string, path: string) => void) => () => f(props.repo.id, props.w.path);
  const at = (f: (path: string) => void) => () => f(props.w.path);
  return (
    <article class="row" classList={{ busy: props.w.busy !== null, missing: missing() }}>
      <div class="line">
        <BranchLabel branch={props.w.branch} />
        <Show when={props.w.busy}>
          {(b) => (
            <span class="busy-label">
              <span class="spinner" aria-hidden="true" />
              {BUSY_LABEL[b()]}
            </span>
          )}
        </Show>
        <span class="badges">
          <For each={badgesFor(props.w, props.repo.mainBranch)}>
            {(b) => (
              <span class={`badge ${b.hue} ${b.emphasis}`} title={b.tooltip}>
                {b.text}
              </span>
            )}
          </For>
        </span>
      </div>
      <div class="line">
        <span class="path" title={props.w.path}>
          {props.w.displayPath}
        </span>
        <span class="actions">
          <Action hint="Push" disabled={!idle()} onPress={act(be.push)} icon={I.Push} />
          <Action hint="Pull (fast-forward only)" disabled={!idle()} onPress={act(be.pull)} icon={I.Pull} />
          <Action
            hint={`Pull ${props.repo.mainBranch} into this branch`}
            disabled={!idle()}
            onPress={act(be.pullMain)}
            icon={I.Merge}
          />
          <span class="gap" />
          <Action hint="Open in editor" disabled={missing()} onPress={at(be.openInEditor)} icon={I.Editor} />
          <Action hint="Open in terminal" disabled={missing()} onPress={at(be.openInTerminal)} icon={I.Terminal} />
          <Action
            hint={`Reveal in ${be.boot.fileManager}`}
            disabled={missing()}
            onPress={at(be.reveal)}
            icon={I.Folder}
          />
        </span>
      </div>
    </article>
  );
}

function Action(props: {
  hint: string;
  disabled: boolean;
  onPress: () => void;
  icon: (p: { class?: string }) => JSX.Element;
}) {
  return (
    <button
      class="icon-button"
      title={props.hint}
      aria-label={props.hint}
      disabled={props.disabled}
      onClick={() => props.onPress()}
    >
      {props.icon({})}
    </button>
  );
}

function PendingRow(props: { p: Pending }) {
  return (
    <article class="row pending" classList={{ failed: props.p.error !== null }}>
      <div class="line">
        <BranchLabel branch={props.p.branch} />
        <Show
          when={props.p.error}
          fallback={
            <span class="busy-label">
              <span class="spinner" aria-hidden="true" />
              Creating…
            </span>
          }
        >
          {(e) => <span class="repo-error">{e()}</span>}
        </Show>
      </div>
    </article>
  );
}
