// Extract the per-bar 4-state encstr produced by bwip-js's auspost
// encoder (i.e. the same "0123" string Postal4Pattern stores as bars).
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bundledPath = findBwipJs(__dirname);
let bundled = fs.readFileSync(bundledPath, "utf8");

// Capture state just after the stop sentinel is written.
const ANCHOR_RE =
  /\$puti\(\$_\.encstr, \$_\.encstr\.length - 2, \$get\(\$_\.auspost_encs, 74\)\); \/\/#16811/;
if (!ANCHOR_RE.test(bundled)) {
  throw new Error("anchor not found");
}
const PATCH = `$puti($_.encstr, $_.encstr.length - 2, $get($_.auspost_encs, 74)); //#16811
        {
          const err = new Error("bwipp.debugAusPost");
          err.errorinfo = {
            encstr: $_.encstr.toString(),
            rscodes: Array.from($_.rscodes.b ? $_.rscodes.b.slice($_.rscodes.o, $_.rscodes.o + $_.rscodes.length) : $_.rscodes),
            checkcode: $_.checkcode.toString(),
          };
          err.errorname = "bwipp.debugAusPost";
          throw err;
        }`;
bundled = bundled.replace(ANCHOR_RE, PATCH);
const patchedPath = patchedPathFor(bundledPath, "bwipjs-auspost-patched.js");
fs.writeFileSync(patchedPath, bundled);

const bwipjs = require(patchedPath);
const text = process.argv[2] || "1112345678";
const custinfoenc = process.argv[3] || "character";

(async () => {
  try {
    await bwipjs.toBuffer({ bcid: "auspost", text, includetext: false, custinfoenc });
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e.message);
      process.exit(3);
    }
    // encstr arrives as a comma-separated list of ASCII byte codes for
    // the "0".."3" digits — decode it back into the readable string our
    // Rust tests compare against.
    const encstr = e.errorinfo.encstr
      .split(",")
      .map((s) => String.fromCharCode(parseInt(s, 10)))
      .join("");
    console.log(JSON.stringify({ text, custinfoenc, encstr }));
  }
})();
