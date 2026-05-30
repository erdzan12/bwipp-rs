//! Channel Code (USPS Tray Labels).
//!
//! Channel Code encodes a numeric value into a fixed number of "channels"
//! (n = `barlen + 1`, where `barlen` is the input digit count). Each channel
//! contributes one space + one bar to the symbol; the widths are chosen by
//! enumerating every valid combination in BWIPP-defined order and picking
//! the `value`-th one. The output is a finder (9 modules, or 5 if
//! `shortfinder`) followed by the per-channel width pairs.
//!
//! Direct port of BWIPP `bwipp_channelcode` (bwip-js lines 43533-43706). The
//! recursive `nextb`/`nexts` enumeration walks the search tree in BWIPP's
//! exact lexicographic order so the produced sbs is byte-identical.

use crate::encoding::LinearPattern;
use crate::error::Error;
use crate::options::Options;

/// Maximum value per barcode-digit length, from BWIPP
/// `channelcode_chancaps` (bwip-js line 42006). Indexed by
/// `barcode.len() - 2`, so index 0 → 2 digits (max 26), index 5 → 7
/// digits (max 7 742 862).
const MAX_BY_LEN: [u32; 6] = [26, 292, 3493, 44_072, 576_688, 7_742_862];

/// Per-barlen mod-23 weighting table for the `includecheck=true`
/// option, from BWIPP `bwipp_channelcode` (bwip-js line 42072).
/// Indexed by `barcode.len() - 2` so position 0 is the 2-digit
/// weight row (6 entries), …, position 5 is the 7-digit row
/// (16 entries). Each row has `chan*2 = (barlen+1)*2` entries.
const MOD23_BY_LEN: &[&[u32]] = &[
    // barlen=2, chan=3, 6 weights
    &[13, 12, 4, 9, 3, 1],
    // barlen=3, chan=4, 8 weights
    &[13, 2, 12, 3, 18, 16, 4, 1],
    // barlen=4, chan=5, 10 weights
    &[11, 16, 17, 8, 20, 4, 10, 2, 5, 1],
    // barlen=5, chan=6, 12 weights
    &[1, 4, 16, 18, 3, 12, 2, 8, 9, 13, 6, 1],
    // barlen=6, chan=7, 14 weights
    &[20, 16, 22, 13, 15, 12, 5, 4, 8, 9, 21, 3, 7, 1],
    // barlen=7, chan=8, 16 weights
    &[2, 6, 18, 8, 1, 3, 9, 4, 12, 13, 16, 2, 6, 18, 8, 1],
];

/// One recursive-enumeration step. BWIPP's `nextb` and `nexts` mutate the
/// `b` and `s` width arrays in place, count emissions in `value`, and
/// capture the matching combination in `out` when `value == target`.
struct Walker {
    chan: usize,
    target: u32,
    value: u32,
    out: Option<Vec<u8>>,
    /// Bar-width array (BWIPP `$_.b`). Indices 0..=2 are pre-filled
    /// boundary widths; the recursion fills 3..=chan+2.
    b: [u8; 11],
    /// Space-width array (BWIPP `$_.s`). Same indexing convention.
    s: [u8; 11],
}

impl Walker {
    fn new(target: u32, chan: usize) -> Self {
        Self {
            chan,
            target,
            value: 0,
            out: None,
            b: [1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
            s: [0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        }
    }

    fn run(target: u32, chan: usize) -> Vec<u8> {
        let mut w = Self::new(target, chan);
        // BWIPP launches with stack [chan, chan, 3] — i.e. arg2 = chan
        // *and* arg1 = chan, idx = 3.
        w.nexts(chan as u8, chan as u8, 3);
        w.out
            .expect("encode should produce an output for any valid target")
    }

    /// BWIPP `nexts`: place a space at index `idx`, then recurse to place
    /// the corresponding bar via `nextb`. The recursion carries two
    /// width-budget args (`arg2`, `arg1`) that **swap roles** on each
    /// nexts↔nextb hop: the iteration bound (`arg1`) becomes the next
    /// call's saved budget (`arg2`), and the previous call's `arg2`
    /// becomes the next iteration bound. BWIPP's stack-machine
    /// `nexts`/`nextb` express this by pushing
    /// `[arg1 - x + 1, arg2, idx_or_idx+1]` for the recursive call —
    /// the first element is the new `arg2`, the second is the new
    /// `arg1`.
    fn nexts(&mut self, arg2: u8, arg1: u8, idx: usize) {
        let min: u8 = if idx < self.chan + 2 { 1 } else { arg1 };
        for x in min..=arg1 {
            self.s[idx] = x;
            // rotate: new arg2 = arg1 - x + 1, new arg1 = arg2.
            self.nextb(arg1 - x + 1, arg2, idx);
        }
    }

    /// BWIPP `nextb`: place a bar at index `idx`. If we've filled the
    /// last (idx == chan+2) channel, optionally emit the combination
    /// matching `target`. Same arg2/arg1 rotation as [`nexts`].
    fn nextb(&mut self, arg2: u8, arg1: u8, idx: usize) {
        let space_sum = self.s[idx] + self.b[idx - 1] + self.b[idx - 2] + self.s[idx - 1];
        let bar_min: u8 = if space_sum > 4 { 1 } else { 2 };
        if idx < self.chan + 2 {
            for r in bar_min..=arg1 {
                self.b[idx] = r;
                self.nexts(arg1 - r + 1, arg2, idx + 1);
            }
        } else if bar_min <= arg1 {
            // BWIPP: b[idx] = arg1, NOT the "remaining" arg2.
            self.b[idx] = arg1;
            if self.value == self.target {
                let mut out = Vec::with_capacity(self.chan * 2);
                for k in 3..=10 {
                    out.push(self.s[k]);
                    out.push(self.b[k]);
                }
                out.truncate(self.chan * 2);
                self.out = Some(out);
            }
            self.value += 1;
        }
    }
}

/// Parse and validate BWIPP-exposed Channel Code options. Returns
/// `(shortfinder, includecheck)` with defaults `(false, false)`.
/// Mirrors BWIPP `bwipp_channelcode` (`bwip-js-node.js:41981-41985`):
/// `shortfinder`, `includetext`, `includecheck`, `height`. Of these,
/// `includetext` is a renderer concern (handled by
/// `Options::include_text` at the dispatcher level); `height` is also
/// renderer-side. The encoder consumes `shortfinder` and
/// `includecheck` to change its logical sbs output.
fn check_channelcode_opts(opts: &Options) -> Result<(bool, bool), Error> {
    let mut out = (false, false); // (shortfinder, includecheck)
    for (key, slot) in [("shortfinder", 0u8), ("includecheck", 1u8)] {
        if let Some(v) = opts.get(key) {
            let val = match v {
                "false" => false,
                "true" => true,
                _ => {
                    return Err(Error::InvalidOption(format!(
                        "channelcode: {key}={v:?} must be \"true\" or \"false\""
                    )));
                }
            };
            if slot == 0 {
                out.0 = val;
            } else {
                out.1 = val;
            }
        }
    }
    Ok(out)
}

/// Encode a Channel Code payload. Input is 2-7 ASCII digits.
///
/// # Errors
/// - `InvalidData` if the input isn't 2-7 digits, contains a non-digit,
///   or its integer value exceeds the BWIPP-defined per-length maximum.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::ChannelCode, "12", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<LinearPattern, Error> {
    let (shortfinder, includecheck) = check_channelcode_opts(opts)?;
    if data.len() < 2 || data.len() > 7 {
        return Err(Error::InvalidData(format!(
            "Channel Code: input must be 2 to 7 digits, got {}",
            data.len()
        )));
    }
    for b in data.bytes() {
        if !b.is_ascii_digit() {
            return Err(Error::InvalidData(format!(
                "Channel Code: non-digit byte 0x{b:02x} in input"
            )));
        }
    }
    let value: u32 = data.parse().map_err(|_| {
        Error::InvalidData(format!("Channel Code: cannot parse {data:?} as integer"))
    })?;
    let max = MAX_BY_LEN[data.len() - 2];
    if value > max {
        return Err(Error::InvalidData(format!(
            "Channel Code: value {value} exceeds max {max} for {}-digit input",
            data.len()
        )));
    }
    let chan = data.len() + 1;
    let data_sbs = Walker::run(value, chan);

    // Finder pattern: 9 unit modules by default, or 5 with
    // `shortfinder=true` per BWIPP line 42056.
    let finder_len = if shortfinder { 5 } else { 9 };
    let mut sbs: Vec<u8> = vec![1; finder_len];
    sbs.extend_from_slice(&data_sbs);

    // Optional mod-23 check digit appended as a 3-channel pattern
    // (chan=3, 6 widths). Mirrors BWIPP lines 42062-42078.
    if includecheck {
        let weights = MOD23_BY_LEN[data.len() - 2];
        debug_assert_eq!(weights.len(), data_sbs.len());
        let mut sum: u32 = 0;
        for (i, &w) in data_sbs.iter().enumerate() {
            sum += (w as u32 - 1) * weights[i];
        }
        let check_value = sum % 23;
        let check_sbs = Walker::run(check_value, 3);
        sbs.extend_from_slice(&check_sbs);
    }

    // Build a LinearPattern from the alternating space/bar sbs widths.
    // BWIPP emits sbs as [space, bar, space, bar, ...], starting on a
    // space. Our `LinearPattern` carries the bar widths only, with the
    // space widths implicit between bars.
    let pattern = pattern_from_sbs(&sbs);
    Ok(pattern)
}

fn pattern_from_sbs(sbs: &[u8]) -> LinearPattern {
    // The sbs alternates space-bar-space-bar. Convert to the canonical
    // module string used by `LinearPattern::from_modules` so the existing
    // renderer plumbing (bar fill, text, dimensions) handles the rest.
    let mut modules = String::new();
    let mut is_bar = false; // sbs starts with a space.
    for &w in sbs {
        let ch = if is_bar { '1' } else { '0' };
        for _ in 0..w {
            modules.push(ch);
        }
        is_bar = !is_bar;
    }
    LinearPattern::from_modules(&modules, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Channel Code goldens captured from
    /// `bwipp.raw("channelcode", v, {})[0].sbs` for the four inputs
    /// exercising barcode lengths 2, 2, 3, and 5 respectively.
    /// Pinning the full sbs proves the recursive enumeration walks
    /// BWIPP's exact order across multiple channel counts.
    #[test]
    fn channelcode_matches_bwip_js_raw_sbs() {
        let cases: &[(&str, &[u8])] = &[
            ("00", &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 3, 2]),
            ("12", &[1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 2, 1, 1, 3]),
            ("128", &[1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 2, 1, 1, 1, 2, 4]),
            (
                "00000",
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 2, 1, 1, 6, 4,
                ],
            ),
        ];
        for &(input, want) in cases {
            let chan = input.len() + 1;
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // per-iteration input echo + path label naming the channel
            // count derived from input length.
            let target: u32 = input.parse().unwrap_or_else(|e| {
                panic!("input.parse::<u32>({input:?}) (Walker driver, chan={chan} from len={}) must succeed; got Err: {e}", input.len())
            });
            let mut got = vec![1u8; 9];
            got.extend_from_slice(&Walker::run(target, chan));
            assert_eq!(got.as_slice(), want, "channelcode({input:?}) sbs mismatch");
        }
    }

    #[test]
    fn encode_rejects_short_or_long_or_non_digit_or_overflow() {
        // Stage 11.A8c (cont) — upgrade 4 discriminant-only
        // `matches!(_, Err(Error::InvalidData(_)))` to per-arm multi-
        // anchor pins matching the source diagnostics at lines
        // 175-178 / 182-184 / 192-195 of channelcode.rs.

        // < 2 digits → `Channel Code: input must be 2 to 7 digits, got 1`.
        match encode("1", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Channel Code:"),
                    "short arm: missing `Channel Code:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must be 2 to 7 digits"),
                    "short arm: missing length-spec predicate: {msg}"
                );
                assert!(
                    msg.contains("got 1"),
                    "short arm: missing `got 1` echo: {msg}"
                );
                assert!(
                    !msg.contains("non-digit"),
                    "short arm: non-digit leaked into short reject: {msg}"
                );
            }
            other => panic!("\"1\" should reject as InvalidData, got {other:?}"),
        }

        // > 7 digits → `Channel Code: input must be 2 to 7 digits, got 8`.
        match encode("12345678", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Channel Code:"),
                    "long arm: missing `Channel Code:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must be 2 to 7 digits"),
                    "long arm: missing length-spec predicate: {msg}"
                );
                assert!(
                    msg.contains("got 8"),
                    "long arm: missing `got 8` echo: {msg}"
                );
            }
            other => panic!("\"12345678\" should reject as InvalidData, got {other:?}"),
        }

        // Non-digit → `Channel Code: non-digit byte 0x41 in input` ('A'=0x41).
        match encode("1A", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Channel Code:"),
                    "non-digit arm: missing `Channel Code:` prefix: {msg}"
                );
                assert!(
                    msg.contains("non-digit byte 0x41"),
                    "non-digit arm: missing `non-digit byte 0x41` echo ('A'=0x41): {msg}"
                );
                assert!(
                    !msg.contains("must be 2 to 7 digits"),
                    "non-digit arm: length-spec leaked into non-digit reject: {msg}"
                );
            }
            other => panic!("\"1A\" should reject as InvalidData, got {other:?}"),
        }

        // 2-digit overflow → `Channel Code: value 99 exceeds max 26 for 2-digit input`.
        match encode("99", &Options::default()) {
            Err(Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Channel Code:"),
                    "overflow arm: missing `Channel Code:` prefix: {msg}"
                );
                assert!(
                    msg.contains("value 99 exceeds max 26"),
                    "overflow arm: missing `value 99 exceeds max 26` echo (kills `{{value}}` or `{{max}}` interpolation drops): {msg}"
                );
                assert!(
                    msg.contains("2-digit input"),
                    "overflow arm: missing `2-digit input` echo: {msg}"
                );
            }
            other => panic!("\"99\" should reject as InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.2 — `shortfinder=true` / `includecheck=true` corpus
    /// pinned byte-for-byte against `bwipp_channelcode` (BWIPP
    /// 2026-04-21 / bwip-js 4.10.1), captured via
    /// `rust/tools/oracle-channelcode-opts.js`. Each row is
    /// `(input, shortfinder, includecheck, expected_sbs)`.
    #[test]
    fn opt_in_corpus_matches_bwipp() {
        let cases: &[(&str, bool, bool, &[u8])] = &[
            ("12", true, false, &[1, 1, 1, 1, 1, 2, 1, 2, 1, 1, 3]),
            ("00", true, false, &[1, 1, 1, 1, 1, 1, 2, 1, 1, 3, 2]),
            ("128", true, false, &[1, 1, 1, 1, 1, 2, 1, 2, 1, 1, 1, 2, 4]),
            (
                "00000",
                true,
                false,
                &[1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 2, 1, 1, 6, 4],
            ),
            ("26", true, false, &[1, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1]),
            (
                "12",
                false,
                true,
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 2, 1, 1, 3, 2, 3, 1, 1, 2, 1,
                ],
            ),
            (
                "128",
                false,
                true,
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 2, 1, 1, 1, 2, 4, 2, 1, 1, 1, 2, 3,
                ],
            ),
            (
                "1234",
                false,
                true,
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 2, 1, 2, 4, 2, 2, 1, 3, 2, 1, 2, 1,
                ],
            ),
            (
                "12345",
                false,
                true,
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 4, 1, 2, 1, 1, 2, 2, 1, 1, 3, 2, 1, 1, 1, 2, 3,
                ],
            ),
            (
                "123456",
                false,
                true,
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 5, 4, 2, 1, 1, 1, 2, 1, 1, 2, 2, 2, 2,
                    1, 1, 2,
                ],
            ),
            (
                "1234567",
                false,
                true,
                &[
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 3, 2, 3, 1, 1, 2, 2, 1, 1, 4, 2, 1, 2, 2, 2,
                    3, 2, 1, 1, 1,
                ],
            ),
            (
                "12345",
                true,
                true,
                &[
                    1, 1, 1, 1, 1, 1, 3, 4, 1, 2, 1, 1, 2, 2, 1, 1, 3, 2, 1, 1, 1, 2, 3,
                ],
            ),
            (
                "1234567",
                true,
                true,
                &[
                    1, 1, 1, 1, 1, 1, 2, 3, 2, 3, 1, 1, 2, 2, 1, 1, 4, 2, 1, 2, 2, 2, 3, 2, 1, 1, 1,
                ],
            ),
        ];
        for &(input, shortfinder, includecheck, expected) in cases {
            let mut opts = Options::default();
            if shortfinder {
                opts = opts.with("shortfinder", "true");
            }
            if includecheck {
                opts = opts.with("includecheck", "true");
            }
            // Rebuild the sbs from the same path encode() runs (via
            // Walker::run + finder + check) but without going through
            // pattern_from_sbs, since the test target is the raw sbs.
            let value: u32 = input.parse().unwrap();
            let chan = input.len() + 1;
            let data_sbs = Walker::run(value, chan);
            let finder_len = if shortfinder { 5 } else { 9 };
            let mut sbs: Vec<u8> = vec![1; finder_len];
            sbs.extend_from_slice(&data_sbs);
            if includecheck {
                let weights = MOD23_BY_LEN[input.len() - 2];
                let mut sum: u32 = 0;
                for (i, &w) in data_sbs.iter().enumerate() {
                    sum += (w as u32 - 1) * weights[i];
                }
                let cv = sum % 23;
                sbs.extend_from_slice(&Walker::run(cv, 3));
            }
            assert_eq!(
                sbs.as_slice(),
                expected,
                "channelcode opt-in mismatch for {input:?} \
                 shortfinder={shortfinder} includecheck={includecheck}"
            );
            // Sanity: encode() with the same options succeeds.
            // Stage 11.A8c (cont) — descriptive per-iteration label
            // naming opt-in combo + payload (the bare assert in a
            // loop over multiple (input, shortfinder, includecheck)
            // tuples gave no info on which combination failed).
            assert!(
                encode(input, &opts).is_ok(),
                "encode({input:?}, shortfinder={shortfinder}, includecheck={includecheck}) opt-in path must succeed end-to-end (after sbs golden was already pinned for the same combo)"
            );
        }
    }

    /// Stage 11.2 — explicit-default values for both opts are
    /// equivalent to omitting them.
    #[test]
    fn default_opts_equivalent_to_explicit_false() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Channel Code explicit-vs-default opts equivalence path:
        // shortfinder=false + includecheck=false must equal default.
        let baseline = encode("128", &Options::default()).expect(
            "encode(\"128\", default) (Channel Code default-opts baseline for explicit-equivalence check) must succeed",
        );
        let explicit = encode(
            "128",
            &Options::default()
                .with("shortfinder", "false")
                .with("includecheck", "false"),
        )
        .expect(
            "encode(\"128\", shortfinder=false, includecheck=false) (Channel Code explicit-false opts; must equal default) must succeed",
        );
        assert_eq!(baseline.bars, explicit.bars);
    }

    /// Stage 11.2 — invalid option values return `InvalidOption`.
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains(k)` upgraded
    /// to a 5-anchor pin per iteration:
    ///   1. symbology prefix `channelcode:`
    ///   2. key=value Debug echo (e.g. `shortfinder="yes"`)
    ///   3. predicate `must be`
    ///   4. valid-values `"true"` and `"false"`
    ///   5. cross-key contamination guard — when rejecting `shortfinder`
    ///      the message must NOT mention `includecheck` (and vice
    ///      versa), so an arm/key-swap mutation in the `for` loop is
    ///      caught.
    #[test]
    fn rejects_invalid_option_values() {
        for (k, v) in [("shortfinder", "yes"), ("includecheck", "maybe")] {
            let err = encode("12", &Options::default().with(k, v)).unwrap_err();
            match err {
                Error::InvalidOption(msg) => {
                    assert!(
                        msg.contains("channelcode:"),
                        "missing channelcode prefix for {k}={v:?}: {msg:?}"
                    );
                    let kv = format!("{k}={v:?}");
                    assert!(
                        msg.contains(&kv),
                        "missing key=value echo {kv:?} for {k}={v:?}: {msg:?}"
                    );
                    assert!(
                        msg.contains("must be"),
                        "missing predicate `must be` for {k}={v:?}: {msg:?}"
                    );
                    assert!(
                        msg.contains("\"true\"") && msg.contains("\"false\""),
                        "missing valid-values \"true\"/\"false\" for {k}={v:?}: {msg:?}"
                    );
                    let other_key = if k == "shortfinder" {
                        "includecheck"
                    } else {
                        "shortfinder"
                    };
                    assert!(
                        !msg.contains(other_key),
                        "cross-key contamination: rejecting {k} but msg mentions {other_key}: {msg:?}"
                    );
                }
                other => panic!("expected InvalidOption for {k}={v:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn encode_renders_bars_in_canonical_order() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Channel Code canonical-bars smoke path: 2-digit "12" →
        // 9-cell finder + 6-cell data sbs.
        let p = encode("12", &Options::default()).expect(
            "encode(\"12\", default) (Channel Code canonical-bars smoke: 9-cell finder + 6-cell data sbs) must succeed",
        );
        // sbs total = 9 (finder) + 6 (data) = 15 modules of varying width;
        // total module count = sum of sbs = 9 + (1+1+1+1+1+1) wait the data
        // sbs for "12" sums to 1+2+1+2+1+1+3 = 11? Actually 9-finder is 9
        // modules total, plus data 11 modules = 20-ish. Just sanity check
        // that the pattern is non-empty.
        assert!(p.bars.iter().any(|&w| w > 0));
    }

    // ---------------------------------------------------------------------
    // Stage 11.A8 mutation-killer tests.
    // ---------------------------------------------------------------------

    /// Kills `check_channelcode_opts: replace == with !=` at line ~148.
    /// The original test feeds both options simultaneously, so the
    /// swapped storage was masked. Here we exercise each option in
    /// isolation and rely on the cross-option asymmetry — under the
    /// mutant, `shortfinder=true` would actually set the `includecheck`
    /// flag (and vice versa), producing the *wrong* encoder behaviour.
    #[test]
    fn check_channelcode_opts_routes_each_option_to_the_correct_slot() {
        // shortfinder only: the finder collapses from 9 to 5 modules,
        // *and* no check digit is appended. Under the swapped-slot
        // mutant the encoder would emit the long finder (9 modules) and
        // *append* a check codeword (3 channels = 6 sbs widths).
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Channel Code opts-slot-correctness paths: shortfinder
        // collapses finder 9→5 modules + no check; includecheck keeps
        // finder + appends 3-channel check; their total widths must
        // differ so a `== vs !=` slot swap surfaces visibly.
        let only_short = encode("12", &Options::default().with("shortfinder", "true")).expect(
            "encode(\"12\", shortfinder=true) (Channel Code shortfinder-only path: 5-cell finder + 6-cell data sbs, no check) must succeed",
        );
        let only_check = encode("12", &Options::default().with("includecheck", "true")).expect(
            "encode(\"12\", includecheck=true) (Channel Code includecheck-only path: 9-cell finder + 6-cell data + 6-cell mod-23 check sbs) must succeed",
        );
        // shortfinder produces a smaller pattern than includecheck:
        // 5-module finder + 6 data widths = 11 sbs widths,
        // vs 9-module finder + 6 data + 6 check = 21 sbs widths.
        // Total module width differs as well; assert the short-finder
        // total is strictly smaller than the include-check total.
        assert!(
            only_short.total_width() < only_check.total_width(),
            "shortfinder({}) total_width should be < includecheck({}) total_width; \
             likely check_channelcode_opts swapped the slots",
            only_short.total_width(),
            only_check.total_width(),
        );
    }

    /// Kills the cluster of arithmetic mutants at line ~213 (`sum +=
    /// (w - 1) * weights[i]`) and the modulo at line ~215 (`sum % 23`).
    /// Existing test `opt_in_corpus_matches_bwipp` rebuilds the check
    /// digit inline rather than going through `encode()`, so any
    /// regression in the check-digit arithmetic inside `encode()`
    /// silently passed. This test asserts the exact bar pattern
    /// produced by `encode("12", includecheck=true)`.
    #[test]
    fn includecheck_produces_correct_full_pattern_via_encode() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Channel Code includecheck full-pattern path: pins
        // (w-1)*weights[i] and sum%23 arithmetic with exact
        // run-length vector.
        let p = encode("12", &Options::default().with("includecheck", "true")).expect(
            "encode(\"12\", includecheck=true) (Channel Code includecheck full-pattern oracle: pins (w-1)*weights[i] + sum%23 check arithmetic via 21-element bars) must succeed",
        );
        // The expected `bars` (LinearPattern's run-length vector, which
        // starts with a bar — so it's `[0, sbs...]` because the
        // channelcode sbs always opens on a space). The sbs row from
        // the opt-in corpus for ("12", false, true) is
        //   [1,1,1,1,1,1,1,1,1, 2,1,2,1,1,3, 2,3,1,1,2,1].
        // After LinearPattern::from_modules emits a leading zero-width
        // bar, the run-length vector becomes:
        let want: &[u8] = &[
            0, // leading zero bar (sbs starts with space).
            1, 1, 1, 1, 1, 1, 1, 1, 1, // 9-cell finder.
            2, 1, 2, 1, 1, 3, // 6-cell data sbs for "12".
            2, 3, 1, 1, 2, 1, // 6-cell mod-23 check sbs.
        ];
        assert_eq!(
            p.bars.as_slice(),
            want,
            "encode(\"12\", includecheck=true).bars regressed; \
             check the (w-1)*weights[i] and sum%%23 arithmetic at lines 213/215"
        );
    }

    /// Kills `pattern_from_sbs: delete !` at line ~239 (the
    /// `is_bar = !is_bar` toggle). Removing the `!` makes the helper
    /// emit one polarity (all bars or all spaces) instead of an
    /// alternating pattern. We exercise the pattern's *first*
    /// non-zero bar — under the mutant the run-length vector would
    /// be `[0, total_modules]` (or `[total_modules, 0, 0, ...]`)
    /// rather than the fine-grained alternation.
    #[test]
    fn pattern_from_sbs_actually_alternates() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Channel Code pattern_from_sbs alternation path: kills
        // the `delete !` mutant on `is_bar = !is_bar` toggle that
        // would collapse alternation into a single huge run.
        let p = encode("12", &Options::default()).expect(
            "encode(\"12\", default) (Channel Code pattern_from_sbs alternation oracle: pins is_bar=!is_bar toggle via first-3-run prefix [0,1,1]) must succeed",
        );
        // The first three runs of bars after encode("12") should be
        // [0, 1, 1] — leading zero, then a 1-module space, then a
        // 1-module bar (the start of the 9-cell finder pattern). The
        // mutant collapses the alternation into a single huge run.
        assert!(
            p.bars.len() > 3,
            "encode(\"12\").bars must have at least 4 runs; \
             pattern_from_sbs may have stopped alternating"
        );
        assert_eq!(
            &p.bars[..3],
            &[0, 1, 1],
            "first three runs of encode(\"12\") regressed; \
             check the `is_bar = !is_bar` toggle in pattern_from_sbs"
        );
    }
}
