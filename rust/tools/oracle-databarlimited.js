// Extract intermediate state from bwip-js's databarlimited encoder.
// Captures the d1w/d2w widths, checkwidths, checksum/seq, and final sbs.
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Anchor: right after the `$_.sbs = $a();` line that closes out
// databarlimited's sbs build (matches the unique surrounding context).
const ANCHOR_RE =
  /\$aload\(\$_\.d2w\); \/\/#12782\s+\$k\[\$j\+\+\] = 1; \/\/#12783\s+\$k\[\$j\+\+\] = 1; \/\/#12783\s+\$k\[\$j\+\+\] = 5; \/\/#12783\s+\$_\.sbs = \$a\(\); \/\/#12783/;
if (!ANCHOR_RE.test(bundled)) {
  throw new Error("databarlimited anchor not found");
}
const flatten = (v) =>
  `Array.from(${v}.b ? ${v}.b.slice(${v}.o, ${v}.o + ${v}.length) : ${v})`;
const PATCH = `$aload($_.d2w); //#12782
        $k[$j++] = 1; //#12783
        $k[$j++] = 1; //#12783
        $k[$j++] = 5; //#12783
        $_.sbs = $a(); //#12783
        {
          const err = new Error("bwipp.debugdataBarLimited");
          err.errorinfo = {
            d1: $_.d1, d2: $_.d2, checksum: $_.checksum, seq: $_.seq,
            d1w: ${flatten("$_.d1w")},
            d2w: ${flatten("$_.d2w")},
            checkwidths: ${flatten("$_.checkwidths")},
            widths: ${flatten("$_.widths")},
            sbs: ${flatten("$_.sbs")},
          };
          err.errorname = "bwipp.debugdataBarLimited";
          throw err;
        }`;
bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-databarlimited-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);
const text = process.argv[2] || "(01)15012345678907";

(async () => {
  try {
    await bwipjs.toBuffer({ bcid: "databarlimited", text, includetext: false });
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e.message);
      process.exit(3);
    }
    console.log(JSON.stringify({ text, ...e.errorinfo }));
  }
})();
