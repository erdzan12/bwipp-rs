// Dump BWIPP's per-row and per-col (n1, n3) contributions and per-row
// N2 contributions for V1-Q HELLO mask 4. Comparing against our same
// dump pinpoints which row/col differs from bwip-js.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Dump per-column n1+n3: after the first evalfulln1n3() call in evalfull
// (line 28246-28251 area). At that point we've just computed column i.
const COL_ANCHOR = "$_.n1 = $f(_XG + $_.n1) //#27851";
const colIdx = src.indexOf(COL_ANCHOR);
if (colIdx < 0) throw new Error("col anchor not found");
const colInsertPoint = colIdx + COL_ANCHOR.length;
const COL_PATCH = `
process.stdout.write("COL i=" + $_.i + " m=" + $_.m + " n3_added=" + _XE + " n1_added=" + _XG + "\\n");
`;
src = src.slice(0, colInsertPoint) + COL_PATCH + src.slice(colInsertPoint);

// Dump per-row n1+n3: after the second evalfulln1n3() call (line
// 28274-28278).
const ROW_ANCHOR = "$_.n1 = $f(_Xa + $_.n1) //#27862";
const rowIdx = src.indexOf(ROW_ANCHOR);
if (rowIdx < 0) throw new Error("row anchor not found");
const rowInsertPoint = rowIdx + ROW_ANCHOR.length;
const ROW_PATCH = `
process.stdout.write("ROW i=" + $_.i + " m=" + $_.m + " n3_added=" + _XY + " n1_added=" + _Xa + "\\n");
`;
src = src.slice(0, rowInsertPoint) + ROW_PATCH + rowInsertPoint;

// Hack: my src.slice(insertPoint) became insertPoint accidentally. Fix.
src = fs.readFileSync(bp, "utf8");

// Redo properly.
{
  const colIdx2 = src.indexOf(COL_ANCHOR);
  src = src.slice(0, colIdx2 + COL_ANCHOR.length) + COL_PATCH + src.slice(colIdx2 + COL_ANCHOR.length);
  const rowIdx2 = src.indexOf(ROW_ANCHOR);
  src = src.slice(0, rowIdx2 + ROW_ANCHOR.length) + ROW_PATCH + src.slice(rowIdx2 + ROW_ANCHOR.length);
}

// Disable evalfull early-exit so we see all masks.
const earlyExit = "if ($f($_.n1 + $_.n2 + $_.n3) >= $_.bestscore) { //#27880\n                $_.earlyexit = true; //#27880\n                break; //#27880\n            } //#27880";
src = src.replace(earlyExit, "/* early-exit disabled for per-line dump */");

const patched = patchedPathFor(bp, "bwipjs-per-line.js");
fs.writeFileSync(patched, src);
const b = require(patched);

(async () => {
  try {
    await b.toBuffer({
      bcid: "qrcode",
      text: "HELLO",
      eclevel: "Q",
      version: "1",
      fixedeclevel: true,
      includetext: false,
    });
  } catch (e) {
    process.stderr.write("error: " + (e && e.message) + "\n");
  }
})();
