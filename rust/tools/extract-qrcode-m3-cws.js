// Dump BWIPP's post-RS cws stream for M3-L HELLO12 (i.e. the final cws
// AFTER the lc4b fixup).

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

const ANCHOR = "bwipp.debugecc#27577";
const idx = src.indexOf(ANCHOR);
const ifIdx = src.lastIndexOf("if (", idx);
const startBrace = src.indexOf("{", ifIdx);
const endBrace = src.indexOf("} //#", startBrace);
const PATCH = `
{
  const err = new Error("debugecc");
  const flatten = (v) => {
    if (v && v.b && v.o !== undefined) {
      const out = [];
      for (let i = 0; i < v.length; i++) out.push(flatten(v.b[v.o + i]));
      return out;
    }
    if (Array.isArray(v)) return v.map(flatten);
    return v;
  };
  err.errorinfo = { cws: flatten($_.cws) };
  throw err;
}`;
src = src.slice(0, ifIdx) + PATCH + src.slice(endBrace + 1);

const patched = patchedPathFor(bp, "bwipjs-m3-cws.js");
fs.writeFileSync(patched, src);
const b = require(patched);

(async () => {
  try {
    await b.toBuffer({
      bcid: "microqrcode",
      text: "HELLO12",
      eclevel: "L",
      version: "M3",
      fixedeclevel: true,
      debugecc: true,
      includetext: false,
    });
  } catch (e) {
    if (e && e.errorinfo) {
      console.log(JSON.stringify(e.errorinfo.cws));
    } else {
      process.stderr.write("error: " + (e && e.message) + "\n");
      process.exit(1);
    }
  }
})();
