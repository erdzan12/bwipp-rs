//! The `Symbology` enum is the entry point: each variant identifies one
//! barcode type and knows how to encode its data.

use crate::encoding::Encoded;
use crate::error::Error;
use crate::options::Options;

mod auspost;
mod aztec;
mod bc412;
mod book_codes;
mod channelcode;
mod codabar;
mod codablockf;
mod code11;
mod code128;
mod code16k;
mod code32;
mod code39;
mod code39_wrappers;
mod code39ext;
mod code49;
mod code49_patterns;
mod code93;
mod code93ext;
mod codeone;
mod composite;
mod databar;
mod databar_expanded;
mod datamatrix_;
mod dotcode;
mod ean;
mod ean_addons;
mod ean_combined;
mod flattermarken;
mod gs1_128;
mod gs1_2d;
mod gs1_cc;
pub(crate) mod gs1_dotcode;
mod hanxin;
mod hibc;
mod identleitcode;
mod interleaved2of5;
mod japan_post;
mod mailmark;
pub mod maxicode;
mod micropdf417;
mod msi;
mod pdf417;
mod pharmacode;
mod plessey;
pub mod posicode;
mod postal4;
mod postal_misc;
mod postnet;
mod qrcode_;
mod qrcode_native;
mod swiss_qr;
mod telepen;
mod twoofive;
pub mod ultracode;
mod usps_impb;
mod usps_onecode;

/// Every barcode type this crate can encode.
///
/// This list grows as we port more BWIPP symbologies. The current set is the
/// foundation — Code 39, Code 128, the EAN/UPC family, ITF, Codabar, plus QR
/// and Data Matrix for 2D coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Symbology {
    /// Code 39 (3 of 9) - alphanumeric, no check digit by default.
    Code39,
    /// Code 39 Full ASCII (extended) - encodes all of ASCII as Code 39 pairs.
    Code39Ext,
    /// Code 93 - higher density than Code 39.
    Code93,
    /// Code 93 Full ASCII (extended) - encodes all of ASCII via the
    /// same `$ % / +` shift-pair scheme as Code 39 Full ASCII, except
    /// `$ % / +` themselves stay in Code 93's base alphabet.
    Code93Ext,
    /// Code 128 - full ASCII, auto-subset.
    Code128,
    /// Code 11 - numeric only, with optional check digit.
    Code11,
    /// IBM BC412 - 35-char alphabet (digits + uppercase except O),
    /// originally used on semiconductor wafers.
    Bc412,
    /// Code 32 (Italian Pharmacode) - 8 digits, derived from Code 39.
    Code32,
    /// Code 2 of 5 (Standard).
    Code2of5,
    /// Data Logic 2 of 5.
    DataLogic2of5,
    /// IATA 2 of 5.
    Iata2of5,
    /// Industrial 2 of 5.
    Industrial2of5,
    /// COOP 2 of 5.
    Coop2of5,
    /// Matrix 2 of 5.
    Matrix2of5,
    /// MSI (a.k.a. MSI Plessey).
    Msi,
    /// Plessey (the original) - hex digits 0-9, A-F.
    Plessey,
    /// POSICODE — linear bar-code symbology shipped by BWIPP with four
    /// versions (`a`, `b`, `limiteda`, `limitedb`). All four versions
    /// are byte-for-byte verified against bwip-js: the single-set
    /// `limiteda` / `limitedb` variants and the multi-set `a` / `b`
    /// variants. The `a` / `b` path uses the full BWIPP auto-encoder
    /// (set-0/1/2 lookup, LA0/LA1 latches, SF0/SF1/SF2 shifts, and
    /// FN4-based ASCII ↔ extended-ASCII transitions). Default is
    /// `"a"` when `opts.extras["version"]` is unset, matching BWIPP.
    /// See `src/symbology/posicode.rs` and `PORT_STATUS.md`.
    Posicode,
    /// Telepen (full ASCII).
    Telepen,
    /// Telepen Numeric (digit-pair packed Telepen).
    TelepenNumeric,
    /// Pharmacode One-Track (positive integer 3..=131070).
    Pharmacode,
    /// Pharmacode Two-Track (positive integer 4..=64570).
    Pharmacode2,
    /// Flattermarken (printer's mark for folded brochures).
    Flattermarken,
    /// VIN (Vehicle Identification Number) - 17-char Code 39 derivative.
    Vin,
    /// LOGMARS - US DoD Code 39 with mandatory mod-43 check digit.
    Logmars,
    /// PZN7 (Pharmazentralnummer, 7-digit form).
    Pzn7,
    /// PZN8 (Pharmazentralnummer, 8-digit form).
    Pzn8,
    /// EAN-13 - 13 digits (12 data + 1 check).
    Ean13,
    /// EAN-8 - 8 digits (7 data + 1 check).
    Ean8,
    /// Marks & Spencer 7-digit code. BWIPP `mands`. Encoded as EAN-8
    /// with a leading-zero pad; rendering is otherwise identical to
    /// the verified `ean8` primary (M&S's cosmetic bar-tail height
    /// adjustment is not preserved — see `ean::encode_mands` doc).
    MarksAndSpencer,
    /// UPC-A - 12 digits (11 data + 1 check).
    UpcA,
    /// UPC-E - 8 digits, compressed UPC-A.
    UpcE,
    /// EAN-2 add-on (2-digit supplemental barcode).
    Ean2,
    /// EAN-5 add-on (5-digit supplemental barcode).
    Ean5,
    /// ISBN - 10 or 13 digit International Standard Book Number.
    Isbn,
    /// ISMN - International Standard Music Number.
    Ismn,
    /// ISSN - International Standard Serial Number.
    Issn,
    /// DAFT (literal D/A/F/T characters → bars).
    Daft,
    /// KIX (Dutch postal 4-state code).
    Kix,
    /// Royal Mail RM4SCC (UK postal 4-state code).
    RoyalMail,
    /// USPS PostNet (5-bar linear postal code).
    Postnet,
    /// USPS PLANET (5-bar linear postal code).
    Planet,
    /// Deutsche Post Identcode (Interleaved 2 of 5 + DP mod-10 check).
    Identcode,
    /// Deutsche Post Leitcode (Interleaved 2 of 5 + DP mod-10 check).
    Leitcode,
    /// GS1-128 (formerly UCC/EAN-128). Code 128 with GS1 FNC1 markers.
    Gs1_128,
    /// SSCC-18 (Serial Shipping Container Code) rendered as GS1-128 + AI (00).
    Sscc18,
    /// EAN-14 / GTIN-14 rendered as GS1-128 + AI (01). BWIPP `ean14` alias.
    Ean14,
    /// UPC Coupon (GS1 North American Coupon, AI 8110).
    UpcCoupon,
    /// GS1 DataMatrix.
    Gs1DataMatrix,
    /// GS1 DataMatrix forced into rectangular layout. BWIPP
    /// `gs1datamatrixrectangular`.
    Gs1DataMatrixRectangular,
    /// GS1 Digital Link Data Matrix. BWIPP `gs1dldatamatrix`. Input
    /// is a GS1 DL URI; encoded as plain Data Matrix after URI
    /// shape-validation.
    Gs1DlDataMatrix,
    /// GS1 Digital Link QR Code. BWIPP `gs1dlqrcode`. Input is a GS1
    /// DL URI; encoded as plain QR Code after URI shape-validation.
    /// Inherits the qrcode-substrate compatibility exception.
    Gs1DlQrCode,
    /// GS1 QR Code. Emits the formal "FNC1 in first position" mode
    /// indicator (`0101` per ISO/IEC 18004 Annex L) plus auto-segmented
    /// payload over the qrcode-crate substrate. Compatibility exception
    /// (mask tie-break — see `COMPATIBILITY_EXCEPTIONS.md` §1a).
    Gs1QrCode,
    /// NTIN (National Trade Item Number) as GS1 DataMatrix with AI (8003).
    Ntin,
    /// PPN (Pharmacy Product Number) as Data Matrix in MH10 envelope.
    Ppn,
    /// EAN-13 with 2-digit add-on supplemental barcode.
    Ean13P2,
    /// EAN-13 with 5-digit add-on supplemental barcode.
    Ean13P5,
    /// EAN-8 with 2-digit add-on supplemental barcode.
    Ean8P2,
    /// EAN-8 with 5-digit add-on supplemental barcode.
    Ean8P5,
    /// UPC-A with 2-digit add-on supplemental barcode.
    UpcAP2,
    /// UPC-A with 5-digit add-on supplemental barcode.
    UpcAP5,
    /// UPC-E with 2-digit add-on supplemental barcode.
    UpcEP2,
    /// UPC-E with 5-digit add-on supplemental barcode.
    UpcEP5,
    /// ISBN-13 with 5-digit add-on.
    IsbnP5,
    /// ISSN with 2-digit add-on.
    IssnP2,
    /// HIBC LIC rendered as Code 128.
    HibcCode128,
    /// HIBC LIC rendered as Code 39.
    HibcCode39,
    /// HIBC LIC rendered as Data Matrix.
    HibcDataMatrix,
    /// HIBC LIC rendered as QR Code.
    HibcQrCode,
    /// HIBC LIC rendered as PDF417.
    HibcPdf417,
    /// HIBC LIC rendered as MicroPDF417.
    HibcMicroPdf417,
    /// HIBC LIC rendered as Codablock-F.
    HibcCodablockF,
    /// HIBC LIC rendered as Aztec Code. BWIPP `hibcazteccode`.
    HibcAztecCode,
    /// HIBC LIC rendered as Data Matrix (rectangular layout).
    /// BWIPP `hibcdatamatrixrectangular`.
    HibcDataMatrixRectangular,
    /// HIBC PAS rendered as Code 128.
    HibcPasCode128,
    /// HIBC PAS rendered as Code 39.
    HibcPasCode39,
    /// HIBC PAS rendered as Data Matrix.
    HibcPasDataMatrix,
    /// HIBC PAS rendered as QR Code.
    HibcPasQrCode,
    /// HIBC PAS rendered as PDF417.
    HibcPasPdf417,
    /// HIBC PAS rendered as MicroPDF417.
    HibcPasMicroPdf417,
    /// HIBC PAS rendered as Codablock-F.
    HibcPasCodablockF,
    /// UPU S10 international tracking number rendered as Code 128.
    UpuS10,
    /// Korean Postal Authority (6-digit + mod-10 check) rendered as Code 128.
    KoreanPostal,
    /// Brazilian CEPNet (8-digit ZIP) rendered as Code 128.
    Cepnet,
    /// Italian Postal 2 of 5 (Interleaved 2 of 5 variant).
    ItalianPostal25,
    /// Italian Postal 3 of 9 (Code 39 with mandatory check).
    ItalianPostal39,
    /// DPD parcel code rendered as Code 128.
    Dpd,
    /// Deutsche Post Postmatrix rendered as Data Matrix.
    DpPostmatrix,
    /// Royal Mail Mailmark — BWIPP's `type`-keyed Data Matrix variant.
    /// Reads `opts.extras["type"]` (7 / 9 / 29) to select size, requires
    /// `JGB ` prefix on the payload.
    Mailmark,
    /// Royal Mail Mailmark 2D (Data Matrix carrying 45/70/90-char Mailmark payload).
    Mailmark2d,
    /// Swiss QR Code (QR-bill SPC payload + ECL=M).
    SwissQrCode,
    /// USPS Intelligent Mail Package Barcode (IMpb) - GS1-128 derivative.
    UspsImpb,
    /// USPS OneCode / Intelligent Mail Barcode (IMb) - 65-bar 4-state letter mail.
    UspsOneCode,
    /// Japan Post 4-state (mod-19 check; digits + dash; letters via 2-slot expansion).
    JapanPost,
    /// Australia Post 4-state Customer (FCC 11).
    AuspostCustomer,
    /// Australia Post 4-state Reply Paid (FCC 45).
    AuspostReplyPaid,
    /// Australia Post 4-state Routing (FCC 59) - 5-char (character mode)
    /// or 8-digit (numeric mode) customer-info suffix.
    AuspostRouting,
    /// Australia Post 4-state Redirection (FCC 62) - 10-char (character
    /// mode) or 15-digit (numeric mode) customer-info suffix.
    AuspostRedirection,
    /// GS1 DataBar Omnidirectional. Verified byte-for-byte against
    /// bwip-js for widths + checksum + 45-element sbs.
    DatabarOmni,
    /// GS1 DataBar Truncated. Same sbs as DataBar Omni rendered
    /// shorter; verified byte-for-byte against bwip-js.
    DatabarTruncated,
    /// GS1 DataBar Limited. Verified byte-for-byte against bwip-js for
    /// widths + check + 46-element sbs across 3 input families.
    DatabarLimited,
    /// GS1 DataBar Stacked - same payload as DataBar Omni but split
    /// across two rows separated by a 1-module-tall separator.
    DatabarStacked,
    /// GS1 DataBar Stacked Omnidirectional - taller stacked variant
    /// with three intermediate separator rows for scanning robustness.
    DatabarStackedOmni,
    /// GS1 DataBar Expanded - variable-length GS1 AI string in a 1D
    /// barcode. All seven BWIPP method dispatchers (1, 0100, 0101,
    /// 0111xxx, 01100, 01101, and the general-purpose 00 fallback)
    /// are ported and byte-verified against bwip-js. Supports the
    /// linkage variant used by `composite_databar_expanded_*`.
    DatabarExpanded,
    /// GS1 DataBar Expanded Stacked - same payload as DataBar
    /// Expanded but split into 4-character-per-row stacked layout
    /// with row + inter-row separators.
    DatabarExpandedStacked,
    /// Codabar (NW-7) - numeric + 6 symbols, start/stop characters.
    Codabar,
    /// Interleaved 2 of 5 - numeric, even number of digits.
    Interleaved2of5,
    /// ITF-14 - 14-digit Interleaved 2 of 5 used for shipping containers.
    Itf14,
    /// QR Code.
    QrCode,
    /// Micro QR Code (M1..M4 variants; 11×11 to 17×17 modules).
    MicroQrCode,
    /// Rectangular Micro QR Code (rMQR), per ISO/IEC 23941:2022. 32
    /// size variants (R7×43 .. R17×139) with EC levels M or H only.
    /// Native byte-for-byte encoder (internal `qrcode_native` module)
    /// verified against bwip-js on a 16-row corpus by the
    /// `encode_rmqr_pixs_corpus_matches_oracle` unit test.
    RectangularMicroQrCode,
    /// Data Matrix (ECC 200).
    DataMatrix,
    /// Data Matrix with rectangular layout forced (no square fallback).
    DataMatrixRectangular,
    /// Data Matrix Rectangular Extension (DMRE). Adds the 17 ISO/IEC
    /// 21471 additional rectangular sizes (8×48..26×64). BWIPP
    /// `datamatrixrectangularextension`.
    DataMatrixRectangularExtension,
    /// Codablock-F (stacked Code 128).
    CodablockF,
    /// PDF417 (stacked 2D, variable-length text/byte/numeric payload).
    Pdf417,
    /// PDF417 Truncated - PDF417 minus the right row-indicator column
    /// and stop pattern. Roughly 34% narrower than full PDF417 at the
    /// cost of some scanner tolerance.
    Pdf417Truncated,
    /// MicroPDF417 (compact stacked 2D, fixed-size variants only — no
    /// length-prefix codeword; symbol shape carried by RAP bars).
    MicroPdf417,
    /// DotCode — 2D dot-matrix barcode for high-speed inkjet
    /// printing. Each module is a circular dot on a diagonal grid,
    /// rendered via [`crate::encoding::Encoded::Dots`] using true SVG
    /// `<circle>` elements / PNG-rasterised discs. Mask selection
    /// mirrors BWIPP's full `evalsymbol` worst-edge / clear-row /
    /// clear-column / outlier scoring, with the lit-mask fallback
    /// when the first-pass best score is `≤ rows × columns / 2`.
    DotCode,
    /// Code 16K — stacked 1D barcode (2..=16 rows × 70 modules per
    /// row), AIM/ANSI MH10.8.3. BWIPP-faithful encoder + stacked
    /// renderer (internal `code16k` module). Verified by a
    /// byte-for-byte `pixs` golden against bwip-js.
    Code16k,
    /// Code 49 — stacked 1D barcode (2..=8 rows × 81 modules per
    /// row), AIM USS Code 49. BWIPP-faithful encoder + WEIGHTX/Y/Z
    /// row-check formula + PATTERNS_0/1 stacked renderer (internal
    /// `code49` module). Verified by a byte-for-byte `pixs` golden
    /// against bwip-js for `"12345"` plus a 6-input `build_ccs`
    /// golden covering each cws-encoder path.
    Code49,
    /// Code One — matrix 2D barcode (AIM USS Code One). BWIPP-faithful
    /// encoder with Mode A (ASCII) + Mode CTX (C40 / Text / X12 packing) +
    /// GF(256) Reed-Solomon (primitive 301) + symbol-size picker +
    /// matrix placement with column-pattern artifacts + reference
    /// islands + forced black dots. Currently covers Version A
    /// (16 × 18 modules). Verified by byte-for-byte `pixs` goldens
    /// against bwip-js for `"A"`, `"Hello"`, `"ABC"`, `"ABCDEFG"` plus
    /// 5 ECC and 11 lookup-decision goldens. Mode D (decimal
    /// compression) + Mode B (byte) + S/T-strip families + larger
    /// matrix versions still deferred — those inputs return
    /// `InvalidData`.
    CodeOne,
    /// `gs1dotcode` — DotCode that carries GS1 Application Identifier
    /// data. Input is the parenthesised `(NN)data(MM)data…` element
    /// string. The wrapper parses + validates each AI through the
    /// internal `util::gs1` parser, flattens with FNC1 separators per
    /// the GS1 spec, lifts to `&[i16]` (every FNC1 byte becomes the
    /// negative `FN1` marker), and drives `dotcode::encode_with_markers`.
    Gs1DotCode,
    /// MaxiCode — UPS-developed fixed-size hexagonal 2D barcode
    /// (33×30 hex grid). All five user-facing modes are wired:
    ///
    /// - Mode 2: structured carrier message with numeric (US ZIP)
    ///   postcode. Use `opts.extras["mode"] = "2"` and the GS-separated
    ///   input `<postcode>\x1d<country>\x1d<service>\x1d<secondary>`
    ///   (optionally with a `[)>\x1e01\x1d<dd>` FID prefix).
    /// - Mode 3: same as mode 2 but with an alphanumeric postcode.
    ///   Use `mode = "3"`.
    /// - Mode 4 (default): general data, 84-byte secondary + 40 ECC.
    /// - Mode 5: general data, enhanced ECC. 68-byte secondary + 56 ECC.
    ///   Use `mode = "5"` for high-noise environments.
    /// - Mode 6: reader-programming data. Same layout as mode 4 but
    ///   the leading codeword signals "config payload". Use `mode = "6"`.
    ///
    /// Full set-A/B/C/D/E shift + latch + intra-latch encoder.
    /// Single-byte SC/SD/SE shifts for runs of 1-2 same-set bytes;
    /// `[shift, shift]` latch + body + back-latch for 3+; intra-
    /// latch SC/SD/SE shifts for cross-set bytes inside an
    /// established latch; set-E EOM back-latch omission per BWIPP.
    /// All paths byte-for-byte verified against bwip-js.
    Maxicode,
    /// Ultracode (AIM USS Ultracode) — colour 2D matrix barcode.
    /// The only colour 2D symbology in the BWIPP catalog; uses a
    /// 6-colour palette (white, cyan, magenta, yellow, green, black)
    /// and Reed-Solomon ECC over GF(283). Renders to an
    /// [`Encoded::ColorMatrix`] which the SVG and PNG renderers paint
    /// per-cell from the symbology's [`ULTRACODE_PALETTE`].
    ///
    /// Default options (`eclevel="EC2"`, `rev=2`) match BWIPP's
    /// defaults; the encoder is byte-for-byte verified against
    /// bwip-js across an 8-input pixs corpus
    /// (`encode_pixs_default_matches_corpus` in
    /// `rust/src/symbology/ultracode.rs`).
    ///
    /// Opt-in BWIPP knobs (`parsefnc`, `eclevel != EC2`, `rev=1`,
    /// `raw=true`, `link1`) are not exposed by the default encoder
    /// path — they require additional oracle corpus before promotion.
    ///
    /// [`ULTRACODE_PALETTE`]: ultracode::ULTRACODE_PALETTE
    Ultracode,
    /// Aztec Code — concentric-bull's-eye 2D matrix symbology.
    /// Supports compact L1-L4 and full L1-L32 layouts, including
    /// reference-grid insertion for full L≥5. Output is byte-
    /// identical to bwip-js across a 27-input oracle corpus covering
    /// ASCII, UTF-8 multibyte via Byte mode, and pair pre-compression
    /// (CR/LF, ". ", ", ", ": "). FNC1 / ECI / Structured Append
    /// markers are out of scope.
    AztecCode,
    /// Aztec Code forced into Compact mode (L1-L4 only). BWIPP
    /// `azteccodecompact`. Rejects payloads that would otherwise
    /// escalate to a full-size symbol; the same encoder otherwise.
    AztecCodeCompact,
    /// Aztec Rune — fixed 11×11 marker carrying a single 0..=255
    /// integer. BWIPP `aztecrune`. Input is a 1- to 3-digit ASCII
    /// decimal string.
    AztecRune,
    /// Channel Code (USPS Tray Labels). BWIPP `channelcode`. Linear,
    /// 3-8 channels (input is 2-7 ASCII digits). Output is a
    /// 9-module finder + per-channel space/bar width pairs.
    ChannelCode,
    /// GS1 Composite: DataBar Omni (linear primary) stacked with a
    /// MicroPDF417-CC-A 2D companion. Input is the pipe-separated
    /// `LINEAR|COMP` form — e.g. `(01)24012345678905|(99)1234567`.
    /// Verified byte-identical to bwip-js for the standard 100×40
    /// `cc=4` layout (3-row CC-A above a 33-module-high linear).
    CompositeDatabarOmniCca,
    /// GS1 Composite: DataBar Omni stacked with a MicroPDF417-CC-B 2D
    /// companion (56-1184 bits of supplementary AI data, vs CC-A's
    /// 56-208 bits). The handler is a drop-in superset of the CC-A
    /// variant — accepts both payload sizes, picking CC-A or CC-B
    /// automatically. CC-B byte-mode codeword wrapping and RS-ECC
    /// verified byte-identical to bwip-js (`pack_ccb_datcws` +
    /// `ccb_cws_compose` tests).
    CompositeDatabarOmniCcb,
    /// GS1 Composite: DataBar Truncated stacked with a MicroPDF417-CC-A
    /// 2D companion. Truncated shares the Omni 95-module sbs but renders
    /// the linear zone at 13 modules tall instead of 33. Verified
    /// byte-identical to bwip-js (100×20 pixs).
    CompositeDatabarTruncatedCca,
    /// GS1 Composite: DataBar Truncated stacked with a MicroPDF417-CC-B
    /// 2D companion. Drop-in superset of `CompositeDatabarTruncatedCca` —
    /// accepts both CC-A and CC-B payload sizes; the linear linkage flag
    /// matches Omni's.
    CompositeDatabarTruncatedCcb,
    /// GS1 Composite: DataBar Stacked stacked with a MicroPDF417-CC-A 2D
    /// companion. Stacked is the 50×13 two-row DataBar variant; the
    /// composite uses CC-A with ucols=2 (~55-cell width) sitting above
    /// the stacked linear. Verified byte-identical to bwip-js (56×24
    /// pixs).
    CompositeDatabarStackedCca,
    /// GS1 Composite: DataBar Stacked stacked with a MicroPDF417-CC-B
    /// 2D companion. Drop-in superset of `CompositeDatabarStackedCca`.
    CompositeDatabarStackedCcb,
    /// GS1 Composite: DataBar Stacked Omnidirectional stacked with a
    /// MicroPDF417-CC-A 2D companion. Stacked-Omni is the 50×69
    /// stacked variant with three internal separator rows
    /// (rowmult `[33, 1, 1, 1, 33]`). Composite uses CC-A with ucols=2.
    /// Verified byte-identical to bwip-js (56×80 pixs).
    CompositeDatabarStackedOmniCca,
    /// GS1 Composite: DataBar Stacked Omnidirectional + CC-B. Drop-in
    /// superset of `CompositeDatabarStackedOmniCca`.
    CompositeDatabarStackedOmniCcb,
    /// GS1 Composite: DataBar Expanded Stacked + CC-A. Uses CC-A
    /// with ucols=4 centered horizontally above the 102-wide
    /// expanded-stacked linear. Verified byte-identical to bwip-js
    /// (102×78 pixs for the canonical input).
    CompositeDatabarExpandedStackedCca,
    /// GS1 Composite: DataBar Expanded Stacked + CC-B. Drop-in
    /// superset of `CompositeDatabarExpandedStackedCca`.
    CompositeDatabarExpandedStackedCcb,
    /// GS1 Composite: DataBar Limited (linear primary) stacked with a
    /// MicroPDF417-CC-A 2D companion. Uses CC-A 3-column layout
    /// (`ccpixx=72`), a 74-cell separator with `sepleft` / `sepright`
    /// boundary zeros (no `sepfinder` logic — Limited doesn't need it),
    /// and a 10-module-tall linear. Verified byte-identical to bwip-js
    /// for the catalog example `(01)15012345678907|(99)1234567`.
    CompositeDatabarLimitedCca,
    /// GS1 Composite: DataBar Limited stacked with a MicroPDF417-CC-B 2D
    /// companion. CC-B uses the non-CCA c=3 metric (`rwid=82`), which
    /// produces a wider 2-D than the linear (74). The composite layout
    /// switches to BWIPP's `ccpixx != 72` branch: each CC row gets a
    /// trailing zero (83 cells), the separator and linear rows are
    /// shifted right by 9 cells (`[0]*9 + sep_74` / `[0]*9 + linpixs_74`).
    /// Drop-in superset of the CC-A variant — also accepts CC-A payloads.
    CompositeDatabarLimitedCcb,
    /// GS1 Composite: GS1-128 (linear primary, variable width) stacked
    /// with a MicroPDF417-CC-A 2D companion. The linear is a normal
    /// GS1-128 plus a terminal `^LNKA` codeword. The 2D is centred
    /// above the leftmost ~10 modules of the linear via an offset `x`
    /// computed from `linwidth`. Verified byte-identical to bwip-js for
    /// the catalog example `(01)04012345123456|(99)1234567`.
    CompositeGs1_128Cca,
    /// GS1 Composite: GS1-128 stacked with a MicroPDF417-CC-B 2D
    /// companion. Drop-in superset of the CC-A variant — accepts
    /// CC-A-sized payloads via `cc.version` dispatch.
    CompositeGs1_128Ccb,
    /// GS1 Composite: EAN-13 (linear primary, 95 modules) stacked with
    /// a MicroPDF417-CC-A 2D companion. Layout includes 3 hardcoded
    /// "guard transition" rows between the CC and the main linear,
    /// representing the outer guard bars extending upward (per BWIPP
    /// `ean13composite`). Verified byte-identical to bwip-js for the
    /// catalog example `5901234123457|(99)1234567`.
    CompositeEan13Cca,
    /// GS1 Composite: EAN-13 + MicroPDF417-CC-B companion. Drop-in
    /// superset of the CC-A variant.
    CompositeEan13Ccb,
    /// GS1 Composite: UPC-A + MicroPDF417-CC-A. Structurally identical
    /// to EAN-13 composite (UPC-A = EAN-13 with leading 0).
    CompositeUpcaCca,
    /// GS1 Composite: UPC-A + MicroPDF417-CC-B.
    CompositeUpcaCcb,
    /// GS1 Composite: EAN-8 (linear primary, 67 modules) + MicroPDF417-CC-A.
    /// Uses `cccolumns=3` (CC-A 3-col, ccpixx=72). Same guard-transition
    /// pattern as EAN-13, with the right guard at column `linpad + 67`
    /// instead of `linpad + 95`.
    CompositeEan8Cca,
    /// GS1 Composite: EAN-8 + MicroPDF417-CC-B (`cccolumns=3`).
    CompositeEan8Ccb,
    /// GS1 Composite: UPC-E (compressed 51-module linear) + MicroPDF417-CC-A.
    /// Uses `cccolumns=2` → CC-A 2-col with `ccpixx=55`. Same EAN-family
    /// guard fanout, with the right guard at column `linpad + 51`.
    CompositeUpceCca,
    /// GS1 Composite: UPC-E + MicroPDF417-CC-B (`cccolumns=2`).
    CompositeUpceCcb,
    /// GS1 Composite: DataBar Expanded (variable-width linear) +
    /// MicroPDF417-CC-A. Uses the linkage variant of the
    /// expanded encoder (`binval\[0\] = 1`) and a sepfinder-based
    /// separator at finder positions 18, 116, … and 69, 167, … .
    /// Linear sbs has `+1` leading zero in the pixs grid.
    CompositeDatabarExpandedCca,
    /// GS1 Composite: DataBar Expanded + MicroPDF417-CC-B.
    CompositeDatabarExpandedCcb,
    /// GS1 Composite: GS1-128 + PDF417-CC-C 2D companion (the
    /// "full PDF417" composite — only valid with GS1-128 since
    /// other linears are too narrow). Uses `linkagec` (vs `linkagea`
    /// for CC-A/CC-B), `x = -7` (linear shifted right by 7 cells),
    /// and `PDF417_ROWMULT = 3` (vs MicroPDF417's 2). The CC-C
    /// `cccolumns` / `eclevel` are derived from the linear's
    /// `linwidth` per BWIPP's `(linwidth - 52) / 17` formula.
    CompositeGs1_128Ccc,
    /// Han Xin Code — Chinese 2D barcode (GB/T 21049-2007, ISO/IEC
    /// 20830:2021). 84 size versions from 23×23 (v1) to 189×189 (v84),
    /// 4 ECC levels (L1..L4), 4 mask patterns. Supports binary/byte
    /// mode only (the same scope as bwip-js). Numeric mode, text mode,
    /// and the GB18030 Region One/Two modes from the standard aren't
    /// yet ported. ECC level defaults to L1; when `mask` is omitted
    /// the encoder picks the mask whose `evalfull` score is lowest
    /// (BWIPP's standard auto-selection).
    ///
    /// Options:
    /// - `opts.extras["eclevel"]` — `"L1"`..`"L4"` (default: `"L1"`).
    /// - `opts.extras["mask"]` — `"0"`..`"3"` (BWIPP's internal mask
    ///   index; defaults to auto-pick via `evalfull` scoring).
    HanXinCode,
}

impl Symbology {
    /// Encode a payload into [`Encoded`] form. Most callers want
    /// [`crate::render_svg`] or [`crate::render_png`] instead, which
    /// wrap `encode` and hand the result to the appropriate renderer.
    /// Use `encode` directly when you need the intermediate bar /
    /// pixel grid (e.g. for a custom renderer or for inspection).
    ///
    /// # Errors
    /// Returns [`Error::InvalidData`] if the payload doesn't match the
    /// symbology's input format (wrong length, illegal characters, bad
    /// check digit), or [`Error::InvalidOption`] if `opts.extras`
    /// carries a key/value the encoder doesn't accept.
    ///
    /// # Example
    /// ```
    /// use bwipp::{Symbology, Options, Encoded};
    /// let encoded = Symbology::Code39.encode("HELLO", &Options::default()).unwrap();
    /// match encoded {
    ///     Encoded::Linear(p) => assert!(!p.bars.is_empty()),
    ///     _ => panic!("Code 39 should produce a Linear pattern"),
    /// }
    /// ```
    pub fn encode(self, data: &str, opts: &Options) -> Result<Encoded, Error> {
        match self {
            Symbology::Code39 => code39::encode(data, opts).map(Encoded::Linear),
            Symbology::Code39Ext => code39ext::encode(data, opts).map(Encoded::Linear),
            Symbology::Code93 => code93::encode(data, opts).map(Encoded::Linear),
            Symbology::Code93Ext => code93ext::encode(data, opts).map(Encoded::Linear),
            Symbology::Code128 => code128::encode(data, opts).map(Encoded::Linear),
            Symbology::Code11 => code11::encode(data, opts).map(Encoded::Linear),
            Symbology::Bc412 => bc412::encode(data, opts).map(Encoded::Linear),
            Symbology::Code32 => code32::encode(data, opts).map(Encoded::Linear),
            Symbology::Code2of5 => twoofive::encode_standard(data, opts).map(Encoded::Linear),
            Symbology::DataLogic2of5 => twoofive::encode_datalogic(data, opts).map(Encoded::Linear),
            Symbology::Iata2of5 => twoofive::encode_iata(data, opts).map(Encoded::Linear),
            Symbology::Industrial2of5 => {
                twoofive::encode_industrial(data, opts).map(Encoded::Linear)
            }
            Symbology::Coop2of5 => twoofive::encode_coop(data, opts).map(Encoded::Linear),
            Symbology::Matrix2of5 => twoofive::encode_matrix(data, opts).map(Encoded::Linear),
            Symbology::Msi => msi::encode(data, opts).map(Encoded::Linear),
            Symbology::Plessey => plessey::encode(data, opts).map(Encoded::Linear),
            Symbology::Posicode => posicode::encode(data, opts).map(Encoded::Linear),
            Symbology::Telepen => telepen::encode(data, opts).map(Encoded::Linear),
            Symbology::TelepenNumeric => telepen::encode_numeric(data, opts).map(Encoded::Linear),
            Symbology::Pharmacode => pharmacode::encode_one_track(data, opts).map(Encoded::Linear),
            Symbology::Pharmacode2 => {
                pharmacode::encode_two_track(data, opts).map(Encoded::Postal4State)
            }
            Symbology::Flattermarken => flattermarken::encode(data, opts).map(Encoded::Linear),
            Symbology::Vin => code39_wrappers::encode_vin(data, opts).map(Encoded::Linear),
            Symbology::Logmars => code39_wrappers::encode_logmars(data, opts).map(Encoded::Linear),
            Symbology::Pzn7 => code39_wrappers::encode_pzn7(data, opts).map(Encoded::Linear),
            Symbology::Pzn8 => code39_wrappers::encode_pzn8(data, opts).map(Encoded::Linear),
            Symbology::Ean13 => ean::encode_ean13(data, opts).map(Encoded::Linear),
            Symbology::Ean8 => ean::encode_ean8(data, opts).map(Encoded::Linear),
            Symbology::MarksAndSpencer => ean::encode_mands(data, opts).map(Encoded::Linear),
            Symbology::UpcA => ean::encode_upca(data, opts).map(Encoded::Linear),
            Symbology::UpcE => ean::encode_upce(data, opts).map(Encoded::Linear),
            Symbology::Ean2 => ean_addons::encode_ean2(data, opts).map(Encoded::Linear),
            Symbology::Ean5 => ean_addons::encode_ean5(data, opts).map(Encoded::Linear),
            Symbology::Isbn => book_codes::encode_isbn(data, opts).map(Encoded::Linear),
            Symbology::Ismn => book_codes::encode_ismn(data, opts).map(Encoded::Linear),
            Symbology::Issn => book_codes::encode_issn(data, opts).map(Encoded::Linear),
            Symbology::Daft => postal4::encode_daft(data, opts).map(Encoded::Postal4State),
            Symbology::Kix => postal4::encode_kix(data, opts).map(Encoded::Postal4State),
            Symbology::RoyalMail => {
                postal4::encode_royalmail(data, opts).map(Encoded::Postal4State)
            }
            Symbology::Postnet => postnet::encode_postnet(data, opts).map(Encoded::Postal4State),
            Symbology::Planet => postnet::encode_planet(data, opts).map(Encoded::Postal4State),
            Symbology::Identcode => {
                identleitcode::encode_identcode(data, opts).map(Encoded::Linear)
            }
            Symbology::Leitcode => identleitcode::encode_leitcode(data, opts).map(Encoded::Linear),
            Symbology::Gs1_128 => gs1_128::encode(data, opts).map(Encoded::Linear),
            Symbology::Sscc18 => gs1_128::encode_sscc18(data, opts).map(Encoded::Linear),
            Symbology::Ean14 => gs1_128::encode_ean14(data, opts).map(Encoded::Linear),
            Symbology::UpcCoupon => gs1_128::encode_coupon(data, opts).map(Encoded::Linear),
            Symbology::Gs1DataMatrix => {
                gs1_2d::encode_gs1_datamatrix(data, opts).map(Encoded::Matrix)
            }
            Symbology::Gs1DataMatrixRectangular => {
                gs1_2d::encode_gs1_datamatrix_rectangular(data, opts).map(Encoded::Matrix)
            }
            Symbology::Gs1DlDataMatrix => {
                gs1_2d::encode_gs1_dl_datamatrix(data, opts).map(Encoded::Matrix)
            }
            Symbology::Gs1DlQrCode => gs1_2d::encode_gs1_dl_qrcode(data, opts).map(Encoded::Matrix),
            Symbology::Gs1QrCode => gs1_2d::encode_gs1_qrcode(data, opts).map(Encoded::Matrix),
            Symbology::Ntin => gs1_2d::encode_ntin(data, opts).map(Encoded::Matrix),
            Symbology::Ppn => gs1_2d::encode_ppn(data, opts).map(Encoded::Matrix),
            Symbology::Ean13P2 => ean_combined::encode_ean13_p2(data, opts).map(Encoded::Linear),
            Symbology::Ean13P5 => ean_combined::encode_ean13_p5(data, opts).map(Encoded::Linear),
            Symbology::Ean8P2 => ean_combined::encode_ean8_p2(data, opts).map(Encoded::Linear),
            Symbology::Ean8P5 => ean_combined::encode_ean8_p5(data, opts).map(Encoded::Linear),
            Symbology::UpcAP2 => ean_combined::encode_upca_p2(data, opts).map(Encoded::Linear),
            Symbology::UpcAP5 => ean_combined::encode_upca_p5(data, opts).map(Encoded::Linear),
            Symbology::UpcEP2 => ean_combined::encode_upce_p2(data, opts).map(Encoded::Linear),
            Symbology::UpcEP5 => ean_combined::encode_upce_p5(data, opts).map(Encoded::Linear),
            Symbology::IsbnP5 => ean_combined::encode_isbn_p5(data, opts).map(Encoded::Linear),
            Symbology::IssnP2 => ean_combined::encode_issn_p2(data, opts).map(Encoded::Linear),
            Symbology::HibcCode128 => hibc::encode_code128(data, opts).map(Encoded::Linear),
            Symbology::HibcCode39 => hibc::encode_code39(data, opts).map(Encoded::Linear),
            Symbology::HibcDataMatrix => hibc::encode_datamatrix(data, opts).map(Encoded::Matrix),
            Symbology::HibcQrCode => hibc::encode_qrcode(data, opts).map(Encoded::Matrix),
            Symbology::HibcPdf417 => hibc::encode_pdf417(data, opts).map(Encoded::Matrix),
            Symbology::HibcMicroPdf417 => hibc::encode_micropdf417(data, opts).map(Encoded::Matrix),
            Symbology::HibcCodablockF => hibc::encode_codablockf(data, opts).map(Encoded::Stacked),
            Symbology::HibcAztecCode => hibc::encode_azteccode(data, opts).map(Encoded::Matrix),
            Symbology::HibcDataMatrixRectangular => {
                hibc::encode_datamatrix_rectangular(data, opts).map(Encoded::Matrix)
            }
            Symbology::HibcPasCode128 => hibc::encode_pas_code128(data, opts).map(Encoded::Linear),
            Symbology::HibcPasCode39 => hibc::encode_pas_code39(data, opts).map(Encoded::Linear),
            Symbology::HibcPasDataMatrix => {
                hibc::encode_pas_datamatrix(data, opts).map(Encoded::Matrix)
            }
            Symbology::HibcPasQrCode => hibc::encode_pas_qrcode(data, opts).map(Encoded::Matrix),
            Symbology::HibcPasPdf417 => hibc::encode_pas_pdf417(data, opts).map(Encoded::Matrix),
            Symbology::HibcPasMicroPdf417 => {
                hibc::encode_pas_micropdf417(data, opts).map(Encoded::Matrix)
            }
            Symbology::HibcPasCodablockF => {
                hibc::encode_pas_codablockf(data, opts).map(Encoded::Stacked)
            }
            Symbology::UpuS10 => postal_misc::encode_upu_s10(data, opts).map(Encoded::Linear),
            Symbology::KoreanPostal => {
                postal_misc::encode_korean_postal(data, opts).map(Encoded::Linear)
            }
            Symbology::Cepnet => postal_misc::encode_cepnet(data, opts).map(Encoded::Linear),
            Symbology::ItalianPostal25 => {
                postal_misc::encode_italian_postal_25(data, opts).map(Encoded::Linear)
            }
            Symbology::ItalianPostal39 => {
                postal_misc::encode_italian_postal_39(data, opts).map(Encoded::Linear)
            }
            Symbology::Dpd => postal_misc::encode_dpd(data, opts).map(Encoded::Linear),
            Symbology::DpPostmatrix => {
                postal_misc::encode_dp_postmatrix(data, opts).map(Encoded::Matrix)
            }
            Symbology::Mailmark => mailmark::encode_typed(data, opts).map(Encoded::Matrix),
            Symbology::Mailmark2d => mailmark::encode_2d(data, opts).map(Encoded::Matrix),
            Symbology::SwissQrCode => swiss_qr::encode(data, opts).map(Encoded::Matrix),
            Symbology::UspsImpb => usps_impb::encode(data, opts).map(Encoded::Linear),
            Symbology::UspsOneCode => usps_onecode::encode(data, opts).map(Encoded::Postal4State),
            Symbology::JapanPost => japan_post::encode(data, opts).map(Encoded::Postal4State),
            Symbology::AuspostCustomer => {
                auspost::encode_customer(data, opts).map(Encoded::Postal4State)
            }
            Symbology::AuspostReplyPaid => {
                auspost::encode_reply_paid(data, opts).map(Encoded::Postal4State)
            }
            Symbology::AuspostRouting => {
                auspost::encode_routing(data, opts).map(Encoded::Postal4State)
            }
            Symbology::AuspostRedirection => {
                auspost::encode_redirection(data, opts).map(Encoded::Postal4State)
            }
            Symbology::DatabarOmni => databar::encode_omni(data, opts).map(Encoded::Linear),
            Symbology::DatabarTruncated => {
                databar::encode_truncated(data, opts).map(Encoded::Linear)
            }
            Symbology::DatabarLimited => databar::encode_limited(data, opts).map(Encoded::Linear),
            Symbology::DatabarStacked => databar::encode_stacked(data, opts).map(Encoded::Matrix),
            Symbology::DatabarStackedOmni => {
                databar::encode_stackedomni(data, opts).map(Encoded::Matrix)
            }
            Symbology::DatabarExpanded => {
                databar_expanded::encode(data, false).map(Encoded::Linear)
            }
            Symbology::DatabarExpandedStacked => {
                databar_expanded::encode_stacked(data, false).map(Encoded::Matrix)
            }
            Symbology::Codabar => codabar::encode(data, opts).map(Encoded::Linear),
            Symbology::Interleaved2of5 => interleaved2of5::encode(data, opts).map(Encoded::Linear),
            Symbology::Itf14 => interleaved2of5::encode_itf14(data, opts).map(Encoded::Linear),
            // QR Code routing: when the `prefer-native-qrcode` Cargo
            // feature is enabled, route through the BWIPP-faithful
            // qrcode_native encoder (byte-for-byte verified vs bwip-js
            // on 9 oracle-pinned corpus rows). Otherwise route through
            // the upstream `qrcode` crate substrate.
            #[cfg(feature = "prefer-native-qrcode")]
            Symbology::QrCode => qrcode_native::encode(data.as_bytes()).map(Encoded::Matrix),
            #[cfg(not(feature = "prefer-native-qrcode"))]
            Symbology::QrCode => qrcode_::encode(data, opts).map(Encoded::Matrix),
            // Micro QR Code routing: same feature-gated split. The
            // native path uses encode_micro_auto which iterates M1..M4
            // to find the smallest fit; the substrate path delegates
            // to the `qrcode` crate's micro-QR encoder.
            #[cfg(feature = "prefer-native-qrcode")]
            Symbology::MicroQrCode => {
                qrcode_native::encode_micro_auto(data.as_bytes(), /*requested_ec=*/ 1)
                    .map(Encoded::Matrix)
            }
            #[cfg(not(feature = "prefer-native-qrcode"))]
            Symbology::MicroQrCode => qrcode_::encode_micro(data, opts).map(Encoded::Matrix),
            // Rectangular Micro QR (rMQR, ISO/IEC 23941:2022). Always
            // routed through the native encoder — there is no `qrcode`-
            // crate substrate for rMQR. The encoder is byte-for-byte
            // verified against bwip-js on a 16-row corpus
            // (`encode_rmqr_pixs_corpus_matches_oracle`). The caller
            // selects size via `version=R7x43` (default `R7x43`) and EC
            // via `eclevel=M|H` (default `M`); rMQR does not accept L/Q.
            Symbology::RectangularMicroQrCode => {
                let version = opts
                    .extras
                    .iter()
                    .find_map(|(k, v)| (k == "version").then(|| v.clone()))
                    .unwrap_or_else(|| "R7x43".to_string());
                let eclevel_char = opts
                    .extras
                    .iter()
                    .find_map(|(k, v)| (k == "eclevel").then(|| v.clone()))
                    .unwrap_or_else(|| "M".to_string());
                let ec_level = match eclevel_char.as_str() {
                    "M" | "m" => 1u8,
                    "H" | "h" => 3u8,
                    other => {
                        return Err(Error::InvalidOption(format!(
                            "rectangularmicroqrcode: eclevel {other} not supported (must be M or H)"
                        )))
                    }
                };
                qrcode_native::encode_rmqr(data.as_bytes(), &version, ec_level).map(Encoded::Matrix)
            }
            Symbology::DataMatrix => datamatrix_::encode(data, opts).map(Encoded::Matrix),
            Symbology::DataMatrixRectangular => {
                let mut o = opts.clone();
                o.extras.retain(|(k, _)| k != "shape");
                o.extras
                    .push(("shape".to_string(), "rectangular".to_string()));
                datamatrix_::encode(data, &o).map(Encoded::Matrix)
            }
            Symbology::DataMatrixRectangularExtension => {
                datamatrix_::encode_rectangular_extension(data, opts).map(Encoded::Matrix)
            }
            Symbology::CodablockF => codablockf::encode(data, opts).map(Encoded::Stacked),
            Symbology::Pdf417 => pdf417::encode(data, opts).map(Encoded::Matrix),
            Symbology::Pdf417Truncated => pdf417::encode_truncated(data, opts).map(Encoded::Matrix),
            Symbology::MicroPdf417 => micropdf417::encode(data, opts).map(Encoded::Matrix),
            Symbology::DotCode => {
                dotcode::encode(data.as_bytes()).map(|sym| Encoded::Dots(sym.to_dotmatrix()))
            }
            Symbology::Gs1DotCode => {
                gs1_dotcode::encode(data.as_bytes()).map(|sym| Encoded::Dots(sym.to_dotmatrix()))
            }
            Symbology::Code16k => code16k::encode(data.as_bytes()).map(Encoded::Matrix),
            Symbology::Code49 => code49::encode(data.as_bytes()).map(Encoded::Matrix),
            Symbology::CodeOne => codeone::encode(data.as_bytes()).map(Encoded::Matrix),
            Symbology::Maxicode => {
                // Mode dispatch via `opts.extras["mode"]` (default 4).
                // Modes 2/3 use the structured carrier-message format
                // (postcode | country | service | secondary, GS-separated);
                // modes 4/6 are general-purpose; mode 5 is enhanced-ECC.
                let mode = opts
                    .get("mode")
                    .map(|s| s.parse::<u8>())
                    .transpose()
                    .map_err(|_| {
                        Error::InvalidOption(
                            "maxicode: mode must be a number 2..=6 (defaults to 4)".into(),
                        )
                    })?
                    .unwrap_or(4);
                let sym = match mode {
                    2 => maxicode::encode_mode_2(data.as_bytes())?,
                    3 => maxicode::encode_mode_3(data.as_bytes())?,
                    4 => maxicode::encode_mode_4(data.as_bytes())?,
                    5 => maxicode::encode_mode_5(data.as_bytes())?,
                    6 => maxicode::encode_mode_6(data.as_bytes())?,
                    other => {
                        return Err(Error::InvalidOption(format!(
                            "maxicode: mode={other} not supported \
                             (only 2..=6 are implemented)",
                        )));
                    }
                };
                Ok(Encoded::Hex(Box::new(sym)))
            }
            Symbology::Ultracode => ultracode::encode(data, opts).map(Encoded::ColorMatrix),
            Symbology::AztecCode => aztec::encode(data.as_bytes()).map(Encoded::Matrix),
            Symbology::AztecCodeCompact => {
                aztec::encode_compact(data.as_bytes()).map(Encoded::Matrix)
            }
            Symbology::AztecRune => aztec::encode_rune(data).map(Encoded::Matrix),
            Symbology::ChannelCode => channelcode::encode(data, opts).map(Encoded::Linear),
            Symbology::CompositeDatabarOmniCca => {
                composite::encode_databaromni_cca(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarOmniCcb => {
                composite::encode_databaromni_ccb(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarTruncatedCca => {
                composite::encode_databartruncated_cca(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarTruncatedCcb => {
                composite::encode_databartruncated_ccb(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarStackedCca => {
                composite::encode_databarstacked_cca(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarStackedCcb => {
                composite::encode_databarstacked_ccb(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarStackedOmniCca => {
                composite::encode_databarstackedomni_cca(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarStackedOmniCcb => {
                composite::encode_databarstackedomni_ccb(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarExpandedStackedCca => {
                composite::encode_databarexpandedstacked_cca(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarExpandedStackedCcb => {
                composite::encode_databarexpandedstacked_ccb(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarLimitedCca => {
                composite::encode_databarlimited_cca(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarLimitedCcb => {
                composite::encode_databarlimited_ccb(data).map(Encoded::Matrix)
            }
            Symbology::CompositeGs1_128Cca => {
                composite::encode_gs1_128_cca(data).map(Encoded::Matrix)
            }
            Symbology::CompositeGs1_128Ccb => {
                composite::encode_gs1_128_ccb(data).map(Encoded::Matrix)
            }
            Symbology::CompositeEan13Cca => composite::encode_ean13_cca(data).map(Encoded::Matrix),
            Symbology::CompositeEan13Ccb => composite::encode_ean13_ccb(data).map(Encoded::Matrix),
            Symbology::CompositeUpcaCca => composite::encode_upca_cca(data).map(Encoded::Matrix),
            Symbology::CompositeUpcaCcb => composite::encode_upca_ccb(data).map(Encoded::Matrix),
            Symbology::CompositeEan8Cca => composite::encode_ean8_cca(data).map(Encoded::Matrix),
            Symbology::CompositeEan8Ccb => composite::encode_ean8_ccb(data).map(Encoded::Matrix),
            Symbology::CompositeUpceCca => composite::encode_upce_cca(data).map(Encoded::Matrix),
            Symbology::CompositeUpceCcb => composite::encode_upce_ccb(data).map(Encoded::Matrix),
            Symbology::CompositeDatabarExpandedCca => {
                composite::encode_databar_expanded_cca(data).map(Encoded::Matrix)
            }
            Symbology::CompositeDatabarExpandedCcb => {
                composite::encode_databar_expanded_ccb(data).map(Encoded::Matrix)
            }
            Symbology::CompositeGs1_128Ccc => {
                composite::encode_gs1_128_ccc(data).map(Encoded::Matrix)
            }
            Symbology::HanXinCode => hanxin::encode_symbology(data, opts).map(Encoded::Matrix),
        }
    }

    /// Stable string identifier (used by [`Symbology::from_id`] and the CLI).
    pub fn id(self) -> &'static str {
        match self {
            Symbology::Code39 => "code39",
            Symbology::Code39Ext => "code39ext",
            Symbology::Code93 => "code93",
            Symbology::Code93Ext => "code93ext",
            Symbology::Code128 => "code128",
            Symbology::Code11 => "code11",
            Symbology::Bc412 => "bc412",
            Symbology::Code32 => "code32",
            Symbology::Code2of5 => "code2of5",
            Symbology::DataLogic2of5 => "datalogic2of5",
            Symbology::Iata2of5 => "iata2of5",
            Symbology::Industrial2of5 => "industrial2of5",
            Symbology::Coop2of5 => "coop2of5",
            Symbology::Matrix2of5 => "matrix2of5",
            Symbology::Msi => "msi",
            Symbology::Plessey => "plessey",
            Symbology::Posicode => "posicode",
            Symbology::Telepen => "telepen",
            Symbology::TelepenNumeric => "telepennumeric",
            Symbology::Pharmacode => "pharmacode",
            Symbology::Pharmacode2 => "pharmacode2",
            Symbology::Flattermarken => "flattermarken",
            Symbology::Vin => "vin",
            Symbology::Logmars => "logmars",
            Symbology::Pzn7 => "pzn7",
            Symbology::Pzn8 => "pzn8",
            Symbology::Ean13 => "ean13",
            Symbology::Ean8 => "ean8",
            Symbology::MarksAndSpencer => "mands",
            Symbology::UpcA => "upca",
            Symbology::UpcE => "upce",
            Symbology::Ean2 => "ean2",
            Symbology::Ean5 => "ean5",
            Symbology::Isbn => "isbn13",
            Symbology::Ismn => "ismn",
            Symbology::Issn => "issn",
            Symbology::Daft => "daft",
            Symbology::Kix => "kix",
            Symbology::RoyalMail => "royalmail",
            Symbology::Postnet => "postnet",
            Symbology::Planet => "planet",
            Symbology::Identcode => "identcode",
            Symbology::Leitcode => "leitcode",
            Symbology::Gs1_128 => "gs1-128",
            Symbology::Sscc18 => "sscc18",
            Symbology::Ean14 => "ean14",
            Symbology::UpcCoupon => "upc_coupon",
            Symbology::Gs1DataMatrix => "gs1datamatrix",
            Symbology::Gs1DataMatrixRectangular => "gs1datamatrixrectangular",
            Symbology::Gs1DlDataMatrix => "gs1dldatamatrix",
            Symbology::Gs1DlQrCode => "gs1dlqrcode",
            Symbology::Gs1QrCode => "gs1qrcode",
            Symbology::Ntin => "ntin",
            Symbology::Ppn => "ppn",
            Symbology::Ean13P2 => "ean13p2",
            Symbology::Ean13P5 => "ean13p5",
            Symbology::Ean8P2 => "ean8p2",
            Symbology::Ean8P5 => "ean8p5",
            Symbology::UpcAP2 => "upcap2",
            Symbology::UpcAP5 => "upcap5",
            Symbology::UpcEP2 => "upcep2",
            Symbology::UpcEP5 => "upcep5",
            Symbology::IsbnP5 => "isbn13p5",
            Symbology::IssnP2 => "issnp2",
            Symbology::HibcCode128 => "hibc_lic_code128",
            Symbology::HibcCode39 => "hibc_lic_code39",
            Symbology::HibcDataMatrix => "hibc_lic_datamatrix",
            Symbology::HibcQrCode => "hibc_lic_qrcode",
            Symbology::HibcPdf417 => "hibc_lic_pdf417",
            Symbology::HibcMicroPdf417 => "hibc_lic_micropdf417",
            Symbology::HibcCodablockF => "hibc_lic_codablockf",
            Symbology::HibcAztecCode => "hibc_lic_azteccode",
            Symbology::HibcDataMatrixRectangular => "hibc_lic_datamatrix_rectangular",
            Symbology::HibcPasCode128 => "hibc_pas_code128",
            Symbology::HibcPasCode39 => "hibc_pas_code39",
            Symbology::HibcPasDataMatrix => "hibc_pas_datamatrix",
            Symbology::HibcPasQrCode => "hibc_pas_qrcode",
            Symbology::HibcPasPdf417 => "hibc_pas_pdf417",
            Symbology::HibcPasMicroPdf417 => "hibc_pas_micropdf417",
            Symbology::HibcPasCodablockF => "hibc_pas_codablockf",
            Symbology::UpuS10 => "upu_s10",
            Symbology::KoreanPostal => "korean_postal",
            Symbology::Cepnet => "cepnet",
            Symbology::ItalianPostal25 => "italian_postal_25",
            Symbology::ItalianPostal39 => "italian_postal_39",
            Symbology::Dpd => "dpd",
            Symbology::DpPostmatrix => "dp_postmatrix",
            Symbology::Mailmark => "mailmark",
            Symbology::Mailmark2d => "mailmark2d",
            Symbology::SwissQrCode => "swissqrcode",
            Symbology::UspsImpb => "usps_impb",
            Symbology::UspsOneCode => "usps_onecode",
            Symbology::JapanPost => "japanpost",
            Symbology::AuspostCustomer => "auspost_customer",
            Symbology::AuspostReplyPaid => "auspost_reply",
            Symbology::AuspostRouting => "auspost_routing",
            Symbology::AuspostRedirection => "auspost_redirection",
            Symbology::DatabarOmni => "databar_omni",
            Symbology::DatabarTruncated => "databar_truncated",
            Symbology::DatabarLimited => "databar_limited",
            Symbology::DatabarStacked => "databar_stacked",
            Symbology::DatabarStackedOmni => "databar_stacked_omni",
            Symbology::DatabarExpanded => "databar_expanded",
            Symbology::DatabarExpandedStacked => "databar_expanded_stacked",
            Symbology::Codabar => "codabar",
            Symbology::Interleaved2of5 => "interleaved2of5",
            Symbology::Itf14 => "itf14",
            Symbology::QrCode => "qrcode",
            Symbology::MicroQrCode => "microqrcode",
            Symbology::RectangularMicroQrCode => "rectangularmicroqrcode",
            Symbology::DataMatrix => "datamatrix",
            Symbology::DataMatrixRectangular => "datamatrixrectangular",
            Symbology::DataMatrixRectangularExtension => "datamatrixrectangularextension",
            Symbology::CodablockF => "codablockf",
            Symbology::Pdf417 => "pdf417",
            Symbology::Pdf417Truncated => "pdf417_truncated",
            Symbology::MicroPdf417 => "micropdf417",
            Symbology::DotCode => "dotcode",
            Symbology::Gs1DotCode => "gs1dotcode",
            Symbology::Code16k => "code16k",
            Symbology::Code49 => "code49",
            Symbology::CodeOne => "codeone",
            Symbology::Maxicode => "maxicode",
            Symbology::Ultracode => "ultracode",
            Symbology::AztecCode => "azteccode",
            Symbology::AztecCodeCompact => "azteccodecompact",
            Symbology::AztecRune => "aztecrune",
            Symbology::ChannelCode => "channelcode",
            Symbology::CompositeDatabarOmniCca => "composite_databar_omni_cca",
            Symbology::CompositeDatabarOmniCcb => "composite_databar_omni_ccb",
            Symbology::CompositeDatabarTruncatedCca => "composite_databar_truncated_cca",
            Symbology::CompositeDatabarTruncatedCcb => "composite_databar_truncated_ccb",
            Symbology::CompositeDatabarStackedCca => "composite_databar_stacked_cca",
            Symbology::CompositeDatabarStackedCcb => "composite_databar_stacked_ccb",
            Symbology::CompositeDatabarStackedOmniCca => "composite_databar_stacked_omni_cca",
            Symbology::CompositeDatabarStackedOmniCcb => "composite_databar_stacked_omni_ccb",
            Symbology::CompositeDatabarExpandedStackedCca => {
                "composite_databar_expanded_stacked_cca"
            }
            Symbology::CompositeDatabarExpandedStackedCcb => {
                "composite_databar_expanded_stacked_ccb"
            }
            Symbology::CompositeDatabarLimitedCca => "composite_databar_limited_cca",
            Symbology::CompositeDatabarLimitedCcb => "composite_databar_limited_ccb",
            Symbology::CompositeGs1_128Cca => "composite_gs1_128_cca",
            Symbology::CompositeGs1_128Ccb => "composite_gs1_128_ccb",
            Symbology::CompositeEan13Cca => "composite_ean13_cca",
            Symbology::CompositeEan13Ccb => "composite_ean13_ccb",
            Symbology::CompositeUpcaCca => "composite_upca_cca",
            Symbology::CompositeUpcaCcb => "composite_upca_ccb",
            Symbology::CompositeEan8Cca => "composite_ean8_cca",
            Symbology::CompositeEan8Ccb => "composite_ean8_ccb",
            Symbology::CompositeUpceCca => "composite_upce_cca",
            Symbology::CompositeUpceCcb => "composite_upce_ccb",
            Symbology::CompositeDatabarExpandedCca => "composite_databar_expanded_cca",
            Symbology::CompositeDatabarExpandedCcb => "composite_databar_expanded_ccb",
            Symbology::CompositeGs1_128Ccc => "composite_gs1_128_ccc",
            Symbology::HanXinCode => "hanxin",
        }
    }

    /// Parse a string identifier (the inverse of [`Symbology::id`]).
    /// Also accepts a handful of historical / convenience aliases —
    /// `code128a/b/c` route to [`Symbology::Code128`], `qrcode_iso`
    /// to [`Symbology::QrCode`], `swedish_postal` to [`Symbology::Sscc18`],
    /// etc. Returns `None` for any unknown id.
    ///
    /// # Example
    /// ```
    /// use bwipp::Symbology;
    /// assert_eq!(Symbology::from_id("ean13"), Some(Symbology::Ean13));
    /// assert_eq!(Symbology::from_id("EAN13"), None); // case-sensitive
    /// assert_eq!(Symbology::from_id("code128a"), Some(Symbology::Code128)); // alias
    /// assert_eq!(Symbology::from_id("not a symbology"), None);
    /// ```
    pub fn from_id(s: &str) -> Option<Self> {
        Some(match s {
            "code39" => Symbology::Code39,
            "code39ext" => Symbology::Code39Ext,
            "code93" => Symbology::Code93,
            "code93ext" => Symbology::Code93Ext,
            "code128" => Symbology::Code128,
            "code11" => Symbology::Code11,
            "bc412" => Symbology::Bc412,
            "code32" => Symbology::Code32,
            "code2of5" => Symbology::Code2of5,
            "datalogic2of5" => Symbology::DataLogic2of5,
            "iata2of5" => Symbology::Iata2of5,
            "industrial2of5" => Symbology::Industrial2of5,
            "coop2of5" => Symbology::Coop2of5,
            "matrix2of5" => Symbology::Matrix2of5,
            "msi" => Symbology::Msi,
            "plessey" | "plessey_bidir" => Symbology::Plessey,
            "posicode" => Symbology::Posicode,
            "telepen" => Symbology::Telepen,
            "telepennumeric" | "telepen_alpha" | "telepen_numeric" => Symbology::TelepenNumeric,
            "pharmacode" => Symbology::Pharmacode,
            "pharmacode2" => Symbology::Pharmacode2,
            "flattermarken" => Symbology::Flattermarken,
            "vin" => Symbology::Vin,
            "logmars" => Symbology::Logmars,
            // `pzn7` is canonical; `pzn` is the upstream BWIPP generic name —
            // BWIPP dispatches by input length (6 → PZN7, 7 → PZN8). Our
            // encoder requires explicit `pzn7`/`pzn8`; the generic alias
            // defaults to PZN7 (the more common 6-digit form).
            "pzn7" | "pzn" => Symbology::Pzn7,
            "pzn8" => Symbology::Pzn8,
            "ean13" => Symbology::Ean13,
            "ean8" => Symbology::Ean8,
            "mands" | "marks_and_spencer" => Symbology::MarksAndSpencer,
            "upca" => Symbology::UpcA,
            "upce" => Symbology::UpcE,
            "ean2" => Symbology::Ean2,
            "ean5" => Symbology::Ean5,
            "isbn" | "isbn13" => Symbology::Isbn,
            "ismn" => Symbology::Ismn,
            "issn" => Symbology::Issn,
            "daft" => Symbology::Daft,
            "kix" => Symbology::Kix,
            "royalmail" => Symbology::RoyalMail,
            // PostNet: the Python catalog exposes per-digit-count aliases
            // (5 / 9 / 11 digits). The Rust encoder validates the digit
            // count itself, so all aliases route to the single variant.
            "postnet" | "usps_postnet5" | "usps_postnet9" | "usps_postnet11" => Symbology::Postnet,
            // PLANET: same pattern (12 / 14 digit aliases).
            "planet" | "planet12" | "planet14" => Symbology::Planet,
            "identcode" => Symbology::Identcode,
            "leitcode" => Symbology::Leitcode,
            "gs1-128" | "ucc128" | "gs1_128" => Symbology::Gs1_128,
            "sscc18" | "nve18" => Symbology::Sscc18,
            // EAN-14 / GTIN-14 — upstream `bwipp_ean14`. Accepts 13 or
            // 14 digits, optionally prefixed with `(01)`.
            "ean14" | "gtin14" => Symbology::Ean14,
            "upc_coupon" | "upc-coupon" | "gs1northamericancoupon" => Symbology::UpcCoupon,
            "gs1datamatrix" | "gs1-datamatrix" => Symbology::Gs1DataMatrix,
            "gs1datamatrixrectangular" | "gs1-datamatrix-rectangular" => {
                Symbology::Gs1DataMatrixRectangular
            }
            "gs1dldatamatrix" | "gs1-dl-datamatrix" => Symbology::Gs1DlDataMatrix,
            "gs1dlqrcode" | "gs1-dl-qrcode" => Symbology::Gs1DlQrCode,
            "gs1qrcode" | "gs1-qrcode" => Symbology::Gs1QrCode,
            "ntin" => Symbology::Ntin,
            "ppn" => Symbology::Ppn,
            "ean13p2" => Symbology::Ean13P2,
            "ean13p5" => Symbology::Ean13P5,
            "ean8p2" => Symbology::Ean8P2,
            "ean8p5" => Symbology::Ean8P5,
            "upcap2" => Symbology::UpcAP2,
            "upcap5" => Symbology::UpcAP5,
            "upcep2" => Symbology::UpcEP2,
            "upcep5" => Symbology::UpcEP5,
            "isbn13p5" | "isbnp5" => Symbology::IsbnP5,
            "issnp2" => Symbology::IssnP2,
            "hibc_lic_code128" | "hibccode128" => Symbology::HibcCode128,
            "hibc_lic_code39" | "hibccode39" => Symbology::HibcCode39,
            "hibc_lic_datamatrix" | "hibcdatamatrix" => Symbology::HibcDataMatrix,
            "hibc_lic_qrcode" | "hibcqrcode" => Symbology::HibcQrCode,
            "hibc_lic_pdf417" | "hibcpdf417" => Symbology::HibcPdf417,
            "hibc_lic_micropdf417" | "hibcmicropdf417" => Symbology::HibcMicroPdf417,
            "hibc_lic_codablockf" | "hibccodablockf" => Symbology::HibcCodablockF,
            "hibc_lic_azteccode" | "hibcazteccode" => Symbology::HibcAztecCode,
            "hibc_lic_datamatrix_rectangular" | "hibcdatamatrixrectangular" => {
                Symbology::HibcDataMatrixRectangular
            }
            "hibc_pas_code128" => Symbology::HibcPasCode128,
            "hibc_pas_code39" => Symbology::HibcPasCode39,
            "hibc_pas_datamatrix" => Symbology::HibcPasDataMatrix,
            "hibc_pas_qrcode" => Symbology::HibcPasQrCode,
            "hibc_pas_pdf417" => Symbology::HibcPasPdf417,
            "hibc_pas_micropdf417" => Symbology::HibcPasMicroPdf417,
            "hibc_pas_codablockf" => Symbology::HibcPasCodablockF,
            "upu_s10" => Symbology::UpuS10,
            "korean_postal" => Symbology::KoreanPostal,
            "cepnet" => Symbology::Cepnet,
            "italian_postal_25" => Symbology::ItalianPostal25,
            "italian_postal_39" => Symbology::ItalianPostal39,
            "dpd" => Symbology::Dpd,
            "dp_postmatrix" => Symbology::DpPostmatrix,
            "mailmark" => Symbology::Mailmark,
            "mailmark2d" => Symbology::Mailmark2d,
            "swissqrcode" | "swiss_qrcode" => Symbology::SwissQrCode,
            "usps_impb" | "uspsimpb" => Symbology::UspsImpb,
            // USPS Intelligent Mail (OneCode / IMb): the Python catalog has
            // both `usps_onecode` and `usps_imb` aliases; both render the
            // same 4-state postal symbol.
            "usps_onecode" | "uspsonecode" | "onecode" | "imb" | "usps_imb" => {
                Symbology::UspsOneCode
            }
            "japanpost" | "japan_post" => Symbology::JapanPost,
            // `auspost` is the upstream BWIPP generic name; locally split
            // into per-service variants. The generic alias defaults to
            // Customer (BWIPP's `auspost` default `opts.type` is `'st'`,
            // which is the customer barcode FCC 11).
            "auspost_customer" | "auspost" => Symbology::AuspostCustomer,
            "auspost_reply" | "auspost_replypaid" => Symbology::AuspostReplyPaid,
            "auspost_routing" => Symbology::AuspostRouting,
            "auspost_redirection" => Symbology::AuspostRedirection,
            "databar_omni" | "databaromni" => Symbology::DatabarOmni,
            "databar_truncated" | "databartruncated" => Symbology::DatabarTruncated,
            "databar_limited" | "databarlimited" => Symbology::DatabarLimited,
            "databar_stacked" | "databarstacked" => Symbology::DatabarStacked,
            "databar_stacked_omni" | "databarstackedomni" => Symbology::DatabarStackedOmni,
            "databar_expanded" | "databarexpanded" => Symbology::DatabarExpanded,
            "databar_expanded_stacked" | "databarexpandedstacked" => {
                Symbology::DatabarExpandedStacked
            }
            // Catalog id aliases for Code 128 subsets (BWIPP auto-selects subset).
            "code128a" | "code128b" | "code128c" => Symbology::Code128,
            // Swedish Postal is an SSCC-18 by another name.
            "swedish_postal" => Symbology::Sscc18,
            // `rationalizedCodabar` is the upstream BWIPP encoder name.
            "codabar" | "rationalizedCodabar" | "rationalizedcodabar" => Symbology::Codabar,
            "interleaved2of5" => Symbology::Interleaved2of5,
            "itf14" => Symbology::Itf14,
            "qrcode" | "qrcode_iso" | "qr_code" => Symbology::QrCode,
            "microqrcode" | "micro_qrcode" | "micro_qr" => Symbology::MicroQrCode,
            "rectangularmicroqrcode" | "rectangular_micro_qrcode" | "rmqr" => {
                Symbology::RectangularMicroQrCode
            }
            "datamatrix" => Symbology::DataMatrix,
            "datamatrixrectangular" | "datamatrix_rectangular" => Symbology::DataMatrixRectangular,
            "datamatrixrectangularextension" | "datamatrix_rectangular_extension" | "dmre" => {
                Symbology::DataMatrixRectangularExtension
            }
            "codablockf" => Symbology::CodablockF,
            "pdf417" | "pdf_417" => Symbology::Pdf417,
            "pdf417_truncated" | "pdf417_compact" | "pdf417compact" => Symbology::Pdf417Truncated,
            "micropdf417" | "micro_pdf417" => Symbology::MicroPdf417,
            "dotcode" | "dot_code" => Symbology::DotCode,
            "gs1dotcode" | "gs1_dotcode" | "gs1-dotcode" => Symbology::Gs1DotCode,
            "code16k" | "code_16k" | "code-16k" => Symbology::Code16k,
            "code49" | "code_49" | "code-49" => Symbology::Code49,
            "codeone" | "code_one" | "code-one" => Symbology::CodeOne,
            "maxicode" => Symbology::Maxicode,
            "ultracode" => Symbology::Ultracode,
            "azteccode" | "aztec" | "aztec_code" => Symbology::AztecCode,
            "azteccodecompact" | "aztec_code_compact" => Symbology::AztecCodeCompact,
            "aztecrune" | "aztec_rune" => Symbology::AztecRune,
            "channelcode" | "channel_code" => Symbology::ChannelCode,
            // Upstream BWIPP exposes the composite encoders under the bare
            // names `databaromnicomposite`/`ean13composite`/etc. and chooses
            // CC-A vs CC-B vs CC-C from the `cc` option. We expose explicit
            // per-version ids (`_cca`/`_ccb`/`_ccc`); the bare upstream alias
            // routes to the CC-A variant (CC-A is the smallest, BWIPP's
            // implicit default for fits-in-CC-A payloads).
            "composite_databar_omni_cca" | "databaromnicomposite_cca" | "databaromnicomposite" => {
                Symbology::CompositeDatabarOmniCca
            }
            "composite_databar_omni_ccb" | "databaromnicomposite_ccb" => {
                Symbology::CompositeDatabarOmniCcb
            }
            "composite_databar_truncated_cca"
            | "databartruncatedcomposite_cca"
            | "databartruncatedcomposite" => Symbology::CompositeDatabarTruncatedCca,
            "composite_databar_truncated_ccb" | "databartruncatedcomposite_ccb" => {
                Symbology::CompositeDatabarTruncatedCcb
            }
            "composite_databar_stacked_cca"
            | "databarstackedcomposite_cca"
            | "databarstackedcomposite" => Symbology::CompositeDatabarStackedCca,
            "composite_databar_stacked_ccb" | "databarstackedcomposite_ccb" => {
                Symbology::CompositeDatabarStackedCcb
            }
            "composite_databar_stacked_omni_cca"
            | "databarstackedomnicomposite_cca"
            | "databarstackedomnicomposite" => Symbology::CompositeDatabarStackedOmniCca,
            "composite_databar_stacked_omni_ccb" | "databarstackedomnicomposite_ccb" => {
                Symbology::CompositeDatabarStackedOmniCcb
            }
            "composite_databar_expanded_stacked_cca"
            | "databarexpandedstackedcomposite_cca"
            | "databarexpandedstackedcomposite" => Symbology::CompositeDatabarExpandedStackedCca,
            "composite_databar_expanded_stacked_ccb" | "databarexpandedstackedcomposite_ccb" => {
                Symbology::CompositeDatabarExpandedStackedCcb
            }
            "composite_databar_limited_cca"
            | "databarlimitedcomposite_cca"
            | "databarlimitedcomposite" => Symbology::CompositeDatabarLimitedCca,
            "composite_databar_limited_ccb" | "databarlimitedcomposite_ccb" => {
                Symbology::CompositeDatabarLimitedCcb
            }
            "composite_gs1_128_cca" | "gs1-128composite_cca" | "gs1-128composite" => {
                Symbology::CompositeGs1_128Cca
            }
            "composite_gs1_128_ccb" | "gs1-128composite_ccb" => Symbology::CompositeGs1_128Ccb,
            "composite_ean13_cca" | "ean13composite_cca" | "ean13composite" => {
                Symbology::CompositeEan13Cca
            }
            "composite_ean13_ccb" | "ean13composite_ccb" => Symbology::CompositeEan13Ccb,
            "composite_upca_cca" | "upcacomposite_cca" | "upcacomposite" => {
                Symbology::CompositeUpcaCca
            }
            "composite_upca_ccb" | "upcacomposite_ccb" => Symbology::CompositeUpcaCcb,
            "composite_ean8_cca" | "ean8composite_cca" | "ean8composite" => {
                Symbology::CompositeEan8Cca
            }
            "composite_ean8_ccb" | "ean8composite_ccb" => Symbology::CompositeEan8Ccb,
            "composite_upce_cca" | "upcecomposite_cca" | "upcecomposite" => {
                Symbology::CompositeUpceCca
            }
            "composite_upce_ccb" | "upcecomposite_ccb" => Symbology::CompositeUpceCcb,
            "composite_databar_expanded_cca"
            | "databarexpandedcomposite_cca"
            | "databarexpandedcomposite" => Symbology::CompositeDatabarExpandedCca,
            "composite_databar_expanded_ccb" | "databarexpandedcomposite_ccb" => {
                Symbology::CompositeDatabarExpandedCcb
            }
            "composite_gs1_128_ccc" | "gs1-128composite_ccc" => Symbology::CompositeGs1_128Ccc,
            "hanxin" | "hanxincode" | "han_xin_code" => Symbology::HanXinCode,
            _ => return None,
        })
    }

    /// The full list of implemented symbologies, in stable order.
    ///
    /// # Example
    ///
    /// ```
    /// use bwipp::Symbology;
    ///
    /// // Symbology::all() is the source of truth for the supported set.
    /// // Every variant comes with a stable id() and display_name().
    /// let count = Symbology::all().len();
    /// assert!(count > 100);
    /// let names: Vec<_> = Symbology::all().iter().map(|s| s.id()).collect();
    /// assert!(names.contains(&"code128"));
    /// ```
    pub fn all() -> &'static [Symbology] {
        &[
            Symbology::Code39,
            Symbology::Code39Ext,
            Symbology::Code93,
            Symbology::Code93Ext,
            Symbology::Code128,
            Symbology::Code11,
            Symbology::Bc412,
            Symbology::Code32,
            Symbology::Code2of5,
            Symbology::DataLogic2of5,
            Symbology::Iata2of5,
            Symbology::Industrial2of5,
            Symbology::Coop2of5,
            Symbology::Matrix2of5,
            Symbology::Msi,
            Symbology::Plessey,
            Symbology::Posicode,
            Symbology::Telepen,
            Symbology::TelepenNumeric,
            Symbology::Pharmacode,
            Symbology::Pharmacode2,
            Symbology::Flattermarken,
            Symbology::Vin,
            Symbology::Logmars,
            Symbology::Pzn7,
            Symbology::Pzn8,
            Symbology::Ean13,
            Symbology::Ean8,
            Symbology::MarksAndSpencer,
            Symbology::UpcA,
            Symbology::UpcE,
            Symbology::Ean2,
            Symbology::Ean5,
            Symbology::Isbn,
            Symbology::Ismn,
            Symbology::Issn,
            Symbology::Daft,
            Symbology::Kix,
            Symbology::RoyalMail,
            Symbology::Postnet,
            Symbology::Planet,
            Symbology::Identcode,
            Symbology::Leitcode,
            Symbology::Gs1_128,
            Symbology::Sscc18,
            Symbology::Ean14,
            Symbology::UpcCoupon,
            Symbology::Gs1DataMatrix,
            Symbology::Gs1DataMatrixRectangular,
            Symbology::Gs1DlDataMatrix,
            Symbology::Gs1DlQrCode,
            Symbology::Gs1QrCode,
            Symbology::Ntin,
            Symbology::Ppn,
            Symbology::Ean13P2,
            Symbology::Ean13P5,
            Symbology::Ean8P2,
            Symbology::Ean8P5,
            Symbology::UpcAP2,
            Symbology::UpcAP5,
            Symbology::UpcEP2,
            Symbology::UpcEP5,
            Symbology::IsbnP5,
            Symbology::IssnP2,
            Symbology::HibcCode128,
            Symbology::HibcCode39,
            Symbology::HibcDataMatrix,
            Symbology::HibcQrCode,
            Symbology::HibcPdf417,
            Symbology::HibcMicroPdf417,
            Symbology::HibcCodablockF,
            Symbology::HibcAztecCode,
            Symbology::HibcDataMatrixRectangular,
            Symbology::HibcPasCode128,
            Symbology::HibcPasCode39,
            Symbology::HibcPasDataMatrix,
            Symbology::HibcPasQrCode,
            Symbology::HibcPasPdf417,
            Symbology::HibcPasMicroPdf417,
            Symbology::HibcPasCodablockF,
            Symbology::UpuS10,
            Symbology::KoreanPostal,
            Symbology::Cepnet,
            Symbology::ItalianPostal25,
            Symbology::ItalianPostal39,
            Symbology::Dpd,
            Symbology::DpPostmatrix,
            Symbology::Mailmark,
            Symbology::Mailmark2d,
            Symbology::SwissQrCode,
            Symbology::UspsImpb,
            Symbology::UspsOneCode,
            Symbology::JapanPost,
            Symbology::AuspostCustomer,
            Symbology::AuspostReplyPaid,
            Symbology::AuspostRouting,
            Symbology::AuspostRedirection,
            Symbology::DatabarOmni,
            Symbology::DatabarTruncated,
            Symbology::DatabarLimited,
            Symbology::DatabarStacked,
            Symbology::DatabarStackedOmni,
            Symbology::DatabarExpanded,
            Symbology::DatabarExpandedStacked,
            Symbology::Codabar,
            Symbology::Interleaved2of5,
            Symbology::Itf14,
            Symbology::QrCode,
            Symbology::MicroQrCode,
            Symbology::RectangularMicroQrCode,
            Symbology::DataMatrix,
            Symbology::DataMatrixRectangular,
            Symbology::DataMatrixRectangularExtension,
            Symbology::CodablockF,
            Symbology::Pdf417,
            Symbology::Pdf417Truncated,
            Symbology::MicroPdf417,
            Symbology::DotCode,
            Symbology::Gs1DotCode,
            Symbology::Code16k,
            Symbology::Code49,
            Symbology::CodeOne,
            Symbology::Maxicode,
            Symbology::Ultracode,
            Symbology::AztecCode,
            Symbology::AztecCodeCompact,
            Symbology::AztecRune,
            Symbology::ChannelCode,
            Symbology::CompositeDatabarOmniCca,
            Symbology::CompositeDatabarOmniCcb,
            Symbology::CompositeDatabarTruncatedCca,
            Symbology::CompositeDatabarTruncatedCcb,
            Symbology::CompositeDatabarStackedCca,
            Symbology::CompositeDatabarStackedCcb,
            Symbology::CompositeDatabarStackedOmniCca,
            Symbology::CompositeDatabarStackedOmniCcb,
            Symbology::CompositeDatabarExpandedStackedCca,
            Symbology::CompositeDatabarExpandedStackedCcb,
            Symbology::CompositeDatabarLimitedCca,
            Symbology::CompositeDatabarLimitedCcb,
            Symbology::CompositeGs1_128Cca,
            Symbology::CompositeGs1_128Ccb,
            Symbology::CompositeEan13Cca,
            Symbology::CompositeEan13Ccb,
            Symbology::CompositeUpcaCca,
            Symbology::CompositeUpcaCcb,
            Symbology::CompositeEan8Cca,
            Symbology::CompositeEan8Ccb,
            Symbology::CompositeUpceCca,
            Symbology::CompositeUpceCcb,
            Symbology::CompositeDatabarExpandedCca,
            Symbology::CompositeDatabarExpandedCcb,
            Symbology::CompositeGs1_128Ccc,
            Symbology::HanXinCode,
        ]
    }

    /// Human-readable name for UI labels. Distinct from [`Symbology::id`]
    /// (which is the stable machine-friendly string used in the API).
    ///
    /// # Example
    ///
    /// ```
    /// use bwipp::Symbology;
    ///
    /// assert_eq!(Symbology::Code39.display_name(), "Code 39");
    /// assert_eq!(Symbology::QrCode.display_name(), "QR Code");
    /// ```
    pub fn display_name(self) -> &'static str {
        match self {
            Symbology::Code39 => "Code 39",
            Symbology::Code39Ext => "Code 39 Full ASCII",
            Symbology::Code93 => "Code 93",
            Symbology::Code93Ext => "Code 93 Full ASCII",
            Symbology::Code128 => "Code 128",
            Symbology::Code11 => "Code 11",
            Symbology::Bc412 => "BC412",
            Symbology::Code32 => "Code 32 (Italian Pharmacode)",
            Symbology::Code2of5 => "Code 2 of 5 (Standard)",
            Symbology::DataLogic2of5 => "Code 2 of 5 Data Logic",
            Symbology::Iata2of5 => "Code 2 of 5 IATA",
            Symbology::Industrial2of5 => "Code 2 of 5 Industry",
            Symbology::Coop2of5 => "Code 2 of 5 COOP",
            Symbology::Matrix2of5 => "Code 2 of 5 Matrix",
            Symbology::Msi => "MSI (MSI Plessey)",
            Symbology::Plessey => "Plessey",
            Symbology::Posicode => "POSICODE",
            Symbology::Telepen => "Telepen",
            Symbology::TelepenNumeric => "Telepen Numeric",
            Symbology::Pharmacode => "Pharmacode One-Track",
            Symbology::Pharmacode2 => "Pharmacode Two-Track",
            Symbology::Flattermarken => "Flattermarken",
            Symbology::Vin => "VIN",
            Symbology::Logmars => "LOGMARS",
            Symbology::Pzn7 => "PZN7",
            Symbology::Pzn8 => "PZN8",
            Symbology::Ean13 => "EAN-13",
            Symbology::Ean8 => "EAN-8",
            Symbology::MarksAndSpencer => "Marks & Spencer",
            Symbology::UpcA => "UPC-A",
            Symbology::UpcE => "UPC-E",
            Symbology::Ean2 => "EAN-2 add-on",
            Symbology::Ean5 => "EAN-5 add-on",
            Symbology::Isbn => "ISBN-13",
            Symbology::Ismn => "ISMN",
            Symbology::Issn => "ISSN",
            Symbology::Daft => "DAFT",
            Symbology::Kix => "KIX (Dutch postal)",
            Symbology::RoyalMail => "Royal Mail RM4SCC",
            Symbology::Postnet => "USPS PostNet",
            Symbology::Planet => "USPS PLANET",
            Symbology::Identcode => "DP Identcode",
            Symbology::Leitcode => "DP Leitcode",
            Symbology::Gs1_128 => "GS1-128",
            Symbology::Sscc18 => "SSCC-18",
            Symbology::Ean14 => "EAN-14 (GTIN-14)",
            Symbology::UpcCoupon => "UPC Coupon (AI 8110)",
            Symbology::Gs1DataMatrix => "GS1 DataMatrix",
            Symbology::Gs1DataMatrixRectangular => "GS1 DataMatrix (Rectangular)",
            Symbology::Gs1DlDataMatrix => "GS1 Digital Link DataMatrix",
            Symbology::Gs1DlQrCode => "GS1 Digital Link QR Code",
            Symbology::Gs1QrCode => "GS1 QR Code",
            Symbology::Ntin => "NTIN",
            Symbology::Ppn => "PPN",
            Symbology::Ean13P2 => "EAN-13 + 2-digit add-on",
            Symbology::Ean13P5 => "EAN-13 + 5-digit add-on",
            Symbology::Ean8P2 => "EAN-8 + 2-digit add-on",
            Symbology::Ean8P5 => "EAN-8 + 5-digit add-on",
            Symbology::UpcAP2 => "UPC-A + 2-digit add-on",
            Symbology::UpcAP5 => "UPC-A + 5-digit add-on",
            Symbology::UpcEP2 => "UPC-E + 2-digit add-on",
            Symbology::UpcEP5 => "UPC-E + 5-digit add-on",
            Symbology::IsbnP5 => "ISBN-13 + 5-digit add-on",
            Symbology::IssnP2 => "ISSN + 2-digit add-on",
            Symbology::HibcCode128 => "HIBC LIC - Code 128",
            Symbology::HibcCode39 => "HIBC LIC - Code 39",
            Symbology::HibcDataMatrix => "HIBC LIC - Data Matrix",
            Symbology::HibcQrCode => "HIBC LIC - QR Code",
            Symbology::HibcPdf417 => "HIBC LIC - PDF417",
            Symbology::HibcMicroPdf417 => "HIBC LIC - MicroPDF417",
            Symbology::HibcCodablockF => "HIBC LIC - Codablock-F",
            Symbology::HibcAztecCode => "HIBC LIC - Aztec Code",
            Symbology::HibcDataMatrixRectangular => "HIBC LIC - Data Matrix (Rectangular)",
            Symbology::HibcPasCode128 => "HIBC PAS - Code 128",
            Symbology::HibcPasCode39 => "HIBC PAS - Code 39",
            Symbology::HibcPasDataMatrix => "HIBC PAS - Data Matrix",
            Symbology::HibcPasQrCode => "HIBC PAS - QR Code",
            Symbology::HibcPasPdf417 => "HIBC PAS - PDF417",
            Symbology::HibcPasMicroPdf417 => "HIBC PAS - MicroPDF417",
            Symbology::HibcPasCodablockF => "HIBC PAS - Codablock-F",
            Symbology::UpuS10 => "UPU S10",
            Symbology::KoreanPostal => "Korean Postal",
            Symbology::Cepnet => "Brazilian CEPNet",
            Symbology::ItalianPostal25 => "Italian Postal 2 of 5",
            Symbology::ItalianPostal39 => "Italian Postal 3 of 9",
            Symbology::Dpd => "DPD parcel",
            Symbology::DpPostmatrix => "DP Postmatrix",
            Symbology::Mailmark => "Royal Mail Mailmark",
            Symbology::Mailmark2d => "Royal Mail Mailmark 2D",
            Symbology::SwissQrCode => "Swiss QR Code",
            Symbology::UspsImpb => "USPS Intelligent Mail Package",
            Symbology::UspsOneCode => "USPS OneCode (IMb)",
            Symbology::JapanPost => "Japan Post 4-state",
            Symbology::AuspostCustomer => "Australia Post 4-state (Customer)",
            Symbology::AuspostReplyPaid => "Australia Post 4-state (Reply Paid)",
            Symbology::AuspostRouting => "Australia Post 4-state (Routing)",
            Symbology::AuspostRedirection => "Australia Post 4-state (Redirection)",
            Symbology::DatabarOmni => "GS1 DataBar Omnidirectional",
            Symbology::DatabarTruncated => "GS1 DataBar Truncated",
            Symbology::DatabarLimited => "GS1 DataBar Limited",
            Symbology::DatabarStacked => "GS1 DataBar Stacked",
            Symbology::DatabarStackedOmni => "GS1 DataBar Stacked Omnidirectional",
            Symbology::DatabarExpanded => "GS1 DataBar Expanded",
            Symbology::DatabarExpandedStacked => "GS1 DataBar Expanded Stacked",
            Symbology::Codabar => "Codabar",
            Symbology::Interleaved2of5 => "Interleaved 2 of 5",
            Symbology::Itf14 => "ITF-14",
            Symbology::QrCode => "QR Code",
            Symbology::MicroQrCode => "Micro QR Code",
            Symbology::RectangularMicroQrCode => "Rectangular Micro QR Code (rMQR)",
            Symbology::DataMatrix => "Data Matrix",
            Symbology::DataMatrixRectangular => "Data Matrix (Rectangular)",
            Symbology::DataMatrixRectangularExtension => "Data Matrix (Rectangular Extension)",
            Symbology::CodablockF => "Codablock-F",
            Symbology::Pdf417 => "PDF417",
            Symbology::Pdf417Truncated => "PDF417 Truncated",
            Symbology::MicroPdf417 => "MicroPDF417",
            Symbology::DotCode => "DotCode",
            Symbology::Gs1DotCode => "GS1 DotCode",
            Symbology::Code16k => "Code 16K",
            Symbology::Code49 => "Code 49",
            Symbology::CodeOne => "Code One",
            Symbology::Maxicode => "MaxiCode",
            Symbology::Ultracode => "Ultracode",
            Symbology::AztecCode => "Aztec Code",
            Symbology::AztecCodeCompact => "Aztec Code (Compact)",
            Symbology::AztecRune => "Aztec Rune",
            Symbology::ChannelCode => "Channel Code",
            Symbology::CompositeDatabarOmniCca => "GS1 DataBar Omni Composite (CC-A)",
            Symbology::CompositeDatabarOmniCcb => "GS1 DataBar Omni Composite (CC-B)",
            Symbology::CompositeDatabarTruncatedCca => "GS1 DataBar Truncated Composite (CC-A)",
            Symbology::CompositeDatabarTruncatedCcb => "GS1 DataBar Truncated Composite (CC-B)",
            Symbology::CompositeDatabarStackedCca => "GS1 DataBar Stacked Composite (CC-A)",
            Symbology::CompositeDatabarStackedCcb => "GS1 DataBar Stacked Composite (CC-B)",
            Symbology::CompositeDatabarStackedOmniCca => {
                "GS1 DataBar Stacked Omni Composite (CC-A)"
            }
            Symbology::CompositeDatabarStackedOmniCcb => {
                "GS1 DataBar Stacked Omni Composite (CC-B)"
            }
            Symbology::CompositeDatabarExpandedStackedCca => {
                "GS1 DataBar Expanded Stacked Composite (CC-A)"
            }
            Symbology::CompositeDatabarExpandedStackedCcb => {
                "GS1 DataBar Expanded Stacked Composite (CC-B)"
            }
            Symbology::CompositeDatabarLimitedCca => "GS1 DataBar Limited Composite (CC-A)",
            Symbology::CompositeDatabarLimitedCcb => "GS1 DataBar Limited Composite (CC-B)",
            Symbology::CompositeGs1_128Cca => "GS1-128 Composite (CC-A)",
            Symbology::CompositeGs1_128Ccb => "GS1-128 Composite (CC-B)",
            Symbology::CompositeEan13Cca => "EAN-13 Composite (CC-A)",
            Symbology::CompositeEan13Ccb => "EAN-13 Composite (CC-B)",
            Symbology::CompositeUpcaCca => "UPC-A Composite (CC-A)",
            Symbology::CompositeUpcaCcb => "UPC-A Composite (CC-B)",
            Symbology::CompositeEan8Cca => "EAN-8 Composite (CC-A)",
            Symbology::CompositeEan8Ccb => "EAN-8 Composite (CC-B)",
            Symbology::CompositeUpceCca => "UPC-E Composite (CC-A)",
            Symbology::CompositeUpceCcb => "UPC-E Composite (CC-B)",
            Symbology::CompositeDatabarExpandedCca => "GS1 DataBar Expanded Composite (CC-A)",
            Symbology::CompositeDatabarExpandedCcb => "GS1 DataBar Expanded Composite (CC-B)",
            Symbology::CompositeGs1_128Ccc => "GS1-128 Composite (CC-C)",
            Symbology::HanXinCode => "Han Xin Code",
        }
    }

    /// Category grouping for UI dropdowns. Stable strings that group
    /// related symbologies (e.g. "1D - Retail / EAN / UPC", "Postal",
    /// "2D - Matrix", "HIBC (Healthcare)").
    ///
    /// # Example
    ///
    /// ```
    /// use bwipp::Symbology;
    ///
    /// // Group all symbologies by category — useful for building a UI
    /// // dropdown or browsing the catalog.
    /// let mut by_cat: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    /// for sym in Symbology::all() {
    ///     by_cat.entry(sym.category()).or_default().push(sym.id());
    /// }
    /// assert!(by_cat.contains_key("2D - Matrix"));
    /// ```
    pub fn category(self) -> &'static str {
        match self {
            Symbology::Code39
            | Symbology::Code39Ext
            | Symbology::Code93
            | Symbology::Code93Ext
            | Symbology::Code128
            | Symbology::Code11
            | Symbology::Code32 => "1D - Standard",

            Symbology::Code2of5
            | Symbology::DataLogic2of5
            | Symbology::Iata2of5
            | Symbology::Industrial2of5
            | Symbology::Coop2of5
            | Symbology::Matrix2of5
            | Symbology::Interleaved2of5
            | Symbology::Itf14 => "1D - 2 of 5 family",

            Symbology::Ean13
            | Symbology::Ean8
            | Symbology::UpcA
            | Symbology::UpcE
            | Symbology::Ean2
            | Symbology::Ean5
            | Symbology::Ean13P2
            | Symbology::Ean13P5
            | Symbology::Ean8P2
            | Symbology::Ean8P5
            | Symbology::UpcAP2
            | Symbology::UpcAP5
            | Symbology::UpcEP2
            | Symbology::UpcEP5
            | Symbology::Gs1_128
            | Symbology::Sscc18
            | Symbology::Ean14
            | Symbology::MarksAndSpencer
            | Symbology::UpcCoupon => "1D - Retail / EAN / UPC",

            Symbology::Msi
            | Symbology::Plessey
            | Symbology::Posicode
            | Symbology::Telepen
            | Symbology::TelepenNumeric
            | Symbology::Codabar
            | Symbology::Vin
            | Symbology::Logmars
            | Symbology::Flattermarken
            | Symbology::Bc412
            | Symbology::ChannelCode => "1D - Specialized",

            Symbology::Pharmacode | Symbology::Pharmacode2 | Symbology::Pzn7 | Symbology::Pzn8 => {
                "1D - Pharmaceutical"
            }

            Symbology::Isbn
            | Symbology::Ismn
            | Symbology::Issn
            | Symbology::IsbnP5
            | Symbology::IssnP2 => "1D - ISBN / Media",

            Symbology::DatabarOmni
            | Symbology::DatabarTruncated
            | Symbology::DatabarLimited
            | Symbology::DatabarStacked
            | Symbology::DatabarStackedOmni
            | Symbology::DatabarExpanded
            | Symbology::DatabarExpandedStacked => "1D - GS1 DataBar",

            Symbology::Daft
            | Symbology::Kix
            | Symbology::RoyalMail
            | Symbology::Postnet
            | Symbology::Planet
            | Symbology::Identcode
            | Symbology::Leitcode
            | Symbology::JapanPost
            | Symbology::AuspostCustomer
            | Symbology::AuspostReplyPaid
            | Symbology::AuspostRouting
            | Symbology::AuspostRedirection
            | Symbology::UpuS10
            | Symbology::KoreanPostal
            | Symbology::Cepnet
            | Symbology::ItalianPostal25
            | Symbology::ItalianPostal39
            | Symbology::Dpd
            | Symbology::UspsImpb
            | Symbology::UspsOneCode => "Postal",

            Symbology::QrCode
            | Symbology::MicroQrCode
            | Symbology::RectangularMicroQrCode
            | Symbology::DataMatrix
            | Symbology::DataMatrixRectangular
            | Symbology::DataMatrixRectangularExtension
            | Symbology::Gs1DataMatrix
            | Symbology::Gs1DataMatrixRectangular
            | Symbology::Gs1DlDataMatrix
            | Symbology::Gs1DlQrCode
            | Symbology::Gs1QrCode
            | Symbology::SwissQrCode
            | Symbology::Mailmark
            | Symbology::Mailmark2d
            | Symbology::DpPostmatrix
            | Symbology::DotCode
            | Symbology::Gs1DotCode
            | Symbology::Maxicode
            | Symbology::Ultracode
            | Symbology::AztecCode
            | Symbology::AztecCodeCompact
            | Symbology::AztecRune
            | Symbology::HanXinCode
            | Symbology::CodeOne => "2D - Matrix",

            Symbology::CodablockF
            | Symbology::Pdf417
            | Symbology::Pdf417Truncated
            | Symbology::MicroPdf417
            | Symbology::Code16k
            | Symbology::Code49 => "2D - Stacked",

            Symbology::Ntin | Symbology::Ppn => "2D - Specialty",

            Symbology::HibcCode128
            | Symbology::HibcCode39
            | Symbology::HibcDataMatrix
            | Symbology::HibcQrCode
            | Symbology::HibcPdf417
            | Symbology::HibcMicroPdf417
            | Symbology::HibcCodablockF
            | Symbology::HibcAztecCode
            | Symbology::HibcDataMatrixRectangular
            | Symbology::HibcPasCode128
            | Symbology::HibcPasCode39
            | Symbology::HibcPasDataMatrix
            | Symbology::HibcPasQrCode
            | Symbology::HibcPasPdf417
            | Symbology::HibcPasMicroPdf417
            | Symbology::HibcPasCodablockF => "HIBC (Healthcare)",

            Symbology::CompositeDatabarOmniCca
            | Symbology::CompositeDatabarOmniCcb
            | Symbology::CompositeDatabarTruncatedCca
            | Symbology::CompositeDatabarTruncatedCcb
            | Symbology::CompositeDatabarStackedCca
            | Symbology::CompositeDatabarStackedCcb
            | Symbology::CompositeDatabarStackedOmniCca
            | Symbology::CompositeDatabarStackedOmniCcb
            | Symbology::CompositeDatabarExpandedStackedCca
            | Symbology::CompositeDatabarExpandedStackedCcb
            | Symbology::CompositeDatabarLimitedCca
            | Symbology::CompositeDatabarLimitedCcb
            | Symbology::CompositeGs1_128Cca
            | Symbology::CompositeGs1_128Ccb
            | Symbology::CompositeEan13Cca
            | Symbology::CompositeEan13Ccb
            | Symbology::CompositeUpcaCca
            | Symbology::CompositeUpcaCcb
            | Symbology::CompositeEan8Cca
            | Symbology::CompositeEan8Ccb
            | Symbology::CompositeUpceCca
            | Symbology::CompositeUpceCcb
            | Symbology::CompositeDatabarExpandedCca
            | Symbology::CompositeDatabarExpandedCcb
            | Symbology::CompositeGs1_128Ccc => "Composite (Linear + 2D)",
        }
    }

    /// A short, realistic sample payload that is guaranteed to encode for this
    /// symbology. Used by the bundled demo to prefill the input field and by
    /// the integration tests to round-trip every encoder. The strings here are
    /// intentionally compact: long enough to look like real data, short enough
    /// to render legibly at the default scale.
    pub fn default_data(self) -> &'static str {
        match self {
            Symbology::Code39 => "HELLO-123",
            Symbology::Code39Ext => "Hello, world!",
            Symbology::Code93 => "CODE93",
            Symbology::Code93Ext => "Hello, world!",
            Symbology::Code128 => "Hello 128",
            Symbology::Code11 => "0123456789",
            Symbology::Bc412 => "ABC123",
            Symbology::Code32 => "01234567",
            Symbology::Code2of5 => "12345",
            Symbology::DataLogic2of5 => "12345",
            Symbology::Iata2of5 => "12345",
            Symbology::Industrial2of5 => "12345",
            Symbology::Coop2of5 => "12345",
            Symbology::Matrix2of5 => "12345",
            Symbology::Msi => "12345",
            Symbology::Plessey => "DEADBEEF",
            Symbology::Posicode => "HELLO",
            Symbology::Telepen => "Hello",
            Symbology::TelepenNumeric => "123456",
            Symbology::Pharmacode => "117",
            Symbology::Pharmacode2 => "117",
            Symbology::Flattermarken => "1234567",
            Symbology::Vin => "1HGCM82633A123456",
            Symbology::Logmars => "LOGMARS123",
            Symbology::Pzn7 => "123456",
            Symbology::Pzn8 => "1234567",
            Symbology::Ean13 => "0123456789012",
            Symbology::Ean8 => "1234567",
            Symbology::MarksAndSpencer => "12345670",
            Symbology::UpcA => "01234567890",
            Symbology::UpcE => "01234565",
            Symbology::Ean2 => "12",
            Symbology::Ean5 => "12345",
            Symbology::Isbn => "978-1-56619-909-4",
            Symbology::Ismn => "979-0-1234-5678-5",
            Symbology::Issn => "0317-8471",
            Symbology::Daft => "DAFTDAFT",
            Symbology::Kix => "2500GG30",
            Symbology::RoyalMail => "LE28HS9Z",
            Symbology::Postnet => "12345",
            Symbology::Planet => "12345678901",
            Symbology::Identcode => "34567890123",
            Symbology::Leitcode => "1234567890123",
            Symbology::Gs1_128 => "(01)04012345123456",
            Symbology::Sscc18 => "106141411234567897",
            Symbology::Ean14 => "0401234512345",
            Symbology::UpcCoupon => "106141416543213500110000310123196000",
            Symbology::Gs1DataMatrix => "(01)04012345123456",
            Symbology::Gs1DataMatrixRectangular => "(01)04012345123456",
            Symbology::Gs1DlDataMatrix => "https://id.gs1.org/01/04012345123456",
            Symbology::Gs1DlQrCode => "https://id.gs1.org/01/04012345123456",
            Symbology::Gs1QrCode => "(01)04012345123456(17)260101",
            Symbology::Ntin => "00012345678905",
            Symbology::Ppn => "110375286414",
            Symbology::Ean13P2 => "012345678905 12",
            Symbology::Ean13P5 => "012345678905 12345",
            Symbology::Ean8P2 => "1234567 12",
            Symbology::Ean8P5 => "1234567 12345",
            Symbology::UpcAP2 => "01234567890 12",
            Symbology::UpcAP5 => "01234567890 12345",
            Symbology::UpcEP2 => "01234565 12",
            Symbology::UpcEP5 => "01234565 12345",
            Symbology::IsbnP5 => "978-1-56619-909-4 50995",
            Symbology::IssnP2 => "0317-8471 13",
            Symbology::HibcCode128 => "A99912345/52001510X3",
            Symbology::HibcCode39 => "A99912345/52001510X3",
            Symbology::HibcDataMatrix => "A99912345/52001510X3",
            Symbology::HibcQrCode => "A99912345/52001510X3",
            Symbology::HibcPdf417 => "A99912345/52001510X3",
            Symbology::HibcMicroPdf417 => "A99912345/52001510X3",
            Symbology::HibcCodablockF => "A99912345/52001510X3",
            Symbology::HibcAztecCode => "A99912345/52001510X3",
            Symbology::HibcDataMatrixRectangular => "A99912345/52001510X3",
            Symbology::HibcPasCode128 => "A/99912345/$$52001510X3",
            Symbology::HibcPasCode39 => "A/99912345/$$52001510X3",
            Symbology::HibcPasDataMatrix => "A/99912345/$$52001510X3",
            Symbology::HibcPasQrCode => "A/99912345/$$52001510X3",
            Symbology::HibcPasPdf417 => "A/99912345/$$52001510X3",
            Symbology::HibcPasMicroPdf417 => "A/99912345/$$52001510X3",
            Symbology::HibcPasCodablockF => "A/99912345/$$52001510X3",
            Symbology::UpuS10 => "RA123456785US",
            Symbology::KoreanPostal => "123456",
            Symbology::Cepnet => "12345678",
            Symbology::ItalianPostal25 => "12345678",
            Symbology::ItalianPostal39 => "ABCDE",
            Symbology::Dpd => "%000393060781000300000110001020796",
            Symbology::DpPostmatrix => "0123456789012345",
            Symbology::Mailmark => "JGB 012100123412345678AB19XY1A               ",
            Symbology::Mailmark2d => "JGB 012100123412345678AB19XY1A               ",
            Symbology::SwissQrCode => concat!(
                "SPC\n0200\n1\nCH4431999123000889012\nS\nMax Muster\nMustergasse\n",
                "22\n8000\nZuerich\nCH\n\n\n\n\n\n\n\n100.00\nCHF\nS\nSimone Muster\n",
                "Musterstrasse\n1\n8000\nZuerich\nCH\nNON\n\nThank you\nEPD",
            ),
            Symbology::UspsImpb => "(420)94401",
            // Canonical USPS OneCode example: 20-digit tracking ID.
            Symbology::UspsOneCode => "01234567094987654321",
            Symbology::JapanPost => "123-4567-890",
            Symbology::AuspostCustomer => "12345678",
            Symbology::AuspostReplyPaid => "12345678",
            // Routing has a 5-char character-mode custinfo capacity.
            Symbology::AuspostRouting => "12345678ABCDE",
            // Redirection has a 10-char character-mode custinfo capacity.
            Symbology::AuspostRedirection => "12345678ABCDEFGHIJ",
            // DataBar default-data examples — every variant is a
            // verified GS1 DataBar encoder (Omni/Truncated/Limited/
            // Stacked/StackedOmni/Expanded/ExpandedStacked); the
            // (01)…GTIN-14 payloads here exercise the standard
            // omni-method/expanded-method dispatch paths.
            Symbology::DatabarOmni => "(01)24012345678905",
            Symbology::DatabarTruncated => "(01)24012345678905",
            // Limited requires the GTIN to start with 0 or 1; the Omni
            // sample starts with 2 so we use a different one here.
            Symbology::DatabarLimited => "(01)15012345678907",
            Symbology::DatabarStacked => "(01)24012345678905",
            Symbology::DatabarStackedOmni => "(01)24012345678905",
            Symbology::DatabarExpanded => "(01)90012345678908",
            Symbology::DatabarExpandedStacked => "(01)90012345678908",
            Symbology::Codabar => "A12345B",
            Symbology::Interleaved2of5 => "12345678",
            Symbology::Itf14 => "1234567890123",
            Symbology::QrCode => "https://example.com",
            // Micro QR has very tight payload limits (M4 byte-mode ≈
            // 11 bytes); a short string fits cleanly in M3/M4.
            Symbology::MicroQrCode => "HELLO QR",
            // rMQR R7x43 M defaults; "HELLO" fits comfortably and
            // produces a 7×43 symbol.
            Symbology::RectangularMicroQrCode => "HELLO",
            Symbology::DataMatrix => "hello",
            Symbology::DataMatrixRectangular => "hello",
            Symbology::DataMatrixRectangularExtension => "ABABABABABABABABABABABABABABABABABABABAB",
            Symbology::CodablockF => "Hello, Codablock-F!",
            Symbology::Pdf417 => "https://github.com/erdzan12/bwipp-rs",
            // Same payload as Pdf417 — exercises the same compaction
            // path but renders to a noticeably narrower bitmap so the
            // truncated variant is visually distinguishable in the
            // demo gallery.
            Symbology::Pdf417Truncated => "https://github.com/erdzan12/bwipp-rs",
            // MicroPDF417 is much lower-capacity than PDF417; the
            // shortest demo string that still rounds out to a non-
            // trivial 2-D shape (1×17, k=7) is "Hello, World!".
            Symbology::MicroPdf417 => "Hello, World!",
            // DotCode handles any byte string; pick something short
            // so the default 10×13 symbol shows up at default scale.
            Symbology::DotCode => "Hello",
            Symbology::Gs1DotCode => "(01)04012345123456",
            Symbology::Code16k => "12345",
            Symbology::Code49 => "12345",
            Symbology::CodeOne => "Hello",
            Symbology::Maxicode => "MAXICODE",
            Symbology::Ultracode => "HELLO",
            Symbology::AztecCode => "HELLO",
            Symbology::AztecCodeCompact => "HELLO",
            Symbology::AztecRune => "42",
            Symbology::ChannelCode => "12",
            Symbology::CompositeDatabarOmniCca => "(01)24012345678905|(99)1234567",
            // CC-B carries more bits than CC-A, so the default sample is
            // a multi-AI payload that gs1_cc auto-routes to CC-B.
            Symbology::CompositeDatabarOmniCcb => {
                "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC"
            }
            Symbology::CompositeDatabarTruncatedCca => "(01)24012345678905|(99)1234567",
            Symbology::CompositeDatabarTruncatedCcb => {
                "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC"
            }
            Symbology::CompositeDatabarStackedCca => "(01)24012345678905|(99)1234567",
            Symbology::CompositeDatabarStackedCcb => {
                "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC"
            }
            Symbology::CompositeDatabarStackedOmniCca => "(01)24012345678905|(99)1234567",
            Symbology::CompositeDatabarStackedOmniCcb => {
                "(01)24012345678905|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC"
            }
            Symbology::CompositeDatabarExpandedStackedCca => {
                "(01)90012345678908(3103)001750|(99)1234567"
            }
            Symbology::CompositeDatabarExpandedStackedCcb => {
                "(01)90012345678908(3103)001750|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC"
            }
            Symbology::CompositeDatabarLimitedCca => "(01)15012345678907|(99)1234567",
            Symbology::CompositeDatabarLimitedCcb => {
                "(01)15012345678907|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC"
            }
            Symbology::CompositeGs1_128Cca => "(01)04012345123456|(99)1234567",
            // CC-B variant uses a longer payload to exercise auto-promotion;
            // the encoder accepts CC-A-sized inputs too via cc.version dispatch.
            Symbology::CompositeGs1_128Ccb => {
                "(01)04012345123456|(10)BATCH(21)SERIAL1234567(91)EXTRADATAFORCC"
            }
            Symbology::CompositeEan13Cca => "5901234123457|(99)1234567",
            Symbology::CompositeEan13Ccb => "5901234123457|(99)1234567",
            Symbology::CompositeUpcaCca => "012345678905|(99)1234567",
            Symbology::CompositeUpcaCcb => "012345678905|(99)1234567",
            Symbology::CompositeEan8Cca => "12345670|(99)1234567",
            Symbology::CompositeEan8Ccb => "12345670|(99)1234567",
            Symbology::CompositeUpceCca => "0123456|(99)1234567",
            Symbology::CompositeUpceCcb => "0123456|(99)1234567",
            Symbology::CompositeDatabarExpandedCca => "(01)90012345678908(3103)001750|(99)1234567",
            Symbology::CompositeDatabarExpandedCcb => "(01)90012345678908(3103)001750|(99)1234567",
            Symbology::CompositeGs1_128Ccc => "(01)04012345123456|(99)1234567",
            Symbology::HanXinCode => "HELLO",
        }
    }

    /// Symbology-specific default option overrides, in the form expected
    /// by [`crate::Options::extras`]: `&[(key, value)]` pairs that the
    /// caller may merge into their own `Options` before rendering. Returns
    /// an empty slice for symbologies that don't need any extras.
    ///
    /// The main use case is the bundled WASM demo, which calls this once
    /// per symbology to learn that e.g. Codablock-F wants `columns=8`.
    pub fn default_extras(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Symbology::CodablockF => &[("columns", "8")],
            Symbology::Pdf417 => &[("eclevel", "2")],
            _ => &[],
        }
    }
}
