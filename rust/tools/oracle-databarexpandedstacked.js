// Extract intermediate state for bwip-js's databarexpandedstacked
// encoder. Monkey-patches the bundled source to throw at the
// renmatrix anchor, then forwards the captured state as a JSON
// payload.
//
// Usage:
//   node tools/oracle-databarexpandedstacked.js "(01)90012345678908"
//
// Output JSON keys: text, pixx, pixy, numrows, segments, datalen,
// rowmult, pixs, sbs_rows.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor on the rowmult assignment — it runs unconditionally inside
// the expandedstacked branch (the renmatrix call below it is gated
// on _render, which databarexpandedstacked sets to false).
const ANCHOR = "$_.rowmult = $a(); //#13775";
if (!bundled.includes(ANCHOR)) {
  throw new Error("anchor not found in bundled source");
}
const flat = (label) =>
  `Array.from($_.${label}.b ? $_.${label}.b.slice($_.${label}.o, $_.${label}.o + $_.${label}.length) : $_.${label})`;
const flat2 = (label) =>
  `(function(){
     const top = $_.${label}.b ? $_.${label}.b.slice($_.${label}.o, $_.${label}.o + $_.${label}.length) : $_.${label};
     return Array.from(top).map(inner => Array.from(inner.b ? inner.b.slice(inner.o, inner.o + inner.length) : inner));
   })()`;
// Insert the throw AFTER the anchor so the captured state has
// already been built.
const PATCH = `${ANCHOR}
        {
          const err = new Error("bwipp.debugdataBarExpandedStacked");
          err.errorinfo = {
            pixx: $_.pixx,
            pixy: $_.pixy,
            numrows: $_.numrows,
            segments: $_.segments,
            datalen: $_.datalen,
            rowmult: ${flat("rowmult")},
            pixs: ${flat("pixs")},
            sbs_rows: ${flat2("rows")},
          };
          err.errorname = "bwipp.debugdataBarExpandedStacked";
          throw err;
        }`;
bundled = bundled.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-dbestacked-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);
const text = process.argv[2] || "(01)90012345678908";

(async () => {
  try {
    await bwipjs.toBuffer({
      bcid: "databarexpandedstacked",
      text,
      includetext: false,
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
