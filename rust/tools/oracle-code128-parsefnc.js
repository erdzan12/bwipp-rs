// Capture BWIPP `bwipp_code128` encoder output with parsefnc=true.
// Each input contains `^FNC1` / `^FNC2` / `^FNC3` / `^LNKA` / `^LNKC`
// markers that BWIPP's parseinput resolves to internal FNC tokens.
//
// Run from repo root: `node rust/tools/oracle-code128-parsefnc.js`
//
// Emits a JSON record per input with the BWIPP raw codeword stream
// (`cws`) so the Rust port can pin pick_codes() byte-for-byte.

"use strict";

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: BWIPP code128's encoder finalises `$_.cws` then falls through
// to renderer. Source: bwip-js-node.js immediately after the cws array
// is set. The canonical hook point for "everything has been encoded,
// nothing has been drawn yet" is the `$_._render` guard near the end
// of bwipp_code128.
const ANCHOR_RE = /if \(\$_\._render\) \{ \/\/#9641\s+bwipp_renlinear\(\); \/\/#9641\s+\} \/\/#9641/;
if (!ANCHOR_RE.test(bundled)) {
    throw new Error("anchor `code128 pre-renlinear #9641` not found in bundle");
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
            const err = new Error("bwipp.debugCode128");
            err.errorinfo = {
              cws: dumpArr($_.cws),
              msg: dumpArr($_.msg),
              encoding: $_.encoding,
              parsefnc: $_.parsefnc,
            };
            err.errorname = "bwipp.debugCode128";
            throw err;
        }
        if ($_._render) { //#9641
            bwipp_renlinear(); //#9641
        } //#9641`;

bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-code128-parsefnc-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);

// Corpus — every record exercises a distinct parsefnc code path.
const CORPUS = [
    // shortest valid: a single ^FNC1 at start (GS1-128 style).
    { barcode: "^FNC112345678901234", parsefnc: true },
    // ^FNC1 mid-message after a Subset-B run.
    { barcode: "ABC^FNC1DEF", parsefnc: true },
    // ^FNC2 in the middle (97 in A/B).
    { barcode: "AB^FNC2CD", parsefnc: true },
    // ^FNC3 in the middle (96 in A/B).
    { barcode: "AB^FNC3CD", parsefnc: true },
    // ^^ literal caret.
    { barcode: "FOO^^BAR", parsefnc: true },
    // Multiple FNCs interleaved.
    { barcode: "^FNC1A^FNC2B^FNC3C", parsefnc: true },
    // Linkage A at the end (composite carrier).
    { barcode: "1234^LNKA", parsefnc: true },
    // Linkage C at the end.
    { barcode: "1234^LNKC", parsefnc: true },
    // parsefnc=false control — markers stay literal.
    { barcode: "AB^FNC1CD", parsefnc: false },
];

(async () => {
    const out = [];
    for (const item of CORPUS) {
        const opts = { bcid: "code128", text: item.barcode, parsefnc: item.parsefnc };
        try {
            await bwipjs.toBuffer(opts);
            console.error(`expected debug throw for ${JSON.stringify(item)}`);
            process.exit(2);
        } catch (e) {
            if (!e.errorinfo) {
                console.error(`uncaught for ${item.barcode}: ${e.message}`);
                process.exit(3);
            }
            out.push({ barcode: item.barcode, parsefnc: item.parsefnc, ...e.errorinfo });
        }
    }
    console.log(JSON.stringify(out, null, 2));
})();
