// Capture BWIPP's final pixs grid (after snake + six-edge bit
// placement) for a fixed mask. Lets the Rust port verify both the
// snake traversal and the corner-bit placement end-to-end.
//
// Usage: node tools/oracle-dotcode-pixs.js "A" 0
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor: right after the six-edge corner bit placement loop closes
// and before the evalsymbol scoring.
const ANCHOR = `        $k[$j++] = 'score'; //#34267
        $k[$j++] = $_.pixs; //#34267
        $_.evalsymbol(); //#34267`;
if (!src.includes(ANCHOR)) throw new Error("anchor not found");
const fl = (v) =>
  `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;
const PATCH = `        {
          const err = new Error("bwipp.debugDotCodePixs");
          err.errorinfo = {
            pixs: ${fl("$_.pixs")},
            mask: $_.mask,
            rows: $_.rows,
            columns: $_.columns,
            ndots: $_.ndots,
          };
          err.errorname = "bwipp.debugDotCodePixs";
          throw err;
        }
` + ANCHOR;
src = src.replace(ANCHOR, PATCH);
const p = patchedPathFor(bp, "bwipjs-dotcode-pixs-patched.js");
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
