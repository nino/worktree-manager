// The app's icons: Lucide's, named for what they do here, and the two
// agents' marks, which Lucide has no equivalent for. Lucide's are inline
// SVG in the text colour, so both appearances work, and each is imported
// on its own so the bundle carries only these.

import type { LucideProps } from "lucide-solid";
import ArrowDownToLine from "lucide-solid/icons/arrow-down-to-line";
import ArrowUpToLine from "lucide-solid/icons/arrow-up-to-line";
import CheckIcon from "lucide-solid/icons/check";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronsUpDown from "lucide-solid/icons/chevrons-up-down";
import Code from "lucide-solid/icons/code";
import CopyIcon from "lucide-solid/icons/copy";
import FolderIcon from "lucide-solid/icons/folder";
import GitMerge from "lucide-solid/icons/git-merge";
import Plus from "lucide-solid/icons/plus";
import RotateCw from "lucide-solid/icons/rotate-cw";
import SearchIcon from "lucide-solid/icons/search";
import Settings from "lucide-solid/icons/settings";
import SquareTerminal from "lucide-solid/icons/square-terminal";
import Trash from "lucide-solid/icons/trash";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import X from "lucide-solid/icons/x";
import type { Component } from "solid-js";

// Sized by the stylesheet (`.icon`); a little lighter than Lucide's 2 at
// these sizes, nearer the SF Symbols the AppKit build draws.
const icon = (Lucide: Component<LucideProps>) => (props: { class?: string }) => (
  <Lucide class={`icon ${props.class ?? ""}`} stroke-width={1.75} aria-hidden="true" />
);

export const Refresh = icon(RotateCw);
export const Push = icon(ArrowUpToLine);
export const Pull = icon(ArrowDownToLine);
export const Merge = icon(GitMerge);
export const Editor = icon(Code);
export const Terminal = icon(SquareTerminal);
export const Folder = icon(FolderIcon);
export const Copy = icon(CopyIcon);
/// Rotated for a closed card.
export const Chevron = icon(ChevronDown);
export const Search = icon(SearchIcon);
export const Close = icon(X);
export const Add = icon(Plus);
export const Gear = icon(Settings);
export const Delete = icon(Trash);
/// The branch button opens a list.
export const UpDown = icon(ChevronsUpDown);
export const Check = icon(CheckIcon);
export const Warning = icon(TriangleAlert);

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
