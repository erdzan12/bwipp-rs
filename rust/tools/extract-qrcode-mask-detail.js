// Dump BWIPP's per-mask n1, n2, n3, n4 evalfull components for V1-L
// HELLO WORLD. Allows direct comparison against our evaluate_mask_full.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor at the n4 computation in evalfull (line 28330 area):
//   $_.n4 = (~~(($abs(...)/5))) * 10
const ANCHOR = "$_.n4 = (~~(";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("n4 anchor not found");
// Walk to end of the statement (next `;`).
const endIdx = src.indexOf(";", idx);
const insertPoint = endIdx + 1;
const PATCH = `
process.stdout.write("MASK_DETAIL m=" + $_.m + " n1=" + $_.n1 + " n2=" + $_.n2 + " n3=" + $_.n3 + " n4=" + $_.n4 + " total=" + ($_.n1 + $_.n2 + $_.n3 + $_.n4) + "\\n");
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);

// Also force evalfull to NOT early-exit so we get all 8 mask details.
// Find `if ($f($_.n1 + $_.n2 + $_.n3) >= $_.bestscore) { $_.earlyexit = true; break; }`
// and replace with a no-op.
const earlyExit = "if ($f($_.n1 + $_.n2 + $_.n3) >= $_.bestscore) { //#27880\n                $_.earlyexit = true; //#27880\n                break; //#27880\n            } //#27880";
src = src.replace(earlyExit, "/* early-exit disabled for mask detail dump */");

const patched = patchedPathFor(bp, "bwipjs-mask-detail.js");
fs.writeFileSync(patched, src);
const b = require(patched);

(async () => {
  try {
    await b.toBuffer({
      bcid: "qrcode",
      text: "HELLO WORLD",
      eclevel: "L",
      version: "1",
      fixedeclevel: true,
      includetext: false,
    });
  } catch (e) {
    process.stderr.write("error: " + (e && e.message) + "\n");
  }
})();
