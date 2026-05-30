// Same as extract-qrcode-mask-detail.js but for V1-Q HELLO.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

const ANCHOR = "$_.n4 = (~~(";
const idx = src.indexOf(ANCHOR);
const endIdx = src.indexOf(";", idx);
const insertPoint = endIdx + 1;
const PATCH = `
process.stdout.write("MASK_DETAIL m=" + $_.m + " n1=" + $_.n1 + " n2=" + $_.n2 + " n3=" + $_.n3 + " n4=" + $_.n4 + " total=" + ($_.n1 + $_.n2 + $_.n3 + $_.n4) + "\\n");
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);
const earlyExit = "if ($f($_.n1 + $_.n2 + $_.n3) >= $_.bestscore) { //#27880\n                $_.earlyexit = true; //#27880\n                break; //#27880\n            } //#27880";
src = src.replace(earlyExit, "/* early-exit disabled */");

const patched = patchedPathFor(bp, "bwipjs-mask-detail-v1q.js");
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
