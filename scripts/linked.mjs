// Playing from a linked engine (#243): two muted test engines, linked through
// their pairing files; A holds three 25 s FLACs, B none. B plays A's tracks,
// seeks, moves gaplessly into the next, then A is killed mid-track and B must
// play on from what it fetched (the current and next tracks, whole).
//
//   cargo build -p headless
//   node scripts/linked.mjs [motif-headless] [work dir]
//
// Needs ffmpeg. Ports 7741 (A) and 7742 (B), HTTP and iroh alike.
import os from "node:os";
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";

const [bin = "target/debug/motif-headless", dir = path.join(os.tmpdir(), "motif-linked")] =
  process.argv.slice(2);
const PORTS = { a: 7741, b: 7742 };
fs.rmSync(dir, { recursive: true, force: true });
const keyOf = (secret) => {
  const der = Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), secret]);
  const pub = crypto.createPublicKey(crypto.createPrivateKey({ key: der, format: "der", type: "pkcs8" }));
  return pub.export({ format: "der", type: "spki" }).subarray(12).toString("hex");
};
const secrets = { a: crypto.randomBytes(32), b: crypto.randomBytes(32) };
for (const [me, other] of [["a", "b"], ["b", "a"]]) {
  const data = path.join(dir, me);
  fs.mkdirSync(data, { recursive: true });
  fs.writeFileSync(path.join(data, "identity.key"), secrets[me]);
  fs.writeFileSync(path.join(data, "pairing.json"), JSON.stringify({
    auto_approve: null, log: [], internet: false,
    devices: [{ id: `dev${other}`, name: other.toUpperCase(), public_key: keyOf(secrets[other]), scopes: ["sync"],
      paired_at_ms: Date.now(), last_seen_ms: null, token_hash: null,
      address: { direct: [`127.0.0.1:${PORTS[other]}`], relay: null } }],
  }));
}
const music = path.join(dir, "music");
fs.mkdirSync(music);
for (let t = 1; t <= 3; t++) {
  execFileSync("ffmpeg", ["-loglevel", "error", "-f", "lavfi", "-i", `anoisesrc=d=25:a=0.3:s=${t}`,
    "-c:a", "flac", "-metadata", `title=T${t}`, "-metadata", "artist=Remote", "-metadata", "album=Remote",
    "-metadata", `track=${t}`, path.join(music, `${t}.flac`)]);
}
const logs = { a: "", b: "" };
const engines = {};
for (const me of ["a", "b"]) {
  const engine = spawn(bin, [], {
    env: { ...process.env, MOTIF_ADDR: `127.0.0.1:${PORTS[me]}`, MOTIF_DATA_DIR: path.join(dir, me),
      MOTIF_UI_DIR: "/nonexistent", RUST_LOG: "info,server=debug" },
  });
  engine.stderr.on("data", (d) => (logs[me] += d));
  engine.stdout.on("data", (d) => (logs[me] += d));
  engines[me] = engine;
}
process.on("exit", () => Object.values(engines).forEach((e) => e.kill()));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await sleep(1500);

async function open(me) {
  const ws = new WebSocket(`ws://127.0.0.1:${PORTS[me]}/ws`);
  const waiting = new Map();
  const s = { ws, state: null, queue: [], context: [] };
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    if (msg.id && waiting.has(msg.id)) { waiting.get(msg.id)(msg); waiting.delete(msg.id); }
    const m = msg.type === "event" ? msg.event : {};
    if (m.type === "state") s.state = m.state;
    if (m.type === "context") s.context = m.items;
    if (m.type === "queue") s.queue = m.items;
  };
  let id = 0;
  const send = (key) => (body) => new Promise((r) => { const i = ++id; waiting.set(i, r); ws.send(JSON.stringify({ id: i, [key]: body })); });
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  return { ...s, get: () => s, q: send("query"), c: send("command") };
}
const until = async (what, f, ms = 20000) => {
  const end = Date.now() + ms;
  while (!(await f())) { if (Date.now() > end) throw new Error(`timed out: ${what}\ncontext ${JSON.stringify(b?.get().context.map((i) => [i.track.title, i.error]))}\nB log:\n${logs.b.slice(-3000)}`); await sleep(100); }
};
const pos = (s) => s.state.position_ms + (s.state.status === "playing" ? Date.now() - s.state.sampled_at_ms : 0);

let b;
const a = await open("a");
await a.q({ type: "add_root", path: music });
b = await open("b");
await b.c({ type: "set_volume", volume: 0 });
const tracksOf = async (e) => (await e.q({ type: "tracks", search: null, sort: [], filter: {}, offset: 0, limit: 10 })).result;
let listed;
await until("A's tracks on B", async () => { const r = await tracksOf(b); listed = r?.tracks; return listed?.length === 3; }, 30000);
const refs = listed.map((t) => t.track).sort((x, y) => (x.title < y.title ? -1 : 1));
console.log("on B:", refs.map((r) => `${r.title} ${r.id}`).join(", "));
const t0 = Date.now();
await b.c({ type: "set_context", tracks: refs, start: 0, kind: "album", label: "Remote" });
await b.c({ type: "play" });
await until("playing", () => b.get().state?.status === "playing" && pos(b.get()) > 300);
console.log(`started in ~${Date.now() - t0} ms (position ${pos(b.get())} ms)`);
await sleep(1000);
await b.c({ type: "seek", position_ms: 20000 });
await sleep(1500);
const afterSeek = pos(b.get());
console.log(`seeked to 20000, now ${afterSeek} ms`);
if (afterSeek < 20500 || afterSeek > 22500 || b.get().state.current?.track.title !== "T1") {
  throw new Error("seek didn't hold");
}
await until("gapless into T2", () => b.get().state.current?.track.title === "T2", 10000);
await sleep(2000);
const s = b.get();
console.log(`now ${s.state.current.track.title} at ${pos(s)} ms, status ${s.state.status}`);
const underruns = (logs.b.match(/audio underrun/g) || []).length;
console.log(`underrun warnings on B: ${underruns}`);
if (underruns) throw new Error("not gapless");
console.log("killing A mid-track");
const killed = Date.now();
engines.a.kill("SIGKILL");
// T2 and the next, T3, are on B's disk by now (#282): B plays on through both.
await until("T3 playing on B without A", () => {
  const s = b.get();
  return s.state.current?.track.title === "T3" && s.state.status === "playing" && pos(s) > 1000;
}, 40000);
const items = [...b.get().context, ...b.get().queue];
console.log(`T3 playing ${Date.now() - killed} ms after A went`);
for (const i of items) console.log(`  ${i.track.title}: ${i.error ?? "ok"}`);
if (items.some((i) => i.error)) throw new Error("a track on disk failed without A");
console.log("ok");
process.exit(0);
