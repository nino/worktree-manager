import { Check } from "lucide-react";

interface BranchOptionProps {
  id: string;
  /** Keyboard-highlighted (aria-selected), not necessarily the picked value. */
  active: boolean;
  /** Whether this option is the current/committed value — shows the checkmark. */
  checked: boolean;
  label: string;
  onMouseEnter: () => void;
  onClick: () => void;
}

/** A single row in a `.branch-picker-list` popup — shared by `BranchPicker` and `BaseRefPicker`. */
export function BranchOption({
  id,
  active,
  checked,
  label,
  onMouseEnter,
  onClick,
}: BranchOptionProps) {
  return (
    <li
      id={id}
      role="option"
      aria-selected={active}
      className="branch-picker-option"
      title={label}
      // Keep focus on the input so click-to-select still fires.
      onMouseDown={(e) => e.preventDefault()}
      onMouseEnter={onMouseEnter}
      onClick={onClick}
    >
      <span className="branch-picker-check" aria-hidden="true">
        {checked && <Check size={12} strokeWidth={2} />}
      </span>
      <span className="branch-picker-label">{label}</span>
    </li>
  );
}
