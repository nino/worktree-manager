import {
  createContext,
  createEffect,
  createMemo,
  createSignal,
  For,
  on,
  onCleanup,
  onMount,
  Match,
  Show,
  Switch,
  useContext,
  type JSX,
} from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { Portal } from "solid-js/web";

import type { Backend } from "./backend";
import { badgesFor } from "./badges";
import { splitAgentPrefix } from "./branch";
import * as I from "./icons";
import { navigate, navRows, repoKey, worktreeKey, type NavKey, type RowKey } from "./nav";
import { noticeIsLong, noticeSummary } from "./notice";
import { choice, moveBy, openPicker, setQuery as setPickerQuery, type PickerState } from "./picker";
import { confirmDelete, forceDelete, type Question } from "./questions";
import { children, normalise, repoMeta, showsRepo } from "./search";
import { NewWorktreeSheet, RepoSettingsSheet, SettingsSheet } from "./sheets";
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

/// The sheet on screen; one at a time, as AppKit shows them.
type Dialog =
  | { kind: "question"; question: Question }
  | { kind: "error"; title: string; message: string }
  | { kind: "newWorktree"; repoId: string }
  | { kind: "repoSettings"; repoId: string }
  | { kind: "confirmRemove"; repoId: string; name: string }
  | { kind: "settings" };

/// An open branch picker, hung from one worktree's branch button.
interface OpenPicker {
  repoId: string;
  path: string;
  state: PickerState;
}

/// What the rows need from the window, without threading it through props.
interface Ui {
  backend: Backend;
  selected: () => RowKey | null;
  select: (key: RowKey) => void;
  picker: () => OpenPicker | null;
  openPicker: (repo: Repo, w: Worktree) => void;
  pickerQuery: (q: string) => void;
  pickerMove: (by: number) => void;
  pickerChoose: (row: number | null) => void;
  closePicker: () => void;
  askDelete: (repo: Repo, path: string) => void;
  newWorktree: (repoId: string) => void;
  repoSettings: (repoId: string) => void;
  /// Drag-and-drop of cards, by their headers.
  drag: () => { repoId: string; gap: number | null } | null;
  startDrag: (repoId: string) => void;
  endDrag: () => void;
}
const UiContext = createContext<Ui>();
const useUi = () => useContext(UiContext)!;

/// The branch button's switches are off while a row is busy or its folder
/// is gone, as are its git actions.
const idle = (w: Worktree) => w.busy === null && !w.prunable;

export function App(props: { backend: Backend; initial: Model }) {
  const be = props.backend;
  // One store for the core's model, patched in place on every push so that
  // only the rows whose data changed are touched.
  const [model, setModel] = createStore<Model>(structuredClone(props.initial));
  // Bumped on every model, for what must look again at each (an open New
  // Worktree sheet re-checks its fields).
  const [version, setVersion] = createSignal(0);
  const apply = (m: Model) => {
    setModel(reconcile(m, { key: "id" }));
    setVersion((v) => v + 1);
  };
  be.onModel(apply);
  // The DOM now holds the boot model, so the window can be shown with
  // content in its first frame; the answer is the model as it is by now.
  onMount(() => be.ready().then(apply));

  const [query, setQuery] = createSignal("");
  const q = createMemo(() => normalise(query()));

  // MARK: Closed cards

  const [closed, setClosed] = createSignal<ReadonlySet<string>>(new Set(be.boot.closedRepos));
  // A search opens every card, so that a match is never hidden in a closed
  // one; cards closed mid-search are closed only until it is cleared, and
  // what is remembered is how they were before it.
  const [closedInSearch, setClosedInSearch] = createSignal<ReadonlySet<string>>(new Set());
  createEffect(on(q, (now, was) => {
    if (!now || !was) setClosedInSearch(new Set<string>());
  }));
  const isOpen = (id: string) => !(q() ? closedInSearch() : closed()).has(id);
  const setOpen = (id: string, open: boolean) => {
    const flip = (s: ReadonlySet<string>) => {
      const next = new Set(s);
      if (open) next.delete(id);
      else next.add(id);
      return next;
    };
    if (q()) {
      setClosedInSearch(flip);
    } else {
      setClosed(flip);
      be.setClosedRepos([...closed()]);
    }
    // A worktree row that just went away cannot keep the selection.
    const sel = selected();
    if (!open && sel && sel !== repoKey(id) && !rows().some((r) => r.key === sel)) {
      setSelected(repoKey(id));
    }
  };

  // MARK: Selection and keys

  const shown = createMemo(() => model.repos.filter((r) => showsRepo(r, q())));
  const rows = createMemo(() => navRows(model.repos, q(), isOpen));
  const [selected, setSelected] = createSignal<RowKey | null>(null);
  const select = (key: RowKey) => setSelected(key);
  // A row the selection named that is gone (deleted, filtered out) leaves
  // nothing selected rather than something unexpected.
  createEffect(() => {
    const sel = selected();
    if (sel && !rows().some((r) => r.key === sel)) setSelected(null);
  });
  const reveal = (key: RowKey) =>
    queueMicrotask(() =>
      [...document.querySelectorAll<HTMLElement>("[data-key]")]
        .find((el) => el.dataset.key === key)
        ?.scrollIntoView?.({ block: "nearest" }),
    );

  const selectedRow = () => rows().find((r) => r.key === selected()) ?? null;
  const repoOf = (id: string) => model.repos.find((r) => r.id === id);
  const worktreeOf = (repoId: string, path: string) => repoOf(repoId)?.worktrees.find((w) => w.path === path);

  // MARK: The branch picker

  const [picker, setPicker] = createSignal<OpenPicker | null>(null);

  // MARK: Moving cards

  // Dragging a card's header: the gap it would land in, by the pointer
  // against each shown card's middle. Where that puts it among every repo
  // (a search hides some) is the core's `drop_target` to say.
  const [drag, setDrag] = createSignal<{ repoId: string; gap: number | null } | null>(null);
  const gapAt = (y: number) => {
    const cards = [...listEl.querySelectorAll<HTMLElement>(".card")];
    const at = cards.findIndex((c) => {
      const r = c.getBoundingClientRect();
      return y < r.top + r.height / 2;
    });
    return at < 0 ? cards.length : at;
  };
  const shownIds = () => shown().map((r) => r.id);
  const onDragOver = (e: DragEvent) => {
    const d = drag();
    if (!d) return;
    e.preventDefault();
    const gap = gapAt(e.clientY);
    if (gap !== d.gap) setDrag({ ...d, gap });
  };
  const onDrop = (e: DragEvent) => {
    const d = drag();
    setDrag(null);
    if (!d || d.gap === null) return;
    e.preventDefault();
    be.moveRepo(d.repoId, shownIds(), d.gap);
  };
  /// ⌥⌘↑/⌥⌘↓: what dragging the selected row's card one place does.
  const moveSelected = (up: boolean) => {
    const row = selectedRow();
    if (!row) return;
    const ids = shownIds();
    const from = ids.indexOf(row.repoId);
    const gap = up ? from - 1 : from + 2;
    if (from < 0 || gap < 0 || gap > ids.length) return;
    be.moveRepo(row.repoId, ids, gap);
    reveal(row.key);
  };
  const ui: Ui = {
    backend: be,
    selected,
    select,
    picker,
    openPicker: (repo, w) => {
      if (!idle(w)) return;
      setSelected(worktreeKey(w.path));
      setPicker({ repoId: repo.id, path: w.path, state: openPicker(repo.branches, w.branch) });
    },
    pickerQuery: (text) => setPicker((p) => p && { ...p, state: setPickerQuery(p.state, text) }),
    pickerMove: (by) => setPicker((p) => p && { ...p, state: moveBy(p.state, by) }),
    pickerChoose: (row) => {
      const p = picker();
      if (!p) return;
      const branch = choice(p.state, row);
      ui.closePicker();
      if (branch !== null) be.switchBranch(p.repoId, p.path, branch);
    },
    closePicker: () => {
      setPicker(null);
      listEl.focus();
    },
    askDelete: (repo, path) => {
      const question = confirmDelete(repo, path);
      if (question) setDialog({ kind: "question", question });
    },
    // Nothing can be created where nothing could be listed: the error
    // beside the path says why.
    newWorktree: (repoId) => {
      if (repoOf(repoId)?.error === null) setDialog({ kind: "newWorktree", repoId });
    },
    repoSettings: (repoId) => setDialog({ kind: "repoSettings", repoId }),
    drag,
    startDrag: (repoId) => setDrag({ repoId, gap: null }),
    // A drop outside the list, or a cancelled drag, moves nothing.
    endDrag: () => setDrag(null),
  };

  // MARK: The delete ladder

  const [dialog, setDialog] = createSignal<Dialog | null>(null);
  const closeDialog = () => {
    setDialog(null);
    listEl.focus();
  };
  const answer = async (question: Question) => {
    setDialog(null);
    const r = await be.deleteWorktree(question.request);
    if (r.ok) return;
    if (r.refusal === "dirty" && !question.request.force) {
      setDialog({ kind: "question", question: forceDelete(question.request) });
    } else {
      // The row was stale or the worktree gone: say why, and bring the
      // list back in line with git.
      setDialog({ kind: "error", title: "Could not delete worktree", message: r.message });
      be.refreshRepo(question.request.repoId);
    }
  };

  // MARK: Keys

  let search!: HTMLInputElement;
  let listEl!: HTMLElement;
  const NAV: Record<string, NavKey> = {
    ArrowUp: "up",
    ArrowDown: "down",
    ArrowLeft: "left",
    ArrowRight: "right",
    Home: "home",
    End: "end",
  };
  const onKey = (e: KeyboardEvent) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key === "f") {
      e.preventDefault();
      search.focus();
      search.select();
      return;
    }
    if (mod && e.key === "r") {
      e.preventDefault();
      be.refresh();
      return;
    }
    // A dialog or the picker has the keyboard; so does a text field.
    if (dialog() || picker()) return;
    if (mod && !e.shiftKey && (e.key === "o" || e.key === "," || e.key === "n")) {
      e.preventDefault();
      if (e.key === "o") be.addRepos();
      else if (e.key === ",") setDialog({ kind: "settings" });
      else {
        const row = selectedRow();
        if (row) ui.newWorktree(row.repoId);
      }
      return;
    }
    if (mod && e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      moveSelected(e.key === "ArrowUp");
      return;
    }
    const target = e.target instanceof Element ? e.target : null;
    if (target?.closest("input, textarea")) {
      // ↓ from the search field goes to the list, as in a Finder window.
      if (e.key !== "ArrowDown" || target !== search) return;
    }
    const row = selectedRow();
    const nav = !mod && !e.altKey && NAV[e.key];
    if (nav) {
      e.preventDefault();
      listEl.focus();
      const r = navigate(rows(), selected(), nav, isOpen);
      if (!r) return;
      if ("select" in r) {
        setSelected(r.select);
        reveal(r.select);
      } else if ("open" in r) setOpen(r.open, true);
      else setOpen(r.close, false);
      return;
    }
    if (!row) return;
    const w = row.path !== undefined ? worktreeOf(row.repoId, row.path) : undefined;
    // Return or ⌘T on a worktree: switch its branch. Return on a card's
    // header opens or closes it.
    if ((e.key === "Enter" && !mod) || (mod && e.key === "t")) {
      e.preventDefault();
      const repo = repoOf(row.repoId);
      if (w && repo) ui.openPicker(repo, w);
      else if (!w && e.key === "Enter") setOpen(row.repoId, !isOpen(row.repoId));
      return;
    }
    // ⌘⌫, as the Finder moves to the Bin: delete, which asks first.
    if (mod && e.key === "Backspace" && w && !w.isMain && w.busy === null) {
      e.preventDefault();
      const repo = repoOf(row.repoId);
      if (repo) ui.askDelete(repo, w.path);
    }
  };
  document.addEventListener("keydown", onKey);
  onCleanup(() => document.removeEventListener("keydown", onKey));

  return (
    <UiContext.Provider value={ui}>
      <div class="window" data-os={be.boot.os}>
        <header class="toolbar" data-tauri-drag-region>
          <Show when={be.boot.os === "macos" || be.boot.os === "browser"}>
            <h1 data-tauri-drag-region>Worktree Manager</h1>
          </Show>
          <span class="toolbar-space" data-tauri-drag-region />
          <div class="tool-group">
            <button class="tool" title="Add repository (⌘O)" aria-label="Add repository" onClick={() => be.addRepos()}>
              <I.Add />
            </button>
            <button
              class="tool"
              classList={{ spinning: model.refreshing }}
              title="Refresh all (⌘R)"
              aria-label="Refresh all"
              onClick={() => be.refresh()}
            >
              <I.Refresh />
            </button>
          </div>
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
          <div class="tool-group single">
            <button
              class="tool"
              title="Settings (⌘,)"
              aria-label="Settings"
              onClick={() => setDialog({ kind: "settings" })}
            >
              <I.Gear />
            </button>
          </div>
        </header>

        <Show when={model.notice}>
          {(n) => <NoticeBar text={n().text} tone={n().tone} onClose={() => be.clearNotice()} />}
        </Show>

        <main
          class="list"
          ref={listEl}
          tabIndex={-1}
          aria-label="Repositories"
          onDragOver={onDragOver}
          onDrop={onDrop}
        >
          <For each={shown()}>
            {(repo, i) => (
              <RepoCard
                dropBefore={drag()?.gap === i() && drag()?.repoId !== repo.id}
                dropAfter={drag()?.gap === shown().length && i() === shown().length - 1}
                repo={repo}
                query={q()}
                open={isOpen(repo.id)}
                onToggle={() => {
                  setSelected(repoKey(repo.id));
                  setOpen(repo.id, !isOpen(repo.id));
                }}
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

        <Switch>
          <Match when={only(dialog(), "question")}>
            {(d) => (
              <Alert
                title={d().question.title}
                message={d().question.message}
                confirm={d().question.confirm}
                critical={d().question.critical}
                onConfirm={() => void answer(d().question)}
                onCancel={closeDialog}
              />
            )}
          </Match>
          <Match when={only(dialog(), "error")}>
            {(d) => <Alert title={d().title} message={d().message} onConfirm={closeDialog} onCancel={closeDialog} />}
          </Match>
          <Match when={only(dialog(), "confirmRemove")}>
            {(d) => (
              <Alert
                title={`Remove “${d().name}” from the list?`}
                message="This does not touch any files on disk."
                confirm="Remove"
                onConfirm={() => {
                  be.removeRepo(d().repoId);
                  closeDialog();
                }}
                onCancel={closeDialog}
              />
            )}
          </Match>
          <Match when={only(dialog(), "newWorktree")}>
            {(d) => (
              <Show when={repoOf(d().repoId)} fallback={<Gone onClose={closeDialog} />}>
                {(repo) => <NewWorktreeSheet repo={repo()} backend={be} version={version} onClose={closeDialog} />}
              </Show>
            )}
          </Match>
          <Match when={only(dialog(), "repoSettings")}>
            {(d) => (
              <Show when={repoOf(d().repoId)} fallback={<Gone onClose={closeDialog} />}>
                {(repo) => (
                  <RepoSettingsSheet
                    repo={repo()}
                    backend={be}
                    onClose={closeDialog}
                    onRemove={() => setDialog({ kind: "confirmRemove", repoId: repo().id, name: repo().name })}
                  />
                )}
              </Show>
            )}
          </Match>
          <Match when={only(dialog(), "settings")}>
            <SettingsSheet model={model} backend={be} terminalNote={be.boot.terminalNote} onClose={closeDialog} />
          </Match>
        </Switch>
      </div>
    </UiContext.Provider>
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
  dropBefore: boolean;
  dropAfter: boolean;
}) {
  const ui = useUi();
  const key = () => repoKey(props.repo.id);
  const rows = createMemo(() => children(props.repo, props.query));
  const stop = (f: () => void) => (e: MouseEvent) => {
    // The band's own click opens and closes the card.
    e.stopPropagation();
    f();
  };
  return (
    <section
      class="card"
      classList={{
        closed: !props.open,
        dragging: ui.drag()?.repoId === props.repo.id,
        "drop-before": props.dropBefore,
        "drop-after": props.dropAfter,
      }}
    >
      <header
        class="band"
        classList={{ selected: ui.selected() === key() }}
        data-key={key()}
        draggable={true}
        onDragStart={(e) => {
          e.dataTransfer?.setData("text/plain", props.repo.path);
          if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
          ui.startDrag(props.repo.id);
        }}
        onDragEnd={() => ui.endDrag()}
        onClick={props.onToggle}
      >
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
          <div class="line tight">
            <span class="path" title={props.repo.path}>
              {props.repo.displayPath}
            </span>
            <CopyButton text={props.repo.path} hint="Copy path" />
          </div>
          <Show when={props.repo.error}>{(e) => <div class="repo-error">{e()}</div>}</Show>
        </div>
        <div class="band-actions">
          <button
            class="push small"
            disabled={props.repo.error !== null}
            title="New worktree (⌘N)"
            onClick={stop(() => ui.newWorktree(props.repo.id))}
          >
            New Worktree
          </button>
          <button
            class="icon-button"
            title="Repo settings"
            aria-label={`Settings for ${props.repo.name}`}
            onClick={stop(() => ui.repoSettings(props.repo.id))}
          >
            <I.Gear />
          </button>
        </div>
      </header>
      <Show when={props.open}>
        <div class="well">
          <For each={rows()}>
            {(c) =>
              c.kind === "worktree" ? <WorktreeRow repo={props.repo} w={c.worktree} /> : <PendingRow p={c.pending} />
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
    <span class="branch-name" aria-label={props.branch ?? "detached HEAD"}>
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

function WorktreeRow(props: { repo: Repo; w: Worktree }) {
  const ui = useUi();
  const be = ui.backend;
  const key = () => worktreeKey(props.w.path);
  const missing = () => props.w.prunable;
  const act = (f: (repoId: string, path: string) => void) => () => f(props.repo.id, props.w.path);
  const at = (f: (path: string) => void) => () => f(props.w.path);
  const shown = () => props.w.branch ?? "(detached)";
  let branchButton!: HTMLButtonElement;
  const picking = () => {
    const p = ui.picker();
    return p && p.path === props.w.path ? p : null;
  };
  return (
    <article
      class="plate"
      classList={{ busy: props.w.busy !== null, missing: missing(), selected: ui.selected() === key() }}
      data-key={key()}
      onMouseDown={() => ui.select(key())}
    >
      <div class="line">
        <span class="branch-anchor">
          <button
            ref={branchButton}
            class="branch"
            disabled={!idle(props.w)}
            title={`Switch branch (current: ${shown()})`}
            aria-haspopup="listbox"
            aria-expanded={picking() !== null}
            onClick={() => ui.openPicker(props.repo, props.w)}
          >
            <BranchLabel branch={props.w.branch} />
            <I.UpDown class="updown" />
          </button>
          <Show when={picking()}>{(p) => <BranchPicker picker={p()} anchor={branchButton} />}</Show>
        </span>
        <Show when={props.w.branch}>{(b) => <CopyButton text={b()} hint="Copy branch name" />}</Show>
        <span class="fill" />
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
        <CopyButton text={props.w.path} hint="Copy path" />
        <span class="fill" />
        <span class="actions">
          <Action hint="Push" disabled={!idle(props.w)} onPress={act(be.push)} icon={I.Push} />
          <Action hint="Pull (fast-forward only)" disabled={!idle(props.w)} onPress={act(be.pull)} icon={I.Pull} />
          <Action
            hint={`Pull ${props.repo.mainBranch} into this branch`}
            disabled={!idle(props.w)}
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
          {/* The primary tree cannot be deleted; the core refuses it too. */}
          <Show when={!props.w.isMain}>
            <span class="gap" />
            <Action
              hint="Delete worktree"
              class="danger"
              disabled={props.w.busy !== null}
              onPress={() => ui.askDelete(props.repo, props.w.path)}
              icon={I.Delete}
            />
          </Show>
        </span>
      </div>
    </article>
  );
}

/// A filter field over the repo's branches, hung below the branch button.
/// Typing ranks (`fuzzy.ts`), ↑/↓ move, Return switches, Escape or a click
/// elsewhere closes.
function BranchPicker(props: { picker: OpenPicker; anchor: HTMLElement }) {
  const ui = useUi();
  let root!: HTMLDivElement;
  let field!: HTMLInputElement;
  // In a portal, placed against the button: a card clips what it holds,
  // and a popover belongs over everything, as an NSPopover is.
  const [place, setPlace] = createSignal<JSX.CSSProperties>({});
  const position = () => {
    const a = props.anchor.getBoundingClientRect();
    const height = root.offsetHeight;
    const below = a.bottom + 6;
    const fitsBelow = below + height <= window.innerHeight - 8;
    setPlace({
      left: `${Math.max(8, Math.min(a.left, window.innerWidth - root.offsetWidth - 8))}px`,
      top: `${fitsBelow ? below : Math.max(8, a.top - 6 - height)}px`,
    });
  };
  onMount(() => {
    position();
    field.focus();
  });
  // Follows its button when the window resizes or the list scrolls.
  window.addEventListener("resize", position);
  document.addEventListener("scroll", position, true);
  onCleanup(() => {
    window.removeEventListener("resize", position);
    document.removeEventListener("scroll", position, true);
  });
  const away = (e: MouseEvent) => {
    if (!root.contains(e.target as Node)) ui.closePicker();
  };
  // Not this turn's mousedown: that is the click that opened it.
  queueMicrotask(() => document.addEventListener("mousedown", away));
  onCleanup(() => document.removeEventListener("mousedown", away));
  createEffect(() => {
    const i = props.picker.state.selected;
    if (i !== null) root.querySelector(`[data-row="${i}"]`)?.scrollIntoView?.({ block: "nearest" });
  });
  return (
    <Portal>
      <div
        class="popover"
        ref={root}
        style={place()}
        role="dialog"
        aria-label="Switch branch"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <input
          ref={field}
          class="popover-field"
          type="text"
          placeholder="e.g., main"
          aria-label="Filter branches"
          value={props.picker.state.query}
          onInput={(e) => ui.pickerQuery(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              ui.pickerMove(e.key === "ArrowDown" ? 1 : -1);
            } else if (e.key === "Enter") {
              e.preventDefault();
              ui.pickerChoose(props.picker.state.selected);
            } else if (e.key === "Escape") {
              e.preventDefault();
              ui.closePicker();
            }
          }}
        />
        <ul class="popover-list" role="listbox">
          <For each={props.picker.state.rows} fallback={<li class="popover-empty">No branch matches.</li>}>
            {(row, i) => (
              <li
                role="option"
                data-row={i()}
                aria-selected={props.picker.state.selected === i()}
                classList={{ selected: props.picker.state.selected === i() }}
                onClick={() => ui.pickerChoose(i())}
              >
                <span class="check">
                  <Show when={row.branch === props.picker.state.current}>
                    <I.Check />
                  </Show>
                </span>
                <Highlighted text={row.branch} positions={row.match.positions} />
              </li>
            )}
          </For>
        </ul>
      </div>
    </Portal>
  );
}

/// `text` with the characters at `positions` (code-point indices) in bold.
function Highlighted(props: { text: string; positions: number[] }) {
  const parts = () => {
    const out: { text: string; strong: boolean }[] = [];
    [...props.text].forEach((c, i) => {
      const strong = props.positions.includes(i);
      const last = out[out.length - 1];
      if (last && last.strong === strong) last.text += c;
      else out.push({ text: c, strong });
    });
    return out;
  };
  return (
    <span class="branch-option" aria-label={props.text}>
      <For each={parts()}>{(p) => (p.strong ? <b>{p.text}</b> : p.text)}</For>
    </span>
  );
}

/// The dialog of `kind`, when that is the one open.
function only<K extends Dialog["kind"]>(d: Dialog | null, kind: K): Extract<Dialog, { kind: K }> | false {
  return d?.kind === kind ? (d as Extract<Dialog, { kind: K }>) : false;
}

/// A repo went away while its sheet was open (removed elsewhere).
function Gone(props: { onClose: () => void }) {
  return <Alert title="That repository is no longer in the list." message="" onConfirm={props.onClose} onCancel={props.onClose} />;
}

/// An alert sheet, as AppKit's are: Escape is Cancel, and Return is Cancel
/// too while the other button would destroy something. Without `confirm`
/// it only says something, with an OK button.
function Alert(props: {
  title: string;
  message: string;
  confirm?: string;
  critical?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  let first: HTMLButtonElement | undefined;
  onMount(() => first?.focus());
  return (
    <div
      class="scrim"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          props.onCancel();
        }
      }}
    >
      <div class="sheet" role="alertdialog" aria-modal="true" aria-labelledby="sheet-title" aria-describedby="sheet-message">
        <Show when={props.critical}>
          <I.Warning class="sheet-icon" />
        </Show>
        <h2 id="sheet-title">{props.title}</h2>
        <Show when={props.message}>
          <p id="sheet-message">{props.message}</p>
        </Show>
        <div class="sheet-buttons">
          <Show
            when={props.confirm}
            fallback={
              <button ref={first} class="push default" onClick={props.onConfirm}>
                OK
              </button>
            }
          >
            {(label) => (
              <>
                <button ref={first} class="push" onClick={props.onCancel}>
                  Cancel
                </button>
                <button class="push destructive" onClick={props.onConfirm}>
                  {label()}
                </button>
              </>
            )}
          </Show>
        </div>
      </div>
    </div>
  );
}

function Action(props: {
  hint: string;
  disabled: boolean;
  onPress: () => void;
  icon: (p: { class?: string }) => JSX.Element;
  class?: string;
}) {
  return (
    <button
      class={`icon-button ${props.class ?? ""}`}
      title={props.hint}
      aria-label={props.hint}
      disabled={props.disabled}
      onClick={() => props.onPress()}
    >
      {props.icon({})}
    </button>
  );
}

/// Copies `text`; the button's hint says what was copied for a moment.
function CopyButton(props: { text: string; hint: string }) {
  const [copied, setCopied] = createSignal(false);
  const copy = (e: MouseEvent) => {
    // Inside a card's band, a click would also close the card.
    e.stopPropagation();
    navigator.clipboard?.writeText(props.text).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      },
      (err) => console.error("copy failed", err),
    );
  };
  return (
    <button class="icon-button copy" title={copied() ? "Copied" : props.hint} aria-label={props.hint} onClick={copy}>
      <I.Copy />
    </button>
  );
}

function PendingRow(props: { p: Pending }) {
  const ui = useUi();
  return (
    <article class="plate pending" classList={{ failed: props.p.error !== null }}>
      <div class="line">
        <span class="branch static">
          <BranchLabel branch={props.p.branch} />
        </span>
        <Show
          when={props.p.error}
          fallback={
            <span class="busy-label">
              <span class="spinner" aria-hidden="true" />
              Creating…
            </span>
          }
        >
          {(e) => (
            <>
              <span class="repo-error" title={e()}>
                {e()}
              </span>
              <span class="fill" />
              <button
                class="icon-button"
                title="Dismiss"
                aria-label="Dismiss"
                onClick={() => ui.backend.dismissCreation(props.p.id)}
              >
                <I.Close />
              </button>
            </>
          )}
        </Show>
      </div>
    </article>
  );
}
