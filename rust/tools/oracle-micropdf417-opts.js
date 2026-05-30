// Capture bwip-js `bwipp_micropdf417` state when the encoder is driven
// with one of the BWIPP-exposed opt-in flags (`ccb`, `cca`, `raw`,
// `parse`, `parsefnc`, `version`, `columns`, `rows`). Each entry in
// CORPUS picks one flag and a representative input so the Rust port
// can pin its byte-for-byte output.
//
// Run from repo root: `node rust/tools/oracle-micropdf417-opts.js`
//
// Emits a JSON record per input with the BWIPP raw codeword stream
// (`datcws`, `cws`), the selected metric (`c`/`r`/`k`), and the
// rendered `pixs` so the Rust port can pin its outputs end-to-end.

"use strict";

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: right after the row-fill loop populates $_.pixs and before
// renmatrix consumes it. Same anchor used by oracle-micropdf417.js.
const ANCHOR = "    var _JL = $_.pixs; //#24734";
if (!bundled.includes(ANCHOR)) {
    throw new Error("anchor `micropdf417 pre-renmatrix #24734` not found in bundle");
}

const fl = (v) =>
    `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;

const PATCH = `    var _JL = $_.pixs; //#24734
    {
      const err = new Error("bwipp.debugMicroPDF417Opts");
      err.errorinfo = {
        datcws: ${fl("$_.datcws")},
        cws: ${fl("$_.cws")},
        c: $_.c, r: $_.r, k: $_.k,
        rapl: $_.rapl, rapc: $_.rapc, rapr: $_.rapr,
        rwid: $_.rwid,
        pixs: ${fl("$_.pixs")},
      };
      err.errorname = "bwipp.debugMicroPDF417Opts";
      throw err;
    }`;

bundled = bundled.replace(ANCHOR, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-micropdf417-opts-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);

// Cover the BWIPP-exposed `bwipp_micropdf417` opt-ins one at a time
// so each Rust port stage can pin its target option's datcws/cws/pixs
// byte-for-byte. Stage 11.4 covers `ccb=true`; Stage 11.5 covers
// `cca=true` (raw `^NNN` codeword input over CC-A metrics).
const CORPUS = [
    // === Stage 11.4 — ccb=true (CC-B byte-mode wrapper) ===
    // Single character — exercises the 1-byte trailing path
    // (pack emits one codeword carrying the raw byte).
    { barcode: "A", flag: "ccb" },
    // 3-byte trailing remainder.
    { barcode: "ABC", flag: "ccb" },
    // 5-byte trailing remainder (just shy of a full group).
    { barcode: "ABCDE", flag: "ccb" },
    // 6 bytes — exactly one full group, `datcws[1] == 924`.
    { barcode: "ABCDEF", flag: "ccb" },
    // 6 + 1 = 7 bytes (one full group + 1 trailing byte).
    { barcode: "ABCDEFG", flag: "ccb" },
    // 12 bytes — exactly two full groups.
    { barcode: "ABCDEFGHIJKL", flag: "ccb" },
    // 16 bytes — two full groups + 4 trailing.
    { barcode: "ABCDEFGHIJKLMNOP", flag: "ccb" },
    // 24 bytes — exactly four full groups; exercises the 924 latch
    // with a longer packed stream (no trailing remainder).
    { barcode: "ABCDEFGHIJKLMNOPQRSTUVWX", flag: "ccb" },

    // === Stage 11.5 — cca=true (raw codewords, CC-A metrics) ===
    // Smallest CC-A symbol: 2 columns × 5 rows, k=4 → 6 datcws fit.
    // 5 raw codewords spread across the GF(929) value range (incl. 0
    // and a value > 255 to ensure no byte truncation).
    { barcode: "^000^123^456^789^900", flag: "cca" },
    // 6 raw codewords — fills the c=2,r=5 metric exactly (no padding).
    { barcode: "^001^002^003^004^005^006", flag: "cca" },
    // 7 codewords — pushes into the next CC-A metric (c=2,r=6,k=4,n=8).
    { barcode: "^010^020^030^040^050^060^070", flag: "cca" },
    // 12 codewords — c=2,r=9,k=6,n=12 (BWIPP's auto-selector iterates
    // c=2 rows before c=3 so this stays in the c=2 layout, exercising
    // the largest 2-column CC-A symbol that still uses k=6 RS).
    {
        barcode:
            "^100^200^300^400^500^600^700^800^900^001^002^003",
        flag: "cca",
    },
    // Boundary: max permitted codeword value 928.
    { barcode: "^928^000^928^000^928", flag: "cca" },

    // === Stage 11.6 — raw=true (raw codewords, non-CCA metrics) ===
    // Single-codeword: smallest non-CCA symbol is c=1, r=11, k=7
    // (capacity 4).
    { barcode: "^900", flag: "raw" },
    // Pair of codewords, including the 902 numeric-mode latch as the
    // first codeword (a common upstream-CCA-emitted shape).
    { barcode: "^902^123", flag: "raw" },
    // 4 codewords — exact fit for c=1,r=11,k=7 (n=4).
    { barcode: "^001^002^003^004", flag: "raw" },
    // 5 codewords — pushes into c=1,r=14 (next non-CCA row).
    { barcode: "^001^002^003^004^005", flag: "raw" },
    // 8 codewords — covers a larger metric (c=2 or c=1,r=20).
    { barcode: "^001^002^003^004^005^006^007^008", flag: "raw" },
    // Max-value codeword and 0 together.
    { barcode: "^000^928^000^928", flag: "raw" },

    // === Stage 11.7 — parse=true (^NNN escape parsing in text input) ===
    // Plain text — no escapes, parse=true must be a no-op.
    { barcode: "ABC", flag: "parse" },
    // ^065 = 'A'. Pure-digit escape, mid-text.
    { barcode: "^065BC", flag: "parse" },
    // Two `^NNN` escapes back-to-back.
    { barcode: "^065^066^067", flag: "parse" },
    // Boundary: max ordinal 255.
    { barcode: "^255AB", flag: "parse" },
    // `^^` is literal caret.
    { barcode: "FOO^^BAR", flag: "parse" },
    // Control name `^TAB` = 9.
    { barcode: "X^TABY", flag: "parse" },
    // Control name `^CR` = 13 (2-char name).
    { barcode: "X^CRY", flag: "parse" },

    // === Stage 11.8 — parsefnc=true (^ECI ordinal escape parsing) ===
    // MicroPDF417's parsefnc dictionary only registers "eci" — FNC1/2/3
    // tokens are not exposed by the symbology (BWIPP fncvals at
    // lines 22462-22466 contains only [parse, parsefnc, eci]).
    // Caret literal `^^` works under parsefnc=true.
    { barcode: "FOO^^BAR", flag: "parsefnc" },
    // ECI 26 (UTF-8) followed by ASCII text.
    { barcode: "^ECI000026ABC", flag: "parsefnc" },
    // ECI 9 (ISO-8859-1) followed by digits.
    { barcode: "^ECI0000091234", flag: "parsefnc" },
    // Plain text — parsefnc=true is a no-op without `^`.
    { barcode: "PDF417", flag: "parsefnc" },
    // Default ECI (3) at start, then text.
    { barcode: "^ECI000003HELLO", flag: "parsefnc" },

    // === Stage 11.9 — version=RxC + columns/rows (explicit symbol size) ===
    // Force specific R×C metrics via the `version` parser. The
    // shortest-fitting metric for "ABC" auto-selects to c=1,r=11; we
    // pick larger explicit sizes so the constraint actually changes
    // the selection.
    { barcode: "ABC", flag: "version", value: "14x1" },
    { barcode: "ABC", flag: "version", value: "20x1" },
    { barcode: "ABCDEFGH", flag: "version", value: "11x2" },
    { barcode: "ABCDEFGH", flag: "version", value: "14x2" },
    // Explicit `columns` only (rows auto-selects).
    { barcode: "ABCDEFGH", flag: "columns", value: 2 },
    // Explicit `rows` only (columns auto-selects).
    { barcode: "ABC", flag: "rows", value: 14 },
    // Both columns and rows set together.
    {
        barcode: "ABCDEFGH",
        flag: "columns_rows",
        columns: 2,
        rows: 11,
    },
];

(async () => {
    const out = [];
    for (const item of CORPUS) {
        const opts = {
            bcid: "micropdf417",
            text: item.barcode,
            includetext: false,
        };
        // Per-flag option construction. Boolean flags (cca/ccb/raw/...)
        // simply set the option to `true`; the sizing flags (version,
        // columns, rows, columns_rows) take a value or paired values.
        if (item.flag === "version") {
            opts.version = item.value;
        } else if (item.flag === "columns") {
            opts.columns = item.value;
        } else if (item.flag === "rows") {
            opts.rows = item.value;
        } else if (item.flag === "columns_rows") {
            opts.columns = item.columns;
            opts.rows = item.rows;
        } else {
            opts[item.flag] = true;
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
            const entry = {
                barcode: item.barcode,
                flag: item.flag,
                ...e.errorinfo,
            };
            if (item.value !== undefined) entry.value = item.value;
            if (item.columns !== undefined) entry.columns = item.columns;
            if (item.rows !== undefined) entry.rows = item.rows;
            out.push(entry);
        }
    }
    console.log(JSON.stringify(out, null, 2));
})();
