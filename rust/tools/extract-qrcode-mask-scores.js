// Dump BWIPP's per-mask evalfull score for V1-L HELLO WORLD. Patches
// the mask-selection loop to log each (mask, score) tuple.
//
// Run from `node-sidecar/`:
//   node ../rust/tools/extract-qrcode-mask-scores.js > /tmp/bwipp_mask_scores.txt

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor right after `$_.score = $k[--$j];` inside the mask loop.
// Looking at bwip-js around line 28385 — `$_.evalfull()` returns the
// score which gets assigned via $_.score (or similar). The mask scoring
// loop pushes 'score' as a key onto the stack, then the score value.
// We splice a log line right after the bestscore comparison.
const ANCHOR = "$_.evalfull(); //#27926";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("evalfull anchor not found");
const insertPoint = idx + ANCHOR.length;
// After the evalfull call, the stack has [..., 'score', score_value]. We
// peek the score and log it with the current mask m.
const PATCH = `
process.stdout.write("MASK m=" + $_.m + " maskbit=" + $_.maskbit + " score=" + $k[$j - 1] + "\\n");
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);
const patched = patchedPathFor(bp, "bwipjs-mask-scores.js");
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
