// Extract intermediate state (dxw / fxw / checksum / final sbs) from
// bwip-js's databarexpanded encoder for a given GS1 AI string.
// Monkey-patches the bundled source so the sbs-assembly stage throws
// an Error whose payload is the captured state.
//
// Usage:
//   node tools/oracle-databarexpanded.js "(01)90012345678908"
//   node tools/oracle-databarexpanded.js "(10)abc123"
//
// Output (JSON):
//   text       — the raw input
//   ais        — parsed AI codes (array of strings)
//   vals       — parsed AI values (array of strings)
//   datalen    — number of data segments (after the dummy
//                placeholder is prepended for the checksum)
//   seq        — finder sequence indices (positions into
//                databarexpanded_finderwidths)
//   checksum   — Reed-Solomon style checksum (mod 211)
//   dxw        — data widths, [seg][7-tuple] (after the placeholder
//                is overwritten with the checksum)
//   fxw        — finder widths, [pair][5-tuple]
//   sbs        — final start-bar-space pattern emitted to renlinear

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: capture state right BEFORE the renlinear call so we have
// the final sbs + dxw + fxw + checksum.
const ANCHOR = "bwipp_renlinear(); //#13670";
if (!bundled.includes(ANCHOR)) {
  throw new Error("anchor not found in bundled source");
}
// Helper: flatten a bwip-js stack-allocated array (with .b/.o/.length)
// into a plain JS array.
const flat = (label) =>
  `Array.from($_.${label}.b ? $_.${label}.b.slice($_.${label}.o, $_.${label}.o + $_.${label}.length) : $_.${label})`;
// Same, but recurse for arrays-of-arrays (dxw / fxw).
const flat2 = (label) =>
  `(function(){
     const top = $_.${label}.b ? $_.${label}.b.slice($_.${label}.o, $_.${label}.o + $_.${label}.length) : $_.${label};
     return Array.from(top).map(inner => Array.from(inner.b ? inner.b.slice(inner.o, inner.o + inner.length) : inner));
   })()`;
const PATCH = `{
          const err = new Error("bwipp.debugdataBarExpanded");
          err.errorinfo = {
            ais: Array.from($_.ais.b ? $_.ais.b.slice($_.ais.o, $_.ais.o + $_.ais.length) : $_.ais),
            vals: Array.from($_.vals.b ? $_.vals.b.slice($_.vals.o, $_.vals.o + $_.vals.length) : $_.vals),
            method: $_.method,
            vlf: ${flat("vlf")},
            cdf: ${flat("cdf")},
            gpf: ${flat("gpf")},
            pad: ${flat("pad")},
            binval: ${flat("binval")},
            datalen: $_.datalen,
            seq: ${flat("seq")},
            checksum: $_.checksum,
            dxw: ${flat2("dxw")},
            fxw: ${flat2("fxw")},
            sbs: ${flat("sbs")},
          };
          err.errorname = "bwipp.debugdataBarExpanded";
          throw err;
        }
        ${ANCHOR}`;
bundled = bundled.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-databarexpanded-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);
const text = process.argv[2] || "(01)90012345678908";

(async () => {
  try {
    await bwipjs.toBuffer({
      bcid: "databarexpanded",
      text,
      includetext: false,
      // Disable GS1 lint so encoder-internal dispatch can be exercised
      // for inputs that BWIPP's validation would otherwise refuse.
      dontlint: true,
    });
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e.message);
      process.exit(3);
    }
    console.log(JSON.stringify({ text, ...e.errorinfo }));
  }
})();
