// 验证无损下载：查看歌曲资源标记 + 以 lossless/hires 实际请求并下载。
import { readFileSync, mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(readFileSync(join(root, "release/music-auto-sync_x64_portable/data/config.json"), "utf8"));
const cookie = config.cookie ?? "";
const BASE = config.apiBase ?? "https://netease-api.muxinxy.com";
const PROXY = "http://127.0.0.1:7897";
const SONG_ID = 28815250;

function api(path, params) {
  const qs = new URLSearchParams(params);
  qs.set("timestamp", Date.now());
  return execFileSync("curl.exe", [
    "-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "40",
    "-G", "--data-urlencode", `cookie=${cookie}`, `${BASE}${path}?${qs.toString()}`,
  ], { encoding: "utf8" });
}

// 1) 资源标记
const detail = JSON.parse(api("/song/detail", { ids: SONG_ID }));
const song = detail.songs?.[0];
if (song) {
  console.log("歌曲:", song.name, "-", song.ar?.map((a) => a.name).join(","));
  console.log("资源标记: l(128k)=", song.l ? "有" : "无", "m(192k)=", song.m ? "有" : "无",
    "h(320k)=", song.h ? "有" : "无", "sq(无损)=", song.sq ? "有" : "无", "hr(Hi-Res)=", song.hr ? "有" : "无");
  if (song.sq) console.log("  sq 大小:", song.sq.size, "br:", song.sq.br);
  if (song.hr) console.log("  hr 大小:", song.hr.size, "br:", song.hr.br);
}

// 2) 以无损/Hi-Res 请求各接口
const attempts = [
  ["/song/url/v1", { level: "lossless" }],
  ["/song/url/v1", { level: "hires" }],
  ["/song/download/url/v1", { level: "lossless" }],
  ["/song/download/url/v1", { level: "hires" }],
  ["/song/download/url", { br: 999000 }],
  ["/song/download/url", { br: 1999000 }],
];

let flacUrl = null;
for (const [path, params] of attempts) {
  try {
    const payload = JSON.parse(api(path, { id: SONG_ID, ...params }));
    const data = Array.isArray(payload.data) ? payload.data[0] : payload.data;
    const info = {
      code: payload.code,
      type: data?.type ?? null,
      level: data?.level ?? null,
      br: data?.br ?? null,
      size: data?.size ?? null,
      url: data?.url ? "有" : "无",
    };
    console.log(`${path} ${JSON.stringify(params)} =>`, JSON.stringify(info));
    if (data?.url && (data.type === "FLAC" || (data.br ?? 0) >= 900000)) flacUrl = data.url;
  } catch (error) {
    console.log(`${path} ${JSON.stringify(params)} => 失败 ${String(error).slice(0, 100)}`);
  }
}

// 3) 若拿到无损链接则下载
if (flacUrl) {
  const folder = "D:\\Music\\lossless-test";
  mkdirSync(folder, { recursive: true });
  const file = join(folder, "平凡之路.flac");
  const out = execFileSync("curl.exe", [
    "-sSL", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "300",
    "-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131",
    "-e", "https://music.163.com/",
    "-o", file, "-w", "%{http_code} %{size_download}", flacUrl.replace(/^http:/, "https:"),
  ], { encoding: "utf8" });
  const [http, size] = out.trim().split(" ");
  console.log(`无损下载: HTTP=${http} 大小=${(Number(size) / 1048576).toFixed(1)}MB -> ${file}`);
} else {
  console.log("结论：所有接口均未返回无损地址（账号权益不足，被降级为 320k MP3）。");
}