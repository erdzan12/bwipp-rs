// Dump BWIPP's $_.formatmap (the materialized reservation positions
// before they get written) for V1 with detailed grouping. Captures the
// EXACT cells BWIPP marks as "format-info" so we can confirm what set
// (8, 13) in the pre-walker pixs dump.
//
// Run from `node-sidecar/`:
//   node ../rust/tools/extract-qrcode-formatmap.js

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor right after `$_.formatmap = $a()` is built (line ~28040).
const ANCHOR = "$_.formatmap = $a();";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("formatmap anchor not found");
const insertPoint = idx + ANCHOR.length;
const PATCH = `
{
  const err = new Error("bwipp.dumpFormatmap");
  const flatten = (v) => {
    if (v && v.b && v.o !== undefined) {
      const out = [];
      for (let i = 0; i < v.length; i++) out.push(flatten(v.b[v.o + i]));
      return out;
    }
    if (Array.isArray(v)) return v.map(flatten);
    return v;
  };
  err.errorinfo = { formatmap: flatten($_.formatmap) };
  throw err;
}
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);
const patched = patchedPathFor(bp, "bwipjs-formatmap.js");
fs.writeFileSync(patched, src);
const b = require(patched);

(async () => {
  try {
    await b.toBuffer({
      bcid: "qrcode",
      text: "HELLO WORLD",
      eclevel: "L",
      version: "1",
      fixedeclevel: true,
      mask: "1",
      includetext: false,
    });
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e && e.message);
      process.exit(3);
    }
    console.log(JSON.stringify(e.errorinfo.formatmap, null, 2));
  }
})();
