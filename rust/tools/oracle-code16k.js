// Extract Code 16K encoder state from bwip-js for a corpus of
// mixed-mode payloads. Uses the same anchor-and-throw pattern as
// oracle-posicode.js / oracle-auspost.js.
//
// Run from repo root:
//   node rust/tools/oracle-code16k.js
//
// Output: JSON array of { barcode, cws (mode prefix excluded),
// rows, dcws_inner, c1, c2 }.

"use strict";

const fs = require("fs");
const path = require("path");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: the line that finalizes cws after the main encoder loop.
// At this point cws / mode / cset / urows are all final.
// Source: bwip-js-node.js line 19978: `$_.cws = $geti($_.cws, 0, $_.j); //#19978`
// We patch in a throw that also runs the symbol-size + check pass
// downstream so we get the final `rows`, `dcws_inner`, `c1`, `c2`.
const ANCHOR_RE = /\$_\.cws = \$geti\(\$_\.cws, 0, \$_\.j\); \/\/#19978/;
if (!ANCHOR_RE.test(bundled)) {
  throw new Error("anchor `code16k cws geti #19978` not found in bundle");
}

const PATCH = `$_.cws = $geti($_.cws, 0, $_.j); //#19978
        {
          const dumpArr = (arr) => {
            if (arr && arr.b) {
              return Array.from(arr.b.slice(arr.o, arr.o + arr.length));
            }
            const out = [];
            for (let i = 0; i < arr.length; i++) out.push(arr[i] | 0);
            return out;
          };
          const err = new Error("bwipp.debugCode16k");
          err.errorinfo = {
            mode: $_.mode | 0,
            cset: $_.cset,
            cws: dumpArr($_.cws),
            msg: dumpArr($_.msg),
          };
          err.errorname = "bwipp.debugCode16k";
          throw err;
        }`;

bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-code16k-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);

const CORPUS = [
  // Pure mode-A from start (Stage 21 — already verified). Sanity row.
  { barcode: "ABC" },
  // Pure mode-B from start.
  { barcode: "Hello" },
  // Mid-message A↔B: starts with B-friendly bytes, then control byte,
  // then back to B.
  { barcode: "A\x01B" },
  { barcode: "Hello\x01" },
  { barcode: "Hi\x01\x02" },
  // Starts with control byte (A) then printable (B).
  { barcode: "\x01ABC" },
  // Mixed text + digits (the existing cws-level paths handle a
  // pure-digit run from start; this tests if the encoder picks up
  // a mid-message digit run).
  { barcode: "AB123" },
  // Lowercase forces mode B; mid-message control forces mode A.
  { barcode: "ab\x01cd" },
  // FN4-ish: byte ≥128 (extended ASCII).
  { barcode: "A\x80" },
  { barcode: "A\xc1B" },

  // Digit-run embedded mid-message — does BWIPP shift to mode C?
  { barcode: "AB1234" },
  { barcode: "AB123456" },
  { barcode: "AB12345678CD" },
  { barcode: "AB1234CD" },
  { barcode: "AB12CD" },
  // Long pure-text run with digit ending.
  { barcode: "ABCDE12345" },
  // Tests for initial-mode selectors (modes 2/5/6):
  { barcode: "1234" },         // Pure digit even → mode 2.
  { barcode: "12345" },        // Pure digit odd → mode 5.
  { barcode: "A12" },          // 1 B byte + 2 digits → mode 5 variant (B then C).
  { barcode: "A1234" },        // 1 B byte + 4 digits → mode 5 variant.
  { barcode: "A12345" },       // 1 B byte + 5 digits (odd) → mode 6 (2 B bytes + C from after).
  { barcode: "AB123" },        // 2 B bytes + 3 digits (odd) → mode 6.
  // Mid-message → C in mode A (SC2/SC3 paths).
  { barcode: "\x011234B" },    // Control byte (A), then 4 digits, then back in A.
  { barcode: "\x01123456B" },  // Control byte (A), then 6 digits, then back in A.
  // Mid-message → C in mode B (SC2/SC3 paths).
  { barcode: "a1234b" },       // Lowercase, 4 digits, lowercase.
  { barcode: "a123456b" },     // Lowercase, 6 digits, lowercase.
  // No-shift-to-C cases (3 digits is too few for SC2 since it needs even count).
  { barcode: "a123b" },        // Lowercase, 3 digits, lowercase.

  // SA2 / SB2 test cases (currently broken in Stage 3a — wrong codeword).
  { barcode: "ab\x01\x02cd" }, // Lowercase + 2 A-only + lowercase (SA2 fires from B).
  { barcode: "AB\x01\x02CD" }, // No actually all bytes are in both A and B (for printable upper)... not great.
  { barcode: "a\x01\x02b" },   // Single B + 2 A-only + B (SA2).

  // Mid-message SWC latch in mode B (long leading text + digits).
  { barcode: "ABCDE12345" },   // 5 leading B bytes + 5 digits → SWC after some chars.
  { barcode: "abcde12345" },   // Lowercase variant.
  { barcode: "abcd1234" },     // 4 lowercase + 4 digits.

  // Mode-C SB1 trailing-byte shift (digit pair + text + digit pair).
  { barcode: "12X12" },        // 4 digits with text in middle (BWIPP SB1 in C).
  { barcode: "1234X1234" },    // Longer version.
];

(async () => {
  const out = [];
  for (const { barcode } of CORPUS) {
    try {
      await bwipjs.toBuffer({ bcid: "code16k", text: barcode, includetext: false });
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
