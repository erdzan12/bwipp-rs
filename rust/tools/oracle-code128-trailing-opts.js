// Capture BWIPP `bwipp_code128` cws for the five remaining opt-in
// flags (raw, parse, newencoder, suppressc, unlatchextbeforec) so the
// Rust port can validate output divergence and pin byte-for-byte
// goldens for the inputs in our corpus.

"use strict";

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

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
            const err = new Error("bwipp.debugCode128Trailing");
            err.errorinfo = {
              cws: dumpArr($_.cws),
            };
            err.errorname = "bwipp.debugCode128Trailing";
            throw err;
        }
        if ($_._render) { //#9641
            bwipp_renlinear(); //#9641
        } //#9641`;

bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-code128-trailing-opts-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);

// Compact corpus that pairs the SAME input with DEFAULT options and
// each opt-in flag in turn. If the default cws matches the opt-in
// cws, the option is a no-op for that input (and we can implement it
// by accepting the value without changing the encoder pipeline). If
// they diverge we have to fully port the option's logic.
const INPUTS = [
    // Cases where the auto-encoder picks subset C → suppressc may
    // divert to subset B.
    "0123456789",
    "12345678",
    // Mid-string subset switching candidates.
    "ABC123DEF",
    "ABC1234567890DEF",
    "AB12CD34",
    // Long all-digit string (definitely subset C territory).
    "0123456789012345",
    // Short text.
    "ABCDEF",
    // Letters with leading digits.
    "1A2B3C",
    // ECI/extended ASCII inputs.
    "\xe9\xe8ABC",          // Latin-1 letters + ASCII.
    "\xe9\xe8\xe7123456",   // Latin-1 → digits (unlatchextbeforec territory).
    // Heavy digit/letter mix to exercise newencoder lookahead.
    "12A34B56C78",
    "AB12345678CD",
    // Very long digit run with one letter.
    "1234567890123456A",
    "A1234567890123456",
    // Even-length digits → subset C; odd → mode switch.
    "12345678901",
];
const FLAGS = ["raw", "parse", "newencoder", "suppressc", "unlatchextbeforec"];

async function captureCws(opts) {
    try {
        await bwipjs.toBuffer(opts);
        return { error: "no debug throw" };
    } catch (e) {
        if (!e.errorinfo) return { error: e.message };
        return e.errorinfo;
    }
}

(async () => {
    const out = [];
    for (const text of INPUTS) {
        const baseline = await captureCws({ bcid: "code128", text });
        for (const flag of FLAGS) {
            const opts = { bcid: "code128", text };
            // `raw=true` requires `^NNN` input; skip ASCII inputs.
            if (flag === "raw") continue;
            opts[flag] = true;
            const result = await captureCws(opts);
            out.push({
                text,
                flag,
                baseline_cws: baseline.cws,
                opt_cws: result.cws,
                error: result.error,
                divergent: JSON.stringify(baseline.cws) !== JSON.stringify(result.cws),
            });
        }
    }
    // raw=true needs ^NNN inputs.
    const rawInputs = [
        "^104^65^66^67",          // start B + ABC
        "^103^11^22^33",          // start A + raw codewords
        "^104^65^99^33^44",       // mid-message subset switch
    ];
    for (const text of rawInputs) {
        const opts = { bcid: "code128", text, raw: true };
        const result = await captureCws(opts);
        out.push({
            text,
            flag: "raw",
            opt_cws: result.cws,
            error: result.error,
        });
    }
    console.log(JSON.stringify(out, null, 2));
})();
