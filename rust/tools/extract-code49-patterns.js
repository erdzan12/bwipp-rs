const fs = require("fs");
const path = require("path");
const src = fs.readFileSync(
  path.join(__dirname, "..", "..", "node-sidecar", "node_modules", "bwip-js", "dist", "bwipp.mjs"),
  "utf8"
);
// Find the patterns line.
const lines = src.split("\n");
let patternsLine = null;
for (const line of lines) {
  if (line.includes("code49_patterns = $a([$a([")) {
    patternsLine = line;
    break;
  }
}
if (!patternsLine) throw new Error("not found");
// Extract just the JS expression (everything after `= ` up to the trailing `;`).
const eqIdx = patternsLine.indexOf("= ") + 2;
let semicolonIdx = patternsLine.lastIndexOf(";");
// trim trailing comments
const expr = patternsLine.slice(eqIdx, semicolonIdx);
// Convert PostScript $a([...]) → JS [...]
const jsExpr = expr.replace(/\$a\(/g, "").replace(/\)/g, "");
const patterns = eval(jsExpr);
console.error("outer:", patterns.length);
console.error("inner0:", patterns[0].length, "inner1:", patterns[1].length);
const fmt = (arr, name) => {
  const lines = ["#[rustfmt::skip]", `pub(crate) const ${name}: [&str; ${arr.length}] = [`];
  for (let i = 0; i < arr.length; i += 8) {
    const slice = arr.slice(i, i + 8);
    lines.push("    " + slice.map(s => `"${s}"`).join(", ") + ",");
  }
  lines.push("];");
  return lines.join("\n");
};
process.stdout.write(fmt(patterns[0], "PATTERNS_0") + "\n\n" + fmt(patterns[1], "PATTERNS_1") + "\n");
