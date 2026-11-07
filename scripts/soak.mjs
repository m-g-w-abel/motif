// Engine memory soak test (issue #53): starts motif-headless on a spare port
// with a generated FLAC library, drives it over /ws and /art, and prints RSS.
//
//   cargo build --release -p headless
//   node scripts/soak.mjs [mode] [seconds]
//
// mode: connect | query | art | play | rescan | mix (default mix, 1800 s).
// Needs ffmpeg and Linux (/proc). BIN, PORT and SOAK_DIR override the binary
// (default target/release/motif-headless), port (7711) and work folder;
// the library in SOAK_DIR/lib is reused between runs.
//
// ATTACH=host:port drives a test engine that is already running (and mutes it),
// with the library it has (nothing is generated or added); ADB_PID=<pid> reads
// its RSS over adb, e.g. a phone test engine behind `adb forward tcp:7799 tcp:7700`.
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const [mode = "mix", secs = "1800"] = process.argv.slice(2);
const bin = process.env.BIN ?? "target/release/motif-headless";
const attach = process.env.ATTACH;
const addr = attach ?? `127.0.0.1:${process.env.PORT ?? 7711}`;
const dir = process.env.SOAK_DIR ?? path.join(os.tmpdir(), "motif-soak");
const lib = path.join(dir, "lib");
const ALBUMS = 100;

// Large covers, like real scans: decoding them is what costs memory.
if (!attach && !fs.existsSync(path.join(lib, `a${ALBUMS}`, "4.flac"))) {
  console.log(`generating ${ALBUMS * 4} tracks in ${lib}`);
  for (let a = 1; a <= ALBUMS; a++) {
    const album = path.join(lib, `a${a}`);
    fs.mkdirSync(album, { recursive: true });
    const cover = path.join(album, "cover.jpg");
    const ff = (...args) => execFileSync("ffmpeg", ["-loglevel", "error", "-y", ...args]);
    ff("-f", "lavfi", "-i", `mandelbrot=s=1400x1400:start_scale=${1 + a / 50}`, "-frames:v", "1", cover);
    for (let t = 1; t <= 4; t++) {
      ff("-f", "lavfi", "-i", `sine=frequency=${200 + a + t * 10}:duration=20`, "-i", cover,
        "-map", "0", "-map", "1", "-c:a", "flac", "-disposition:v", "attached_pic",
        "-metadata", `title=T${t}`, "-metadata", `artist=Artist ${a % 20}`,
        "-metadata", `album=Album ${a}`, "-metadata", `track=${t}`, path.join(album, `${t}.flac`));
    }
    fs.rmSync(cover);
  }
}

const data = path.join(dir, "data");
let status;
if (attach) {
  const pid = process.env.ADB_PID;
  status = () => pid ? execFileSync("adb", ["shell", `cat /proc/${pid}/status`], { encoding: "utf8" }) : "VmRSS: 0";
} else {
  fs.rmSync(data, { recursive: true, force: true });
  const engine = spawn(bin, [], {
    env: { ...process.env, MOTIF_ADDR: addr, MOTIF_DATA_DIR: data, MOTIF_UI_DIR: "/nonexistent", RUST_LOG: "warn" },
    stdio: "inherit",
  });
  process.on("exit", () => engine.kill());
  status = () => fs.readFileSync(`/proc/${engine.pid}/status`, "utf8");
}
const rss = () => Number(/VmRSS:\s+(\d+)/.exec(status())[1]) / 1024;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function open() {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://${addr}/ws`);
    let id = 0;
    const waiting = new Map();
    const status = [];
    ws.onmessage = (e) => {
      const m = JSON.parse(e.data);
      if (m.event?.status) status.forEach((f) => f(m.event.status));
      if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); }
    };
    const send = (key) => (body) => new Promise((r) => { const i = ++id; waiting.set(i, r); ws.send(JSON.stringify({ id: i, [key]: body })); });
    ws.onopen = () => resolve({ ws, status, q: send("query"), c: send("command") });
    ws.onerror = reject;
  });
}

let main;
for (let i = 0; !main; i++) {
  await sleep(500);
  main = await open().catch(() => { if (i > 20) throw new Error(`no engine at ${addr}`); });
}
await main.c({ type: "set_volume", volume: 0 });
if (!attach) {
  await new Promise((done) => {
    main.status.push((s) => s.tracks >= ALBUMS * 4 && !s.scanning && done());
    main.q({ type: "add_root", path: lib });
  });
  await sleep(5000);
}
const albums = (await main.q({ type: "albums", search: null, sort: [], offset: 0, limit: 10000 })).result.albums;

const steps = {
  async connect() { const s = await open(); await s.q({ type: "status" }); s.ws.close(); },
  async query() {
    for (let o = 0; o < ALBUMS * 4; o += 50) await main.q({ type: "tracks", search: null, sort: [], offset: o, limit: 50 });
    await main.q({ type: "groups", search: "album", sort: [] });
    await main.q({ type: "album_tracks", albums: albums.slice(0, 20).map((a) => a.key), sort: [] });
  },
  // Every album at once, like a grid: a new thumbnail size each round, plus full size.
  async art() {
    const size = [64, 96, 128, 200, 256, 300, 400, 512][round % 8];
    await Promise.all(albums.filter((a) => a.art).map(async (a) => {
      await (await fetch(`http://${addr}/art/${a.art}?size=${size}`)).arrayBuffer();
      await (await fetch(`http://${addr}/art/${a.art}`)).arrayBuffer();
    }));
  },
  async play() {
    await main.q({ type: "play_album", album: albums[round % albums.length].key, sort: [] });
    await sleep(300);
    await main.c({ type: "seek", position_ms: 15000 });
    await sleep(300);
    await main.c({ type: "next" });
    await sleep(300);
  },
  async rescan() { await main.q({ type: "rescan" }); await sleep(5000); },
};

const start = Date.now();
const log = () => console.log(`${Math.round((Date.now() - start) / 1000)}s rss=${rss().toFixed(1)}MB`);
log();
const timer = setInterval(log, 60000);
let round = 0;
while (Date.now() - start < Number(secs) * 1000) {
  if (mode === "mix") {
    for (const step of ["connect", "query", "art", "play"]) await steps[step]();
    if (round % 20 === 0) await steps.rescan();
  } else await steps[mode]();
  round++;
}
await main.c({ type: "stop" });
clearInterval(timer);
await sleep(3000);
log();
console.log(`${round} rounds of ${mode}`);
process.exit(0);
