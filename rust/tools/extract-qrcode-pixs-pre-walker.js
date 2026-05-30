// Dump BWIPP's pixs array right BEFORE the walker runs (after all
// function-pattern + format-info + version-info reservations are placed).
// This lets us see which cells BWIPP considers reserved vs UNSET.
//
// Run from `node-sidecar/`:
//   node ../rust/tools/extract-qrcode-pixs-pre-walker.js > /tmp/bwipp_pixs_pre_walker.txt

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor right BEFORE the walker loop's `for (;;) {` (which we know from
// line 28131 — the walker setup ends at line 28130 with $_.num = 0).
const ANCHOR = "$_.num = 0; //#27767";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("walker setup anchor not found");
const insertPoint = idx + ANCHOR.length;
const PATCH = `
{
  const err = new Error("bwipp.dumpPixsPreWalker");
  const cols = $_.cols;
  const rows = $_.rows;
  const pixs = [];
  for (let i = 0; i < rows * cols; i++) pixs.push($get($_.pixs, i));
  err.errorinfo = { rows, cols, pixs };
  throw err;
}
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);
const patched = patchedPathFor(bp, "bwipjs-pixs-pre-walker.js");
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
      mask: "1",
      includetext: false,
    });
    console.error("expected throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e && e.message);
      process.exit(3);
    }
    const { rows, cols, pixs } = e.errorinfo;
    console.log(`# pre-walker pixs ${rows}x${cols}`);
    for (let r = 0; r < rows; r++) {
      let line = "";
      for (let c = 0; c < cols; c++) {
        const v = pixs[r * cols + c];
        if (v === -1) line += "?";
        else if (v === 0) line += ".";
        else if (v === 1) line += "#";
        else line += "!";
      }
      console.log(line);
    }
  }
})();
