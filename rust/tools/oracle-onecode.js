const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bundledPath = findBwipJs(__dirname);
let src = fs.readFileSync(bundledPath, "utf8");
const ANCHOR = "if ($has($_.options, 'debugcws')) { //#15856";
if (!src.includes(ANCHOR)) throw new Error("anchor not found");
src = src.replace(
  ANCHOR,
  `{
    const err = new Error("dbg");
    const fl = (v) => Array.from(v.b ? v.b.slice(v.o, v.o + v.length) : v);
    err.errorinfo = {
      barlen: $_.barlen,
      binval: fl($_.binval),
      bytes: fl($_.bytes),
      fcs: $_.fcs,
      codewords: fl($_.codewords),
    };
    throw err;
  }
  if ($has($_.options, 'debugcws')) { //#15856`
);
const patched = patchedPathFor(bundledPath, "bwipjs-onecode-patched.js");
fs.writeFileSync(patched, src);
const b = require(patched);
const text = process.argv[2] || "01234567094987654321";
b.toBuffer({ bcid: "onecode", text, includetext: false }).then(()=>{}).catch(e => {
  if (e.errorinfo) console.log(JSON.stringify({text, ...e.errorinfo}));
  else { console.error("no info:", e.message); process.exit(1); }
});
