// Extract POSICODE encoder state from bwip-js for a fixed corpus.
// Uses the "anchor + patched throw" pattern (see oracle-auspost.js)
// to capture cws, d, v, cbs, and sbs right after the encoder
// finishes building them and before the renderer is called.
//
// Run from repo root:
//   node rust/tools/oracle-posicode.js
//
// Output: JSON array of { version, barcode, cws, d, v, cbs, sbs }.

"use strict";

const fs = require("fs");
const path = require("path");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: the line that trims sbs to its final length in bwipp_posicode.
// At this point cws / d / v / cbs / sbs are all final.
const ANCHOR_RE = /\$_\.sbs = \$geti\(\$_\.sbs, 0, \$_\.j\); \/\/#18520/;
if (!ANCHOR_RE.test(bundled)) {
  throw new Error("anchor `posicode sbs geti #18520` not found in bundle");
}

const PATCH = `$_.sbs = $geti($_.sbs, 0, $_.j); //#18520
        {
          const dumpArr = (arr) => {
            if (arr && arr.b) {
              return Array.from(arr.b.slice(arr.o, arr.o + arr.length));
            }
            const out = [];
            for (let i = 0; i < arr.length; i++) out.push(arr[i] | 0);
            return out;
          };
          // sbs / cbs are strings (PostScript-style); they read byte
          // values via charCodeAt-style access. $get on a string
          // returns the codepoint; the module-width is codepoint - 48.
          const dumpStr = (s) => {
            if (s && s.b) {
              const out = [];
              for (let i = 0; i < s.length; i++) out.push(s.b[s.o + i] - 48);
              return out;
            }
            const out = [];
            for (let i = 0; i < s.length; i++) out.push((s.charCodeAt ? s.charCodeAt(i) : s[i]) - 48);
            return out;
          };
          const err = new Error("bwipp.debugPosicode");
          err.errorinfo = {
            version: $_.version,
            v: $_.v | 0,
            cws: dumpArr($_.cws),
            d: dumpArr($_.d),
            cbs: dumpStr($_.cbs),
            sbs: dumpStr($_.sbs),
          };
          err.errorname = "bwipp.debugPosicode";
          throw err;
        }`;

bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-posicode-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);

const CORPUS = [
  // limiteda — single-set, no shifts/latches/FNC.
  { version: "limiteda", barcode: "0" },
  { version: "limiteda", barcode: "1" },
  { version: "limiteda", barcode: "A" },
  { version: "limiteda", barcode: "Z" },
  { version: "limiteda", barcode: "0123456789" },
  { version: "limiteda", barcode: "ABCDEFGHIJ" },
  { version: "limiteda", barcode: "HELLO" },
  { version: "limiteda", barcode: "POSICODE" },
  { version: "limiteda", barcode: "12345" },
  { version: "limiteda", barcode: "ABC-123" },

  // limitedb — same alphabet, wider patterns, d[i] += 1 in cbs.
  { version: "limitedb", barcode: "0" },
  { version: "limitedb", barcode: "1" },
  { version: "limitedb", barcode: "A" },
  { version: "limitedb", barcode: "Z" },
  { version: "limitedb", barcode: "0123456789" },
  { version: "limitedb", barcode: "HELLO" },
  { version: "limitedb", barcode: "12345" },

  // version a — set-0 only (digits + uppercase) — tests the
  // happy-path auto-encoder where no latches / shifts / FNC fire.
  { version: "a", barcode: "0" },
  { version: "a", barcode: "1" },
  { version: "a", barcode: "9" },
  { version: "a", barcode: "A" },
  { version: "a", barcode: "Z" },
  { version: "a", barcode: "HELLO" },
  { version: "a", barcode: "POSICODE" },
  { version: "a", barcode: "12345" },
  { version: "a", barcode: "ABC123" },
  // version a — set-1 (lowercase) only — exercises the LA1 latch
  // out of set0 into set1 at position 0.
  { version: "a", barcode: "abc" },
  { version: "a", barcode: "hello" },
  // version a — mixed set-0 / set-1 — exercises both latches and
  // shifts (SF1 from set0 for a single lowercase char; SF0 from
  // set1 for a single uppercase).
  { version: "a", barcode: "Ab" },
  { version: "a", barcode: "AbC" },
  { version: "a", barcode: "aB" },
  { version: "a", barcode: "Abcd" },
  // version a — set-2 (control) shift via SF2 — the simplest
  // way to surface SF2 is to embed a single control byte. We use
  // 0x01..0x05 which map to set-2 row "A"..="E" (charmapsnormal
  // rows 10..14, third column).
  { version: "a", barcode: "A" },
  { version: "a", barcode: "A" },
  // version a — punctuation that crosses sets.
  { version: "a", barcode: "A-B" },
  { version: "a", barcode: "A.B" },
  { version: "a", barcode: "A B" },

  // version b — same alphabet/state-machine as a, only the bar
  // patterns differ (wider) plus d[i] += 1 in cbs. Smaller
  // corpus — the state-machine is shared, so the b row mainly
  // pins the bar-pattern table delta.
  { version: "b", barcode: "0" },
  { version: "b", barcode: "1" },
  { version: "b", barcode: "A" },
  { version: "b", barcode: "Z" },
  { version: "b", barcode: "HELLO" },
  { version: "b", barcode: "12345" },
  { version: "b", barcode: "abc" },
  { version: "b", barcode: "Ab" },

  // version a — FN4 transitions (extended-ASCII bytes ≥ 0x80).
  // Single extended byte at end → single FN4 shift; long run
  // of extended bytes → double FN4 latch.
  { version: "a", barcode: "A\x80" },          // 'A' then extended 0x80
  { version: "a", barcode: "A\xc1" },          // 'A' then extended 0xc1 (= 'A' | 0x80)
  { version: "a", barcode: "\x80" },           // single extended byte
  { version: "a", barcode: "\xc1\xc2\xc3" },   // 3-byte extended run
  { version: "a", barcode: "AB\xc1\xc2\xc3" }, // mixed standard-then-extended-run
  { version: "a", barcode: "\xc1A" },          // extended single then standard
];

(async () => {
  const out = [];
  for (const { version, barcode } of CORPUS) {
    try {
      await bwipjs.toBuffer({ bcid: "posicode", text: barcode, version, includetext: false });
      console.error(`expected encoder to throw for ${version}/${barcode}`);
      process.exit(2);
    } catch (e) {
      if (!e.errorinfo) {
        console.error(`uncaught for ${version}/${barcode}: ${e.message}`);
        process.exit(3);
      }
      out.push({ version, barcode, ...e.errorinfo });
    }
  }
  console.log(JSON.stringify(out, null, 2));
})();
