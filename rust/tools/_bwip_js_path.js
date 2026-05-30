// Shared helper: locate bwip-js's bundled Node source and return the
// directory it lives in. All `oracle-*.js` and `verify-*.js` scripts use
// this so they can run from any cwd as long as bwip-js is installed as
// a sibling under `node-sidecar/`.
const fs = require("fs");
const path = require("path");

function findBwipJs(callerDir) {
  const candidates = [
    path.resolve(callerDir, "node_modules/bwip-js/dist/bwip-js-node.js"),
    path.resolve(callerDir, "../../node-sidecar/node_modules/bwip-js/dist/bwip-js-node.js"),
  ];
  const found = candidates.find((p) => fs.existsSync(p));
  if (!found) {
    throw new Error(`bwip-js not found. Tried:\n  ${candidates.join("\n  ")}`);
  }
  return found;
}

// Patched output goes next to bwip-js's dist so its `require()` resolves
// the sibling modules correctly. Caller passes a unique filename so two
// concurrent oracles don't stomp on each other.
function patchedPathFor(bwipJsPath, filename) {
  return path.resolve(path.dirname(bwipJsPath), "../../../" + filename);
}

module.exports = { findBwipJs, patchedPathFor };
