// Extract BWIPP's qrcode mode-selector threshold tables + mode-indicator
// strings as Rust source. The threshold tables are 39-row arrays of i16
// (with sentinel value 10000 for "infeasible" cells, matching BWIPP's
// `$_.e`). The qrcode_mids table maps (version-bin × mode) → mode
// indicator bit-string ("0001"/"0010"/etc).
//
// Run from `node-sidecar/` directory:
//   node ../rust/tools/extract-qrcode-mode-tables.js > /tmp/qrcode_mode_tables.txt

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");
const ANCHOR = "$_.qrcode_modeANbeforeE = ";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("anchor not found");
const insertPoint = src.indexOf(";", idx) + 1;
const PATCH = `
{
  const err = new Error("qrcode-mode-tables");
  const flatten = (v) => {
    if (v && v.b && v.o !== undefined) {
      const out = [];
      for (let i = 0; i < v.length; i++) out.push(flatten(v.b[v.o + i]));
      return out;
    }
    if (Array.isArray(v)) return v.map(flatten);
    return v;
  };
  const inflate_mids = (mids) => {
    // qrcode_mids is a 39-row array; each row is a 5-element array of
    // mode-indicator strings.
    const rows = [];
    for (let i = 0; i < mids.length; i++) {
      const row = mids.b[mids.o + i];
      const cells = [];
      for (let j = 0; j < row.length; j++) {
        cells.push(row.b[row.o + j]);
      }
      rows.push(cells);
    }
    return rows;
  };
  err.errorinfo = {
    mode0forceKB: flatten($_.qrcode_mode0forceKB),
    mode0forceA: flatten($_.qrcode_mode0forceA),
    mode0forceN: flatten($_.qrcode_mode0forceN),
    mode0NbeforeB: flatten($_.qrcode_mode0NbeforeB),
    modeBKbeforeB: flatten($_.qrcode_modeBKbeforeB),
    modeBKbeforeA: flatten($_.qrcode_modeBKbeforeA),
    modeBKbeforeN: flatten($_.qrcode_modeBKbeforeN),
    modeBKbeforeE: flatten($_.qrcode_modeBKbeforeE),
    modeBAbeforeK: flatten($_.qrcode_modeBAbeforeK),
    modeBAbeforeB: flatten($_.qrcode_modeBAbeforeB),
    modeBAbeforeN: flatten($_.qrcode_modeBAbeforeN),
    modeBAbeforeE: flatten($_.qrcode_modeBAbeforeE),
    modeBNbeforeK: flatten($_.qrcode_modeBNbeforeK),
    modeBNbeforeB: flatten($_.qrcode_modeBNbeforeB),
    modeBNbeforeA: flatten($_.qrcode_modeBNbeforeA),
    modeBNbeforeE: flatten($_.qrcode_modeBNbeforeE),
    modeANbeforeA: flatten($_.qrcode_modeANbeforeA),
    modeANbeforeB: flatten($_.qrcode_modeANbeforeB),
    modeANbeforeE: flatten($_.qrcode_modeANbeforeE),
    mids: inflate_mids($_.qrcode_mids),
  };
  throw err;
}
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);
const p = patchedPathFor(bp, "bwipjs-qrcode-mode-tables.js");
fs.writeFileSync(p, src);
const b = require(p);
(async () => {
  try {
    await b.toBuffer({ bcid: "qrcode", text: "X", includetext: false });
  } catch (e) {
    if (!e.errorinfo) { console.error("no errorinfo:", e && e.message); process.exit(3); }
    const info = e.errorinfo;
    console.log(JSON.stringify(info, null, 2));
  }
})();
