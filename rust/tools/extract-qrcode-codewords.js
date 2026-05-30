// Dump BWIPP's post-RS interleaved codeword stream ($_.cws after the
// blocking + interleave step) for a chosen (text, version, eclevel)
// triple. Patches bwip-js to throw a normal Error with `errorinfo`
// populated at the `debugecc` checkpoint, because BWIPP's native
// raiseerror discards non-string `info` payloads into the unexported
// bwipp_error map.
//
// Run from `node-sidecar/` directory:
//   node ../rust/tools/extract-qrcode-codewords.js
//
// Output is JSON to stdout: { "label": { cws: [...] } }. Stage 4b
// pins build_codeword_stream() against the dumped vectors
// byte-for-byte.

const fs = require("fs");
const path = require("path");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);

// Patch right at the qrcode-specific `debugecc` raiseerror site. The
// bwip-js dist contains 11 `debugecc` checkpoints across different
// barcode encoders; the qrcode one is tagged with BWIPP source-line
// comment `#27577`. The original code is:
//   if ($has($_.options, 'debugecc')) {
//     $k[$j++] = "bwipp.debugecc#27577";
//     $k[$j++] = $_.cws;
//     bwipp_raiseerror();
//   }
// We splice in a custom throw that builds a real Error with
// `errorinfo` set to a JS array copy of $_.cws (which is a typed
// PostScript array, BWIPP-style {b, o, length}).
let src = fs.readFileSync(bp, "utf8");
const ANCHOR = "bwipp.debugecc#27577";
const idx = src.indexOf(ANCHOR);
if (idx < 0) throw new Error("qrcode debugecc anchor (bwipp.debugecc#27577) not found");
// Walk backward to the `if (...)` start, forward to the matching `}`.
const ifIdx = src.lastIndexOf("if (", idx);
if (ifIdx < 0) throw new Error("debugecc if-block start not found");
const startBrace = src.indexOf("{", ifIdx);
const endBrace = src.indexOf("} //#", startBrace);
if (startBrace < 0 || endBrace < 0) throw new Error("debugecc block bounds not found");
const PATCH_BODY = `
{
  const err = new Error("bwipp.debugecc");
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
  err.errorname = "bwipp.debugecc";
  throw err;
}`;
// endBrace points at the `}` of `} //#`. Cut everything from the `if (`
// up through that brace, replace with our PATCH_BODY (a bare block
// that always throws).
const blockEnd = endBrace + 1; // include the `}`
src = src.slice(0, ifIdx) + PATCH_BODY + src.slice(blockEnd);

const patched = patchedPathFor(bp, "bwipjs-qrcode-codewords-patched.js");
fs.writeFileSync(patched, src);
const b = require(patched);

async function dump(label, opts) {
  try {
    // BWIPP auto-upgrades eclevel when the message fits a higher level
    // unless `fixedeclevel: true` is passed (see bwip-js #27437).
    // We force the requested eclevel so the oracle's dcws / dmod /
    // block layout matches what we'd pass to build_codeword_stream().
    await b.toBuffer({ fixedeclevel: true, ...opts, debugecc: true, includetext: false });
    return { label, error: "expected throw" };
  } catch (e) {
    if (e && e.errorinfo) {
      const cws = e.errorinfo.cws;
      return { label, opts, cws };
    }
    return { label, error: e && e.message };
  }
}

(async () => {
  const samples = [
    { label: "v1_l_HELLO_WORLD", opts: { bcid: "qrcode", text: "HELLO WORLD", eclevel: "L", version: "1" } },
    { label: "v1_m_01234567",   opts: { bcid: "qrcode", text: "01234567",    eclevel: "M", version: "1" } },
    { label: "v5_q_HELLO_WORLD",opts: { bcid: "qrcode", text: "HELLO WORLD HELLO WORLD HELLO WORLD", eclevel: "Q", version: "5" } },
    { label: "m1_12345",        opts: { bcid: "microqrcode", text: "12345" } },
    { label: "r7x43_1234",      opts: { bcid: "rectangularmicroqrcode", text: "1234", version: "R7x43" } },
  ];
  const results = {};
  for (const s of samples) {
    results[s.label] = await dump(s.label, s.opts);
  }
  console.log(JSON.stringify(results, null, 2));
})();
