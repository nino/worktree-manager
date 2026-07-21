import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Square, X } from "lucide-react";
import type { RunningCommand } from "@shared/types";
import { api } from "../api";
import { displayPath } from "../format";
import { stripAnsi } from "../ansi";
import { useRuns, sameRun, type RunSelection } from "../runs";

const ICON = { size: 13, strokeWidth: 1.75 } as const;

// MARK: Scrollback view

/**
 * Streams one run's output. Subscribes to live output first, then fetches the
 * existing backlog, so a drawer opened mid-run shows history and keeps updating.
 * Remounted (via `key`) whenever the selection changes, so state resets cleanly.
 */
function TerminalView({ selection }: { selection: RunSelection }) {
  const [text, setText] = useState("");
  const scrollRef = useRef<HTMLPreElement>(null);
  // Stick to the bottom unless the user has scrolled up to read history.
  const stickRef = useRef(true);

  useEffect(() => {
    let cancelled = false;
    let backlog = "";
    let live = "";
    const apply = () => {
      if (!cancelled) setText(backlog + live);
    };

    const offOutput = api.onCommandOutput((e) => {
      if (e.worktreePath !== selection.worktreePath || e.commandId !== selection.commandId) return;
      live += e.chunk;
      apply();
    });
    const offExit = api.onCommandExit((e) => {
      if (e.worktreePath !== selection.worktreePath || e.commandId !== selection.commandId) return;
      const how = e.signal ? `signal ${e.signal}` : `code ${e.exitCode ?? 0}`;
      live += `\n[process exited — ${how}]\n`;
      apply();
    });

    void api.getCommandBuffer(selection.worktreePath, selection.commandId).then((buf) => {
      backlog = buf;
      apply();
    });

    return () => {
      cancelled = true;
      offOutput();
      offExit();
    };
  }, [selection.worktreePath, selection.commandId]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    stickRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  };

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && stickRef.current) el.scrollTop = el.scrollHeight;
  }, [text]);

  return (
    <pre className="terminal-output" ref={scrollRef} onScroll={onScroll}>
      {stripAnsi(text)}
    </pre>
  );
}

// MARK: Drawer

function label(run: RunningCommand): string {
  return `${run.name} · ${displayPath(run.worktreePath)}`;
}

/** Bottom drawer hosting the integrated terminal for the selected run. */
export function TerminalDrawer() {
  const runs = useRuns();
  if (!runs.drawerOpen) return null;

  const { running, selected } = runs;
  const selectedRun = selected ? (running.find((r) => sameRun(selected, r)) ?? null) : null;
  const selectedRunning = selectedRun !== null;

  // Options: every running command, plus the selected one if it has since exited
  // (so its final output stays visible until the user switches away).
  const options: RunSelection[] = running.map((r) => ({
    worktreePath: r.worktreePath,
    commandId: r.commandId,
  }));
  const selectedInList = selected !== null && options.some((o) => sameRun(selected, o));
  if (selected && !selectedInList) options.push(selected);

  const currentIndex = selected ? options.findIndex((o) => sameRun(selected, o)) : -1;

  const optionLabel = (sel: RunSelection): string => {
    const run = running.find((r) => sameRun(sel, r));
    if (run) return label(run);
    return `${displayPath(sel.worktreePath)} (exited)`;
  };

  return (
    <section className="terminal-drawer" aria-label="Integrated terminal">
      <header className="terminal-head">
        <span className="terminal-title">Terminal</span>
        {options.length > 0 ? (
          <select
            className="branch-select terminal-picker"
            title="Choose which command's output to show"
            value={currentIndex}
            onChange={(e) => {
              const opt = options[Number(e.target.value)];
              if (opt) runs.view(opt);
            }}
          >
            {currentIndex === -1 && <option value={-1}>Select a command…</option>}
            {options.map((o, i) => (
              <option key={`${o.worktreePath}::${o.commandId}`} value={i}>
                {optionLabel(o)}
              </option>
            ))}
          </select>
        ) : (
          <span className="terminal-none">No commands running</span>
        )}
        <span className="spacer" />
        {selectedRunning && selected && (
          <button
            className="btn btn-sm btn-danger-ghost"
            title="Stop this command"
            aria-label="Stop this command"
            onClick={() => void runs.stop(selected.worktreePath, selected.commandId)}
          >
            <Square {...ICON} /> Stop
          </button>
        )}
        <button
          className="btn btn-sm btn-icon"
          title="Close terminal"
          aria-label="Close terminal"
          onClick={runs.closeDrawer}
        >
          <X {...ICON} />
        </button>
      </header>

      {selected ? (
        <TerminalView
          selection={selected}
          key={`${selected.worktreePath}::${selected.commandId}`}
        />
      ) : (
        <p className="terminal-empty">
          {running.length > 0
            ? "Select a running command above to view its output."
            : "Start a command from a worktree row to see its output here."}
        </p>
      )}
    </section>
  );
}
