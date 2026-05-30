// Capture bwip-js raw().pixs for QR Code with a short alphanumeric input.
// The qrcode-crate substrate generally agrees with BWIPP on simple
// inputs (no mask-score tie); pinning a single byte-for-byte golden
// for "HELLO" gives us a regression net.
//
// Run:
//   $ node rust/tools/oracle-qrcode.js

const { findBwipJs } = require("./_bwip_js_path.js");
const bwipp = require(findBwipJs(__dirname));

const cases = [
  { id: "qrcode_HELLO", sym: "qrcode", data: "HELLO", opts: { eclevel: "M" } },
];

for (const { id, sym, data, opts } of cases) {
  const raw = bwipp.raw(sym, data, opts);
  const pixs = raw[0].pixs;
  const w = raw[0].pixx;
  const h = raw[0].pixy;
  process.stdout.write(`// ${id}: ${w}x${h} (pixs len ${pixs.length})\n`);
  for (let y = 0; y < h; y++) {
    let row = "";
    for (let x = 0; x < w; x++) {
      row += pixs[y * w + x] ? "1" : "0";
    }
    process.stdout.write(`// ${row}\n`);
  }
  process.stdout.write("// flat array:\n");
  const flat = Array.from(pixs).map(p => p ? "1" : "0").join(", ");
  process.stdout.write(`// [${flat}]\n`);
}
