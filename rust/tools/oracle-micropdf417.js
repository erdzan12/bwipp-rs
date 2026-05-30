// Capture MicroPDF417 internal state from bwip-js:
//   - datcws: the per-row data codewords after compaction
//   - cws: the full padded + RS-checked codeword array
//   - c, r, k, rapl, rapc, rapr: the metrics row bwip-js selected
//   - pixs: row-major bit array of the fully rendered symbol (rwid * r)
//   - rwid: row width in bars (38, 55, 82 (or 72 for CCA), or 99)
//
// Usage:
//   node tools/oracle-micropdf417.js "PDF417"
//   node tools/oracle-micropdf417.js "1234567890" 30 4   # cols=30, rows=4
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let src = fs.readFileSync(bundledPath, "utf8");

// Anchor: right after the row-fill loop populates $_.pixs and before
// renmatrix consumes it.
const ANCHOR = "    var _JL = $_.pixs; //#24734";
if (!src.includes(ANCHOR)) throw new Error("anchor not found");
const fl = (v) =>
  `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;
const PATCH = `    var _JL = $_.pixs; //#24734
    {
      const err = new Error("bwipp.debugMicroPDF417");
      err.errorinfo = {
        datcws: ${fl("$_.datcws")},
        cws: ${fl("$_.cws")},
        c: $_.c, r: $_.r, k: $_.k,
        rapl: $_.rapl, rapc: $_.rapc, rapr: $_.rapr,
        n: $_.n, m: $_.m,
        rwid: $_.rwid,
        pixs: ${fl("$_.pixs")},
      };
      err.errorname = "bwipp.debugMicroPDF417";
      throw err;
    }`;
src = src.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-micropdf417-patched.js");
fs.writeFileSync(patchedPath, src);

const bwipjs = require(patchedPath);
const text = process.argv[2] || "PDF417";
const opts = { bcid: "micropdf417", text, includetext: false };
if (process.argv[3]) opts.columns = parseInt(process.argv[3], 10);
if (process.argv[4]) opts.rows = parseInt(process.argv[4], 10);

(async () => {
  try {
    await bwipjs.toBuffer(opts);
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e.message);
      process.exit(3);
    }
    console.log(JSON.stringify({ text, ...e.errorinfo }));
  }
})();
