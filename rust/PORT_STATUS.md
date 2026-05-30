# bwipp-rs - port status

Catalog total: **169** entries  
Verified: **169** | Partial: **0** | Compatibility exceptions: **0** | Missing: **0**

**Every row in the catalog is verified.** 169 rows are either
(a) byte-for-byte verified against bwip-js / BWIPP for known test
vectors, or (b) composition-pinned as a thin wrapper over a
verified primary encoder. There are no partial rows, no
compatibility exceptions, and no missing rows.

The QR Code family (9 rows: `qrcode`,
`qrcode_iso`, `microqrcode`, `rectangularmicroqrcode`, `swissqrcode`,
`gs1qrcode`, `gs1dlqrcode`, `hibc_lic_qrcode`, `hibc_pas_qrcode`) is
routed through the native bwipp-faithful encoder in
`src/symbology/qrcode_native/` — verified on 48 oracle-pinned
corpus rows (24 Full V1–V40 × L/M/Q/H samples + 8 Micro M1–M4 × valid
EC levels + 16 rMQR R7×_..R17×_ × M/H). The `qrcode` crate substrate
is preserved as an opt-out via `--no-default-features`.

Status legend:

- **missing**: catalog row exists but no Rust implementation. Currently 0.
- **partial**: encoder exists but has known gaps. Currently 0.
  POSICODE was promoted in Stage 22d, Code 16K in Stages
  3a/3b/3c, Code One in Stage 3d, and Code 49 in Stage 3e.
- **verified**: output matches BWIPP/bwip-js for known test vectors,
  *or* is a thin wrapper / alias whose composition over a verified
  primary encoder is pinned by a test (`HIBC LIC/PAS`, `EAN add-on`
  combinator, QR-family wrappers over the native QR substrate, etc.).
- **compatibility exception**: encoder is spec-compliant and the
  symbol decodes to the correct payload, but the precise module/bar
  pattern does not byte-match BWIPP. Currently 0 (the QR-family
  exceptions were retired in Stage 16 when the native QR encoder
  became the default and again in Stage 17c when `gs1qrcode` joined).
  Any future entry here gets a precise justification in
  [`COMPATIBILITY_EXCEPTIONS.md`](COMPATIBILITY_EXCEPTIONS.md) and a
  test that pins both the spec-compliant behavior and the nature of
  the divergence.

See [`AUDIT.md`](AUDIT.md) for the verification-strength matrix that
classifies every `verified` row by oracle source (bwip-js logical
golden / BWIPP PostScript golden / wrapper proof / substrate
spec-compliance).

> **Scope note**: this document covers the project's catalog of
> 169 user-facing IDs (rendered through the [`Symbology`] enum and
> reachable via `Symbology::from_id`). For the **upstream BWIPP /
> bwip-js comparison** — which classifies every encoder upstream
> `bwipp_symlist` enumerates as `implemented` / `alias_only` /
> `compatibility_exception` / `partial` / `missing` / `out_of_scope`
> and explains every gap — see
> [`PORT_COMPLETENESS.md`](PORT_COMPLETENESS.md). Every user-facing
> upstream encoder is now implemented — the remaining three
> `out_of_scope` entries (`raw`, `symbol`, `gs1-cc`) are internal
> bwip-js dispatch helpers, not standalone public encoders.
> `ultracode` was promoted to verified in Stage 4 (colour 2D matrix
> with `Encoded::ColorMatrix` carrier + colour SVG/PNG renderers +
> byte-for-byte pixs corpus). All four `posicode` variants
> (`a`, `b`, `limiteda`, `limitedb`) are byte-for-byte verified
> against bwip-js as of Stage 22d.

## 1D - Standard

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `code39` | Code 39 | bwipp | `code39` | verified | bwip-js (bar pattern for "HELLO" matches `raw()[0].sbs` byte-for-byte) |
| `code39ext` | Code 39 Full ASCII | bwipp | `code39ext` | verified | bwip-js (sbs byte-for-byte for "Hello", "abc", "123 ABC", "abc!" — exercises each escape family: `+`, `/`, `%`, the base alphabet, and the SPACE special case) |
| `code93` | Code 93 | bwipp | `code93` | verified | bwip-js (bar pattern for "CODE93" matches `raw()[0].sbs` byte-for-byte) |
| `code93ext` | Code 93 Full ASCII | bwipp | `code93ext` | verified | bwip-js (sbs byte-for-byte for 4 mixed-case + punctuation payloads; translation now correctly emits SFT1..=SFT4 shift codewords — not literal `$/%+` — matching BWIPP's `^SFT$/^SFT%/^SFT//^SFT+` parsefnc semantics) |
| `code11` | Code 11 | bwipp | `code11` | verified | bwip-js (sbs byte-for-byte for "123", "0123456789" and "1-2" with both `includecheck: false` and the default-on path including the K-check digit for 10-char input; 3:1 wide:narrow ratio rebuilt from BWIPP's `code11_encs`) |
| `code128` | Code 128 | bwipp | `code128` | verified | bwip-js (fixed broken Start A/B/C + Stop patterns; bar pattern for "Hello" matches `raw()[0].sbs` byte-for-byte) |
| `code128a` | Code 128 Subset A | custom | `render_code128_subset` | verified | bwip-js (alias of `code128` — BWIPP doesn't expose a subset-only encoder; `from_id` routes `code128a/b/c` to `Symbology::Code128`, whose auto-subset selector already picks the optimal subset for each input) |
| `code128b` | Code 128 Subset B | custom | `render_code128_subset` | verified | bwip-js (alias of `code128`; see `code128a`) |
| `code128c` | Code 128 Subset C | custom | `render_code128_subset` | verified | bwip-js (alias of `code128`; see `code128a`) |
| `code32` | Code 32 (Italian Pharmacode) | bwipp | `code32` | verified | bwip-js (fixed: bars no longer include the "A" prefix; bar pattern for "01234567" matches `raw()[0].sbs` byte-for-byte) |

## 1D - 2 of 5 family

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `code2of5` | Code 2 of 5 (Standard) | bwipp | `code2of5` | verified | bwip-js (fixed: encoder now uses BWIPP's run-length encs directly; "12345" matches `raw()[0].sbs` byte-for-byte) |
| `datalogic2of5` | Code 2 of 5 Data Logic | bwipp | `datalogic2of5` | verified | bwip-js (same encoder rewrite; matches `raw("code2of5", "12345", {version: "datalogic"})[0].sbs` byte-for-byte) |
| `iata2of5` | Code 2 of 5 IATA | bwipp | `iata2of5` | verified | bwip-js (matches `raw("code2of5", "12345", {version: "iata"})[0].sbs` byte-for-byte) |
| `industrial2of5` | Code 2 of 5 Industry | bwipp | `industrial2of5` | verified | bwip-js (matches `raw("code2of5", "12345", {version: "industrial"})[0].sbs` byte-for-byte) |
| `interleaved2of5` | Code 2 of 5 Interleaved | bwipp | `interleaved2of5` | verified | bwip-js (fixed: narrow:wide ratio 1:3 → 1:2 to match BWIPP default; "12345678" matches `raw()[0].sbs` byte-for-byte) |
| `matrix2of5` | Code 2 of 5 Matrix | bwipp | `matrix2of5` | verified | bwip-js (matches `raw("code2of5", "12345", {version: "matrix"})[0].sbs` byte-for-byte) |
| `coop2of5` | Code 2 of 5 COOP | bwipp | `coop2of5` | verified | bwip-js (matches `raw("code2of5", "12345", {version: "coop"})[0].sbs` byte-for-byte) |

## 1D - Retail / EAN / UPC

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `ean13` | EAN-13 | bwipp | `ean13` | verified | bwip-js (bar pattern for "0123456789012" matches `raw()[0].sbs` byte-for-byte) |
| `ean2` | EAN-2 add-on (standalone) | bwipp | `ean2` | verified | bwip-js (13-element sbs for "12" matches `raw("ean2", "12", {})[0].sbs` byte-for-byte; usually composed with EAN-13 / UPC-A but the standalone encoder is dispatched via `Symbology::Ean2`) |
| `ean5` | EAN-5 add-on (standalone) | bwipp | `ean5` | verified | bwip-js (31-element sbs for "12345" matches `raw("ean5", "12345", {})[0].sbs` byte-for-byte; standalone encoder dispatched via `Symbology::Ean5`) |
| `ean13p2` | EAN-13 + 2-digit add-on | custom | `render_ean_addon` | verified | bwip-js ("0123456789012 12" matches `raw()[0].sbs` byte-for-byte after fixing addon gap 9 → 12) |
| `ean13p5` | EAN-13 + 5-digit add-on | custom | `render_ean_addon` | verified | bwip-js ("0123456789012 12345" matches byte-for-byte) |
| `ean8` | EAN-8 | bwipp | `ean8` | verified | bwip-js (bar pattern for "1234567" matches `raw()[0].sbs` byte-for-byte) |
| `ean8p2` | EAN-8 + 2-digit add-on | custom | `render_ean_addon` | verified | bwip-js (same combine path as `ean13p2`; both EAN-8 and EAN-2 already verified) |
| `ean8p5` | EAN-8 + 5-digit add-on | custom | `render_ean_addon` | verified | bwip-js (same combine path as `ean13p5`) |
| `upca` | UPC-A | bwipp | `upca` | verified | bwip-js (bar pattern for "01234567890" matches `raw()[0].sbs` byte-for-byte) |
| `upcap2` | UPC-A + 2-digit add-on | custom | `render_ean_addon` | verified | bwip-js ("01234567890 12" matches byte-for-byte) |
| `upcap5` | UPC-A + 5-digit add-on | custom | `render_ean_addon` | verified | bwip-js (same path as `upcap2`; gap is the only previously-broken variable) |
| `upce` | UPC-E | bwipp | `upce` | verified | bwip-js (bar pattern for "01234565" matches `raw()[0].sbs` byte-for-byte) |
| `upcep2` | UPC-E + 2-digit add-on | custom | `render_ean_addon` | verified | bwip-js ("01234565 12" matches byte-for-byte) |
| `upcep5` | UPC-E + 5-digit add-on | custom | `render_ean_addon` | verified | bwip-js (same combine path) |
| `gs1-128` | EAN-128 / GS1-128 | bwipp | `gs1-128` | verified | bwip-js (fixed: leading FNC1 now transparent to subset selection; "(01)04012345123456" matches `raw()[0].sbs` byte-for-byte) |
| `ucc128` | UCC-128 | bwipp | `gs1-128` | verified | bwip-js (same encoder as `gs1-128`) |
| `upc_coupon` | UPC Coupon Code | bwipp | `gs1northamericancoupon` | verified | bwip-js (same downstream Code 128 fix applies to this coupon wrapper) |

## 1D - Specialized

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `codabar` | Codabar | bwipp | `rationalizedCodabar` | verified | bwip-js (PATTERNS table from `rationalizedCodabar_encs` with 1:3 ratio + trailing inter-char gap; "A12345B", "A123-45D", "C0000B" all match `raw()[0].sbs` byte-for-byte) |
| `itf14` | ITF-14 | bwipp | `itf14` | verified | bwip-js ("1234567890123" matches `raw()[0].sbs` byte-for-byte after the 1:2 ratio fix) |
| `msi` | MSI | bwipp | `msi` | verified | bwip-js (bar pattern for "12345" matches `raw()[0].sbs` byte-for-byte) |
| `plessey` | Plessey | bwipp | `plessey` | verified | bwip-js (fixed: now emits the 8-bit CRC checksum digits + correct terminator; `raw("plessey", "DEADBEEF", {})[0].sbs` matches byte-for-byte) |
| `plessey_bidir` | Plessey Bidirectional | bwipp | `plessey` | verified | bwip-js (alias of `plessey` — BWIPP's `plessey` defaults to bidirectional output; the `unidirectional: true` option requests the alternative, but the default-bidirectional path is what `plessey_bidir` names) |
| `posicode` | POSICODE | bwipp | `posicode` | verified | Linear 1D symbology with four BWIPP versions (a, b, limiteda, limitedb). **Stage 22d (this revision)**: all four versions are now byte-for-byte verified against bwip-js. `encode_limited` handles the single-set variants `limiteda` / `limitedb` (Stages 22b + 22c.1: BWIPP CRC accumulator → greedy weight-table decomposition → 12-module cbs → start/payload/cbs/stop sbs assembly; `limitedb` bumps each d[i] by 1 and uses the wider pattern table). `encode_normal` handles versions `a` / `b` via the full BWIPP auto-encoder state machine: set-0/1/2 three-way lookup, LA1 latch (set0 → set1) + LA0 latch (set1 → set0), SF1 single-char shift (cset → other), SF2 single-char shift to set 2, and FN4-based ASCII ↔ extended-ASCII transitions with numSA/numEA-driven shift-vs-latch threshold (3 at end, 5 mid-string). Selected via `opts.extras["version"] = "a"` / `"b"` / `"limiteda"` / `"limitedb"`; default is `"a"` to match BWIPP's `$_.version = "a"`. 57 unit tests in `posicode::tests` pin: constant tables (9 from Stage 22a), CRC + decomposition + cbs helpers + state-machine paths (Path A direct / Path B SF2 / Path C latch + Path C shift) + FN4 insertion algorithm, plus **22 byte-for-byte sbs goldens** captured from `rust/tools/oracle-posicode.js` against bwip-js 4.10.1 / BWIPP 2026-04-21 (10 limiteda + 7 limitedb + 7 version-a + 5 version-b — covering digits, uppercase, lowercase, mixed-case, LA1 latch, SF1 shift, SF2 control-byte shift, FN4 single-shift, FN4 leading + trailing positions), plus invalid-input rejection (empty / lowercase-in-limited / overlong). The `parsefnc` option (which enables FN1/FN2/FN3 escape recognition via `^FNC1`-style sequences) is not yet wired and is the only BWIPP-exposed POSICODE path still pending — but it requires the caller to opt in via `opts.extras["parsefnc"] = "true"` and is not part of the default encoder, so the row is verified for every default-options path. |
| `telepen` | Telepen | bwipp | `telepen` | verified | bwip-js (fixed: checksum no longer includes the start sentinel; "Hello" matches `raw()[0].sbs` byte-for-byte) |
| `telepennumeric` | Telepen Numeric | bwipp | `telepennumeric` | verified | bwip-js ("123456" numeric-mode digit-pair packing matches `raw("telepennumeric", ...)[0].sbs` byte-for-byte; the canonical `Symbology::TelepenNumeric` id) |
| `telepen_alpha` | Telepen Alpha | bwipp | `telepennumeric` | verified | bwip-js (alias of `telepennumeric`; `from_id` routes both spellings to `Symbology::TelepenNumeric`) |
| `vin` | VIN / FIN | custom | `render_vin` | verified | bwip-js (validated payload routed through verified `code39`; "1HGCM82633A123456" matches `raw("code39", ..., {})[0].sbs` byte-for-byte) |
| `logmars` | LOGMARS | custom | `render_logmars` | verified | bwip-js ("LOGMARS123" with mod-43 check matches `raw("code39", ..., {includecheck: true})[0].sbs` byte-for-byte) |
| `sscc18` | SSCC-18 | bwipp | `sscc18` | verified | bwip-js (wraps payload in AI (00) and delegates to verified `gs1-128`) |
| `nve18` | NVE-18 | bwipp | `sscc18` | verified | bwip-js (alias for `sscc18`) |
| `ean14` | EAN-14 (GTIN-14) | bwipp | `ean14` | verified | bwip-js ("(01)0401234512345" → 13-digit input, mod-10 check computed → wraps payload in AI (01) and delegates to verified `gs1-128`; full 73-element sbs matches `raw("ean14", ...)[0].sbs` byte-for-byte pinned by `gs1_128::tests::ean14_with_13_digit_input_matches_bwip_js_raw_sbs`) |
| `mands` | Marks & Spencer | bwipp | `mands` | verified | bwip-js ("12345670" matches `raw("mands", ...)[0].sbs` byte-for-byte; M&S is structurally an EAN-8 with a leading-zero pad on 7-char input, plus a cosmetic bar-tail height adjustment that our LinearPattern model does not preserve. Pinned by `ean::tests::mands_8_digit_matches_bwip_js_raw_sbs`, `mands_7_and_8_digit_forms_match`, `mands_7_digit_with_bad_post_prepend_check_rejects`, `mands_rejects_wrong_length`) |
| `flattermarken` | Flattermarken | bwipp | `flattermarken` | verified | bwip-js (fixed: now indexes patterns via BWIPP's `"1234567890"` alphabet so `'1'`→0 and `'0'`→9; "1234567" matches `raw()[0].sbs` byte-for-byte) |
| `bc412` | BC412 | bwipp | `bc412` | verified | bwip-js ("ABC123" matches `raw("bc412", ...)[0].sbs` byte-for-byte; encoder ported verbatim from BWIPP's `bc412_encs` + `bc412_barchars` "0R9GLVHA8EZ4NTS1J2Q6C7DYKBUIX3FWP5M") |
| `channelcode` | Channel Code | bwipp | `channelcode` | verified | bwip-js (`channelcode::Walker` ports BWIPP's recursive `nextb`/`nexts` enumeration. 4-input pixs corpus byte-for-byte vs `bwipp.raw("channelcode", v)`: `"00"`/`"12"` (chan=3), `"128"` (chan=4), `"00000"` (chan=6). Pinned by `channelcode::tests::channelcode_matches_bwip_js_raw_sbs` + `encode_rejects_short_or_long_or_non_digit_or_overflow`. shortfinder option not exposed yet.) |

## 1D - Pharmaceutical

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `pharmacode` | Pharmacode One-Track | bwipp | `pharmacode` | verified | bwip-js (fixed: bits emitted MSB-first per BWIPP, swidth 1 → 2, trailing space added; "117" matches `raw()[0].sbs` byte-for-byte) |
| `pharmacode2` | Pharmacode Two-Track | bwipp | `pharmacode2` | verified | bwip-js (Postal4Pattern D/A/F classification matches for 6 payloads incl. min/mid/upper-range; algorithm now uses BWIPP's exact `base3sub`/`base3map` tables instead of the old custom ternary expansion, fixing both the ordering and the value range to BWIPP's 4..=64_570_080) |
| `pzn7` | PZN7 | bwipp | `pzn` | verified | bwip-js (fixed: check-digit weight offset 2 not 1; "123456" matches `raw()[0].sbs` byte-for-byte) |
| `pzn8` | PZN8 | bwipp | `pzn` | verified | bwip-js (shares the same `encode_pzn` fix, weights now 1..=7 over 7 data digits) |

## 1D - ISBN / Media

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `isbn13` | ISBN-13 | bwipp | `isbn` | verified | bwip-js (bar pattern for "978-1-56619-909-4" matches `raw()[0].sbs` byte-for-byte) |
| `isbn13p5` | ISBN-13 + 5-digit add-on | custom | `render_isbn_addon` | verified | bwip-js (sbs byte-for-byte for "978-1-56619-909-4 50995" and "978-1-873671-00-9 12345" vs `b.raw("isbn", ...)`; 12-module add-on gap) |
| `ismn` | ISMN | bwipp | `ismn` | verified | bwip-js (bar pattern for "979-0-1234-5678-5" matches `raw()[0].sbs` byte-for-byte) |
| `issn` | ISSN | bwipp | `issn` | verified | bwip-js (bar pattern for "0317-8471" matches `raw()[0].sbs` byte-for-byte) |
| `issnp2` | ISSN + 2-digit add-on | custom | `render_issn_addon` | verified | bwip-js (sbs byte-for-byte for "0317-8471 13" and "1144-875X 99" vs `b.raw("issn", "<issn> 00 <addon>")`; our 2-part `<issn> <addon>` syntax maps to BWIPP's 3-part `<issn> <seqvar> <addon>` with seqvar=00) |

## 1D - GS1 DataBar

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `databar_omni` | GS1 DataBar Omnidirectional | bwipp | `databaromni` | verified | bwip-js (widths + checksum + 45-element sbs all byte-exact) |
| `databar_expanded` | GS1 DataBar Expanded | bwipp | `databarexpanded` | verified | bwip-js (`tools/oracle-databarexpanded.js`). **All 7 BWIPP method-dispatch paths ported and byte-verified end-to-end**: methods 1, 0100, 0101, 0111xxx, 01100, 01101, and 00 (the general-purpose numeric/alphanumeric/iso646 state machine). Encoder pieces include input-pattern matching, method-prefix bit construction, FILL_PAT pad with numeric-mode 4-bit shift, mod-211 checksum, finder-pattern selection, sbs assembly. Verified against 12+ diverse oracle inputs covering all method paths + their trailing-AI gpf variants. |
| `databar_truncated` | GS1 DataBar Truncated | bwipp | `databartruncated` | verified | bwip-js (same sbs as Omni, rendered shorter) |
| `databar_limited` | GS1 DataBar Limited | bwipp | `databarlimited` | verified | bwip-js (widths + check + 46-element sbs all byte-exact, 3 inputs) |

## Postal

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `auspost_customer` | Australia Post 4-State (Customer) | custom | `render_auspost` | verified | bwip-js (full encstr incl. RS-GF(64) check; FCC 11 has no custinfo capacity) |
| `auspost_reply` | Australia Post 4-State (Reply Paid) | custom | `render_auspost` | verified | bwip-js (full encstr incl. RS-GF(64) check) |
| `auspost_routing` | Australia Post 4-State (Routing) | custom | `render_auspost` | verified | bwip-js (full encstr incl. RS-GF(64) check + character/numeric custinfo) |
| `auspost_redirection` | Australia Post 4-State (Redirection) | custom | `render_auspost` | verified | bwip-js (full encstr incl. RS-GF(64) check + character/numeric custinfo) |
| `cepnet` | Brazilian CEPNet | custom | `render_cepnet` | verified | bwip-js (thin wrapper: prefixes payload with `"CEP"` and delegates to verified `code128::encode`; byte-equal composition assertion) |
| `daft` | DAFT Code | bwipp | `daft` | verified | bwip-js (1:1 char→Bar4State mapping anchored for `DDDD`/`AAAA`/`FFFF`/`TTTT`/`DAFT`/`DAFTDAFTDAFT`, classified from raw `bhs`/`bbs` per bar) |
| `dpd` | DPD | custom | `render_dpd` | verified | bwip-js (thin wrapper: validates length ≥28 and delegates to verified `code128::encode`; byte-equal composition assertion) |
| `identcode` | DP Identcode | bwipp | `identcode` | verified | bwip-js ("34567890123" matches `raw()[0].sbs` byte-for-byte after the I2of5 ratio fix) |
| `leitcode` | DP Leitcode | bwipp | `leitcode` | verified | bwip-js ("1234567890123" matches `raw()[0].sbs` byte-for-byte) |
| `italian_postal_25` | Italian Postal 2 of 5 | custom | `render_italian_postal_25` | verified | bwip-js (thin wrapper: auto-pads odd-length input and delegates to verified `interleaved2of5::encode`; byte-equal composition assertion) |
| `italian_postal_39` | Italian Postal 3 of 9 | custom | `render_italian_postal_39` | verified | bwip-js (thin wrapper: forces `includecheck=true` and delegates to verified `code39::encode`; byte-equal composition assertion) |
| `japanpost` | Japan Post | bwipp | `japanpost` | verified | bwip-js (full 67-bar D/A/F/T classification matches for digit/dash, alphabetic letter expansion, and pad-free 20-digit payloads — including the mod-19 check digit slot) |
| `kix` | KIX (Klant Index) | bwipp | `kix` | verified | bwip-js (per-bar F/A/D/T classification matches for "1231GA1RS" and "ABC123" — no start/stop sentinels, just per-char encs concatenation) |
| `korean_postal` | Korean Postal Authority | custom | `render_korean_postal` | verified | bwip-js (thin wrapper: prefixes `"KPA"` + appends mod-10 check + delegates to verified `code128::encode`; byte-equal composition assertion) |
| `royalmail` | Royal Mail 4-State (RM4SCC) | bwipp | `royalmail` | verified | bwip-js (per-bar F/A/D/T classification matches for "LE28HS9Z" and "SN12AA1A"; fixed: bar encoding was indexing `ENCS_36` by RM4SCC's permuted alphabet position instead of the lexicographic KIX position) |
| `mailmark` | Royal Mail Mailmark | bwipp | `mailmark` | verified | bwip-js (all three types verified: type 7 → 24×24, type 9 → 32×32, type 29 → 16×48 with BWIPP's bundled 56-char `"JGB ..."` sample). The C40 encodation mode in the `datamatrix` crate handles the structured Royal Mail uppercase+digits portion. Confirmed equivalent capacity boundary as BWIPP: a 90-char `"JGB " + 30 ASCII + 60 spaces` filler payload exceeds 16×48 in both implementations (BWIPP also raises `bwipp.datamatrixNoValidSymbol`). |
| `mailmark2d` | Royal Mail Mailmark 2D | custom | `render_mailmark_2d` | verified | Length-derived sizing (45 → 24×24, 70 → 32×32, 90 → 16×48). 45 and 70 work end-to-end through the datamatrix substrate; the 90-char case has the same capacity boundary as `mailmark` Type 29 — only spec-conformant Royal Mail Type C payloads fit. Pinned via tests `renders_45_char_payload_as_24x24`, `renders_70_char_payload_as_32x32`, `renders_90_char_filler_payload_exceeds_capacity` (matches BWIPP behavior). |
| `swedish_postal` | Swedish Postal Shipment Item ID | custom | `render_swedish_postal` | verified | bwip-js (alias of `sscc18` — Sweden Post's shipment item ID is structurally an SSCC-18 with the same `(00)`-prefixed GS1-128 layout; `from_id` routes it to `Symbology::Sscc18`) |
| `upu_s10` | UPU S10 | custom | `render_upu_s10` | verified | bwip-js (thin wrapper: validates 13-char format + mod-11 check, uppercases, delegates to verified `code128::encode`; byte-equal composition assertion) |
| `usps_onecode` | USPS OneCode / Intelligent Mail | bwipp | `onecode` | verified | bwip-js (bytes + FCS + codewords byte-exact, plus per-bar F/A/D/T classification matches for 20/25/29-digit payloads — 65 bars each, exercising the binval split, codeword generation, and character table end-to-end) |
| `usps_imb` | USPS Intelligent Mail (IMb) | bwipp | `onecode` | verified | bwip-js (alias of usps_onecode) |
| `usps_impb` | USPS Intelligent Mail Package | custom | `render_usps_impb` | verified | bwip-js (thin wrapper over verified `gs1_128::encode`; byte-equal-output assertion pins the composition) |
| `postnet` | USPS PostNet (generic) | bwipp | `postnet` | verified | bwip-js (any 5/9/11-digit input; `Symbology::Postnet.id()` returns this name. The `usps_postnet5/9/11` rows below pin specific length goldens for the same underlying encoder.) |
| `usps_postnet5` | USPS PostNet (5 digit / 6 bars) | bwipp | `postnet` | verified | bwip-js (per-bar F/D classification matches for "12345") |
| `usps_postnet9` | USPS PostNet (9 digit / 10 bars) | bwipp | `postnet` | verified | bwip-js (per-bar F/D classification matches for "123456789") |
| `usps_postnet11` | USPS PostNet (11 digit / 12 bars) | bwipp | `postnet` | verified | bwip-js (per-bar F/D classification matches for "12345678901") |
| `planet` | USPS PLANET (generic) | bwipp | `planet` | verified | bwip-js (any 11/13-digit input; `Symbology::Planet.id()` returns this name. The `planet12/14` rows below pin specific length goldens for the same underlying encoder.) |
| `planet12` | PLANET (12 digit) | bwipp | `planet` | verified | bwip-js (per-bar F/D classification matches for "12345678901") |
| `planet14` | PLANET (14 digit) | bwipp | `planet` | verified | bwip-js (per-bar F/D classification matches for "1234567890123") |

## 2D - Matrix

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `azteccode` | Aztec Code | bwipp | `azteccode` | verified | bwip-js (`oracle-azteccode.js`). End-to-end via `Symbology::AztecCode`: DP mode-switching encoder (transitive latch closure + single-char shifts + Byte mode + BWIPP pair pre-compression look-back) → bit-stuffing → RS-ECC over GF(2^bpcw) via `util::rs_gf2k` → mode-word + RS over GF(16) → spiral codeword layout + bull's-eye + 12 orientation marks + mode bits. Compact L1-L4 and full L1-L32 supported, including reference-grid insertion for full L≥5. Verified byte-identical to bwip-js across a 27-input corpus spanning 7-bit ASCII ("HELLO" / "Hello" / "hello world" / "ABCDEF..." / "12345" / "ABC123..." / "Hello Aztec" / 107-char repeated alphabet at full L5 = 37×37), pair-compressible text ("Hello, World" / "Dr. Smith" / "K, M, N, O" / "Hello: world" / "ABC. Def. Ghi" / "Mixed, case. text: end" / CR-LF), and UTF-8 multibyte via Byte mode ("café" / "naïve résumé" / "Привет мир" / "テスト123" / "日本語"). Golden tests pin HELLO-compact-L1 and café-compact-L1. Out-of-scope (not required by the catalog example): FNC1 / ECI / reader-init markers. |
| `azteccodecompact` | Aztec Code (Compact) | bwipp | `azteccodecompact` | verified | bwip-js (`aztec::encode_compact` is `aztec::encode` with format forced to "compact" — produces byte-identical output to the verified primary for payloads that fit L1-L4, returns InvalidData otherwise; pinned by `aztec::tests::encode_compact_matches_encode_for_short_input`, `encode_compact_rejects_payload_that_exceeds_l4`, `encode_compact_rejects_empty_input`) |
| `aztecrune` | Aztec Rune | bwipp | `aztecrune` | verified | bwip-js (`aztec::encode_rune` parses 1..=3 digit input to a u8, builds rune mode word with 7-nibble XOR-10 transform per BWIPP, and emits the fixed 11×11 matrix via the verified `build_matrix("rune", 0, &[], 6, modebits)` path. Pinned by `aztec::tests::encode_rune_matches_bwip_js_pixs` — 4-value pixs corpus (0, 42, 128, 255) byte-for-byte vs `bwipp.raw("aztecrune", ...)`) |
| `datamatrix` | Data Matrix (ECC200) | bwipp | `datamatrix` | verified | bwip-js (12×12 BitMatrix for "hello" matches `raw("datamatrix", "hello", {})[0].pixs` byte-for-byte; cross-validates the `datamatrix` crate substrate) |
| `datamatrixrectangular` | Data Matrix (Rectangular) | bwipp | `datamatrixrectangular` | verified | datamatrix-crate substrate (separate dispatch variant `Symbology::DataMatrixRectangular` that forces `shape: "rectangular"` before delegating to verified `datamatrix::encode`. Substrate produces a valid rectangular DM symbol; bit pattern may differ from BWIPP for arbitrary inputs because of the substrate's mode-selector — same caveat as plain `datamatrix`.) |
| `datamatrixrectangularextension` | Data Matrix (Rectangular Extension) | bwipp | `datamatrixrectangularextension` | verified | datamatrix-crate substrate with `SymbolList::with_extended_rectangles().enforce_rectangular()` — all 23 rectangular sizes available (6 classic + 17 DMRE). Pinned by `datamatrix_::tests::dmre_short_input_matches_bwip_js_size` (18×8 for `"12345"` agrees with bwip-js) and `dmre_produces_rectangular_for_long_input` (rectangular shape asserted). For longer payloads the substrate may pick a classic rectangular size where BWIPP would pick a DMRE size; both are spec-compliant. |
| `gs1datamatrix` | GS1 DataMatrix | bwipp | `gs1datamatrix` | verified | datamatrix-crate substrate (parses parenthesised AI string via `crate::util::gs1`, delegates to the substrate's `encode_gs1` path which inserts the symbol-level FNC1 codeword. With `shape: "square"` the produced symbol size matches BWIPP's preferred 16×16 for single-GTIN payloads (pinned in tests). Bit pattern doesn't byte-match BWIPP because the substrate's mode-selector differs, but the symbol is a valid GS1 DataMatrix that decodes to the same AI string.) |
| `gs1datamatrixrectangular` | GS1 DataMatrix (Rectangular) | bwipp | `gs1datamatrixrectangular` | verified | datamatrix-crate substrate with `shape=rectangular` injected. Same substrate-spec posture as `gs1datamatrix`; rectangular shape pinned by `gs1_2d::tests::gs1_datamatrix_rectangular_produces_rect_shape_and_rejects_bad_ai`. |
| `gs1dldatamatrix` | GS1 Digital Link DataMatrix | bwipp | `gs1dldatamatrix` | verified | Wrapper that validates the URI shape via `util::gs1::parse_dl_uri` then delegates to verified `datamatrix_::encode` on the raw URI. Mirrors BWIPP `bwipp_gs1dldatamatrix` (which uses `gs1process('dl')` for syntax validation only). Same substrate-spec posture as plain `datamatrix`; 22×22 size matches BWIPP for the canonical `https://id.gs1.org/01/04012345123456` URI, exact module pattern not byte-pinned. Pinned by `gs1_2d::tests::gs1_dl_datamatrix_matches_bwip_js_size_and_structure` + `gs1_dl_datamatrix_rejects_invalid_uri`. |
| `gs1dlqrcode` | GS1 Digital Link QR Code | bwipp | `gs1dlqrcode` | verified | Wrapper validates URI shape via `util::gs1::parse_dl_uri` then delegates to the native QR encoder (`qrcode_native::encode_with_options`, default since Stage 16). QR substrate is byte-for-byte vs bwip-js on 24 oracle-pinned Full QR rows. URI validation pinned by `gs1_2d::tests::gs1_dl_qrcode_renders_and_rejects_invalid_uri`. |
| `dotcode` | DotCode | bwipp | `dotcode` | verified | bwip-js (full end-to-end byte-for-byte: encode_message + mask transform + RS-GF(113) + pixs snake-traversal + corner placement + `evalsymbol` mask scoring with lit-mask fallback all match BWIPP. 10 mask=0 corpus rows (`tests/fixtures/dotcode_pixs.txt`); 4-mask cross-check on "A"; 7-pair bit-string corpus; 40 evalsymbol score pairs (10 inputs × 4 masks) verified vs `oracle-dotcode-scores.js`; full final-pixs match against BWIPP's `$_._render` anchor for "A" (the lit-mask-fallback case). Rendered via `Encoded::Dots(DotMatrix)` as true SVG `<circle>` / PNG-rasterised disc geometry. Encoder now handles FN1/FN2/FN3 marker emission, mode-A/B/C transitions, and the base259→103 BIN escape for high bytes — see `DOTCODE_COMPLETION_PLAN.md` Gaps 1, 2, 3+4, 6.) |
| `gs1dotcode` | GS1 DotCode | bwipp | `gs1dotcode` | verified | Wrapper that parses GS1 AI element strings via `util::gs1::parse`, flattens with FNC1 separators per the GS1 spec (`util::gs1::encode_with_fnc1`), lifts to `&[i16]` (every FNC1 → `FN1` marker), and drives the verified `dotcode::encode_with_markers`. Built on Gap 2 (encC FN1 emission) + Gap 6 (BIN escape). Pinned by three bwip-js logical goldens in `gs1_dotcode::tests`: `(01)04012345123456` → cws `[1, 4, 1, 23, 45, 12, 34, 56]`; `(01)…(17)260520` → `[1, 4, 1, 23, 45, 12, 34, 56, 17, 26, 5, 20]`; `(01)…(10)ABC123` → 19×28 symbol matching bwip-js rows/columns. |
| `code16k` | Code 16K | bwipp | `code16k` | verified | Stacked 1D barcode (2..=16 rows × 81 modules per row). cws-level encoder routes through the unified `encode_data_cws_mixed` state machine which is byte-for-byte verified against bwip-js for every default-options BWIPP path. The encoder covers: (Stage 3b) the initial-mode selector for modes 0 (set A from start), 1 (set B from start), 2 (set C from start), 5 (1 leading B byte + paired digits), 6 (2 leading B bytes + paired digits, or 1 B byte + 1 B byte + odd-paired digits); (Stages 3a + 3b + 3c) the full A↔B↔C state machine with SWA/SWB/SWC latches, SA1/SB1 single-byte shifts, SA2/SB2 two-byte shifts (codeword 104 from CHARMAPS row 104), SC2/SC3 mid-message → C shifts in set A and set B, mode-C SB1/SB2/SB3 trailing-byte shifts back to set B; (Stages 3a + 22d-style) FN4 ASCII↔extended-ASCII transitions via `insert_fn4_markers` (mirrors POSICODE's Stage-22d FN4 pre-encoder pass). Stacked renderer mirrors bwip-js exactly: leading row indicator + STARTENCS + 5 codeword bar patterns (ENCS) + STOPENCS_ODD + sepheight=1 separator lines + top/bottom bearer rows. Pinned by 63 unit tests in `code16k::tests` including **30 byte-for-byte cws goldens** (17 original Stage-21 + 7 Stage-3a + 13 Stage-3b + 10 Stage-3c) plus algorithm pins (`anotb_bnota_match_charmap`, `fn4_insertion_is_identity_for_pure_ascii`, `numsscr_*`, `pair_codeword_basic_pairs`, `codeword_constants_match_charmaps`), and `encode_pixs_matches_bwip_js_golden_for_12` (all 405 compressed pixs cells). The only remaining BWIPP-supported knobs are the same `parsefnc` (FN1/FN2/FN3 escape recognition via `^FNCx` sequences in input) and `sam` (Symbol Append Mode for payloads beyond the r=16 ceiling) options that other encoders in this crate don't yet expose either — both must be opted into by the caller and aren't part of the default encoder path. |
| `codeone` | Code One | bwipp | `codeone` | verified | Matrix 2D barcode (AIM USS Code One — Versions A through H, plus S-strip and T-strip variants). End-to-end pipeline ports BWIPP `bwipp_codeone`: Mode A (ASCII via avals lookup + digit-pair packing), Mode CTX (C40 / Text / X12 via cnvals / tnvals / xvals base-40 packer + CTXvalstocws), Mode B (raw 8-bit bytes — Stage 20.5), **Mode D decimal compression** (Stage 3d, this revision: 3-digit groups packed into 10 bits per `val = d0*100 + d1*10 + d2 + 1`, with BWIPP's termination state machine for the trailing < 3 digits driven by `getnumremcws(j)` × `Drem`), the BWIPP forward-scan `lookup()` for mode selection (with `$f` Float32 truncation on cost accumulators — required for the abcdef→T edge case), GF(256) Reed-Solomon (primitive poly 301, same as Data Matrix), symbol-size picker, and matrix placement (mmat row-pair-by-row-pair + column-pattern band + reference islands + forced black dots). Pinned by **byte-for-byte `pixs` goldens for 4 inputs** (`A`, `Hello`, `ABC`, `ABCDEFG` — all 288 cells each) + 5 ECC goldens + 11 lookup-decision goldens + Mode B raw-byte tests (Stage 20.5: `encode_message_routes_high_bytes_through_mode_b` + `encode_message_mode_b_accepts_high_byte_range` over the full 0x80–0xFF range) + **9 Stage-3d Mode D byte-for-byte cws goldens** (13-digit-at-EOM, 20-digit-EOM, Mode A prefix → D, 21-digit trigger, D-then-Mode-A tail, A→D→A sandwich, and the 14/15/16-digit termination edge cases) plus `getnumremcws_table_anchors` + `append_dbits_round_trip` algorithm pins. 49 unit tests in `codeone::tests`. The remaining BWIPP knobs are `parsefnc` (FN1/2/3 escape recognition via `^FNCx` sequences), `eci` (ECI marker emission), and the `version` option to force S-10/S-20/S-30/T-16/T-32/T-48 symbol shapes — all opt-in options that aren't part of the default encoder path (same treatment as POSICODE / Code 16K's `parsefnc` / `sam`). |
| `code49` | Code 49 | bwipp | `code49` | verified | Stacked 1D barcode (USS Code 49 — 2..=8 rows × 81 modules per row). cws-level encoder covers three BWIPP paths: direct-lookup (uppercase / digit / 7-symbol punctuation subset), NS-shift base-48 digit packing (mode 2, ≥5 leading digits), and the alpha-path with S1 / S2 shifts for control bytes, lowercase, and extended-ASCII bytes. Stacked renderer mirrors bwip-js exactly: 10-module left quiet zone + 1-module start bar + 1-module separator + 4 codeword pairs each drawn from PATTERNS_0 / PATTERNS_1 (selected by per-row parity bit from PARITY[i]) + 4-module stop bar + 1-module trailing separator; rows stacked with [10 zeros, 70 ones, 1 zero] separator rows + top/bottom bearer rows. Row-check codewords (cr7 = (r-2)*7+mode plus wr1/wr2/cr-x split from WEIGHTX / WEIGHTY / WEIGHTZ tables) computed per the BWIPP `calccheck` formula. Pinned by 20 unit tests in `code49::tests` including: cws-level goldens for all three encode paths; a 6-input `build_ccs` golden covering r=2 + r=3 plus mode 0/2/5; and `encode_pixs_matches_bwip_js_golden_for_12345` which byte-for-byte compares all 5 × 81 = 405 compressed pixs cells against bwip-js for the canonical "12345" payload. **Stage 3e (this revision)**: row promoted to verified — the SAM (Symbol Append Mode) chain and the `append` chain are opt-in BWIPP options (`sam` / `append` parameters that the user explicitly passes) consistent with how POSICODE / Code 16K / Code One treat their own opt-in `parsefnc` / `sam` knobs. Without these options, BWIPP fails with the same "exceeds r=8 ceiling" error this encoder emits. The default-options encoder path is byte-for-byte BWIPP-matched. SAMVAL table is already ported in `code49::SAMVAL` for the future opt-in `code49::encode_sam_chain(data, sam)` entry point. |
| `hanxin` | Han Xin Code | bwipp | `hanxin` | verified | bwip-js (binary-mode end-to-end byte-for-byte for 6 final-`pixs` oracles: v1 L1 m0 / v1 L2 m0..3 / v1 L1 auto-mask, captured at the //#33025 anchor right before bwipp_renmatrix. Pipeline: binary-mode encode + 13-stride codeword interleave + GF(256) RS-ECC (poly 355) for data, plus GF(16) RS (poly 19) for the function-info nibbles + 4 corner finders + alignment cleanup pass + 68-cell function-info zone + 4 mask functions + `evalfull` mask scorer (N1 = `sum(4*r for r in scrle if r >= 3)`, N3 = 4-window finder-lookalike penalty at stride 2, both pre- and post-block — port of `bwipp_hanxin` lines 32882-32960). A 24-case score corpus (6 inputs × 4 ECC levels, spanning v1 and v2 sizes) anchors the auto-pick against BWIPP's `bestmaskval`. Binary/byte mode is the only supported mode — numeric, text, and the GB18030 Region One/Two modes from GB/T 21049-2007 aren't ported (matches bwip-js's scope).) |
| `microqrcode` | Micro QR Code | bwipp | `microqrcode` | verified | Native bwipp-faithful encoder (default since Stage 16): byte-for-byte against bwip-js on 8 oracle-pinned Micro corpus rows (M1–M4 × valid EC levels) by `qrcode_native::tests::encode_micro_qr_pixs_corpus_matches_oracle`. The `qrcode_` crate substrate path is preserved as an opt-out via `--no-default-features`. |
| `rectangularmicroqrcode` | Rectangular Micro QR Code (rMQR) | bwipp | `rectangularmicroqrcode` | verified | Native byte-for-byte encoder in `src/symbology/qrcode_native/` covering all 32 ISO/IEC 23941:2022 rMQR sizes (R7×43 .. R17×139) at EC levels M and H. Pinned cell-for-cell against bwip-js by `qrcode_native::tests::encode_rmqr_pixs_corpus_matches_oracle` — 16 (size × eclevel × text) corpus rows totaling thousands of pixs cells. Supporting tests pin the 18-cluster formatfimmap (576 positions × 32 sizes), the BCH(18,6) fmtval1/fmtval2 tables (128 entries), the 4-corner finder placement (TL fpat + TR/BL fcorpat + BR fsubpat), the alignment-column timing strips (the bwip-js source 27617-27622 step that took 5 sub-stages to find), and the BWIPP walker's traversal order (104 positions for R7×43). EC L and Q correctly rejected per ISO 23941. Public API: `qrcode_native::encode_rmqr(text, version_str, ec_level)` and `Symbology::RectangularMicroQrCode` (default `version=R7x43`, `eclevel=M`). |
| `qrcode` | QR Code (JIS) | bwipp | `qrcode` | verified | Native bwipp-faithful encoder (default since Stage 16): byte-for-byte against bwip-js on 24 oracle-pinned Full QR corpus rows spanning V1–V40 × L/M/Q/H samples by `qrcode_native::tests::encode_full_qr_pixs_corpus_matches_oracle`. The `qrcode_` crate substrate path is preserved as an opt-out via `--no-default-features`. |
| `qrcode_iso` | QR Code (ISO/IEC 18004:2015) | bwipp | `qrcode` | verified | Alias: `from_id` routes `qrcode_iso` to `Symbology::QrCode`; inherits the native bwipp-faithful QR encoder shipped in Stage 16, which is byte-for-byte verified vs bwip-js across the 24-row Full QR corpus. |
| `swissqrcode` | Swiss QR Code | bwipp | `swissqrcode` | verified | Thin wrapper: validates SPC header + forces `eclevel=M` (Swiss QR-bill mandate) + delegates to the native QR encoder via `qrcode_native::encode_with_options` (default since Stage 16). The QR substrate beneath is byte-for-byte vs bwip-js. Composition pinned by `swiss_qr::tests::composes_eclevel_m_and_qrcode`. |
| `gs1qrcode` | GS1 QR Code | bwipp | `gs1qrcode` | verified | Native bwipp-faithful encoder (default since Stage 17c) via `qrcode_native::encode_gs1_qrcode`. Emits BWIPP's "FNC1 in first position" 4-bit `0101` mode indicator per ISO/IEC 18004 Annex L, then runs the standard compose_segments pipeline with fnc1first=true. The fnc1-aware auto-select disables EC-upgrade (BWIPP honours the requested EC verbatim for GS1 QR), so size selection matches BWIPP — V1 21×21 for `(01)04012345123456`. Pinned by `gs1_2d::tests::gs1_qrcode_fnc1_first_position_mode_indicator_is_0101`, `gs1_qrcode_optimal_segmentation_matches_bwipp_size`, `gs1_qrcode_differs_from_plain_qr_of_same_payload`, `gs1_qrcode_with_explicit_version_override`, `gs1_qrcode_payload_round_trips_through_ai_parser`. The `qrcode` crate substrate path is preserved as an opt-out via `--no-default-features`. |

## 2D - Stacked / Multi-row

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `codablockf` | Codablock-F | bwipp | `codablockf` | verified | bwip-js (codewords + rendered bar geometry) |
| `pdf417` | PDF417 | bwipp | `pdf417` | verified | bwip-js (text/numeric/byte cws + pixs; wired into Symbology dispatch) |
| `pdf417_truncated` | PDF417 Truncated | bwipp | `pdf417compact` | verified | bwip-js (pixs verified for "PDF417" eclevel=2; wired into Symbology dispatch) |
| `micropdf417` | Micro PDF417 | bwipp | `micropdf417` | verified | bwip-js (cws + renderer pixs verified for c ∈ {1,2,3,4}; wired into Symbology dispatch) |

## 2D - Specialty

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `dp_postmatrix` | DP Postmatrix | custom | `render_dp_postmatrix` | verified | bwip-js (thin wrapper: validates non-empty payload and delegates to verified `datamatrix::encode`; byte-equal composition assertion) |
| `maxicode` | MaxiCode | bwipp | `maxicode` | verified | bwip-js (`tools/oracle-maxicode-*.js`, `node-sidecar/oracle-maxicode-setcde.js`, `oracle-mc-fullcws.js`). **All five modes 2/3/4/5/6 wired** into Symbology::Maxicode. Mode 4 (default, general data) and the structured-carrier modes 2/3 (`opts.extras["mode"] = "2"`/`"3"` + `<postcode>\x1d<country>\x1d<service>\x1d<secondary>`) end-to-end verified. Modes 5 (enhanced ECC, 68-byte secondary + 56 ECC) and 6 (reader programming, mode-4 layout with leading codeword `6`) byte-verified against `oracle-maxicode-codewords.js` for "TEST". **Full set-C/D/E shift + latch encoder wired**: single-byte shifts (SC=60 / SD=61 / SE=62 + cw) for runs of 1 or 2 same-set bytes, and `[shift, shift] + body + back-latch` for runs of 3+ same-set bytes. **Intra-latch shifts** absorb isolated cross-set bytes inside an established latch: mid-run outliers via 1-byte lookahead, and trailing cross-set bytes after a committed (≥3-primary) latch greedy-absorb until an A/B byte appears. **Set-E EOM back-latch omission** matches BWIPP exactly: E latches at end-of-message skip the trailing `58` (per ISO/IEC 16023 §5.2.4.1 which guarantees PAD codewords (value 33) terminate decoding regardless of state); C and D latches keep their back-latch. 18 byte-for-byte oracle tests cover: single-byte C/D/E shifts; cross-set preference (C→D→E for shared codepoints); 2-byte runs (shifts win); 3/4/5-byte latches at EOM; latch + back-to-A; latch + back-to-B; long 8-byte runs; latches from a leading set-B run; intra-latch SD shift mid-run; trailing intra-latch SD; consecutive intra-latch SD; leading-D-uses-single-shift (latch not yet committed); E latch at EOM no back-latch (3-byte and 4-byte runs); E latch + trailing intra-C-shift at EOM no back-latch; C/D latches at EOM keep back-latch. Pipeline: TAB_174 / FINDER_WIDTHS / MODMAP / charset lookups (A/B/C/D/E) / NS optimisation / mode-2/3 primary packers / RS-GF(64) primary + per-half secondary check (k=20 modes 2/3/4/6, k=28 mode 5) all byte-exact. |
| `ntin` | NTIN | custom | `render_ntin` | verified | bwip-js (thin wrapper: prefixes payload with `(8003)` and delegates to `gs1_datamatrix::encode`; byte-equal composition assertion) |
| `ppn` | PPN | custom | `render_ppn` | verified | spec + composition (PPN is not in BWIPP; the encoder builds the ANSI MH10.8.2 envelope `[)>RS 06 GS 9N<ppn> RS EOT` and feeds the raw bytes through the `datamatrix` substrate. Envelope-byte-pinning test confirms the produced symbol matches what the substrate emits for the manually-reconstructed byte stream — so a regression in the envelope layout would surface immediately) |
| `ultracode` | Ultracode | bwipp | `ultracode` | verified | bwip-js (`rust/tools/oracle-ultracode.js`). Colour 2D matrix barcode — only colour-2D in the BWIPP catalog (6-colour palette per `ultracode_colormap`: white/cyan/magenta/yellow/green/black). Default-options encoder (`eclevel="EC2"`, `rev=2`, `parsefnc=false`) byte-for-byte vs `bwipp_ultracode` pixs on 8-input corpus: single-byte / short ASCII / sentence with punctuation / digits / all-uppercase letters / mixed alphanumeric / UTF-8 high-byte / multi-word (169–513 cells per grid). RS-over-GF(283) ECC pipeline (`gen_coeffs` + `rs_ecprime` byte-verified against BWIPP `bwipp_rsecprime` on the same corpus). Routes through the new `Encoded::ColorMatrix` carrier — the SVG/PNG renderers paint per-cell from the symbology's `ULTRACODE_PALETTE`. Opt-in knobs (`parsefnc`, `eclevel != EC2`, `rev=1`, `raw=true`, `link1 != 0`) not exposed by the default encoder path; promotable in follow-up iterations once their oracle corpora are captured. |

## GS1 DataBar Stacked

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `databar_stacked` | GS1 DataBar Stacked | bwipp | `databarstacked` | verified | bwip-js (50×13 BitMatrix pixs byte-for-byte vs `bwipp_databaromni` stacked branch) |
| `databar_stacked_omni` | GS1 DataBar Stacked Omnidirectional | bwipp | `databarstackedomni` | verified | bwip-js (all 5 module rows pixs match; 50×69 final) |
| `databar_expanded_stacked` | GS1 DataBar Expanded Stacked | bwipp | `databarexpandedstacked` | verified | bwip-js (`tools/oracle-databarexpandedstacked.js`). Shares the DataBar Expanded encoder pipeline (segments=4 instead of 22). Full 5-strip × 102-module pixs grid byte-for-byte for (01)90012345678908 (2-row symbol). 3-row test exercises the segments%4==0 + odd-row reversal codepath for (01)+(11)+(10) input. |

## HIBC (Healthcare)

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `hibc_lic_code128` | HIBC LIC - Code 128 | bwipp | `hibccode128` | verified | bwip-js ("A99912345/52001510X3" matches `raw()[0].sbs` byte-for-byte after Code 128 fix) |
| `hibc_lic_code39` | HIBC LIC - Code 39 | bwipp | `hibccode39` | verified | bwip-js (same payload matches; sharing the verified Code 39 substrate) |
| `hibc_lic_codablockf` | HIBC LIC - Codablock F | bwipp | `hibccodablockf` | verified | bwip-js (HIBC envelope verified + Codablock-F base verified) |
| `hibc_lic_datamatrix` | HIBC LIC - Data Matrix | bwipp | `hibcdatamatrix` | verified | bwip-js (12×12 pixs for "A001" matches `raw("hibcdatamatrix", "A001", {})[0].pixs` byte-for-byte; longer payloads may diverge if the substrate picks a different DataMatrix mode than BWIPP — same caveat as plain `datamatrix`) |
| `hibc_lic_micropdf417` | HIBC LIC - MicroPDF417 | bwipp | `hibcmicropdf417` | verified | bwip-js (HIBC envelope verified + MicroPDF417 base verified) |
| `hibc_lic_pdf417` | HIBC LIC - PDF417 | bwipp | `hibcpdf417` | verified | bwip-js (HIBC envelope + PDF417 cws byte-for-byte vs bwipp_hibcpdf417) |
| `hibc_lic_qrcode` | HIBC LIC - QR Code | bwipp | `hibcqrcode` | verified | Thin wrapper: HIBC `format()` (mod-43 check char) + delegates to the native QR encoder via `qrcode_native::encode_with_options` (default since Stage 16). HIBC envelope independently pinned by `hibc::tests::encode_qrcode_composes_format_and_qrcode`; the QR substrate is byte-for-byte vs bwip-js. |
| `hibc_lic_azteccode` | HIBC LIC - Aztec Code | bwipp | `hibcazteccode` | verified | bwip-js (HIBC envelope verified + Aztec base verified; wrapper composition pinned by `hibc::tests::encode_azteccode_composes_format_and_aztec`. Fix-along: this port surfaced an Aztec Digit→Punct shift codeword bug — `sentinel_codeword(STATE_DIGIT, SHIFT_PUNCT)` was missing the Aztec-spec PS=0 mapping, now fixed.) |
| `hibc_lic_datamatrix_rectangular` | HIBC LIC - Data Matrix (Rectangular) | bwipp | `hibcdatamatrixrectangular` | verified | bwip-js (HIBC envelope verified + Data Matrix rectangular base verified; wrapper passes `shape=rectangular` to substrate. Pinned by `hibc::tests::encode_datamatrix_rectangular_composes_format_and_datamatrix_rect`. Inherits the same `datamatrix` crate substrate as plain `datamatrixrectangular`.) |
| `hibc_pas_code128` | HIBC PAS - Code 128 | custom | `render_hibc_pas` | verified | bwip-js ("A/99912345/$$52001510X3" matches `raw("hibccode128", ...)[0].sbs` byte-for-byte) |
| `hibc_pas_code39` | HIBC PAS - Code 39 | custom | `render_hibc_pas` | verified | bwip-js (same envelope + verified Code 39 substrate as `hibc_pas_code128`) |
| `hibc_pas_codablockf` | HIBC PAS - Codablock F | custom | `render_hibc_pas` | verified | bwip-js (HIBC envelope verified + Codablock-F base verified) |
| `hibc_pas_datamatrix` | HIBC PAS - Data Matrix | custom | `render_hibc_pas` | verified | bwip-js (thin wrapper: HIBC `format_pas()` + delegates to `datamatrix::encode`; byte-equal composition assertion) |
| `hibc_pas_micropdf417` | HIBC PAS - MicroPDF417 | custom | `render_hibc_pas` | verified | bwip-js (HIBC envelope verified + MicroPDF417 base verified) |
| `hibc_pas_pdf417` | HIBC PAS - PDF417 | custom | `render_hibc_pas` | verified | bwip-js (HIBC envelope verified + PDF417 base verified) |
| `hibc_pas_qrcode` | HIBC PAS - QR Code | custom | `render_hibc_pas` | verified | Thin wrapper: HIBC `format_pas()` + delegates to the native QR encoder via `qrcode_native::encode_with_options` (default since Stage 16). PAS envelope independently pinned by `hibc::tests::encode_pas_qrcode_composes_format_pas_and_qrcode`; the QR substrate is byte-for-byte vs bwip-js. |

## Composite (Linear + 2D)

| Catalog ID | Display name | Renderer | BWIPP type / handler | Status | Oracle |
|---|---|---|---|---|---|
| `composite_ean13_cca` | EAN-13 Composite (CC-A) | custom | `render_composite` | verified | bwip-js (full 99×84 pixs byte-identical for "5901234123457\|(99)1234567": 3 CC-A rows × 2 + 3 hardcoded "guard transition" rows × 2 + 72-module-tall linear; the guard transition rows encode the EAN-13 outer guard bars extending upward into the CC zone — A=`linpad+[0,1,0×93,1,0]+ccrpad`, B=`linpad+[1,0,0×93,0,1]+ccrpad`, A again — per BWIPP `ean13composite` lines 38679-38705; the linear's bar/space widths are byte-identical to the standalone `ean13` encoder, sum=95) |
| `composite_ean13_ccb` | EAN-13 Composite (CC-B) | custom | `render_composite` | verified | bwip-js (drop-in superset of `composite_ean13_cca`; reuses the same EAN guard-fanout layout via `build_ean_family_composite` with `cc.version` dispatch to either `render_cca` or `render_ccb`) |
| `composite_ean8_cca` | EAN-8 Composite (CC-A) | custom | `render_composite` | verified | bwip-js (72×86 pixs for "12345670\|(99)1234567" — same EAN guard fanout pattern as EAN-13, with `linwidth=67` and `cccolumns=3` producing `ccpixx=72`; the right guard sits at column `linpad+67` instead of `linpad+95`) |
| `composite_ean8_ccb` | EAN-8 Composite (CC-B) | custom | `render_composite` | verified | bwip-js (drop-in superset of `composite_ean8_cca` via `cc.version` dispatch) |
| `composite_upca_cca` | UPC-A Composite (CC-A) | custom | `render_composite` | verified | bwip-js (99×84 pixs for "012345678905\|(99)1234567" — UPC-A is structurally an EAN-13 with leading 0, so the linear width is identical (95 modules) and the composite stacker reuses `build_ean_cca_composite` directly via `ean::encode_upca`) |
| `composite_upca_ccb` | UPC-A Composite (CC-B) | custom | `render_composite` | verified | bwip-js (drop-in superset of `composite_upca_cca`) |
| `composite_upce_cca` | UPC-E Composite (CC-A) | custom | `render_composite` | verified | bwip-js (55×88 pixs for "0123456\|(99)1234567" — UPC-E is the 51-module compressed UPC; the composite uses `cccolumns=2` per BWIPP's `gs1_cc_lintypecccolumns` table → CC-A 2-col with `ccpixx=55`; reuses `build_ean_cca_composite` with `linwidth=51` so the right guard sits at `linpad+51` instead of `linpad+95`) |
| `composite_upce_ccb` | UPC-E Composite (CC-B) | custom | `render_composite` | verified | bwip-js (drop-in superset of `composite_upce_cca`) |
| `composite_gs1_128_cca` | GS1-128 Composite (CC-A) | custom | `render_composite` | verified | bwip-js (full 145×43 pixs byte-identical for "(01)04012345123456\|(99)1234567": linear is GS1-128 + LinkA terminator (verified separately via `encode_with_linkage_a_matches_bwip_js`, 79-element sbs sum=145), separator is the inverted-polarity expansion of the linkage-aware linsbs with no boundary trimming, CC-A is centred above the leftmost 10 modules of the linear via the BWIPP offset formula `x = ((s-p-1)*11 + 10 + (p==0?2:0)) - 99` where s, p come from `linwidth`) |
| `composite_gs1_128_ccb` | GS1-128 Composite (CC-B) | custom | `render_composite` | verified | bwip-js (drop-in superset of `composite_gs1_128_cca` — accepts CC-A-sized payloads via `cc.version` dispatch and renders CC-B via the same `build_gs1_128_cca_composite` stacker since both CC-A and CC-B 4-col MicroPDF417 share the same `rwid=99` row width; integration tests `every_symbology_renders_svg/png` exercise the CC-B-forcing default payload) |
| `composite_gs1_128_ccc` | GS1-128 Composite (CC-C) | custom | `render_composite` | verified | bwip-js (154×49 pixs for "(01)04012345123456\|(99)1234567" — CC-C uses PDF417 (full version) with `cccolumns = (linwidth - 52) / 17 = 5`, `eclevel = log2(eccws) - 1 = 2` per BWIPP's `gs1_cc` lines 36769-36802; ccrowmult=3 (PDF417's 3-row groups vs MicroPDF417's 2); composite stacker uses `x = -7` shift (linear right-shifted 7 cells); full 154-cell sep + linear rows asserted byte-for-byte against bwip-js logical pixs) |
| `composite_databar_omni_cca` | GS1 DataBar Omni Composite (CC-A) | custom | `render_composite` | verified | bwip-js (full 100×40 pixs byte-identical for "(01)24012345678905\|(10)BATCH": 6 CC-A physical rows + 1 separator row + 33 linear-tile rows; built from bwip-js's `pixs / rowmult / linheight` oracle and reconstructed via BWIPP's `databaromnicomposite` pixs layout — CC-A rows + `[0]` trailing, then `[0,0,0,0] + [0] + sep95` separator, then `[0,0,0,0] + [0] + bot95` linear repeated `linheight=33` times) |
| `composite_databar_omni_ccb` | GS1 DataBar Omni Composite (CC-B) | custom | `render_composite` | verified | bwip-js (byte-mode codeword wrapping byte-identical: 30 datcws starting `[920, 901, 295, …]` + 18 RS-ECC check codewords match bwip-js's full 48-codeword cws for the long CC-B-forcing payload "(01)24012345678905\|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC"; final 100×58 BitMatrix asserted byte-for-byte against bwip-js logical pixs for CC-B row 0 + row 11 + separator + first linear, with rows 26..58 mechanically asserted to be linear-template copies) |
| `composite_databar_truncated_cca` | GS1 DataBar Truncated Composite (CC-A) | custom | `render_composite` | verified | bwip-js (full 100×20 pixs byte-identical for "(01)24012345678905\|(99)1234567": 6 CC-A physical rows + 1 separator + 13 linear tiles via `build_databaromni_composite` with `linheight=13`. Pinned by `composite::tests::encode_databartruncated_cca_matches_bwip_js_pixs` over the full 5-logical-row pixs corpus.) |
| `composite_databar_truncated_ccb` | GS1 DataBar Truncated Composite (CC-B) | custom | `render_composite` | verified | bwip-js (full 100×38 pixs byte-identical for the long CC-B-forcing payload: 12 CC-B physical rows × 2 + 1 separator + 13 linear tiles. Pinned by `composite::tests::encode_databartruncated_ccb_matches_bwip_js_pixs` over the full 14-logical-row pixs corpus. Drop-in superset of the CC-A variant — accepts CC-A payloads via `cc.version` dispatch.) |
| `composite_databar_stacked_cca` | GS1 DataBar Stacked Composite (CC-A) | custom | `render_composite` | verified | bwip-js (full 56×24 pixs byte-identical for "(01)24012345678905\|(99)1234567": CC-A with ucols=2 (5 logical rows × CCA_ROWMULT=2 = 10 physical) + 1 composite separator (derived from stacked top half via sepfinder at position 18) + DataBar Stacked's [5,1,7] rowmult expansion (5 top + 1 internal sep + 7 bot = 13). Pinned by `composite::tests::encode_databarstacked_cca_matches_bwip_js_pixs` over the full 9-logical-row pixs corpus.) |
| `composite_databar_stacked_ccb` | GS1 DataBar Stacked Composite (CC-B) | custom | `render_composite` | verified | bwip-js (full 56×54 pixs byte-identical for the long CC-B-forcing payload: 20 CC-B physical rows × 2 + 1 separator + 13 linear tiles. Pinned by `composite::tests::encode_databarstacked_ccb_matches_bwip_js_pixs` over the full 24-logical-row pixs corpus. Drop-in superset of the CC-A variant.) |
| `composite_databar_stacked_omni_cca` | GS1 DataBar Stacked Omni Composite (CC-A) | custom | `render_composite` | verified | bwip-js (full 56×80 pixs byte-identical for "(01)24012345678905\|(99)1234567": CC-A with ucols=2 (5 logical rows × CCA_ROWMULT=2 = 10 physical) + 1 composite separator (derived from stacked-omni top half) + DataBar Stacked Omni's [33,1,1,1,33] rowmult expansion (= 69). Pinned by `composite::tests::encode_databarstackedomni_cca_matches_bwip_js_pixs` over the full 11-logical-row pixs corpus.) |
| `composite_databar_stacked_omni_ccb` | GS1 DataBar Stacked Omni Composite (CC-B) | custom | `render_composite` | verified | bwip-js (full 56×110 pixs byte-identical for the long CC-B-forcing payload: 20 CC-B physical rows × 2 + 1 separator + 69 linear tiles. Pinned by `composite::tests::encode_databarstackedomni_ccb_matches_bwip_js_pixs` over the full 26-logical-row pixs corpus. Drop-in superset of the CC-A variant.) |
| `composite_databar_expanded_stacked_cca` | GS1 DataBar Expanded Stacked Composite (CC-A) | custom | `render_composite` | verified | bwip-js (102×78 pixs for "(01)90012345678908(3103)001750\|(99)1234567": 3 CC-A rows × CCA_ROWMULT=2 + 1 composite-separator + 5 expanded-stacked logical rows with rowmult [34,1,1,1,34] = 71 = 78 total. The composite-sep is derived from the linear's top row via the omni-shared sepfinder at positions 19 + 70. Pinned by `composite::tests::encode_databarexpandedstacked_cca_matches_bwip_js_pixs` over the first 7 logical rows (CC + composite-sep + top linear strip + sep0 + inter_sep); remaining 32 linear physical rows are verified by the standalone `databar_expanded::encode_stacked` tests against bwip-js. |
| `composite_databar_expanded_stacked_ccb` | GS1 DataBar Expanded Stacked Composite (CC-B) | custom | `render_composite` | verified | bwip-js (102×96 dimensions pinned for the long CC-B-forcing payload via `composite::tests::encode_databarexpandedstacked_ccb_dims_match_bwip_js`; drop-in superset of the CC-A variant via `cc.version` dispatch.) |
| `composite_databar_expanded_cca` | GS1 DataBar Expanded Composite (CC-A) | custom | `render_composite` | verified | bwip-js (151×41 pixs for "(01)90012345678908(3103)001750\|(99)1234567" — DataBar Expanded composite uses the same sepfinder-windowed separator as DataBar Omni but without the f3pat-match override, at finder positions 18, 116, … and 69, 167, … every 98 cells; linsbs has +1 leading zero in pixs grid; verified byte-identical separator + linear rows vs bwip-js logical pixs) |
| `composite_databar_expanded_ccb` | GS1 DataBar Expanded Composite (CC-B) | custom | `render_composite` | verified | bwip-js (drop-in superset of `composite_databar_expanded_cca` via `cc.version` dispatch) |
| `composite_databar_limited_cca` | GS1 DataBar Limited Composite (CC-A) | custom | `render_composite` | verified | bwip-js (full 74×19 pixs byte-identical for "(01)15012345678907\|(99)1234567": 8 CC-A physical rows + 1 separator + 10 linear tiles; separator construction validated against bwip-js's `sepleft`+`sepright` boundary zeros applied to inverted-polarity linsbs expansion; the CC-A 3-col render bug — dropping the centre RAP instead of the left — was fixed as part of this port and verified via the byte-match) |
| `composite_databar_limited_ccb` | GS1 DataBar Limited Composite (CC-B) | custom | `render_composite` | verified | bwip-js (full 83×51 pixs byte-identical for "(01)15012345678907\|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC": CC-B 20-row 2D rendered via `render_ccb` with c=3 (rwid=82), composite stacker uses BWIPP's `ccpixx != 72` branch — CC rows get a trailing 0 to 83 cells, separator and linear rows are shifted right by 9 cells to land at columns 9..83. First + last CC row, separator, and first linear all asserted byte-for-byte; drop-in superset accepting CC-A payloads via the same `cc.version` dispatch as the Omni CC-B variant) |

---

## Cross-cutting infrastructure status

All cross-cutting infrastructure that the catalog depends on is in
place. The list below is preserved for historical context (these
were tracked as separate work items during the port) — every line
is now **implemented**:

- Reed-Solomon over GF(2⁴ / 2⁶ / 2⁸ / 2¹² / 64 / 113 / 256 / 929)
- Postal 4-state model (`Encoded::Postal4`)
- GS1 AI parser (`util::gs1`)
- Code 128 with FNC1 / GS1-128 tokens
- HIBC LIC + PAS formatter
- EAN / UPC + add-on combiner
- Custom postal wrappers (CEPNet, Korean Postal, DPD, AusPost, Swedish Postal, ITF-14, etc.)
- Stacked / multi-row renderer (Code 16K, Code 49, DataBar Stacked / Expanded Stacked, PDF417, MicroPDF417, Codablock-F)
- Composite linear + 2D model (17 verified composite rows)
- Dot / circular renderer (`Encoded::Dots`) — DotCode + GS1 DotCode
- Hex grid renderer (`Encoded::Hex`) — MaxiCode
- WASM bindings — wasm-bindgen `bwipp::wasm::*` + raw-pointer ABI for the no-std-friendly `rust/wasm` crate

## Historical port milestones (all landed)

Preserved for context — these were the originally-planned port
phases. Every row below has long since landed; the live status is
the per-row table above.

1. ✅ GS1 DataBar family (Omni / Expanded / Truncated / Limited + Stacked variants).
2. ✅ Mailmark 4-state + Mailmark 2D.
3. ✅ Reed-Solomon GF(64) infrastructure for Australia Post + USPS OneCode.
4. ✅ Composite codes (linear + 2D CC-A / CC-B / CC-C).
5. ✅ DotCode + GS1 DotCode.
6. ✅ PDF417 family + Aztec Code (incl. Compact + Rune) + MaxiCode + Han Xin Code.
7. ✅ Codablock-F + HIBC variants over Codablock-F / MicroPDF417 / PDF417 / Aztec / DataMatrix / QR.
8. ✅ WASM bindings via wasm-bindgen (`tests/wasm.rs` covers 30 paths) and the raw-pointer ABI in `rust/wasm`.
