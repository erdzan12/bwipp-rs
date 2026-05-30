// Extract bwip-js's qrcode_metrics table and emit Rust source for
// FULL_METRICS: [VersionMetric; 76]. Used at qrcode_native Stage 2b.
//
// Run from `node-sidecar/` directory:
//   node ../rust/tools/extract-qrcode-metrics.js > /tmp/qrcode_metrics_rust.txt
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");
const ANCHOR = "        $_.qrcode_metrics = ";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("anchor not found");
const insertPoint = src.indexOf(";", idx) + 1;
const PATCH = `
        {
          const err = new Error("bwipp.debugQrcodeMetrics");
          const flatten = (v) => {
            if (v && v.b && v.o !== undefined) {
              const out = [];
              for (let i = 0; i < v.length; i++) out.push(flatten(v.b[v.o + i]));
              return out;
            }
            if (Array.isArray(v)) return v.map(flatten);
            return v;
          };
          err.errorinfo = { metrics: flatten($_.qrcode_metrics) };
          err.errorname = "bwipp.debugQrcodeMetrics"; throw err;
        }`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);
const p = patchedPathFor(bp, "bwipjs-qrcode-extract-patched.js");
fs.writeFileSync(p, src);
const b = require(p);
(async () => {
  try {
    await b.toBuffer({ bcid: "qrcode", text: "HELLO", includetext: false });
    console.error("expected throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) { console.error("no errorinfo:", e && e.message); process.exit(3); }
    const metrics = e.errorinfo.metrics;
    console.log(`pub(crate) const FULL_METRICS: [VersionMetric; ${metrics.length}] = [`);
    for (const row of metrics) {
      const [fmt, ver, layout, rows, cols, fimax, fimas, datacap, eclen, blocks] = row;
      const fmtVariant = fmt === "full" ? "Full" : fmt === "micro" ? "Micro" : "Rmqr";
      const eclenStr = `[${eclen.join(", ")}]`;
      const blocksStr = `[${blocks.join(", ")}]`;
      console.log(`    VersionMetric { format: Format::${fmtVariant}, version_str: "${ver}", layout_id: ${typeof layout === 'number' ? layout : 0}, rows: ${rows}, cols: ${cols}, fimax: ${fimax}, fimas: ${fimas}, datacap_bits: ${datacap}, eclen: ${eclenStr}, blocks: ${blocksStr} },`);
    }
    console.log(`];`);
  }
})();
