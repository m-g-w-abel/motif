// Jank measurement (docs/DECISION-UI-STACK.md): runs the phone's main
// transitions twice on a freshly loaded test page and reports, per
// transition, the frames drawn late (in-page rAF) and where the browser's time
// went (a DevTools trace). Needs the phone's WebView over adb as in CLAUDE.md,
// plus `adb reverse tcp:5182 tcp:5182` to a dev server whose engine is a muted
// test engine with a library and a track loaded, paused. Acts only on the dev
// server's page (its own origin and storage), never on the app's own page.
//
//   CDP=http://127.0.0.1:9333 node scripts/jank.mjs [base-url] [variant]
//
// variant names the run in the output (e.g. "baseline"); JANK_OUT is a file for
// the raw per-transition numbers as JSON.
import { execFileSync } from "node:child_process";
import fs from "node:fs";

const base = process.argv[2] ?? "http://localhost:5182";
const variant = process.argv[3] ?? "baseline";
const phone = process.env.CDP;
if (!phone) throw new Error("set CDP to the phone's DevTools address (docs/DECISION-UI-STACK.md)");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const target = (await (await fetch(`${phone}/json/list`)).json()).find((t) => t.type === "page");
const home = target.url.startsWith(base) ? "http://tauri.localhost/" : target.url;
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let next = 0;
const waiting = new Map();
const events = [];
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  const w = waiting.get(msg.id);
  if (!w) return void events.push(msg);
  waiting.delete(msg.id);
  if (msg.error) w.reject(new Error(msg.error.message));
  else w.resolve(msg.result);
};
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = ++next;
    waiting.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
const run = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
};
const until = async (expression, ms = 8000) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await run(expression)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${expression}`);
};

await send("Page.enable");
const origin = JSON.stringify(new URL(base).origin);
const { identifier } = await send("Page.addScriptToEvaluateOnNewDocument", {
  source: `if (location.origin === ${origin}) localStorage.setItem("engines", JSON.stringify({ engines: [{ id: "jank", name: "Jank check", addresses: [location.host], token: null }], current: "jank" }))`,
});
// An experiment: CSS (JANK_CSS) added to the test page, to see what a fix would be worth before writing it.
const css = process.env.JANK_CSS;
const injected = css
  ? await send("Page.addScriptToEvaluateOnNewDocument", {
      source: `if (location.origin === ${origin}) addEventListener('DOMContentLoaded', () => { const s = document.createElement('style'); s.textContent = ${JSON.stringify(css)}; document.head.append(s); })`,
    })
  : null;
let restored = false;
async function restore() {
  if (restored) return;
  restored = true;
  try {
    await send("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    if (injected) await send("Page.removeScriptToEvaluateOnNewDocument", { identifier: injected.identifier });
    await send("Page.navigate", { url: home });
  } catch {}
}
process.on("exit", () => ws.close());

const settled = `(() => {
  const screens = [...document.querySelectorAll('main > .screen')];
  return screens.filter((s) => getComputedStyle(s).visibility !== 'hidden').length === 1
    && !screens.some((s) => s.getAnimations().length)
    && !document.querySelector('.motion-layer')?.children.length
    && ![...document.querySelectorAll('.sheet, .bar')].some((e) => e.getAnimations({ subtree: true }).length);
})()`;

function adb(...args) {
  return execFileSync("adb", ["shell", ...args]);
}
async function drag(x0, y0, x1, y1) {
  const k = await run("devicePixelRatio");
  adb("input", "swipe", ...[x0, y0, x1, y1].map((v) => String(Math.round(v * k))), "300");
}
async function tap(selector) {
  const at = await run(`(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`);
  const k = await run("devicePixelRatio");
  adb("input", "tap", ...at.map((v) => String(Math.round(v * k))));
}

// Frames: rAF intervals, kept per transition (a label set by `mark`).
const recorder = `window.jank ??= (() => { const j = { cur: null, all: {} };
(function loop(t) {
  if (j.cur) { const a = (j.all[j.cur] ??= { last: null, d: [] }); if (a.last !== null) a.d.push(t - a.last); a.last = t; }
  requestAnimationFrame(loop);
})(0); return j; })(); window.jank.all = {}; 0`;
const mark = (label) => run(`(() => { const j = window.jank; if (j.cur) j.all[j.cur].last = null; j.cur = ${JSON.stringify(label)}; if (${JSON.stringify(label)}) { performance.mark('T:start:' + ${JSON.stringify(label)}); } return 0; })()`);

const CATEGORIES = [
  "-*", "devtools.timeline", "disabled-by-default-devtools.timeline", "v8", "v8.execute", "blink", "blink.user_timing", "cc", "gpu", "viz",
  "disabled-by-default-v8.compile", "disabled-by-default-devtools.timeline.frame", "disabled-by-default-devtools.timeline.layers", "latencyInfo", "benchmark", "rail", "input",
].join(",");

async function trace(body) {
  events.length = 0;
  const chunks = [];
  await send("Tracing.start", { traceConfig: { recordMode: "recordContinuously", includedCategories: CATEGORIES.split(",").filter((c) => c !== "-*") }, transferMode: "ReturnAsStream" });
  await body();
  await send("Tracing.end");
  let done;
  for (let i = 0; i < 400 && !done; i++) {
    done = events.find((e) => e.method === "Tracing.tracingComplete");
    if (!done) await sleep(50);
  }
  if (!done?.params.stream) throw new Error("no trace stream");
  for (;;) {
    const r = await send("IO.read", { handle: done.params.stream, size: 1 << 20 });
    chunks.push(r.data);
    if (r.eof) break;
  }
  await send("IO.close", { handle: done.params.stream });
  return JSON.parse(chunks.join("")).traceEvents ?? JSON.parse(chunks.join(""));
}

/** Where a window's time went, by thread kind and event name (durations of complete events, ms). */
function attribute(evs, from, to) {
  const threads = new Map();
  for (const e of evs) if (e.ph === "M" && e.name === "thread_name") threads.set(`${e.pid}:${e.tid}`, e.args.name);
  const sums = new Map();
  for (const e of evs) {
    if (e.ph !== "X" || e.ts < from || e.ts > to || !e.dur) continue;
    const th = threads.get(`${e.pid}:${e.tid}`) ?? "?";
    const kind = /CrRendererMain|^main$/i.test(th) ? "main" : /Raster|Compositor.?Tile|ThreadPool/i.test(th) ? "raster/pool" : /Compositor/i.test(th) ? "compositor" : /Viz|GPU|CrGpu/i.test(th) ? "gpu" : "other";
    const key = `${kind}|${e.name}`;
    sums.set(key, (sums.get(key) ?? 0) + e.dur / 1000);
  }
  return Object.fromEntries([...sums].sort((a, b) => b[1] - a[1]).slice(0, 40).map(([k, v]) => [k, +v.toFixed(1)]));
}

const GROUPS = {
  "script compile/eval": ["v8.compile", "V8.CompileCode", "V8.CompileLazy", "EvaluateScript", "v8.parseOnBackground", "V8.ParseFunction", "v8.produceCache", "V8.Compile"],
  "script run (calls)": ["FunctionCall", "v8.run", "RunMicrotasks", "FireAnimationFrame", "HitTest", "EventDispatch", "TimerFire", "V8.Execute"],
  "style/layout": ["UpdateLayoutTree", "Layout", "RecalculateStyles", "ScheduleStyleRecalculation", "UpdateLayer", "PrePaint", "IntersectionObserverController::computeIntersections"],
  "paint/layers/commit": ["Paint", "Layerize", "Commit", "CompositeLayers", "UpdateLayerTree", "Layer::PushPropertiesTo", "UpdateLayers"],
  "raster": ["RasterTask", "TileManager::FlushAndIssueSignals", "GPURasterizer", "RasterBufferImpl::Playback", "RasterizerImpl"],
  "image decode": ["ImageDecodeTask", "Decode Image", "Decode LazyPixelRef", "ImageFrameGenerator::decode", "DecodeImage", "Resize Image"],
  GC: ["MinorGC", "MajorGC", "V8.GC_MC_BACKGROUND_MARKING", "V8.GCScavenger", "BlinkGC.AtomicPhase", "V8.GC_SCAVENGER_BACKGROUND_SCAVENGE_PARALLEL", "V8.GCFinalizeMC"],
  "gpu/draw": ["DrawFrame", "GLRenderer::DrawFrame", "SkiaRenderer::DrawFrame", "Display::DrawAndSwap", "Graphics.Pipeline.DrawAndSwap", "GLES2DecoderImpl::HandleDrawElements", "SwapBuffers"],
};
function grouped(sums) {
  const out = {};
  for (const [g, names] of Object.entries(GROUPS)) {
    let t = 0;
    for (const [k, v] of Object.entries(sums)) if (names.includes(k.split("|")[1])) t += v;
    out[g] = +t.toFixed(1);
  }
  return out;
}

/** Frames the compositor reports in a window: presented, and dropped (the main thread, or the compositor, missed the deadline). */
function pipeline(evs, from, to) {
  const out = { presented: 0, dropped: 0, droppedMain: 0, droppedCompositor: 0 };
  for (const e of evs) {
    if (e.name !== "PipelineReporter" || e.ph !== "e" || e.ts < from || e.ts > to) continue;
    const st = e.args?.chrome_frame_reporter ?? {};
    if (st.state === "STATE_PRESENTED_ALL" || st.state === "STATE_PRESENTED_PARTIAL") out.presented++;
    else if (st.state === "STATE_DROPPED") {
      out.dropped++;
      if (st.reason === "REASON_MAIN_THREAD" || st.main_thread_frame_dropped) out.droppedMain++;
      else out.droppedCompositor++;
    }
  }
  return out;
}

function stats(d) {
  if (!d?.length) return null;
  const s = [...d].sort((a, b) => a - b);
  const refresh = s[Math.floor(s.length / 2)];
  const late = d.filter((x) => x > refresh * 1.5);
  return { frames: d.length, refreshMs: +refresh.toFixed(1), late: late.length, worstMs: +s.at(-1).toFixed(1), lateMs: +late.reduce((a, b) => a + b - refresh, 0).toFixed(0) };
}

const steps = [
  ["open album", async () => tap("main > .screen:not([inert]) .card:not(.loading)")],
  ["back (swipe)", async () => drag(20, 410, 300, 410)],
  ["open album again", async () => tap("main > .screen:not([inert]) .card:not(.loading)")],
  ["back (swipe 2)", async () => drag(20, 410, 300, 410)],
  ["sheet expand", async () => tap(".bar .identity")],
  ["sheet collapse", async () => { const [x, y] = await run("(() => { const r = document.querySelector('.sheet').getBoundingClientRect(); return [r.x + r.width / 2, r.y + 120]; })()"); return drag(x, y, x, y + 420); }],
];

const report = { variant, runs: [] };
try {
  for (const run_ of ["first", "second"]) {
    if (run_ === "first") {
      await send("Page.navigate", { url: `${base}/?dev-view=albums` });
      await until(`!!document.querySelector('main > .screen:not([inert]) .card:not(.loading) img')?.complete && ${settled}`, 30000);
      await sleep(1500);
    }
    await run(recorder);
    const evs = await trace(async () => {
      for (const [name, act] of steps) {
        await mark(`${run_}: ${name}`);
        process.stderr.write(`${run_}: ${name}\n`);
        const profiling = process.env.JANK_PROFILE === name;
        if (profiling) {
          await send("Profiler.enable");
          await send("Profiler.setSamplingInterval", { interval: 200 });
          await send("Profiler.start");
        }
        await act();
        await sleep(200);
        await until(settled, 8000).catch(() => {});
        if (profiling) {
          const { profile } = await send("Profiler.stop");
          const self = new Map();
          const dt = profile.timeDeltas;
          const byId = new Map(profile.nodes.map((n) => [n.id, n]));
          profile.samples.forEach((id, i) => {
            const f = byId.get(id).callFrame;
            const key = `${f.functionName || "(anon)"} ${f.url.split("/").slice(-2).join("/")}:${f.lineNumber}`;
            self.set(key, (self.get(key) ?? 0) + (dt[i] ?? 0) / 1000);
          });
          console.log(`profile of ${run_}: ${name}`);
          for (const [k, v] of [...self].sort((a, b) => b[1] - a[1]).slice(0, 14)) console.log(`  ${v.toFixed(1)}ms  ${k}`);
        }
        await sleep(300);
        await mark("");
        await sleep(200);
      }
    });
    const marks = evs.filter((e) => e.name?.startsWith("T:start:")).sort((a, b) => a.ts - b.ts);
    const frames = await run("JSON.stringify(Object.fromEntries(Object.entries(window.jank.all).map(([k, v]) => [k, v.d])))").then(JSON.parse);
    for (const [i, [name]] of steps.entries()) {
      const label = `${run_}: ${name}`;
      const m = marks.find((e) => e.name === `T:start:${label}`);
      const t0 = m?.ts ?? 0;
      const t1 = (marks[marks.indexOf(m) + 1]?.ts ?? t0 + 1.2e6);
      const sums = m ? attribute(evs, t0, Math.min(t1, t0 + 1.0e6)) : {};
      report.runs.push({ label, ...stats(frames[label]), pipeline: pipeline(evs, t0, Math.min(t1, t0 + 1.0e6)), causes: grouped(sums), top: sums });
    }
  }
} finally {
  await restore();
}
if (process.env.JANK_OUT) fs.writeFileSync(process.env.JANK_OUT, JSON.stringify(report, null, 1));
console.log(`variant: ${variant}`);
for (const r of report.runs) console.log(`${r.label.padEnd(26)} late ${String(r.late ?? "-").padStart(3)}/${r.frames ?? "-"}  worst ${r.worstMs ?? "-"}ms  over ${r.lateMs ?? "-"}ms  ${JSON.stringify(r.pipeline)}  ${JSON.stringify(r.causes)}`);
process.exit(0);
