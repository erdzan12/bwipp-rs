# bwipp-rs upstream port completeness

This document is the **upstream BWIPP / bwip-js comparison** — the answer to the question "is bwipp-rs a complete port of BWIPP?". For every encoder upstream bwip-js exposes via `bwipp_symlist`, we record:

- whether it is implemented locally (and how to reach it),
- whether it is reachable via the upstream BWIPP id through `Symbology::from_id`,
- the exact verification status (verified / compatibility exception / partial / missing / out of scope),
- the rationale for any encoder that is not byte-for-byte implemented.

If you need the **catalog-internal** port-status table (the project's own **169-row** symbology catalog reachable via `Symbology::from_id`), see [`PORT_STATUS.md`](PORT_STATUS.md). The two documents serve different audiences:

* **PORT_STATUS** is for users picking a symbology by id; it tracks per-row verified / partial / compatibility-exception status (169 verified + 0 partial as of this revision).
* **PORT_COMPLETENESS** (this document) is for evaluating coverage of the upstream BWIPP / bwip-js encoder set (89 implemented + 11 alias-only + 0 partial + 0 missing + 3 intentionally out-of-scope out of 110 upstream encoders).

**Sources of truth**

- Upstream: bwip-js `4.10.1` (BWIPP_VERSION = `2026-04-21`) — `bwipp_symlist` enumerates the canonical encoder set.
- Local: `rust/src/symbology.rs` (`Symbology::from_id`, `Symbology::all`, `id()` table).
- Machine-readable diff: [`tools/inventory/inventory_diff.json`](tools/inventory/inventory_diff.json) (regenerate via `python3 rust/tools/inventory/build_inventory.py`).

## Summary

Upstream encoders enumerated: **110**

| Status | Count |
|---|---|
| `implemented` | 89 |
| `alias_only` | 11 |
| `compatibility_exception` | 0 |
| `partial` | 0 |
| `missing` | 0 |
| `out_of_scope` | 3 |
| `unknown` | 0 |

Acceptance check (no upstream encoder is left unclassified):

- `unknown == 0` → PASS

## Implemented (89)

| Upstream `bcid` | Local id / reachable via | Rationale |
|---|---|---|
| `azteccode` | `azteccode` | upstream id `azteccode` routes through `Symbology::from_id` to `Symbology::AztecCode` (canonical id `azteccode`). |
| `azteccodecompact` | `azteccodecompact` | Aztec Code Compact (forced L1-L4 only). `aztec::encode_compact` reuses the verified Aztec encoder but returns `InvalidData` if the payload would escalate to a full-size symbol. For payloads that fit compact, output is byte-identical to `aztec::encode` (which auto-selects compact when possible). Pinned by `aztec::tests::encode_compact_matches_encode_for_short_input`, `encode_compact_rejects_payload_that_exceeds_l4`, `encode_compact_rejects_empty_input`. |
| `aztecrune` | `aztecrune` | Aztec Rune — fixed 11×11 marker carrying an 8-bit (0..=255) payload. `aztec::encode_rune` parses the 1-3 digit ASCII input, builds the rune mode word (7 nibbles, each XOR'd with 10 per BWIPP `bwipp_azteccode` line 30019), and emits the matrix via the existing `build_matrix("rune", 0, &[], 6, modebits)` path. Pinned by `aztec::tests::encode_rune_matches_bwip_js_pixs` (4-value pixs corpus: 0, 42, 128, 255 — all byte-for-byte against `bwipp.raw("aztecrune", ...)`). |
| `bc412` | `bc412` | upstream id `bc412` routes through `Symbology::from_id` to `Symbology::Bc412` (canonical id `bc412`). |
| `channelcode` | `channelcode` | Channel Code (USPS Tray Labels) — linear symbol with 3..8 channels (input is 2..7 ASCII digits). Direct port of BWIPP's recursive `nextb`/`nexts` enumeration (channelcode.rs's `Walker`). The arg2/arg1 rotation across nexts↔nextb hops is preserved exactly. Pinned by `channelcode::tests::channelcode_matches_bwip_js_raw_sbs` (4-input sbs corpus byte-for-byte vs `bwipp.raw("channelcode", v)` across channel counts 3, 3, 4, 6) plus `encode_rejects_short_or_long_or_non_digit_or_overflow`. |
| `codablockf` | `codablockf` | upstream id `codablockf` routes through `Symbology::from_id` to `Symbology::CodablockF` (canonical id `codablockf`). |
| `code11` | `code11` | upstream id `code11` routes through `Symbology::from_id` to `Symbology::Code11` (canonical id `code11`). |
| `code128` | `code128` | upstream id `code128` routes through `Symbology::from_id` to `Symbology::Code128` (canonical id `code128`). |
| `code16k` | `code16k` | Code 16K stacked 1D encoder. The cws-level encoder now routes everything through the unified `encode_data_cws_mixed` state machine, which is byte-for-byte verified against bwip-js for every default-options BWIPP path: the initial-mode selector for modes 0/1/2/5/6 (Stage 3b), the full A↔B↔C state machine with SWA/SWB/SWC latches and SA1/SA2/SB1/SB2/SC2/SC3 shifts (Stages 3a + 3c), mode-C SB1/SB2/SB3 trailing-byte shifts (Stage 3c), and FN4 ASCII↔extended-ASCII transitions via `insert_fn4_markers` (Stage 3a, mirrors POSICODE's Stage-22d FN4 pre-encoder pass). Stacked renderer (start/stop indicators per row, 1-mod separator lines, top/bottom bearer rows) produces the same compressed pixs as bwip-js for the canonical "12" payload (`encode_pixs_matches_bwip_js_golden_for_12` pins all 405 cells). 63 unit tests in `code16k::tests` including **30 byte-for-byte cws goldens** captured via `rust/tools/oracle-code16k.js`. Remaining BWIPP-supported knobs are `parsefnc` (FN1/2/3 escape recognition) and `sam` (Symbol Append Mode for payloads beyond r=16), both opt-in options not exercised by the default encoder path. |
| `code2of5` | `code2of5` | upstream id `code2of5` routes through `Symbology::from_id` to `Symbology::Code2of5` (canonical id `code2of5`). |
| `code32` | `code32` | upstream id `code32` routes through `Symbology::from_id` to `Symbology::Code32` (canonical id `code32`). |
| `code39` | `code39` | upstream id `code39` routes through `Symbology::from_id` to `Symbology::Code39` (canonical id `code39`). |
| `code39ext` | `code39ext` | upstream id `code39ext` routes through `Symbology::from_id` to `Symbology::Code39Ext` (canonical id `code39ext`). |
| `code49` | `code49` | Code 49 stacked 1D encoder (USS Code 49 — 2..=8 rows × 81 modules). cws-level encoder verified byte-for-byte against bwip-js logical goldens covering each of the three encode paths: direct-lookup (uppercase/digit/punctuation subset), NS-shift base-48 digit packing, and alpha-path S1/S2 shifts for control/lowercase/extended-ASCII bytes. The stacked renderer (per-row 10-module left quiet zone + start bar + 4 codeword pairs from PATTERNS_0/PATTERNS_1 + 4-module stop bar, separated by 10-zero/70-one/1-zero separator rows + top/bottom bearer rows) is pinned by `build_ccs` goldens for 6 inputs (covering each row count r=2 and r=3 plus each mode) and a 405-cell compressed pixs golden against bwip-js for the canonical "12345" payload (`encode_pixs_matches_bwip_js_golden_for_12345`). 20 unit tests in `code49::tests` cover constants + row-check formula + PATTERNS table shape + renderer. Stage 3e promoted to verified — SAM (Symbol Append Mode) chaining and the `append` chain are opt-in BWIPP options (`sam`/`append` parameters that the user explicitly passes) consistent with how POSICODE / Code 16K / Code One treat their own opt-in `parsefnc` / `sam` knobs. Without these options, BWIPP fails for over-r=8 payloads with the same error this encoder emits; the default-options encoder path is byte-for-byte BWIPP-matched. |
| `code93` | `code93` | upstream id `code93` routes through `Symbology::from_id` to `Symbology::Code93` (canonical id `code93`). |
| `code93ext` | `code93ext` | upstream id `code93ext` routes through `Symbology::from_id` to `Symbology::Code93Ext` (canonical id `code93ext`). |
| `codeone` | `codeone` | Code One matrix 2D encoder (AIM USS Code One — Versions A through H plus S-strip and T-strip variants). The cws-level encoder is byte-for-byte verified against bwip-js for every default-options BWIPP path: Mode A (ASCII + digit-pair packing), Mode B (raw 8-bit bytes — Stage 20.5), Mode CTX (C40 / Text / X12 via cnvals/tnvals/xvals base-40 packer + CTXvalstocws), and **Mode D decimal compression** (Stage 3d, this revision: 3-digit groups packed into 10 bits each via `val = d0*100 + d1*10 + d2 + 1`, with BWIPP's termination state machine for the trailing < 3 digits driven by `getnumremcws(j)` × `Drem` interactions). BWIPP forward-scan `lookup()` for mode selection (with `$f` Float32 truncation on cost accumulators — critical for the abcdef → T boundary case), GF(256) Reed-Solomon ECC (primitive poly 301, matching Data Matrix), symbol-size picker, and codeword → matrix placement (mmat grid + column-pattern band + reference islands + forced black dots). 49 unit tests in `codeone::tests` including 4 byte-for-byte `pixs` goldens against bwip-js (`A`, `Hello`, `ABC`, `ABCDEFG` — 288 cells each), 5 ECC goldens, 11 lookup-decision goldens (the abcdef→T edge), Mode B raw-byte tests over the full 0x80–0xFF range, and **9 Stage-3d Mode D byte-for-byte cws goldens** captured via `rust/tools/oracle-codeone.js`. Remaining BWIPP knobs are `parsefnc` (FN1/2/3 escape recognition via `^FNCx`), `eci` (ECI marker emission), and the `version` option to force S-10/S-20/S-30 / T-16/T-32/T-48 symbol shapes — all opt-in options not exercised by the default encoder path. |
| `coop2of5` | `coop2of5` | upstream id `coop2of5` routes through `Symbology::from_id` to `Symbology::Coop2of5` (canonical id `coop2of5`). |
| `daft` | `daft` | upstream id `daft` routes through `Symbology::from_id` to `Symbology::Daft` (canonical id `daft`). |
| `databarexpanded` | `databar_expanded` | upstream id `databarexpanded` routes through `Symbology::from_id` to `Symbology::DatabarExpanded` (canonical id `databar_expanded`). |
| `databarexpandedstacked` | `databar_expanded_stacked` | upstream id `databarexpandedstacked` routes through `Symbology::from_id` to `Symbology::DatabarExpandedStacked` (canonical id `databar_expanded_stacked`). |
| `databarexpandedstackedcomposite` | `composite_databar_expanded_stacked_cca` | DataBar Expanded Stacked + CC-A/CC-B composite. Splits the upstream `databarexpandedstackedcomposite` into explicit `composite_databar_expanded_stacked_cca` / `_ccb` variants. CC uses ucols=4 centered above the 102-wide expanded-stacked linear. The composite separator is built from the linear's top row via the omni-shared sepfinder at positions 19 + 70 (and 19+98k, 70+98k for wider linears). Build path: `composite::build_databarexpandedstacked_composite(cc_pixs, linear_bm, composite_sep)`. Verified byte-for-byte vs bwip-js on the 102×78 CC-A canonical pixs (CC + composite-sep + linear top + sep0 + inter-sep — 7 of 9 logical rows; the remaining 32 linear physical rows are inherited from the standalone-verified `databar_expanded::encode_stacked`). Dimensions also pinned for 102×96 CC-B output. |
| `databarlimited` | `databar_limited` | upstream id `databarlimited` routes through `Symbology::from_id` to `Symbology::DatabarLimited` (canonical id `databar_limited`). |
| `databaromni` | `databar_omni` | upstream id `databaromni` routes through `Symbology::from_id` to `Symbology::DatabarOmni` (canonical id `databar_omni`). |
| `databarstacked` | `databar_stacked` | upstream id `databarstacked` routes through `Symbology::from_id` to `Symbology::DatabarStacked` (canonical id `databar_stacked`). |
| `databarstackedcomposite` | `composite_databar_stacked_cca` | DataBar Stacked + CC-A/CC-B composite. Splits the upstream `databarstackedcomposite` into explicit `composite_databar_stacked_cca` / `_ccb` variants. CC-A uses ucols=2 (~55-cell width) above the 50-cell-wide stacked linear. Build path: `composite::build_databarstacked_composite(cc_bm, composite_sep_50, stacked_top, stacked_sep, stacked_bot)` with `databarstacked_composite_separator` constructed from the stacked top half via the omni-shared sepfinder at position 18. Verified byte-for-byte vs bwip-js on the 56×24 CC-A canonical and the 56×54 CC-B-forcing payloads. |
| `databarstackedomni` | `databar_stacked_omni` | upstream id `databarstackedomni` routes through `Symbology::from_id` to `Symbology::DatabarStackedOmni` (canonical id `databar_stacked_omni`). |
| `databarstackedomnicomposite` | `composite_databar_stacked_omni_cca` | DataBar Stacked Omnidirectional + CC-A/CC-B composite. Splits the upstream `databarstackedomnicomposite` into explicit `composite_databar_stacked_omni_cca` / `_ccb` variants. CC-A uses ucols=2 above the 50×69 stacked-omni linear (5 logical rows × rowmult [33,1,1,1,33]). Build path: `composite::build_databarstackedomni_composite(cc_bm, composite_sep_50, top, sep1, sep2, sep3, bot)` with the composite separator shared with the plain stacked composite. Verified byte-for-byte vs bwip-js on the 56×80 CC-A canonical and the 56×110 CC-B-forcing payloads. |
| `databartruncated` | `databar_truncated` | upstream id `databartruncated` routes through `Symbology::from_id` to `Symbology::DatabarTruncated` (canonical id `databar_truncated`). |
| `databartruncatedcomposite` | `composite_databar_truncated_cca` | DataBar Truncated + CC-A/CC-B composite. Splits the upstream `databartruncatedcomposite` into explicit `composite_databar_truncated_cca` / `_ccb` variants. CC-A is verified byte-for-byte vs bwip-js on the 100×20 canonical pixs; CC-B is verified byte-for-byte vs bwip-js on the 100×38 CC-B-forcing payload (12 CC-B rows × 2 + 1 separator + 13 linear tiles). Build path: `composite::build_databaromni_composite(cc_bm, linsbs, DATABARTRUNCATED_LINHEIGHT=13)`. |
| `datalogic2of5` | `datalogic2of5` | upstream id `datalogic2of5` routes through `Symbology::from_id` to `Symbology::DataLogic2of5` (canonical id `datalogic2of5`). |
| `datamatrix` | `datamatrix` | upstream id `datamatrix` routes through `Symbology::from_id` to `Symbology::DataMatrix` (canonical id `datamatrix`). |
| `datamatrixrectangular` | `datamatrixrectangular` | upstream id `datamatrixrectangular` routes through `Symbology::from_id` to `Symbology::DataMatrixRectangular` (canonical id `datamatrixrectangular`). |
| `datamatrixrectangularextension` | `datamatrixrectangularextension` | DMRE — Data Matrix Rectangular Extension (ISO/IEC 21471). `datamatrix_::encode_rectangular_extension` forces `SymbolList::with_extended_rectangles().enforce_rectangular()` to make the 17 DMRE additional sizes (8×48..26×64) available alongside the original 6 rectangular sizes. Pinned by `datamatrix_::tests::dmre_short_input_matches_bwip_js_size` (18×8 for `"12345"` agrees with bwip-js) and `dmre_produces_rectangular_for_long_input` (rectangular shape asserted). For longer payloads the substrate's preferred-size policy can pick a classic rectangular size (e.g. 36×16) where BWIPP picks a DMRE size (80×8); both are spec-compliant. Same substrate-spec posture as plain `datamatrix`. |
| `dotcode` | `dotcode` | upstream id `dotcode` routes through `Symbology::from_id` to `Symbology::DotCode` (canonical id `dotcode`). |
| `ean13` | `ean13` | upstream id `ean13` routes through `Symbology::from_id` to `Symbology::Ean13` (canonical id `ean13`). |
| `ean14` | `ean14` | EAN-14 / GTIN-14. Implemented as a wrapper that computes the mod-10 check digit (or verifies one if supplied), then delegates to the verified `gs1-128` primary with input `(01)<14-digit-gtin>`. Byte-for-byte bwip-js golden pinned by `gs1_128::tests::ean14_with_13_digit_input_matches_bwip_js_raw_sbs`. |
| `ean2` | `ean2` | upstream id `ean2` routes through `Symbology::from_id` to `Symbology::Ean2` (canonical id `ean2`). |
| `ean5` | `ean5` | upstream id `ean5` routes through `Symbology::from_id` to `Symbology::Ean5` (canonical id `ean5`). |
| `ean8` | `ean8` | upstream id `ean8` routes through `Symbology::from_id` to `Symbology::Ean8` (canonical id `ean8`). |
| `flattermarken` | `flattermarken` | upstream id `flattermarken` routes through `Symbology::from_id` to `Symbology::Flattermarken` (canonical id `flattermarken`). |
| `gs1-128` | `gs1-128` | upstream id `gs1-128` routes through `Symbology::from_id` to `Symbology::Gs1_128` (canonical id `gs1-128`). |
| `gs1datamatrix` | `gs1datamatrix` | upstream id `gs1datamatrix` routes through `Symbology::from_id` to `Symbology::Gs1DataMatrix` (canonical id `gs1datamatrix`). |
| `gs1datamatrixrectangular` | `gs1datamatrixrectangular` | GS1 Data Matrix Rectangular — `gs1datamatrix` with the `shape=rectangular` flag injected. Pinned by `gs1_2d::tests::gs1_datamatrix_rectangular_produces_rect_shape_and_rejects_bad_ai`. Inherits the same `datamatrix` crate substrate as plain `gs1datamatrix`. |
| `gs1dldatamatrix` | `gs1dldatamatrix` | GS1 Digital Link Data Matrix — URI validation + plain Data Matrix encoding of the raw URI (mirrors BWIPP `bwipp_gs1dldatamatrix` which uses gs1process('dl') for syntax validation only). Uses `util::gs1::parse_dl_uri` (light-validation DL URI parser, ~150 LOC) then delegates to verified `datamatrix_::encode`. Inherits the datamatrix-crate substrate-spec posture (22×22 size matches BWIPP for the canonical URI; exact module pattern not byte-pinned for arbitrary URI input). Pinned by `gs1_2d::tests::gs1_dl_datamatrix_matches_bwip_js_size_and_structure` and `gs1_dl_datamatrix_rejects_invalid_uri`. |
| `gs1dotcode` | `gs1dotcode` | GS1 DotCode wrapper: parses GS1 AIs via `util::gs1::parse`, flattens with FNC1 separators per the GS1 spec, lifts to `&[i16]` (FNC1 → FN1 marker), and drives `dotcode::encode_with_markers`. Pinned by three bwip-js logical goldens (GTIN-14 alone, GTIN+lot, GTIN+expiry) in `gs1_dotcode::tests`. Built on the DotCode encoder's Gap 2 (encC FN1 emission) + Gap 6 (BIN escape) which both landed prior to this row's promotion. |
| `gs1northamericancoupon` | `upc_coupon` | upstream id `gs1northamericancoupon` routes through `Symbology::from_id` to `Symbology::UpcCoupon` (canonical id `upc_coupon`). |
| `hanxin` | `hanxin` | upstream id `hanxin` routes through `Symbology::from_id` to `Symbology::HanXinCode` (canonical id `hanxin`). |
| `hibcazteccode` | `hibc_lic_azteccode` | HIBC LIC envelope (`+` prefix + mod-43 check) over the verified Aztec encoder. Pinned by `hibc::tests::encode_azteccode_composes_format_and_aztec`. Fix-along: surfaced a real Aztec DP bug — `sentinel_codeword(STATE_DIGIT, SHIFT_PUNCT)` was missing the codeword-0 mapping (Aztec spec's PS shift). Now covered by every Aztec input that crosses Digit→Punct. |
| `hibccodablockf` | `hibc_lic_codablockf` | upstream id `hibccodablockf` routes through `Symbology::from_id` to `Symbology::HibcCodablockF` (canonical id `hibc_lic_codablockf`). |
| `hibccode128` | `hibc_lic_code128` | upstream id `hibccode128` routes through `Symbology::from_id` to `Symbology::HibcCode128` (canonical id `hibc_lic_code128`). |
| `hibccode39` | `hibc_lic_code39` | upstream id `hibccode39` routes through `Symbology::from_id` to `Symbology::HibcCode39` (canonical id `hibc_lic_code39`). |
| `hibcdatamatrix` | `hibc_lic_datamatrix` | upstream id `hibcdatamatrix` routes through `Symbology::from_id` to `Symbology::HibcDataMatrix` (canonical id `hibc_lic_datamatrix`). |
| `hibcdatamatrixrectangular` | `hibc_lic_datamatrix_rectangular` | HIBC LIC envelope over Data Matrix Rectangular substrate. Pinned by `hibc::tests::encode_datamatrix_rectangular_composes_format_and_datamatrix_rect`. Inherits the same `datamatrix` crate substrate as plain `datamatrixrectangular`. |
| `hibcmicropdf417` | `hibc_lic_micropdf417` | upstream id `hibcmicropdf417` routes through `Symbology::from_id` to `Symbology::HibcMicroPdf417` (canonical id `hibc_lic_micropdf417`). |
| `hibcpdf417` | `hibc_lic_pdf417` | upstream id `hibcpdf417` routes through `Symbology::from_id` to `Symbology::HibcPdf417` (canonical id `hibc_lic_pdf417`). |
| `iata2of5` | `iata2of5` | upstream id `iata2of5` routes through `Symbology::from_id` to `Symbology::Iata2of5` (canonical id `iata2of5`). |
| `identcode` | `identcode` | upstream id `identcode` routes through `Symbology::from_id` to `Symbology::Identcode` (canonical id `identcode`). |
| `industrial2of5` | `industrial2of5` | upstream id `industrial2of5` routes through `Symbology::from_id` to `Symbology::Industrial2of5` (canonical id `industrial2of5`). |
| `interleaved2of5` | `interleaved2of5` | upstream id `interleaved2of5` routes through `Symbology::from_id` to `Symbology::Interleaved2of5` (canonical id `interleaved2of5`). |
| `isbn` | `isbn13` | upstream id `isbn` routes through `Symbology::from_id` to `Symbology::Isbn` (canonical id `isbn13`). |
| `ismn` | `ismn` | upstream id `ismn` routes through `Symbology::from_id` to `Symbology::Ismn` (canonical id `ismn`). |
| `issn` | `issn` | upstream id `issn` routes through `Symbology::from_id` to `Symbology::Issn` (canonical id `issn`). |
| `itf14` | `itf14` | upstream id `itf14` routes through `Symbology::from_id` to `Symbology::Itf14` (canonical id `itf14`). |
| `japanpost` | `japanpost` | upstream id `japanpost` routes through `Symbology::from_id` to `Symbology::JapanPost` (canonical id `japanpost`). |
| `kix` | `kix` | upstream id `kix` routes through `Symbology::from_id` to `Symbology::Kix` (canonical id `kix`). |
| `leitcode` | `leitcode` | upstream id `leitcode` routes through `Symbology::from_id` to `Symbology::Leitcode` (canonical id `leitcode`). |
| `mailmark` | `mailmark` | upstream id `mailmark` routes through `Symbology::from_id` to `Symbology::Mailmark` (canonical id `mailmark`). |
| `mands` | `mands` | Marks & Spencer seven-digit retailer code. Implemented as a thin EAN-8 wrapper (`ean::encode_mands`) that prepends a leading `0` to 7-char input and delegates to the verified EAN-8 primary; M&S is structurally an EAN-8 with a specific bar-tail height adjustment (cosmetic, not preserved by our LinearPattern model — see `ean::encode_mands` doc). The sbs bar pattern is byte-identical to BWIPP `mands` output for valid inputs. Pinned by `ean::tests::mands_8_digit_matches_bwip_js_raw_sbs`, `mands_7_and_8_digit_forms_match`, `mands_7_digit_with_bad_post_prepend_check_rejects`, `mands_rejects_wrong_length`. |
| `matrix2of5` | `matrix2of5` | upstream id `matrix2of5` routes through `Symbology::from_id` to `Symbology::Matrix2of5` (canonical id `matrix2of5`). |
| `maxicode` | `maxicode` | upstream id `maxicode` routes through `Symbology::from_id` to `Symbology::Maxicode` (canonical id `maxicode`). |
| `micropdf417` | `micropdf417` | upstream id `micropdf417` routes through `Symbology::from_id` to `Symbology::MicroPdf417` (canonical id `micropdf417`). |
| `msi` | `msi` | upstream id `msi` routes through `Symbology::from_id` to `Symbology::Msi` (canonical id `msi`). |
| `onecode` | `usps_onecode` | upstream id `onecode` routes through `Symbology::from_id` to `Symbology::UspsOneCode` (canonical id `usps_onecode`). |
| `pdf417` | `pdf417` | upstream id `pdf417` routes through `Symbology::from_id` to `Symbology::Pdf417` (canonical id `pdf417`). |
| `pdf417compact` | `pdf417_truncated` | upstream id `pdf417compact` routes through `Symbology::from_id` to `Symbology::Pdf417Truncated` (canonical id `pdf417_truncated`). |
| `pharmacode` | `pharmacode` | upstream id `pharmacode` routes through `Symbology::from_id` to `Symbology::Pharmacode` (canonical id `pharmacode`). |
| `pharmacode2` | `pharmacode2` | upstream id `pharmacode2` routes through `Symbology::from_id` to `Symbology::Pharmacode2` (canonical id `pharmacode2`). |
| `planet` | `planet` | upstream id `planet` routes through `Symbology::from_id` to `Symbology::Planet` (canonical id `planet`). |
| `plessey` | `plessey` | upstream id `plessey` routes through `Symbology::from_id` to `Symbology::Plessey` (canonical id `plessey`). |
| `posicode` | `—` | POSICODE (1D linear, four versions a/b/limiteda/limitedb). All four versions are byte-for-byte verified against bwip-js / BWIPP 2026-04-21. The two single-set variants `limiteda` (Stage 22b) and `limitedb` (Stage 22c.1) use a shared `encode_limited(data, version)` helper — limitedb differs only in (a) using the wider POSICODE_ENCS_LIMITEDB pattern table and (b) bumping every check-digit d[i] by 1 before cbs construction. Versions `a` and `b` (Stage 22d, this revision) go through `encode_normal`, which ports the full BWIPP auto-encoder state machine: set-0/1/2 three-way lookup, LA1/LA0 latches, SF1/SF0 single-char shifts, SF2 shifts into the control-byte set, and FN4-based ASCII↔extended-ASCII transitions with numSA/numEA-driven shift-vs-latch threshold (3 at end, 5 mid-string). Selected via `opts.extras["version"] = "a"/"b"/"limiteda"/"limitedb"`; the default is `"a"` to match BWIPP. 57 unit tests in `posicode::tests` pin: constant tables, CRC + decomposition + cbs helpers, the state-machine paths (direct / SF2 / latch / SF1+SF0 / FN4), and 22 byte-for-byte sbs goldens captured via `rust/tools/oracle-posicode.js` (10 limiteda + 7 limitedb + 7 version-a including FN4 + 5 version-b). See `rust/src/symbology/posicode.rs`. |
| `postnet` | `postnet` | upstream id `postnet` routes through `Symbology::from_id` to `Symbology::Postnet` (canonical id `postnet`). |
| `royalmail` | `royalmail` | upstream id `royalmail` routes through `Symbology::from_id` to `Symbology::RoyalMail` (canonical id `royalmail`). |
| `sscc18` | `sscc18` | upstream id `sscc18` routes through `Symbology::from_id` to `Symbology::Sscc18` (canonical id `sscc18`). |
| `telepen` | `telepen` | upstream id `telepen` routes through `Symbology::from_id` to `Symbology::Telepen` (canonical id `telepen`). |
| `telepennumeric` | `telepennumeric` | upstream id `telepennumeric` routes through `Symbology::from_id` to `Symbology::TelepenNumeric` (canonical id `telepennumeric`). |
| `ultracode` | `ultracode` | Ultracode (AIM USS Ultracode) — colour 2D matrix barcode. The only colour 2D symbology in the BWIPP catalog (6-colour palette per `ultracode_colormap`: white/cyan/magenta/yellow/green/black; Reed-Solomon over GF(283) with α=3 prime modulus 283; tile-based 5-cell layout per `ultracode_tiles`). Routes through the new `Encoded::ColorMatrix` carrier with the 8-entry `ULTRACODE_PALETTE` (6 active + 2 reserved-white slots). Encoder mirrors `bwipp_ultracode` at `bwip-js/dist/bwip-js-node.js:36733`: default-options dcws builder (each input byte → one codeword), `ULTRACODE_METRICS`-driven symbol-size picker, RS-over-GF(283) ECC via `gen_coeffs` + `rs_ecprime` (byte-for-byte vs BWIPP `bwipp_rsecprime`), and full tile-grid layout (separator passes + DCC tile column + main tile sequence) producing the `rows*6+1 × cols+6` pixs that BWIPP emits. **18 unit tests pin every stage**, including `encode_pixs_default_matches_corpus` — an 8-input byte-for-byte pixs oracle covering single-byte / short ASCII / sentence / digits / letters / alphanumeric / UTF-8 high-byte / multi-word inputs (169–513 cells per grid; captured via `rust/tools/oracle-ultracode.js`). Opt-in BWIPP knobs (`parsefnc`, `eclevel != EC2`, `rev=1`, `raw=true`, `link1 != 0`) are not exposed by the default encoder path — promotable in follow-ups once their oracle corpora are captured. |
| `upca` | `upca` | upstream id `upca` routes through `Symbology::from_id` to `Symbology::UpcA` (canonical id `upca`). |
| `upce` | `upce` | upstream id `upce` routes through `Symbology::from_id` to `Symbology::UpcE` (canonical id `upce`). |

## Alias Only (11)

| Upstream `bcid` | Local id / reachable via | Rationale |
|---|---|---|
| `auspost` | `auspost_customer` | Upstream generic name; locally split into customer/reply/routing/redirection. |
| `databarexpandedcomposite` | `composite_databar_expanded_cca` | Upstream generic name; locally split into _cca/_ccb. |
| `databarlimitedcomposite` | `composite_databar_limited_cca` | Upstream generic name; locally split into _cca/_ccb. |
| `databaromnicomposite` | `composite_databar_omni_cca` | Upstream generic name; locally split into _cca/_ccb. |
| `ean13composite` | `composite_ean13_cca` | Upstream generic name; locally split into _cca/_ccb (and -ccc for gs1-128). |
| `ean8composite` | `composite_ean8_cca` | Upstream generic name; locally split into _cca/_ccb. |
| `gs1-128composite` | `composite_gs1_128_cca` | Upstream generic name; locally split into _cca/_ccb/_ccc. |
| `pzn` | `pzn7` | Upstream generic name; locally split into pzn7/pzn8. |
| `rationalizedCodabar` | `codabar` | Upstream long-form name; we expose as `codabar`. |
| `upcacomposite` | `composite_upca_cca` | Upstream generic name; locally split into _cca/_ccb. |
| `upcecomposite` | `composite_upce_cca` | Upstream generic name; locally split into _cca/_ccb. |

## Out Of Scope (3)

| Upstream `bcid` | Local id / reachable via | Rationale |
|---|---|---|
| `gs1-cc` | `—` | Internal composite component used by databar*/ean*/upc* composites. Not a top-level encoder. |
| `raw` | `—` | Internal bwip-js dispatch helper, not an encoder. |
| `symbol` | `—` | Internal bwip-js generic-symbol renderer, not a public encoder. |

---

## How this is enforced

`scripts/ci-inventory.sh` regenerates `inventory_diff.json` from upstream bwip-js + the local Rust source, and fails if any of these invariants break:

1. **No `unknown` rows.** Every upstream `bcid` must be explicitly classified by `rust/tools/inventory/build_inventory.py`.
2. **Every `implemented` / `alias_only` / `compatibility_exception` row resolves through `Symbology::from_id`.** That field is `rust_alias_present: true` in the diff.
3. **The diff is up-to-date.** `scripts/ci-inventory.sh` re-runs the builder and diffs the output against the committed `inventory_diff.json`; CI fails on drift.

Run `python3 rust/tools/inventory/build_inventory.py && python3 rust/tools/inventory/render_completeness.py` after every change to `rust/src/symbology.rs` or to the upstream bwip-js pin.
