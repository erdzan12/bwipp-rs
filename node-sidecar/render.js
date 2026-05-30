#!/usr/bin/env node
// Stdio bridge: reads a single JSON request from stdin, writes SVG to stdout.
// Request shape: { "bcid": "qrcode", "text": "hello", "options": { "scale": 3, ... } }
// On error: non-zero exit + JSON error on stderr.

const bwipjs = require("bwip-js");

function readStdin() {
  return new Promise((resolve, reject) => {
    let buf = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => (buf += chunk));
    process.stdin.on("end", () => resolve(buf));
    process.stdin.on("error", reject);
  });
}

(async () => {
  try {
    const raw = await readStdin();
    if (!raw.trim()) {
      throw new Error("empty request on stdin");
    }
    const req = JSON.parse(raw);
    if (!req.bcid || typeof req.bcid !== "string") {
      throw new Error("missing or invalid 'bcid' (symbology)");
    }
    if (req.text === undefined || req.text === null) {
      throw new Error("missing 'text' (data to encode)");
    }

    const opts = Object.assign(
      { bcid: req.bcid, text: String(req.text), scale: 3, includetext: false },
      req.options || {},
    );

    const svg = bwipjs.toSVG(opts);
    process.stdout.write(svg);
    process.exit(0);
  } catch (err) {
    process.stderr.write(
      JSON.stringify({ error: err && err.message ? err.message : String(err) }) +
        "\n",
    );
    process.exit(1);
  }
})();
