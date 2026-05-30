// Dump BWIPP's pixs[181] (= row 8 col 13) at multiple checkpoints
// to find exactly where (8, 13) gets set.

const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");
const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// We patch every place that calls $put on pixs to log if it's writing to
// pixs[181]. Add a wrapper around $put or inject at specific lines.

// Inject right after `$_.pixs = $a();` (line 27829) — patch $put usage.
// Actually simpler: replace ALL $put(....pixs, ...) calls with a wrapper
// would require careful regex. Let me instead add a checkpoint at known
// places.

// Checkpoint 1: after finder placement (around line 27945).
// Checkpoint 2: after format-info reservation (line 28049).
// Checkpoint 3: after dark-module pre-init (line 28092).
// Checkpoint 4: right before walker init (line 28130).

const checkpoints = [
  { tag: "after_finders", anchor: "$_.putalgnpat = function() {" },
  { tag: "after_algnpat", anchor: "$_.formatmap = $a();" },
  { tag: "after_formatmap_write", anchor: "$_.versionmap = $a([]);" },
  { tag: "after_dark_preinit", anchor: "if ($_.mask == -1)" },
  { tag: "after_masklayers", anchor: "$_.posx = $f($_.cols - _VX)" },
];

for (const { tag, anchor } of checkpoints) {
  const idx = src.indexOf(anchor);
  if (idx < 0) throw new Error(`anchor not found: ${tag}`);
  const PATCH = `\nprocess.stdout.write("CHECKPOINT ${tag}: pixs[181]=" + $get($_.pixs, 181) + "\\n");\n`;
  src = src.slice(0, idx) + PATCH + src.slice(idx);
}

const patched = patchedPathFor(bp, "bwipjs-pixs-181.js");
fs.writeFileSync(patched, src);
const b = require(patched);

(async () => {
  try {
    await b.toBuffer({
      bcid: "qrcode",
      text: "HELLO WORLD",
      eclevel: "L",
      version: "1",
      fixedeclevel: true,
      mask: "1",
      includetext: false,
    });
  } catch (e) {
    process.stderr.write("error: " + (e && e.message) + "\n");
  }
})();
