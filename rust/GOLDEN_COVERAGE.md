# bwipp-rs golden-coverage matrix

This document is the per-symbology evidence ledger. For every catalog
row in [`PORT_STATUS.md`](PORT_STATUS.md) it answers:

* What kind of output does it produce? (Linear / Matrix / Postal4 /
  Stacked / Hex / Composite / Dots)
* What proves it matches BWIPP / bwip-js? (Logical golden against an
  oracle — bar-space run lengths, codewords, module patterns, mask
  scores — vs. wrapper proof over a verified primary encoder vs.
  substrate spec-compliance.)
* Which test function(s) pin the claim?
* Which oracle script regenerates the fixture, if applicable?

The verification-strength taxonomy mirrors [`AUDIT.md`](AUDIT.md):

| Strength            | Meaning |
|---------------------|---------|
| **bwip-js logical golden** | `raw().sbs` bar-pattern, codewords, or per-module pixs byte-match. |
| **BWIPP/PostScript logical golden** | Same, but captured from Ghostscript-rendered BWIPP output. |
| **wrapper proof** | Pinned routing to a verified primary encoder (HIBC, EAN add-on combinator, postal alias). Payload-transformation tests + delegation tests. |
| **substrate spec proof** | Uses an upstream spec-compliant Rust crate (`qrcode`, `datamatrix`) whose module pattern can tie-break differently from BWIPP. Symbol size, ec level, and decoded payload are pinned; raw module pattern is **not** required to byte-match. |
| **compatibility exception** | Documented divergence with a precise rationale and pinned regression tests. See [`COMPATIBILITY_EXCEPTIONS.md`](COMPATIBILITY_EXCEPTIONS.md). |

Substrate spec proof is weaker than logical golden — by design — and
this matrix calls out every row that relies on it. The audit's
position is that for most catalog inputs, the substrate's module
pattern matches BWIPP; only tie-broken cases diverge, and where they
do (today, only `gs1qrcode`) we lift the row to a compatibility
exception.

---

## How to read the per-family tables

* **Output**: `Linear` = `Encoded::Linear(LinearPattern)`; `Matrix` =
  `Encoded::Matrix(BitMatrix)`; `Postal4` = `Encoded::Postal4State`;
  `Stacked` = `Encoded::Stacked`; `Hex` = `Encoded::Hex(MaxiCodeSymbol)`;
  `Dots` = `Encoded::Dots(DotMatrix)`; `Composite` = `Stacked` over a
  Linear + 2D pair.
* **Reach**: CLI / WASM (raw-pointer bridge + wasm-bindgen) / Web
  (Vercel `web/`). Every catalog row resolves through
  `Symbology::from_id` after the alias additions made during this
  audit, so the reach column is collapsed to a single "✓ all" unless
  there's a per-surface caveat.
* **Tests**: function names under `rust/src/symbology/<family>.rs`
  (unit tests next to the encoder) or `rust/tests/integration.rs`.
  Names are clickable when run via `cargo test <name>`.
* **Oracle**: regeneration script under `rust/tools/` if any; "(inline
  bwip-js)" if the golden bytes were captured from bwip-js but live
  inline in the test module as a constant array.

---

## 1. 1D – Standard linear (verified, bwip-js logical golden)

| Catalog id        | Output | Tests | Oracle |
|-------------------|--------|-------|--------|
| `code39`          | Linear | `code39::tests::matches_bwip_js_raw_sbs`, `matches_bwipp_with_check_and_punct` | (inline bwip-js) |
| `code39ext`       | Linear | `code39ext::tests::sbs_matches_bwipp` | (inline bwip-js) |
| `code93`          | Linear | `code93::tests::matches_bwip_js_raw_sbs`, `matches_bwip_js_various_inputs`, `matches_bwip_js_with_check` | (inline bwip-js) |
| `code93ext`       | Linear | `code93ext::tests::sbs_matches_bwipp` | (inline bwip-js) |
| `code128`         | Linear | `code128::tests::matches_bwip_js_raw_sbs`, `matches_bwip_js_mixed_paths`, `matches_bwip_js_subset_paths` | (inline bwip-js) |
| `code128a/b/c`    | Linear | alias to verified primary | `from_id` routes all three to `Symbology::Code128`, pinned by `integration::alias_ids_route_to_canonical_symbology`. Auto-subset selection covered by the `code128::tests::matches_bwip_js_subset_paths` corpus (byte-for-byte bwip-js logical golden). | (inline bwip-js) |
| `code11`          | Linear | `code11::tests::sbs_matches_bwipp_without_check`, `sbs_matches_bwipp_with_check` | (inline bwip-js) |
| `bc412`           | Linear | `bc412::tests::matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `code32`          | Linear | `code32::tests::matches_bwip_js_raw_sbs` | (inline bwip-js) |

## 2. 1D – 2-of-5 family (verified, bwip-js logical golden)

| Catalog id          | Output | Tests | Oracle |
|---------------------|--------|-------|--------|
| `code2of5`          | Linear | `twoofive::tests::variants_match_bwip_js_raw_sbs` (standard) | (inline bwip-js) |
| `datalogic2of5`     | Linear | `twoofive::tests::variants_match_bwip_js_raw_sbs` (`version: "datalogic"` arm) | — |
| `iata2of5`          | Linear | `twoofive::tests::variants_match_bwip_js_raw_sbs` (`version: "iata"` arm) | — |
| `industrial2of5`    | Linear | `twoofive::tests::variants_match_bwip_js_raw_sbs` (`version: "industrial"` arm) | — |
| `matrix2of5`        | Linear | `twoofive::tests::variants_match_bwip_js_raw_sbs` (`version: "matrix"` arm) | — |
| `coop2of5`          | Linear | `twoofive::tests::variants_match_bwip_js_raw_sbs` (`version: "coop"` arm) | — |
| `interleaved2of5`   | Linear | `interleaved2of5::tests::matches_bwip_js_raw_sbs`, `matches_bwip_js_more_payloads` | (inline bwip-js) |
| `itf14`             | Linear | `interleaved2of5::tests::itf14_matches_bwip_js_raw_sbs` | (inline bwip-js) |

## 3. 1D – Specialized + Pharmaceutical + ISBN/Media

| Catalog id     | Output | Strength | Tests | Oracle |
|----------------|--------|----------|-------|--------|
| `msi`          | Linear | bwip-js golden | `msi::tests::matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `plessey`      | Linear | bwip-js golden | `plessey::tests::matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `plessey_bidir`| Linear | wrapper alias | `integration::alias_ids_route_to_canonical_symbology` → `Plessey` | — |
| `telepen`      | Linear | bwip-js golden | `telepen::tests::matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `telepen_alpha` | Linear | wrapper alias | `integration::alias_ids_route_to_canonical_symbology` → `TelepenNumeric` | — |
| `telepennumeric` | Linear | bwip-js golden | `telepen::tests::numeric_matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `pharmacode`   | Linear | bwip-js golden | `pharmacode::tests::one_track_matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `pharmacode2`  | Linear | bwip-js golden | `pharmacode::tests::two_track_matches_bwip_js` | (inline bwip-js) |
| `posicode`            | Linear | bwip-js sbs byte-for-byte across all 4 versions (a / b / limiteda / limitedb) including LA latches, SF shifts, and FN4 ASCII↔extended-ASCII transitions | **Anchor (limiteda)**: `posicode::tests::encode_limiteda_digit_zero_matches_bwip_js_sbs`, `posicode::tests::encode_limiteda_digit_one_matches_bwip_js_sbs`, `posicode::tests::encode_limiteda_uppercase_a_matches_bwip_js_sbs`, `posicode::tests::encode_limiteda_uppercase_z_matches_bwip_js_sbs`, `posicode::tests::encode_limiteda_digit_run_matches_bwip_js_sbs`. **Anchor (limitedb)**: `posicode::tests::encode_limitedb_digit_zero_matches_bwip_js_sbs`, `posicode::tests::encode_limitedb_digit_one_matches_bwip_js_sbs`, `posicode::tests::encode_limitedb_uppercase_a_matches_bwip_js_sbs`, `posicode::tests::encode_limitedb_uppercase_z_matches_bwip_js_sbs`, `posicode::tests::encode_limitedb_digit_run_matches_bwip_js_sbs`, `posicode::tests::limitedb_d_is_limiteda_d_plus_one` (symmetry pin). **Anchor (version a)**: `posicode::tests::encode_a_digit_zero_matches_bwip_js_sbs`, `encode_a_digit_one_matches_bwip_js_sbs`, `encode_a_uppercase_a_matches_bwip_js_sbs`, `encode_a_uppercase_z_matches_bwip_js_sbs`, `encode_a_hello_matches_bwip_js_sbs`, `encode_a_digit_run_matches_bwip_js_sbs`, `encode_a_lowercase_run_matches_bwip_js_sbs` (LA1 latch path), `encode_a_la1_mid_message_matches_bwip_js_sbs`, `encode_a_sf1_single_shift_matches_bwip_js_sbs` (SF1 path), `encode_a_sf2_control_byte_matches_bwip_js_sbs` (SF2 path), `encode_a_with_fn4_extended_byte_matches_bwip_js_sbs` (FN4 trailing-shift path), `encode_a_with_fn4_leading_extended_matches_bwip_js_sbs` (FN4 leading path). **Anchor (version b)**: `encode_b_digit_zero_matches_bwip_js_sbs`, `encode_b_uppercase_a_matches_bwip_js_sbs`, `encode_b_hello_matches_bwip_js_sbs`, `encode_b_lowercase_run_matches_bwip_js_sbs` (LA1 + b-table). Plus algorithm pins (`compute_v_matches_bwip_js_oracle`, `decompose_check_digits_matches_bwip_js_oracle`, `build_cbs_matches_bwip_js_oracle`), state-machine pins (`normal_sets_lookup_matches_charmap`, `select_codewords_normal_*` for set0_only / set1_latch / sf1 paths / sf2), FN4 helper pins (`fn4_insertion_is_identity_for_ascii_only`, `fn4_insertion_single_shift_for_trailing_extended`), invalid-input rejection, unknown-version rejection, default-routing pin (`encode_default_routes_to_version_a`), and the Stage 22a table-validation tests. **57 unit tests** in `posicode::tests` total — **22 byte-for-byte sbs goldens** (10 limiteda + 7 limitedb + 7 version-a including FN4 + 5 version-b). | `rust/tools/oracle-posicode.js` (45 corpus rows captured from bwip-js 4.10.1 / BWIPP 2026-04-21: 10 limiteda + 7 limitedb + 20 version-a + 8 version-b) |
| `pzn7`/`pzn8`  | Linear | bwip-js logical golden | `code39_wrappers::tests::pzn7_matches_bwip_js_raw_sbs`, `pzn8_matches_bwip_js_raw_sbs` (byte-for-byte sbs) | (inline bwip-js) |
| `flattermarken`| Linear | bwip-js golden | `flattermarken::tests::matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `vin`          | Linear | bwip-js logical golden | `code39_wrappers::tests::vin_matches_bwip_js_code39` (190-element sbs byte-for-byte) | (inline bwip-js) |
| `logmars`      | Linear | bwip-js logical golden | `code39_wrappers::tests::logmars_matches_bwip_js_code39_with_check` (130-element sbs byte-for-byte) | (inline bwip-js) |
| `codabar`      | Linear | bwip-js golden | `codabar::tests::matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `channelcode`  | Linear | bwip-js golden | `channelcode::tests::channelcode_matches_bwip_js_raw_sbs` (4-input corpus: 00 / 12 / 128 / 00000 across channel counts 3..6) + `encode_rejects_short_or_long_or_non_digit_or_overflow` | (inline bwip-js) |
| `isbn13`       | Linear | bwip-js golden | `book_codes::tests::isbn_matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `ismn`         | Linear | bwip-js golden | `book_codes::tests::ismn_matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `issn`         | Linear | bwip-js golden | `book_codes::tests::issn_matches_bwip_js_raw_sbs` | (inline bwip-js) |

## 4. 1D – Retail / EAN / UPC (verified, bwip-js logical golden)

| Catalog id      | Output | Tests | Notes |
|-----------------|--------|-------|-------|
| `ean13`         | Linear | `ean::tests::ean13_matches_bwip_js_raw_sbs` | |
| `ean8`          | Linear | `ean::tests::ean8_matches_bwip_js_raw_sbs` | |
| `upca`          | Linear | `ean::tests::upca_matches_bwip_js_raw_sbs` | |
| `upce`          | Linear | `ean::tests::upce_matches_bwip_js_raw_sbs` | Native non-expansion path; check-digit + 6-digit input pinned. |
| `ean2`          | Linear | `ean_addons::tests::ean2_matches_bwip_js_raw_sbs` | standalone 13-element sbs |
| `ean5`          | Linear | `ean_addons::tests::ean5_matches_bwip_js_raw_sbs` | standalone 31-element sbs |
| `ean13p2`/`p5`  | Linear | `ean_combined::tests::ean13_p2_matches_bwip_js_raw_sbs`, `ean13_p5_matches_bwip_js_raw_sbs` | combined gap=12 pinned |
| `ean8p2`/`p5`   | Linear | `ean_combined::tests::ean8_p2_matches_bwip_js_raw_sbs`, `ean8_p5_matches_bwip_js_raw_sbs` | |
| `upcap2`/`p5`   | Linear | `ean_combined::tests::upca_p2_matches_bwip_js_raw_sbs`, `upca_p5_matches_bwip_js_raw_sbs` | |
| `upcep2`/`p5`   | Linear | `ean_combined::tests::upce_p2_matches_bwip_js_raw_sbs`, `upce_p5_matches_bwip_js_raw_sbs` | |
| `isbn13p5`      | Linear | `ean_combined::tests::isbn_p5_matches_bwipp_sbs` | |
| `issnp2`        | Linear | `ean_combined::tests::issn_p2_matches_bwipp_sbs` | |

## 5. 1D – GS1 (verified, bwip-js logical golden + wrapper)

| Catalog id        | Output | Tests | Notes |
|-------------------|--------|-------|-------|
| `gs1-128`         | Linear | `gs1_128::tests::matches_bwip_js_raw_sbs`, `matches_bwip_js_multi_ai` | AI parser + FNC1 + Code 128 dispatch |
| `ucc128`          | Linear | wrapper alias to `Gs1_128` (pinned in `from_id` via `integration::alias_ids_route_to_canonical_symbology`) + verified primary via `gs1_128::tests::matches_bwip_js_raw_sbs` | |
| `sscc18`/`nve18`  | Linear | `gs1_128::tests::sscc18_wraps_with_ai_00` (delegates to verified `gs1-128` primary) + `gs1_128::tests::matches_bwip_js_raw_sbs` (bwip-js gs1-128 byte-for-byte sbs) | |
| `ean14`           | Linear | `gs1_128::tests::ean14_with_13_digit_input_matches_bwip_js_raw_sbs` (73-element sbs byte-for-byte vs `bwipp.raw("ean14", "(01)0401234512345")`) + `ean14_accepts_14_digit_input_with_correct_check`, `ean14_rejects_wrong_check_digit`, `ean14_rejects_short_input`, `ean14_accepts_parenthesized_and_unprefixed_forms` | mod-10 check + AI (01) wrap + delegate to verified `gs1-128` |
| `mands`           | Linear | `ean::tests::mands_8_digit_matches_bwip_js_raw_sbs` (43-element sbs byte-for-byte vs `bwipp.raw("mands", "12345670")`) + `mands_7_and_8_digit_forms_match`, `mands_7_digit_with_bad_post_prepend_check_rejects`, `mands_rejects_wrong_length` | EAN-8 substrate with leading-zero pad. BWIPP's bar-tail height adjustment is cosmetic and not preserved by `LinearPattern`. |
| `upc_coupon`      | Linear | `gs1_128::tests::coupon_wraps_with_ai_8110` (AI 8110 envelope) + `gs1_128::tests::matches_bwip_js_raw_sbs` (verified gs1-128 primary, bwip-js byte-for-byte) | |
| `usps_impb`       | Stacked + check | `usps_impb::tests::matches_underlying_gs1_128`, `usps_impb::tests::renders_canonical_payload`, `usps_impb::tests::rejects_empty`, `usps_impb::tests::rejects_non_ai_payload` | |
| `gs1datamatrix`   | Matrix | substrate spec + wrapper | `gs1_2d::tests::gs1_datamatrix_square_shape_matches_bwip_js_size`, plus the verified `datamatrix` byte-for-byte golden. Module pattern matches BWIPP for the catalog input but is not byte-pinned for arbitrary input (datamatrix-crate substrate). |
| `gs1datamatrixrectangular` | Matrix | substrate spec + wrapper | `gs1_2d::tests::gs1_datamatrix_rectangular_produces_rect_shape_and_rejects_bad_ai`. Inherits the `datamatrix` crate substrate-spec posture; shape forced to rectangular. |
| `gs1dldatamatrix` | Matrix | substrate spec + wrapper | `gs1_2d::tests::gs1_dl_datamatrix_matches_bwip_js_size_and_structure` (22×22 dim + L-finder + timing pattern asserted) + `gs1_dl_datamatrix_rejects_invalid_uri`. URI validation via `util::gs1::parse_dl_uri` pinned by `util::gs1::tests::parse_dl_uri_*`. Inherits the `datamatrix` crate substrate-spec posture. |
| `gs1dlqrcode`     | Matrix | wrapper composition pinned | `gs1_2d::tests::gs1_dl_qrcode_renders_and_rejects_invalid_uri`. URI validated via `util::gs1::parse_dl_uri`; payload routes through the native QR encoder via `gs1qrcode`'s FNC1-first-position path (the historical QR-substrate compat exception was retired in Stage 16 + 17c). |
| `ntin`            | Matrix | wrapper composition pinned | `gs1_2d::tests::ntin_composes_8003_and_gs1_datamatrix` (byte-for-byte equals `gs1_datamatrix("(8003){digits}")`) plus `ntin_wraps_with_8003`, `ntin_accepts_explicit_ai`. |
| `ppn`             | Matrix | wrapper composition pinned | `gs1_2d::tests::ppn_envelope_bytes_are_correct` (asserts envelope byte stream + byte-for-byte equals `datamatrix::encode(envelope_bytes)`) + `ppn_renders_with_envelope`, `ppn_rejects_empty`. |
| `gs1qrcode`       | Matrix | bwip-js byte-for-byte (native) | Native bwipp-faithful encoder via `qrcode_native::encode_gs1_qrcode` (FNC1-first-position bit-stream prefix per ISO/IEC 18004 Annex L). FNC1-first-position pinned by `gs1_2d::tests::gs1_qrcode_fnc1_first_position_mode_indicator_is_0101`, `gs1_qrcode_optimal_segmentation_matches_bwipp_size`, `gs1_qrcode_differs_from_plain_qr_of_same_payload`. Historical compat exception retired in Stage 17c. |

## 6. Postal

| Catalog id                  | Output  | Strength | Tests | Oracle |
|-----------------------------|---------|----------|-------|--------|
| `postnet`, `usps_postnet5/9/11` | Postal4 | bwip-js per-bar F/D | `postnet::tests::postnet_matches_bwip_js`; alias coverage in `integration::alias_ids_route_to_canonical_symbology` | (inline bwip-js) |
| `planet`, `planet12/14`     | Postal4 | bwip-js per-bar F/D | `postnet::tests::planet_matches_bwip_js`; alias coverage as above | (inline bwip-js) |
| `usps_onecode`, `usps_imb`  | Postal4 | bwip-js logical | `usps_onecode::tests::binval_then_bytes_match_bwip_js_20`, `codewords_match_bwip_js`, `end_to_end_matches_bwip_js`; alias pinned | `rust/tools/oracle-onecode.js` |
| `royalmail`                 | Postal4 | bwip-js | `postal4::tests::rm4scc_matches_bwip_js` | (inline bwip-js) |
| `kix`                       | Postal4 | bwip-js | `postal4::tests::kix_matches_bwip_js` | (inline bwip-js) |
| `daft`                      | Postal4 | bwip-js | `postal4::tests::daft_matches_bwip_js` | (inline bwip-js) |
| `auspost_customer/redirection/reply/routing` | Postal4 | bwip-js | `auspost::tests::encstrs_match_bwip_js`, `custinfo_character_mode_matches_bwip_js`, `custinfo_numeric_mode_matches_bwip_js`, `bar_shapes_match_bwip_js` | `rust/tools/oracle-auspost.js` |
| `japanpost`                 | Postal4 | bwip-js | `japan_post::tests::bar_sequence_matches_bwip_js` | (inline bwip-js) |
| `mailmark`, `mailmark2d`    | Matrix  | **substrate spec + wrapper** | `mailmark::tests::renders_45_char_payload_as_24x24`, `renders_70_char_payload_as_32x32`, `type_29_with_real_mailmark_sample_matches_16x48`, `typed_and_2d_produce_identical_24x24`. Symbol sizes byte-match BWIPP's expected output; the underlying Data Matrix module pattern relies on the `datamatrix` crate's spec-compliant emitter (same substrate-spec posture as plain `datamatrix`). | — |
| `identcode`/`leitcode`      | Linear  | bwip-js | `identleitcode::tests::identcode_matches_bwip_js_raw_sbs`, `leitcode_matches_bwip_js_raw_sbs` | (inline bwip-js) |
| `upu_s10`                   | Linear  | wrapper composition pinned | `postal_misc::tests::upu_s10_delegates_to_code128` (byte-for-byte equals `code128::encode(upper(input))`), `upu_s10_accepts_valid`, `upu_s10_rejects_bad_check` | — |
| `korean_postal`             | Linear  | wrapper composition pinned | `postal_misc::tests::korean_postal_delegates_to_code128` (byte-for-byte equals `code128::encode("KPA" + payload + check)`), `korean_postal_known_check` | — |
| `cepnet`                    | Linear  | wrapper composition pinned | `postal_misc::tests::cepnet_delegates_to_code128` (byte-for-byte equals `code128::encode("CEP" + payload)`) | — |
| `italian_postal_25`         | Linear  | wrapper composition pinned | `postal_misc::tests::italian_postal_25_delegates_to_i2of5` (byte-for-byte equals `interleaved2of5::encode(zero_padded)`), `..pads_odd_length` | — |
| `italian_postal_39`         | Linear  | wrapper composition pinned | `postal_misc::tests::italian_postal_39_delegates_to_code39_with_check` (byte-for-byte equals `code39::encode(payload, includecheck=true)`), `..includes_check` | — |
| `dpd`                       | Linear  | wrapper composition pinned | `postal_misc::tests::dpd_delegates_to_code128` (byte-for-byte equals `code128::encode(payload)`) | — |
| `dp_postmatrix`             | Matrix  | substrate spec + wrapper | `postal_misc::tests::dp_postmatrix_delegates_to_datamatrix`, `..renders` | — |
| `swedish_postal`            | Linear  | wrapper alias | aliased to `Sscc18` (see `from_id`); pinned in `integration::alias_ids_route_to_canonical_symbology` | — |

## 7. 2D – Matrix

| Catalog id              | Output | Strength | Tests | Oracle |
|-------------------------|--------|----------|-------|--------|
| `qrcode`/`qrcode_iso`/`qr_code` | Matrix | bwip-js byte-for-byte | `qrcode_native::tests::encode_full_qr_pixs_corpus_matches_oracle` — 24 oracle-pinned Full QR corpus rows spanning V1–V40 × L/M/Q/H samples, byte-for-byte against bwip-js. The native encoder became the default in Stage 16; the upstream `qrcode` crate substrate is preserved as an opt-out via `--no-default-features` (substrate regression baseline still pinned by `qrcode_::tests::substrate_baseline_pixs_for_hello`). | `rust/tests/fixtures/qrcode_native_pixs.txt` |
| `microqrcode`           | Matrix | bwip-js byte-for-byte (native) | `qrcode_native::tests::encode_micro_qr_pixs_corpus_matches_oracle` — 8 oracle-pinned corpus rows spanning M1–M4 × valid EC levels, byte-for-byte against bwip-js. Plus `qrcode_::tests::micro_qr_encodes_small_payload`, `micro_qr_forced_version_picks_size`, `micro_qr_rejects_eclevel_h`, `micro_qr_rejects_out_of_range_version`. Historical compat exception retired in Stage 16. | `rust/tests/fixtures/qrcode_native_micro_pixs.txt` |
| `rectangularmicroqrcode` | Matrix | **bwip-js byte-for-byte** | `qrcode_native::tests::encode_rmqr_pixs_corpus_matches_oracle` — 16 (size × eclevel × text) corpus rows pinned cell-for-cell against bwip-js, spanning all 5 height categories (R7..R17) × M and H. Supporting tests pin the formatfimmap positions (576 across 32 sizes), the BCH(18,6) fmtval tables (128 entries), the 4-corner finder/sub-finder placement, the alignment-column timing strips, and the walker's traversal order (104 positions for R7×43). | `rust/tests/fixtures/qrcode_native_rmqr_pixs.txt` (16 rows generated via `rust/tools/oracle-rmqr-pixs.js`) |
| `swissqrcode`           | Matrix | wrapper composition pinned | `swiss_qr::tests::renders_minimal_spc`, `swiss_qr::tests::composes_eclevel_m_and_qrcode`, plus the SPC-validated payload constructor (`swiss_qr::tests::rejects_non_spc_payload`, `swiss_qr::tests::rejects_empty`). Inherits the native QR encoder's bwip-js parity (Stage 16 cutover). | — |
| `gs1qrcode`             | Matrix | composition pin | Native bwipp-faithful encoder via `qrcode_native::encode_gs1_qrcode` (FNC1-first-position bit-stream prefix per ISO/IEC 18004 Annex L). Pinned by `gs1_2d::tests::gs1_qrcode_fnc1_first_position_mode_indicator_is_0101`, `gs1_qrcode_optimal_segmentation_matches_bwipp_size`, `gs1_qrcode_differs_from_plain_qr_of_same_payload`, `gs1_qrcode_with_explicit_version_override`, `gs1_qrcode_payload_round_trips_through_ai_parser`. | — |
| `datamatrix`            | Matrix | bwip-js logical golden | `datamatrix_::tests::matches_bwip_js_raw_pixs` — 12×12 module-by-module byte match for `"hello"`. Wider catalog coverage via shape tests; tie-broken inputs are theoretically possible but no tie has been observed against bwip-js for the inputs exercised. | (inline bwip-js) |
| `datamatrixrectangular` | Matrix | substrate spec | `datamatrix_::tests::dmre_produces_rectangular_for_long_input` + `datamatrix_::tests::dmre_short_input_matches_bwip_js_size` + verified plain-DM substrate via `datamatrix_::tests::matches_bwip_js_raw_pixs`. Same `datamatrix` substrate, different size policy. | — |
| `datamatrixrectangularextension` | Matrix | substrate spec | `datamatrix_::tests::dmre_short_input_matches_bwip_js_size` (18×8 for `"12345"` agrees with bwip-js) + `dmre_produces_rectangular_for_long_input`. Substrate enforces `SymbolList::with_extended_rectangles()`; size policy can pick a classic rectangular size where BWIPP picks DMRE. | — |
| `hanxin`                | Matrix | bwip-js logical | `hanxin::tests::evalfull_scores_match_bwip_js_a_l1`, `evalfull_scores_match_bwip_js_hello_l2`, plus 6 pixs oracles + 24-case mask-score corpus | (inline bwip-js) |
| `dotcode`               | Dots   | bwip-js logical | `dotcode::tests::mask_constants_match_bwipp`, `encode_b_matches_bwipp_oracle`, `render_pixs_matches_oracle_full_pipeline`, `eval_symbol_matches_bwipp_scores`, `full_pipeline_matches_oracle`, `rs_ecc_matches_oracle`, `encode_mode_a_run_from_c_matches_bwipp_oracle`, plus marker-aware goldens for FN1 / FN2 / FN3 emission (`encode_with_markers_*`), full encB / encA dispatch (`encode_message_with_markers_*`), BIN escape (`base259_to_103_matches_bwipp_polynomial`, `encode_message_with_markers_pure_binary_run` + text-then-bin + bin-to-b exits). | `rust/tools/oracle-dotcode*.js` |
| `gs1dotcode`            | Dots   | wrapper composition pinned | `gs1_dotcode::tests::encode_gtin_14_matches_bwip_js_logical_cws` (cws `[1, 4, 1, 23, 45, 12, 34, 56]`), `encode_gtin_with_expiry_matches_bwip_js_logical_cws` (cws `[1, 4, 1, 23, 45, 12, 34, 56, 17, 26, 5, 20]`), `encode_gtin_with_lot_matches_bwip_js_logical_cws` (rows=19, columns=28 matching bwip-js). Wraps verified `util::gs1::parse` / `encode_with_fnc1` + `dotcode::encode_with_markers`. | `rust/tools/oracle-dotcode*.js` (`bcid: "gs1dotcode"`) |
| `code16k`               | Matrix | bwip-js byte-for-byte cws + pixs pin across every default-options BWIPP path | **Pixs anchor**: `code16k::tests::encode_pixs_matches_bwip_js_golden_for_12` (405 cells byte-for-byte vs bwip-js). **Stage-3a (mid-message A↔B + FN4)**: `mixed_mode_b_with_trailing_control_byte_swa_latch`, `mixed_mode_b_with_two_trailing_control_bytes_swa_latch`, `mixed_mode_b_with_mid_message_sa1_shift`, `mixed_mode_a_from_start_for_control_byte_in_middle`, `mixed_mode_a_from_start_for_leading_control_byte`, `mixed_extended_ascii_one_byte_via_fn4_shift`, `mixed_extended_ascii_with_following_byte_via_fn4`. **Stage-3b (initial-mode selector + mode-C main loop)**: `initial_mode_pure_digits_even_picks_mode_c`, `initial_mode_pure_digits_odd_picks_mode_5`, `initial_mode_one_b_byte_then_2_even_digits_picks_mode_5`, `initial_mode_one_b_byte_then_4_even_digits_picks_mode_5`, `initial_mode_one_b_byte_then_5_odd_digits_picks_mode_6`, `initial_mode_two_b_bytes_then_2_even_digits_then_text_picks_mode_6`, `initial_mode_two_b_bytes_then_4_even_digits_picks_mode_6`, `initial_mode_two_b_bytes_then_6_even_digits_picks_mode_6`, `initial_mode_two_b_bytes_then_8_digits_then_text_picks_mode_6`, `initial_mode_two_b_bytes_then_4_digits_then_text_picks_mode_6`, `initial_mode_lowercase_then_digits_then_lowercase`, `initial_mode_lowercase_then_6_digits_then_lowercase`, `initial_mode_lowercase_then_3_odd_digits_then_lowercase_mode_6`. **Stage-3c (mid-message SC2/SC3 + SWC + mode-C SB shifts)**: `mixed_mode_b_with_sa2_two_byte_shift`, `mixed_mode_b_with_sa2_amid_lowercase`, `mid_message_swc_latch_after_long_text`, `mid_message_swc_latch_lowercase`, `mid_message_swc_with_4_digits_even`, `mode_c_sb1_shift_for_single_text_byte`, `mode_c_sb1_shift_longer_payload`, `mid_message_sc2_from_a`, `mid_message_sc3_from_a`, `codeword_constants_match_charmaps`. **Original-mode anchors**: 17 logical-cws goldens via `encode_cws_digit_only`, `encode_cws_text_only`, `encode_cws_digit_with_shift_b`, `encode_cws_mode_a_*`. **Algorithm + dispatcher pins**: `anotb_bnota_match_charmap`, `fn4_insertion_is_identity_for_pure_ascii`, `numsscr_pure_digit_run`, `numsscr_stops_at_non_digit`, `numsscr_from_offset`, `pair_codeword_basic_pairs`, `mixed_wrapper_adds_row_indicator_and_checks`, `dispatcher_routes_mixed_through_encode_cws_mixed`, `compute_checksums_matches_bwipp_goldens`. **63 unit tests** in `code16k::tests` total, with **30 byte-for-byte cws goldens** covering modes 0/1/2/5/6 from start plus every mid-message transition (SA1/SA2/SB1/SB2/SC2/SC3/SWA/SWB/SWC/FN4). The remaining BWIPP knobs (`parsefnc` for FN1/2/3 / SAM) are opt-in options not exercised by the default encoder path. | `rust/tools/oracle-code16k.js` (24-row corpus captured from bwip-js 4.10.1 / BWIPP 2026-04-21) |
| `codeone`               | Matrix | bwip-js byte-for-byte cws + pixs pin across every default-options BWIPP path | **Pixs anchor**: `codeone::tests::compose_pixs_matches_bwip_js_golden_for_hello` (288 cells byte-for-byte vs bwip-js). **Stage-3d Mode D anchors**: `mode_d_thirteen_digits_at_eom_matches_oracle`, `mode_d_twenty_digits_at_eom_matches_oracle`, `mode_d_after_mode_a_prefix_matches_oracle`, `mode_d_twentyone_digit_trigger_matches_oracle`, `mode_d_with_mode_a_tail_matches_oracle`, `mode_d_sandwiched_between_mode_a_matches_oracle`, `mode_d_sixteen_digits_one_trailing_matches_oracle`, `mode_d_fourteen_digits_two_trailing_matches_oracle`, `mode_d_fifteen_digits_clean_termination_matches_oracle` — 9 byte-for-byte cws goldens captured via `rust/tools/oracle-codeone.js` covering the bit-buffer packing (3-digit groups → 10 bits), Mode-A→D 4-bit `1111` handshake, BWIPP termination state machine (`getnumremcws` × `Drem`), and the Mode-D → Mode-A return path. **Algorithm pins**: `getnumremcws_table_anchors`, `append_dbits_round_trip`. **Mode B anchors**: `encode_message_routes_high_bytes_through_mode_b` + `encode_message_mode_b_accepts_high_byte_range` (Stage 20.5). 49 unit tests in `codeone::tests` covering the full pipeline: constants (METRICS_NONSTYPE / METRICS_STYPE / CPATMAP / BLACKDOTMAP / STYPEVALS / RSPARAMS), Mode A `avals_*` + digit-pair packing, Mode B `encode_mode_b_run`, Mode CTX `cnvals/tnvals/xvals + ctxvals_to_cws + encode_ctx_run`, Mode D `encode_d_step` + `append_dbits` + `getnumremcws`, BWIPP `lookup()` forward-scan with `ff()` Float32 truncation pinned by 11 mode-decision goldens, GF(256) ECC pinned by 5 ECC goldens, symbol-size picker, `cws_to_mmat` placement, `compose_pixs` byte-for-byte pixs goldens for 4 inputs. Remaining BWIPP knobs (`parsefnc`, `eci`, `version` for S-strip/T-strip) are opt-in options not exercised by the default encoder path. | `rust/tools/oracle-codeone.js` (16-row corpus captured from bwip-js 4.10.1 / BWIPP 2026-04-21) |
| `code49`                | Matrix | bwip-js byte-for-byte cws + pixs pin for every default-options BWIPP path | **Pixs anchor**: `code49::tests::encode_pixs_matches_bwip_js_golden_for_12345` (405 cells byte-for-byte vs bwip-js). 20 unit tests in `code49::tests`: constants (CHARMAP, METRICS, SAMVAL, PARITY, WEIGHTX/Y/Z, PATTERNS_0/1 with 2 × 2401 entries); `lookup_direct_spot_checks`, `charvals_spot_checks`, `pick_symbol_size_picks_smallest_metrics_row`; cws-level goldens for each path (`encode_cws_direct_matches_bwip_js_goldens` for 7 direct-lookup inputs, `base48_matches_bwip_js_polynomial` for 5 base-48 packs, `encode_cws_ns_digits_matches_bwip_js_goldens` for 8 NS-shift cases covering each remainder branch, `encode_cws_alpha_matches_bwip_js_goldens` for 8 alpha-mode-0/4/5 inputs, `encode_cws_dispatches_correctly` top-level dispatch); `build_ccs_matches_bwip_js_goldens` (6-input golden covering r=2 + r=3 cases verifying the cr7 / wr1 / wr2 / check_x row-check formula); `encode_pixs_matches_bwip_js_golden_for_12345` (405 compressed-pixs cells = 5 rows × 81 modules byte-identical to bwip-js); `encode_produces_valid_bitmatrix_for_supported_inputs` (end-to-end r=2/r=3 dimension checks). Stage 3e promoted to verified: SAM (Symbol Append Mode) chaining and `append` chaining are opt-in BWIPP options (`sam`/`append` parameters) not exercised by the default encoder path; SAMVAL table is already ported for the future opt-in `code49::encode_sam_chain` entry point. | `rust/tools/oracle-code49.js` |
| `maxicode`              | Hex    | bwip-js logical | `maxicode::tests::encode_set_a_only_matches_oracle`, `encode_set_a_with_ns_matches_oracle_corpus`, `encode_ns_run_9_digit_matches_oracle`, `latch_seq_shape_matches_latch_len`, plus 18 mode-by-mode oracle tests | `rust/tools/oracle-maxicode*.js` |
| `ultracode`             | ColorMatrix | bwip-js logical | `ultracode::tests::encode_pixs_default_matches_corpus` (8-input byte-for-byte pixs corpus covering single-byte / short ASCII / sentence / digits / letters / alphanumeric / UTF-8 high-byte / multi-word), `ecc_codewords_match_corpus`, `gen_coeffs_matches_corpus`, `metadata_matches_corpus`, `pick_symbol_size_matches_corpus`, `palette_shape_and_anchors`, `bwipp_tile_digit_lookup_matches_colormap`, plus 11 supporting unit tests (constants tables shape, GF(283) RS field invariants, encoder input-validation guards, ColorMatrix dispatch) | `rust/tools/oracle-ultracode.js` |
| `azteccode`             | Matrix | bwip-js logical | `aztec::tests::encode_hello_matches_bwip_js_compact_l1`, `encode_hello_world_matches_bwip_js`, `encode_high_bit_byte_matches_bwip_js`, `encode_digits_matches_bwip_js_compact`, plus mode-bits/build-matrix shape tests | (inline bwip-js) |
| `azteccodecompact`      | Matrix | wrapper composition pinned | `aztec::tests::encode_compact_matches_encode_for_short_input` (byte-identical to verified primary for short input), `encode_compact_rejects_payload_that_exceeds_l4`, `encode_compact_rejects_empty_input` | — |
| `aztecrune`             | Matrix | bwip-js logical | `aztec::tests::encode_rune_matches_bwip_js_pixs` (11×11 pixs byte-for-byte vs `bwipp.raw("aztecrune", v)` for v ∈ {0,42,128,255}), `encode_rune_rejects_invalid_input` | (inline bwip-js) |

## 8. 2D – Stacked / Multi-row

| Catalog id        | Output | Strength | Tests | Oracle |
|-------------------|--------|----------|-------|--------|
| `pdf417`          | Matrix | bwip-js logical | `pdf417::tests::text_hello_world_matches_bwip_js`, `data_codewords_match_bwip_js`, `pdf417_cws_matches_bwip_js`, `pdf417_render_matches_bwip_js_pixs` | `rust/tools/oracle-pdf417.js` |
| `pdf417_truncated`| Matrix | bwip-js logical | `pdf417::tests::pdf417_render_truncated_matches_bwip_js_pixs` | `rust/tools/oracle-pdf417-truncated.js` |
| `micropdf417`     | Matrix | bwip-js logical | `micropdf417::tests::data_codewords_match_bwip_js`, `pack_ccb_datcws_matches_bwip_js_oracle`, `render_ccb_cws_matches_bwip_js_after_rs_ecc` | `rust/tools/oracle-micropdf417.js` |
| `codablockf`      | Stacked | bwip-js logical | `codablockf::tests::codewords_match_bwip_js_oracle`, `rendered_bars_match_bwip_js` | `rust/tools/oracle-codablockf.js`, `verify-codablockf.js` |

## 9. GS1 DataBar family (verified, bwip-js logical golden)

| Catalog id                     | Output  | Tests |
|--------------------------------|---------|-------|
| `databar_omni`                 | Stacked | `databar::tests::omni_rendered_sbs_matches_bwip_js`, `omni_widths_match_bwip_js_oracle` |
| `databar_truncated`            | Stacked | `databar::tests::omni_rendered_sbs_matches_bwip_js` (structurally identical to Omni — `encode_truncated` delegates to `render_omni`; the truncated row differs only in the conventional render-height default) |
| `databar_limited`              | Stacked | `databar::tests::limited_rendered_sbs_matches_bwip_js` |
| `databar_stacked`              | Stacked | `databar::tests::stacked_matches_bwip_js_pixs` |
| `databar_stacked_omni`         | Stacked | `databar::tests::stackedomni_matches_bwip_js_pixs` |
| `databar_expanded`             | Stacked | `databar_expanded::tests::extract_data_character_matches_oracle_segments_for_input_a`, `extract_checksum_character_matches_oracle_input_a`, plus every method arm |
| `databar_expanded_stacked`     | Stacked | `databar_expanded::tests::encode_stacked_matches_oracle_for_input_a`, `databar_expanded::tests::encode_stacked_three_rows_with_reversed_middle_row` |

Oracles: `rust/tools/oracle-databar*.js`.

## 10. GS1 Composite (17 variants — verified, bwip-js logical golden)

The composite encoder (`gs1_cc.rs` + `composite.rs`) builds a linear
half + CC-A/CC-B/CC-C 2D companion and stacks them. The 17 variants
share a single dispatch table; each variant is exercised by its own
`integration::every_symbology_renders_svg` arm, plus the family-level
tests:

* `composite::tests::cca_post_ecc_cws_for_batch_match_bwip_js`
* `composite::tests::cca_render_for_batch_matches_bwip_js_row2`
* `composite::tests::encode_databaromni_cca_matches_bwip_js_pixs_first_8_rows`
* `composite::tests::encode_databartruncated_cca_matches_bwip_js_pixs`
* `composite::tests::encode_databartruncated_ccb_matches_bwip_js_pixs`
* `composite::tests::encode_databarstacked_cca_matches_bwip_js_pixs`
* `composite::tests::encode_databarstacked_ccb_matches_bwip_js_pixs`
* `composite::tests::encode_databarstackedomni_cca_matches_bwip_js_pixs`
* `composite::tests::encode_databarstackedomni_ccb_matches_bwip_js_pixs`
* `composite::tests::encode_databarexpandedstacked_cca_matches_bwip_js_pixs`
* `composite::tests::encode_databarexpandedstacked_ccb_dims_match_bwip_js`
* `composite::tests::encode_gs1_128_ccc_dimensions_match_bwip_js`
* `composite::tests::encode_gs1_128_ccc_matches_bwip_js_separator_and_linear`
* `composite::tests::encode_gs1_128_ccc_matches_bwip_js_cc_row_0_first_cells`

The 25 catalog rows: `composite_databar_omni_cca/_ccb`,
`composite_databar_truncated_cca/_ccb`,
`composite_databar_stacked_cca/_ccb`,
`composite_databar_stacked_omni_cca/_ccb`,
`composite_databar_expanded_stacked_cca/_ccb`,
`composite_databar_limited_cca/_ccb`,
`composite_databar_expanded_cca/_ccb`,
`composite_gs1_128_cca/_ccb/_ccc`,
`composite_ean13_cca/_ccb`, `composite_ean8_cca/_ccb`,
`composite_upca_cca/_ccb`, `composite_upce_cca/_ccb`.

Every row is reachable through `Symbology::from_id` with both the
underscore-separated and `databar*composite_*` forms (see
`alias_ids_route_to_canonical_symbology`).

## 11. Healthcare (HIBC)

HIBC LIC and HIBC PAS are **wrapper symbologies**: they apply a
`+`-prefixed payload + check-digit ("modulo-43 over the HIBC
alphabet") and then dispatch to a verified primary encoder.

The wrapper is pinned by:

* `hibc::tests::check_digit_known_vector` — the prefix + check-digit
  transformation matches BWIPP's documented test vector.
* `hibc::tests::format_rejects_empty`, `format_rejects_invalid_character`,
  `format_uppercases_input` — input-validation parity.

For the dispatch:

| Catalog id (LIC + PAS)      | Strength | Tests |
|-----------------------------|----------|-------|
| `hibc_lic_code128`          | bwip-js logical golden | `hibc::tests::encode_code128_matches_bwip_js_raw_sbs` (127-element sbs for the canonical input) |
| `hibc_lic_code39`           | bwip-js logical golden | `hibc::tests::encode_code39_matches_bwip_js_raw_sbs` (byte-for-byte `raw("hibccode39", ...)[0].sbs`) + `encode_code39_renders` + format test |
| `hibc_lic_datamatrix`       | wrapper composition pinned + substrate | `hibc::tests::encode_datamatrix_composes_format_and_datamatrix` (byte-for-byte equals `datamatrix(format(input))`) + `encode_datamatrix_renders` + verified primary |
| `hibc_lic_qrcode`           | wrapper composition pinned | `hibc::tests::encode_qrcode_composes_format_and_qrcode` (byte-for-byte equals `qrcode(format(input))`). Inherits the native QR encoder's bwip-js parity (Stage 16 cutover). |
| `hibc_lic_pdf417`           | wrapper composition pinned | `hibc::tests::encode_pdf417_composes_format_and_pdf417` (byte-for-byte equals `pdf417(format(input))`, plus negative assertion against unformatted input) + `encode_pdf417_renders` + verified primary |
| `hibc_lic_micropdf417`      | wrapper composition pinned | `hibc::tests::encode_micropdf417_composes_format_and_micropdf417` (byte-for-byte equals `micropdf417(format(input))`) + `encode_micropdf417_renders` + verified primary |
| `hibc_lic_codablockf`       | wrapper composition pinned | `hibc::tests::encode_codablockf_composes_format_and_codablockf` (byte-for-byte equals `codablockf(format(input))` row-by-row) + `encode_codablockf_renders` + verified primary |
| `hibc_lic_azteccode`        | wrapper composition pinned | `hibc::tests::encode_azteccode_composes_format_and_aztec` + verified Aztec primary. Surfaced + fixed an Aztec DP gap (`sentinel_codeword(STATE_DIGIT, SHIFT_PUNCT)` was missing; now returns codeword 0 per Aztec spec PS). |
| `hibc_lic_datamatrix_rectangular` | wrapper composition pinned + substrate | `hibc::tests::encode_datamatrix_rectangular_composes_format_and_datamatrix_rect` + verified datamatrix-rect substrate. |
| `hibc_pas_code128`          | bwip-js logical golden | `hibc::tests::encode_pas_code128_matches_bwip_js_raw_sbs` (byte-for-byte `raw("hibccode128", ...)[0].sbs` with PAS envelope) |
| `hibc_pas_code39`           | bwip-js logical golden | `hibc::tests::encode_pas_code39_matches_bwip_js_raw_sbs` (byte-for-byte sbs) |
| `hibc_pas_datamatrix`       | wrapper composition pinned + substrate | `hibc::tests::encode_pas_datamatrix_composes_format_pas_and_datamatrix` |
| `hibc_pas_qrcode`           | wrapper composition pinned | `hibc::tests::encode_pas_qrcode_composes_format_pas_and_qrcode` (byte-for-byte equals `qrcode(format_pas(input))`). Inherits the native QR encoder's bwip-js parity (Stage 17c). |
| `hibc_pas_pdf417`           | wrapper composition pinned | `hibc::tests::encode_pas_pdf417_composes_format_pas_and_pdf417` (byte-for-byte equals `pdf417(format_pas(input))`) |
| `hibc_pas_micropdf417`      | wrapper composition pinned | `hibc::tests::encode_pas_micropdf417_composes_format_pas_and_micropdf417` (byte-for-byte equals `micropdf417(format_pas(input))`) |
| `hibc_pas_codablockf`       | wrapper composition pinned | `hibc::tests::encode_pas_codablockf_composes_format_pas_and_codablockf` (byte-for-byte equals `codablockf(format_pas(input))` row-by-row) |

**Honest assessment** — four of the HIBC rows
(`hibc_lic_code128`, `hibc_lic_code39`, `hibc_pas_code128`,
`hibc_pas_code39`) have byte-for-byte bwip-js logical goldens. The
remaining LIC + PAS rows are **wrapper-composition pinned**: each
asserts byte-for-byte equality between the wrapper output and the
verified primary applied to the formatted payload (LIC uses
`format()`, PAS uses `format_pas()`). Combined with the underlying
primaries' bwip-js logical goldens, this transitively proves every
HIBC row matches BWIPP byte-for-byte (modulo the documented QR-family
substrate exception for `hibc_lic_qrcode` / `hibc_pas_qrcode`).

## 12. Substrate-spec rows (substrate spec + wrapper)

These rows delegate the 2D module layout to either the `qrcode` or
`datamatrix` crate. The substrates are spec-compliant — symbols
decode to the correct payload and conform to ISO/IEC 18004 / 16022 —
but their mask-selection or encoding-mode policy can pick a different
output than BWIPP for the same input. The publish-readiness audit
confirmed empirically that **the entire qrcode-substrate family
routes through the in-crate native `qrcode_native` encoder by
default (since Stage 16) and is byte-for-byte verified against
bwip-js on a 48-row corpus (24 Full V1–V40 × L/M/Q/H samples + 8
Micro M1–M4 × valid EC levels + 16 rMQR R7×_..R17×_ × M/H). The
`qrcode` crate substrate is preserved as an opt-out
(`--no-default-features`); the regression baseline
`qrcode_::tests::substrate_baseline_pixs_for_hello` pins the
upstream substrate's behaviour so a future drift fails CI.

The datamatrix-crate substrate rows have NOT been observed to
diverge for the catalog inputs we test; they remain `verified`
with the caveat that future inputs could expose a tie-break:

* `datamatrixrectangular`, `gs1datamatrix`, `ntin`, `ppn`,
  `hibc_lic_datamatrix`, `hibc_pas_datamatrix`, `dp_postmatrix`,
  `mailmark`, `mailmark2d` (datamatrix-crate substrate — **verified**)

For all of these:

* The encoded payload is verified — for GS1 wrappers, by the GS1 AI
  parser + FNC1 emission tests; for HIBC, by `hibc::format`; for
  Swiss QR, by the SPC-Validated payload builder.
* The symbol *size* is asserted against BWIPP's expected output where
  available (Mailmark uses this for types 7/9/29).
* The substrate itself is upstream-maintained and spec-compliant.

If a future input ever surfaces where the substrate's module pattern
demonstrably diverges from BWIPP **and** that divergence breaks a
scanner, we promote the affected row to a compatibility exception
(see the `gs1qrcode` precedent) and add a regression test pinning
both the divergence and the scanner-visible payload.

## 13. Catalog reachability summary

| Surface             | Coverage |
|---------------------|----------|
| Rust `Symbology::from_id` | 168/168 PORT_STATUS rows |
| `Symbology::all()` (canonical ids) | 153 |
| CLI (`bwipp <id> …`) | uses `from_id` → 168/168 |
| Raw-pointer WASM ABI (`bwipp_wasm_*`) | uses `from_id` → 168/168 |
| wasm-bindgen `renderSvg/Png/listSymbologies` | uses `from_id` → 168/168 |
| Web (`web/`) | catalog.ts mirrors the legacy reference catalog 135/135; substrate aliases handled by `rustCandidatesFor()` in `web/src/lib/rust-engine.ts` |

## 14. Next-iteration hardening (non-blocking)

Concrete items the audit identified as legitimate but non-blocking
ways to tighten the matrix further:

1. ~~**Inline bwip-js byte-for-byte goldens for HIBC LIC Code 39 and
   HIBC PAS Code 128**~~ — **resolved**.
   `hibc::tests::encode_code39_matches_bwip_js_raw_sbs` and
   `hibc::tests::encode_pas_code128_matches_bwip_js_raw_sbs` both pin
   byte-for-byte `raw(...)[0].sbs` matches against bwip-js. Earlier
   audit notes listed these as next-iteration but the tests were
   already committed.
2. ~~**DotCode RS interleaving** for `nw > 112`~~ — **resolved**.
   `dotcode::encode` now returns `Error::InvalidData` when
   `nw = nd + nc > 112` (the threshold where BWIPP switches to
   interleaved-streams RS), with regression tests
   `dotcode::tests::high_level_encode_rejects_long_payload_requiring_interleaved_rs`
   and `high_level_encode_accepts_short_payload_under_threshold` pinning
   the boundary. Implementing the BWIPP interleave path remains a
   non-blocking enhancement.
3. **Symbol-size goldens for substrate-spec rows** — every
   substrate-backed row asserts size in one or two tests; a uniform
   "expected width/height per BWIPP" table per family would catch a
   substrate-version bump that changed encoding-mode policy.
4. **Strict pixs goldens for substrate rows** under a feature flag
   `--features bwipp_compat_pixs`, gated by a Cargo feature so the
   default build remains permissive. Out of scope for v0.1.

None of these block release of the current catalog.
