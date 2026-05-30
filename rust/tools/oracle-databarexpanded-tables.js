// Dump the static tables (tab174, finderwidths, finderseq,
// checkweights, fillpat, seppad) from bwip-js's databarexpanded
// global init so the Rust port can pin them byte-for-byte without
// re-transcribing from the BWIPP PostScript source.
//
// The function init runs once on first call and stores the tables
// on `bwipp_databarexpanded.globals`. We don't need to encode
// anything — just trigger the init by calling toBuffer with any
// valid input, then read globals.
const { findBwipJs } = require("./_bwip_js_path");
const bwipjs = require(findBwipJs(__dirname));

(async () => {
  // Trigger global init (the encoder for any valid input does it).
  await bwipjs.toBuffer({
    bcid: "databarexpanded",
    text: "(01)90012345678908",
    scale: 1,
    height: 10,
    includetext: false,
  });

  // Now globals are populated. The bwip-js bundle stores them on
  // the function object.
  const g = bwipjs.toBuffer.bwipp_databarexpanded
    ? bwipjs.toBuffer.bwipp_databarexpanded.globals
    : null;
  // bwip-js doesn't expose the inner symbol map — fall back to
  // re-reading the bundled JS source and grepping for the tables.
  const fs = require("fs");
  const src = fs.readFileSync(findBwipJs(__dirname), "utf8");
  const pull = (name) => {
    const re = new RegExp(`\\$_\\.databarexpanded_${name}\\s*=\\s*\\$a\\(\\[([^\\]]+)\\]\\)`);
    const m = src.match(re);
    if (!m) throw new Error(`could not find table ${name}`);
    return m[1].split(",").map((s) => parseInt(s.trim(), 10));
  };
  const tab174 = pull("tab174");
  const finderwidths = pull("finderwidths");
  const checkweights = pull("checkweights");
  const fillpat = pull("fillpat");
  const seppad = pull("seppad");
  // finderseq is array-of-arrays so the simple regex doesn't work.
  // Extract the relevant slice manually.
  const seqRe = /\$_\.databarexpanded_finderseq\s*=\s*\$a\(\[([\s\S]+?)\]\); \/\/#\d+\s+\$_\.databarexpanded_checkweights/;
  const seqMatch = src.match(seqRe);
  if (!seqMatch) throw new Error("could not find finderseq");
  // seqMatch[1] looks like: $a([0, 1]), $a([0, 3, 2]), $a([0, 5, 2, 7]), ...
  const finderseq = [];
  const innerRe = /\$a\(\[([^\]]+)\]\)/g;
  let m;
  while ((m = innerRe.exec(seqMatch[1])) !== null) {
    finderseq.push(m[1].split(",").map((s) => parseInt(s.trim(), 10)));
  }
  console.log(JSON.stringify({
    tab174,
    finderwidths,
    finderseq,
    checkweights,
    fillpat,
    seppad,
  }, null, 2));
})();
