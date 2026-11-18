// Landing check (docs/ANIMATION.md, Checking): drives the UI in headless
// Chromium through screen changes (taps and swipes, phone and desktop) and
// fails if any shared element's copy landed off its real element (box) or
// looking different (colour, weight, size), or if a change left anything
// behind (a second screen, copies, hidden elements).
//
//   node scripts/landing.mjs [base-url]
//
// base-url is a dev server (`npm run dev`; default http://localhost:5173)
// whose engine has a library with albums and a track loaded, paused (the
// now-playing sheet's checks need one). Use a muted test engine of your own
// (CLAUDE.md), not one playing. Needs Chromium (CHROME overrides it).
//
// On the phone: CDP=http://127.0.0.1:9333 drives the app's own WebView (a
// debug build, its DevTools forwarded as in CLAUDE.md, the app in front)
// instead, at the phone's real size, font scale and pixel ratio. base-url is
// then the dev server as the phone reaches it (`adb reverse tcp:5182
// tcp:5182`); the WebView goes back to the app's own page at the end.
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import zlib from "node:zlib";

const base = process.argv[2] ?? "http://localhost:5173";
const chrome = process.env.CHROME ?? "chromium";
const phone = process.env.CDP;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let browser;
let profile;
let target;
/**
 * Closes headless Chromium (its whole process group, so no helper outlives it)
 * and removes its profile: always, whether the run passes, fails, throws or is
 * interrupted. Safe to call more than once.
 */
function closeBrowser() {
  if (browser && browser.exitCode === null && browser.signalCode === null) {
    try {
      process.kill(-browser.pid, "SIGKILL");
    } catch {
      browser.kill("SIGKILL");
    }
  }
  if (profile) fs.rmSync(profile, { recursive: true, force: true });
  profile = undefined;
}
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"])
  process.on(signal, () => {
    closeBrowser();
    process.exit(130);
  });
process.on("uncaughtException", (e) => {
  closeBrowser();
  console.error(e);
  process.exit(1);
});
process.on("unhandledRejection", (e) => {
  closeBrowser();
  console.error(e);
  process.exit(1);
});
process.on("exit", closeBrowser);
if (phone) {
  target = (await (await fetch(`${phone}/json/list`)).json()).find((t) => t.type === "page");
  if (!target) throw new Error(`no page at ${phone}`);
} else {
  profile = fs.mkdtempSync(path.join(os.tmpdir(), "motif-landing-"));
  // Its own process group, so closing it takes every Chromium process with it.
  browser = spawn(chrome, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--disable-features=OverscrollHistoryNavigation", "about:blank"], { stdio: "ignore", detached: true });
  let port;
  for (let i = 0; i < 100 && !port; i++) {
    await sleep(100);
    try {
      port = fs.readFileSync(path.join(profile, "DevToolsActivePort"), "utf8").split("\n")[0];
    } catch {}
  }
  if (!port) throw new Error(`${chrome} didn't start`);
  target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
}
// Where the phone's WebView goes back to (the app's own page, even if a failed run left it on ours).
const home = target.url.startsWith(base) ? "http://tauri.localhost/" : target.url;

const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let next = 0;
const waiting = new Map();
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  const w = waiting.get(msg.id);
  if (!w) return;
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
const until = async (expression, ms = 5000) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await run(expression)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${expression}`);
};

let forget = async () => {};
if (phone) {
  // The app's WebView refuses its Tauri bridge (and so the embedded engine) to the dev server's
  // page; that page (its own storage, apart from the app's) uses the dev server's engine instead.
  await send("Page.enable");
  // Only there: never the app's own page, whose storage holds the user's engines and their tokens.
  const origin = JSON.stringify(new URL(base).origin);
  const { identifier } = await send("Page.addScriptToEvaluateOnNewDocument", {
    source: `if (location.origin === ${origin}) localStorage.setItem("engines", JSON.stringify({ engines: [{ id: "landing", name: "Landing check", addresses: [location.host], token: null }], current: "landing" }))`,
  });
  forget = () => send("Page.removeScriptToEvaluateOnNewDocument", { identifier });
}

/** The Scale setting the test page uses (`dev-scale`), in percent, and its font (`dev-font`, empty: the theme's). */
let scale = 100;
let font = "";

async function open(query, width, height) {
  if (phone && width > 700) return false;
  if (!phone) {
    await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: width <= 700 });
    await send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
  }
  await send("Page.navigate", { url: `${base}/?${query}&dev-scale=${scale}&dev-font=${encodeURIComponent(font)}` });
  await until(`!!window.motifLandings && ${settled}`, 20000);
  return true;
}

/**
 * A real finger from (x0, y) to (x1, y), x from the music area's left edge
 * (desktop has the left panel before it): touch events through the browser,
 * or on the phone through Android's input, so a browser that takes the touch
 * for its own pan (and cancels it) fails the check as it would fail the user.
 */
async function swipe(dx0, dx1, y) {
  const left = await run("document.querySelector('main').getBoundingClientRect().left");
  return drag(left + dx0, y, left + dx1, y);
}

/** A real finger from (x0, y0) to (x1, y1), in the page's pixels (`swipe`). */
async function drag(x0, y0, x1, y1) {
  if (phone) {
    // Screen pixels, below the status bar the WebView sits under.
    // Screen pixels; the app draws edge to edge, so the page starts at the screen's top.
    const k = await run("devicePixelRatio");
    execFileSync("adb", ["shell", "input", "swipe", ...[x0, y0, x1, y1].map((v) => String(Math.round(v * k))), "300"]);
    return sleep(100);
  }
  const at = (i) => [{ x: x0 + ((x1 - x0) * i) / 12, y: y0 + ((y1 - y0) * i) / 12, id: 1 }];
  await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: at(0) });
  for (let i = 1; i <= 12; i++) {
    await sleep(16);
    await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: at(i) });
  }
  await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
}

/** A finger dragging the middle of `selector` by `dy`. */
async function dragBy(selector, dy) {
  const [x, y] = await run(`(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`);
  return drag(x, y, x, y + dy);
}

/** One screen showing (another may be kept hidden beneath), none about to show or moving, no copies left, nothing moving in the sheet or the bar. */
const settled = `(() => {
  const screens = [...document.querySelectorAll('main > .screen')];
  return screens.filter((s) => getComputedStyle(s).visibility !== 'hidden').length === 1
    && !screens.some((s) => (!s.inert && getComputedStyle(s).visibility === 'hidden') || s.getAnimations().length)
    && !document.querySelector('.motion-layer')?.children.length
    && !document.querySelector('.sheet .leaving')?.children.length
    && !document.querySelector('.sheet .behind')?.children.length
    && ![...document.querySelectorAll('.sheet, .bar')].some((e) => e.getAnimations({ subtree: true }).some((a) => a.effect?.getTiming().fill === 'both'));
})()`;
const failures = [];

/**
 * Watches a change frame by frame: anything that moves (a shared element or
 * its copy) must move from the start (no delay, no early end), and, while a
 * finger drives it, the drawn progress (a whole-length track's) must start
 * where the change starts and never jump between frames.
 */
const watch = `{
  const rec = (window.motifWatch = { late: [], p: [], up: null });
  const t0 = performance.now();
  for (const end of ['pointerup', 'pointercancel', 'touchend']) addEventListener(end, () => (rec.up ??= performance.now()), { capture: true, once: true });
  const tick = (t) => {
    let drawn = null;
    for (const a of document.getAnimations()) {
      const g = a.effect?.getTiming?.();
      const el = a.effect?.target;
      if (!g || g.fill !== 'both' || !el) continue;
      const late = g.delay > 0.5 || g.endDelay > 0.5;
      if (late && (el.dataset?.move || el.closest?.('.motion-layer'))) rec.late.push(el.dataset?.move ?? (el.closest('.motion-layer > *')?.className || 'a copy'));
      if (!late && g.duration > 200 && drawn === null) drawn = a.effect.getComputedTiming().progress;
    }
    if (rec.up === null && drawn !== null) rec.p.push([t, drawn]);
    if (t - t0 < 1500) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
} 0`;
/** The most a dragged change may move in a frame's time, 16.7 ms (the finger itself moves up to about 0.1 in one here); a dropped frame is slow, not a jump. */
const STEP = 0.15;

/** Something moving: a change's tracks (filled both ways) in the page. */
const moving = "document.getAnimations().some((a) => a.effect?.getTiming().fill === 'both')";
/** A visible button where the change ended (the sheet's when it is open, else the screen's): its centre, unless something covers it there. */
const pressable = `(() => {
  const sheet = document.querySelector('.sheet.open');
  const area = sheet ?? document.querySelector('main > .screen:not([inert])');
  // Not a handle: its press steps the sheet as well as clicking.
  for (const b of area?.querySelectorAll('button:not(.step)') ?? []) {
    const r = b.getBoundingClientRect();
    const [x, y] = [r.x + r.width / 2, r.y + r.height / 2];
    if (r.width < 16 || r.height < 16 || y < 40 || y > innerHeight - 100 || x < 0 || x > innerWidth) continue;
    if (!b.contains(document.elementFromPoint(x, y))) continue;
    return [x, y];
  }
  return null;
})()`;

/**
 * As soon as a change stops moving, a real finger taps a button it left in
 * view: the click must arrive (it is swallowed here, so nothing happens).
 * Chromium drops taps while a fling it started runs, and a part still
 * hidden for its hand-over can't be pressed: either leaves buttons dead
 * just after a change.
 */
async function tapsRightAfter(name) {
  const end = Date.now() + 3000;
  while ((await run(moving)) && Date.now() < end) await sleep(10);
  const at = await run(pressable);
  if (!at) return;
  await run(`window.motifClicked = false; addEventListener('click', (e) => { window.motifClicked = true; e.preventDefault(); e.stopPropagation(); }, { capture: true, once: true }); 0`);
  if (phone) {
    const k = await run("devicePixelRatio");
    execFileSync("adb", ["shell", "input", "tap", ...at.map((v) => String(Math.round(v * k)))]);
  } else {
    await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: at[0], y: at[1], id: 2 }] });
    await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  }
  await sleep(300);
  if (!(await run("window.motifClicked"))) failures.push(`${name}: a tap right after it ended was lost`);
}

/** Runs `act`, waits for the change to settle, then checks its landings (at least `least` of them); `dragged`: a finger drives it. */
async function check(name, act, least = 1, dragged = false) {
  await run("window.motifLandings.length = 0");
  await run(watch);
  await act();
  // The change begins a frame or two later (the new screen is laid out first).
  await sleep(150);
  await tapsRightAfter(name);
  try {
    await until(settled);
  } catch {
    failures.push(`${name}: didn't settle (screens or copies left over)`);
    return;
  }
  const landings = await run("JSON.stringify(window.motifLandings)").then(JSON.parse);
  const hidden = await run("[...document.querySelectorAll('main > .screen:not([inert]) [style*=\"visibility: hidden\"], .sheet [style*=\"visibility: hidden\"], .bar [style*=\"visibility: hidden\"]')].map((e) => e.dataset.move ?? e.className)");
  // A quarter pixel: words a fraction of a pixel off still hop visibly at the hand-over.
  const bad = landings.filter((l) => l.box > 0.25 || l.look.length);
  for (const l of bad) failures.push(`${name}: ${l.name} landed ${l.box.toFixed(1)}px off${l.look.length ? `, ${l.look.join(", ")} differ` : ""}`);
  if (landings.length < least) failures.push(`${name}: ${landings.length} shared elements flew, expected at least ${least}`);
  if (hidden.length) failures.push(`${name}: left hidden: ${hidden.join(", ")}`);
  const watched = await run("window.motifWatch");
  const late = [...new Set(watched.late)];
  if (late.length) failures.push(`${name}: moved late (a span on a move): ${late.join(", ")}`);
  if (dragged) {
    const p = watched.p;
    if (!p.length) failures.push(`${name}: nothing followed the finger`);
    else if (Math.min(p[0][1], 1 - p[0][1]) > STEP) failures.push(`${name}: its first frame jumped to ${p[0][1].toFixed(2)}`);
    for (let i = 1; i < p.length; i++)
      if (Math.abs(p[i][1] - p[i - 1][1]) > STEP * Math.max(1, (p[i][0] - p[i - 1][0]) / 16.7)) {
        failures.push(`${name}: jumped from ${p[i - 1][1].toFixed(2)} to ${p[i][1].toFixed(2)} in ${Math.round(p[i][0] - p[i - 1][0])} ms under the finger`);
        break;
      }
  }
  const mine = failures.filter((f) => f.startsWith(`${name}:`));
  console.log(`${mine.length ? "FAIL" : "ok  "} ${name} (${landings.length} landed)${mine.map((f) => `\n       ${f.slice(name.length + 2)}`).join("")}`);
}

const card = "document.querySelector('main > .screen:not([inert]) .card:not(.loading)')";
/** A tap on the bar's track, which opens the sheet (pointer events, as the bar's own drag reads them). */
const tapBar = `(() => { const el = document.querySelector('.bar .identity'); const r = el.getBoundingClientRect(); const o = { bubbles: true, button: 0, clientX: r.x + 20, clientY: r.y + 10, pointerType: 'touch', isPrimary: true, pointerId: 7 }; el.dispatchEvent(new PointerEvent('pointerdown', o)); window.dispatchEvent(new PointerEvent('pointerup', o)); })()`;
const step = (name) => `document.querySelector('.sheet').dataset.step === '${name}'`;

/**
 * The now-playing sheet (phones): open and close, by taps and by real
 * drags, and every step change, by its handle's tap and by dragging it.
 * Each shared part (cover, title, artist, album, play, seek bar) must land
 * on its real element, and closing must put each back on the bar's.
 */
async function sheet(label) {
  await open("dev-view=albums", 384, 820);
  await until("!!document.querySelector('.bar .identity img')?.complete");
  const shared = 6;
  await check(`${label}: open the sheet`, () => run(tapBar), shared);
  await check(`${label}: now playing to the half queue`, tap("document.querySelector('.more .step')"), 1);
  await check(`${label}: half to the full queue`, tap("document.querySelector('.queue-slot > .step')"), 4);
  await check(`${label}: full back to half`, tap("document.querySelector('.mini .step')"), 4);
  await check(`${label}: half back to now playing`, () => dragBy(".queue-slot > .step", 200), 1, true);
  if (!(await run(step("now")))) failures.push(`${label}: dragging the half queue's handle down didn't go back to now playing`);
  await check(`${label}: drag up to the half queue`, () => dragBy(".more .step", -200), 1, true);
  if (!(await run(step("half")))) failures.push(`${label}: dragging the handle up didn't show the half queue`);
  await check(`${label}: half back to now playing (again)`, () => dragBy(".queue-slot > .step", 200), 1, true);
  await check(`${label}: lyrics`, tap("document.querySelector('.lyrics-button')"), 4);
  await check(`${label}: lyrics back to now playing`, tap("document.querySelector('.mini .step')"), 4);
  await check(`${label}: close by dragging down`, () => dragBy(".details .hero-title", 400), shared, true);
  if (await run("document.querySelector('.sheet').classList.contains('open')")) failures.push(`${label}: dragging now playing down didn't close it`);
  await check(`${label}: open by dragging the bar up`, () => dragBy(".bar .identity", -500), shared, true);
  if (!(await run("document.querySelector('.sheet').classList.contains('open')"))) failures.push(`${label}: dragging the bar up didn't open the sheet`);
  // Each step change, held at its start: the half and full queue, back, lyrics.
  await startsInPlace(`${label}: now playing to the half queue starts in place`, tap("document.querySelector('.more .step')"));
  await startsInPlace(`${label}: half to the full queue starts in place`, tap("document.querySelector('.queue-slot > .step')"));
  await startsInPlace(`${label}: full back to half starts in place`, tap("document.querySelector('.mini .step')"));
  await dragBy(".queue-slot > .step", 200);
  await until(settled);
  await startsInPlace(`${label}: lyrics start in place`, tap("document.querySelector('.lyrics-button')"));
  await startsInPlace(`${label}: lyrics back to now playing starts in place`, tap("document.querySelector('.mini .step')"));
  // Each change held at its first, middle and last frames.
  await heldFrames(`${label}: now playing to the half queue, held`, tap("document.querySelector('.more .step')"));
  await heldFrames(`${label}: half to the full queue, held`, tap("document.querySelector('.queue-slot > .step')"));
  await heldFrames(`${label}: full back to half, held`, tap("document.querySelector('.mini .step')"));
  // The full queue scrolled: its rows stay drawn as it shrinks to the half queue (a scroller cut to its new size at once lost them).
  await scrolledBack(`${label}: full back to half with the queue scrolled, held`, tap("document.querySelector('.mini .step')"));
  await scrolledBack(`${label}: full back to half with the queue scrolled, dragged, held`, () => dragBy(".mini .step", 200));
  // A drag, let go: its clock is held where the finger left it, and set from there like the others'.
  await heldFrames(`${label}: half back to now playing, held`, () => dragBy(".queue-slot > .step", 200));
  await heldFrames(`${label}: lyrics, held`, tap("document.querySelector('.lyrics-button')"));
  await heldFrames(`${label}: lyrics back to now playing, held`, tap("document.querySelector('.mini .step')"));
  await heldFrames(`${label}: close, held`, () => run("window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))"));
  await heldFrames(`${label}: open, held`, () => run(tapBar));
  await heldFrames(`${label}: close with the title scrolled, held`, () => run("window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))"), true);
  await run(tapBar);
  await check(`${label}: close from the full queue`, async () => {
    await run("document.querySelector('.more .step').click()");
    await until(settled);
    await run("document.querySelector('.queue-slot > .step').click()");
    await until(settled);
    await run("window.motifLandings.length = 0");
    // Escape closes it from any step.
    await run("window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))");
  }, 4);
}
const tap = (selector) => () => run(`${selector}.click()`);

/** From the half queue: up to the full queue, scrolled to its end, then `act` back to half, held (`heldFrames`). */
async function scrolledBack(name, act) {
  await run("document.querySelector('.queue-slot > .step').click()");
  await until(settled);
  if (!(await run("(() => { const l = document.querySelector('.sheet .queue .list'); l.scrollTop = l.scrollHeight; return l.scrollTop > 20; })()")))
    return console.log(`skip ${name} (the queue is too short to scroll)`);
  await heldFrames(name, act);
}

/** The phone's system bars (device pixel rows), which change on their own (the clock): left out of comparisons. */
let bars;
function systemBars() {
  if (bars) return bars;
  const window = execFileSync("adb", ["shell", "dumpsys", "window"], { encoding: "utf8", maxBuffer: 64 << 20 });
  const frame = (type) => window.match(new RegExp(`type=${type} frame=\\[\\d+,(\\d+)\\]\\[\\d+,(\\d+)\\]`))?.slice(1).map(Number);
  bars = { top: frame("statusBars")?.[1] ?? 0, bottom: frame("navigationBars")?.[0] ?? Infinity };
  return bars;
}

/**
 * A screenshot as RGBA pixels (a PNG: 8 bits, not interlaced). On the phone, the
 * display itself (`adb screencap`) without its system bars: the WebView's DevTools
 * capture draws animated layers wrongly while the animations' clock is stopped (a
 * frozen first frame that the display shows exactly as before captured 32,000
 * pixels off), so it can't be trusted for held frames.
 */
async function pixels() {
  const png = phone ? execFileSync("adb", ["exec-out", "screencap", "-p"], { maxBuffer: 64 << 20 }) : Buffer.from((await send("Page.captureScreenshot", { format: "png" })).data, "base64");
  let at = 8;
  let width = 0;
  let height = 0;
  let type = 0;
  const idat = [];
  while (at < png.length) {
    const len = png.readUInt32BE(at);
    const kind = png.toString("ascii", at + 4, at + 8);
    const body = png.subarray(at + 8, at + 8 + len);
    if (kind === "IHDR") [width, height, type] = [body.readUInt32BE(0), body.readUInt32BE(4), body[9]];
    if (kind === "IDAT") idat.push(body);
    at += 12 + len;
  }
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const bpp = type === 6 ? 4 : 3;
  const stride = width * bpp;
  const out = Buffer.alloc(width * height * 4);
  let prev = Buffer.alloc(stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const row = Buffer.from(raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)));
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? row[i - bpp] : 0;
      const b = prev[i];
      const c = i >= bpp ? prev[i - bpp] : 0;
      const pa = Math.abs(b - c);
      const pb = Math.abs(a - c);
      const pc = Math.abs(a + b - 2 * c);
      const add = [0, a, b, (a + b) >> 1, pa <= pb && pa <= pc ? a : pb <= pc ? b : c][filter];
      row[i] = (row[i] + add) & 255;
    }
    const hidden = phone && (y < systemBars().top || y >= systemBars().bottom);
    if (!hidden) for (let x = 0; x < width; x++) for (let k = 0; k < 3; k++) out[(y * width + x) * 4 + k] = row[x * bpp + k];
    prev = row;
  }
  return { width, height, out, png };
}

/**
 * How many pixels of two screenshots differ clearly: none of the other's
 * within `r` pixels (a CSS pixel) is close, so words drawn a pixel off (a copy
 * and its original rasterise their letters a little differently) don't count.
 */
function differ(a, b, r = 1) {
  const { width: w, height: h } = a;
  // Clearly: a drawing popping in or out on the dark sheet differs by far more; the same words rasterised a little differently by a scaled copy, less.
  const near = (i, j) => Math.abs(a.out[i] - b.out[j]) + Math.abs(a.out[i + 1] - b.out[j + 1]) + Math.abs(a.out[i + 2] - b.out[j + 2]) <= 300;
  let n = 0;
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      if (near(i, i)) continue;
      let found = false;
      for (let dy = -r; dy <= r && !found; dy++)
        for (let dx = -r; dx <= r && !found; dx++) {
          const [X, Y] = [x + dx, y + dy];
          if (X >= 0 && Y >= 0 && X < w && Y < h) found = near(i, (Y * w + X) * 4);
        }
      if (!found) n++;
    }
  return n;
}
/** More clearly different CSS pixels than a few glyphs' edges: something popped, vanished or changed places in the stack. */
const POPPED = 400;

/** What is drawn, by what it shows (words, an image, an icon), with where and how opaque: originals and copies alike. */
const drawings = `(() => {
  const out = [];
  // The open sheet covers the bar.
  const covered = document.querySelector('.sheet.open');
  for (const el of document.querySelectorAll(covered ? '.sheet *, .motion-layer *' : '.sheet *, .bar *, .motion-layer *')) {
    const img = el.tagName === 'IMG';
    const words = !el.children.length && el.textContent.trim();
    if (!img && !words) continue;
    const r = el.getBoundingClientRect();
    if (!r.width || !r.height || r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth) continue;
    let o = 1;
    for (let e = el; e && o > 0.02; e = e.parentElement) {
      const s = getComputedStyle(e);
      if (s.visibility === 'hidden' || s.display === 'none') o = 0;
      o *= Number(s.opacity);
    }
    if (o > 0.05) out.push(img ? 'image ' + el.src.split('/').pop().slice(0, 40) : 'words ' + words.slice(0, 30));
  }
  return [...new Set(out)];
})()`;

/**
 * A sheet change held at its first, middle and last frames (its animations'
 * clock stopped and set): the first frame must look exactly like before the
 * change and the last exactly like after it (nothing pops in, vanishes or
 * swaps places in the stack as it starts or hands over), what is drawn both
 * before and after must be drawn in the middle too, and nothing may change
 * once it has settled.
 */
async function heldFrames(name, act, scrolled = false) {
  const r = Math.round(await run("devicePixelRatio"));
  await until(settled);
  // A long title's marquee scrolls on its own clock, not a change's: wait until it has stopped, and stays
  // stopped past the delay before it starts (`WAIT` in marquee.ts), so it doesn't start inside the case.
  const still = "!document.getAnimations().some((a) => a.playState === 'running' && a.effect?.target?.classList?.contains('marquee')) && ![...document.querySelectorAll('.marquee')].some((m) => m.style.transform)";
  await until(still, 45000).catch(() => {});
  await sleep(2500);
  await until(still, 45000).catch(() => {});
  if (scrolled) {
    // Or a change that starts from a scrolled title: tapped, it scrolls at once; caught paused at its end.
    await run("document.querySelector('.sheet .hero h1').click()");
    await until("!!document.querySelector('.sheet .hero h1 .marquee').style.transform && !document.getAnimations().some((a) => a.playState === 'running' && a.effect?.target?.classList?.contains('marquee'))", 45000);
  }
  await sleep(200);
  const before = await pixels();
  const drawnBefore = await run(drawings);
  await send("Animation.enable");
  await send("Animation.setPlaybackRate", { playbackRate: 0 });
  const at = (ms) => run(`(() => { for (const a of document.getAnimations()) if (a.effect?.getTiming().fill === 'both') { a.pause(); a.currentTime = ${ms}; } })()`);
  const bad = [];
  let middle = [];
  let last;
  let firstShot;
  try {
    await act();
    await sleep(400);
    await at(0);
    await sleep(100);
    firstShot = await pixels();
    const first = differ(before, firstShot, r);
    if (first > POPPED * r * r) bad.push(`its first frame differs from before it (${first} px)`);
    await at(40);
    await sleep(100);
    middle = await run(drawings);
    await at(259.9);
    await sleep(100);
    last = await pixels();
  } finally {
    await send("Animation.setPlaybackRate", { playbackRate: 1 });
    await run("(() => { for (const a of document.getAnimations()) if (a.effect?.getTiming().fill === 'both') a.play(); })()");
  }
  await sleep(300);
  await until(settled);
  await sleep(200);
  const after = await pixels();
  const drawnAfter = await run(drawings);
  const end = differ(last, after, r);
  if (end > POPPED * r * r) bad.push(`its last frame differs from after it (${end} px)`);
  const lost = drawnBefore.filter((d) => drawnAfter.includes(d) && !middle.includes(d));
  if (lost.length) bad.push(`not drawn in the middle: ${lost.slice(0, 4).join(", ")}`);
  await sleep(500);
  const late = differ(after, await pixels(), r);
  if (late > POPPED * r * r) bad.push(`changed after it settled (${late} px)`);
  for (const b of bad) failures.push(`${name}: ${b}`);
  // Debugging: DUMP=dir saves a failing case's frames (before, first, last, after).
  if (bad.length && process.env.DUMP)
    for (const [k, f] of Object.entries({ before, first: firstShot, last, after })) fs.writeFileSync(path.join(process.env.DUMP, `${name.replace(/\W+/g, "-")}-${k}.png`), f.png);
  console.log(`${bad.length ? "FAIL" : "ok  "} ${name}${bad.map((b) => `\n       ${b}`).join("")}`);
}

/** The most a row may move in a frame's time (16.7 ms) when nothing jumps: a 260 ms glide of a row's height moves under 20 px in its fastest frame. */
const ROW_STEP = 30;

/** Runs `act` (a finger on the queue) and fails if any of the queue's rows jumps between frames instead of gliding. */
async function rowsGlide(name, act) {
  await run(`{
    const rows = [...document.querySelectorAll('.sheet .queue-slot li')];
    const rec = (window.motifRows = { jumps: [] });
    const last = new Map();
    const t0 = performance.now();
    let prev = t0;
    const tick = (t) => {
      for (const r of rows) {
        // Hidden while a copy stands in for it (the copy's landing is checked like any other).
        if (!r.isConnected || !r.getClientRects().length || getComputedStyle(r).visibility === 'hidden') {
          last.delete(r);
          continue;
        }
        const top = r.getBoundingClientRect().top;
        const was = last.get(r);
        if (was !== undefined && Math.abs(top - was) > ${ROW_STEP} * Math.max(1, (t - prev) / 16.7)) rec.jumps.push(r.textContent.trim().slice(0, 16) + ' ' + Math.round(top - was) + 'px');
        last.set(r, top);
      }
      prev = t;
      if (t - t0 < 2000) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  } 0`);
  await act();
  await sleep(2100);
  await until(settled);
  const jumps = await run("window.motifRows.jumps");
  if (jumps.length) failures.push(`${name}: rows jumped: ${jumps.slice(0, 4).join(", ")}`);
  console.log(`${jumps.length ? "FAIL" : "ok  "} ${name}`);
}

/**
 * Nothing pops, in the motion system itself: on a test stage built in the page with
 * its own `change()`, an element that no part declares fades out when a change takes
 * it away (its drawing still there, opaque, at the start; gone by the end) and fades
 * in when a change brings it.
 */
async function undeclared() {
  const result = await run(`(async () => {
    const { change } = await import('/src/lib/motion/change.ts');
    const root = document.createElement('div');
    root.style.cssText = 'position: fixed; left: 0; top: 0; width: 300px; height: 200px; background: #000; z-index: 99';
    root.innerHTML = '<div data-enter="fade" style="height: 100px"><span class="gone" style="color: #fff">Going</span> <span>Staying</span></div>';
    document.body.append(root);
    const part = root.firstElementChild;
    const gone = part.querySelector('.gone');
    const box = gone.getBoundingClientRect();
    /** What draws the words "Going" where they were, and how opaque it is at progress p. */
    const drawn = (t, p) => {
      t.seek(p);
      for (const el of document.querySelectorAll('body *')) {
        if (el.children.length || el.textContent !== 'Going' || el === gone) continue;
        const r = el.getBoundingClientRect();
        if (Math.abs(r.left - box.left) > 1 || Math.abs(r.top - box.top) > 1) continue;
        let o = 1;
        for (let e = el; e; e = e.parentElement) o *= Number(getComputedStyle(e).opacity);
        return o;
      }
      return 0;
    };
    const out = {};
    const t = change(root, () => gone.remove());
    out.goneStart = drawn(t, 0);
    out.goneEnd = drawn(t, 1);
    t.end();
    const come = document.createElement('span');
    come.textContent = 'Coming';
    const u = change(root, () => part.append(come));
    u.seek(0);
    out.comeStart = Number(getComputedStyle(come).opacity);
    u.seek(1);
    out.comeEnd = Number(getComputedStyle(come).opacity);
    u.end();
    root.remove();
    return JSON.stringify(out);
  })()`).then(JSON.parse);
  const bad = [];
  if (result.goneStart < 0.95) bad.push(`an undeclared element taken away vanished at once (drawn at ${result.goneStart.toFixed(2)} at the start)`);
  if (result.goneEnd > 0.05) bad.push(`an undeclared element taken away was still drawn at the end (${result.goneEnd.toFixed(2)})`);
  if (result.comeStart > 0.05 || result.comeEnd < 0.95) bad.push(`an undeclared element brought in popped (${result.comeStart.toFixed(2)} at the start, ${result.comeEnd.toFixed(2)} at the end)`);
  for (const b of bad) failures.push(`motion: ${b}`);
  console.log(`${bad.length ? "FAIL" : "ok  "} motion: undeclared elements fade${bad.map((b) => `\n       ${b}`).join("")}`);
}

/**
 * A finger is followed one to one at any zoom (the Scale setting; Android's font
 * size): fingers move in screen pixels, so the distance a drive spreads its progress
 * over is in screen pixels too (a change's `distance`, a swipe's).
 */
async function fingerScale(label) {
  const r = await run(`(async () => {
    const { change } = await import('/src/lib/motion/change.ts');
    const { slide } = await import('/src/lib/gestures.ts');
    const root = document.createElement('div');
    root.style.cssText = 'position: fixed; left: 0; top: 0; width: 200px; height: 300px; z-index: 99';
    root.innerHTML = '<div data-move="x" style="height: 20px; width: 50px"></div>';
    document.body.append(root);
    const part = root.firstElementChild;
    const z = part.getBoundingClientRect().width / 50;
    const t = change(root, () => (part.style.marginTop = '150px'));
    const moved = part.getBoundingClientRect().top;
    const out = { z, change: t.distance, moved: (moved + (t.end(), 0)) };
    const swipe = slide(part)(-1);
    out.swipe = swipe.distance;
    out.width = part.getBoundingClientRect().width;
    swipe.drive.release(false);
    root.remove();
    return JSON.stringify(out);
  })()`).then(JSON.parse);
  const bad = [];
  // The part moves 150 CSS pixels: that many times the zoom on screen.
  if (Math.abs(r.change - 150 * r.z) > 1) bad.push(`a change spreads its progress over ${r.change.toFixed(1)} px for a move of ${(150 * r.z).toFixed(1)} on screen`);
  if (Math.abs(r.swipe - r.width) > 1) bad.push(`a swipe spreads its progress over ${r.swipe.toFixed(1)} px for an element ${r.width.toFixed(1)} wide on screen`);
  for (const b of bad) failures.push(`${label}: ${b}`);
  console.log(`${bad.length ? "FAIL" : "ok  "} ${label}: a finger is followed one to one${bad.map((b) => `\n       ${b}`).join("")}`);
}

/** The queue in the phone sheet (it needs one): reordering by the handle, a reorder let go where it started, removing by a swipe. */
async function queue(label) {
  await open("dev-view=albums", 384, 820);
  await until(`!!${card}`);
  await run(`${card}.click()`);
  await sleep(300);
  await until(settled);
  // Three songs queued by swiping them left.
  for (let i = 0; i < 3; i++) {
    const [x, y] = await run(`(() => { const r = [...document.querySelectorAll('main > .screen:not([inert]) .song')][${i}].getBoundingClientRect(); return [r.x + r.width * 0.8, r.y + r.height / 2]; })()`);
    await drag(x, y, x - 250, y);
    await sleep(700);
  }
  await open("dev-view=albums&dev-sheet=full", 384, 820);
  await until("document.querySelectorAll('.sheet .queue-slot ol li .handle').length >= 3");
  // The sheet comes up first.
  await sleep(300);
  await until(settled);
  const handle = `(() => { const r = document.querySelector('.sheet .queue-slot ol li .handle').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`;
  const pitch = await run("(() => { const [a, b] = document.querySelectorAll('.sheet .queue-slot ol li'); return b.getBoundingClientRect().top - a.getBoundingClientRect().top; })()");
  await rowsGlide(`${label}: reorder the queue`, async () => {
    const [x, y] = await run(handle);
    await drag(x, y, x, y + pitch * 1.3);
  });
  await rowsGlide(`${label}: a reorder let go where it started`, async () => {
    const [x, y] = await run(handle);
    await drag(x, y, x, y + pitch * 0.4);
  });
  // The dragged row stays under the finger while the rows it passes make way.
  {
    const [x, y] = await run(handle);
    const top = await run("document.querySelector('.sheet .queue-slot ol li').getBoundingClientRect().top");
    await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y, id: 1 }] });
    let off = 0;
    for (let i = 1; i <= 30; i++) {
      await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y + i * 5, id: 1 }] });
      await sleep(20);
      const now = await run("document.querySelector('.sheet .queue-slot ol li.dragging')?.getBoundingClientRect().top ?? null");
      if (now !== null) off = Math.max(off, Math.abs(now - top - i * 5));
    }
    await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    if (off > 1) failures.push(`${label}: a dragged queue row fell ${off.toFixed(0)}px behind the finger`);
    await sleep(600);
    await until(settled);
  }
  // A swiped-out row's strip closes up as the rows below move into its place, never covering them.
  await run(`window.motifOverlap = 0; { const first = document.querySelector('.sheet .queue-slot ol li'); const t0 = performance.now(); const tick = () => {
    const strip = document.querySelector('.swipe-strip');
    if (strip && !first.isConnected) motifOverlap = Math.max(motifOverlap, strip.getBoundingClientRect().bottom - document.querySelector('.sheet .queue-slot ol li').getBoundingClientRect().top);
    if (performance.now() - t0 < 2000) requestAnimationFrame(tick);
  }; requestAnimationFrame(tick); } 0`);
  await rowsGlide(`${label}: remove from the queue by a swipe`, async () => {
    const [x, y] = await run("(() => { const r = document.querySelector('.sheet .queue-slot ol li .entry').getBoundingClientRect(); return [r.x + r.width * 0.6, r.y + r.height / 2]; })()");
    await drag(x, y, Math.max(4, x - 300), y);
  });
  await sleep(2000);
  const over = await run("motifOverlap");
  if (over > 1) failures.push(`${label}: a removed row's strip covered the next row by ${over.toFixed(0)}px`);
  // A short, quick swipe: the row can be removed while it is still sliding out.
  await run(`window.motifOverlap = 0; { const first = document.querySelector('.sheet .queue-slot ol li'); const t0 = performance.now(); const tick = () => {
    const strip = document.querySelector('.swipe-strip');
    const next = document.querySelector('.sheet .queue-slot ol li');
    if (strip && next && !first.isConnected) motifOverlap = Math.max(motifOverlap, strip.getBoundingClientRect().bottom - next.getBoundingClientRect().top);
    if (performance.now() - t0 < 2000) requestAnimationFrame(tick);
  }; requestAnimationFrame(tick); } 0`);
  {
    const [x, y] = await run("(() => { const r = document.querySelector('.sheet .queue-slot ol li .entry').getBoundingClientRect(); return [r.x + r.width * 0.6, r.y + r.height / 2]; })()");
    if (phone) await drag(x, y, x - 90, y);
    else {
      const at = (i) => [{ x: x - (90 * i) / 4, y, id: 1 }];
      await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: at(0) });
      for (let i = 1; i <= 4; i++) {
        await sleep(16);
        await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: at(i) });
      }
      await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    }
  }
  await sleep(2000);
  const short = await run("motifOverlap");
  if (short > 1) failures.push(`${label}: a row removed by a short swipe left its strip over the next row by ${short.toFixed(0)}px`);
  await run("document.querySelector('.sheet .queue-slot .heading .link')?.click()");
}

/** What a stage draws (its innermost elements and its buttons), with where each is: set aside before a change. */
const drawnIn = (root) => `(() => {
  const out = [];
  for (const el of document.querySelectorAll('${root} *')) {
    if (el.closest('.leaving') || (el.children.length && el.tagName !== 'BUTTON') || !el.getClientRects().length || getComputedStyle(el).visibility === 'hidden') continue;
    const r = el.getBoundingClientRect();
    if (r.bottom < 0 || r.top > innerHeight || !r.width || !r.height) continue;
    out.push([el, r]);
  }
  return out;
})()`;

/**
 * Every change starts exactly where things were: with the clock held at 0,
 * whatever is still drawn after the change has begun (not hidden for a copy
 * standing in for it) is where it was before it. Elements the layout moved
 * but nothing animated jump here.
 */
async function startsInPlace(name, act) {
  await run(`window.motifBefore = ${drawnIn(".sheet")}; 0`);
  await send("Animation.enable");
  await send("Animation.setPlaybackRate", { playbackRate: 0 });
  try {
    await act();
    await sleep(300);
    const off = await run(`window.motifBefore.filter(([el, r]) => {
      if (!el.isConnected || !el.getClientRects().length) return false;
      for (let e = el; e; e = e.parentElement) if (getComputedStyle(e).visibility === 'hidden' || getComputedStyle(e).opacity === '0') return false;
      const n = el.getBoundingClientRect();
      return Math.max(Math.abs(n.left - r.left), Math.abs(n.top - r.top)) > 1;
    }).map(([el, r]) => (el.dataset.move ?? el.getAttribute('aria-label') ?? el.textContent.trim().slice(0, 20) ?? el.className) + ' ' + Math.round(el.getBoundingClientRect().top - r.top) + 'px')`);
    if (off.length) failures.push(`${name}: didn't start where it was: ${off.slice(0, 6).join(", ")}`);
  } finally {
    await send("Animation.setPlaybackRate", { playbackRate: 1 });
  }
  await until(settled);
  console.log(`${failures.some((f) => f.startsWith(`${name}:`)) ? "FAIL" : "ok  "} ${name}`);
}

try {
  // Debugging: STEPS=steps.mjs runs that module's default export with these helpers instead.
  if (process.env.STEPS) await (await import(path.resolve(process.env.STEPS))).default({ send, run, until, open, swipe, drag, dragBy, sleep, settled, card, undeclared, heldFrames, tapBar, fingerScale });
  else {
    // At the Scale setting's default and above it (the root's zoom; on Android it includes the system font size),
    // and in a font whose glyphs sit differently in their line than the default's (Roboto's on phones do).
    for ([scale, font] of [
      [100, ""],
      [125, ""],
      [100, "DejaVu Sans"],
    ]) {
      const phone_ = `phone${scale === 100 ? "" : ` ${scale}%`}${font ? ` ${font}` : ""}`;
      // Albums scrolled to the end: the rows at the top of the view are drawn (at any zoom).
      await open("dev-view=albums", 384, 820);
      await until(`!!${card}`);
      const gap = await run(`(async () => {
        const s = document.querySelector('main > .screen:not([inert]) .album-grid .scroll');
        s.scrollTop = s.scrollHeight;
        await new Promise((r) => setTimeout(r, 300));
        const first = s.querySelector('li')?.getBoundingClientRect();
        return first ? first.top - s.getBoundingClientRect().top : null;
      })()`);
      if (gap === null || gap > 1) failures.push(`${phone_}: albums scrolled to the end left ${gap?.toFixed(0) ?? "all"}px empty at the top`);
      // Phone: a category from the Library home and back by its back button; desktop has no home screen.
      await open("dev-view=home", 384, 820);
      await check(`${phone_}: open a category`, tap("document.querySelectorAll('main > .screen:not([inert]) .categories button')[1]"), 2);
      await check(`${phone_}: back by the button`, tap("document.querySelector('main > .screen:not([inert]) .back')"), 2);
      // Search: an album found, and back to the results.
      await open("dev-view=search", 384, 820);
      await run("const i = document.querySelector('main > .screen:not([inert]) input[type=search]'); i.value = 'Short'; i.dispatchEvent(new Event('input', { bubbles: true }))");
      await until("!!document.querySelector('main > .screen:not([inert]) button.album img')?.complete");
      // Cover, title and artist.
      await check(`${phone_}: open an album from Search`, tap("document.querySelector('main > .screen:not([inert]) button.album')"), 3);
      await check(`${phone_}: swipe back to Search`, () => swipe(20, 300, 410), 2, true);
      for (const [label, w, h] of [
        [phone_, 384, 820],
        ["desktop", 1200, 800],
      ]) {
        if (label === "desktop" && (scale !== 100 || font)) continue;
        if (!(await open("dev-view=albums", w, h))) continue;
        await until(`!!${card}?.querySelector('img')?.complete`);
        await check(`${label}: open an album`, tap(card), 3);
        if (label === "desktop") {
          await check(`${label}: back by the button`, tap("document.querySelector('main > .screen:not([inert]) .back')"), 2);
          await check(`${label}: open it again`, tap(card), 3);
        }
        await check(`${label}: swipe back`, () => swipe(20, w * 0.8, h / 2), 2, true);
        await check(`${label}: swipe back cancelled`, async () => {
          await run(`${card}.click()`);
          await until(settled);
          await run("window.motifLandings.length = 0");
          await swipe(20, 40, h / 2);
        }, 0);
      }
      // An album page always opens at the top, so what flies to it lands where it rests: scrolled down, back, and open again.
      await open("dev-view=albums", 384, 560);
      await until(`!!${card}?.querySelector('img')?.complete`);
      await check(`${phone_}: open an album to scroll`, tap(card), 3);
      const scroller = "[...document.querySelectorAll('main > .screen:not([inert]) *')].find((e) => e.scrollHeight > e.clientHeight + 20 && /auto|scroll/.test(getComputedStyle(e).overflowY))";
      if (!(await run(`(() => { const s = ${scroller}; if (!s) return false; s.scrollTop = s.scrollHeight; return s.scrollTop > 20; })()`))) failures.push(`${phone_}: the album page didn't scroll`);
      await check(`${phone_}: swipe back from the scrolled album`, () => swipe(20, 300, 280), 2, true);
      await check(`${phone_}: open the album again`, tap(card), 3);
      if (await run(`(${scroller})?.scrollTop`)) failures.push(`${phone_}: the album page opened scrolled`);
      // The grid scrolled a little (on a fixed head, its first row would sit partly under it): the cover flies back
      // to a card that shows whole, since the head scrolls with the grid; nothing covers the card's corners.
      await open("dev-view=albums", 384, 820);
      await until(`!!${card}?.querySelector('img')?.complete`);
      await run("document.querySelector('main > .screen:not([inert]) .album-grid .scroll').scrollTop = 60");
      await sleep(300);
      await check(`${phone_}: open an album from a scrolled grid`, tap(card), 3);
      await check(`${phone_}: swipe back to the scrolled grid`, () => swipe(20, 300, 600), 2, true);
      const covered = await run(`(() => { const c = ${card}; const r = c.getBoundingClientRect(); return [[r.left + 10, r.top + 10], [r.right - 10, r.top + 10], [r.left + 10, r.bottom - 10], [r.right - 10, r.bottom - 10]].filter(([x, y]) => !c.contains(document.elementFromPoint(x, y))).length; })()`);
      if (covered) failures.push(`${phone_}: back on the scrolled grid, ${covered} corners of the card the cover flew to are hidden`);
      await fingerScale(phone_);
      await sheet(phone_);
      if (scale === 100 && !font) await queue(phone_);
    }
    if (!phone) {
      [scale, font] = [100, ""];
      await open("dev-view=albums", 1200, 800);
      await until("!!document.querySelector('.bar .identity img')?.complete");
      await check("desktop: open the sheet", tap("document.querySelector('.bar [aria-label=\"Expand player\"]')"), 6);
      await check("desktop: close the sheet", tap("document.querySelector('.sheet [aria-label=Collapse]')"), 6);
    }
  }
  if (!process.env.STEPS) {
    [scale, font] = [100, ""];
    await open("dev-view=albums", 384, 820);
    await undeclared();
    await open("dev-settings=interface", 384, 820);
    await check("phone: Settings section back to the list", () => swipe(20, 300, 400), 0);
    await check("phone: Settings list to a section", tap("document.querySelector('main > .screen:not([inert]) .phone-list button')"), 0);
  }
} finally {
  if (phone) {
    await forget();
    await send("Page.navigate", { url: home });
  }
  ws.close();
  closeBrowser();
}
if (failures.length) {
  console.log(`\n${failures.length} failed:\n${failures.join("\n")}`);
  process.exit(1);
}
console.log("\nall landed");
