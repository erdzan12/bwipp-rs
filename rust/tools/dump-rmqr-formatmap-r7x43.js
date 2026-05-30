// Dump the actual rMQR formatfimmap positions for R7x43 — to verify
// my RMQR_FORMATFIMMAP_CLUSTERS covers everything BWIPP reserves.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path.js");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

const ANCHOR = "$_.formatmap = $a(); //#27692";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("anchor not found");

const PATCH = `
{
  const err = new Error("dump-rmqr-formatmap");
  const fm = $_.formatmap;
  const positions = [];
  for (let i = 0; i < fm.length; i++) {
    const pair = fm.b[fm.o + i];
    const p0 = pair.b[pair.o + 0];
    const p1 = pair.b[pair.o + 1];
    positions.push({
      i,
      tl: [p0.b[p0.o + 0], p0.b[p0.o + 1]],
      dup: [p1.b[p1.o + 0], p1.b[p1.o + 1]],
    });
  }
  err.errorinfo = { rows: $_.rows, cols: $_.cols, positions };
  throw err;
}
`;
const insertAt = idx + ANCHOR.length;
src = src.slice(0, insertAt) + PATCH + src.slice(insertAt);

const patched = patchedPathFor(bp, "bwipjs-rmqr-formatmap-r7x43.js");
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
