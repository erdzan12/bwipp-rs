// Dump BWIPP's fmtvalsrmqr1 + fmtvalsrmqr2 tables (64 entries each).

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

const ANCHOR = "$_.fmtvalsrmqr2 = $_.fmtvalsrmqr2;";
let idx = src.indexOf(ANCHOR);
if (idx < 0) {
  // Try alt anchor.
  idx = src.indexOf("$_.fmtvalsrmqr2 = $a(64);");
  if (idx < 0) throw new Error("fmtvalsrmqr2 anchor not found");
  // Inject after the build loop ends.
}
// Inject right before "if ($eq($_.format, " (which is downstream and out of the init).
// Look for next checkpoint after fmtvalsrmqr is initialized.
const POST_ANCHOR = "$_.vervals = $_.vervals;";
const postIdx = src.indexOf(POST_ANCHOR);
if (postIdx < 0) throw new Error("vervals anchor not found");
const insertPoint = postIdx + POST_ANCHOR.length;
const PATCH = `
{
  const err = new Error("dump-rmqr-fmtvals");
  const flatten = (v) => {
    if (v && v.b && v.o !== undefined) {
      const out = [];
      for (let i = 0; i < v.length; i++) out.push(v.b[v.o + i]);
      return out;
    }
    return v;
  };
  err.errorinfo = {
    fmtvalsrmqr1: flatten($_.fmtvalsrmqr1),
    fmtvalsrmqr2: flatten($_.fmtvalsrmqr2),
  };
  throw err;
}
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);
const patched = patchedPathFor(bp, "bwipjs-rmqr-fmtvals.js");
fs.writeFileSync(patched, src);
const b = require(patched);
(async () => {
  try {
    await b.toBuffer({ bcid: "qrcode", text: "X", includetext: false });
  } catch (e) {
    if (e.errorinfo) console.log(JSON.stringify(e.errorinfo, null, 2));
    else { console.error(e && e.message); process.exit(1); }
  }
})();
