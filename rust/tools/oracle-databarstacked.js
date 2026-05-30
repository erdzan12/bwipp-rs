// Capture bwip-js's intermediate pixs + rowmult for DataBar Stacked
// and DataBar Stacked Omnidirectional, right before they hand off to
// bwipp_renmatrix.
//
// Usage:
//   node tools/oracle-databarstacked.js stacked     "(01)24012345678905"
//   node tools/oracle-databarstacked.js stackedomni "(01)24012345678905"
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor: the renmatrix dispatch inside bwipp_databaromni for stacked
// formats. Replace just the `if ($_._render) { ... }` so we throw with
// the captured arguments before any pixel-level rendering happens.
const ANCHOR = "        if ($_._render) { //#12070\n            bwipp_renmatrix(); //#12070\n        } //#12070";
if (!src.includes(ANCHOR)) throw new Error("anchor not found");
const fl = (v) =>
  `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;
const PATCH = `        {
          const err = new Error("bwipp.debugDataBarStacked");
          err.errorinfo = {
            pixs: ${fl("$_.pixs")},
            rowmult: ${fl("$_.rowmult")},
            pixy: $_.pixy,
            format: $_.format,
          };
          err.errorname = "bwipp.debugDataBarStacked";
          throw err;
        }`;
src = src.replace(ANCHOR, PATCH);
const patched = patchedPathFor(bp, "bwipjs-databarstacked-patched.js");
fs.writeFileSync(patched, src);

const bwipjs = require(patched);
const bcid = process.argv[2] || "databarstacked";
const text = process.argv[3] || "(01)24012345678905";

(async () => {
  try {
    await bwipjs.toBuffer({ bcid, text, includetext: false });
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e.message);
      process.exit(3);
    }
    console.log(JSON.stringify({ bcid, text, ...e.errorinfo }));
  }
})();
