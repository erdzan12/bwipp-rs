// Capture BWIPP's evalsymbol score for each of the 4 mask candidates
// on a given input. The Rust port can verify its evalsymbol port by
// comparing scores per mask.
//
// Usage: node tools/oracle-dotcode-scores.js "A"
const fs = require("fs");
const { findBwipJs, patchedPathFor } = require("./_bwip_js_path");

const bp = findBwipJs(__dirname);
let src = fs.readFileSync(bp, "utf8");

// Anchor: right after `evalsymbol()` returns its score, before the
// `if score > bestscore` comparison. We collect the per-mask scores
// into a global array, then throw at the end of the masks $forall.
const SCORE_ANCHOR = `        $k[$j++] = 'score'; //#34267
        $k[$j++] = $_.pixs; //#34267
        $_.evalsymbol(); //#34267
        var _Qf = $k[--$j]; //#34267
        var _Qg = $k[--$j]; //#34267
        $_[_Qg] = _Qf; //#34267`;
if (!src.includes(SCORE_ANCHOR)) throw new Error("score anchor not found");
const SCORE_PATCH = SCORE_ANCHOR + `
        if (!globalThis.__dc_scores) globalThis.__dc_scores = [];
        globalThis.__dc_scores.push({ mask: $_.mask, score: $_.score });
        if (globalThis.__dc_bestmask === undefined || $_.score > globalThis.__dc_bestscore) {
          globalThis.__dc_bestmask = $_.mask;
          globalThis.__dc_bestscore = $_.score;
        }`;
src = src.replace(SCORE_ANCHOR, SCORE_PATCH);

// And then dump the scores array right after the masks $forall closes
// and BWIPP picks the best.
const END_ANCHOR = `    $_.pixs = $_.bestsym; //#34300`;
if (!src.includes(END_ANCHOR)) throw new Error("end anchor not found");
const END_PATCH = `    {
      const err = new Error("bwipp.debugDotCodeScores");
      err.errorinfo = {
        scores: globalThis.__dc_scores,
        bestMask: globalThis.__dc_bestmask,
        bestScore: globalThis.__dc_bestscore,
        rows: $_.rows,
        columns: $_.columns,
      };
      err.errorname = "bwipp.debugDotCodeScores";
      throw err;
    }
` + END_ANCHOR;
src = src.replace(END_ANCHOR, END_PATCH);

const p = patchedPathFor(bp, "bwipjs-dotcode-scores-patched.js");
fs.writeFileSync(p, src);

const b = require(p);
const text = process.argv[2] || "A";

(async () => {
  // Reset scores/bestmask between runs so successive invocations don't accumulate.
  globalThis.__dc_scores = [];
  globalThis.__dc_bestmask = undefined;
  globalThis.__dc_bestscore = undefined;
  try {
    await b.toBuffer({ bcid: "dotcode", text, includetext: false });
    console.error("expected encoder to throw");
    process.exit(2);
  } catch (e) {
    if (!e.errorinfo) {
      console.error("no errorinfo:", e && e.message);
      process.exit(3);
    }
    console.log(JSON.stringify({ text, ...e.errorinfo }));
  }
})();
