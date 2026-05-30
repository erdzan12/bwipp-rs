// Dump BWIPP's rMQR formatfimmap as an 18-cluster closure formula
// table: for each cluster, the constants used by func1 (TL position)
// and func2 (Dup position formula in terms of rows/cols).
//
// Empirically derives the offsets by running each closure pair against
// (rows, cols) = (1000, 1000) and observing the output.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

const ANCHOR = "$_.qrcode_formatfimmap = _7E;";
const idx = src.indexOf(ANCHOR);
const insertPoint = idx + ANCHOR.length;
const PATCH = `
{
  const err = new Error("dump-rmqr-clusters");
  // Run each rMQR closure pair against two distinct (rows, cols) inputs
  // and derive the BL/Dup formula coefficients.
  const fm = $_.qrcode_formatfimmap;
  const rmqr = fm.get("rmqr");
  const clusters = [];
  for (let i = 0; i < rmqr.length; i++) {
    const pair = rmqr.b[rmqr.o + i];
    const f1 = pair.b[pair.o + 0];
    const f2 = pair.b[pair.o + 1];
    // Run func1 with (rows=1000, cols=2000) — it ignores inputs and
    // pushes constants. The output is [tl_col, tl_row].
    $k[$j++] = 1000;
    $k[$j++] = 2000;
    f1();
    const tl_row = $k[--$j];  // top of stack
    const tl_col = $k[--$j];  // second
    // Run func2 twice to extract dx, dy: once with (1000, 2000) and
    // once with (500, 1000). Compare outputs.
    $k[$j++] = 1000;
    $k[$j++] = 2000;
    f2();
    const a_row = $k[--$j];
    const a_col = $k[--$j];
    $k[$j++] = 500;
    $k[$j++] = 1000;
    f2();
    const b_row = $k[--$j];
    const b_col = $k[--$j];
    // a_col = cols_a - dx → dx = cols_a - a_col = 2000 - a_col.
    // similarly dy = rows_a - a_row = 1000 - a_row.
    const dx = 2000 - a_col;
    const dy = 1000 - a_row;
    // Verify with (rows=500, cols=1000): expected (1000-dx, 500-dy).
    const dx2 = 1000 - b_col;
    const dy2 = 500 - b_row;
    const ok = dx === dx2 && dy === dy2;
    clusters.push({ tl_col, tl_row, dup_dx: dx, dup_dy: dy, ok });
  }
  err.errorinfo = { clusters };
  throw err;
}
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);
const patched = patchedPathFor(bp, "bwipjs-rmqr-clusters.js");
fs.writeFileSync(patched, src);
const b = require(patched);
(async () => {
  try {
    await b.toBuffer({ bcid: "qrcode", text: "X", includetext: false });
  } catch (e) {
    if (e.errorinfo) console.log(JSON.stringify(e.errorinfo.clusters, null, 2));
    else { console.error(e && e.message); process.exit(1); }
  }
})();
