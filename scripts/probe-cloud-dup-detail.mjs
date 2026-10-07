// 调研：对重复组的 songId 查 /song/detail，看哪份音质更高（只读）。
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(readFileSync(join(root, "release/music-auto-sync_x64_portable/data/config.json"), "utf8"));
const cookie = config.cookie ?? "";
const BASE = config.apiBase ?? "https://netease-api.muxinxy.com";

const pairs = [
  ["起风了", [3319364943, 1385227380]],
  ["All Falls Down", [1333159925, 515453363]],
  ["你根本不懂", [517009807, 535028042]],
  ["Lemon Tree", [26527878, 27180681]],
  ["Proud Of You", [229070, 5197160]],
  ["去年夏天", [557581476, 1306853600]],
  ["识食物者为俊杰", [487190408, 1338150150]],
  ["能否否", [569214126, 1301756460]],
];
const ids = pairs.flatMap(([, ids]) => ids).join(",");

const raw = execFileSync("curl.exe", [
  "-sS", "--proxy", "http://127.0.0.1:7897", "--connect-timeout", "15", "--max-time", "60",
  "-G", "-H", "Accept: application/json",
  "--data-urlencode", `cookie=${cookie}`,
  "--data-urlencode", `ids=${ids}`,
  "--data-urlencode", `timestamp=${Date.now()}`,
  `${BASE}/song/detail`,
], { encoding: "utf8" });
const res = JSON.parse(raw);
const byId = new Map((res.songs ?? []).map((s) => [s.id, s]));
for (const [name, ids] of pairs) {
  console.log(`\n${name}:`);
  for (const id of ids) {
    const s = byId.get(id);
    if (!s) { console.log(`  ${id}: 无返回`); continue; }
    const priv = (s.privilege ?? {});
    console.log(`  ${id}: ${(s.ar ?? []).map((a) => a.name).join("/")} | 大小=${((s.l?.size ?? s.h?.size ?? s.m?.size ?? 0) / 1048576).toFixed(1)}MB | 最高=${priv.maxbr ?? "?"}kbps | dl=${priv.dlLevel ?? "?"}`);
  }
}
