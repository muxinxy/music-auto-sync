// 调研：歌单中灰色歌曲（st<0，官方已失效）与云盘备份的覆盖情况（只读）。
// 云盘价值 = 解灰备份：灰歌只有云盘里有副本才能继续播放。
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(readFileSync(join(root, "release/music-auto-sync_x64_portable/data/config.json"), "utf8"));
const cookie = config.cookie ?? "";
const uid = config.cookieUser?.userId;
if (!cookie || !uid) { console.error("config.json 中没有有效会话或 uid"); process.exit(1); }
const BASE = config.apiBase ?? "https://netease-api.muxinxy.com";

function curlOnce(pathWithQuery, formPairs) {
  const args = [
    "-sS", "--connect-timeout", "15", "--max-time", "180",
    "-G", "-H", "Accept: application/json",
    "--data-urlencode", `cookie=${cookie}`,
  ];
  for (const [k, v] of formPairs ?? []) args.push("--data-urlencode", `${k}=${v}`);
  args.push(`${BASE}${pathWithQuery}`);
  return JSON.parse(execFileSync("curl.exe", args, { encoding: "utf8" }));
}
function curlJson(pathWithQuery, formPairs) {
  for (let i = 0; i < 3; i++) {
    try {
      return curlOnce(pathWithQuery, formPairs);
    } catch (e) {
      if (i === 2) throw new Error(`GET ${pathWithQuery.split("?")[0]} 失败（重试 3 次）: ${e.status ?? ""}`);
    }
  }
}

// 1) 云盘全量
const cloud = [];
let offset = 0;
while (true) {
  const page = curlJson(`/user/cloud?limit=200&offset=${offset}`, [[`timestamp`, Date.now()]]);
  if (page.code !== 200) throw new Error(`/user/cloud code=${page.code}`);
  const data = page.data ?? [];
  cloud.push(...data);
  if (data.length < 200) break;
  offset += 200;
}
const norm = (s) => (s ?? "").toLowerCase().replace(/[\s'’‘·（）()\-_&,，。]/g, "");
const cloudNames = cloud.map((it) => {
  const ss = it.simpleSong ?? {};
  return { fileName: it.fileName ?? "", name: norm(ss.name), artist: norm((ss.ar ?? []).map((a) => a?.name).join("/")), songId: ss.id };
});

// 2) 歌单列表（自己创建的 + 收藏的都查）
const pl = curlJson(`/user/playlist?uid=${uid}`, [[`timestamp`, Date.now()]]);
if (pl.code !== 200) throw new Error(`/user/playlist code=${pl.code}`);
const playlists = (pl.playlist ?? []).filter((p) => p.userId === uid || p.subscribed);
console.log(`云盘 ${cloud.length} 首，歌单 ${playlists.length} 个（含收藏）`);

// 3) 逐歌单拉曲目，找灰歌
let totalGrey = 0, greyWithBackup = 0, totalTracks = 0;
let stCounts = {};
for (const p of playlists) {
  const tracks = [];
  let off = 0;
  let failed = false;
  while (true) {
    try {
      const r = curlJson(`/playlist/track/all?id=${p.id}&limit=500&offset=${off}`, [[`timestamp`, Date.now()]]);
      if (r.code !== 200) break;
      // 代理返回 songs 数组（与后端 api/mod.rs 一致），song 对象自带 st 字段
      const t = r.songs ?? r.tracks ?? [];
      tracks.push(...t);
      if (t.length < 500) break;
      off += 500;
    } catch (e) {
      console.log(`  (拉取失败，跳过该歌单: ${p.name})`);
      failed = true;
      break;
    }
  }
  if (failed) continue;
  totalTracks += tracks.length;
  for (const t of tracks) {
    const st = t.st ?? t.privilege?.st ?? "missing";
    stCounts[st] = (stCounts[st] ?? 0) + 1;
  }
  const grey = tracks.filter((t) => (t.st ?? t.privilege?.st ?? 0) < 0);
  if (grey.length === 0) continue;
  totalGrey += grey.length;
  const lines = grey.map((t) => {
    const nm = norm(t.name), ar = norm((t.ar ?? []).map((a) => a?.name).join("/"));
    // 匹配：云盘文件名的歌名部分含曲目名，或歌名+歌手都匹配
    const hit = cloudNames.find((c) => c.name.includes(nm) && nm.length > 1 && (ar ? c.artist.includes(ar) || ar.includes(c.artist) : true))
      ?? cloudNames.find((c) => c.fileName.toLowerCase().includes(nm) && nm.length > 1);
    if (hit) greyWithBackup++;
    return `    ${hit ? "✓" : "✗"} ${t.name} - ${(t.ar ?? []).map((a) => a.name).join("/")}${hit ? ` ← ${hit.fileName}` : ""}`;
  });
  console.log(`\n[${p.name}] ${p.trackCount ?? tracks.length} 首, 灰歌 ${grey.length}:`);
  lines.forEach((l) => console.log(l));
}
console.log(`\n== 汇总: 扫描 ${totalTracks} 首, st 分布=${JSON.stringify(stCounts)}, 灰歌 ${totalGrey} 首, 其中云盘有备份 ${greyWithBackup} 首, 缺口 ${totalGrey - greyWithBackup} 首`);
