// Capture BWIPP `bwipp_ultracode` encoder state with non-default opt-in
// values: eclevel ∈ {EC0, EC1, EC3, EC4, EC5}, rev=1, link1>0, start!=257.
//
// Same anchor-and-throw pattern as oracle-ultracode.js (the default-
// options corpus). Stage 11.10+ pin every option permutation against
// the values captured here.
//
// Run from repo root: `node rust/tools/oracle-ultracode-opts.js`

"use strict";

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

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
            const err = new Error("bwipp.debugUltracodeOpts");
            err.errorinfo = {
              dcws: dumpArr($_.dcws),
              ecws: dumpArr($_.ecws),
              mcc: $_.mcc | 0,
              qcc: $_.qcc | 0,
              acc: $_.acc | 0,
              tcc: $_.tcc | 0,
              pads: $_.pads | 0,
              rows: $_.rows | 0,
              columns: $_.columns | 0,
              dcc: $_.dcc | 0,
              pixs: dumpArr($_.pixs),
            };
            err.errorname = "bwipp.debugUltracodeOpts";
            throw err;
        }
        if ($_._render) { //#34868
            bwipp_renmatrix(); //#34868
        } //#34868`;

bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-ultracode-opts-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);

// Each record can override eclevel/rev/link1/start; missing keys mean
// BWIPP defaults. EC0 requires rev=1 (BWIPP validation line 36544).
const CORPUS = [
    // === Stage 11.10 — eclevel non-default variants ===
    { barcode: "Hello", eclevel: "EC1" },
    { barcode: "Hello", eclevel: "EC3" },
    { barcode: "Hello", eclevel: "EC4" },
    { barcode: "Hello", eclevel: "EC5" },
    // Longer payload exercises ceil(mcc/25) factor in qcc formula.
    { barcode: "The quick brown fox jumps over the lazy dog.", eclevel: "EC3" },

    // === Stage 11.11 — rev=1 (legacy revision) ===
    { barcode: "Hello", rev: 1 },
    { barcode: "ABCDEFGHIJKLMNOP", rev: 1 },
    // rev=1 enables EC0 (BWIPP cross-validation).
    { barcode: "Hello", eclevel: "EC0", rev: 1 },
    { barcode: "A1B2C3", eclevel: "EC0", rev: 1 },

    // === Stage 11.12 — link1 > 0 (linked-symbol mode) ===
    { barcode: "Hello", link1: 1 },
    { barcode: "Hello", link1: 2 },

    // === Stage 11.13 — start != 257 (custom start codeword) ===
    { barcode: "Hello", start: 258 },
    { barcode: "Hello", start: 261 },

    // === Stage 11.11 — raw=true (raw codewords, ultracode_tiles 0..=284) ===
    { barcode: "^001^002^003", raw: true },
    { barcode: "^000^283^284", raw: true },
    // Longer raw stream to force a different symbol size.
    {
        barcode: "^001^002^003^004^005^006^007^008^009^010^011^012",
        raw: true,
    },

    // === Stage 11.12 — parse=true (^NNN ordinal escape parsing) ===
    { barcode: "^065BC", parse: true },
    { barcode: "^065^066^067", parse: true },
    { barcode: "X^TABY", parse: true },
    { barcode: "FOO^^BAR", parse: true },

    // === Stage 11.13 — parsefnc=true (^FNC1/^FNC3 escape parsing) ===
    { barcode: "ABC^FNC1DEF", parsefnc: true },
    { barcode: "^FNC1A^FNC3B", parsefnc: true },
    { barcode: "FOO^^BAR", parsefnc: true },
];

(async () => {
    const out = [];
    for (const item of CORPUS) {
        const opts = { bcid: "ultracode", text: item.barcode };
        if (item.eclevel !== undefined) opts.eclevel = item.eclevel;
        if (item.rev !== undefined) opts.rev = item.rev;
        if (item.link1 !== undefined) opts.link1 = item.link1;
        if (item.start !== undefined) opts.start = item.start;
        if (item.raw !== undefined) opts.raw = item.raw;
        if (item.parse !== undefined) opts.parse = item.parse;
        if (item.parsefnc !== undefined) opts.parsefnc = item.parsefnc;
        try {
            await bwipjs.toBuffer(opts);
            console.error(`expected debug throw for ${JSON.stringify(item)}`);
            process.exit(2);
        } catch (e) {
            if (!e.errorinfo) {
                console.error(`uncaught for ${JSON.stringify(item)}: ${e.message}`);
                process.exit(3);
            }
            out.push({
                barcode: item.barcode,
                eclevel: item.eclevel ?? "EC2",
                rev: item.rev ?? 2,
                link1: item.link1 ?? 0,
                start: item.start ?? 257,
                raw: item.raw ?? false,
                parse: item.parse ?? false,
                parsefnc: item.parsefnc ?? false,
                ...e.errorinfo,
            });
        }
    }
    console.log(JSON.stringify(out, null, 2));
})();
