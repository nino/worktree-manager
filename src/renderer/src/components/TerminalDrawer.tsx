import { useEffect, useRef } from "react";
import { Square, X } from "lucide-react";
import type { RunningCommand } from "@shared/types";
import { api } from "../api";
import { displayPath } from "../format";
import { useRuns, sameRun, type RunSelection } from "../runs";

const ICON = { size: 13, strokeWidth: 1.75 } as const;

// White Platinum-ish terminal theme (classic Terminal.app was light too).
const TERMINAL_THEME = {
  background: "#ffffff",
  foreground: "#000000",
  cursor: "#000000",
  selectionBackground: "#b4d5fe",
} as const;

// MARK: Terminal view

/**
 * Renders one run's output in a real xterm.js terminal, so ANSI colours and
 * cursor control sequences (progress bars, spinners, clears) render correctly.
 * Subscribes to live output first, then writes the backlog, so a drawer opened
 * mid-run shows history in order and keeps updating. xterm is imported lazily so
 * it never loads outside the browser (e.g. in unit tests). Remounted (via `key`)
 * whenever the selection changes, so the terminal resets cleanly.
 */
function TerminalView({ selection }: { selection: RunSelection }) {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let cancelled = false;
    let cleanup: (() => void) | undefined;

    void (async () => {
      const [{ Terminal }, { FitAddon }] = await Promise.all([
        import("@xterm/xterm"),
        import("@xterm/addon-fit"),
      ]);
      if (cancelled) return;

      const term = new Terminal({
        convertEol: true,
        scrollback: 5000,
        fontFamily: '"Monaco", "Courier New", ui-monospace, monospace',
        fontSize: 11,
        theme: { ...TERMINAL_THEME },
      });
      const fit = new FitAddon();
      term.loadAddon(fit);
      term.open(host);
      fit.fit();

      // Hold live chunks until the backlog is written so ordering is preserved.
      let backlogWritten = false;
      const pending: string[] = [];
      const onChunk = (chunk: string) => {
        if (backlogWritten) term.write(chunk);
        else pending.push(chunk);
      };

      const offOutput = api.onCommandOutput((e) => {
        if (e.worktreePath === selection.worktreePath && e.commandId === selection.commandId) {
          onChunk(e.chunk);
        }
      });
      const offExit = api.onCommandExit((e) => {
        if (e.worktreePath === selection.worktreePath && e.commandId === selection.commandId) {
          const how = e.signal ? `signal ${e.signal}` : `code ${e.exitCode ?? 0}`;
          onChunk(`\r\n[process exited — ${how}]\r\n`);
        }
      });

      const buf = await api.getCommandBuffer(selection.worktreePath, selection.commandId);
      if (!cancelled && buf) term.write(buf);
      backlogWritten = true;
      for (const c of pending) term.write(c);
      pending.length = 0;

      const resize = new ResizeObserver(() => {
        try {
          fit.fit();
        } catch {
          // Container not laid out (e.g. drawer hidden) — ignore.
        }
      });
      resize.observe(host);

      cleanup = () => {
        offOutput();
        offExit();
        resize.disconnect();
        term.dispose();
      };
    })();

    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [selection.worktreePath, selection.commandId]);

  return <div className="terminal-xterm" ref={hostRef} />;
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
