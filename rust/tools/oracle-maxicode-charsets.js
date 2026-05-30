// Dump inverse setA..setE maps from bwip-js's maxicode_charmaps
// by triggering the encoder's init and reading the global charvals
// arrays at the encoder's anchor.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: after charvals is populated (line 29142+) but before
// we get too deep. Use the "seta = charvals[0]" assignment.
const ANCHOR = "$_.seta = $get($_.charvals, 0); //#28575";
if (!bundled.includes(ANCHOR)) {
  throw new Error("anchor not found");
}
// Dump each set as a Map serialized to array-of-pairs.
const PATCH = `${ANCHOR}
        {
          const dump = (m) => {
            const out = [];
            m.forEach((v, k) => out.push([k, v]));
            return out;
          };
          const err = new Error("dbg");
          err.errorinfo = {
            setA: dump($get($_.charvals, 0)),
            setB: dump($get($_.charvals, 1)),
            setC: dump($get($_.charvals, 2)),
            setD: dump($get($_.charvals, 3)),
            setE: dump($get($_.charvals, 4)),
          };
          err.errorname = "dbg";
          throw err;
        }`;
bundled = bundled.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-maxi-charsets-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);
(async () => {
  try {
    await bwipjs.toBuffer({
      bcid: "maxicode",
      text: "X",
      parse: false,
      mode: 4,
    });
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e.message);
      process.exit(3);
    }
    console.log(JSON.stringify(e.errorinfo));
  }
})();
