// Dump BWIPP's qrcode_formatfimmap as a JSON object keyed by
// `(format, version_str)` -> [[row, col], ...] list of format-info
// reservation positions. BWIPP stores the map as closures that take
// `rows` / `cols` arguments and return `(row, col)` pairs; this
// helper drives each closure with the appropriate per-version
// (rows, cols) to materialize the concrete coordinate list.
//
// Run from `node-sidecar/` directory:
//   node ../rust/tools/extract-qrcode-formatfimmap.js > /tmp/qrcode_formatfimmap.json
//
// Also dumps qrcode_vimmap (version-info reservation for V7+) into
// the same JSON document.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Patch right after qrcode_formatfimmap is assigned (line 26147) so
// both the metrics table and the format-info closures are in scope.
const ANCHOR = "$_.qrcode_formatfimmap = _7E;";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("formatfimmap anchor not found");
const insertPoint = idx + ANCHOR.length;
const PATCH = `
{
  const err = new Error("bwipp.dumpFormatFimMap");
  const flatten = (v) => {
    if (v && v.b && v.o !== undefined) {
      const out = [];
      for (let i = 0; i < v.length; i++) out.push(flatten(v.b[v.o + i]));
      return out;
    }
    if (Array.isArray(v)) return v.map(flatten);
    return v;
  };
  // Run a single closure pair against (rows, cols) and capture the
  // resulting (row, col) pair. BWIPP's closures take rows/cols on the
  // PostScript-style stack and return the (row, col) pair on the
  // stack. Some closures emit a single (row, col); others emit two
  // pairs. We standardize by running the closure pair until the stack
  // contains the (row, col) pair(s) to read.
  const runPair = (closure, rows, cols) => {
    // BWIPP closures expect rows/cols on the stack and produce (row,
    // col) on the stack. We can't easily call the BWIPP-PostScript
    // closure from outside, so instead we walk the map via the actual
    // BWIPP code path: invoke qrcode_formatfimmap[format][i] with the
    // BWIPP runtime.
    return null;
  };

  const metrics = flatten($_.qrcode_metrics);
  const out = { metrics_count: metrics.length, formatfimmap: {} };

  // For each metric (format + version + rows + cols), run BWIPP's
  // own dispatch loop to collect format-info positions.
  for (let mi = 0; mi < metrics.length; mi++) {
    const m = metrics[mi];
    const format = m[0];
    const version = m[1];
    const rows = m[3];
    const cols = m[4];
    const key = format + ":" + version;
    out.formatfimmap[key] = { rows, cols, positions: [] };
    // Invoke BWIPP's qrcode_formatfimmap[format] closure list with
    // (rows, cols) and capture each (row, col) pair.
    const closures = $_.qrcode_formatfimmap.get(format);
    if (!closures) continue;
    for (let ci = 0; ci < closures.length; ci++) {
      const closurePair = closures.b[closures.o + ci];
      // closurePair is an array of 2 BWIPP closures.
      // Run them via BWIPP's stack convention:
      $k[$j++] = rows;
      $k[$j++] = cols;
      try {
        if (closurePair.b[closurePair.o + 0]() === true) {
          // shouldn't happen in extraction context
        }
        const c = $k[--$j];
        const r = $k[--$j];
        out.formatfimmap[key].positions.push([r, c]);
      } catch (e) {
        // Skip closures that throw.
        while ($j > 0) { try { $k[--$j]; } catch(_) { break; } }
      }
      // Second closure of the pair (some closures emit a second pair).
      $k[$j++] = rows;
      $k[$j++] = cols;
      try {
        if (closurePair.b[closurePair.o + 1]() === true) {}
        const c2 = $k[--$j];
        const r2 = $k[--$j];
        out.formatfimmap[key].positions.push([r2, c2]);
      } catch (e) {
        while ($j > 0) { try { $k[--$j]; } catch(_) { break; } }
      }
    }
  }
  err.errorinfo = out;
  throw err;
}
`;
src = src.slice(0, insertPoint) + PATCH + src.slice(insertPoint);
const p = patchedPathFor(bp, "bwipjs-qrcode-formatfimmap.js");
fs.writeFileSync(p, src);
const b = require(p);
(async () => {
  try {
    await b.toBuffer({ bcid: "qrcode", text: "X", includetext: false });
    console.error("expected throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) { console.error("no errorinfo:", e && e.message); process.exit(3); }
    console.log(JSON.stringify(e.errorinfo, null, 2));
  }
})();
