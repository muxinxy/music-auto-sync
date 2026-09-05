// 调研：账号统计接口真实返回结构（/user/subcount、/user/detail、/vip/info/v2）。
// 仅使用本地配置文件中的登录凭据，不输出 cookie 明文。
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
const uid = config.cookieUser?.userId;

function curlJson(pathWithQuery) {
  const raw = execFileSync("curl.exe", [
    "-sS", "--proxy", PROXY, "--connect-timeout", "15", "--max-time", "30",
    "-G", "-H", "Accept: application/json",
    "--data-urlencode", `cookie=${cookie}`,
    `${BASE}${pathWithQuery}`,
  ], { encoding: "utf8" });
  return JSON.parse(raw);
}

const TS = Date.now();

try {
  const sub = curlJson(`/user/subcount?timestamp=${TS}`);
  console.log("== /user/subcount code:", sub.code);
  console.log("data keys:", sub.data ? Object.keys(sub.data) : "(no data)");
  console.log("data:", JSON.stringify(sub.data ?? sub, null, 2).slice(0, 1200));
} catch (e) {
  console.log("/user/subcount FAILED:", String(e));
}

if (uid) {
  try {
    const det = curlJson(`/user/detail?uid=${uid}&timestamp=${TS}`);
    const p = det.profile;
    console.log("\n== /user/detail code:", det.code);
    if (p) {
      console.log("profile keys sample:", ["nickname", "avatarUrl", "level", "userId", "follows", "followeds", "eventCount", "createTime"].map((k) => `${k}=${p[k] ?? "(none)"}`).join(" | "));
    } else {
      console.log("profile:", JSON.stringify(det).slice(0, 500));
    }
  } catch (e) {
    console.log("/user/detail FAILED:", String(e));
  }
}

try {
  const vip = curlJson(`/vip/info/v2?timestamp=${TS}`);
  console.log("\n== /vip/info/v2 code:", vip.code);
  const d = vip.data;
  console.log("data keys:", d ? Object.keys(d) : "(no data)");
  console.log("redVipLevel:", d?.redVipLevel ?? d?.redVipCount ?? d?.vipCode ?? "(none)");
} catch (e) {
  console.log("/vip/info/v2 FAILED:", String(e));
}

try {
  const like = curlJson(`/likelist?uid=${uid}&timestamp=${TS}`);
  console.log("\n== /likelist code:", like.code, "ids count:", (like.ids ?? []).length);
} catch (e) {
  console.log("/likelist FAILED:", String(e));
}
