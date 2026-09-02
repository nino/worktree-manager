---
name: screenshot
description: Regenerate docs/screenshot.png (the README image) after any change to the renderer's layout or styling. Use when asked to update, refresh, or retake the screenshot, or after a restyle that the user wants to review visually. Covers running the capture on Linux (Claude Code on the web) as well as macOS.
---

# Regenerating the README screenshot

`docs/screenshot.png` is produced by `scripts/screenshot.mjs`, which builds
throwaway demo repos, seeds a sandboxed config profile, launches the production
build under Playwright, and captures the window. Never edit the PNG by hand and
never point the script at real user data.

## On macOS

```sh
pnpm screenshot
```

That builds `out/` and captures. A Retina display gives the 2x image directly.

## On Linux (Claude Code on the web)

The script supports Linux, but three things differ from a Mac:

1. **The Electron binary must be present.** `pnpm install` downloads it via the
   root `postinstall`. Check `ls node_modules/electron/dist` shows more than
   the licence files; if it doesn't, run `corepack pnpm install`.
2. **There is no display.** Run the capture under Xvfb. The window is
   1080x760 CSS pixels and the script forces a device scale factor of 2, so the
   virtual screen has to be at least 2160x1520 or Chromium clamps the window
   and the image comes out small. Use a comfortably larger screen:

   ```sh
   corepack pnpm exec electron-vite build
   xvfb-run -a -s "-screen 0 2600x1800x24" node scripts/screenshot.mjs
   ```

   `pnpm screenshot` also works, wrapped the same way in `xvfb-run`.

3. **Running as root.** Chromium refuses to start its sandbox as root; the
   script already passes `--no-sandbox` in that one case. Do not add it
   anywhere else.

Fonts on Linux are DejaVu, not San Francisco or Menlo, so the image differs from
a Mac capture. That is fine for reviewing a layout change; say so when handing
the image over, and note that a Mac run of `pnpm screenshot` restores the real
fonts before publishing.

## Afterwards

- Confirm the size: `file docs/screenshot.png` should report 2160 x 1520.
- Look at the image (Read the PNG) before committing. Check every row type the
  demo data stages: primary, dirty, ahead/behind, no upstream, and the
  `claude/` and `cursor/` agent-prefix branches.
- Commit the PNG together with the styling change it documents.

## Changing what the shot shows

Demo repos and their states are built in `buildDemoRepos()` inside the script;
the sandboxed config (repos, commands, worktrees root) is `seedProfile()`. Add
new states there rather than in a real repo. The script cleans up `~/wtm-demo`
and the temp profile on exit, success or failure, and refuses to touch a
`~/wtm-demo` it did not create.
