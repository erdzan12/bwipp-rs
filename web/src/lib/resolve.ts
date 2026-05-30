import type { CatalogEntry } from "./catalog";

export type ResolvedBarcode = {
  bcid: string;
  text: string;
  options: Record<string, unknown>;
  warning?: string;
};

// QR Code family was a documented compatibility exception until
// Stage 16 of the Rust crate's port, when `prefer-native-qrcode`
// became a default Cargo feature. The web workbench is backed by
// the WASM build of bwipp-rs, so QR output is now byte-for-byte
// BWIPP-equivalent by default and no compatibility warning is
// surfaced on `ResolvedBarcode.warning`.
//
// If a future regression re-introduces a substrate-side divergence,
// re-add the warning string + the qualifying check here. The
// resolve layer is the single chokepoint where `ResolvedBarcode.warning`
// is set, so this is the right place to land that change.

export function resolveBarcode(entry: CatalogEntry, data: string): ResolvedBarcode {
  if (entry.renderer === "bwipp") {
    if (!entry.bwippType) {
      throw new Error(`${entry.name} has no BWIPP encoder configured.`);
    }
    const result: ResolvedBarcode = {
      bcid: entry.bwippType,
      text: data,
      options: normalizeOptions(entry.options),
    };
    return result;
  }

  const opts = entry.options;
  switch (entry.customHandler) {
    case "render_code128_subset":
      return renderCode128Subset(data, opts);
    case "render_ean_addon":
      return renderEanAddon(data, opts);
    case "render_isbn_addon":
      return renderIsbnAddon(data, opts);
    case "render_issn_addon":
      return renderIssnAddon(data, opts);
    case "render_vin":
      return renderVin(data);
    case "render_logmars":
      return { bcid: "code39", text: data.trim().toUpperCase(), options: { includecheck: true, includecheckintext: true } };
    case "render_auspost":
      return renderAuspost(data, opts);
    case "render_cepnet":
      return renderCepnet(data);
    case "render_dpd":
      return renderDpd(data);
    case "render_italian_postal_25":
      return renderItalianPostal25(data);
    case "render_italian_postal_39":
      return { bcid: "code39", text: data.trim().toUpperCase(), options: { includecheck: true } };
    case "render_korean_postal":
      return renderKoreanPostal(data);
    case "render_mailmark_2d":
      return renderMailmark2d(data);
    case "render_swedish_postal":
      return renderSwedishPostal(data);
    case "render_upu_s10":
      return renderUpuS10(data);
    case "render_usps_impb":
      return nonEmpty("IMpb payload is empty", data, "gs1-128", { parse: true });
    case "render_dp_postmatrix":
      return nonEmpty("DP Postmatrix payload is empty", data, "datamatrix", {});
    case "render_ntin":
      return renderNtin(data);
    case "render_ppn":
      return renderPpn(data);
    case "render_hibc_pas":
      return renderHibcPas(data, opts);
    case "render_composite":
      return renderComposite(data, opts);
    default:
      throw new Error(`${entry.name} is not available in this web preview yet.`);
  }
}

export function normalizeOptions(options: Record<string, unknown>): Record<string, unknown> {
  const normalized: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(options)) {
    normalized[key.toLowerCase()] = value;
  }
  return normalized;
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function asNumber(value: unknown, fallback = 0): number {
  return typeof value === "number" ? value : fallback;
}

function stripNonDigits(value: string): string {
  return value.replace(/\D/g, "");
}

function gs1Check(digitsNoCheck: string): string {
  let total = 0;
  [...digitsNoCheck].reverse().forEach((char, index) => {
    const n = Number(char);
    total += n * (index % 2 === 0 ? 3 : 1);
  });
  return String((10 - (total % 10)) % 10);
}

function splitAddon(data: string, addonLength: number): [string, string] {
  const parts = data.trim().split(/\s+/);
  if (parts.length !== 2) {
    throw new Error(`Expected two whitespace-separated parts: main data and ${addonLength}-digit add-on.`);
  }
  const [main, addon] = parts;
  if (!/^\d+$/.test(addon) || addon.length !== addonLength) {
    throw new Error(`Add-on must be exactly ${addonLength} digits.`);
  }
  return [main, addon];
}

function renderCode128Subset(data: string, opts: Record<string, unknown>): ResolvedBarcode {
  const subset = asString(opts.subset, "B").toUpperCase();
  const escapes: Record<string, string> = { A: "^104", B: "^105", C: "^099" };
  if (!escapes[subset]) {
    throw new Error(`Code 128 subset must be A, B, or C.`);
  }
  return { bcid: "code128", text: `${escapes[subset]}${data}`, options: { parse: true } };
}

function renderEanAddon(data: string, opts: Record<string, unknown>): ResolvedBarcode {
  const base = asString(opts.base);
  const addonLength = asNumber(opts.addon_len);
  const map: Record<string, string> = { ean13: "ean13", ean8: "ean8", upca: "upca", upce: "upce" };
  if (!map[base]) {
    throw new Error(`Unsupported add-on base: ${base}.`);
  }
  const [main, addon] = splitAddon(data, addonLength);
  const bwippOptions: Record<string, unknown> = {};
  for (const key of ["permitaddon", "addontextxoffset", "addontextyoffset"]) {
    if (key in opts) {
      bwippOptions[key] = opts[key];
    }
  }
  return { bcid: map[base], text: `${main} ${addon}`, options: bwippOptions };
}

function renderIsbnAddon(data: string, opts: Record<string, unknown>): ResolvedBarcode {
  const [main, addon] = splitAddon(data, asNumber(opts.addon_len));
  return { bcid: "isbn", text: `${main} ${addon}`, options: {} };
}

function renderIssnAddon(data: string, opts: Record<string, unknown>): ResolvedBarcode {
  const [main, addon] = splitAddon(data, asNumber(opts.addon_len));
  return { bcid: "issn", text: `${main} ${addon}`, options: {} };
}

function renderVin(data: string): ResolvedBarcode {
  const vin = data.trim().toUpperCase();
  if (vin.length !== 17) {
    throw new Error(`VIN must be exactly 17 characters; got ${vin.length}.`);
  }
  if (/[IOQ]/.test(vin) || !/^[A-HJ-NPR-Z0-9]{17}$/.test(vin)) {
    throw new Error("VIN contains invalid characters. Letters I, O, and Q are not allowed.");
  }
  return { bcid: "code39", text: vin, options: {} };
}

function renderAuspost(data: string, opts: Record<string, unknown>): ResolvedBarcode {
  const fcc = asString(opts.fcc, "11");
  const payload = data.trim();
  const full = /^(11|45|59|62)/.test(payload) ? payload : `${fcc}${payload}`;
  return { bcid: "auspost", text: full, options: {} };
}

function renderCepnet(data: string): ResolvedBarcode {
  const digits = stripNonDigits(data);
  if (digits.length !== 8) {
    throw new Error("CEPNet payload must be 8 digits.");
  }
  return {
    bcid: "code128",
    text: `CEP${digits}`,
    options: {},
    warning: "CEPNet is rendered through the local project's Code 128 compatibility wrapper.",
  };
}

function renderDpd(data: string): ResolvedBarcode {
  const payload = data.trim();
  if (payload.length < 28) {
    throw new Error(`DPD payload should be at least 28 characters; got ${payload.length}.`);
  }
  return { bcid: "code128", text: payload, options: {} };
}

function renderItalianPostal25(data: string): ResolvedBarcode {
  let digits = stripNonDigits(data);
  if (!digits) {
    throw new Error("Italian Postal 2 of 5 requires digits.");
  }
  if (digits.length % 2) {
    digits = `0${digits}`;
  }
  return { bcid: "interleaved2of5", text: digits, options: {} };
}

function renderKoreanPostal(data: string): ResolvedBarcode {
  const digits = stripNonDigits(data);
  if (digits.length !== 6) {
    throw new Error("Korean Postal code must be exactly 6 digits.");
  }
  const weights = [3, 1, 3, 1, 3, 1];
  const total = [...digits].reduce((sum, digit, index) => sum + Number(digit) * weights[index], 0);
  const check = (10 - (total % 10)) % 10;
  return {
    bcid: "code128",
    text: `KPA${digits}${check}`,
    options: {},
    warning: "Korean Postal is rendered through the local project's Code 128 compatibility wrapper.",
  };
}

function renderMailmark2d(data: string): ResolvedBarcode {
  const payload = data.replace(/\n$/, "");
  if (![45, 70, 90].includes(payload.length)) {
    throw new Error(`Mailmark 2D payload must be 45, 70, or 90 characters; got ${payload.length}.`);
  }
  if (payload.length === 90) {
    return { bcid: "datamatrixrectangular", text: payload, options: { version: "16x48" } };
  }
  return { bcid: "datamatrix", text: payload, options: {} };
}

function renderSwedishPostal(data: string): ResolvedBarcode {
  let digits = stripNonDigits(data);
  if (![17, 18].includes(digits.length)) {
    throw new Error("Swedish postal payload must be 17 or 18 digits.");
  }
  if (digits.length === 17) {
    digits += gs1Check(digits);
  }
  return { bcid: "gs1-128", text: `(00)${digits}`, options: { parse: true } };
}

function renderUpuS10(data: string): ResolvedBarcode {
  const payload = data.trim().toUpperCase();
  if (!/^[A-Z]{2}\d{8,9}[A-Z]{2}$/.test(payload)) {
    throw new Error("UPU S10 format is 2 letters + 8 or 9 digits + 2 letters.");
  }
  return { bcid: "code128", text: payload, options: {} };
}

function renderNtin(data: string): ResolvedBarcode {
  let payload = data.replace(/\s+/g, "");
  if (!payload.startsWith("(")) {
    if (!/^\d+$/.test(payload)) {
      throw new Error("NTIN payload must be numeric.");
    }
    payload = `(8003)${payload}`;
  }
  return { bcid: "gs1datamatrix", text: payload, options: { parse: true } };
}

function renderPpn(data: string): ResolvedBarcode {
  const rs = "\x1e";
  const gs = "\x1d";
  const eot = "\x04";
  return { bcid: "datamatrix", text: `[)>${rs}06${gs}9N${data.trim()}${rs}${eot}`, options: {} };
}

function renderHibcPas(data: string, opts: Record<string, unknown>): ResolvedBarcode {
  const bcid = asString(opts.bwipp_type);
  if (!bcid) {
    throw new Error("HIBC PAS entry is missing bwipp_type.");
  }
  let payload = data.trimStart();
  if (!payload.startsWith("+")) {
    payload = `+${payload}`;
  }
  const result: ResolvedBarcode = { bcid, text: payload, options: {} };
  // HIBC PAS over QR Code: previously surfaced a substrate-divergence
  // warning. Now defaults to the native bwipp-faithful encoder, so
  // no warning is needed.
  return result;
}

function renderComposite(data: string, opts: Record<string, unknown>): ResolvedBarcode {
  const base = asString(opts.base);
  const cc = asString(opts.cc, "A").toLowerCase();
  if (!data.includes("|")) {
    throw new Error("Composite data must contain `|` separating the linear and 2D companion data.");
  }
  const [linear, twod] = data.split("|", 2);
  return {
    bcid: base,
    text: `${linear.trim()}|${twod.trim()}`,
    options: { parse: true, ccversion: cc },
  };
}

function nonEmpty(message: string, data: string, bcid: string, options: Record<string, unknown>): ResolvedBarcode {
  const payload = data.trim();
  if (!payload) {
    throw new Error(message);
  }
  return { bcid, text: payload, options };
}
