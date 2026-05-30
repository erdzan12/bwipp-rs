// Dump bwip-js's `dotcode_charmaps` table to stdout as JSON, with all
// marker symbols normalized to the same negative-i16 encoding we use in
// the Rust port. The Rust test compares each row vs the captured JSON.
//
// Usage: node tools/oracle-dotcode-charmaps.js > dotcode-charmaps.json
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Patch the table init so we can capture it before encoding runs.
const ANCHOR = "    $_.charvals = $a([new Map, new Map, new Map]); //#33178";
if (!src.includes(ANCHOR)) throw new Error("anchor not found");
const PATCH = `    {
      const cm = ${"$_"}.dotcode_charmaps.b
        ? ${"$_"}.dotcode_charmaps.b.slice(${"$_"}.dotcode_charmaps.o, ${"$_"}.dotcode_charmaps.o + ${"$_"}.dotcode_charmaps.length)
        : ${"$_"}.dotcode_charmaps;
      // Each entry of cm is a 3-element BWIPP array.
      const dump = [];
      for (const row of cm) {
        const arr = row.b ? row.b.slice(row.o, row.o + row.length) : row;
        dump.push([arr[0], arr[1], arr[2]]);
      }
      // Marker symbols in BWIPP are bound at runtime — they're strings
      // by default, then re-bound to symbol objects after the symbol
      // table init. We want their canonical name.
      const markerNames = {};
      for (const k of Object.keys($_)) {
        if (k.startsWith("dotcode_")) markerNames[k.slice(8)] = ${"$_"}[k];
      }
      // markerNames[name] holds the BWIPP symbol object for each marker.
      // Build a reverse map: symbol-object → "name" (uppercased).
      const symToName = new Map();
      for (const [name, sym] of Object.entries(markerNames)) {
        symToName.set(sym, name.toUpperCase());
      }
      const norm = (v) => {
        if (typeof v === "number") return v;
        if (typeof v === "string") {
          // Either a 1-byte char (we want char code) or a multi-byte
          // "NN" codeword string (return as int).
          if (/^\\d+$/.test(v)) return parseInt(v, 10);
          return v.charCodeAt(0);
        }
        // Marker symbol object — return its name.
        const name = symToName.get(v);
        if (!name) throw new Error("unknown marker: " + JSON.stringify(v));
        return name;
      };
      const out = dump.map((row) => row.map(norm));
      const err = new Error("charmap-dump");
      err.errorinfo = { charmaps: out };
      err.errorname = "bwipp.dumpCharmaps";
      throw err;
    }
    $_.charvals = $a([new Map, new Map, new Map]); //#33178`;
src = src.replace(ANCHOR, PATCH);
const p = patchedPathFor(bp, "bwipjs-dotcode-charmaps-patched.js");
fs.writeFileSync(p, src);

const b = require(p);

(async () => {
  try {
    await b.toBuffer({ bcid: "dotcode", text: "1234", includetext: false });
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e && e.message);
      process.exit(3);
    }
    console.log(JSON.stringify(e.errorinfo, null, 2));
  }
})();
