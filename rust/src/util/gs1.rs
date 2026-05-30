//! GS1 Application Identifier (AI) parser.
//!
//! GS1 element strings are conventionally written with the AI in parentheses
//! followed by its data, e.g. `(01)04012345123456(17)260101`. The parser
//! splits such a string into `(ai, data)` pairs, validates each AI's length
//! against the GS1 General Specifications, and produces the **flat
//! representation** symbologies need: a byte sequence in which variable-
//! length data values are terminated with FNC1 (`0x1D`) when followed by
//! another element.
//!
//! This module is the cross-cutting foundation for GS1-128, GS1 DataMatrix,
//! GS1 QR Code, GS1 DataBar composites, NTIN, PPN, UPC Coupon, and USPS
//! IMpb. Each of those callers stays light: parse with [`parse`], emit the
//! resulting bytes via the symbology-specific channel.
//!
//! Reference: GS1 General Specifications (current edition) and BWIPP's
//! `gs1process.ps.src`. The AI table here covers the AIs we need today and
//! is extended on demand.

#![allow(dead_code)]

/// FNC1 byte. Inserted at the start of every GS1 message and after any
/// variable-length AI that has a following element.
pub const FNC1: u8 = 0x1D;

/// One element of a parsed GS1 message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// Application Identifier (numeric string, 2–4 digits).
    pub ai: String,
    /// Element data exactly as supplied (digits / characters, no separator).
    pub data: String,
}

/// Error variants returned by [`parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Expected `(` at this position.
    MissingOpenParen { position: usize },
    /// Expected `)` to close the AI.
    UnclosedAi { position: usize },
    /// AI digits weren't 2–4 long or contained a non-digit.
    InvalidAi { ai: String },
    /// AI is unknown (not in our table).
    UnknownAi { ai: String },
    /// Element data length doesn't fit the AI's permitted lengths.
    BadLength {
        ai: String,
        expected: String,
        got: usize,
    },
    /// Element data contains a character the AI's format doesn't allow.
    BadCharacter { ai: String, ch: char },
    /// Input was empty.
    Empty,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MissingOpenParen { position } => {
                write!(f, "GS1 parse: expected '(' at position {position}")
            }
            ParseError::UnclosedAi { position } => {
                write!(f, "GS1 parse: unclosed AI starting at position {position}")
            }
            ParseError::InvalidAi { ai } => {
                write!(f, "GS1 parse: invalid AI {ai:?} (must be 2-4 digits)")
            }
            ParseError::UnknownAi { ai } => {
                write!(f, "GS1 parse: unknown AI {ai:?}")
            }
            ParseError::BadLength { ai, expected, got } => write!(
                f,
                "GS1 parse: AI ({ai}) requires data length {expected}, got {got}"
            ),
            ParseError::BadCharacter { ai, ch } => {
                write!(f, "GS1 parse: AI ({ai}) does not allow character {ch:?}")
            }
            ParseError::Empty => write!(f, "GS1 parse: input is empty"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a GS1 element string of the form `(NN)data(NN)data...` into a
/// vector of [`Element`]s. Returns an error if any AI is malformed or its
/// data violates the AI's length / character rules.
pub fn parse(input: &str) -> Result<Vec<Element>, ParseError> {
    if input.is_empty() {
        return Err(ParseError::Empty);
    }
    let bytes = input.as_bytes();
    let mut pos = 0;
    let mut out: Vec<Element> = Vec::new();

    while pos < bytes.len() {
        if bytes[pos] != b'(' {
            return Err(ParseError::MissingOpenParen { position: pos });
        }
        let ai_start = pos + 1;
        let close = bytes[ai_start..]
            .iter()
            .position(|&b| b == b')')
            .map(|i| ai_start + i)
            .ok_or(ParseError::UnclosedAi { position: pos })?;
        let ai = &input[ai_start..close];
        if !(2..=4).contains(&ai.len()) || !ai.chars().all(|c| c.is_ascii_digit()) {
            return Err(ParseError::InvalidAi { ai: ai.to_string() });
        }
        // Data ends at the next '(' or end-of-string.
        let data_start = close + 1;
        let data_end = bytes[data_start..]
            .iter()
            .position(|&b| b == b'(')
            .map(|i| data_start + i)
            .unwrap_or(bytes.len());
        let data = &input[data_start..data_end];

        let spec = AI_TABLE
            .iter()
            .find(|s| s.ai == ai)
            .ok_or_else(|| ParseError::UnknownAi { ai: ai.to_string() })?;
        spec.validate_data(data)?;

        out.push(Element {
            ai: ai.to_string(),
            data: data.to_string(),
        });
        pos = data_end;
    }
    Ok(out)
}

/// Encode a list of [`Element`]s into the byte stream a GS1 symbology
/// expects: leading FNC1, then for each element the AI digits followed by
/// the data; if the AI is variable-length and a later element follows, an
/// FNC1 separator is inserted after the data.
pub fn encode_with_fnc1(elements: &[Element]) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        elements
            .iter()
            .map(|e| e.ai.len() + e.data.len())
            .sum::<usize>()
            + elements.len()
            + 1,
    );
    out.push(FNC1);
    for (i, e) in elements.iter().enumerate() {
        out.extend_from_slice(e.ai.as_bytes());
        out.extend_from_slice(e.data.as_bytes());
        let spec = AI_TABLE
            .iter()
            .find(|s| s.ai == e.ai)
            .expect("element AI not in table");
        if spec.variable && i + 1 < elements.len() {
            out.push(FNC1);
        }
    }
    out
}

/// Convenience: parse a GS1 element string and immediately convert to the
/// flat FNC1-separated byte stream.
pub fn parse_and_encode(input: &str) -> Result<Vec<u8>, ParseError> {
    Ok(encode_with_fnc1(&parse(input)?))
}

/// GS1 Digital Link URI parser (light-validation flavour).
///
/// Per GS1 General Specifications §7.2, a GS1 DL URI looks like:
///
///   `https://<authority>/<path-AIs>?<query-AIs>`
///
/// where the path contains alternating `/AI/value` pairs and the optional
/// query string contains additional `AI=value` parameters. This parser
/// implements **light validation**: it confirms the URI shape, walks at
/// least one path-segment AI, and accepts the rest. It does **not** lint
/// the AI classification (primary key vs qualifier vs attribute) or the
/// per-AI value content — bwipp's `gs1process('dl')` does both, but for
/// catalog use a permissive parser is enough.
///
/// Returns the parsed AI elements (in path order, then query order) and
/// the **original URI string** that callers like `gs1dldatamatrix` /
/// `gs1dlqrcode` encode into the resulting symbol. Errors when the URI
/// doesn't have an `http(s)://` scheme, no path AIs are found, or an AI
/// fails the basic 2-4-digit check.
pub fn parse_dl_uri(uri: &str) -> Result<Vec<Element>, DlUriError> {
    let rest = uri
        .strip_prefix("https://")
        .or_else(|| uri.strip_prefix("http://"))
        .ok_or(DlUriError::BadScheme)?;

    // Skip the authority (everything up to the first `/`).
    let path_and_query = rest
        .split_once('/')
        .map(|x| x.1)
        .ok_or(DlUriError::NoPath)?;

    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path_and_query, None),
    };

    let mut elements: Vec<Element> = Vec::new();

    // Walk path segments in pairs of (AI, value). The path may have an
    // optional version prefix (e.g. `/01/<value>` or `/gtin/<value>`)
    // — for the value-only form we only support numeric AIs (2-4
    // digits). Per the GS1 DL spec the path always alternates
    // `/AI/value`; we stop reading once a segment isn't a valid AI.
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut i = 0;
    while i + 1 < segments.len() {
        let ai = segments[i];
        if !is_valid_ai(ai) {
            // First non-AI segment (could be a "convenience" alpha key
            // like `/gtin/...` which we don't implement). Stop walking.
            break;
        }
        let value = url_percent_decode(segments[i + 1]);
        elements.push(Element {
            ai: ai.to_string(),
            data: value,
        });
        i += 2;
    }

    if elements.is_empty() {
        return Err(DlUriError::NoAiInPath);
    }

    if let Some(q) = query {
        for kv in q.split('&').filter(|s| !s.is_empty()) {
            let (k, v) = kv
                .split_once('=')
                .ok_or_else(|| DlUriError::BadQueryParam {
                    param: kv.to_string(),
                })?;
            if !is_valid_ai(k) {
                // Non-AI query params are technically allowed by GS1 DL
                // (anything that isn't a registered AI is opaque
                // metadata). Skip silently.
                continue;
            }
            elements.push(Element {
                ai: k.to_string(),
                data: url_percent_decode(v),
            });
        }
    }

    Ok(elements)
}

/// Error variants from [`parse_dl_uri`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DlUriError {
    /// URI does not start with `http://` or `https://`.
    BadScheme,
    /// URI authority is not followed by a path.
    NoPath,
    /// No GS1 AI path segments were found.
    NoAiInPath,
    /// Query param doesn't have an `AI=value` shape.
    BadQueryParam { param: String },
}

impl std::fmt::Display for DlUriError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DlUriError::BadScheme => write!(f, "GS1 DL URI: must start with http:// or https://"),
            DlUriError::NoPath => write!(f, "GS1 DL URI: missing path after authority"),
            DlUriError::NoAiInPath => write!(f, "GS1 DL URI: no AI segments found in path"),
            DlUriError::BadQueryParam { param } => {
                write!(
                    f,
                    "GS1 DL URI: bad query param {param:?} (expected `key=value`)"
                )
            }
        }
    }
}

impl std::error::Error for DlUriError {}

fn is_valid_ai(s: &str) -> bool {
    matches!(s.len(), 2..=4) && s.bytes().all(|b| b.is_ascii_digit())
}

/// Minimal percent-decoder. Real GS1 DL URIs may percent-encode
/// reserved characters in their values; decode them so the parsed
/// element data matches what bwip-js sees after URI normalisation.
fn url_percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Whether AI `ai` is variable-length per the table. Variable AIs
/// need an FNC1 separator after their data when another element
/// follows. Returns `None` if the AI isn't in the table.
pub fn ai_is_variable_length(ai: &str) -> Option<bool> {
    AI_TABLE.iter().find(|s| s.ai == ai).map(|s| s.variable)
}

// ---------------------------------------------------------------------------
// AI table
// ---------------------------------------------------------------------------

/// Format kind for an AI's data field.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DataFormat {
    /// Decimal digits only.
    Numeric,
    /// GS1 AI 82 character set: digits, A-Z, lowercase letters, and some
    /// punctuation. (Approximate — `validate_data` accepts a broad set.)
    Alphanumeric,
}

struct AiSpec {
    ai: &'static str,
    /// Minimum data length (inclusive).
    min_len: usize,
    /// Maximum data length (inclusive). `None` = variable-length AI.
    max_len: Option<usize>,
    /// Whether the AI's data is variable-length (`true` means FNC1 needed
    /// after it when more elements follow).
    variable: bool,
    /// Allowed character set.
    format: DataFormat,
}

impl AiSpec {
    fn validate_data(&self, data: &str) -> Result<(), ParseError> {
        let n = data.chars().count();
        let expected = match self.max_len {
            Some(max) if max == self.min_len => format!("{}", self.min_len),
            Some(max) => format!("{}-{}", self.min_len, max),
            None => format!("{}+", self.min_len),
        };
        match self.max_len {
            Some(max) if n < self.min_len || n > max => {
                return Err(ParseError::BadLength {
                    ai: self.ai.to_string(),
                    expected,
                    got: n,
                });
            }
            None if n < self.min_len => {
                return Err(ParseError::BadLength {
                    ai: self.ai.to_string(),
                    expected,
                    got: n,
                });
            }
            _ => {}
        }
        for ch in data.chars() {
            match self.format {
                DataFormat::Numeric => {
                    if !ch.is_ascii_digit() {
                        return Err(ParseError::BadCharacter {
                            ai: self.ai.to_string(),
                            ch,
                        });
                    }
                }
                DataFormat::Alphanumeric => {
                    if !ch.is_ascii_graphic() {
                        return Err(ParseError::BadCharacter {
                            ai: self.ai.to_string(),
                            ch,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

const N_FIXED: &[AiSpec] = &[];

/// The AI catalog. Limited to entries we actually need to encode today;
/// extend on demand. Fixed-length AIs have `variable: false` and the same
/// `min_len`/`max_len`; variable-length AIs set `variable: true` and a
/// reasonable upper bound (max 30 for most data fields).
const AI_TABLE: &[AiSpec] = &[
    // SSCC (Serial Shipping Container Code) — 18 digits.
    AiSpec {
        ai: "00",
        min_len: 18,
        max_len: Some(18),
        variable: false,
        format: DataFormat::Numeric,
    },
    // GTIN — 14 digits.
    AiSpec {
        ai: "01",
        min_len: 14,
        max_len: Some(14),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Variable measure trade item GTIN — 14 digits.
    AiSpec {
        ai: "02",
        min_len: 14,
        max_len: Some(14),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Batch / lot — up to 20 alphanumeric.
    AiSpec {
        ai: "10",
        min_len: 1,
        max_len: Some(20),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    // Production date YYMMDD.
    AiSpec {
        ai: "11",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Due date.
    AiSpec {
        ai: "12",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Packaging date.
    AiSpec {
        ai: "13",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Best-before date.
    AiSpec {
        ai: "15",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Sell-by date.
    AiSpec {
        ai: "16",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Expiration date.
    AiSpec {
        ai: "17",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Internal product variant (e.g. weight class) — 2 digits.
    AiSpec {
        ai: "20",
        min_len: 2,
        max_len: Some(2),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Serial number — up to 20 alphanumeric.
    AiSpec {
        ai: "21",
        min_len: 1,
        max_len: Some(20),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    // Additional product identification — alphanumeric, variable.
    AiSpec {
        ai: "240",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "241",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    // Variable count — up to 8 digits.
    AiSpec {
        ai: "30",
        min_len: 1,
        max_len: Some(8),
        variable: true,
        format: DataFormat::Numeric,
    },
    // Net weight, kilograms — 3 + 6 digits.
    AiSpec {
        ai: "3100",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3101",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3102",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3103",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3104",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3105",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Length / first dimension, metric (32xx) — 6 digits, last digit
    // of the AI is the implicit decimal-point position.
    AiSpec {
        ai: "3200",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3201",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3202",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3203",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3204",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3205",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Amount payable - single monetary area, up to 15 digits.
    // Last digit of AI is the implicit decimal place.
    AiSpec {
        ai: "3920",
        min_len: 1,
        max_len: Some(15),
        variable: true,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3921",
        min_len: 1,
        max_len: Some(15),
        variable: true,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3922",
        min_len: 1,
        max_len: Some(15),
        variable: true,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3923",
        min_len: 1,
        max_len: Some(15),
        variable: true,
        format: DataFormat::Numeric,
    },
    // Amount payable - with ISO currency, 3-digit currency code +
    // up to 15 digits of value.
    AiSpec {
        ai: "3930",
        min_len: 4,
        max_len: Some(18),
        variable: true,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3931",
        min_len: 4,
        max_len: Some(18),
        variable: true,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3932",
        min_len: 4,
        max_len: Some(18),
        variable: true,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "3933",
        min_len: 4,
        max_len: Some(18),
        variable: true,
        format: DataFormat::Numeric,
    },
    // Customer purchase order number — up to 30 alphanumeric.
    AiSpec {
        ai: "400",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    // Ship-to / Bill-to location code (SSCC) — 13 digits.
    AiSpec {
        ai: "410",
        min_len: 13,
        max_len: Some(13),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "411",
        min_len: 13,
        max_len: Some(13),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "412",
        min_len: 13,
        max_len: Some(13),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "413",
        min_len: 13,
        max_len: Some(13),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "414",
        min_len: 13,
        max_len: Some(13),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "415",
        min_len: 13,
        max_len: Some(13),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "420",
        min_len: 1,
        max_len: Some(20),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "421",
        min_len: 4,
        max_len: Some(12),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    // NDC / NTIN — 14 digits.
    AiSpec {
        ai: "8003",
        min_len: 14,
        max_len: Some(14),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "8004",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "8005",
        min_len: 6,
        max_len: Some(6),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "8006",
        min_len: 18,
        max_len: Some(18),
        variable: false,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "8007",
        min_len: 1,
        max_len: Some(34),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "8008",
        min_len: 8,
        max_len: Some(12),
        variable: true,
        format: DataFormat::Numeric,
    },
    AiSpec {
        ai: "8018",
        min_len: 18,
        max_len: Some(18),
        variable: false,
        format: DataFormat::Numeric,
    },
    // Coupon Code (NA).
    AiSpec {
        ai: "8110",
        min_len: 1,
        max_len: Some(70),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    // Free-text mutual agreement zones.
    AiSpec {
        ai: "90",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "91",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "92",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "93",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "94",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "95",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "96",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "97",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "98",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
    AiSpec {
        ai: "99",
        min_len: 1,
        max_len: Some(30),
        variable: true,
        format: DataFormat::Alphanumeric,
    },
];

// Silence the unused warning for the empty `N_FIXED` table.
const _: &[AiSpec] = N_FIXED;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_fixed_length_ai() {
        let v = parse("(01)04012345123456").unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].ai, "01");
        assert_eq!(v[0].data, "04012345123456");
    }

    #[test]
    fn parses_multiple_fixed_length_ais() {
        let v = parse("(01)04012345123456(17)260101(10)A1B2").unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[2].ai, "10");
        assert_eq!(v[2].data, "A1B2");
    }

    #[test]
    fn encode_inserts_fnc1_after_variable_only_when_followed() {
        let v = parse("(10)A1B2(01)04012345123456").unwrap();
        let bytes = encode_with_fnc1(&v);
        // Leading FNC1 + "10" + "A1B2" + FNC1 + "01" + "04012345123456"
        assert_eq!(bytes[0], FNC1);
        // Find the second FNC1: should be at position 1 + 2 + 4 = 7
        let second_fnc1_pos = 1 + 2 + 4;
        assert_eq!(bytes[second_fnc1_pos], FNC1);
        // Length matches: 1 + 2 + 4 + 1 + 2 + 14 = 24
        assert_eq!(bytes.len(), 24);
    }

    #[test]
    fn encode_omits_trailing_fnc1_after_variable_at_end() {
        let v = parse("(01)04012345123456(10)A1B2").unwrap();
        let bytes = encode_with_fnc1(&v);
        // The (10) variable element is last, so no FNC1 after it.
        assert_eq!(bytes.last().copied().unwrap(), b'2');
    }

    #[test]
    fn rejects_missing_open_paren() {
        // Stage 11.A8c (cont) — upgrade variant-only `matches!`
        // pattern to bind the position field and pin its value. Kills
        // mutations that route a different rejection through the
        // MissingOpenParen variant with a wrong position value.
        match parse("01)1234567890123") {
            Err(ParseError::MissingOpenParen { position }) => {
                assert_eq!(
                    position, 0,
                    "missing-paren should report position 0 (input starts with '0')"
                );
            }
            other => panic!("expected MissingOpenParen at position 0, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unclosed_ai() {
        // Stage 11.A8c (cont) — bind position field. The '(' at idx 0
        // never closes; UnclosedAi.position must echo the open-paren
        // position (0).
        match parse("(01") {
            Err(ParseError::UnclosedAi { position }) => {
                assert_eq!(
                    position, 0,
                    "unclosed-AI should report position 0 (the open-paren idx)"
                );
            }
            other => panic!("expected UnclosedAi at position 0, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_ai() {
        // Stage 11.A8c (cont) — bind ai field to pin which AI value
        // triggered InvalidAi. Two arms exercise both the over-length
        // (5 chars) and under-length (1 char) reject branches of the
        // 2-4 digit predicate.
        match parse("(99999)X") {
            Err(ParseError::InvalidAi { ai }) => {
                assert_eq!(
                    ai, "99999",
                    "5-digit AI must surface full \"99999\" value (kills `{{ai}}` interpolation drop)"
                );
            }
            other => panic!("expected InvalidAi {{ai: \"99999\"}}, got {other:?}"),
        }
        match parse("(0)X") {
            Err(ParseError::InvalidAi { ai }) => {
                assert_eq!(
                    ai, "0",
                    "1-digit AI must surface \"0\" (kills under-length branch mutations)"
                );
            }
            other => panic!("expected InvalidAi {{ai: \"0\"}}, got {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_length() {
        // Stage 11.A8c (cont) — bind ai + expected + got fields. AI
        // (01) wants 14 digits; supply 13. Pinning all three fields
        // kills mutations on any of the three interpolations.
        match parse("(01)0401234512345") {
            Err(ParseError::BadLength { ai, expected, got }) => {
                assert_eq!(ai, "01", "ai must be \"01\"");
                assert_eq!(expected, "14", "expected length must be \"14\"");
                assert_eq!(got, 13, "got length must be 13");
            }
            other => panic!("expected BadLength, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_digit_in_numeric_ai() {
        // Stage 11.A8c (cont) — bind ai + ch fields. AI (01) is
        // numeric-only; the first non-digit ('A' at idx 10) triggers
        // BadCharacter.
        match parse("(01)040123ABCD3456") {
            Err(ParseError::BadCharacter { ai, ch }) => {
                assert_eq!(ai, "01", "ai must be \"01\"");
                assert_eq!(
                    ch, 'A',
                    "ch must be 'A' (first non-digit after the 6-char numeric prefix)"
                );
            }
            other => panic!("expected BadCharacter {{ai: \"01\", ch: 'A'}}, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_encode_round_trip() {
        let bytes = parse_and_encode("(01)04012345123456(17)260101").unwrap();
        // Just sanity-check length and the leading FNC1.
        assert_eq!(bytes[0], FNC1);
        // Both AIs are fixed-length numeric so no embedded FNC1.
        let occurrences = bytes.iter().filter(|&&b| b == FNC1).count();
        assert_eq!(occurrences, 1);
    }

    /// Stage 11.A8c — pin `encode_with_fnc1` directly. The existing
    /// `parse_and_encode_round_trip` only verifies leading FNC1 + count
    /// for two fixed AIs; it doesn't exercise the variable-AI separator
    /// branch (`spec.variable && i + 1 < elements.len()`) or the
    /// no-separator-on-last-variable case. Mutations to catch:
    ///   - `out.push(FNC1)` leading removed: byte[0] would no longer
    ///     be the FNC1 sentinel.
    ///   - `spec.variable && i + 1 < elements.len()` → `||`: would add
    ///     a separator after every fixed AI that isn't last (false
    ///     positives).
    ///   - `i + 1 < elements.len()` → `<=`: would add a separator
    ///     after the last variable AI (trailing FNC1).
    ///   - `i + 1` → `i`: shifts the separator one element early.
    #[test]
    fn encode_with_fnc1_handles_variable_separator() {
        // Empty: just the leading FNC1 sentinel.
        let bytes = encode_with_fnc1(&[]);
        assert_eq!(bytes, vec![FNC1], "empty elements list → only leading FNC1");

        // Single fixed AI (01 = GTIN, fixed 14 digits): no separator.
        let fixed_only = vec![Element {
            ai: "01".to_string(),
            data: "04012345123456".to_string(),
        }];
        let bytes = encode_with_fnc1(&fixed_only);
        assert_eq!(bytes[0], FNC1);
        assert_eq!(
            bytes.iter().filter(|&&b| b == FNC1).count(),
            1,
            "single fixed AI: only the leading FNC1"
        );
        assert_eq!(&bytes[1..3], b"01");
        assert_eq!(&bytes[3..], b"04012345123456");

        // Single variable AI alone: NO trailing separator (no next
        // element). Catches `i + 1 < elements.len()` → `<=` mutation.
        let var_only = vec![Element {
            ai: "10".to_string(),
            data: "ABC".to_string(),
        }];
        let bytes = encode_with_fnc1(&var_only);
        assert_eq!(
            bytes.iter().filter(|&&b| b == FNC1).count(),
            1,
            "single variable AI must NOT add a trailing FNC1 (no next element)"
        );
        assert_eq!(&bytes[1..], b"10ABC");

        // Two fixed AIs (01 + 17): no embedded separator.
        let two_fixed = vec![
            Element {
                ai: "01".to_string(),
                data: "04012345123456".to_string(),
            },
            Element {
                ai: "17".to_string(),
                data: "260101".to_string(),
            },
        ];
        let bytes = encode_with_fnc1(&two_fixed);
        assert_eq!(
            bytes.iter().filter(|&&b| b == FNC1).count(),
            1,
            "two fixed AIs: still only the leading FNC1"
        );

        // Variable + fixed: FNC1 separator AFTER the variable data,
        // BEFORE the next AI.
        let var_then_fixed = vec![
            Element {
                ai: "10".to_string(),
                data: "ABC".to_string(),
            },
            Element {
                ai: "01".to_string(),
                data: "04012345123456".to_string(),
            },
        ];
        let bytes = encode_with_fnc1(&var_then_fixed);
        assert_eq!(
            bytes.iter().filter(|&&b| b == FNC1).count(),
            2,
            "variable AI followed by another element MUST emit a separator"
        );
        // Expected layout: FNC1, "10ABC", FNC1, "01...".
        assert_eq!(bytes[0], FNC1);
        assert_eq!(&bytes[1..6], b"10ABC");
        assert_eq!(bytes[6], FNC1);
        assert_eq!(&bytes[7..], b"0104012345123456");

        // Fixed + variable: variable is last → no trailing separator.
        let fixed_then_var = vec![
            Element {
                ai: "01".to_string(),
                data: "04012345123456".to_string(),
            },
            Element {
                ai: "10".to_string(),
                data: "ABC".to_string(),
            },
        ];
        let bytes = encode_with_fnc1(&fixed_then_var);
        assert_eq!(
            bytes.iter().filter(|&&b| b == FNC1).count(),
            1,
            "trailing variable AI must NOT emit a separator (no next element)"
        );
        // Expected: FNC1, "01" + 14d, "10ABC".
        let tail = &bytes[bytes.len() - 5..];
        assert_eq!(tail, b"10ABC", "variable AI tail uninterrupted");
    }

    #[test]
    fn parse_dl_uri_extracts_path_ais() {
        let v = parse_dl_uri("https://id.gs1.org/01/04012345123456").unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].ai, "01");
        assert_eq!(v[0].data, "04012345123456");
    }

    #[test]
    fn parse_dl_uri_extracts_multiple_path_ais() {
        let v = parse_dl_uri("https://id.gs1.org/01/04012345123456/21/SERIAL123").unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].ai, "01");
        assert_eq!(v[1].ai, "21");
        assert_eq!(v[1].data, "SERIAL123");
    }

    #[test]
    fn parse_dl_uri_extracts_query_ais() {
        let v = parse_dl_uri("https://x.example/01/04012345123456?17=251231&10=ABC").unwrap();
        // Path: AI 01. Query: AI 17, AI 10.
        assert_eq!(v.len(), 3);
        assert_eq!(v[1].ai, "17");
        assert_eq!(v[1].data, "251231");
        assert_eq!(v[2].ai, "10");
        assert_eq!(v[2].data, "ABC");
    }

    #[test]
    fn parse_dl_uri_decodes_percent_escapes() {
        let v = parse_dl_uri("https://x.example/01/04012345123456?10=A%2FB").unwrap();
        assert_eq!(v[1].data, "A/B");
    }

    #[test]
    fn parse_dl_uri_rejects_bad_scheme() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to
        // assert_eq!(..., Err(DlUriError::BadScheme)) form. The
        // DlUriError variants are all unit / no-payload (BadScheme,
        // NoPath, NoAiInPath, BadQueryParam{param}). Using
        // assert_eq! surfaces the actual variant on regression —
        // e.g. if a mutation routes `ftp://` through the NoPath arm
        // instead of BadScheme, the failure message would show
        // `Err(NoPath)` rather than just `assertion failed`.
        assert_eq!(
            parse_dl_uri("ftp://x.example/01/04012345123456"),
            Err(DlUriError::BadScheme),
            "ftp:// scheme must reject as BadScheme — kills variant-swap mutations between BadScheme / NoPath / NoAiInPath"
        );
    }

    #[test]
    fn parse_dl_uri_rejects_no_ai() {
        // Stage 11.A8c — assert_eq! form with explicit variant
        // (sibling of parse_dl_uri_rejects_bad_scheme). The
        // `https://example.com/foo` input has a valid scheme +
        // path, so the only valid rejection is NoAiInPath (the
        // path walker found no 2-4-digit AI segment). Cross-variant
        // guard against BadScheme is implicit in assert_eq!.
        assert_eq!(
            parse_dl_uri("https://example.com/foo"),
            Err(DlUriError::NoAiInPath),
            "URI with valid scheme but no AI digit segments must reject as NoAiInPath — kills variant-swap mutations"
        );
    }

    #[test]
    fn parse_dl_uri_stops_at_first_non_ai_segment() {
        // If a segment isn't 2-4 digits, we stop walking the path —
        // the GS1 DL spec allows "convenience" alpha keys we don't
        // implement, but we still require at least one numeric AI
        // before such a segment.
        let v = parse_dl_uri("https://x.example/01/04012345123456/foo/bar").unwrap();
        assert_eq!(v.len(), 1);
    }

    /// Stage 11.A8c — pin `parse_dl_uri`'s less-exercised branches:
    ///
    ///   1. `http://` scheme acceptance (the existing tests all use
    ///      `https://`, leaving the `or_else()` branch on line 192
    ///      unexercised — a mutant that swaps the schemes would still
    ///      pass all current tests).
    ///   2. `NoPath` error when the URI has scheme + authority but no
    ///      `/` follows (line 199).
    ///   3. `BadQueryParam` error when a query string entry lacks `=`
    ///      (line 238).
    ///   4. Non-AI query param silently skipped (line 245).
    #[test]
    fn parse_dl_uri_branch_coverage() {
        // 1. http:// scheme.
        let v = parse_dl_uri("http://x.example/01/04012345123456").unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].ai, "01");
        // 2. NoPath: no `/` after authority. "https://example.com"
        // is just the scheme + authority; split_once('/') returns
        // None because there's no slash after `example.com`.
        match parse_dl_uri("https://example.com") {
            Err(DlUriError::NoPath) => {}
            other => panic!("expected NoPath, got {other:?}"),
        }
        // 3. BadQueryParam: a query entry without `=`. "?foo" has no
        // separator. (Note: empty query entries from "?&" are
        // silently skipped via the `.filter(!s.is_empty())` step.)
        match parse_dl_uri("https://x.example/01/04012345123456?foo") {
            Err(DlUriError::BadQueryParam { param }) => {
                assert_eq!(param, "foo");
            }
            other => panic!("expected BadQueryParam('foo'), got {other:?}"),
        }
        // 4. Non-AI query param silently skipped. "lang=en" isn't a
        // 2-4-digit AI, so `is_valid_ai("lang")` returns false and
        // the entry is skipped. The valid path AI still produces 1
        // element.
        let v = parse_dl_uri("https://x.example/01/04012345123456?lang=en").unwrap();
        assert_eq!(
            v.len(),
            1,
            "non-AI query params must be skipped silently, not rejected"
        );
        assert_eq!(v[0].ai, "01");
    }

    /// Stage 11.A8c — pin `is_valid_ai` length boundaries. Kills
    /// `2..=4` range mutations and `all(is_ascii_digit)` short-circuit
    /// flips on line 289.
    #[test]
    fn is_valid_ai_length_boundaries() {
        // Valid: 2, 3, 4 digit-only AIs.
        assert!(is_valid_ai("01"));
        assert!(is_valid_ai("123"));
        assert!(is_valid_ai("1234"));
        assert!(is_valid_ai("99"));
        // Too short.
        assert!(!is_valid_ai(""));
        assert!(!is_valid_ai("0"));
        // Too long.
        assert!(!is_valid_ai("12345"));
        assert!(!is_valid_ai("123456"));
        // Right length but non-digit.
        assert!(!is_valid_ai("ab"));
        assert!(!is_valid_ai("1A"));
        assert!(!is_valid_ai("12a"));
        assert!(!is_valid_ai("X234"));
    }

    /// Stage 11.A8c — pin `ai_is_variable_length` for representative
    /// entries across the AI table. The helper is used by
    /// databar_expanded and gs1_cc to decide whether an FNC1
    /// separator must follow an element; mutations on
    /// `.find()` / `.map()` could swap variable ↔ fixed-length
    /// classification and survive end-to-end tests for any input
    /// that doesn't depend on a particular FNC1 placement.
    ///
    /// Spot-checks across both classes:
    ///   * Fixed-length AIs (`variable = false`):
    ///       "01" (GTIN-14), "02", "11" (date), "17" (expiry),
    ///       "20" (variant code).
    ///   * Variable-length AIs (`variable = true`):
    ///       "10" (lot/batch), "21" (serial).
    ///   * Unknown AIs return None — `"99999"` (5-digit, doesn't
    ///     match) and `""` (empty).
    /// Stage 11.A8c — pin `hex_digit(b)` four-arm match: digits,
    /// lowercase, uppercase, and default-None.
    ///
    /// Mutations caught:
    ///   * `b - b'0'` → `b - b'1'` shifts the digit codes.
    ///   * `b - b'a' + 10` constant `10` → `9` or `11` would shift
    ///     the lowercase / uppercase codes (caught by 'a' → 10 anchor).
    ///   * Any range bound mutation (`'9'` → `'8'`, `'f'` → `'g'`,
    ///     etc.) would either over-accept (mapping next char to a
    ///     value) or under-accept (boundary returning None).
    ///   * Default arm returning `Some(_)` would fail the punctuation
    ///     and out-of-range char None assertions.
    #[test]
    fn hex_digit_per_arm_and_boundaries() {
        // Decimal digit arm.
        assert_eq!(hex_digit(b'0'), Some(0));
        assert_eq!(hex_digit(b'9'), Some(9));
        assert_eq!(hex_digit(b'5'), Some(5));
        // Lowercase hex arm.
        assert_eq!(hex_digit(b'a'), Some(10));
        assert_eq!(hex_digit(b'f'), Some(15));
        assert_eq!(hex_digit(b'c'), Some(12));
        // Uppercase hex arm.
        assert_eq!(hex_digit(b'A'), Some(10));
        assert_eq!(hex_digit(b'F'), Some(15));
        assert_eq!(hex_digit(b'C'), Some(12));
        // Just-outside boundaries → None.
        assert_eq!(hex_digit(b'/'), None, "below '0'");
        assert_eq!(hex_digit(b':'), None, "above '9'");
        assert_eq!(hex_digit(b'`'), None, "below 'a'");
        assert_eq!(hex_digit(b'g'), None, "above 'f'");
        assert_eq!(hex_digit(b'@'), None, "below 'A'");
        assert_eq!(hex_digit(b'G'), None, "above 'F'");
        // Far defaults.
        assert_eq!(hex_digit(b' '), None);
        assert_eq!(hex_digit(0), None);
        assert_eq!(hex_digit(255), None);
    }

    #[test]
    fn ai_is_variable_length_classifies_known_ais() {
        // Fixed-length.
        assert_eq!(
            ai_is_variable_length("01"),
            Some(false),
            "01 (GTIN-14) fixed"
        );
        assert_eq!(ai_is_variable_length("02"), Some(false));
        assert_eq!(ai_is_variable_length("11"), Some(false), "11 (date) fixed");
        assert_eq!(
            ai_is_variable_length("17"),
            Some(false),
            "17 (expiry) fixed"
        );
        assert_eq!(ai_is_variable_length("20"), Some(false));
        // Variable-length.
        assert_eq!(ai_is_variable_length("10"), Some(true), "10 (lot) variable");
        assert_eq!(
            ai_is_variable_length("21"),
            Some(true),
            "21 (serial) variable"
        );
        // AI 99 is in the table as a variable alphanumeric.
        assert_eq!(
            ai_is_variable_length("99"),
            Some(true),
            "99 variable alphanumeric"
        );
        // Unknown AIs return None.
        assert_eq!(ai_is_variable_length("99999"), None);
        assert_eq!(ai_is_variable_length(""), None);
        assert_eq!(
            ai_is_variable_length("89"),
            None,
            "AI 89 not in BWIPP table"
        );
    }

    /// Stage 11.A8c — pin `hex_digit` for every defined case + a few
    /// rejects. Kills the per-arm `delete match arm` and `- b'X' + N`
    /// arithmetic mutations on line 313-319.
    #[test]
    fn hex_digit_all_cases() {
        // Decimal digits.
        assert_eq!(hex_digit(b'0'), Some(0));
        assert_eq!(hex_digit(b'9'), Some(9));
        assert_eq!(hex_digit(b'5'), Some(5));
        // Lowercase hex.
        assert_eq!(hex_digit(b'a'), Some(10));
        assert_eq!(hex_digit(b'f'), Some(15));
        assert_eq!(hex_digit(b'c'), Some(12));
        // Uppercase hex.
        assert_eq!(hex_digit(b'A'), Some(10));
        assert_eq!(hex_digit(b'F'), Some(15));
        assert_eq!(hex_digit(b'D'), Some(13));
        // Non-hex.
        assert_eq!(hex_digit(b'g'), None);
        assert_eq!(hex_digit(b'G'), None);
        assert_eq!(hex_digit(b' '), None);
        assert_eq!(hex_digit(b':'), None); // just above '9'
        assert_eq!(hex_digit(b'/'), None); // just below '0'
        assert_eq!(hex_digit(b'@'), None); // just below 'A'
        assert_eq!(hex_digit(b'['), None); // just above 'Z' (but '`' below 'a')
        assert_eq!(hex_digit(b'`'), None); // just below 'a'
    }

    /// Stage 11.A8c — pin `AiSpec::validate_data` per-branch behaviour.
    /// The helper enforces length bounds and per-format char rules; a
    /// single mutation would silently accept malformed data through
    /// parse().
    ///
    /// Branches:
    /// 1. max_len = Some(N) with N == min_len → fixed-length check
    ///    (n < min OR n > max → BadLength).
    /// 2. max_len = Some(N) with N > min_len → range check.
    /// 3. max_len = None → variable, only checks n < min_len.
    /// 4. Char loop: Numeric requires `is_ascii_digit`; Alphanumeric
    ///    requires `is_ascii_graphic`.
    ///
    /// Mutations caught:
    /// * `n < self.min_len || n > max` → || ↔ && would accept partial
    ///   violations.
    /// * `n < self.min_len` boundary (would accept too-short or
    ///   reject min-len exactly).
    /// * `is_ascii_digit` / `is_ascii_graphic` predicate swap.
    /// * Variable-length None arm allowing n > min_len.
    #[test]
    fn validate_data_per_branch() {
        // Fixed-length numeric AI: min=3, max=Some(3).
        let fixed = AiSpec {
            ai: "test",
            min_len: 3,
            max_len: Some(3),
            variable: false,
            format: DataFormat::Numeric,
        };
        // Stage 11.A8c (cont) — descriptive label naming fixed-length
        // exact-match happy path.
        assert!(
            fixed.validate_data("123").is_ok(),
            "AiSpec(min=3, max=3, fixed).validate_data(\"123\") (exact-length numeric) must accept — kills off-by-one length-guard mutants on min==max fixed-length specs"
        );
        // Too-short / too-long route to BadLength with ai="test",
        // expected="3" (since min==max), got=actual char count. Pin
        // each variant + per-input length echo so the format/value
        // interpolations can't drift.
        match fixed.validate_data("12").unwrap_err() {
            ParseError::BadLength { ai, expected, got } => {
                assert_eq!(ai, "test", "ai field must echo AiSpec.ai");
                assert_eq!(expected, "3", "expected = `{{min}}` when min==max");
                assert_eq!(got, 2, "got = actual char count (2)");
            }
            other => panic!("\"12\" should be BadLength, got {other:?}"),
        }
        match fixed.validate_data("1234").unwrap_err() {
            ParseError::BadLength { ai, expected, got } => {
                assert_eq!(ai, "test");
                assert_eq!(expected, "3");
                assert_eq!(got, 4, "got = actual char count (4)");
            }
            other => panic!("\"1234\" should be BadLength, got {other:?}"),
        }
        // Non-digit rejected.
        // Stage 11.A8c (cont) — upgrade partial-field `matches!`
        // pattern to bind both `ai` and `ch` fields. The original
        // pin only bound `ch: 'A'` and used `..` to swallow `ai`;
        // this kills mutations that route a different AI's reject
        // through the BadCharacter variant with 'A' as the offending
        // char.
        match fixed.validate_data("12A").unwrap_err() {
            ParseError::BadCharacter { ai, ch } => {
                assert_eq!(ai, "test", "ai must echo the AiSpec's ai (\"test\")");
                assert_eq!(ch, 'A', "ch must be 'A' (the first non-digit at idx 2)");
            }
            other => panic!("\"12A\" should be BadCharacter, got {other:?}"),
        }

        // Range numeric AI: min=2, max=Some(5).
        let range = AiSpec {
            ai: "rng",
            min_len: 2,
            max_len: Some(5),
            variable: true,
            format: DataFormat::Numeric,
        };
        assert!(range.validate_data("12").is_ok(), "min boundary");
        assert!(range.validate_data("12345").is_ok(), "max boundary");
        // Range AI: expected = "2-5" (since min != max), got = actual.
        match range.validate_data("1").unwrap_err() {
            ParseError::BadLength { ai, expected, got } => {
                assert_eq!(ai, "rng");
                assert_eq!(expected, "2-5", "range AI expected = \"min-max\"");
                assert_eq!(got, 1, "below-min count");
            }
            other => panic!("\"1\" should be BadLength, got {other:?}"),
        }
        match range.validate_data("123456").unwrap_err() {
            ParseError::BadLength { ai, expected, got } => {
                assert_eq!(ai, "rng");
                assert_eq!(expected, "2-5");
                assert_eq!(got, 6, "above-max count");
            }
            other => panic!("\"123456\" should be BadLength, got {other:?}"),
        }

        // Variable AI: min=4, max=None.
        let var = AiSpec {
            ai: "var",
            min_len: 4,
            max_len: None,
            variable: true,
            format: DataFormat::Alphanumeric,
        };
        assert!(var.validate_data("abcd").is_ok(), "min boundary");
        assert!(var.validate_data("abcdefghij").is_ok(), "long ok");
        // Variable AI: max_len=None → expected = "4+" suffix style.
        match var.validate_data("abc").unwrap_err() {
            ParseError::BadLength { ai, expected, got } => {
                assert_eq!(ai, "var");
                assert_eq!(expected, "4+", "open-max AI expected = \"min+\"");
                assert_eq!(got, 3);
            }
            other => panic!("\"abc\" should be BadLength, got {other:?}"),
        }
        // Non-graphic char (space=0x20 is graphic? No — is_ascii_graphic
        // excludes whitespace). Pin BOTH the char echo and the
        // BadCharacter variant.
        match var.validate_data("ab c").unwrap_err() {
            ParseError::BadCharacter { ai, ch } => {
                assert_eq!(ai, "var");
                assert_eq!(ch, ' ', "BadCharacter must echo the offending char");
            }
            other => panic!("\"ab c\" should be BadCharacter, got {other:?}"),
        }
        match var.validate_data("abc\n").unwrap_err() {
            ParseError::BadCharacter { ai, ch } => {
                assert_eq!(ai, "var");
                assert_eq!(ch, '\n', "BadCharacter must echo newline as '\\n'");
            }
            other => panic!("\"abc\\n\" should be BadCharacter, got {other:?}"),
        }
        // Alphanumeric accepts digits + letters + punctuation.
        // Stage 11.A8c (cont) — descriptive label naming alphanumeric
        // mixed-char-class happy path.
        assert!(
            var.validate_data("ab12!@#$").is_ok(),
            "var-length Alphanumeric AiSpec.validate_data(\"ab12!@#$\") (lowercase letters + digits + 4 distinct punctuation) must accept — covers the full Alphanumeric char-class boundary, kills overly-strict char-class mutants"
        );
    }

    /// Stage 11.A8c — pin `url_percent_decode` for representative
    /// encoded sequences. Kills the `* 16` / `+ lo` arithmetic and
    /// the `b'%'` short-circuit mutations on lines 295-310.
    #[test]
    fn url_percent_decode_known_sequences() {
        // No encoding → identity.
        assert_eq!(url_percent_decode("abc"), "abc");
        assert_eq!(url_percent_decode(""), "");
        // Simple percent-encoded ASCII: %20 → space.
        assert_eq!(url_percent_decode("a%20b"), "a b");
        // Multiple encoded.
        assert_eq!(url_percent_decode("%41%42%43"), "ABC");
        // Encoded at start / end.
        assert_eq!(url_percent_decode("%41bc"), "Abc");
        assert_eq!(url_percent_decode("ab%43"), "abC");
        // Lowercase hex.
        assert_eq!(url_percent_decode("%2f"), "/");
        // Mixed case hex.
        assert_eq!(url_percent_decode("%2F%6e"), "/n");
        // Bare % not followed by 2 hex digits → emit literal.
        assert_eq!(url_percent_decode("a%"), "a%");
        assert_eq!(url_percent_decode("a%X"), "a%X");
        assert_eq!(url_percent_decode("a%XY"), "a%XY");
    }
}
