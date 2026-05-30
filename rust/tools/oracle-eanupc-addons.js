// Capture bwip-js raw().sbs for the four EAN/UPC add-on combinations
// that the Rust crate doesn't yet pin byte-for-byte. Run from repo
// root or rust/tools/:
//
//   $ cd node-sidecar && npm install   # ensures bwip-js is present
//   $ node rust/tools/oracle-eanupc-addons.js
//
// Output is human-readable; copy the arrays into ean_combined.rs.

const path = require("path");
const { findBwipJs } = require("./_bwip_js_path.js");
const bwipPath = findBwipJs(__dirname);
const bwipp = require(bwipPath);

const cases = [
  { id: "ean8p2",  sym: "ean8", data: "1234567 12",        gap: 9,  opts: { permitaddon: true } },
  { id: "ean8p5",  sym: "ean8", data: "1234567 12345",     gap: 9,  opts: { permitaddon: true } },
  { id: "upcap5",  sym: "upca", data: "01234567890 12345", gap: 12, opts: {} },
  { id: "upcep5",  sym: "upce", data: "01234565 12345",    gap: 12, opts: {} },
];

for (const { id, sym, data, gap, opts } of cases) {
  const raw = bwipp.raw(sym, data, opts);
  process.stdout.write(`// ${id}: raw.length=${raw.length}\n`);
  raw.forEach((row, idx) => {
    process.stdout.write(`//   raw[${idx}].sbs (len=${row.sbs.length}): ${Array.from(row.sbs).join(", ")}\n`);
  });
  const main = Array.from(raw[0].sbs);
  const addon = raw[1] ? Array.from(raw[1].sbs) : [];
  const combined = addon.length > 0 ? [...main, gap, ...addon] : main;
  process.stdout.write(`// ${id} combined sbs (len=${combined.length}, gap=${gap}): ${combined.join(", ")}\n\n`);
}
