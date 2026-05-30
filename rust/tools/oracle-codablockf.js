// Extract Codablock-F codeword sequence from bwip-js as a golden oracle.
//
// Strategy: bwip-js's `debugcws` option raises an error whose payload is
// the internal codeword array, but the public throw drops the payload.
// We patch the bundled module's `bwipp_raiseerror` function (which lives
// in a closure but is exposed indirectly through `bwipp_error` — itself
// closure-private). Workaround: edit the eval'd source on the fly.
//
// Usage: node oracle-codablockf.js <text> <columns>

const fs = require("fs");
const Module = require("module");
const path = require("path");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

// Patch bwipp.js so raiseerror attaches the info to the thrown value.
// srcPath = `${bwipJsPackageDir}/src/bwipp.js` (alongside `dist/`).
const bundledPath0 = findBwipJs(__dirname);
const bwipPkgDir = path.resolve(path.dirname(bundledPath0), "..");
const srcPath = path.resolve(bwipPkgDir, "src/bwipp.js");
let src = fs.readFileSync(srcPath, "utf8");
const before = `bwipp_error.set('errorinfo', info);

    if (typeof info == 'string' || info instanceof Uint8Array) {`;
const after = `bwipp_error.set('errorinfo', info);

    if (typeof info == 'string' || info instanceof Uint8Array) {`;
if (!src.includes(before)) {
  throw new Error("source did not match expected pattern");
}
src = src.replace(
  /var info = \$k\[--\$j\];\s+var name = \$k\[--\$j\];\s+bwipp_error\.set\('errorname', name\);\s+bwipp_error\.set\('errorinfo', info\);/,
  `var info = $k[--$j];
    var name = $k[--$j];
    bwipp_error.set('errorname', name);
    bwipp_error.set('errorinfo', info);
    // PATCH for oracle: always throw an Error with the info attached.
    {
      const err = new Error(String(name));
      err.errorinfo = info;
      err.errorname = name;
      throw err;
    }`
);

// Write patched source to a sibling file and require it.
const patchedPath = patchedPathFor(bundledPath0, "bwipp-patched.js");
fs.writeFileSync(patchedPath, src);

// bwip-js dist/bwip-js-node.js is the high-level wrapper that imports the
// PS-port. We have to invoke the encoder directly because the wrapper
// catches errors and re-throws them as bwip-js exceptions.
//
// Easier path: import the bundled bwip-js, but stub out its raiseerror via
// a trick: bwip-js-node.js inlines bwipp.js, so we patch the bundled
// source the same way.

const bundledPath = bundledPath0;
let bundled = fs.readFileSync(bundledPath, "utf8");
const ORIG_RAISE = `bwipp_error.set('errorinfo', info);

    if (typeof info == 'string' || info instanceof Uint8Array) {
        throw new Error($z(name) + ": " + $z(info));
    } else {
        throw $z(name);
    }`;
if (!bundled.includes(ORIG_RAISE)) {
  throw new Error("bundled source did not match expected raiseerror");
}
bundled = bundled.replace(
  ORIG_RAISE,
  `bwipp_error.set('errorinfo', info);
    {
      const err = new Error(String(name));
      err.errorinfo = info;
      err.errorname = name;
      throw err;
    }`
);
const patchedBundledPath = patchedPathFor(bundledPath0, "bwipjs-node-patched.js");
fs.writeFileSync(patchedBundledPath, bundled);

const bwipjs = require(patchedBundledPath);

const text = process.argv[2] || "AB";
const columns = parseInt(process.argv[3] || "8", 10);

(async () => {
  try {
    await bwipjs.toBuffer({
      bcid: "codablockf",
      text,
      columns,
      debugcws: true,
      includetext: false,
    });
    console.error("expected debugcws to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo on thrown error:", e.message);
      process.exit(3);
    }
    // errorinfo is the cws array — bwip-js's postscript $a stores data
    // in `.b` (a hidden backing array) and `.o` (offset). geti() returns
    // a sparse view; the live values are in .b starting at .o.
    const info = e.errorinfo;
    const o = info.o || 0;
    const backing = info.b || info;
    const cws = [];
    for (let i = o; i < o + info.length; i++) cws.push(backing[i]);
    console.log(JSON.stringify({ text, columns, cws_length: cws.length, cws }, null, 0));
  }
})();
