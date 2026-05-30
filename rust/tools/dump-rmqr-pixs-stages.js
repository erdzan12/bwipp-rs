// Dump pixs state at multiple stages of BWIPP's rMQR R7x43 encoding:
//  - Stage A: after finder + timing (right before alignment)
//  - Stage B: after alignment (right before formatmap)
//  - Stage C: after formatmap (right before walker)
// to pinpoint which step writes (row=3, col=21) = 0.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path.js");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor STAGE A: right before `$_.algnpat = $_.qrcode_algnpatrmqr;`
// (which is at line 28361). Insert print before alignment placement.
const ANCHOR_A = "$_.algnpat = $_.qrcode_algnpatrmqr; //#27679";
const idx_a = src.indexOf(ANCHOR_A);
if (idx_a < 0) throw new Error("anchor A not found");

// Anchor STAGE B: right before `$k[$j++] = Infinity; //#27690` which
// starts the formatmap build loop.
const ANCHOR_B = "$k[$j++] = Infinity; //#27690";
const idx_b = src.indexOf(ANCHOR_B);
if (idx_b < 0) throw new Error("anchor B not found");

// Anchor STAGE C: right before `var _VX = ` which is the walker init.
const ANCHOR_C = "var _VX = $ne($_.format, \"rmqr\") ? 1 : 2;";
const idx_c = src.indexOf(ANCHOR_C);
if (idx_c < 0) throw new Error("anchor C not found");

const PRINT_FN = `
if (!globalThis.__stageDump) globalThis.__stageDump = {};
globalThis.__stageDump[__stageName] = (function(){
  const v = $_.pixs;
  if (v && v.b && v.o !== undefined) {
    const out = [];
    for (let i = 0; i < v.length; i++) out.push(v.b[v.o + i]);
    return out;
  }
  return Array.from(v);
})();
`;

function injectStage(srcArg, idx, stageName) {
  const code = `\n{ const __stageName = "${stageName}"; ${PRINT_FN} }\n`;
  return srcArg.slice(0, idx) + code + srcArg.slice(idx);
}

// Insert C first (highest offset), then B, then A.
src = injectStage(src, idx_c, "C");
src = injectStage(src, idx_b, "B");
src = injectStage(src, idx_a, "A");

const patched = patchedPathFor(bp, "bwipjs-rmqr-pixs-stages.js");
fs.writeFileSync(patched, src);

const b = require(patched);
(async () => {
  try {
    await b.toBuffer({
      bcid: "rectangularmicroqrcode",
      text: "HI",
      version: "R7x43",
      eclevel: "M",
      fixedeclevel: true,
      includetext: false,
    });
    const sd = globalThis.__stageDump || {};
    // Print pixs as a 7x43 grid for each stage.
    for (const stage of ["A", "B", "C"]) {
      const p = sd[stage];
      if (!p) {
        console.log(`STAGE ${stage}: not captured`);
        continue;
      }
      console.log(`STAGE ${stage} (${stage === "A" ? "pre-alignment" : stage === "B" ? "post-alignment" : "pre-walker"}):`);
      for (let r = 0; r < 7; r++) {
        let row = "";
        for (let c = 0; c < 43; c++) {
          const v = p[r * 43 + c];
          row += v === -1 ? "." : v.toString();
        }
        console.log(`r${r}: ${row}`);
      }
      console.log();
    }
  } catch (e) {
    console.error(e && e.message);
    process.exit(1);
  }
})();
