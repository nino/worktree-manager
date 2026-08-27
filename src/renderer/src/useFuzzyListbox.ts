import { useEffect, useId, useMemo, useRef, useState } from "react";
import { fuzzyFilterBranches } from "./fuzzy";

interface UseFuzzyListboxOptions {
  /** Current filter text. */
  query: string;
  /** Items to filter, in the caller's preferred order. */
  items: string[];
  /** Element the popup is positioned under and excluded from outside-click close
   *  (the trigger button for BranchPicker, the input itself for BaseRefPicker). */
  anchorRef: React.RefObject<HTMLElement | null>;
}

/**
 * Shared state machine for a portalled fuzzy-filtered listbox popup: open
 * state, highlighted index, position, and the effects that keep it anchored,
 * dismissible, and scrolled to the active option. Callers own the query
 * string and all keyboard/selection policy.
 */
export function useFuzzyListbox({ query, items, anchorRef }: UseFuzzyListboxOptions) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const [pos, setPos] = useState({ top: 0, left: 0, width: 0 });
  const popRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const listboxId = useId();

  const filtered = useMemo(() => fuzzyFilterBranches(query, items), [query, items]);
  const active = filtered.length === 0 ? -1 : Math.min(activeIndex, filtered.length - 1);

  const anchor = () => {
    const el = anchorRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({ top: r.bottom + 2, left: r.left, width: r.width });
  };

  const openMenu = (initialIndex = 0) => {
    setActiveIndex(initialIndex);
    anchor();
    setOpen(true);
  };
  const close = () => setOpen(false);

  // Re-anchor while the tree scrolls or the window resizes.
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

  // Close on outside mousedown.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (anchorRef.current?.contains(target)) return;
      if (popRef.current?.contains(target)) return;
      close();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  // Keep the highlighted option in view during keyboard nav.
  useEffect(() => {
    if (!open || active < 0) return;
    const node = listRef.current?.children[active] as HTMLElement | undefined;
    node?.scrollIntoView({ block: "nearest" });
  }, [open, active]);

  return {
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
  };
}
