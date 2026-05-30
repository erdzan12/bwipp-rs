// Dump MaxiCode's final pixs (list of black-cell positions in the
// 33×30 grid).
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

const ANCHOR = "$_.pixs = $a(); //#29428";
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
            pixs: ${flat("pixs")},
            pixs_len: $_.pixs.length,
          };
          err.errorname = "dbg";
          throw err;
        }`;
bundled = bundled.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-maxi-pixs-patched.js");
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
