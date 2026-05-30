// Dump BWIPP's masksym col 8 cells for V1-Q HELLO mask 4 (i.e. the
// post-mask data values BWIPP feeds into evalfull).

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Patch evalfull's body: peek at sym (the masksym arg) before the n1/n3
// loops run. We've already established that BWIPP's evalfull pops `sym`
// from the stack as the first thing it does.
const ANCHOR = "$_.sym = _Wo; //#27833";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("sym anchor not found");
const insertPoint = idx + ANCHOR.length;
const PATCH = `
if ($_.m === 4) {
  const col8 = [];
  for (let r = 0; r < $_.rows; r++) col8.push($get($_.sym, 8 + r * $_.cols));
  process.stdout.write("BWIPP col 8 mask 4: [" + col8.join(", ") + "]\\n");
}
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);

const earlyExit = "if ($f($_.n1 + $_.n2 + $_.n3) >= $_.bestscore) { //#27880\n                $_.earlyexit = true; //#27880\n                break; //#27880\n            } //#27880";
src = src.replace(earlyExit, "/* early-exit disabled */");

const patched = patchedPathFor(bp, "bwipjs-col8-v1q.js");
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
