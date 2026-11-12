// Landing check (docs/ANIMATION.md, Checking): drives the UI in headless
// Chromium through screen changes (taps and swipes, phone and desktop) and
// fails if any shared element's copy landed off its real element (box) or
// looking different (colour, weight, size), or if a change left anything
// behind (a second screen, copies, hidden elements).
//
//   node scripts/landing.mjs [base-url]
//
// base-url is a dev server (`npm run dev`; default http://localhost:5173)
// whose engine has a library with albums. Use a muted test engine of your
// own (CLAUDE.md), not one playing. Needs Chromium (CHROME overrides it).
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const base = process.argv[2] ?? "http://localhost:5173";
const chrome = process.env.CHROME ?? "chromium";
const profile = fs.mkdtempSync(path.join(os.tmpdir(), "motif-landing-"));
const browser = spawn(chrome, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--disable-features=OverscrollHistoryNavigation", "about:blank"], { stdio: "ignore" });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let port;
for (let i = 0; i < 100 && !port; i++) {
  await sleep(100);
  try {
    port = fs.readFileSync(path.join(profile, "DevToolsActivePort"), "utf8").split("\n")[0];
  } catch {}
}
if (!port) throw new Error(`${chrome} didn't start`);
const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
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

async function open(query, width, height) {
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: width <= 700 });
  await send("Page.navigate", { url: `${base}/?${query}` });
  await until("!!window.motifLandings && document.querySelectorAll('main > .screen').length === 1");
}

/**
 * A finger from (x0, y) to (x1, y), in steps, as the UI's gestures see it:
 * touch pointer events made in the page, since headless Chromium cancels its
 * own touches as soon as they move.
 */
async function swipe(dx0, dx1, y) {
  // From the music area's left edge (desktop has the left panel before it).
  const left = await run("document.querySelector('main').getBoundingClientRect().left");
  const [x0, x1] = [left + dx0, left + dx1];
  const fire = (type, x) =>
    run(`(document.elementFromPoint(${x0}, ${y}) ?? document.body).dispatchEvent(new PointerEvent('${type}', { pointerType: 'touch', pointerId: 7, isPrimary: true, button: 0, clientX: ${x}, clientY: ${y}, bubbles: true, composed: true }))`);
  await fire("pointerdown", x0);
  for (let i = 1; i <= 12; i++) {
    await sleep(16);
    await fire("pointermove", x0 + ((x1 - x0) * i) / 12);
  }
  await fire("pointerup", x1);
}

const settled = "document.querySelectorAll('main > .screen').length === 1 && !document.querySelector('.motion-layer')?.children.length";
const failures = [];

/** Runs `act`, waits for the change to settle, then checks its landings (at least `least` of them). */
async function check(name, act, least = 1) {
  await run("window.motifLandings.length = 0");
  await act();
  try {
    await until(settled);
  } catch {
    failures.push(`${name}: didn't settle (screens or copies left over)`);
    return;
  }
  const landings = await run("JSON.stringify(window.motifLandings)").then(JSON.parse);
  const hidden = await run("[...document.querySelectorAll('main [style*=\"visibility: hidden\"]')].map((e) => e.dataset.move ?? e.className)");
  const bad = landings.filter((l) => l.box > 1 || l.look.length);
  for (const l of bad) failures.push(`${name}: ${l.name} landed ${l.box.toFixed(1)}px off${l.look.length ? `, ${l.look.join(", ")} differ` : ""}`);
  if (landings.length < least) failures.push(`${name}: ${landings.length} shared elements flew, expected at least ${least}`);
  if (hidden.length) failures.push(`${name}: left hidden: ${hidden.join(", ")}`);
  const mine = failures.filter((f) => f.startsWith(`${name}:`));
  console.log(`${mine.length ? "FAIL" : "ok  "} ${name} (${landings.length} landed)${mine.map((f) => `\n       ${f.slice(name.length + 2)}`).join("")}`);
}

const card = "document.querySelector('main .card:not(.loading)')";
const tap = (selector) => () => run(`${selector}.click()`);

// Debugging: STEPS=steps.mjs runs that module's default export with these helpers instead.
if (process.env.STEPS) await (await import(path.resolve(process.env.STEPS))).default({ send, run, until, open, swipe, sleep, settled, card });
else {
  // Phone: a category from the Library home and back by its back button; desktop has no home screen.
  await open("dev-view=home", 384, 820);
  await check("phone: open a category", tap("document.querySelectorAll('main .categories button')[1]"), 2);
  await check("phone: back by the button", tap("document.querySelector('main .back')"), 2);
  // Search: an album found, and back to the results.
  await open("dev-view=search", 384, 820);
  await run("const i = document.querySelector('main input[type=search]'); i.value = 'Short'; i.dispatchEvent(new Event('input', { bubbles: true }))");
  await until("!!document.querySelector('main button.album img')?.complete");
  await check("phone: open an album from Search", tap("document.querySelector('main button.album')"), 2);
  await check("phone: swipe back to Search", () => swipe(20, 300, 410), 2);
  for (const [label, w, h] of [
    ["phone", 384, 820],
    ["desktop", 1200, 800],
  ]) {
    await open("dev-view=albums", w, h);
    await until(`!!${card}?.querySelector('img')?.complete`);
    await check(`${label}: open an album`, tap(card), 2);
    if (label === "desktop") {
      await check(`${label}: back by the button`, tap("document.querySelector('main .back')"), 2);
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
}
if (!process.env.STEPS) {
  await open("dev-settings=interface", 384, 820);
  await check("phone: Settings section back to the list", () => swipe(20, 300, 400), 0);
  await check("phone: Settings list to a section", tap("document.querySelector('main .phone-list button')"), 0);
}

ws.close();
browser.kill();
await new Promise((r) => browser.once("exit", r));
fs.rmSync(profile, { recursive: true, force: true });
if (failures.length) {
  console.log(`\n${failures.length} failed:\n${failures.join("\n")}`);
  process.exit(1);
}
console.log("\nall landed");
