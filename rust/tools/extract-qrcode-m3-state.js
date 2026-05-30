// Dump BWIPP's intermediate state for M3-L HELLO12.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor right BEFORE the padding loop (line 27701).
const ANCHOR = "var _OM = $_.lc4b ? 5 : 1;";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("anchor not found");
const insertPoint = idx + ANCHOR.length;
const PATCH = `
{
  process.stdout.write("dcws=" + $_.dcws + " ncws=" + $_.ncws + " dmod=" + $_.dmod + " lc4b=" + $_.lc4b + " term.length=" + ($_.term ? $_.term.length : "?") + "\\n");
  if ($_.msgbits && $_.msgbits.length !== undefined) {
    let m = "";
    for (let i = 0; i < $_.msgbits.length; i++) {
      const v = $_.msgbits.b ? $_.msgbits.b[$_.msgbits.o + i] : $_.msgbits[i];
      m += String.fromCharCode(v);
    }
    process.stdout.write("msgbits.length=" + $_.msgbits.length + " msgbits=" + m + "\\n");
  }
}
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);

// Also dump pad after the loop.
const ANCHOR2 = "$_.cws = $a($_.dcws);";
const idx2 = src.indexOf(ANCHOR2);
const PATCH2 = `
{
  let p = "";
  for (let i = 0; i < $_.pad.length; i++) p += String.fromCharCode($_.pad.b[$_.pad.o + i]);
  process.stdout.write("pad.length=" + $_.pad.length + " pad=" + p + "\\n");
}
`;
src = src.slice(0, idx2) + PATCH2 + src.slice(idx2);

const patched = patchedPathFor(bp, "bwipjs-m3-state.js");
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
  }
})();
