// Icons as inline SVG: no font or image requests, and they take the text
// colour, so both appearances work.

import type { JSX } from "solid-js";

// `body` is a function: Solid's JSX makes real DOM nodes, and one node
// inserted in two rows would move rather than appear twice.
const icon = (body: () => JSX.Element) => (props: { class?: string }) => (
  <svg
    class={`icon ${props.class ?? ""}`}
    viewBox="0 0 16 16"
    fill="none"
    stroke="currentColor"
    stroke-width="1.4"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    {body()}
  </svg>
);

export const Refresh = icon(() =>
  <>
    <path d="M13 8a5 5 0 1 1-1.5-3.6" />
    <path d="M13 2.5v3h-3" />
  </>,
);
export const Push = icon(() =>
  <path d="M8 13V3M4 7l4-4 4 4" />);
export const Pull = icon(() =>
  <path d="M8 3v10M4 9l4 4 4-4" />);
export const Merge = icon(() =>
  <>
    <circle cx="4.5" cy="3.5" r="1.5" />
    <circle cx="4.5" cy="12.5" r="1.5" />
    <circle cx="11.5" cy="8" r="1.5" />
    <path d="M4.5 5v6M4.5 5c0 3 3 3 5.5 3" />
  </>,
);
export const Editor = icon(() =>
  <path d="M6 4 2 8l4 4M10 4l4 4-4 4" />);
export const Terminal = icon(() =>
  <>
    <rect x="1.5" y="2.5" width="13" height="11" rx="2" />
    <path d="m4.5 6 2 2-2 2M8.5 10.5h3" />
  </>,
);
export const Folder = icon(() =>
  <path d="M1.5 4.5a1 1 0 0 1 1-1h3.5l1.5 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" />);
export const Chevron = icon(() =>
  <path d="m6 4 4 4-4 4" />);
export const Search = icon(() =>
  <>
    <circle cx="7" cy="7" r="4.5" />
    <path d="m10.5 10.5 3.5 3.5" />
  </>,
);
export const Close = icon(() =>
  <path d="m4 4 8 8M12 4l-8 8" />);

/// Claude's mark: an eight-armed burst in its orange.
export const ClaudeMark = () => (
  <svg class="mark claude" viewBox="0 0 16 16" aria-hidden="true">
    <path
      d="M8 1.5v13M1.5 8h13M3.4 3.4l9.2 9.2M12.6 3.4l-9.2 9.2"
      stroke="currentColor"
      stroke-width="2.2"
      stroke-linecap="round"
    />
  </svg>
);

/// Cursor's mark: a cube's outline.
export const CursorMark = () => (
  <svg class="mark cursor" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path
      d="M8 1.5 14 5v6l-6 3.5L2 11V5zM2 5l6 3.5L14 5M8 8.5v6"
      stroke="currentColor"
      stroke-width="1.3"
      stroke-linejoin="round"
    />
  </svg>
);
