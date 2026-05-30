// Dump BWIPP's walker positions + codeword bits for R7x43 M "HI".
//
// Patches bwip-js's walker loop (around lines 28479-28510) to record
// every visited (posx, posy) and the bit written at that position.
// Output is JSON to stdout with two fields:
//
//   {
//     "positions": [[col, row], ...], // walker traversal order
//     "bits":      [0|1, ...],        // bits written at each pos
//     "pixs":      [0|1, ...]         // full row-major final pixs
//   }
//
// Run from `node-sidecar/`:
//   node ../rust/tools/dump-rmqr-walker-r7x43.js > /tmp/rmqr_walker.json

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path.js");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Patch the walker — replace the body of the `for (;;)` loop to also
// record positions. We hook by injecting record code after the
// "if ($get($_.pixs, $_.pos) == -1) {" block — that's where BWIPP
// writes data bits.
const ANCHOR =
  "if ($get($_.pixs, $_.pos) == -1) { //#27775\n            $_.bit = ($bs($get($_.cws, ~~($_.num / 8)), -(7 - ($_.num % 8)))) & 1; //#27772";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("walker anchor not found");
const REPLACEMENT =
  ANCHOR +
  "\n" +
  "            if (!globalThis.__rmqrWalker) globalThis.__rmqrWalker = { positions: [], bits: [] };\n" +
  "            globalThis.__rmqrWalker.positions.push([$_.posx, $_.posy]);\n" +
  "            globalThis.__rmqrWalker.bits.push($_.bit);\n";
src = src.replace(ANCHOR, REPLACEMENT);

const patched = patchedPathFor(bp, "bwipjs-rmqr-walker-r7x43.js");
fs.writeFileSync(patched, src);

const b = require(patched);
(async () => {
  try {
    const raw = b.raw("rectangularmicroqrcode", "HI", {
      version: "R7x43",
      eclevel: "M",
      fixedeclevel: true,
      includetext: false,
    });
    const r = raw[0];
    const w = globalThis.__rmqrWalker || { positions: [], bits: [] };
    const flat = Array.from(r.pixs).map((p) => (p ? 1 : 0));
    process.stdout.write(
      JSON.stringify(
        {
          rows: r.pixy,
          cols: r.pixx,
          positions: w.positions,
          bits: w.bits,
          pixs: flat,
        },
        null,
        2,
      ) + "\n",
    );
  } catch (e) {
    console.error("FAILED:", e && e.message);
    process.exit(1);
  }
})();
