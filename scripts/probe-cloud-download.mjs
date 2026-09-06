// 调研:/song/cloud/download 返回结构与可用性
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(
  readFileSync(join(root, "release/music-auto-sync_x64_portable/data/config.json"), "utf8")
);
const cookie = config.cookie ?? "";
const BASE = config.apiBase ?? "https://netease-api.muxinxy.com";
const PROXY = "http://127.0.0.1:7897";

function curlJson(args) {
  const raw = execFileSync(
    "curl.exe",
    ["-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "60", ...args],
    { encoding: "utf8" }
  );
  return JSON.parse(raw);
}

const list = curlJson([
  "-G", "-H", "Accept: application/json",
  "--data-urlencode", `cookie=${cookie}`,
  "--data-urlencode", "limit=3",
  "--data-urlencode", `timestamp=${Date.now()}`,
  `${BASE}/user/cloud`,
]);
const item = (list.data ?? [])[0];
if (!item) {
  console.log("cloud empty");
  process.exit(0);
}
console.log("probe item songId:", item.songId, "simpleSong.id:", item.simpleSong?.id, "fileName:", item.fileName);

const dl = curlJson([
  "-G", "-H", "Accept: application/json",
  "--data-urlencode", `cookie=${cookie}`,
  "--data-urlencode", `id=${item.songId}`,
  "--data-urlencode", `timestamp=${Date.now()}`,
  `${BASE}/song/cloud/download`,
]);
console.log("download response keys:", Object.keys(dl));
console.log(JSON.stringify(dl).slice(0, 600));
