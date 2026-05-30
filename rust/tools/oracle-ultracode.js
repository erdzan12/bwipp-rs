// Capture BWIPP `bwipp_ultracode` encoder state for a corpus of
// default-options inputs. Same anchor-and-throw pattern as
// oracle-codeone.js / oracle-code16k.js.
//
// Run from repo root:
//   node rust/tools/oracle-ultracode.js
//
// Output: JSON array of one record per input:
//   {
//     barcode, eclevel, rev,
//     dcws, ecws, coeffs,
//     mcc, qcc, tcc, pads, rows, columns, dcc,
//     pixs
//   }

"use strict";

const fs = require("fs");
const path = require("path");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor 1: just before `bwipp_renmatrix` so we capture pixs + the
// upstream-computed `rows`, `columns`, `dcc`, `mcc`, `qcc`, `tcc`,
// `pads`, `dcws`, `ecws`, and `coeffs`. The actual call sequence:
//   ... compute everything ...
//   if ($_._render) { bwipp_renmatrix(); }
//   $_ = $__;
// We patch the `if ($_._render)` line so we still see all globals.
//
// Source: bwip-js-node.js:37256 `if ($_._render) { //#34868`
const ANCHOR_RE = /if \(\$_\._render\) \{ \/\/#34868\s+bwipp_renmatrix\(\); \/\/#34868\s+\} \/\/#34868/;
if (!ANCHOR_RE.test(bundled)) {
  throw new Error("anchor `ultracode pre-renmatrix #34868` not found in bundle");
}

const PATCH = `{
            const dumpArr = (arr) => {
              if (arr && arr.b) {
                return Array.from(arr.b.slice(arr.o, arr.o + arr.length));
              }
              const out = [];
              for (let i = 0; i < arr.length; i++) out.push(arr[i] | 0);
              return out;
            };
            const err = new Error("bwipp.debugUltracode");
            err.errorinfo = {
              dcws: dumpArr($_.dcws),
              ecws: dumpArr($_.ecws),
              coeffs: dumpArr($_.coeffs),
              mcc: $_.mcc | 0,
              qcc: $_.qcc | 0,
              tcc: $_.tcc | 0,
              pads: $_.pads | 0,
              rows: $_.rows | 0,
              columns: $_.columns | 0,
              dcc: $_.dcc | 0,
              pixs: dumpArr($_.pixs),
            };
            err.errorname = "bwipp.debugUltracode";
            throw err;
        }
        if ($_._render) { //#34868
            bwipp_renmatrix(); //#34868
        } //#34868`;

bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-ultracode-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);

// Corpus — default options (eclevel=EC2, rev=2, parsefnc=false).
// Each record may override eclevel/rev.
const CORPUS = [
  // Smallest: 1 byte → 1 dcw → mcc=4 → EC2 qcc≈7 → tcc≈11 → minc 7.
  { barcode: "A" },
  { barcode: "Hello" },
  { barcode: "Hello, World!" },
  { barcode: "12345" },
  { barcode: "ABCDEFGHIJKLMNOPQRSTUVWXYZ" },
  { barcode: "abcdef0123456789" },
  // Bytes 0..255 sentinels.
  { barcode: "\x00\x01\x02\x7f\x80\xff" },
  // Slightly longer.
  { barcode: "The quick brown fox jumps over the lazy dog." },
];

(async () => {
  const out = [];
  for (const item of CORPUS) {
    const opts = Object.assign({}, { bcid: "ultracode", text: item.barcode }, {
      eclevel: item.eclevel,
      rev: item.rev,
    });
    // Strip undefined to use BWIPP defaults.
    for (const k of Object.keys(opts)) {
      if (opts[k] === undefined) delete opts[k];
    }
    try {
      await bwipjs.toBuffer(opts);
      console.error(`expected debug throw for ${JSON.stringify(item)}`);
      process.exit(2);
    } catch (e) {
      if (!e.errorinfo) {
        console.error(`uncaught for ${item.barcode}: ${e.message}`);
        process.exit(3);
      }
      out.push({
        barcode: item.barcode,
        eclevel: item.eclevel || "EC2",
        rev: item.rev != null ? item.rev : 2,
        ...e.errorinfo,
      });
    }
  }
  console.log(JSON.stringify(out, null, 2));
})();
