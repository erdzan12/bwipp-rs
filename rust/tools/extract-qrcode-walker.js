// Trace BWIPP's qrcode walker for V1-L "HELLO WORLD" with mask 0 forced.
// Dumps every (num, posx, posy, pos, bit) the walker visits so we can
// diff against our Rust walker output and find the divergence point.
//
// Run from `node-sidecar/`:
//   node ../rust/tools/extract-qrcode-walker.js > /tmp/bwipp_walker_v1_l.txt
//
// The cws is captured separately via extract-qrcode-codewords.js (Stage 4b).

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Walker loop body starts with `for (;;) {` near line 28131. We splice a
// console.log right before the `$_.num = $_.num + 1` increment so we
// capture every placed bit.
const ANCHOR = "$_.num = $_.num + 1; //#27774";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("walker num++ anchor not found");
const PATCH = `
process.stdout.write("WALK num=" + $_.num + " posx=" + $_.posx + " posy=" + $_.posy + " pos=" + $_.pos + " bit=" + $_.bit + "\\n");
`;
src = src.slice(0, idx) + PATCH + src.slice(idx);

const patched = patchedPathFor(bp, "bwipjs-walker-trace.js");
fs.writeFileSync(patched, src);
const b = require(patched);

(async () => {
  try {
    // Force mask=0 so we don't depend on BWIPP's mask scoring.
    await b.toBuffer({
      bcid: "qrcode",
      text: "HELLO WORLD",
      eclevel: "L",
      version: "1",
      fixedeclevel: true,
      mask: "1", // BWIPP mask option is 1-indexed: 1 = mask 0
      includetext: false,
    });
  } catch (e) {
    process.stderr.write("error: " + (e && e.message) + "\n");
    process.exit(1);
  }
})();
