"use client";

import { useEffect, useMemo, useState } from "react";
import bwipjs from "bwip-js/browser";
import {
  AlertTriangle,
  Barcode,
  FileCode2,
  ImageDown,
  Search,
  Settings2,
} from "lucide-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { catalog, categories, type CatalogEntry } from "@/lib/catalog";
import { downloadBlob, safeFilename, svgToPngBlob } from "@/lib/download";
import { resolveBarcode } from "@/lib/resolve";
import { loadRustEngine, resolveRustSymbology, type RustEngine } from "@/lib/rust-engine";

// bwip-js emits `<svg viewBox="0 0 W H">` with no width/height, so the element
// has no intrinsic size: it fills the container width and the preview's
// `max-height` clamp then squashes it (stretched, wider than tall). Give it an
// intrinsic size from the viewBox so it scales with a fixed aspect ratio,
// matching the Rust encoder (which already emits width/height). No-op when the
// SVG already declares a width.
function ensureSvgDimensions(svg: string): string {
  if (/<svg[^>]*\swidth=/i.test(svg)) {
    return svg;
  }
  const vb = svg.match(
    /<svg[^>]*\sviewBox=["']\s*[\d.]+\s+[\d.]+\s+([\d.]+)\s+([\d.]+)\s*["']/i,
  );
  if (!vb) {
    return svg;
  }
  const [, w, h] = vb;
  return svg.replace(
    /<svg\b/i,
    `<svg width="${w}" height="${h}" preserveAspectRatio="xMidYMid meet"`,
  );
}

type EngineId = "bwip-js" | "rust-wasm";

type RenderResult =
  | {
      ok: true;
      svg: string;
      resolvedBcid: string;
      warning?: string;
    }
  | {
      ok: false;
      error: string;
    };

export function BarcodeWorkbench() {
  const [selectedId, setSelectedId] = useState("qrcode");
  const selected = catalog.find((entry) => entry.id === selectedId) ?? catalog[0];
  // Rust/WASM is the primary renderer; bwip-js stays available as a
  // comparison engine so users can diff outputs side-by-side, but it is
  // explicitly *not* the default — see AGENT_GOAL.md.
  const [engine, setEngine] = useState<EngineId>("rust-wasm");
  const [payload, setPayload] = useState(selected.example);
  const [query, setQuery] = useState("");
  const [scale, setScale] = useState(3);
  const [height, setHeight] = useState(12);
  const [quietZone, setQuietZone] = useState(8);
  const [includeText, setIncludeText] = useState(false);
  const [foreground, setForeground] = useState("#111827");
  const [background, setBackground] = useState("#ffffff");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [advancedJson, setAdvancedJson] = useState("{}");
  const [rustEngine, setRustEngine] = useState<RustEngine | null>(null);
  const [rustLoadError, setRustLoadError] = useState<string | null>(null);
  const [rustResult, setRustResult] = useState<RenderResult>({
    ok: false,
    error: "Rust WASM is loading.",
  });

  useEffect(() => {
    setPayload(selected.example);
    setAdvancedJson(JSON.stringify(selected.options, null, 2));
  }, [selected]);

  useEffect(() => {
    let cancelled = false;
    loadRustEngine()
      .then((loaded) => {
        if (!cancelled) {
          setRustEngine(loaded);
          setRustLoadError(null);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setRustLoadError(error instanceof Error ? error.message : String(error));
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const filteredByCategory = useMemo(() => {
    const term = query.trim().toLowerCase();
    const filtered = term
      ? catalog.filter((entry) =>
          [entry.id, entry.name, entry.category, entry.bwippType ?? "", entry.notes]
            .join(" ")
            .toLowerCase()
            .includes(term),
        )
      : catalog;

    return categories
      .map((category) => ({
        category,
        entries: filtered.filter((entry) => entry.category === category),
      }))
      .filter((group) => group.entries.length > 0);
  }, [query]);

  const jsResult = useMemo<RenderResult>(() => {
    try {
      const advanced = advancedJson.trim() ? JSON.parse(advancedJson) : {};
      const resolved = resolveBarcode(selected, payload);
      // `height` is a linear bar-height option. bwip-js also applies it to 2D
      // matrix symbols, squishing them into a non-square box (a stretched QR,
      // Data Matrix, Aztec, …). The Rust encoder ignores it for these, so omit
      // it here too for matrix codes — by category, plus a bcid keyword match
      // to catch HIBC/GS1 matrix variants — so they render at their natural
      // (square / fixed) aspect ratio.
      const isMatrix =
        selected.category === "2D - Matrix" ||
        selected.category === "2D - Specialty" ||
        /(qrcode|datamatrix|aztec|maxicode|dotcode|hanxin|codeone)/i.test(resolved.bcid);
      const options = {
        scale,
        ...(isMatrix ? {} : { height }),
        includetext: includeText,
        textxalign: "center",
        paddingwidth: quietZone,
        paddingheight: quietZone,
        backgroundcolor: background,
        barcolor: foreground,
        textcolor: foreground,
        ...resolved.options,
        ...advanced,
        bcid: resolved.bcid,
        text: resolved.text,
      };
      const svg = bwipjs.toSVG(options);
      return {
        ok: true,
        svg,
        resolvedBcid: resolved.bcid,
        warning: resolved.warning,
      };
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }, [advancedJson, background, foreground, height, includeText, payload, quietZone, scale, selected]);

  const rustSymbology = useMemo(() => {
    if (!rustEngine) {
      return null;
    }
    return resolveRustSymbology(selected, rustEngine.supportedIds);
  }, [rustEngine, selected]);

  useEffect(() => {
    if (engine !== "rust-wasm") {
      return;
    }

    if (rustLoadError) {
      setRustResult({ ok: false, error: `Rust WASM failed to load: ${rustLoadError}` });
      return;
    }

    const activeRustEngine = rustEngine;
    const activeRustSymbology = rustSymbology;

    if (!activeRustEngine) {
      setRustResult({ ok: false, error: "Rust WASM is loading." });
      return;
    }

    if (!activeRustSymbology) {
      setRustResult({
        ok: false,
        error: `${selected.name} is not implemented in the Rust WASM snapshot yet.`,
      });
      return;
    }

    const readyRustEngine: RustEngine = activeRustEngine;
    const readyRustSymbology: string = activeRustSymbology;

    let cancelled = false;
    async function renderWithRust() {
      try {
        const advanced = advancedJson.trim() ? JSON.parse(advancedJson) : {};
        const result = await readyRustEngine.encodeSvg(readyRustSymbology, payload, {
          scale,
          barHeight: height,
          quietZone,
          includeText,
          foreground,
          background,
          extras: { ...selected.options, ...advanced },
        });

        if (cancelled) {
          return;
        }

        if (result.ok) {
          setRustResult({
            ok: true,
            svg: result.svg,
            resolvedBcid: readyRustSymbology,
          });
        } else {
          setRustResult({
            ok: false,
            error: result.error,
          });
        }
      } catch (error) {
        if (!cancelled) {
          setRustResult({
            ok: false,
            error: error instanceof Error ? error.message : String(error),
          });
        }
      }
    }

    renderWithRust();
    return () => {
      cancelled = true;
    };
  }, [
    advancedJson,
    background,
    engine,
    foreground,
    height,
    includeText,
    payload,
    quietZone,
    rustEngine,
    rustLoadError,
    rustSymbology,
    scale,
    selected,
  ]);

  const result = engine === "rust-wasm" ? rustResult : jsResult;

  function downloadSvg() {
    if (!result.ok) {
      return;
    }
    downloadBlob(`${safeFilename(selected.id)}.svg`, result.svg, "image/svg+xml;charset=utf-8");
  }

  async function downloadPng() {
    if (!result.ok) {
      return;
    }
    try {
      const blob = await svgToPngBlob(result.svg);
      downloadBlob(`${safeFilename(selected.id)}.png`, blob, "image/png");
    } catch {
      // The visible render path already shows the actionable error.
    }
  }

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark">
            <Barcode size={20} strokeWidth={2.2} />
          </span>
          <div>
            <h1>Barcode Studio</h1>
            <p>{catalog.length} BWIPP catalog modes</p>
          </div>
        </div>
        <div className="engine-cluster">
          <div className="engine-switch" role="group" aria-label="Rendering engine">
            <Button
              type="button"
              size="sm"
              variant={engine === "rust-wasm" ? "default" : "secondary"}
              onClick={() => setEngine("rust-wasm")}
              title="Primary renderer (Rust compiled to WebAssembly)"
            >
              Rust WASM (default)
            </Button>
            <Button
              type="button"
              size="sm"
              variant={engine === "bwip-js" ? "default" : "secondary"}
              onClick={() => setEngine("bwip-js")}
              title="Reference / comparison renderer (bwip-js)"
            >
              bwip-js (comparison)
            </Button>
          </div>
          <div className={engine === "rust-wasm" ? "engine-pill rust" : "engine-pill"}>
            {engine === "rust-wasm"
              ? `${rustEngine?.supportedIds.size ?? 0} Rust modes`
              : `${catalog.length} JS modes (reference)`}
          </div>
        </div>
      </header>

      <section className="workspace">
        <aside className="sidebar" aria-label="Barcode settings">
          <div className="field">
            <Label htmlFor="mode-search">
              <Search size={15} />
              Search
            </Label>
            <Input
              id="mode-search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="QR, DotCode, GS1..."
            />
          </div>

          <div className="field">
            <Label htmlFor="mode-select">
              <Barcode size={15} />
              Mode
            </Label>
            <select
              id="mode-select"
              className="native-select"
              value={selectedId}
              onChange={(event) => setSelectedId(event.target.value)}
            >
              {filteredByCategory.map((group) => (
                <optgroup key={group.category} label={group.category}>
                  {group.entries.map((entry) => (
                    <option key={entry.id} value={entry.id}>
                      {entry.name}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </div>

          <ModeMeta
            entry={selected}
            engine={engine}
            resolvedBcid={result.ok ? result.resolvedBcid : selected.bwippType ?? "custom"}
            rustAvailable={Boolean(rustSymbology)}
          />

          <div className="field">
            <Label htmlFor="payload">Payload</Label>
            <Textarea id="payload" value={payload} onChange={(event) => setPayload(event.target.value)} rows={7} />
          </div>

          <div className="option-grid">
            <NumberField id="scale" label="Scale" value={scale} min={1} max={8} onChange={setScale} />
            <NumberField id="height" label="Height" value={height} min={1} max={80} onChange={setHeight} />
            <NumberField id="quiet" label="Quiet" value={quietZone} min={0} max={40} onChange={setQuietZone} />
          </div>

          <div className="color-row">
            <label>
              Ink
              <Input type="color" value={foreground} onChange={(event) => setForeground(event.target.value)} />
            </label>
            <label>
              Paper
              <Input type="color" value={background} onChange={(event) => setBackground(event.target.value)} />
            </label>
          </div>

          <Label className="checkbox-row">
            <Checkbox checked={includeText} onCheckedChange={(checked) => setIncludeText(Boolean(checked))} />
            Human-readable text
          </Label>

          <Button variant="secondary" type="button" onClick={() => setAdvancedOpen((open) => !open)}>
            <Settings2 size={16} />
            Advanced options
          </Button>

          {advancedOpen ? (
            <div className="field">
              <Label htmlFor="advanced-json">BWIPP options JSON</Label>
              <Textarea
                id="advanced-json"
                className="mono"
                value={advancedJson}
                onChange={(event) => setAdvancedJson(event.target.value)}
                rows={7}
                spellCheck={false}
              />
            </div>
          ) : null}
        </aside>

        <section className="preview-panel" aria-label="Barcode preview">
          <div className="preview-toolbar">
            <div>
              <h2>{selected.name}</h2>
              <p>{selected.id}</p>
            </div>
            <div className="actions">
              <Button type="button" onClick={downloadSvg} disabled={!result.ok}>
                <FileCode2 size={16} />
                SVG
              </Button>
              <Button type="button" onClick={downloadPng} disabled={!result.ok}>
                <ImageDown size={16} />
                PNG
              </Button>
            </div>
          </div>

          {result.ok ? (
            <>
              {result.warning ? (
                <Alert className="warning">
                  <AlertTriangle size={16} />
                  <AlertDescription>{result.warning}</AlertDescription>
                </Alert>
              ) : null}
              <div className="preview-surface">
                <div className="svg-frame" dangerouslySetInnerHTML={{ __html: ensureSvgDimensions(result.svg) }} />
              </div>
            </>
          ) : (
            <Alert variant="destructive" className="error-box">
              <AlertTriangle size={18} />
              <AlertDescription>{result.error}</AlertDescription>
            </Alert>
          )}
        </section>
      </section>
    </main>
  );
}

function ModeMeta({
  entry,
  engine,
  resolvedBcid,
  rustAvailable,
}: {
  entry: CatalogEntry;
  engine: EngineId;
  resolvedBcid: string;
  rustAvailable: boolean;
}) {
  return (
    <Card className="mode-meta">
      <CardContent className="mode-meta-content">
      <div>
        <span>Catalog</span>
        <Badge variant="secondary">{entry.renderer}</Badge>
      </div>
      <div>
        <span>Engine</span>
        <Badge variant={engine === "rust-wasm" ? "default" : "secondary"}>{engine}</Badge>
      </div>
      <div>
        <span>Rust</span>
        <Badge variant={rustAvailable ? "default" : "secondary"}>{rustAvailable ? "available" : "pending"}</Badge>
      </div>
      <div>
        <span>Encoder</span>
        <strong>{resolvedBcid}</strong>
      </div>
      {entry.notes ? <p>{entry.notes}</p> : null}
      </CardContent>
    </Card>
  );
}

function NumberField({
  id,
  label,
  value,
  min,
  max,
  onChange,
}: {
  id: string;
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  return (
    <Label className="number-field" htmlFor={id}>
      {label}
      <Input
        id={id}
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </Label>
  );
}
