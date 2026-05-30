// Dump MaxiCode static tables from bwip-js: character maps, module
// position map, mode-latch sequences/lengths, pad codes.
//
// Output JSON keys:
//   charmaps     — 63 × 5 entries: each row's value in modes 0..=4
//   modmap       — 864-entry codeword index → grid position map
//   latchseq     — 5 × 5 sub-arrays of latch byte sequences
//   latchlen     — 5 × 5 length lookups for the same
//   pad_code     — 5-entry pad codeword per mode

const fs = require("fs");
const { findBwipJs } = require("./_bwip_js_path");

const src = fs.readFileSync(findBwipJs(__dirname), "utf8");

const pull = (name) => {
  const re = new RegExp(`\\$_\\.maxicode_${name}\\s*=\\s*\\$a\\(\\[([^\\]]+)\\]\\)`);
  const m = src.match(re);
  if (!m) throw new Error(`could not find table ${name}`);
  return m[1].split(",").map((s) => parseInt(s.trim(), 10));
};

const modmap = pull("modmap");
const pad_code = pull("pad_code");

// latchlen is array-of-arrays of integers — simpler regex form.
const llRe = /\$_\.maxicode_latchlen\s*=\s*\$a\(\[([\s\S]+?)\]\); \/\/#\d+/;
const llMatch = src.match(llRe);
if (!llMatch) throw new Error("could not find latchlen");
const latchlen = [];
const llInner = /\$a\(\[([^\]]+)\]\)/g;
let m;
while ((m = llInner.exec(llMatch[1])) !== null) {
  latchlen.push(m[1].split(",").map((s) => parseInt(s.trim(), 10)));
}

// charmaps and latchseq use mixed string/integer/sentinel content;
// just extract the raw text for documentation, not the parsed value.
const cmRe = /\$_\.maxicode_charmaps\s*=\s*\$a\(\[([\s\S]+?)\]\); \/\/#\d+/;
const cmMatch = src.match(cmRe);
const charmapsRaw = cmMatch ? cmMatch[1].length : 0;

const lsRe = /\$_\.maxicode_latchseq\s*=\s*\$a\(\[([\s\S]+?)\]\); \/\/#\d+/;
const lsMatch = src.match(lsRe);
const latchseqRaw = lsMatch ? lsMatch[1].length : 0;

console.log(JSON.stringify({
  modmap_count: modmap.length,
  modmap,
  pad_code,
  latchlen,
  charmaps_raw_chars: charmapsRaw,
  latchseq_raw_chars: latchseqRaw,
}));
