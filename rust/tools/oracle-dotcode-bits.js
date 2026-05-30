// Capture BWIPP's intermediate `bits` string + `pixs` grid for
// DotCode, right after RS + mask + bit packing but before the
// renderer scales them up.
//
// Usage: node tools/oracle-dotcode-bits.js "A" [mask]
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor: right after `$_.bits` is fully built and before pixs assembly.
const ANCHOR = `        if ($_.rembits > 0) { //#34227
            $puti($_.bits, ($_.nw * 9) + 2, $geti("11111111111111111", 0, $_.rembits)); //#34226
        } //#34226`;
if (!src.includes(ANCHOR)) throw new Error("anchor not found");
const fl = (v) =>
  `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;
const PATCH = ANCHOR + `
        {
          const err = new Error("bwipp.debugDotCodeBits");
          err.errorinfo = {
            bits: $_.bits.toString(),
            mask: $_.mask,
            nd: $_.nd,
            nw: $_.nw,
            rows: $_.rows,
            columns: $_.columns,
            ndots: $_.ndots,
            rembits: $_.rembits,
          };
          err.errorname = "bwipp.debugDotCodeBits";
          throw err;
        }`;
src = src.replace(ANCHOR, PATCH);
const p = patchedPathFor(bp, "bwipjs-dotcode-bits-patched.js");
fs.writeFileSync(p, src);

const b = require(p);
const text = process.argv[2] || "A";
const mask = process.argv[3];
const opts = { bcid: "dotcode", text, includetext: false };
if (mask !== undefined) opts.mask = parseInt(mask, 10);

(async () => {
  try {
    await b.toBuffer(opts);
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e && e.message);
      process.exit(3);
    }
    console.log(JSON.stringify({ text, ...e.errorinfo }));
  }
})();
