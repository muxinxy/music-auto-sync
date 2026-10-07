import fs from "node:fs";

const f = "D:/Drive/Music/网易云歌单/知名歌手/unknown_artist - 一路向北.MP3";
const b = fs.readFileSync(f);
const declared = b[6] * 2 ** 24 + b[7] * 2 ** 16 + b[8] * 2 ** 8 + b[9] + 10;
console.log("declared tag end:", declared, "file size:", b.length);

// QMCI 块开头
console.log("around QMCI (85738):", b.slice(85730, 85850).toString("hex"));

// 在声明标签结束之后找 mp3 帧同步（0xFFEx，且帧头字段合法）
const isSync = (i) =>
  b[i] === 0xff && (b[i + 1] & 0xe0) === 0xe0 && (b[i + 1] & 0x06) !== 0 && (b[i + 2] & 0xf0) !== 0xf0 && (b[i + 2] & 0xf0) !== 0;
const candidates = [];
for (let i = declared; i < Math.min(declared + 65536, b.length - 4); i++) {
  if (isSync(i)) candidates.push(i);
}
console.log("first syncs after declared end:", candidates.slice(0, 10));

// 全文件 mp3 同步密度采样：每 1MB 数一下同步个数
for (let mb = 0; mb * 1024 * 1024 < b.length; mb++) {
  const start = mb * 1024 * 1024;
  let count = 0;
  for (let i = start; i < Math.min(start + 1024 * 1024, b.length - 4); i++) if (isSync(i)) count++;
  console.log(`MB${mb}: syncs=${count}`);
}

// 看看 335210 前后 64 字节
console.log("at declared end:", b.slice(declared - 16, declared + 48).toString("hex"));
