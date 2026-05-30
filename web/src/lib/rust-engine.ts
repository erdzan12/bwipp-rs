import type { CatalogEntry } from "./catalog";

export type RustEngine = {
  supportedIds: Set<string>;
  encodeSvg: (symbology: string, data: string, options: RustRenderOptions) => Promise<RustEncodeResult>;
};

export type RustRenderOptions = {
  scale: number;
  barHeight: number;
  quietZone: number;
  includeText: boolean;
  foreground: string;
  background: string;
  extras: Record<string, unknown>;
};

export type RustEncodeResult =
  | {
      ok: true;
      svg: string;
    }
  | {
      ok: false;
      error: string;
    };

type WasmExports = {
  memory: WebAssembly.Memory;
  bwipp_wasm_alloc: (len: number) => number;
  bwipp_wasm_dealloc: (ptr: number, len: number) => void;
  bwipp_wasm_supported_ids: () => number;
  bwipp_wasm_encode_svg: (
    symbologyPtr: number,
    symbologyLen: number,
    dataPtr: number,
    dataLen: number,
    optionsPtr: number,
    optionsLen: number,
  ) => number;
};

const encoder = new TextEncoder();
const decoder = new TextDecoder();

let enginePromise: Promise<RustEngine> | null = null;

export function loadRustEngine(): Promise<RustEngine> {
  enginePromise ??= createRustEngine();
  return enginePromise;
}

export function rustCandidatesFor(entry: CatalogEntry): string[] {
  const aliases: Record<string, string[]> = {
    telepen_alpha: ["telepennumeric"],
    usps_postnet5: ["postnet"],
    usps_postnet9: ["postnet"],
    usps_postnet11: ["postnet"],
    planet12: ["planet"],
    planet14: ["planet"],
  };

  return unique([entry.id, entry.bwippType ?? "", ...(aliases[entry.id] ?? [])].filter(Boolean));
}

export function resolveRustSymbology(entry: CatalogEntry, supportedIds: Set<string>): string | null {
  return rustCandidatesFor(entry).find((candidate) => supportedIds.has(candidate)) ?? null;
}

function unique(values: string[]): string[] {
  return [...new Set(values)];
}

async function createRustEngine(): Promise<RustEngine> {
  const exports = await instantiate();
  const supportedIds = new Set(JSON.parse(takeString(exports, exports.bwipp_wasm_supported_ids())) as string[]);

  return {
    supportedIds,
    encodeSvg: async (symbology, data, options) => {
      const sym = passString(exports, symbology);
      const payload = passString(exports, data);
      const opts = passString(exports, serializeOptions(options));
      try {
        const resultPtr = exports.bwipp_wasm_encode_svg(sym.ptr, sym.len, payload.ptr, payload.len, opts.ptr, opts.len);
        return JSON.parse(takeString(exports, resultPtr)) as RustEncodeResult;
      } finally {
        dropString(exports, sym);
        dropString(exports, payload);
        dropString(exports, opts);
      }
    },
  };
}

async function instantiate(): Promise<WasmExports> {
  const response = await fetch("/wasm/bwipp_wasm.wasm");
  try {
    const { instance } = await WebAssembly.instantiateStreaming(response.clone(), {});
    return instance.exports as WasmExports;
  } catch {
    const bytes = await response.arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, {});
    return instance.exports as WasmExports;
  }
}

function serializeOptions(options: RustRenderOptions): string {
  const lines = [
    `scale=${Math.trunc(options.scale)}`,
    `bar_height=${Math.trunc(options.barHeight)}`,
    `quiet_zone=${Math.trunc(options.quietZone)}`,
    `include_text=${options.includeText ? "1" : "0"}`,
    `foreground=${options.foreground}`,
    `background=${options.background}`,
  ];

  for (const [key, value] of Object.entries(options.extras)) {
    if (value === undefined || value === null) {
      continue;
    }
    lines.push(`extra:${key}=${String(value)}`);
  }

  return lines.join("\n");
}

function passString(exports: WasmExports, value: string): { ptr: number; len: number } {
  const bytes = encoder.encode(value);
  if (bytes.length === 0) {
    return { ptr: 0, len: 0 };
  }
  const ptr = exports.bwipp_wasm_alloc(bytes.length);
  new Uint8Array(exports.memory.buffer, ptr, bytes.length).set(bytes);
  return { ptr, len: bytes.length };
}

function dropString(exports: WasmExports, value: { ptr: number; len: number }) {
  if (value.ptr && value.len) {
    exports.bwipp_wasm_dealloc(value.ptr, value.len);
  }
}

function takeString(exports: WasmExports, ptr: number): string {
  const length = new DataView(exports.memory.buffer, ptr, 4).getUint32(0, true);
  const bytes = new Uint8Array(exports.memory.buffer, ptr + 4, length);
  const text = decoder.decode(bytes);
  exports.bwipp_wasm_dealloc(ptr, length + 4);
  return text;
}
