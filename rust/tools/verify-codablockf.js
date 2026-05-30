// Render Codablock-F via bwip-js's SVG drawing and extract per-row bar
// run-lengths for byte-exact comparison with bwipp-rs's StackedPattern.
const { findBwipJs } = require("./_bwip_js_path");
const bwipjs = require(findBwipJs(__dirname));

const text = process.argv[2] || "AB";
const columns = parseInt(process.argv[3] || "8", 10);

const svg = bwipjs.toSVG({
  bcid: "codablockf",
  text,
  columns,
  includetext: false,
  paddingwidth: 0,
  paddingheight: 0,
  scale: 1,
  backgroundcolor: "ffffff",
});

// bwip-js renders the bars as a single <path> with fill-rule="evenodd".
// The first sub-path is the bounding rectangle; subsequent sub-paths are
// the SPACES (subtracted from the rectangle via evenodd). We want
// run-length pairs (bar_width, space_width) per row.
const pathMatch = svg.match(/<path d="([^"]+)"/);
if (!pathMatch) {
  console.error("no <path>");
  process.exit(1);
}
const d = pathMatch[1];

// Each sub-path is "M{x1} {y1}L{x1} {y2}L{x2} {y2}L{x2} {y1}Z" forming a
// rectangle (corners listed clockwise or counterclockwise). Extract them.
const re = /M(\d+) (\d+)L(\d+) (\d+)L(\d+) (\d+)L(\d+) (\d+)Z/g;
const subRects = [];
let m;
while ((m = re.exec(d)) !== null) {
  const xs = [+m[1], +m[3], +m[5], +m[7]];
  const ys = [+m[2], +m[4], +m[6], +m[8]];
  subRects.push({
    x1: Math.min(...xs),
    x2: Math.max(...xs),
    y1: Math.min(...ys),
    y2: Math.max(...ys),
  });
}

const outer = subRects[0]; // bounding box
const spaces = subRects.slice(1);
// Group spaces by y range. Each row has its own (y1, y2). Full-height
// spaces span all rows; we still bucket them by their y range — rendered
// row width equals total run sum per (y1, y2) group.
const yKey = r => `${r.y1}-${r.y2}`;
const yGroups = new Map();
for (const r of spaces) {
  const k = yKey(r);
  if (!yGroups.has(k)) yGroups.set(k, []);
  yGroups.get(k).push(r);
}

// To recover per-row bars: a row's bars span its (y1, y2) AND the bars of
// any "wider" y-range that overlaps it. We'll union all spaces that
// overlap each candidate row's y range, then compute run-lengths from x=0.
//
// Codablock-F always has two y ranges per row in the output:
//   * narrow (row only, e.g. y1=12, y2=22 for the bottom row)
//   * full (spans both rows, e.g. y1=1, y2=22) for bars that are at the
//     same x in both rows (start, stop, sometimes padding).
// We bucket the narrow ranges as rows, and merge in full-range bars.
const rowRanges = [];
const ranges = [...yGroups.keys()].map(k => k.split("-").map(Number));
const allSpan = ranges.find(([a, b]) => b - a === outer.y2 - outer.y1 - 2);
// Heuristic: rows are the y-ranges narrower than the symbol height.
const narrowRanges = ranges.filter(r => r !== allSpan);
narrowRanges.sort((a, b) => a[0] - b[0]);

for (const [y1, y2] of narrowRanges) {
  const k1 = `${y1}-${y2}`;
  const myRow = yGroups.get(k1);
  // Include full-height bars (start/stop) that overlap this row's vertical span.
  const fullKeys = [...yGroups.keys()].filter(k => k !== k1).filter(k => {
    const [a, b] = k.split("-").map(Number);
    return a <= y1 && b >= y2;
  });
  const merged = [...myRow];
  for (const fk of fullKeys) merged.push(...yGroups.get(fk));
  merged.sort((a, b) => a.x1 - b.x1);
  rowRanges.push(merged);
}

// Convert each row's space rectangles into a (bar,space,bar,space,...) run-length
// sequence starting from x=0.
const rows = rowRanges.map(spaceRects => {
  const runs = [];
  let cursor = 0;
  for (const sp of spaceRects) {
    if (sp.x1 > cursor) runs.push(sp.x1 - cursor); // bar
    runs.push(sp.x2 - sp.x1); // space
    cursor = sp.x2;
  }
  if (cursor < outer.x2) runs.push(outer.x2 - cursor); // trailing bar
  return runs;
});

console.log(JSON.stringify({ text, columns, rows }, null, 0));
