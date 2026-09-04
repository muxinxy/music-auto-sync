import { execFileSync } from "node:child_process";

const API = "https://api.github.com/repos/NeteaseCloudMusicApiEnhanced/api-enhanced/contents/module?per_page=100&page=";
const names = [];
for (let page = 1; page <= 4; page++) {
  const raw = execFileSync("curl.exe", [
    "-sS", "--proxy", "http://127.0.0.1:7897", "--connect-timeout", "15", "--max-time", "40",
    API + page,
  ], { encoding: "utf8" });
  const json = JSON.parse(raw);
  if (!Array.isArray(json)) { console.log("page", page, "非数组:", raw.slice(0, 120)); break; }
  if (json.length === 0) { console.log("page", page, "空"); break; }
  names.push(...json.map((entry) => entry.name));
}
console.log("总模块数:", names.length);
const relevant = names.filter((name) => /download|song_url|player|enhance|\.url/i.test(name));
console.log("下载/地址相关(" + relevant.length + "):");
console.log(relevant.sort().join("\n") || "(空)");