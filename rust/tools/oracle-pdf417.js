const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");
const A = "if ($has($_.options, 'debugecc')) { //#23267";
const fl = (v) => `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;
const PATCH = `{
    const err = new Error("dbg");
    err.errorinfo = {
      n: $_.n, k: $_.k, eclevel: $_.eclevel,
      coeffs: ${fl("$_.coeffs")},
      datcws: ${fl("$_.datcws")},
      cws: ${fl("$_.cws")},
    };
    throw err;
  }
  ${A}`;
if (!src.includes(A)) throw new Error("anchor");
src = src.replace(A, PATCH);
const p = patchedPathFor(bp, "bwipjs-pdf417-patched.js");
fs.writeFileSync(p, src);
const b = require(p);
const text = process.argv[2] || "PDF417";
const ec = parseInt(process.argv[3] || "2", 10);
b.toBuffer({ bcid: "pdf417", text, eclevel: ec }).then(()=>{}).catch(e => {
  if (e.errorinfo) console.log(JSON.stringify({text, ec, ...e.errorinfo}));
  else { console.error("no info:", e.message); process.exit(1); }
});
