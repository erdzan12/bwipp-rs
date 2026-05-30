// Run hanxin_funmap and dump the 68 (x, y) coordinates per size.
const fs = require("fs");
const path = require("path");
const bundledPath = path.resolve(__dirname, "node_modules/bwip-js/dist/bwip-js-node.js");
let bundled = fs.readFileSync(bundledPath, "utf8");
// Insert dump right after hanxin_funmap is built.
const ANCHOR = "$_.hanxin_funmap = $a([_Cq,";
if (!bundled.includes(ANCHOR)) { console.error("anchor missing"); process.exit(2); }
// Find the matching `]);` line ending and insert after.
const fullLineMatch = bundled.match(/\$_\.hanxin_funmap = \$a\(\[[^\]]+\]\); \/\/#\d+/);
if (!fullLineMatch) { console.error("funmap line not matched"); process.exit(2); }
const fullLine = fullLineMatch[0];
const PATCH = `${fullLine}
{
  // Dump funmap for size=23 (v1) and size=31 (v5).
  const dumps = {};
  for (const size of [23, 31, 41, 99, 189]) {
    const cells = [];
    for (const entry of $_.hanxin_funmap) {
      const pair = [];
      for (const fn of entry) {
        // The stack-based call: push size, call fn, fn either returns true (break)
        // or leaves 2 vals on the stack to be astored as (x, y).
        const j_save = $j;
        $k[$j++] = size;
        const ret = fn();
        if (ret === true) { $j = j_save; continue; }
        // Pop 2 vals from stack
        const y = $k[--$j];
        const x = $k[--$j];
        pair.push([x, y]);
      }
      cells.push(pair);
    }
    dumps[size] = cells;
  }
  const err = new Error("debug");
  err.errorname = "bwipp.debug";
  err.errorinfo = { dumps };
  throw err;
}`;
bundled = bundled.replace(fullLine, PATCH);
const patched = path.resolve(__dirname, "bwipjs-funmap-patched.js");
fs.writeFileSync(patched, bundled);
const bwipjs = require(patched);
(async () => {
  try {
    await bwipjs.toBuffer({ bcid: "hanxin", text: "A" });
    console.error("no exception");
    process.exit(2);
  } catch (e) {
    if (e.errorname === "bwipp.debug") console.log(JSON.stringify(e.errorinfo));
    else console.error("err:", e.message);
  }
})();
