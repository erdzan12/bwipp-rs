// Capture BWIPP `bwipp_channelcode` sbs output with shortfinder=true
// and/or includecheck=true. Default-options goldens already live in
// channelcode::tests; this harness adds the two opt-in variants.
//
// Run from repo root: `node rust/tools/oracle-channelcode-opts.js`

"use strict";

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: just before bwipp_renlinear inside bwipp_channelcode. The
// canonical hook point is the `$_._render` guard at the end of the
// encoder.
const ANCHOR_RE = /if \(\$_\._render\) \{ \/\/#42107\s+bwipp_renlinear\(\); \/\/#42107\s+\} \/\/#42107/;
if (!ANCHOR_RE.test(bundled)) {
    throw new Error("anchor `channelcode pre-renlinear #42107` not found in bundle");
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
            const err = new Error("bwipp.debugChannelcode");
            err.errorinfo = {
              sbs: dumpArr($_.sbs),
              finder: dumpArr($_.finder),
              data: dumpArr($_.data),
              check: dumpArr($_.check),
              shortfinder: $_.shortfinder,
              includecheck: $_.includecheck,
            };
            err.errorname = "bwipp.debugChannelcode";
            throw err;
        }
        if ($_._render) { //#42107
            bwipp_renlinear(); //#42107
        } //#42107`;

bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-channelcode-opts-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);

// Cover both opt-ins across barcode lengths 2..=7 so the per-barlen
// mod23 weighting table is exercised in includecheck cases.
const CORPUS = [
    // shortfinder=true alone.
    { barcode: "12", shortfinder: true, includecheck: false },
    { barcode: "00", shortfinder: true, includecheck: false },
    { barcode: "128", shortfinder: true, includecheck: false },
    { barcode: "00000", shortfinder: true, includecheck: false },
    { barcode: "26", shortfinder: true, includecheck: false }, // boundary: max 2-digit
    // includecheck=true alone — exercises per-barlen mod23 row.
    { barcode: "12", shortfinder: false, includecheck: true },
    { barcode: "128", shortfinder: false, includecheck: true },
    { barcode: "1234", shortfinder: false, includecheck: true },
    { barcode: "12345", shortfinder: false, includecheck: true },
    { barcode: "123456", shortfinder: false, includecheck: true },
    { barcode: "1234567", shortfinder: false, includecheck: true },
    // Both options together.
    { barcode: "12345", shortfinder: true, includecheck: true },
    { barcode: "1234567", shortfinder: true, includecheck: true },
];

(async () => {
    const out = [];
    for (const item of CORPUS) {
        const opts = {
            bcid: "channelcode",
            text: item.barcode,
            shortfinder: item.shortfinder,
            includecheck: item.includecheck,
        };
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
                shortfinder: item.shortfinder,
                includecheck: item.includecheck,
                ...e.errorinfo,
            });
        }
    }
    console.log(JSON.stringify(out, null, 2));
})();
