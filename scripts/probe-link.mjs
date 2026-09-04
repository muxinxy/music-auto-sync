import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const config = JSON.parse(readFileSync(join(root, "release/music-auto-sync_x64_portable/data/config.json"), "utf8"));
const cookie = config.cookie;
const BASE = config.apiBase ?? "https://netease-api.muxinxy.com";

// 1) 取 exhigh 直链
const searchOut = execFileSync("curl.exe", [
  "-sS", "--proxy", "http://127.0.0.1:7897", "--connect-timeout", "15", "--max-time", "30",
  "-G", "--data-urlencode", `cookie=${cookie}`,
  `${BASE}/song/url/v1?id=28815250&level=exhigh&timestamp=${Date.now()}`,
], { encoding: "utf8" });
const url = JSON.parse(searchOut).data?.[0]?.url;
if (!url) {
  console.error("未拿到直链");
  process.exit(1);
}
const httpsUrl = url.replace(/^http:/, "https:");
console.log("直链(https化):", httpsUrl.slice(0, 100));

// 2) 先走代理测一次；再直连（无 --proxy）测一次
for (const [label, args] of [
  ["代理出口", ["--proxy", "http://127.0.0.1:7897"]],
  ["本机直连", []],
] ) {
  try {
    const out = execFileSync("curl.exe", [
      "-sS", "--connect-timeout", "15", "--max-time", "120",
      ...args,
      "-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131",
      "-e", "https://music.163.com/",
      "-o", join(process.env.TEMP, "ncm-full.bin"),
      "-w", "%{http_code} %{size_download} %{time_total}s", httpsUrl,
    ], { encoding: "utf8" });
    const parts = out.trim().split(" ");
    const bytes = Number(parts[1] ?? 0);
    let header = "";
    try {
      const head = readFileSync(join(process.env.TEMP, "ncm-full.bin"));
      header = `前4字节=${[...head.slice(0, 4)].map((b) => b.toString(16)).join(" ")}`;
    } catch {}
    console.log(`${label}: HTTP=${parts[0]} 大小=${(bytes / 1048576).toFixed(1)}MB (${bytes}) ${header}`);
  } catch (error) {
    console.log(`${label}: 失败 ${String(error).slice(0, 140)}`);
  }
}