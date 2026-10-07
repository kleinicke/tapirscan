import type { Format } from "tapirscan";
import type { Region, Result } from "./types";

export const benchmarkVersion = 5;
export const methods = [
  { id: "native", label: "Native browser", color: "#dfcf78" },
  { id: "zxing", label: "ZXing", color: "#58b9ff" },
  { id: "zbar", label: "ZBar", color: "#b7a4ff" },
  { id: "low", label: "Tapirscan Low", color: "#8be56f" },
  { id: "medium", label: "Tapirscan Medium", color: "#ff6f61" },
] as const;
export type Method = (typeof methods)[number]["id"];
export interface Observation {
  regions: Region[];
  scanMs?: number;
  error?: string;
}
export interface Scene {
  name: string;
  tags: string[];
  width: number;
  height: number;
  thumbnail: string;
  observations: Partial<Record<Method, Observation>>;
}
export interface Report {
  version: number;
  created: string;
  browser: string;
  source: string;
  kind: "batch" | "stress";
  status: "running" | "complete" | "cancelled" | "failed";
  totalInputs: number;
  formats: Format[];
  limit: number;
  scenes: Scene[];
  note: string;
  identity?: string;
  engines?: Record<string, string>;
}
export function uniqueCodes(regions: Region[]): number {
  return new Set(regions.map((region) => region.text)).size;
}
export function repeats(regions: Region[]): number {
  return regions.length - new Set(regions.map((region) => region.text)).size;
}
export function statistics(scenes: Scene[], method: Method) {
  const observations = scenes.flatMap((scene) => scene.observations[method] ?? []);
  const successful = observations.filter((observation) => !observation.error);
  const times = successful.flatMap((observation) => observation.scanMs ?? []).sort((a, b) => a - b);
  const total = times.reduce((sum, time) => sum + time, 0);
  return {
    attempted: observations.length,
    completed: successful.length,
    errors: observations.length - successful.length,
    found: successful.filter((observation) => observation.regions.length).length,
    zeroUnique: successful.filter((observation) => uniqueCodes(observation.regions) === 0).length,
    oneUnique: successful.filter((observation) => uniqueCodes(observation.regions) === 1).length,
    multipleUnique: successful.filter((observation) => uniqueCodes(observation.regions) >= 2)
      .length,
    reads: successful.reduce((sum, observation) => sum + observation.regions.length, 0),
    repeated: successful.filter((observation) => repeats(observation.regions)).length,
    extras: successful.reduce((sum, observation) => sum + repeats(observation.regions), 0),
    mean: times.length ? total / times.length : null,
    median: times.length
      ? (times[Math.floor((times.length - 1) / 2)] + times[Math.floor(times.length / 2)]) / 2
      : null,
    p95: times.length ? times[Math.ceil(times.length * 0.95) - 1] : null,
    total,
  };
}
function context(source: HTMLCanvasElement) {
  const result = source.getContext("2d");
  if (!result) throw new Error("Canvas 2D is unavailable");
  return result;
}
function canvas(width: number, height: number) {
  const element = document.createElement("canvas");
  element.width = Math.max(1, Math.round(width));
  element.height = Math.max(1, Math.round(height));
  return element;
}
export function snapshot(source: ImageBitmap, limit: number): HTMLCanvasElement {
  const ratio = Math.min(1, limit / Math.max(source.width, source.height));
  const result = canvas(source.width * ratio, source.height * ratio);
  context(result).drawImage(source, 0, 0, result.width, result.height);
  return result;
}
function thumbnail(source: HTMLCanvasElement) {
  const ratio = Math.min(1, 720 / Math.max(source.width, source.height));
  const result = canvas(source.width * ratio, source.height * ratio);
  context(result).drawImage(source, 0, 0, result.width, result.height);
  return result.toDataURL("image/jpeg", 0.95);
}

/** Independent workers, one image in flight, explicit cancellation and no decoder fallback. */
export class BenchmarkRunner {
  private workers = new Map<Method, Worker>();
  private pending = new Set<(reason: Error) => void>();
  private warm = new Set<Method>();
  private stopped = false;
  private formats: Format[];
  private releaseVersion?: string;
  constructor(formats: Format[], releaseVersion?: string) {
    this.releaseVersion = releaseVersion;
    this.formats = formats;
  }
  private isStopped() {
    return this.stopped;
  }
  cancel() {
    this.stopped = true;
    for (const reject of this.pending) reject(new Error("Benchmark cancelled"));
    this.pending.clear();
    for (const worker of this.workers.values()) worker.terminate();
    this.workers.clear();
  }
  private async scan(method: Method, image: ImageData): Promise<Observation> {
    if (this.stopped) throw new Error("Benchmark cancelled");
    let worker = this.workers.get(method);
    if (!worker) {
      worker =
        method === "medium" || method === "low"
          ? new Worker(new URL("./scan.worker.ts", import.meta.url), { type: "module" })
          : new Worker(new URL("./reference.worker.ts", import.meta.url), { type: "module" });
      this.workers.set(method, worker);
    }
    const target = worker;
    return new Promise((resolve, reject) => {
      const fail = (error: Error) => {
        cleanup();
        target.terminate();
        this.workers.delete(method);
        reject(error);
      };
      const timer = setTimeout(() => {
        fail(new Error("Scan timed out after 60 seconds"));
      }, 60000);
      const cleanup = () => {
        clearTimeout(timer);
        this.pending.delete(fail);
        target.onmessage = null;
        target.onerror = null;
      };
      this.pending.add(fail);
      target.onerror = (event) => {
        fail(new Error(event.message || "Worker failed"));
      };
      target.onmessage = (
        event: MessageEvent<{ type?: string; error?: string; result?: Result }>,
      ) => {
        if (event.data.type === "initializing") return;
        cleanup();
        if (event.data.error) resolve({ regions: [], error: event.data.error });
        else if (event.data.result) {
          const result = event.data.result;
          resolve({
            regions: result.regions.filter((region) => !!region.text),
            scanMs: result.scanMs,
          });
        } else fail(new Error("Invalid worker response"));
      };
      const buffer = image.data.slice().buffer;
      target.postMessage(
        {
          engine: method,
          scannerVersion: method === "low" ? "low" : "medium",
          releaseVersion: this.releaseVersion,
          formats: this.formats,
          benchmark: true,
          finishCandidates: false,
          zxingEnhanced: true,
          engineBaseUrl: new URL(`${import.meta.env.BASE_URL}engines/`, document.baseURI).href,
          width: image.width,
          height: image.height,
          buffer,
        },
        [buffer],
      );
    });
  }
  async scene(
    source: HTMLCanvasElement,
    name: string,
    tags: string[],
    changed: (scene: Scene) => void,
  ): Promise<Scene> {
    const scene: Scene = {
      name,
      tags,
      width: source.width,
      height: source.height,
      thumbnail: thumbnail(source),
      observations: {},
    };
    const image = context(source).getImageData(0, 0, source.width, source.height);
    for (const method of methods) {
      if (this.stopped) throw new Error("Benchmark cancelled");
      try {
        if (!this.warm.has(method.id)) {
          const warmup = await this.scan(method.id, new ImageData(32, 32));
          this.warm.add(method.id);
          if (warmup.error) {
            scene.observations[method.id] = warmup;
            changed(scene);
            continue;
          }
        }
        scene.observations[method.id] = await this.scan(method.id, image);
      } catch (error) {
        if (this.isStopped()) throw error;
        scene.observations[method.id] = {
          regions: [],
          error: error instanceof Error ? error.message : String(error),
        };
      }
      changed(scene);
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
    }
    return scene;
  }
}

export interface Variation {
  name: string;
  tags: string[];
  angle?: number;
  scale?: number;
  x?: number;
  y?: number;
  blur?: number;
  contrast?: number;
  squeeze?: number;
  perspective?: number;
  zoom?: number;
}
export const variations: Variation[] = [
  ...[0.35, 0.6, 0.85].map((zoom) => ({
    name: `Zoom target: ${String(zoom * 100)}% of frame`,
    zoom,
    tags: ["zoomed", "viewport crop"],
  })),
  {
    name: "Zoomed rotation 25°",
    zoom: 0.7,
    angle: 25,
    tags: ["zoomed", "rotated", "viewport crop"],
  },
  {
    name: "Zoomed rotation 75°",
    zoom: 0.7,
    angle: 75,
    tags: ["zoomed", "rotated", "viewport crop"],
  },
  ...[
    { x: -1, y: 0, name: "left" },
    { x: 1, y: 0, name: "right" },
    { x: 0, y: -1, name: "top" },
    { x: 0, y: 1, name: "bottom" },
  ].map((edge) => ({
    ...edge,
    name: `Zoomed barcode near ${edge.name} edge`,
    zoom: 0.65,
    tags: ["zoomed", "edge", "viewport crop"],
  })),
  { name: "Complete image on canvas", tags: ["full image"] },
  ...[10, 25, 45, 80, 90, 115, 160, 180, 225, 270, 315].map((angle) => ({
    name: `Rotation ${String(angle)}°`,
    angle,
    tags: [Math.min(angle % 90, 90 - (angle % 90)) > 9 ? "rotated" : "axis aligned"],
  })),
  ...[0.5, 0.25, 0.12, 0.06].map((scale) => ({
    name: `Scale ${String(scale * 100)}%`,
    scale,
    tags: ["small"],
  })),
  ...[
    { x: -1, y: 0, name: "left" },
    { x: 1, y: 0, name: "right" },
    { x: 0, y: -1, name: "top" },
    { x: 0, y: 1, name: "bottom" },
    { x: -1, y: -1, name: "top left" },
    { x: 1, y: 1, name: "bottom right" },
    { x: 1, y: -1, name: "top right" },
    { x: -1, y: 1, name: "bottom left" },
  ].map((position) => ({ ...position, name: `Edge ${position.name}`, tags: ["edge"] })),
  { name: "Small rotated", scale: 0.2, angle: 35, tags: ["small", "rotated"] },
  { name: "Blur 1.5px", blur: 1.5, tags: ["blurred"] },
  { name: "Low contrast", contrast: 0.25, tags: ["low contrast"] },
  { name: "Horizontal compression", squeeze: 0.4, tags: ["compressed"] },
  { name: "Perspective tilt", perspective: 1, tags: ["perspective"] },
  { name: "Strong perspective tilt", perspective: 2, tags: ["perspective"] },
  {
    name: "Small rotated perspective",
    perspective: 1,
    angle: 35,
    scale: 0.3,
    tags: ["perspective", "rotated", "small"],
  },
];
export const stressNote =
  "Full-image variants preserve every barcode and background. Zoomed variants transform the same photo inside a fixed viewport, centered on the first detected barcode (Medium first), or the image center if none is found. Background and other barcodes can leave the viewport; no code is digitally removed. Zoom targets the limiting frame dimension, capped at 8× magnification; it uses interpolation, not new image detail. The selected barcode gets a 10% margin where its estimated bounds allow.";
export function renderVariation(patch: HTMLCanvasElement, variation: Variation, baseline?: Scene) {
  if (variation.zoom) return renderZoom(patch, variation, baseline);
  const angle = ((variation.angle ?? 0) * Math.PI) / 180;
  const width = Math.max(
    1,
    Math.round(patch.width * (variation.scale ?? 1) * (variation.squeeze ?? 1)),
  );
  const height = Math.max(1, Math.round(patch.height * (variation.scale ?? 1)));
  let small = canvas(width, height);
  context(small).drawImage(patch, 0, 0, patch.width, patch.height, 0, 0, small.width, small.height);
  if (variation.perspective) {
    // Project rows through a pinhole-style homography, preserving source texture.
    const projected = canvas(small.width, small.height);
    const ctx = context(projected);
    const strength = variation.perspective;
    for (let row = 0; row < small.height; row++) {
      const top = row / small.height,
        bottom = (row + 1) / small.height;
      const d0 = 1 + strength * (1 - top),
        d1 = 1 + strength * (1 - bottom);
      const span = small.width / ((d0 + d1) / 2);
      const y0 = (top / d0) * small.height,
        y1 = (bottom / d1) * small.height;
      ctx.drawImage(
        small,
        0,
        row,
        small.width,
        1,
        (small.width - span) / 2,
        y0,
        span,
        y1 - y0 + 0.05,
      );
    }
    small = projected;
  }
  const side = Math.ceil(Math.hypot(patch.width, patch.height)) + 32;
  const result = canvas(side, side),
    ctx = context(result);
  ctx.fillStyle = "white";
  ctx.fillRect(0, 0, result.width, result.height);
  const boundWidth =
    Math.abs(Math.cos(angle)) * small.width + Math.abs(Math.sin(angle)) * small.height;
  const boundHeight =
    Math.abs(Math.sin(angle)) * small.width + Math.abs(Math.cos(angle)) * small.height;
  ctx.translate(
    side / 2 + (variation.x ?? 0) * (side / 2 - boundWidth / 2),
    side / 2 + (variation.y ?? 0) * (side / 2 - boundHeight / 2),
  );
  ctx.rotate(angle);
  ctx.filter = `blur(${String(variation.blur ?? 0)}px) contrast(${String(variation.contrast ?? 1)})`;
  ctx.drawImage(small, -small.width / 2, -small.height / 2);
  return result;
}

/** Move the full source photo through a fixed viewport; never extract a barcode patch. */
function renderZoom(source: HTMLCanvasElement, variation: Variation, baseline?: Scene) {
  const region =
    baseline?.observations.medium?.regions.at(0) ??
    methods.flatMap((method) => baseline?.observations[method.id]?.regions ?? []).at(0);
  const points = region?.polygon ?? [];
  const xs = points.map((point) => point[0]),
    ys = points.map((point) => point[1]);
  const cx = points.length ? (Math.min(...xs) + Math.max(...xs)) / 2 : source.width / 2;
  const cy = points.length ? (Math.min(...ys) + Math.max(...ys)) / 2 : source.height / 2;
  const bw = points.length ? Math.max(1, Math.max(...xs) - Math.min(...xs)) : source.width / 4;
  const bh = points.length ? Math.max(1, Math.max(...ys) - Math.min(...ys)) : source.height / 4;
  const angle = ((variation.angle ?? 0) * Math.PI) / 180;
  const width = Math.abs(Math.cos(angle)) * bw + Math.abs(Math.sin(angle)) * bh;
  const height = Math.abs(Math.sin(angle)) * bw + Math.abs(Math.cos(angle)) * bh;
  const scale = Math.max(
    1,
    Math.min(8, (variation.zoom ?? 0.6) * Math.min(source.width / width, source.height / height)),
  );
  const result = canvas(source.width, source.height),
    ctx = context(result);
  ctx.fillStyle = "white";
  ctx.fillRect(0, 0, result.width, result.height);
  const marginX = Math.min(result.width / 2, width * scale * 0.6);
  const marginY = Math.min(result.height / 2, height * scale * 0.6);
  ctx.translate(
    result.width / 2 + (variation.x ?? 0) * (result.width / 2 - marginX),
    result.height / 2 + (variation.y ?? 0) * (result.height / 2 - marginY),
  );
  ctx.rotate(angle);
  ctx.scale(scale, scale);
  ctx.translate(-cx, -cy);
  ctx.drawImage(source, 0, 0);
  return result;
}
