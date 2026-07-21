import { useEffect, useId, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check } from "lucide-react";
import { fuzzyFilterBranches } from "../fuzzy";

interface Props {
  /** Branch names fetched for the repo (may omit the current branch). */
  branches: string[];
  /** The worktree's current branch — always kept selectable. */
  current: string;
  disabled: boolean;
  /** Called with the chosen branch (never the current one). */
  onSelect: (branch: string) => void;
}

/**
 * A Platinum popup-menu-style branch picker with a fuzzy-finder dropdown.
 * Closed, it looks like the old native branch `<select>`; open, it reveals a
 * filter input above a scrollable, fuzzy-matched, keyboard-navigable list. The
 * menu is portalled to `document.body` so the repo panel's `overflow: hidden`
 * can't clip it, and it re-anchors to the trigger while the tree scrolls.
 */
export function BranchPicker({ branches, current, disabled, onSelect }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const [pos, setPos] = useState({ top: 0, left: 0, width: 0 });

  const triggerRef = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const listboxId = useId();

  // MARK: Derived options

  // Keep the current branch selectable even when it is missing from the
  // fetched list (e.g. it was just created, or the list is stale).
  const allBranches = useMemo(
    () => (branches.includes(current) ? branches : [current, ...branches]),
    [branches, current],
  );
  const filtered = useMemo(() => fuzzyFilterBranches(query, allBranches), [query, allBranches]);
  // Clamp the highlight to the current result set.
  const active = filtered.length === 0 ? -1 : Math.min(activeIndex, filtered.length - 1);

  // MARK: Open / close

  const anchor = () => {
    const el = triggerRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({ top: r.bottom + 2, left: r.left, width: r.width });
  };

  const openMenu = () => {
    if (disabled) return;
    setQuery("");
    setActiveIndex(Math.max(0, allBranches.indexOf(current)));
    anchor();
    setOpen(true);
  };

  const close = (refocus = false) => {
    setOpen(false);
    setQuery("");
    if (refocus) triggerRef.current?.focus();
  };

  const choose = (branch: string, refocus = false) => {
    close(refocus);
    if (branch !== current) onSelect(branch);
  };

  // MARK: Effects

  // Focus the filter input as soon as the menu opens.
  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  // Re-anchor to the trigger while the tree scrolls or the window resizes.
  useEffect(() => {
    if (!open) return;
    const onMove = () => anchor();
    window.addEventListener("scroll", onMove, true);
    window.addEventListener("resize", onMove);
    return () => {
      window.removeEventListener("scroll", onMove, true);
      window.removeEventListener("resize", onMove);
    };
  }, [open]);

  // Close when clicking outside the trigger or the portalled menu.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      if (popRef.current?.contains(target)) return;
      close();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  // Keep the highlighted option scrolled into view during keyboard nav.
  useEffect(() => {
    if (!open || active < 0) return;
    const node = listRef.current?.children[active] as HTMLElement | undefined;
    node?.scrollIntoView({ block: "nearest" });
  }, [open, active]);

  // MARK: Keyboard

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex(Math.min(active + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex(Math.max(active - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (active >= 0) choose(filtered[active], true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close(true);
    }
  };

  // MARK: Render

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className="branch-select"
        disabled={disabled}
        aria-label="Switch branch"
        aria-haspopup="listbox"
        aria-expanded={open}
        title={`Switch branch (current: ${current})`}
        onClick={() => (open ? close() : openMenu())}
      >
        {current}
      </button>

      {open &&
        createPortal(
          <div
            ref={popRef}
            className="branch-picker-pop"
            style={{ top: pos.top, left: pos.left, minWidth: Math.max(pos.width, 180) }}
          >
            <input
              ref={inputRef}
              className="branch-picker-input"
              type="text"
              role="combobox"
              aria-expanded="true"
              aria-controls={listboxId}
              aria-autocomplete="list"
              aria-label="Filter branches"
              aria-activedescendant={active >= 0 ? `${listboxId}-opt-${active}` : undefined}
              placeholder="e.g., main"
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setActiveIndex(0);
              }}
              onKeyDown={onKeyDown}
            />
            <ul
              ref={listRef}
              id={listboxId}
              className="branch-picker-list"
              role="listbox"
              aria-label="Branches"
            >
              {filtered.length === 0 ? (
                <li className="branch-picker-empty" role="presentation">
                  No branches match
                </li>
              ) : (
                filtered.map((branch, index) => (
                  <li
                    key={branch}
                    id={`${listboxId}-opt-${index}`}
                    role="option"
                    aria-selected={index === active}
                    className="branch-picker-option"
                    title={branch}
                    // Keep focus on the input so click-to-select still fires.
                    onMouseDown={(e) => e.preventDefault()}
                    onMouseEnter={() => setActiveIndex(index)}
                    onClick={() => choose(branch, true)}
                  >
                    <span className="branch-picker-check" aria-hidden="true">
                      {branch === current && <Check size={12} strokeWidth={2} />}
                    </span>
                    <span className="branch-picker-label">{branch}</span>
                  </li>
                ))
              )}
            </ul>
          </div>,
          document.body,
        )}
    </>
  );
}
