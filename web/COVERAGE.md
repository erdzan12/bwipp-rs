# Web demo catalog coverage

The **Rust crate** (`rust/`) exposes **169 user-facing catalog IDs**
(every row in [`rust/PORT_STATUS.md`](../rust/PORT_STATUS.md), reachable via
`Symbology::from_id`). The **web workbench's curated picker**
([`web/src/lib/catalog.ts`](src/lib/catalog.ts)) lists **147** of them.

This is a *curated demo list*, not a coverage gap in the engine: the
WASM bundle wraps `Symbology::all()` / `render_svg(from_id(id))`, so the
client **can render any of the 169 IDs** — the 22 below simply aren't given
their own dropdown row. Nothing is silently broken or omitted from the
engine; only the demo's UI list is curated.

`ultracode` — the catalog's one **colour** 2D symbology — **is** in the web
picker (added Stage A8d). It renders client-side in colour: `renderSvg`
returns an SVG whose cells carry the 6-colour `ULTRACODE_PALETTE` fills
(`#00ffff/#ff00ff/#ffff00/#00ff00/#000000/#ffffff`).

## The 22 IDs not in the curated picker (all still renderable via `renderSvg(id)`)

### A. Variants / flavours of a base symbology the picker already lists (17)

Reachable in the demo by selecting the base entry (and, where relevant, its
options), or directly via `renderSvg(id)`:

| Not separately listed | Collapses into / base in picker |
|-----------------------|---------------------------------|
| `azteccodecompact`, `aztecrune` | `azteccode` |
| `datamatrixrectangular`, `datamatrixrectangularextension`, `gs1datamatrixrectangular` | `datamatrix` / `gs1datamatrix` |
| `rectangularmicroqrcode` | `microqrcode` |
| `gs1qrcode`, `gs1dlqrcode` | `qrcode` |
| `gs1dldatamatrix` | `datamatrix` |
| `gs1dotcode` | `dotcode` |
| `ean2`, `ean5` | EAN add-ons (`ean13`/`ean8`) |
| `ean14`, `mands` | GS1-128 / EAN-8 wrappers |
| `hibc_lic_azteccode`, `hibc_lic_datamatrix_rectangular` | HIBC family (other `hibc_*` rows listed) |
| `telepennumeric` | `telepen` |

### B. Standalone symbologies deliberately omitted from the curated demo (5)

Niche encoders kept out of the demo dropdown to keep it focused; each still
renders via `renderSvg(id)` and is fully verified in the crate:

`bc412`, `channelcode`, `posicode`, `planet`, `postnet`.

## Drift guard

`scripts/check-doc-counts.sh` asserts the web picker entry count equals the
number stated here (147) so this document and the catalog can't drift apart
silently. If you add/remove a picker entry, update the count in both places.
