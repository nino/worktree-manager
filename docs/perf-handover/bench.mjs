/**
 * Paint-cost A/B for the renderer's stylesheet: traces Chromium's rasteriser
 * while a window resize and a row-arrival animation play, against both the
 * working-tree styles.css and the one at a chosen git ref.
 *
 * This is the harness that produced the numbers in README.md here. It renders a
 * static replica of the app's DOM (markup.mjs) rather than booting Electron, so
 * only the CSS is under test — which is the point: it isolates paint cost from
 * React, IPC and git.
 *
 *   BASE=HEAD~1 CHROME=/path/to/Chrome node docs/perf-handover/bench.mjs
 *
 * CHROME defaults to Google Chrome's macOS location. Any Chromium build works;
 * playwright-core ships no browser of its own. Absolute times are meaningless
 * across machines (they track the rasteriser's load) — only the ratios are.
 */
import { join } from "node:path";

const ROOT = join(import.meta.dirname, "../..");
const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const BASE = process.env.BASE ?? "HEAD";

import { readFileSync } from "node:fs";
import { execSync } from "node:child_process";
import { chromium } from "playwright-core";
import { page as makePage } from "./markup.mjs";

const NEW = readFileSync(join(ROOT, "src/renderer/src/styles.css"), "utf8");
const OLD = execSync(`git show ${BASE}:src/renderer/src/styles.css`, {
  cwd: ROOT,
  encoding: "utf8",
});

// Inline the committed PNGs so the harness needs no asset server.
const uri = (f) =>
  `data:image/png;base64,${readFileSync(join(ROOT, "src/renderer/src/assets", f)).toString("base64")}`;
const inlined = NEW.replace('url("./assets/grain.png")', `url("${uri("grain.png")}")`).replace(
  'url("./assets/grain@2x.png")',
  `url("${uri("grain@2x.png")}")`,
);

const browser = await chromium.launch({ executablePath: CHROME });
const CATS = ["disabled-by-default-devtools.timeline"];
const KEEP = new Set(["UpdateLayoutTree", "Layout", "Paint", "PrePaint", "Commit", "RasterTask"]);

async function trace(css, act) {
  const ctx = await browser.newContext({
    viewport: { width: 1080, height: 760 },
    deviceScaleFactor: 2,
  });
  const pg = await ctx.newPage();
  await pg.setContent(makePage(css), { waitUntil: "load" });
  await pg.waitForTimeout(500);
  const client = await ctx.newCDPSession(pg);
  const ev = [];
  client.on("Tracing.dataCollected", ({ value }) => ev.push(...value));
  const done = new Promise((r) => client.once("Tracing.tracingComplete", r));
  await client.send("Tracing.start", {
    traceConfig: { includedCategories: CATS },
    transferMode: "ReportEvents",
  });
  await act(pg);
  await client.send("Tracing.end");
  await done;
  let total = 0;
  for (const e of ev) if (e.ph === "X" && KEEP.has(e.name)) total += e.dur / 1000;
  await ctx.close();
  return total;
}

const resize = async (pg) => {
  for (let i = 0; i < 30; i++)
    await pg.setViewportSize({ width: 700 + ((i * 9) % 380), height: 760 });
  await pg.waitForTimeout(200);
};
const arrive = async (pg) => {
  await pg.evaluate(() =>
    [...document.querySelectorAll(".wt-row")].slice(2, 5).forEach((r) => r.classList.add("wt-new")),
  );
  await pg.waitForTimeout(1400);
};

const res = {};
for (let r = 0; r < 4; r++)
  for (const [label, css] of [
    ["old", OLD],
    ["new", inlined],
  ])
    for (const [act, fn] of [
      ["resize", resize],
      ["arrival", arrive],
    ]) {
      const k = `${act}/${label}`;
      (res[k] ??= []).push(await trace(css, fn));
    }

// Visual: capture the sheen mid-sweep in both, and a row at rest.
async function shotAt(css, ms) {
  const ctx = await browser.newContext({
    viewport: { width: 900, height: 300 },
    deviceScaleFactor: 2,
  });
  const pg = await ctx.newPage();
  await pg.setContent(makePage(css));
  await pg.waitForTimeout(300);
  await pg.evaluate(() => document.querySelectorAll(".wt-row")[2].classList.add("wt-new"));
  await pg.waitForTimeout(ms);
  const b = await pg.locator(".wt-row").nth(2).screenshot();
  await ctx.close();
  return b.toString("base64");
}
async function diff(a, b) {
  const pg = await browser.newPage();
  const r = await pg.evaluate(
    async ([x, y]) => {
      const load = (d) =>
        new Promise((res) => {
          const i = new Image();
          i.onload = () => res(i);
          i.src = "data:image/png;base64," + d;
        });
      const [ia, ib] = await Promise.all([load(x), load(y)]);
      if (ia.width !== ib.width || ia.height !== ib.height) return { max: -1, mean: -1 };
      const px = (im) => {
        const c = document.createElement("canvas");
        c.width = im.width;
        c.height = im.height;
        const g = c.getContext("2d");
        g.drawImage(im, 0, 0);
        return g.getImageData(0, 0, im.width, im.height).data;
      };
      const da = px(ia),
        db = px(ib);
      let max = 0,
        sum = 0,
        n = 0;
      for (let i = 0; i < da.length; i += 4)
        for (let k = 0; k < 3; k++) {
          const d = Math.abs(da[i + k] - db[i + k]);
          if (d > max) max = d;
          sum += d;
          n++;
        }
      return { max, mean: sum / n };
    },
    [a, b],
  );
  await pg.close();
  return r;
}

const vis = {};
for (const ms of [0, 300, 550, 800, 1300])
  vis[ms] = await diff(await shotAt(OLD, ms), await shotAt(inlined, ms));
await browser.close();

console.log(`\nPerf vs ${BASE} (best of 4, DPR 2, 48 rows)\n`);
for (const act of ["resize", "arrival"]) {
  const o = Math.min(...res[`${act}/old`]),
    n = Math.min(...res[`${act}/new`]);
  console.log(
    `  ${act.padEnd(8)} ${BASE} ${o.toFixed(0).padStart(5)} ms → working tree ${n.toFixed(0).padStart(5)} ms   (${((n / o) * 100).toFixed(0)}%, ${(o / n).toFixed(1)}x)`,
  );
}
console.log("\nVisual diff of an arriving row (0 = pixel-identical)\n");
for (const [ms, d] of Object.entries(vis))
  console.log(
    `  t=${String(ms).padStart(4)}ms   max ${String(d.max).padStart(3)}/255   mean ${d.mean.toFixed(2)}`,
  );
