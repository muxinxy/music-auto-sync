import fs from "node:fs";

const files = [
  "D:/Drive/Music/网易云歌单/知名歌手/unknown_artist - 一路向北.MP3",
  "D:/Drive/Music/网易云歌单/知名歌手/unknown_artist - 半岛铁盒.MP3",
  "D:/Drive/Music/网易云歌单/知名歌手/unknown_artist - 发如雪.MP3",
];

for (const f of files) {
  const b = fs.readFileSync(f);
  const syncsafe = ((b[6] & 0x7f) << 21) | ((b[7] & 0x7f) << 14) | ((b[8] & 0x7f) << 7) | (b[9] & 0x7f);
  const plain = b[6] * 2 ** 24 + b[7] * 2 ** 16 + b[8] * 2 ** 8 + b[9];
  console.log("\n==", f.split("/").pop(), "size=", b.length, "syncsafe+10=", syncsafe + 10, "plain+10=", plain + 10);
  for (const s of [syncsafe + 10, plain + 10]) {
    if (s + 8 < b.length) {
      console.log("  offset", s, JSON.stringify(b.slice(s, s + 12).toString("latin1")), b.slice(s, s + 4).toString("hex"));
    } else {
      console.log("  offset", s, "out of range");
    }
  }
  // 找真正的音频头：第一个 0xFF Ex
  const idx = b.findIndex((v, i) => i > 10 && v === 0xff && (b[i + 1] & 0xe0) === 0xe0);
  console.log("  first 0xFFEx at", idx, JSON.stringify(b.slice(idx, idx + 12).toString("latin1")));
  // ID3v2.3 每帧头 4 字节 ID + 4 字节 size（大端非 syncsafe）+2 flag；扫描标签里的帧 ID 看哪帧异常
  let off = 10;
  const end = Math.min(plain + 10, b.length);
  const frames = [];
  while (off + 10 <= end) {
    const id = b.slice(off, off + 4).toString("latin1");
    if (!/^[A-Z0-9]{4}$/.test(id)) {
      frames.push(`??@${off}:${JSON.stringify(id)}`);
      break;
    }
    const size = b[off + 4] * 2 ** 24 + b[off + 5] * 2 ** 16 + b[off + 6] * 2 ** 8 + b[off + 7];
    frames.push(`${id}@${off}(${size})`);
    if (size <= 0) break;
    off += 10 + size;
  }
  console.log("  frames:", frames.slice(0, 30).join(" "));
  console.log("  frame walk ended at", off, "tag end (plain+10) =", plain + 10);
}
