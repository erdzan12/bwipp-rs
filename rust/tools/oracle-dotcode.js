// Capture bwip-js's intermediate `cws` array for DotCode — the
// codeword stream produced by encA/encB/encC + RS-GF(113) ECC,
// right before bwipp_dotcode hands off to the renderer.
//
// Usage: node tools/oracle-dotcode.js "1234"
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

const ANCHOR = "    if ($has($_.options, 'debugcws')) { //#33962";
if (!src.includes(ANCHOR)) throw new Error("anchor not found");
const fl = (v) =>
  `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;
const PATCH = `    {
      const err = new Error("bwipp.debugDotCode");
      err.errorinfo = {
        cws: ${fl("$_.cws")},
        mode: $_.mode,
        nd: $_.nd,
        nw: $_.nw,
        rows: $_.rows,
        columns: $_.columns,
      };
      err.errorname = "bwipp.debugDotCode";
      throw err;
    }
    if ($has($_.options, 'debugcws')) { //#33962`;
src = src.replace(ANCHOR, PATCH);
const p = patchedPathFor(bp, "bwipjs-dotcode-patched.js");
fs.writeFileSync(p, src);

const b = require(p);
const text = process.argv[2] || "1234";

(async () => {
  try {
    await b.toBuffer({ bcid: "dotcode", text, includetext: false });
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e && e.message);
      process.exit(3);
    }
    console.log(JSON.stringify({ text, ...e.errorinfo }));
  }
})();
