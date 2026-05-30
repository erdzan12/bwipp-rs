// Track every BWIPP write to pixs at (row=3, col=21) for R7x43-M-HI,
// with a Node stack trace at each write. This will reveal which BWIPP
// step / line writes the mysterious 0 at that cell.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path.js");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Patch $put — the function BWIPP uses to write into arrays.
// $put is defined near the top of bwip-js, search for "function $put".
const ANCHOR = "function $put(d, k, v) {";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("$put anchor not found");

const PATCH = `function $put(d, k, v) {
  if (globalThis.__trackPixs && d === globalThis.__trackPixs && k === globalThis.__trackPixsIdx) {
    const tag = (globalThis.__trackWrites = globalThis.__trackWrites || []);
    const err = new Error();
    tag.push({ v, line: (err.stack || "").split("\\n").slice(1, 4).join(" | ") });
  }
`;
src = src.replace(ANCHOR, PATCH);

// Insert tracker right before the finder placement loop so cols/rows
// are populated.
const ALLOC = "$_.fpats = $get($_.qrcode_fpatmap, $_.format);";
const aIdx = src.indexOf(ALLOC);
if (aIdx < 0) throw new Error("fpatmap anchor not found");
const TRACK_SETUP = `
globalThis.__trackPixs = $_.pixs;
globalThis.__trackPixsIdx = 3 * $_.cols + 21; // (row=3, col=21)
`;
src = src.slice(0, aIdx) + TRACK_SETUP + src.slice(aIdx);

const patched = patchedPathFor(bp, "bwipjs-rmqr-track321.js");
fs.writeFileSync(patched, src);
const b = require(patched);
(async () => {
  try {
    await b.toBuffer({
      bcid: "rectangularmicroqrcode",
      text: "HI",
      version: "R7x43",
      eclevel: "M",
      fixedeclevel: true,
      includetext: false,
    });
    const w = globalThis.__trackWrites || [];
    console.log(`Writes to pixs[(row=3, col=21)] (linear idx ${globalThis.__trackPixsIdx}):`);
    for (let i = 0; i < w.length; i++) {
      console.log(`  ${i}: value=${w[i].v}`);
      console.log(`     at: ${w[i].line}`);
    }
  } catch (e) {
    console.error(e && e.message);
    process.exit(1);
  }
})();
