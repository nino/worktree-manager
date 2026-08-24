/**
 * Bakes the brushed-aluminium grain tile into the PNGs styles.css references.
 *
 * The grain used to live in the `--grain` token as an `feTurbulence` SVG. That
 * made the rasteriser re-run the filter for every tile of every element
 * carrying the grain — ~30 rules, including every button and every worktree
 * row — which measured as a third of the paint cost of a window resize. Baking
 * the same filter's output to a tile is visually indistinguishable (mean delta
 * 0.08/255) and drops that cost to an ordinary image blit.
 *
 * The filter below is the source of truth for the texture: retune it here and
 * re-run `pnpm bake-grain` — never hand-edit the PNGs.
 *
 *   baseFrequency 0.004 0.7 — stretched wide, so the noise reads as brushed
 *                             streaks rather than sand
 *   saturate 0              — greyscale, so `overlay` only modulates lightness
 *   alpha slope 0.16        — faint; the metal gradients stay in charge
 *
 * Rendering runs in Electron's own Chromium (the same engine that paints the
 * app, and already a dependency) rather than a Playwright-managed browser, so
 * this needs no extra download — the same reasoning as scripts/screenshot.mjs.
 */
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const TILE = 280;
const SCALES = [1, 2];
const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, "../src/renderer/src/assets");

const svg = (px) =>
  `<svg xmlns='http://www.w3.org/2000/svg' width='${px}' height='${px}'>` +
  `<filter id='g'><feTurbulence type='fractalNoise' baseFrequency='0.004 0.7' numOctaves='4' seed='11' stitchTiles='stitch'/>` +
  `<feColorMatrix type='saturate' values='0'/>` +
  `<feComponentTransfer><feFuncA type='linear' slope='0.16'/></feComponentTransfer></filter>` +
  `<rect width='${px}' height='${px}' filter='url(%23g)'/></svg>`;

// MARK: Electron side

if (process.versions.electron) {
  const { app, BrowserWindow } = await import("electron");
  await app.whenReady();
  const win = new BrowserWindow({ show: false, width: TILE, height: TILE });
  await win.loadURL("data:text/html,<canvas id='c'></canvas>");

  for (const scale of SCALES) {
    const px = TILE * scale;
    // The SVG is authored at the target pixel size, so Chromium rasterises the
    // filter at that resolution: the 2x tile carries real retina detail rather
    // than an upscale of the 1x one.
    const dataUri = await win.webContents.executeJavaScript(`(async () => {
      const img = new Image();
      await new Promise((resolve, reject) => {
        img.onload = resolve;
        img.onerror = reject;
        img.src = "data:image/svg+xml,${encodeURIComponent(svg(px)).replace(/'/g, "%27")}";
      });
      const canvas = document.getElementById("c");
      canvas.width = ${px};
      canvas.height = ${px};
      canvas.getContext("2d").drawImage(img, 0, 0, ${px}, ${px});
      return canvas.toDataURL("image/png");
    })()`);

    const name = scale === 1 ? "grain.png" : `grain@${scale}x.png`;
    const bytes = Buffer.from(dataUri.split(",")[1], "base64");
    writeFileSync(join(OUT, name), bytes);
    console.log(`${name}  ${px}x${px}  ${(bytes.length / 1024).toFixed(1)} kB`);
  }

  app.quit();
} else {
  // MARK: Node side — re-exec this file under Electron.
  const electronPath = (await import("electron")).default;
  const child = spawn(electronPath, [fileURLToPath(import.meta.url)], { stdio: "inherit" });
  child.on("exit", (code) => process.exit(code ?? 1));
}
