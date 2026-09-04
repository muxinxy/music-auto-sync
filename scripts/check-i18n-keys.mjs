import fs from "node:fs";

const zh = JSON.parse(fs.readFileSync("src/locales/zh-CN.json", "utf8"));
const en = JSON.parse(fs.readFileSync("src/locales/en.json", "utf8"));

function flat(o, prefix = "", out = {}) {
  for (const k of Object.keys(o)) {
    const v = o[k];
    if (v && typeof v === "object") flat(v, `${prefix}${k}.`, out);
    else out[`${prefix}${k}`] = v;
  }
  return out;
}
const zhFlat = flat(zh);
const enFlat = flat(en);

const missing = [];
for (const k of Object.keys(zhFlat)) if (!(k in enFlat)) missing.push(`en missing: ${k}`);
for (const k of Object.keys(enFlat)) if (!(k in zhFlat)) missing.push(`zh missing: ${k}`);
if (missing.length) console.log(missing.join("\n"));
else console.log("locale key parity OK:", Object.keys(zhFlat).length, "keys");

const files = [
  "src/App.tsx",
  "src/pages/Playlists.tsx",
  "src/pages/Sync.tsx",
  "src/pages/Login.tsx",
  "src/pages/Quarantine.tsx",
  "src/pages/Settings.tsx",
];
const re = /\bt\(\s*["'`]([A-Za-z0-9_.]+)["'`]/g;
const usedMissing = [];
for (const f of files) {
  const txt = fs.readFileSync(f, "utf8");
  let m;
  while ((m = re.exec(txt))) {
    const key = m[1];
    if (!(key in zhFlat)) usedMissing.push(`${f}: ${key}`);
  }
}
if (usedMissing.length) {
  console.log("MISSING literal t() keys:\n" + usedMissing.join("\n"));
  process.exit(1);
}
console.log("all literal t() keys exist");
