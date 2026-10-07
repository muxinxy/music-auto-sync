// 调研：云盘重复数据的真实分布（只读，不删除任何东西）。
// 拉全量 /user/cloud，按多种键分组统计重复情况，评估"云盘去重清理"功能的实际价值。
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const configPath = join(root, "release/music-auto-sync_x64_portable/data/config.json");
const config = JSON.parse(readFileSync(configPath, "utf8"));
const cookie = config.cookie ?? "";
if (!cookie || !/MUSIC_U=/.test(cookie)) {
  console.error("config.json 中没有有效会话 cookie");
  process.exit(1);
}
const BASE = config.apiBase ?? "https://netease-api.muxinxy.com";
const PROXY = "http://127.0.0.1:7897";

function curlJson(pathWithQuery) {
  const raw = execFileSync("curl.exe", [
    "-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "60",
    "-G", "-H", "Accept: application/json",
    "--data-urlencode", `cookie=${cookie}`,
    `${BASE}${pathWithQuery}`,
  ], { encoding: "utf8" });
  return JSON.parse(raw);
}

// 全量分页拉取（200/页，与 core/cloud.rs 一致）
const all = [];
let offset = 0;
let count = null;
while (true) {
  const page = curlJson(`/user/cloud?limit=200&offset=${offset}&timestamp=${Date.now()}`);
  if (page.code !== 200) throw new Error(`/user/cloud code=${page.code}`);
  const data = page.data ?? [];
  if (count === null) count = page.count ?? null;
  all.push(...data);
  if (data.length < 200) break;
  offset += 200;
}
console.log(`云盘总数: count=${count ?? "?"}, 实际拉取=${all.length}`);

const items = all.map((it) => {
  const ss = it.simpleSong ?? {};
  const ar = (ss.ar ?? []).map((a) => a?.name).filter(Boolean).join("/");
  return {
    songId: ss.id,
    songName: ss.name ?? "",
    artist: ar,
    dt: ss.dt ?? 0,
    fileSize: ss.fileSize ?? 0,
    bitrate: ss.bitrate ?? 0,
    fileName: it.fileName ?? "",
    addTime: it.addTime ?? 0,
    songIdChanged: it.simpleSong?.id !== it.songId, // 字段映射核对
  };
});

function groupBy(keyFn, label) {
  const map = new Map();
  for (const it of items) {
    const k = keyFn(it);
    if (!k) continue;
    if (!map.has(k)) map.set(k, []);
    map.get(k).push(it);
  }
  const groups = [...map.values()].filter((g) => g.length > 1);
  const dupItems = groups.reduce((n, g) => n + g.length, 0);
  console.log(`\n== 按 ${label} ==`);
  console.log(`重复组: ${groups.length}, 涉及条目: ${dupItems} (占 ${(dupItems / Math.max(items.length, 1) * 100).toFixed(1)}%)`);
  for (const g of groups.slice(0, 10)) {
    console.log(`  x${g.length}: ${g[0].songName} - ${g[0].artist} | dt=${g[0].dt} | ${g.map((x) => `${x.fileName}(${x.songId},${(x.fileSize / 1048576).toFixed(1)}MB,${x.bitrate}kbps)`).join(" / ")}`);
  }
  return groups;
}

// 键1: 同一网易曲目 id（同步比对键，重复意味着比对键歧义）
groupBy((it) => it.songId, "网易曲目 id (songId)");
// 键2: 歌名+歌手+时长（songId 匹配失败或重新上传后的表现）
groupBy((it) => `${it.songName}::${it.artist}::${it.dt}`, "歌名+歌手+时长");
// 键3: 文件名（上传同名文件）
groupBy((it) => it.fileName, "文件名");

// 补充统计：songId=0（未匹配官方曲库）的条目
const unmatched = items.filter((it) => !it.songId);
console.log(`\n未匹配官方曲库 (songId=0): ${unmatched.length}`);
unmatched.slice(0, 5).forEach((it) => console.log(`  ${it.fileName} | ${it.songName} - ${it.artist}`));

// 补充统计：字段映射核对（simpleSong.id vs 顶层 songId 是否一致）
const mismatch = items.filter((it) => it.songIdChanged).length;
console.log(`\nsimpleSong.id 与顶层 songId 不一致的条目: ${mismatch}`);
