// Capture bwip-js's post-RS codeword stream (`rscws`) for DotCode —
// the data + check codewords + mask choice after Reed-Solomon over
// GF(113), right before the dot grid is filled.
//
// Usage: node tools/oracle-dotcode-rscws.js "1234"
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

const ANCHOR = "        if ($has($_.options, 'debugecc')) { //#34212";
if (!src.includes(ANCHOR)) throw new Error("anchor not found");
const fl = (v) =>
  `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;
const PATCH = `        {
          const err = new Error("bwipp.debugDotCodeRsCws");
          err.errorinfo = {
            rscws: ${fl("$_.rscws")},
            mask: $_.mask,
            nd: $_.nd,
            nw: $_.nw,
            rows: $_.rows,
            columns: $_.columns,
          };
          err.errorname = "bwipp.debugDotCodeRsCws";
          throw err;
        }
        if ($has($_.options, 'debugecc')) { //#34212`;
src = src.replace(ANCHOR, PATCH);
const p = patchedPathFor(bp, "bwipjs-dotcode-rscws-patched.js");
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
