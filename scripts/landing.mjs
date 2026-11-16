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

const base = process.argv[2] ?? "http://localhost:5173";
const chrome = process.env.CHROME ?? "chromium";
const phone = process.env.CDP;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let browser;
let profile;
let target;
if (phone) {
  target = (await (await fetch(`${phone}/json/list`)).json()).find((t) => t.type === "page");
  if (!target) throw new Error(`no page at ${phone}`);
} else {
  profile = fs.mkdtempSync(path.join(os.tmpdir(), "motif-landing-"));
  browser = spawn(chrome, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--disable-features=OverscrollHistoryNavigation", "about:blank"], { stdio: "ignore" });
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
    && ![...document.querySelectorAll('.sheet, .bar')].some((e) => e.getAnimations({ subtree: true }).some((a) => a.effect?.getTiming().fill === 'both'));
})()`;
const failures = [];

/** Runs `act`, waits for the change to settle, then checks its landings (at least `least` of them). */
async function check(name, act, least = 1) {
  await run("window.motifLandings.length = 0");
  await act();
  // The change begins a frame or two later (the new screen is laid out first).
  await sleep(150);
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
  await check(`${label}: half back to now playing`, () => dragBy(".queue-slot > .step", 200), 1);
  if (!(await run(step("now")))) failures.push(`${label}: dragging the half queue's handle down didn't go back to now playing`);
  await check(`${label}: drag up to the half queue`, () => dragBy(".more .step", -200), 1);
  if (!(await run(step("half")))) failures.push(`${label}: dragging the handle up didn't show the half queue`);
  await check(`${label}: half back to now playing (again)`, () => dragBy(".queue-slot > .step", 200), 1);
  await check(`${label}: lyrics`, tap("document.querySelector('.lyrics-button')"), 4);
  await check(`${label}: lyrics back to now playing`, tap("document.querySelector('.mini .step')"), 4);
  await check(`${label}: close by dragging down`, () => dragBy(".details .hero-title", 400), shared);
  if (await run("document.querySelector('.sheet').classList.contains('open')")) failures.push(`${label}: dragging now playing down didn't close it`);
  await check(`${label}: open by dragging the bar up`, () => dragBy(".bar .identity", -500), shared);
  if (!(await run("document.querySelector('.sheet').classList.contains('open')"))) failures.push(`${label}: dragging the bar up didn't open the sheet`);
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

try {
  // Debugging: STEPS=steps.mjs runs that module's default export with these helpers instead.
  if (process.env.STEPS) await (await import(path.resolve(process.env.STEPS))).default({ send, run, until, open, swipe, drag, dragBy, sleep, settled, card });
  else {
    // At the Scale setting's default and above it (the root's zoom; on Android it includes the system font size),
    // and in a font whose glyphs sit differently in their line than the default's (Roboto's on phones do).
    for ([scale, font] of [
      [100, ""],
      [125, ""],
      [100, "DejaVu Sans"],
    ]) {
      const phone_ = `phone${scale === 100 ? "" : ` ${scale}%`}${font ? ` ${font}` : ""}`;
      // Phone: a category from the Library home and back by its back button; desktop has no home screen.
      await open("dev-view=home", 384, 820);
      await check(`${phone_}: open a category`, tap("document.querySelectorAll('main > .screen:not([inert]) .categories button')[1]"), 2);
      await check(`${phone_}: back by the button`, tap("document.querySelector('main > .screen:not([inert]) .back')"), 2);
      // Search: an album found, and back to the results.
      await open("dev-view=search", 384, 820);
      await run("const i = document.querySelector('main > .screen:not([inert]) input[type=search]'); i.value = 'Short'; i.dispatchEvent(new Event('input', { bubbles: true }))");
      await until("!!document.querySelector('main > .screen:not([inert]) button.album img')?.complete");
      await check(`${phone_}: open an album from Search`, tap("document.querySelector('main > .screen:not([inert]) button.album')"), 2);
      await check(`${phone_}: swipe back to Search`, () => swipe(20, 300, 410), 2);
      for (const [label, w, h] of [
        [phone_, 384, 820],
        ["desktop", 1200, 800],
      ]) {
        if (label === "desktop" && (scale !== 100 || font)) continue;
        if (!(await open("dev-view=albums", w, h))) continue;
        await until(`!!${card}?.querySelector('img')?.complete`);
        await check(`${label}: open an album`, tap(card), 2);
        if (label === "desktop") {
          await check(`${label}: back by the button`, tap("document.querySelector('main > .screen:not([inert]) .back')"), 2);
          await check(`${label}: open it again`, tap(card), 2);
        }
        await check(`${label}: swipe back`, () => swipe(20, w * 0.8, h / 2), 2);
        await check(`${label}: swipe back cancelled`, async () => {
          await run(`${card}.click()`);
          await until(settled);
          await run("window.motifLandings.length = 0");
          await swipe(20, 40, h / 2);
        }, 0);
      }
      await sheet(phone_);
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
    await open("dev-settings=interface", 384, 820);
    await check("phone: Settings section back to the list", () => swipe(20, 300, 400), 0);
    await check("phone: Settings list to a section", tap("document.querySelector('main > .screen:not([inert]) .phone-list button')"), 0);
  }
} finally {
  if (phone) {
    await forget();
    await send("Page.navigate", { url: home });
  }
}
ws.close();
if (browser) {
  browser.kill();
  await new Promise((r) => browser.once("exit", r));
  fs.rmSync(profile, { recursive: true, force: true });
}
if (failures.length) {
  console.log(`\n${failures.length} failed:\n${failures.join("\n")}`);
  process.exit(1);
}
console.log("\nall landed");
