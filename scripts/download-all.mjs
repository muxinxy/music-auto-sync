// 用本地登录 cookie 调用全部歌曲下载接口，下载《平凡之路》到 D:\Music\<接口名>\
// 每个接口一个文件夹，汇总写入 D:\Music\manifest.json。不输出 cookie 明文。
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(readFileSync(join(root, "release/music-auto-sync_x64_portable/data/config.json"), "utf8"));
const cookie = config.cookie ?? "";
if (!cookie || !/MUSIC_U=/.test(cookie)) { console.error("无有效 cookie"); process.exit(1); }

const BASE = config.apiBase ?? "https://netease-api.muxinxy.com";
const PROXY = "http://127.0.0.1:7897";
const SONG_ID = 28815250; // 平凡之路
const OUT = "D:\\Music";
mkdirSync(OUT, { recursive: true });

const UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131";
const REFERER = "https://music.163.com/";

function api(path, params) {
  const qs = new URLSearchParams(params);
  qs.set("timestamp", Date.now());
  const url = `${BASE}${path}?${qs.toString()}`;
  return execFileSync("curl.exe", [
    "-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "40",
    "-G", "--data-urlencode", `cookie=${cookie}`, url,
  ], { encoding: "utf8" });
}

function parseUrl(payload) {
  const code = payload.code ?? null;
  const data = payload.data;
  let url = null, info = null;
  if (Array.isArray(data)) {
    const first = data[0];
    if (first) {
      url = typeof first.url === "string" ? first.url : null;
      info = { br: first.br ?? null, size: first.size ?? null, type: first.type ?? null, level: first.level ?? null };
    }
  } else if (typeof data === "string") {
    url = data;
  } else if (data && typeof data.url === "string") {
    url = data.url;
  }
  return { code, url: url ? url.replace(/^http:/, "https:") : null, info };
}

function download(folder, filename, url) {
  const file = join(folder, filename);
  const out = execFileSync("curl.exe", [
    "-sSL", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "180",
    "-A", UA, "-e", REFERER,
    "-o", file, "-w", "%{http_code} %{size_download}", url,
  ], { encoding: "utf8" });
  const [http, size] = out.trim().split(" ");
  return { file, http, size: Number(size ?? 0), bytes: fileExists(file) };
}

const cases = [
  { name: "song-url", path: "/song/url", params: { id: SONG_ID, br: 320000 }, filename: "平凡之路-br320000.mp3" },
  { name: "song-url-v1", path: "/song/url/v1", params: { id: SONG_ID, level: "lossless" }, filename: "平凡之路-lossless.mp3" },
  { name: "song-url-v1-302", path: "/song/url/v1/302", params: { id: SONG_ID, level: "lossless" }, filename: "平凡之路-lossless-302.mp3" },
  { name: "song-download-url", path: "/song/download/url", params: { id: SONG_ID, br: 999000 }, filename: "平凡之路-br999000.mp3" },
  { name: "song-download-url-v1", path: "/song/download/url/v1", params: { id: SONG_ID, level: "lossless" }, filename: "平凡之路-lossless.mp3" },
];

const manifest = { songId: SONG_ID, song: "平凡之路", generatedAt: new Date().toISOString(), results: {} };
console.log("songId=" + SONG_ID);

for (const c of cases) {
  const folder = join(OUT, c.name);
  mkdirSync(folder, { recursive: true });
  try {
    if (c.path.endsWith("/302")) {
      // 302 模式接口直接重定向到真实地址，跟随下载即可。
      const qs = new URLSearchParams(c.params);
      qs.set("timestamp", Date.now());
      const target = `${BASE}${c.path}?${qs.toString()}`;
      const dl = execFileSync("curl.exe", [
        "-sSL", "-L", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "180",
        "-A", UA, "-e", REFERER,
        "--data-urlencode", `cookie=${cookie}`,
        "-o", join(folder, c.filename), "-w", "%{http_code} %{size_download} %{url_effective}", target,
      ], { encoding: "utf8" });
      const [http, size] = dl.trim().split(" ");
      manifest.results[c.name] = { path: c.path, params: c.params, downloadHttp: http, downloadBytes: Number(size ?? 0) };
      console.log(`${c.name}: 302跟随 dl=${http} ${(Number(size ?? 0) / 1048576).toFixed(1)}MB`);
      continue;
    }
    const parsed = parseUrl(JSON.parse(api(c.path, c.params)));
    if (parsed.url) {
      const dl = download(folder, c.filename, parsed.url);
      manifest.results[c.name] = {
        path: c.path,
        params: c.params,
        apiCode: parsed.code,
        info: parsed.info,
        downloadHttp: dl.http,
        downloadBytes: dl.size,
      };
      console.log(`${c.name}: code=${parsed.code} info=${JSON.stringify(parsed.info)} dl=${dl.http} ${(dl.size / 1048576).toFixed(1)}MB`);
    } else {
      manifest.results[c.name] = { path: c.path, params: c.params, apiCode: parsed.code, error: "无地址返回" };
      console.log(`${c.name}: code=${parsed.code} 无地址`);
    }
  } catch (error) {
    manifest.results[c.name] = { path: c.path, params: c.params, error: String(error).slice(0, 160) };
    console.log(`${c.name}: 失败 ${String(error).slice(0, 120)}`);
  }
}

function fileExists(p) {
  try { return readFileSync(p).length; } catch { return 0; }
}

writeFileSync(join(OUT, "manifest.json"), JSON.stringify(manifest, null, 2), "utf8");
console.log("manifest -> D:\\Music\\manifest.json");