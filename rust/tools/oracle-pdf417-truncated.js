// Capture the rendered pixs for a PDF417 *compact* (truncated)
// symbol from bwip-js. The compact branch (`compact=true`) drops the
// right row-indicator column and replaces the 18-bar stop pattern
// with a single 1-bar stop.
//
// Usage: node tools/oracle-pdf417-truncated.js "PDF417" [eclevel]
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor: right after the row-fill loop closes (line 23320 in the
// 2026-03-31 snapshot) and before bwip-js packages pixs for
// renmatrix.
const ANCHOR = "    var _GV = $_.pixs; //#23327";
if (!src.includes(ANCHOR)) throw new Error("anchor not found");
const fl = (v) =>
  `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;
const PATCH = `    var _GV = $_.pixs; //#23327
    {
      const err = new Error("bwipp.debugPDF417Compact");
      err.errorinfo = {
        pixs: ${fl("$_.pixs")},
        cws: ${fl("$_.cws")},
        rwid: $_.rwid,
        r: $_.r,
        c: $_.c,
        eclevel: $_.eclevel,
      };
      err.errorname = "bwipp.debugPDF417Compact";
      throw err;
    }`;
src = src.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bp, "bwipjs-pdf417compact-patched.js");
fs.writeFileSync(patchedPath, src);

const bwipjs = require(patchedPath);
const text = process.argv[2] || "PDF417";
const eclevel = parseInt(process.argv[3] || "2", 10);

(async () => {
  try {
    await bwipjs.toBuffer({
      bcid: "pdf417compact",
      text,
      includetext: false,
      eclevel,
    });
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e.message);
      process.exit(3);
    }
    console.log(JSON.stringify({ text, eclevel, ...e.errorinfo }));
  }
})();
