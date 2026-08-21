import { useEffect, useRef, useState } from "react";
import { Check, Copy } from "lucide-react";

interface Props {
  /** The real, unabbreviated text to put on the clipboard. */
  text: string;
  /** Tooltip / accessible name, e.g. "Copy path". */
  label: string;
}

const CONFIRM_MS = 1200;

/** Ghost icon button that copies `text` and flashes a checkmark. */
export function CopyButton({ text, label }: Props) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const copy = (event: React.MouseEvent) => {
    // Rows and headers behind this button have their own click handlers
    // (collapse toggles, pickers) — copying must never trigger them.
    event.stopPropagation();
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), CONFIRM_MS);
    });
  };

  return (
    <button
      className={`copy-btn${copied ? " copied" : ""}`}
      title={label}
      aria-label={label}
      onClick={copy}
    >
      {copied ? <Check size={11} strokeWidth={2.25} /> : <Copy size={11} strokeWidth={1.75} />}
    </button>
  );
}
