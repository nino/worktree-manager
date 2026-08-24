# Renderer performance — handover

Branch: `claude/window-resize-animation-perf-etsw0g`. Commit `8f28dec` carries the
changes. **Delete this whole `docs/perf-handover/` directory before merging** —
it's working notes, not documentation the repo should keep.

Everything below was measured in a Linux container with **software rasterisation
and no GPU**. Absolute milliseconds mean nothing there. Ratios do, and the ratios
are what the changes were chosen on. Re-measuring on your Mac is the first job.

---

## What changed

### 1. The grain is now a baked PNG, not a live SVG filter

`--grain` was an `feTurbulence` data URI. About thirty rules carry it — every
button, every worktree row, every panel — and the rasteriser re-runs the filter
for every tile of every one of them. It was **a third of the paint cost of a
window resize**.

It's now a 1x/2x PNG pair in `src/renderer/src/assets/`, selected by
`image-set()`. The PNGs are that same filter's output, so the surface is
unchanged: mean delta **0.08/255** against the live filter over a whole repo
panel at DPR 2.

`scripts/bake-grain.mjs` owns the filter parameters and regenerates both tiles
(`pnpm bake-grain`). The texture stays tunable; it's just no longer re-computed
at paint time.

Two dead ends worth not repeating:

- Wrapping the PNG in an SVG to give it a 280×280 intrinsic size (which would
  have avoided touching `background-size` at ~30 call sites) came in at **138% of
  baseline** — worse than the original. An SVG wrapper defeats Chromium's image
  tile cache. `image-set` is the right tool.
- A 1x-only PNG performs identically to `image-set` but has 7× the visual delta
  at DPR 2 (mean 0.54 vs 0.08). Keep both tiles.

### 2. Arrival animations moved onto compositable properties

The glint animated `background-position`; the landing highlight animated a
`999px`-spread inset `box-shadow`. Neither is compositable, so **every frame
repainted the row's whole grain + gradient + blend-mode stack**. This is the
"animations don't feel smooth" complaint.

The glint is now a `transform` sweep and the highlight a veil fading its
`opacity`. Measured **~9× cheaper**.

The glint's travel is preserved exactly rather than eyeballed: a 55%-wide band
positioned from `-45%` to `145%` moves its left edge across `(100% - 55%)` of the
row, i.e. `-20.25%` → `65.25%`, which is what the `translateX` keyframes use.

Two things this dragged in:

- Under `prefers-reduced-motion` both pseudo-elements are now `display: none`
  rather than `animation: none`. They're only ever mid-animation states, so
  un-animating them would have parked a white veil on the row at full strength.
- The veil is positioned, so `.wt-info` and `.wt-actions` are lifted over it.
  They're named explicitly rather than matched with `> *` because `.wt-overlay`
  (the delete cover) is their sibling and is absolutely positioned — a blanket
  rule would reset it to `relative` and stop it covering the row.

### 3. The grow-box drag has back-pressure

`GrowBox` sent one `ipcRenderer.invoke` per `mousemove`. A trackpad emits well
above 60Hz and each resize costs a full relayout and repaint, so sizes queued up
and the window trailed the cursor.

It now keeps only the latest size, sends it once the previous resize is
acknowledged, and drops sizes the window already has. No IPC contract change —
`setWindowSize` still returns a promise, which is what provides the
back-pressure.

**This one is unmeasured.** It can't be exercised without a real window. It's
sound in principle, but it's the change most likely to feel different in a way I
couldn't see — including the possibility that it now feels _laggier_ if macOS's
resize acknowledgement is slow, in which case a `requestAnimationFrame` gate
instead of the in-flight gate is the fallback.

---

## Numbers

`BASE=HEAD~1 CHROME=<chromium> node docs/perf-handover/bench.mjs`, DPR 2, 48
worktree rows, best of four interleaved rounds:

| Scenario                    | Before |  After | Ratio |
| --------------------------- | -----: | -----: | ----: |
| 30 window resizes           | 6635ms | 4451ms |   67% |
| Arrival animation on 3 rows | 2161ms |  239ms |   11% |

Variant sweep that led to the grain decision (resize scenario, relative to the
unmodified stylesheet):

| Variant                                            | Cost |
| -------------------------------------------------- | ---: |
| baseline                                           | 100% |
| baked grain (**shipped**)                          |  68% |
| baked grain + `contain: layout paint` on `.wt-row` |  64% |
| baked grain + no `background-blend-mode`           |  55% |
| no grain at all                                    |  50% |
| no `--brush` hairlines                             |  94% |
| no 999px inset box-shadows                         | 100% |
| no `background-attachment: local` on `.tree`       | 100% |

Baking the grain gets two thirds of the way to "no grain at all" with no visual
change.

---

## Ruled out — don't re-investigate

- **`background-attachment: local` on `.tree`** — no measurable cost.
- **The `inset 0 0 0 999px` box-shadows** (hover, `.wt-main`) — no cost _at
  rest_. The huge spread looks alarming and isn't. Only _animating_ one was
  expensive, and that one is gone.
- **`--brush`**, the repeating-linear-gradient hairlines — 6% at most, and it's
  load-bearing for the texture. Leave it.
- **`contain: paint` clipping the branch popover** — it can't; `BranchPicker`
  renders through `createPortal`, so the menu isn't a descendant of the row.

---

## Open leads, roughly in order

1. **Re-measure on the Mac.** Everything above is software raster. macOS uses GPU
   rasterisation, where filters, blends and large shadows have a completely
   different cost profile — the grain win could be larger _or_ much smaller.
   Take a DevTools Performance trace while actually dragging the grow box, and
   check whether the bottleneck is even in the renderer: it may be in the main
   process or in macOS's own window-resize path, which no CSS change touches.

2. **Verify `scripts/bake-grain.mjs` runs.** I could not run it — Electron
   refuses to start as root in the container without `--no-sandbox`, and adding
   that flag to a macOS-targeted script would be wrong. **The committed PNGs are
   verified** (they're what I generated and measured, via Chromium). The script
   that regenerates them is not. Run `pnpm bake-grain` once and confirm
   `git diff --stat src/renderer/src/assets/` comes back empty or visually
   identical. If the script is broken, fix the script — don't touch the PNGs.

3. **Eyes on the arrival animation.** The pixel diff converges to ~0 once the
   animation ends, but mid-flight it's mean 2.8–4.5/255, which is a real if
   small difference in how the highlight reads. It should be checked on the
   brushed-metal surface at full size before you trust it. Revert with
   `git show 8f28dec -- src/renderer/src/styles.css` if it looks wrong; the
   grain and GrowBox changes are independent of it.

4. **`background-blend-mode`** is the next-largest lever — 68% → 55%. It can't
   just be dropped (the grain would cover the metal instead of modulating it),
   but the grain could plausibly be baked _pre-blended_ into the handful of
   surface gradients that actually use it. That's a bigger visual risk and wants
   a real display to judge.

5. **`contain: layout paint` on `.wt-row`** — a small further win (68% → 64%),
   mostly in layout. Safe from the popover angle (see above), but check the poof
   layer and the row's `wrap`ping actions bar.

6. **React re-render pressure — entirely unmeasured.** There is no `React.memo`
   anywhere in the renderer. Every `WorktreeRow` calls `useRuns()` and
   `useCreations()`, and both context values change identity on every running-
   commands poll (10s) — so every row re-renders on a timer, and again on the
   15s `useRepos` poll. This won't be the resize lag, but it's a plausible cause
   of general sluggishness. React DevTools Profiler will show it in a minute.

7. **`BranchPicker` re-anchors on every `resize` event** —
   `getBoundingClientRect()` (a forced layout) plus a `setState` per resize
   frame, while a menu is open. Rare in practice, cheap to fix with an rAF gate
   if you're in there anyway.

8. **`listReposWorktrees` on the 15s poll** shells out git per worktree. Worth
   timing in the main process against your real repo set — if it's slow it'll
   show up as periodic jank rather than resize lag.

---

## The harness

`bench.mjs` traces Chromium's rasteriser over a static replica of the app's DOM
(`markup.mjs`) — no Electron, no React, no IPC, so only the CSS is under test.
It A/Bs the working tree against any git ref and reports both paint cost and a
pixel diff.

```sh
CHROME=/Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
  BASE=HEAD~1 node docs/perf-handover/bench.mjs
```

`CHROME` defaults to that path; `playwright-core` ships no browser of its own.
`markup.mjs` is hand-written to match `App.tsx` → `RepoNode` → `WorktreeRow`, so
it needs updating if the row markup changes materially.

For measuring the real thing rather than the CSS in isolation, the trace has to
come from the running app — `pnpm dev`, then DevTools → Performance.
