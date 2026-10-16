// The sheets with fields: New Worktree, a repo's settings, the app's
// settings. Each keeps its own drafts, seeded when it opens; the model is
// never written back into a field while it is open (`wtm-ui`'s `dialogs`).

import { createEffect, createSignal, For, on, onMount, Show, type Accessor, type JSX } from "solid-js";

import type { Backend } from "./backend";
import type { Model } from "./wire/Model";
import type { NewWorktreeCheck } from "./wire/NewWorktreeCheck";
import type { NewWorktreeFields } from "./wire/NewWorktreeFields";
import type { Repo } from "./wire/Repo";
import type { Settings } from "./wire/Settings";

/// A form sheet over the window. Escape is Cancel; Return presses the
/// default button when it is on.
export function FormSheet(props: {
  title: string;
  subtitle?: string;
  children: JSX.Element;
  buttons: JSX.Element;
  onCancel: () => void;
  onSubmit: () => void;
}) {
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
      <form
        class="sheet form-sheet"
        role="dialog"
        aria-modal="true"
        aria-label={props.title}
        onSubmit={(e) => {
          e.preventDefault();
          props.onSubmit();
        }}
      >
        <h2>{props.title}</h2>
        <Show when={props.subtitle}>
          <p class="sheet-subtitle">{props.subtitle}</p>
        </Show>
        <div class="form">{props.children}</div>
        <div class="sheet-buttons row">{props.buttons}</div>
      </form>
    </div>
  );
}

/// One labelled row of a form, with an optional hint under it.
function Row(props: { label: string; for?: string; hint?: string; children: JSX.Element }) {
  return (
    <>
      <label class="form-label" for={props.for}>
        {props.label}
      </label>
      <div class="form-field">
        {props.children}
        <Show when={props.hint}>
          <p class="form-hint">{props.hint}</p>
        </Show>
      </div>
    </>
  );
}

// MARK: New Worktree

/// Name a branch, new or existing, and where it starts. Every change is
/// checked by the core (`new_worktree::check`), which also says where the
/// branch will come from; Create sends exactly what the last check said,
/// and is off while a check is outstanding.
export function NewWorktreeSheet(props: {
  repo: Repo;
  backend: Backend;
  /// Bumped on every new model, so the sheet re-checks against it.
  version: Accessor<number>;
  onClose: () => void;
}) {
  const be = props.backend;
  const [name, setName] = createSignal("");
  const [newBranch, setNewBranch] = createSignal(true);
  const [base, setBase] = createSignal(props.repo.defaultBaseRef);
  const [check, setCheck] = createSignal<{ fields: NewWorktreeFields; result: NewWorktreeCheck } | null>(null);
  const fields = (): NewWorktreeFields => ({
    repoId: props.repo.id,
    name: name(),
    newBranch: newBranch(),
    base: newBranch() ? base() : "",
  });
  const same = (a: NewWorktreeFields, b: NewWorktreeFields) =>
    a.name === b.name && a.newBranch === b.newBranch && a.base === b.base;

  let asked = 0;
  createEffect(
    on([name, newBranch, base, props.version], () => {
      const f = fields();
      const n = ++asked;
      void be.checkNewWorktree(f).then((result) => {
        // Answers can arrive out of order; only the latest counts.
        if (n === asked) setCheck({ fields: f, result });
      });
    }),
  );
  // The remote branches known now are as of the last fetch, which can be
  // minutes old; a branch pushed since would be taken for a new one.
  onMount(() => be.fetchRepo(props.repo.id));

  const current = () => {
    const c = check();
    return c && same(c.fields, fields()) ? c.result : null;
  };
  const create = () => current()?.create ?? null;
  const submit = () => {
    const request = create();
    if (!request) return;
    be.createWorktree(request);
    props.onClose();
  };
  let field!: HTMLInputElement;
  onMount(() => field.focus());

  // The note line is there even when empty, so the sheet never jumps.
  const note = () => {
    const c = check()?.result;
    if (!c) return { text: "", error: false };
    if (c.problem) return { text: c.problem, error: true };
    return { text: c.note ?? (c.folder ? `Folder: ${c.folder}` : ""), error: false };
  };

  return (
    <FormSheet
      title={`New worktree — ${props.repo.name}`}
      onCancel={props.onClose}
      onSubmit={submit}
      buttons={
        <>
          <button type="button" class="push" onClick={props.onClose}>
            Cancel
          </button>
          <button type="submit" class="push default" disabled={!create()}>
            Create
          </button>
        </>
      }
    >
      <Row label="Branch name:" for="nw-branch">
        <input
          ref={field}
          id="nw-branch"
          class="text-field"
          type="text"
          spellcheck={false}
          autocomplete="off"
          placeholder="e.g., feature/my-thing"
          value={name()}
          onInput={(e) => setName(e.currentTarget.value)}
        />
        <p class="form-note" classList={{ error: note().error }} aria-live="polite">
          {note().text}
        </p>
      </Row>
      <div class="form-label" />
      <div class="form-field">
        <div class="segmented" role="radiogroup" aria-label="Branch">
          <For each={["New branch", "Existing branch"]}>
            {(label, i) => (
              <button
                type="button"
                role="radio"
                aria-checked={newBranch() === (i() === 0)}
                classList={{ on: newBranch() === (i() === 0) }}
                onClick={() => setNewBranch(i() === 0)}
              >
                {label}
              </button>
            )}
          </For>
        </div>
      </div>
      <Show when={newBranch()}>
        <Row label="Base ref:" for="nw-base">
          <input
            id="nw-base"
            class="text-field"
            type="text"
            list="nw-bases"
            spellcheck={false}
            autocomplete="off"
            placeholder={`e.g., ${props.repo.defaultBaseRef}`}
            // A branch from a remote has no base.
            disabled={check()?.result.fromRemote ?? false}
            value={base()}
            onInput={(e) => setBase(e.currentTarget.value)}
          />
          <datalist id="nw-bases">
            <For each={props.repo.baseRefCandidates}>{(r) => <option value={r} />}</For>
          </datalist>
        </Row>
      </Show>
    </FormSheet>
  );
}

// MARK: Repo settings

export function RepoSettingsSheet(props: {
  repo: Repo;
  backend: Backend;
  onClose: () => void;
  onRemove: () => void;
}) {
  const [name, setName] = createSignal(props.repo.name);
  const [main, setMain] = createSignal(props.repo.mainBranch);
  const [init, setInit] = createSignal(props.repo.initCommand);
  const save = () => {
    props.backend.updateRepo({ repoId: props.repo.id, name: name(), mainBranch: main(), initCommand: init() });
    props.onClose();
  };
  let first!: HTMLInputElement;
  onMount(() => first.focus());
  return (
    <FormSheet
      title={`Repo settings — ${props.repo.name}`}
      subtitle={props.repo.displayPath}
      onCancel={props.onClose}
      onSubmit={save}
      buttons={
        <>
          <button type="button" class="push" onClick={props.onRemove}>
            Remove Repo…
          </button>
          <span class="fill" />
          <button type="button" class="push" onClick={props.onClose}>
            Cancel
          </button>
          <button type="submit" class="push default">
            Save
          </button>
        </>
      }
    >
      <Row label="Display name:" for="rs-name">
        <input
          ref={first}
          id="rs-name"
          class="text-field"
          placeholder="e.g., my-app"
          value={name()}
          onInput={(e) => setName(e.currentTarget.value)}
        />
      </Row>
      <Row label="Main branch:" for="rs-main" hint="Worktrees show ahead/behind counts relative to this branch.">
        <input
          id="rs-main"
          class="text-field"
          placeholder="e.g., main"
          value={main()}
          onInput={(e) => setMain(e.currentTarget.value)}
        />
      </Row>
      <Row label="Init command:" for="rs-init" hint="Runs inside each new worktree after it is created.">
        <input
          id="rs-init"
          class="text-field"
          placeholder="e.g., pnpm i"
          value={init()}
          onInput={(e) => setInit(e.currentTarget.value)}
        />
      </Row>
    </FormSheet>
  );
}

// MARK: App settings

export function SettingsSheet(props: { model: Model; backend: Backend; terminalNote: string; onClose: () => void }) {
  const s = props.model.settings;
  const [root, setRoot] = createSignal(s.worktreesRoot);
  const [editor, setEditor] = createSignal(s.editorCommand);
  const [include, setInclude] = createSignal(s.copyWorktreeInclude);
  const [channel, setChannel] = createSignal(s.updateChannel);
  const draft = (): Settings => ({
    worktreesRoot: root(),
    editorCommand: editor(),
    copyWorktreeInclude: include(),
    updateChannel: channel(),
  });
  const save = () => {
    if (!root().trim()) return;
    props.backend.setSettings(draft());
    props.onClose();
  };
  const browse = async () => {
    const picked = await props.backend.pickFolder();
    if (picked) setRoot(picked);
  };
  let first!: HTMLInputElement;
  onMount(() => first.focus());
  return (
    <FormSheet
      title="Settings"
      onCancel={props.onClose}
      onSubmit={save}
      buttons={
        <>
          <button type="button" class="push" onClick={props.onClose}>
            Cancel
          </button>
          <button type="submit" class="push default" disabled={!root().trim()}>
            Save
          </button>
        </>
      }
    >
      <Row
        label="Worktrees root:"
        for="st-root"
        hint="Worktrees are created under this folder, grouped by repo name."
      >
        <div class="field-with-button">
          <input
            ref={first}
            id="st-root"
            class="text-field"
            placeholder="e.g., ~/.claude-worktrees"
            value={root()}
            onInput={(e) => setRoot(e.currentTarget.value)}
          />
          <button type="button" class="push" onClick={() => void browse()}>
            Browse…
          </button>
        </div>
      </Row>
      <Row label="Editor command:" for="st-editor">
        <input
          id="st-editor"
          class="text-field"
          placeholder="e.g., code"
          value={editor()}
          onInput={(e) => setEditor(e.currentTarget.value)}
        />
        <p class="form-hint">
          Used by “Open in editor”. The worktree path is appended, or substituted for {"{path}"} if present.
        </p>
        <p class="form-hint">{props.terminalNote}</p>
      </Row>
      <Row
        label=".worktreeinclude:"
        for="st-include"
        hint="A repo’s .worktreeinclude, as Claude Code reads it, names ignored files such as .env in .gitignore syntax. When copying, new worktrees get the matching files from the repo’s main folder."
      >
        <select
          id="st-include"
          class="popup"
          value={include() ? "copy" : "ignore"}
          onChange={(e) => setInclude(e.currentTarget.value === "copy")}
        >
          <option value="ignore">Ignore</option>
          <option value="copy">Copy listed files</option>
        </select>
      </Row>
      <Row
        label="Software updates:"
        for="st-channel"
        hint="Beta builds arrive before they are released to everyone, and are less tested. Only an app installed from a release updates itself."
      >
        <select
          id="st-channel"
          class="popup"
          value={channel()}
          onChange={(e) => setChannel(e.currentTarget.value as Settings["updateChannel"])}
        >
          <option value="stable">Stable</option>
          <option value="beta">Beta</option>
        </select>
      </Row>
    </FormSheet>
  );
}
