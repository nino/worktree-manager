import { useRef } from "react";
import { createPortal } from "react-dom";
import { BranchOption } from "./BranchOption";
import { useFuzzyListbox } from "../useFuzzyListbox";

interface BaseRefPickerProps {
  /** Local + remote-tracking branch names to offer as suggestions. */
  branches: string[];
  /** True while `branches` hasn't resolved yet, to distinguish "loading" from "none". */
  loading: boolean;
  /** Controlled value — may be a listed branch, a tag, a SHA, or freeform text. */
  value: string;
  placeholder?: string;
  onChange: (ref: string) => void;
}

/**
 * A plain text field with a fuzzy-filtered suggestion dropdown of existing
 * branches. Unlike `BranchPicker`, the field is the value: every keystroke
 * commits live via `onChange`, so typing a ref that matches nothing (a tag, a
 * SHA, a branch not yet fetched) still works — Enter just dismisses the
 * suggestion list instead of requiring a match.
 */
export function BaseRefPicker({
  branches,
  loading,
  value,
  placeholder,
  onChange,
}: BaseRefPickerProps) {
  const inputRef = useRef<HTMLInputElement>(null);

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
  } = useFuzzyListbox({ query: value, items: branches, anchorRef: inputRef });

  const pick = (branch: string) => {
    close();
    onChange(branch);
  };

  // Keys are only meaningful while the suggestion list is showing — when it's
  // closed there's nothing to navigate/pick/dismiss, so leave the event alone
  // (in particular, an unhandled Escape then bubbles up to close the dialog,
  // as expected, instead of being silently swallowed here).
  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!open) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex(Math.min(active + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex(Math.max(active - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (active >= 0) pick(filtered[active]);
      else close();
    } else if (e.key === "Escape") {
      e.preventDefault();
      // Stop this Escape from also bubbling to Modal's global keydown
      // listener, which would close the whole dialog instead of just the
      // suggestion list.
      e.stopPropagation();
      close();
    }
  };

  return (
    <>
      <input
        ref={inputRef}
        type="text"
        role="combobox"
        aria-expanded={open}
        aria-controls={listboxId}
        aria-autocomplete="list"
        aria-activedescendant={open && active >= 0 ? `${listboxId}-opt-${active}` : undefined}
        placeholder={placeholder}
        value={value}
        onFocus={() => openMenu(0)}
        onChange={(e) => {
          onChange(e.target.value);
          setActiveIndex(0);
          if (!open) openMenu(0);
        }}
        onKeyDown={onKeyDown}
      />

      {open &&
        createPortal(
          <div
            ref={popRef}
            className="branch-picker-pop"
            style={{ top: pos.top, left: pos.left, minWidth: Math.max(pos.width, 180) }}
          >
            <ul
              ref={listRef}
              id={listboxId}
              className="branch-picker-list"
              role="listbox"
              aria-label="Branches"
            >
              {filtered.length === 0 ? (
                <li className="branch-picker-empty" role="presentation">
                  {loading
                    ? "Loading branches…"
                    : value.trim()
                      ? `No matches — press Enter to use "${value.trim()}"`
                      : "No branches found"}
                </li>
              ) : (
                filtered.map((branch, index) => (
                  <BranchOption
                    key={branch}
                    id={`${listboxId}-opt-${index}`}
                    active={index === active}
                    checked={branch === value.trim()}
                    label={branch}
                    onMouseEnter={() => setActiveIndex(index)}
                    onClick={() => pick(branch)}
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
