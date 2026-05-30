// Extract MaxiCode's `pri` (10-codeword primary message) for a
// given input. Patches bwip-js to throw right after `$_.pri` is
// computed in the mode 2/3 branch.
//
// Usage:
//   node tools/oracle-maxicode-primary.js "<postcode>" "<ccode>" "<scode>"
//   node tools/oracle-maxicode-primary.js "12345" "840" "001"

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor right before the `$puti($_.sec, 0, $_.encmsg)` line, by
// then `$_.pri` (10-element 6-bit codewords) is fully populated.
const ANCHOR = "$puti($_.sec, 0, $_.encmsg); //#29307";
if (!bundled.includes(ANCHOR)) {
  throw new Error("anchor not found in bundled source");
}
const flat = (label) =>
  `Array.from($_.${label}.b ? $_.${label}.b.slice($_.${label}.o, $_.${label}.o + $_.${label}.length) : $_.${label})`;
const PATCH = `${ANCHOR}
        {
          const err = new Error("bwipp.debugMaxiCodePrimary");
          err.errorinfo = {
            mode: $_.mode,
            pcode: $_.pcode,
            ccode: $_.ccode,
            scode: $_.scode,
            pri: ${flat("pri")},
          };
          err.errorname = "bwipp.debugMaxiCodePrimary";
          throw err;
        }`;
bundled = bundled.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-maxicode-primary-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);
const pcode = process.argv[2] || "12345";
const ccode = process.argv[3] || "840";
const scode = process.argv[4] || "001";
const mode = parseInt(process.argv[5] || "2", 10);

// MaxiCode input format: postcode <GS> country <GS> service <GS> [data]
// where <GS> is ASCII 0x1d. We supply minimal data ("X") so the
// encoder makes it to the primary builder.
const text = `${pcode}\x1d${ccode}\x1d${scode}\x1dX`;

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
