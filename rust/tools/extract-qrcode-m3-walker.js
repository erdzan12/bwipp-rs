// Dump BWIPP's walker trace for M3-L HELLO12.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

const ANCHOR = "$_.num = $_.num + 1; //#27774";
const idx = src.indexOf(ANCHOR);
const PATCH = `
process.stdout.write("WALK num=" + $_.num + " posx=" + $_.posx + " posy=" + $_.posy + " pos=" + $_.pos + " bit=" + $_.bit + "\\n");
`;
src = src.slice(0, idx) + PATCH + src.slice(idx);

const patched = patchedPathFor(bp, "bwipjs-m3-walker.js");
fs.writeFileSync(patched, src);
const b = require(patched);

(async () => {
  try {
    await b.toBuffer({
      bcid: "microqrcode",
      text: "HELLO12",
      eclevel: "L",
      version: "M3",
      fixedeclevel: true,
      includetext: false,
    });
  } catch (e) {
    process.stderr.write("error: " + (e && e.message) + "\n");
    process.exit(1);
  }
})();
