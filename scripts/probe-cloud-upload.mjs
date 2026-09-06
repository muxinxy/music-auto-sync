// 冒烟：云盘直传完整链路（token → 对象存储传输 → complete → 列表确认 → 删除清理）。
// 用法：node scripts/probe-cloud-upload.mjs <真实音频文件路径>
// 仅使用本地配置文件中的登录凭据，不输出 cookie 明文。
// 上传前用 MD5 确认该文件不在云盘；验证后立即按 songId 删除，不留残留。
import { readFileSync, unlinkSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { join, dirname, basename } from "node:path";
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
const srcPath = process.argv[2];
if (!srcPath) {
  console.error("用法: node scripts/probe-cloud-upload.mjs <音频文件>");
  process.exit(1);
}
const payload = readFileSync(srcPath);
const md5 = createHash("md5").update(payload).digest("hex");
const fileSize = payload.length;
const fileName = basename(srcPath);
console.log("test file:", fileName, fileSize, "bytes md5:", md5);

function curlJson(args) {
  const raw = execFileSync(
    "curl.exe",
    ["-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "120", ...args],
    { encoding: "utf8" }
  );
  return JSON.parse(raw);
}

async function fetchCloudMd5s() {
  const out = new Map(); // md5 -> songId
  let offset = 0;
  for (;;) {
    const page = curlJson([
      "-G", "-H", "Accept: application/json",
      "--data-urlencode", `cookie=${cookie}`,
      "--data-urlencode", "limit=200",
      "--data-urlencode", `offset=${offset}`,
      "--data-urlencode", `timestamp=${Date.now()}`,
      `${BASE}/user/cloud`,
    ]);
    const items = page.data ?? [];
    for (const item of items) {
      const key = item.privateCloud?.md5 ?? "";
      if (key) out.set(key, item.songId);
    }
    if (items.length < 200 || offset >= (page.count ?? 0)) break;
    offset += items.length;
  }
  return out;
}

const before = await fetchCloudMd5s();
console.log("cloud md5 entries:", before.size);
if (before.has(md5)) {
  console.log("!! 该文件的 MD5 已存在于云盘（songId=" + before.get(md5) + "），换一个文件测试");
  process.exit(1);
}

try {
  // 1) token
  const token = curlJson([
    "-X", "POST", "-H", "Accept: application/json",
    "--data-urlencode", `cookie=${cookie}`,
    "--data-urlencode", `md5=${md5}`,
    "--data-urlencode", `fileSize=${fileSize}`,
    "--data-urlencode", `filename=${fileName}`,
    "--data-urlencode", `timestamp=${TS}`,
    `${BASE}/cloud/upload/token`,
  ]);
  console.log("== token code:", token.code, "needUpload:", token.data?.needUpload);
  const d = token.data ?? {};
  if (!d.uploadUrl || !d.songId || !d.resourceId) {
    console.error("token 缺少字段:", Object.keys(d));
    process.exit(1);
  }

  // 2) 传输（上游参考实现为 POST + x-nos-token/Content-MD5；失败再试 PUT）。
  if (d.needUpload) {
    const base = ["--proxy", PROXY, "--connect-timeout", "15", "--max-time", "300",
      "-H", `x-nos-token: ${d.uploadToken}`,
      "-H", `Content-MD5: ${md5}`,
      "-H", "Content-Type: audio/mpeg",
      "--data-binary", `@${srcPath}`];
    let status = "000";
    try {
      status = execFileSync("curl.exe", ["-sS", "-o", "nul", "-w", "%{http_code}", "-X", "POST", ...base, d.uploadUrl], { encoding: "utf8" });
      console.log("== transfer POST status:", status);
    } catch {
      status = "000";
    }
    if (!status.startsWith("2")) {
      status = execFileSync("curl.exe", ["-sS", "-o", "nul", "-w", "%{http_code}", "-X", "PUT", ...base, d.uploadUrl], { encoding: "utf8" });
      console.log("== transfer PUT status:", status);
    }
    if (!status.startsWith("2")) {
      console.error("对象存储传输失败:", status);
      process.exit(1);
    }
  } else {
    console.log("== 服务器已有同 MD5（秒传），跳过传输");
  }

  // 3) complete
  const complete = curlJson([
    "-X", "POST", "-H", "Accept: application/json",
    "--data-urlencode", `cookie=${cookie}`,
    "--data-urlencode", `songId=${d.songId}`,
    "--data-urlencode", `resourceId=${d.resourceId}`,
    "--data-urlencode", `md5=${md5}`,
    "--data-urlencode", `filename=${fileName}`,
    "--data-urlencode", `timestamp=${TS}`,
    `${BASE}/cloud/upload/complete`,
  ]);
  console.log("== complete code:", complete.code, JSON.stringify(complete).slice(0, 300));
  if (complete.code !== 200) {
    console.error("complete 失败。NOS 对象可能残留（无害），云盘不应有条目。");
    process.exit(1);
  }

  // 4) 列表确认（按 MD5 定位新条目）
  await new Promise((r) => setTimeout(r, 2500));
  const after = await fetchCloudMd5s();
  const newSongId = after.get(md5);
  if (!newSongId) {
    console.log("== 列表中暂未看到新条目（网易导入可能异步），请稍后手动检查:", fileName);
    process.exit(0);
  }
  console.log("== 列表确认: 新条目 songId=", newSongId, "（上传前不存在）");

  // 5) 清理：从云盘删除测试条目
  const del = curlJson([
    "-X", "POST", "-H", "Accept: application/json",
    "--data-urlencode", `cookie=${cookie}`,
    "--data-urlencode", `id=${newSongId}`,
    "--data-urlencode", `timestamp=${TS}`,
    `${BASE}/user/cloud/del`,
  ]);
  console.log("== del code:", del.code, JSON.stringify(del).slice(0, 200));
  await new Promise((r) => setTimeout(r, 1500));
  const final = await fetchCloudMd5s();
  console.log(final.has(md5) ? "!! 测试文件仍在云盘，请手动删除" : "== 清理完成，云盘已无测试文件");
} finally {
  try { rmSync(join(root, "nul"), { force: true }); } catch {}
  void unlinkSync;
}
