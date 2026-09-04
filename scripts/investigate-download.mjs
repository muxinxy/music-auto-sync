// 调研：88VIP 会员账号在不同歌曲地址接口下能否拿到无损完整音频。
// 仅使用本地配置文件中的登录凭据调用自托管 API，不输出 cookie 明文。
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
const TS = Date.now();
const SONG = process.argv[2] ?? "平凡之路 朴树";

function curlGet(url) {
  return execFileSync("curl.exe", [
    "-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "30",
    "-G", "-H", "Accept: application/json",
    "--data-urlencode", `cookie=${cookie}`,
    url,
  ], { encoding: "utf8" });
}

// 1) 搜索歌曲
const searchUrl = `${BASE}/search?keywords=${encodeURIComponent(SONG)}&limit=5&timestamp=${TS}`;
let songs = [];
try {
  const raw = execFileSync("curl.exe", [
    "-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "30",
    searchUrl,
  ], { encoding: "utf8" });
  songs = JSON.parse(raw).result?.songs ?? [];
} catch (error) {
  console.error("搜索失败:", String(error));
  process.exit(1);
}
if (songs.length === 0) {
  console.error("未找到歌曲");
  process.exit(1);
}
const song = songs[0];
const songId = song.id;
console.log(`搜索到：${song.name} - ${(song.ar ?? []).map((a) => a.name).join(",")} (id=${songId}) al=${song.al?.name}`);

// 2) 各候选接口
const candidates = [
  ["/song/url/v1", [{ id: songId, level: "lossless" }]],
  ["/song/url/v1", [{ id: songId, level: "exhigh" }]],
  ["/song/url/v1", [{ id: songId, level: "higher" }]],
  ["/song/url/v1", [{ id: songId, level: "standard" }]],
  ["/song/download/url/v1", [{ id: songId, level: "lossless" }]],
  ["/song/download/url/v1", [{ id: songId, level: "exhigh" }]],
  ["/song/download/url", [{ id: songId, br: 999000 }]],
  ["/song/download/url", [{ id: songId, br: 320000 }]],
];

function summarize(data) {
  const first = Array.isArray(data) ? data[0] : data;
  if (!first || typeof first !== "object") return { shape: typeof data };
  return {
    url: typeof first.url === "string" ? first.url.slice(0, 90) : first.url,
    br: first.br ?? null,
    size: first.size ?? null,
    type: first.type ?? null,
    level: first.level ?? null,
  };
}

const results = {};
for (const [path, paramObject] of candidates) {
  const params = new URLSearchParams(Object.entries(paramObject[0]));
  params.set("timestamp", TS);
  const url = `${BASE}${path}?${params.toString()}`;
  let code = null;
  let data = null;
  try {
    const payload = JSON.parse(curlGet(url));
    code = payload.code;
    data = payload.data;
    results[`${path} ${params.toString()}`] = { code, ...summarize(data) };
  } catch (error) {
    results[`${path} ${params.toString()}`] = {
      error: String(error).slice(0, 120),
    };
  }
}

// 3) 探测返回直链（带 UA/Referer）的文件大小与可下载性
console.log("\n接口返回：");
let firstUrl = null;
for (const [name, info] of Object.entries(results)) {
  console.log(name, "=>", JSON.stringify(info));
  if (!firstUrl && info.url && /^http/.test(info.url)) firstUrl = info.url;
}
if (firstUrl) {
  const tmp = process.env.TEMP + "\\ncm-probe.bin";
  try {
    const out = execFileSync("curl.exe", [
      "-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "90",
      "-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131",
      "-e", "https://music.163.com/",
      "-o", tmp, "-w", "%{http_code} %{size_download} %{content_type}", firstUrl,
    ], { encoding: "utf8" });
    console.log(`\n完整下载探测: ${out.trim()} bytes=${(Number(out.trim().split(" ")[1]) / 1048576).toFixed(1)} MB`);
  } catch (error) {
    console.log("完整下载失败:", String(error).slice(0, 160));
  }
}