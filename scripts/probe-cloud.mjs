// 调研：云盘接口真实返回结构（/user/cloud、/cloud/upload/token）。
// 仅使用本地配置文件中的登录凭据，不输出 cookie 明文；token 探测不触发任何上传。
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

function curlPostForm(pathWithQuery, formPairs) {
  const args = [
    "-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "60",
    "-X", "POST", "-H", "Accept: application/json",
  ];
  for (const [k, v] of formPairs) args.push("--data-urlencode", `${k}=${v}`);
  args.push(`${BASE}${pathWithQuery}`);
  const raw = execFileSync("curl.exe", args, { encoding: "utf8" });
  return JSON.parse(raw);
}

const TS = Date.now();

// 1) 列表结构（少量样本）
try {
  const page = curlJson(`/user/cloud?limit=3&timestamp=${TS}`);
  console.log("== /user/cloud limit=3 code:", page.code);
  console.log("top-level keys:", Object.keys(page));
  if (page.count !== undefined) console.log("count:", page.count);
  const data = page.data ?? [];
  console.log("data length:", data.length);
  if (data.length > 0) {
    const item = data[0];
    console.log("item keys:", Object.keys(item));
    console.log("item sample:", JSON.stringify(item, null, 2).slice(0, 1600));
    if (item.simpleSong) {
      console.log("simpleSong keys:", Object.keys(item.simpleSong));
      const ss = item.simpleSong;
      console.log("simpleSong core:", JSON.stringify({
        id: ss.id, name: ss.name, dt: ss.dt,
        ar: ss.ar, al: ss.al, fileSize: ss.fileSize, bitrate: ss.bitrate,
      }));
    }
  }
} catch (e) {
  console.log("/user/cloud FAILED:", String(e));
}

// 2) 分页上限（limit=200 是否生效）
try {
  const page = curlJson(`/user/cloud?limit=200&offset=0&timestamp=${TS}`);
  const data = page.data ?? [];
  console.log("\n== /user/cloud limit=200 -> items:", data.length, "count:", page.count ?? "(none)");
  if (data.length === 200) {
    const page2 = curlJson(`/user/cloud?limit=200&offset=200&timestamp=${TS}`);
    console.log("offset=200 -> items:", (page2.data ?? []).length);
  }
} catch (e) {
  console.log("/user/cloud limit=200 FAILED:", String(e));
}

// 3) 上传凭证（固定假 MD5，仅验证端点与返回字段，绝不 PUT）
try {
  const token = curlPostForm(`/cloud/upload/token?timestamp=${TS}`, [
    ["cookie", cookie],
    ["md5", "d41d8cd98f00b204e9800998ecf8427e"],
    ["fileSize", "1024"],
    ["filename", "probe-cloud-test.mp3"],
  ]);
  console.log("\n== /cloud/upload/token (form) code:", token.code);
  const d = token.data ?? {};
  console.log("data keys:", Object.keys(d));
  console.log("needUpload:", d.needUpload, "songId:", d.songId, "resourceId:", d.resourceId);
  console.log("uploadUrl prefix:", typeof d.uploadUrl === "string" ? d.uploadUrl.slice(0, 120) : d.uploadUrl);
} catch (e) {
  console.log("/cloud/upload/token (form) FAILED:", String(e));
}
