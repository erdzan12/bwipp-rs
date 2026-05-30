// Extract MaxiCode's `encmsg` (secondary message codeword stream)
// after the charset state machine encodes it.
//
// Usage:
//   node tools/oracle-maxicode-secondary.js "<secondary-data>" [mode]
//   node tools/oracle-maxicode-secondary.js "TEST1234" 4
//
// For mode 2/3, also supply the postcode/country/service prefix via
// GS-separated input. For mode 4, supply just the data.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor right where encmsg is finalised (line 30289).
const ANCHOR = "$_.encmsg = $a(); //#29232";
if (!bundled.includes(ANCHOR)) {
  throw new Error("anchor not found in bundled source");
}
const flat = (label) =>
  `Array.from($_.${label}.b ? $_.${label}.b.slice($_.${label}.o, $_.${label}.o + $_.${label}.length) : $_.${label})`;
const PATCH = `${ANCHOR}
        {
          const err = new Error("bwipp.debugMaxiCodeSecondary");
          err.errorinfo = {
            mode: $_.mode,
            encmsg: ${flat("encmsg")},
            encmsg_length: $_.encmsg.length,
          };
          err.errorname = "bwipp.debugMaxiCodeSecondary";
          throw err;
        }`;
bundled = bundled.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-maxicode-secondary-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);
const data = process.argv[2] || "TEST";
const mode = parseInt(process.argv[3] || "4", 10);
// Mode 2/3 need the GS-prefixed format; mode 4 takes raw data.
const text = (mode === 2 || mode === 3)
  ? `12345\x1d840\x1d001\x1d${data}`
  : data;

(async () => {
  try {
    await bwipjs.toBuffer({
      bcid: "maxicode",
      text,
      parse: false,
      mode,
    });
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e.message);
      process.exit(3);
    }
    console.log(JSON.stringify(e.errorinfo));
  }
})();
