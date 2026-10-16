// Icons as inline SVG, drawn after the SF Symbols the AppKit build uses
// (`symbol_name` in wtm-macos), at its 12-point medium weight. No font or
// image requests, and they take the text colour, so both appearances work.

import type { JSX } from "solid-js";

// `body` is a function: Solid's JSX makes real DOM nodes, and one node
// inserted in two rows would move rather than appear twice.
const icon = (body: () => JSX.Element) => (props: { class?: string }) => (
  <svg
    class={`icon ${props.class ?? ""}`}
    viewBox="0 0 16 16"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    {body()}
  </svg>
);

/// arrow.clockwise
export const Refresh = icon(() => (
  <>
    <path d="M13 8.5A5 5 0 1 1 11.2 4.6" />
    <path d="M11.6 1.8v3.2H8.4" />
  </>
));
/// arrow.up.to.line
export const Push = icon(() => <path d="M3 2.5h10M8 14V5.5M4.5 9 8 5.5 11.5 9" />);
/// arrow.down.to.line
export const Pull = icon(() => <path d="M3 13.5h10M8 2v8.5M4.5 7 8 10.5 11.5 7" />);
/// arrow.triangle.merge
export const Merge = icon(() => <path d="M8 14V7.5M8 7.5 3.5 3M8 7.5 12.5 3M5.5 9.5 8 7.5l2.5 2M3.5 3v2.5M3.5 3H6" />);
/// chevron.left.forwardslash.chevron.right
export const Editor = icon(() => <path d="M4.5 4.5 1.5 8l3 3.5M11.5 4.5l3 3.5-3 3.5M9.5 3l-3 10" />);
/// terminal
export const Terminal = icon(() => (
  <>
    <rect x="1.5" y="2.5" width="13" height="11" rx="1.8" />
    <path d="m4.5 6 2 1.8-2 1.8M8 10h3" />
  </>
));
/// folder
export const Folder = icon(() => (
  <path d="M1.5 4.3c0-.7.5-1.3 1.2-1.3h3.2l1.5 1.5h5.9c.7 0 1.2.5 1.2 1.2v6.6c0 .7-.5 1.2-1.2 1.2H2.7c-.7 0-1.2-.5-1.2-1.2zM1.5 6.5h13" />
));
/// doc.on.doc
export const Copy = icon(() => (
  <>
    <rect x="5" y="4.5" width="8" height="10" rx="1.5" />
    <path d="M3 11.5V3c0-.8.6-1.5 1.5-1.5H10" />
  </>
));
/// chevron.down (rotated for a closed card)
export const Chevron = icon(() => <path d="m4 6 4 4 4-4" />);
/// magnifyingglass
export const Search = icon(() => (
  <>
    <circle cx="7" cy="7" r="4.5" />
    <path d="m10.3 10.3 3.7 3.7" />
  </>
));
/// xmark
export const Close = icon(() => <path d="m4 4 8 8M12 4l-8 8" />);

/// Clawd, the Claude Code mascot, in his own colours (wtm-macos `toolicon`).
export const ClaudeMark = () => (
  <svg class="mark claude" viewBox="0 0 16 16" aria-hidden="true">
    <g fill="#d97757">
      <rect x="2" y="4.5" width="12" height="7.5" rx="2.5" />
      <rect x="4" y="12.5" width="2.4" height="2" rx="0.5" />
      <rect x="9.6" y="12.5" width="2.4" height="2" rx="0.5" />
    </g>
    <g fill="#17181a">
      <rect x="4.5" y="5" width="2.2" height="3.6" rx="0.6" />
      <rect x="9.3" y="5" width="2.2" height="3.6" rx="0.6" />
    </g>
  </svg>
);

/// Cursor's cube, in the colour of the text beside it.
export const CursorMark = () => (
  <svg class="mark cursor" viewBox="0 0 16 16" aria-hidden="true" fill="currentColor">
    <path d="M8 1.5 14 5 8 8.5 2 5z" opacity="0.9" />
    <path d="M2 5v6.5L8 15V8.5z" opacity="0.55" />
    <path d="M14 5v6.5L8 15V8.5z" opacity="0.35" />
    <path d="M2 5l12 6.5M8 8.5V15" stroke="currentColor" stroke-width="0.9" opacity="0.6" fill="none" />
  </svg>
);
