# bwip-js oracles

Node scripts that extract reference output from bwip-js for
byte-for-byte comparison with bwipp-rs.

## Available oracles

| Script | Purpose |
|---|---|
| `oracle-auspost.js <text> [enc]` | AusPost `encstr` (4-state bars) + `rscodes` + `checkcode` from the RS-GF(64) ECC. |
| `oracle-codablockf.js <text> <columns>` | Codablock-F `debugcws` codeword sequence. |
| `verify-codablockf.js <text> <columns>` | Codablock-F rendered SVG → per-row bar/space run-lengths. |
| `oracle-databarlimited.js <gs1-input>` | DataBar Limited widths, check character, mod-89 checksum, 46-element sbs. |
| `oracle-databaromni.js <gs1-input>` | DataBar Omni 32-element widths array + mod-79 checksum. |
| `oracle-databarexpanded.js <gs1-input>` | DataBar Expanded `dxw` (per-segment data widths) + `fxw` (finder widths) + `seq` + mod-211 checksum + final `sbs`. |
| `oracle-databarstacked.js <stacked\|stackedomni> <gs1-input>` | DataBar Stacked / Stacked Omni `pixs` + `rowmult` + `pixy`. |
| `oracle-dotcode.js <text>` | DotCode `cws` codeword stream + chosen `mode` / `nd` / `nw` / row / column metrics. |
| `oracle-dotcode-charmaps.js` | Full 113×3 charmaps table dump (no args). |
| `oracle-dotcode-rscws.js <text> [mask]` | Post-RS codeword stream (`rscws`) + mask + dimensions. |
| `oracle-dotcode-bits.js <text> [mask]` | Assembled `bits` string + mask + dimensions + rembits. |
| `oracle-dotcode-pixs.js <text> [mask]` | Rendered pixs grid (mask=0 path or explicit mask) + dimensions. |
| `oracle-dotcode-scores.js <text>` | Per-mask `evalsymbol` score for the 4 mask candidates + the mask BWIPP picks. |
| `oracle-micropdf417.js <text> [cols] [rows]` | MicroPDF417 `datcws` / `cws` / metrics / `pixs`. |
| `oracle-onecode.js <text>` | USPS OneCode intermediate bytes, FCS, and 10 codewords. |
| `oracle-pdf417.js <text> [eclevel]` | PDF417 `datcws` + generator `coeffs` + full `cws`. |
| `oracle-pdf417-truncated.js <text> [eclevel]` | PDF417 Compact (truncated) `pixs` + `cws` + dimensions. |

All scripts monkey-patch bwip-js's bundled source so that
`bwipp_raiseerror` attaches its payload to the thrown error — the
default form drops the codeword array.

## Usage

```sh
# Works from any cwd; the helper finds bwip-js automatically.
node /path/to/rust/tools/oracle-codablockf.js "AB" 8
node /path/to/rust/tools/verify-codablockf.js "AB" 8
```

The scripts share a tiny helper, `_bwip_js_path.js`, that locates bwip-js
in two candidate locations (legacy `tools/node_modules/bwip-js`, current
`node-sidecar/node_modules/bwip-js`). The patched bundle is written next
to bwip-js's own `dist/` so its `require()` resolves sibling modules
correctly; the patched files are gitignored under
`node-sidecar/bwipjs-*-patched.js` / `node-sidecar/bwipp-patched.js`.

The bwip-js version captured in the existing goldens is the one bundled
in the repo's `node-sidecar/node_modules`
(2026-03-31 vendor snapshot). Re-running against a newer bwip-js may
produce different codewords / bars if the BWIPP encoder is updated
upstream.
