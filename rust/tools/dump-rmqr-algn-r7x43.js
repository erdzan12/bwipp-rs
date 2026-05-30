// Dump pixs state immediately after BWIPP's alignment-pattern
// placement for R7x43-M-HI. Lets us compare cell-by-cell what cells
// BWIPP considers "set" vs UNSET at walker time.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path.js");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor: right after the formatmap write loop, before the walker
// init `var _VX = $ne($_.format, "rmqr") ? 1 : 2;`.
const ANCHOR = "var _VX = $ne($_.format, \"rmqr\") ? 1 : 2;";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("anchor not found");

const PATCH = `
{
  const err = new Error("dump-rmqr-pixs-pre-walker");
  const flatten = (v) => {
    if (v && v.b && v.o !== undefined) {
      const out = [];
      for (let i = 0; i < v.length; i++) out.push(v.b[v.o + i]);
      return out;
    }
    return Array.from(v);
  };
  err.errorinfo = { rows: $_.rows, cols: $_.cols, pixs: flatten($_.pixs) };
  throw err;
}
`;
src = src.slice(0, idx) + PATCH + src.slice(idx);

const patched = patchedPathFor(bp, "bwipjs-rmqr-pixs-pre-walker.js");
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
  } catch (e) {
    if (e.errorinfo) console.log(JSON.stringify(e.errorinfo, null, 2));
    else { console.error(e && e.message); process.exit(1); }
  }
})();
