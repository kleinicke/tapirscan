/** Independent EAN-13 supplied-region scanner. Experimental; no localizer or
 * reference decoder is imported. All result confidence remains uncalibrated. */
export type Point = readonly [number, number];
export type Quad = readonly [Point, Point, Point, Point];
export interface Image { data: Uint8Array; width: number; height: number; channels: 1 | 3 | 4; stride: number }
export interface Policy {
  transitionCleanup?: boolean; sourceIdentity?: boolean; interiorNormalization?: boolean; guardBias?: boolean;
  /** Experimental: accept one full-quiet scanline; increases false-read risk. Default false. */
  allowSingleRow?: boolean;
  maxRetryPathsPerCandidate?: number; maxRetryPathsPerFrame?: number;
  maxAssociationChecks?: number; maxAssociationPixels?: number; maxResults?: number;
}
export interface Detection { text: string; polygon: Quad; support: number; axis: number }
export interface Barcode extends Detection { candidate_indices: number[] }
export interface Observation { text: string; axis: number; fraction: number; left: number; right: number; cost: number; gap: number }
export interface Candidate {
  candidate_index: number; coverage: readonly (readonly [number | null, number | null])[];
  error: boolean; error_detail: 'invalid_geometry' | 'sampling_error' | null;
  unfinished: boolean; ms: number; work: Record<string, number>;
  detections: Detection[]; observations: Observation[];
}
export interface ScanFrame {
  unfinished: boolean; candidates: Candidate[]; barcodes: Barcode[];
  reconciliation: { comparisons: number; merged: number; ambiguous: number; conflicting: number;
    pending_observations: number; truncated: boolean; source_pairs: number; source_matches: number;
    source_pixels: number; source_capped: number; pending_coverage_checks: number; pending_quarantined: number };
}
export interface ScanResult extends ScanFrame {
  /** This WASM build has no internal clock. Candidate ms values are unavailable zeros. */
  candidateTimingsAvailable: false;
  /** Actual host wall time, including validation, preparation/copies, decode and JSON parsing. */
  elapsedMs: number;
}
interface Exports {
  memory: WebAssembly.Memory; regions_version(): number; regions_new(): number; regions_destroy(id: number): number;
  regions_prepare(id: number, w: number, h: number, c: number, stride: number): number;
  regions_input_ptr(id: number): number; regions_input_len(id: number): number; regions_quads_ptr(id: number): number;
  regions_localize(id: number, fitLimit: number): number; regions_output_ptr(id: number): number; regions_output_len(id: number): number;
  regions_scan(id: number, count: number, flags: number, perCandidate: number, perFrame: number, checks: number, pixels: number, results: number): number;
}
/** Explicit indices reject sparse arrays; nonfinite numeric geometry reaches Rust. */
export function isQuadShape(value: unknown): value is Quad {
  if (!Array.isArray(value) || value.length !== 4) return false;
  for (let i=0;i<4;i++) {
    if (!Object.hasOwn(value,i)) return false;
    const point=value[i];
    if (!Array.isArray(point) || point.length!==2) return false;
    for (let j=0;j<2;j++) if (!Object.hasOwn(point,j) || typeof point[j]!=='number') return false;
  }
  return true;
}
export class ScannerError extends Error {
  readonly code: string;
  constructor(code: string, message: string) { super(message); this.name = 'ScannerError'; this.code = code; }
}
function integer(value: number, min: number, max: number, name: string): number {
  if (!Number.isSafeInteger(value) || value < min || value > max) throw new ScannerError('invalid_input', `Invalid ${name}`);
  return value;
}
function status(code: number): void {
  if (code !== 0) throw new ScannerError(`core_${code}`, `Independent scanner rejected operation (${code})`);
}
function parseFrame(text: string, count: number): ScanFrame {
  const v: unknown = JSON.parse(text);
  if (!v || typeof v !== 'object') throw new ScannerError('invalid_output', 'Expected frame object');
  const f = v as Partial<ScanFrame>;
  if (!Array.isArray(f.candidates) || f.candidates.length !== count || !Array.isArray(f.barcodes) || typeof f.unfinished !== 'boolean' || !f.reconciliation) throw new ScannerError('invalid_output', 'Invalid frame shape');
  for (let i = 0; i < f.candidates.length; i++) {
    const c = f.candidates[i];
    if (c.candidate_index !== i || !Array.isArray(c.coverage) || c.coverage.length !== 4 || !Array.isArray(c.observations) || !Array.isArray(c.detections) || typeof c.error !== 'boolean') throw new ScannerError('invalid_output', 'Invalid candidate shape');
  }
  for (const b of f.barcodes) if (!/^\d{13}$/.test(b.text) || !Array.isArray(b.polygon) || b.polygon.length !== 4 || !Array.isArray(b.candidate_indices) || b.candidate_indices.some(i => !Number.isInteger(i) || i < 0 || i >= count)) throw new ScannerError('invalid_output', 'Invalid barcode shape');
  return f as ScanFrame;
}
/** Validate before asynchronous work; returned bytes are owned by the caller. */
export function snapshotImage(image: Image): Image {
  if (!image || typeof image !== 'object') throw new ScannerError('invalid_input', 'Invalid image');
    const width = integer(image.width, 1, 0xffffffff, 'width'), height = integer(image.height, 1, 0xffffffff, 'height');
    if (![1,3,4].includes(image.channels) || !(image.data instanceof Uint8Array)) throw new ScannerError('invalid_input', 'Invalid image buffer/channels');
    const stride = integer(image.stride, width * image.channels, 0xffffffff, 'stride');
    const required = integer((height - 1) * stride + width * image.channels, 1, 128 * 1024 * 1024, 'image length');
    if (image.data.byteLength < required) throw new ScannerError('invalid_input', 'Image buffer is too short');
    return { data: new Uint8Array(image.data.subarray(0, required)), width, height, channels: image.channels, stride };
}
export function snapshotPolicy(policy: Policy): Policy {
  if (!policy || typeof policy !== 'object') throw new ScannerError('invalid_input', 'Invalid policy');
  const copy = { ...policy };
  for (const key of ['transitionCleanup', 'sourceIdentity', 'interiorNormalization', 'guardBias', 'allowSingleRow'] as const) if (copy[key] !== undefined && typeof copy[key] !== 'boolean') throw new ScannerError('invalid_input', `Invalid ${key}`);
  integer(copy.maxRetryPathsPerCandidate ?? 512, 0, 4096, 'candidate budget');
  integer(copy.maxRetryPathsPerFrame ?? 8192, 0, 65536, 'frame budget');
  integer(copy.maxAssociationChecks ?? 200000, 0, 2000000, 'comparison budget');
  integer(copy.maxAssociationPixels ?? 2000000, 0, 16000000, 'pixel budget');
  integer(copy.maxResults ?? 1024, 1, 4096, 'result budget');
  return copy;
}
/** Uncalibrated support ordering; ties preserve spatial output order. */
export function rankBarcodes(barcodes: readonly Barcode[]): Barcode[] {
  return [...barcodes].sort((a, b) => b.support - a.support);
}
export class IndependentScanner {
  #exports: Exports;
  #handle: number;
  private constructor(exports: Exports) {
    this.#exports = exports;
    if (exports.regions_version() !== 1) throw new ScannerError('abi_version', 'Unsupported scanner ABI');
    this.#handle = exports.regions_new();
    if (!this.#handle) throw new ScannerError('capacity', 'Scanner handle capacity exhausted');
  }
  static async create(bytes: BufferSource): Promise<IndependentScanner> {
    const module = await WebAssembly.compile(bytes);
    const instance = await WebAssembly.instantiate(module, {});
    const exports = instance.exports as unknown as Exports;
    for (const name of ['regions_localize','regions_version','regions_new','regions_destroy','regions_prepare','regions_input_ptr','regions_input_len','regions_quads_ptr','regions_output_ptr','regions_output_len','regions_scan']) {
      if (typeof instance.exports[name] !== 'function') throw new ScannerError('abi_shape', `Missing ${name}`);
    }
    if (!(exports.memory instanceof WebAssembly.Memory)) throw new ScannerError('abi_shape', 'Missing memory');
    return new IndependentScanner(exports);
  }
  /** All supplied regions receive the cheap pass; successful reads do not end scanning. */
  scan(image: Image, quads: readonly Quad[], policy: Policy = {}): ScanResult {
    return this.#scan(image, quads, policy, false);
  }
  /** Synchronous transaction: upload once, localize, decode the same owned pixels. */
  scanLocalized(image: Image, policy: Policy = {}, fitLimit = 8, fullFrame = false) {
    const start = performance.now();
    if (!this.#handle) throw new ScannerError('disposed', 'Scanner is disposed');
    if (!image || typeof image !== 'object') throw new ScannerError('invalid_input','Invalid image');
    const width=integer(image.width,3,0xffffffff,'width'),height=integer(image.height,3,0xffffffff,'height');
    if (![1,3,4].includes(image.channels) || !(image.data instanceof Uint8Array)) throw new ScannerError('invalid_input','Invalid image buffer/channels');
    const stride=integer(image.stride,width*image.channels,0xffffffff,'stride');
    const required=integer((height-1)*stride+width*image.channels,1,128*1024*1024,'image length');
    if(image.data.byteLength<required)throw new ScannerError('invalid_input','Image buffer is too short');
    integer(fitLimit,0,8,'shear limit');
    const e=this.#exports,id=this.#handle;
    status(e.regions_prepare(id,width,height,image.channels,stride));
    if(e.regions_input_len(id)!==required)throw new ScannerError('abi_shape','Input allocation mismatch');
    new Uint8Array(e.memory.buffer,e.regions_input_ptr(id),required).set(image.data.subarray(0,required));
    status(e.regions_localize(id,fitLimit));
    const localization=JSON.parse(new TextDecoder().decode(new Uint8Array(e.memory.buffer,e.regions_output_ptr(id),e.regions_output_len(id))));
    if(!Array.isArray(localization.proposals)||localization.proposals.length>32||localization.proposals.some((p:any)=>!isQuadShape(p.polygon)))throw new ScannerError('abi_shape','Invalid localization');
    const searchWindows=fullFrame?[{kind:'full_frame_search',polygon:[[0,0],[width-1,0],[width-1,height-1],[0,height-1]],candidateIndex:localization.proposals.length}]:[];
    const localizationMs=performance.now()-start,decodeStart=performance.now();
    const scan=this.#scan(image,[...localization.proposals.map((p:any)=>p.polygon),...searchWindows.map(p=>p.polygon)],policy,true);
    return {localization,searchWindows,scan,localizationMs,decodingMs:performance.now()-decodeStart,scanMs:performance.now()-start};
  }
  #scan(image: Image, quads: readonly Quad[], policy: Policy, prepared: boolean): ScanResult {
    const start = performance.now();
    if (!this.#handle) throw new ScannerError('disposed', 'Scanner is disposed');
    if (!image || typeof image !== 'object' || !Array.isArray(quads) || !policy || typeof policy !== 'object') throw new ScannerError('invalid_input', 'Invalid scan arguments');
    for (const key of ['transitionCleanup', 'sourceIdentity', 'interiorNormalization', 'guardBias', 'allowSingleRow'] as const) if (policy[key] !== undefined && typeof policy[key] !== 'boolean') throw new ScannerError('invalid_input', `Invalid ${key}`);
    const e = this.#exports, id = this.#handle;
    const width = integer(image.width, 1, 0xffffffff, 'width'), height = integer(image.height, 1, 0xffffffff, 'height');
    if (![1,3,4].includes(image.channels) || !(image.data instanceof Uint8Array)) throw new ScannerError('invalid_input', 'Invalid image buffer/channels');
    const stride = integer(image.stride, width * image.channels, 0xffffffff, 'stride');
    const required = integer((height - 1) * stride + width * image.channels, 1, 128 * 1024 * 1024, 'image length');
    if (image.data.byteLength < required) throw new ScannerError('invalid_input', 'Image buffer is too short');
    integer(quads.length, 0, 64, 'candidate count');
    for (const q of quads) if (!isQuadShape(q)) throw new ScannerError('invalid_input', 'Invalid quad shape');
    const perCandidate = integer(policy.maxRetryPathsPerCandidate ?? 512, 0, 4096, 'candidate budget');
    const perFrame = integer(policy.maxRetryPathsPerFrame ?? 8192, 0, 65536, 'frame budget');
    const checks = integer(policy.maxAssociationChecks ?? 200000, 0, 2000000, 'comparison budget');
    const pixels = integer(policy.maxAssociationPixels ?? 2000000, 0, 16000000, 'pixel budget');
    const results = integer(policy.maxResults ?? 1024, 1, 4096, 'result budget');
    const flags = (policy.transitionCleanup ? 1 : 0) | (policy.sourceIdentity ? 2 : 0) | (policy.interiorNormalization ? 4 : 0) | (policy.guardBias ? 8 : 0) | (policy.allowSingleRow ? 16 : 0);
    if (!prepared) {
    status(e.regions_prepare(id, width, height, image.channels, stride));
    if (e.regions_input_len(id) !== required) throw new ScannerError('abi_shape', 'Input allocation mismatch');
    new Uint8Array(e.memory.buffer, e.regions_input_ptr(id), required).set(image.data.subarray(0, required));
    }
    const coordinates = new Float64Array(e.memory.buffer, e.regions_quads_ptr(id), 512);
    quads.forEach((q: Quad, i: number) => q.forEach((p: Point, j: number) => { coordinates[i * 8 + j * 2] = p[0]; coordinates[i * 8 + j * 2 + 1] = p[1]; }));
    status(e.regions_scan(id, quads.length, flags, perCandidate, perFrame, checks, pixels, results));
    // Scan can grow memory. Never reuse the earlier input/coordinate views.
    const output = new Uint8Array(e.memory.buffer, e.regions_output_ptr(id), e.regions_output_len(id)).slice();
    const frame = parseFrame(new TextDecoder().decode(output), quads.length);
    return { ...frame, candidateTimingsAvailable: false, elapsedMs: performance.now() - start };
  }
  /** Separate convenience; it never changes find-all work or suppresses frame evidence. */
  best(result: ScanFrame): Barcode | undefined {
    return rankBarcodes(result.barcodes)[0];
  }
  dispose(): void { if (this.#handle) { const id = this.#handle; this.#handle = 0; status(this.#exports.regions_destroy(id)); } }
}
