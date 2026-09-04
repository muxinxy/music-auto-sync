// 无损专项测试：多首歌 × 多接口 × lossless/hires，检查是否存在 FLAC 返回。
// 若拿到 FLAC 直链则下载并校验文件头（fLaC）。
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(readFileSync(join(root, "release/music-auto-sync_x64_portable/data/config.json"), "utf8"));
const cookie = config.cookie ?? "";
const BASE = config.apiBase ?? "https://netease-api.muxinxy.com";
const PROXY = "http://127.0.0.1:7897";

function api(path, params) {
  const qs = new URLSearchParams(params);
  qs.set("timestamp", Date.now());
  return execFileSync("curl.exe", [
    "-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "40",
    "-G", "--data-urlencode", `cookie=${cookie}`, `${BASE}${path}?${qs.toString()}`,
  ], { encoding: "utf8" });
}

function summarize(payload) {
  const data = payload.data;
  const first = Array.isArray(data) ? data[0] : data;
  if (!first || typeof first !== "object" || !first.url) {
    return { url: null, type: null, level: null, br: first?.br ?? null, size: first?.size ?? null };
  }
  return {
    url: first.url.replace(/^http:/, "https:"),
    type: first.type ?? null,
    level: first.level ?? null,
    br: first.br ?? null,
    size: first.size ?? null,
  };
}

// 选曲：平凡之路 + 搜索“晴天 周杰伦”“演员 林俊杰”等热门无损常见曲目
const targets = [];
const searchList = ["平凡之路 朴树", "晴天 周杰伦", "不为谁而作的歌 林俊杰"];
for (const keyword of searchList) {
  try {
    const raw = api("/search", { keywords: keyword, limit: 1 });
    const song = JSON.parse(raw).result?.songs?.[0];
    if (song) targets.push({ id: song.id, name: `${song.name} - ${(song.ar ?? []).map((a) => a.name).join(",")}` });
  } catch {}
}

const attempts = [
  ["/song/url/v1", { level: "lossless" }, "cnip"],
  ["/song/url/v1", { level: "hires" }, "cnip"],
  ["/song/download/url/v1", { level: "lossless" }, "cnip"],
  ["/song/url/v1", { level: "lossless" }, "nocnip"],
  ["/song/download/url", { br: "999000" }, "cnip"],
];

let losslessFound = 0;
for (const target of targets) {
  console.log(`\n== ${target.name} (id=${target.id}) ==`);
  for (const [path, extra, ipMode] of attempts) {
    const params = { id: target.id, ...extra };
    if (ipMode === "cnip") params.randomCNIP = "true";
    try {
      const info = summarize(JSON.parse(api(path, params)));
      const tag = `${path} ${JSON.stringify(extra)} [${ipMode}]`;
      console.log(`${tag} => type=${info.type} level=${info.level} br=${info.br} size=${info.size ? (info.size / 1048576).toFixed(1) + "MB" : info.size}`);
      const isFlac = /flac/i.test(String(info.type ?? "")) || /lossless|hires/i.test(String(info.level ?? ""));
      if (isFlac && info.url) {
        losslessFound += 1;
        const out = join(process.env.TEMP, `lossless-${target.id}-${losslessFound}.bin`);
        const dl = execFileSync("curl.exe", [
          "-sSL", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "300",
          "-A", "Mozilla/5.0 Chrome/131", "-e", "https://music.163.com/",
          "-o", out, "-w", "%{http_code} %{size_download}", info.url,
        ], { encoding: "utf8" });
        let head = "";
        try {
          const head4 = readFileSync(out).slice(0, 4);
          head = [...head4].map((b) => b.toString(16).padStart(2, "0")).join(" ");
        } catch {}
        console.log(`  ↓ 下载 ${dl.trim()} 头4字节=${head} ${head === "66 4c 61 43" ? "(fLaC 无损✓)" : ""}`);
      }
    } catch (error) {
      console.log(`${path} ${JSON.stringify(extra)} [${ipMode}] => 错误 ${String(error).slice(0, 100)}`);
    }
  }
}
console.log(`\n结论: ${losslessFound > 0 ? `拿到 ${losslessFound} 个无损直链` : "所有组合均未返回无损（FLAC），账号权益不含无损下载"}`);