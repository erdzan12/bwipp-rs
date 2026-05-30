// Extract intermediate state (widths array + checksum) from bwip-js's
// databaromni encoder for a given input. Monkey-patches the bundled
// source so the post-widths-computation stage throws an Error whose
// payload is the captured state.
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: capture state right BEFORE the checklt assignment so we have
// the final widths + checksum + group parameters.
const ANCHOR = "$_.checklt = $geti($_.databaromni_checkwidths,";
if (!bundled.includes(ANCHOR)) {
  throw new Error("anchor not found in bundled source");
}
const flatten = (label) =>
  `Array.from($_.${label}.b ? $_.${label}.b.slice($_.${label}.o, $_.${label}.o + $_.${label}.length) : $_.${label})`;
const PATCH = `{
          const err = new Error("bwipp.debugdataBarOmni");
          err.errorinfo = {
            left: $_.left, right: $_.right,
            d1: $_.d1, d2: $_.d2, d3: $_.d3, d4: $_.d4,
            d1gs: $_.d1gs, d2gs: $_.d2gs, d3gs: $_.d3gs, d4gs: $_.d4gs,
            d1to: $_.d1to, d1te: $_.d1te, d1mwo: $_.d1mwo, d1mwe: $_.d1mwe, d1elo: $_.d1elo, d1ele: $_.d1ele,
            d2to: $_.d2to, d2te: $_.d2te, d2mwo: $_.d2mwo, d2mwe: $_.d2mwe, d2elo: $_.d2elo, d2ele: $_.d2ele,
            d3to: $_.d3to, d3te: $_.d3te, d3mwo: $_.d3mwo, d3mwe: $_.d3mwe, d3elo: $_.d3elo, d3ele: $_.d3ele,
            d4to: $_.d4to, d4te: $_.d4te, d4mwo: $_.d4mwo, d4mwe: $_.d4mwe, d4elo: $_.d4elo, d4ele: $_.d4ele,
            d1w: ${flatten("d1w")},
            d2w: ${flatten("d2w")},
            d3w: ${flatten("d3w")},
            d4w: ${flatten("d4w")},
            widths: ${flatten("widths")},
            checksum: $_.checksum,
          };
          err.errorname = "bwipp.debugdataBarOmni";
          throw err;
        }
        ${ANCHOR}`;
bundled = bundled.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-databaromni-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);
const text = process.argv[2] || "(01)24012345678905";

(async () => {
  try {
    await bwipjs.toBuffer({
      bcid: "databaromni",
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
