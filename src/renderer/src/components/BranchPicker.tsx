import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { BranchOption } from "./BranchOption";
import { useFuzzyListbox } from "../useFuzzyListbox";

interface BranchPickerProps {
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
export function BranchPicker({ branches, current, disabled, onSelect }: BranchPickerProps) {
  const [query, setQuery] = useState("");
  const triggerRef = useRef<HTMLButtonElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Keep the current branch selectable even when it is missing from the
  // fetched list (e.g. it was just created, or the list is stale).
  const allBranches = useMemo(
    () => (branches.includes(current) ? branches : [current, ...branches]),
    [branches, current],
  );

  const {
    open,
    openMenu,
    close,
    filtered,
    active,
    setActiveIndex,
    popRef,
    listRef,
    pos,
    listboxId,
  } = useFuzzyListbox({ query, items: allBranches, anchorRef: triggerRef });

  const show = () => {
    if (disabled) return;
    setQuery("");
    openMenu(Math.max(0, allBranches.indexOf(current)));
  };

  const hide = (refocus = false) => {
    close();
    setQuery("");
    if (refocus) triggerRef.current?.focus();
  };

  const choose = (branch: string, refocus = false) => {
    hide(refocus);
    if (branch !== current) onSelect(branch);
  };

  // Focus the filter input as soon as the menu opens.
  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

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
      hide(true);
    }
  };

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
        onClick={() => (open ? hide() : show())}
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
                  <BranchOption
                    key={branch}
                    id={`${listboxId}-opt-${index}`}
                    active={index === active}
                    checked={branch === current}
                    label={branch}
                    onMouseEnter={() => setActiveIndex(index)}
                    onClick={() => choose(branch, true)}
                  />
                ))
              )}
            </ul>
          </div>,
          document.body,
        )}
    </>
  );
}
