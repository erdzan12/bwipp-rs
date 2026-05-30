// Extract Code One encoder state from bwip-js for a corpus of
// payloads that exercise Mode D (decimal compression) and the other
// modes. Uses the same anchor-and-throw pattern as
// oracle-posicode.js / oracle-code16k.js.
//
// Run from repo root:
//   node rust/tools/oracle-codeone.js
//
// Output: JSON array of { barcode, cws, version }.

"use strict";

const fs = require("fs");
const path = require("path");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: the line that finalizes cws after the main encoder loop.
// Source: bwip-js-node.js line 31717: `$_.cws = $geti($_.cws, 0, $_.j); //#31717`
// At this point cws / mode are final; symbol-size selection + RS
// follow downstream.
const ANCHOR_RE = /\$_\.cws = \$geti\(\$_\.cws, 0, \$_\.j\); \/\/#31717/;
if (!ANCHOR_RE.test(bundled)) {
  throw new Error("anchor `codeone cws geti #31717` not found in bundle");
}

const PATCH = `$_.cws = $geti($_.cws, 0, $_.j); //#31717
        {
          const dumpArr = (arr) => {
            if (arr && arr.b) {
              return Array.from(arr.b.slice(arr.o, arr.o + arr.length));
            }
            const out = [];
            for (let i = 0; i < arr.length; i++) out.push(arr[i] | 0);
            return out;
          };
          const err = new Error("bwipp.debugCodeone");
          err.errorinfo = {
            cws: dumpArr($_.cws),
            mode: $_.mode | 0,
            msg: dumpArr($_.msg),
          };
          err.errorname = "bwipp.debugCodeone";
          throw err;
        }`;

bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-codeone-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);

const CORPUS = [
  // Currently-verified baseline (Mode A from start).
  { barcode: "A" },
  { barcode: "Hello" },
  { barcode: "ABC" },
  { barcode: "ABCDEFG" },

  // Mode A digit pairs (already verified).
  { barcode: "12" },
  { barcode: "12345678" }, // 8 digits, all-Mode-A pair-packed.

  // Mode D trigger via "13+ digits at EOM" rule.
  { barcode: "1234567890123" },        // exactly 13 digits at end → Mode D.
  { barcode: "12345678901234567890" }, // 20 digits at end (still < 21).
  { barcode: "A1234567890123" },       // 'A' in Mode A, then 13 trailing digits.

  // Mode D trigger via ">= 21" digits anywhere.
  { barcode: "123456789012345678901" },         // 21 digits.
  { barcode: "123456789012345678901ABC" },      // 21 digits + Mode A tail.
  { barcode: "ABC123456789012345678901DEF" },   // Mode A + 21 digits + Mode A tail.

  // Mode D with various digit counts (divisible by 3 vs not).
  { barcode: "123456789012" },          // 12 digits — Mode A pair-packing.
  { barcode: "1234567890123456" },      // 16 digits — Mode D from start (>= 13 at EOM).
  { barcode: "12345678901234" },        // 14 digits — Mode D, ends with 2 trailing.
  { barcode: "123456789012345" },       // 15 digits — Mode D, clean termination.
];

(async () => {
  const out = [];
  for (const { barcode } of CORPUS) {
    try {
      await bwipjs.toBuffer({ bcid: "codeone", text: barcode, includetext: false });
      console.error(`expected encoder to throw for ${barcode}`);
      process.exit(2);
    } catch (e) {
      if (!e.errorinfo) {
        console.error(`uncaught for ${barcode}: ${e.message}`);
        process.exit(3);
      }
      out.push({ barcode, ...e.errorinfo });
    }
  }
  console.log(JSON.stringify(out, null, 2));
})();
