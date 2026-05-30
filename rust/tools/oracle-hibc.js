// Capture bwip-js raw().sbs for HIBC LIC + PAS variants the Rust crate
// doesn't yet pin byte-for-byte. Run from any cwd:
//
//   $ node rust/tools/oracle-hibc.js
//
// Copy the resulting arrays into hibc.rs alongside the existing
// `encode_code128_matches_bwip_js_raw_sbs` test.

const { findBwipJs } = require("./_bwip_js_path.js");
const bwipp = require(findBwipJs(__dirname));

// bwip-js's `hibccode128` / `hibccode39` accept the raw payload and apply
// their own `+`-prefix + mod-43 check digit before calling the underlying
// Code 128 / Code 39 encoder — exactly what the Rust `format()` helper
// does. Pass payloads WITHOUT a leading `+` to match what the Rust API
// expects.
const cases = [
  { id: "hibc_lic_code39",  sym: "hibccode39",  data: "A99912345/52001510X3", opts: {} },
  { id: "hibc_pas_code39",  sym: "hibccode39",  data: "/EX2501XZ/16D20240115", opts: {} },
];

for (const { id, sym, data, opts } of cases) {
  try {
    const raw = bwipp.raw(sym, data, opts);
    process.stdout.write(`// ${id}: raw.length=${raw.length}, sbs len=${raw[0].sbs.length}\n`);
    process.stdout.write(`// ${Array.from(raw[0].sbs).join(", ")}\n\n`);
  } catch (e) {
    process.stdout.write(`// ${id}: ERROR ${e.message}\n\n`);
  }
}
