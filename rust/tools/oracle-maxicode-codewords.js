// Dump MaxiCode's final 144-codeword stream (after RS-ECC).
// Anchors on the debugcws path BWIPP exposes when the option is set.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

const ANCHOR = "$_.codewords = $a(); //#29391";
if (!bundled.includes(ANCHOR)) {
  throw new Error("anchor not found");
}
const flat = (label) =>
  `Array.from($_.${label}.b ? $_.${label}.b.slice($_.${label}.o, $_.${label}.o + $_.${label}.length) : $_.${label})`;
const PATCH = `${ANCHOR}
        {
          const err = new Error("dbg");
          err.errorinfo = {
            mode: $_.mode,
            pri: ${flat("pri")},
            sec: ${flat("sec")},
            prichk: ${flat("prichk")},
            secchk: ${flat("secchk")},
            codewords: ${flat("codewords")},
          };
          err.errorname = "dbg";
          throw err;
        }`;
bundled = bundled.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-maxi-cws-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);
const data = process.argv[2] || "X";
const mode = parseInt(process.argv[3] || "4", 10);
const text = (mode === 2 || mode === 3)
  ? `12345\x1d840\x1d001\x1d${data}`
  : data;

(async () => {
  try {
    await bwipjs.toBuffer({ bcid: "maxicode", text, parse: false, mode });
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e.message);
      process.exit(3);
    }
    console.log(JSON.stringify(e.errorinfo));
  }
})();
