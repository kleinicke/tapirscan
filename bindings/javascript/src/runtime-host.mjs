/** Explicit indices reject sparse arrays; nonfinite numeric geometry reaches Rust. */
export function isQuadShape(value) {
  if (!Array.isArray(value) || value.length !== 4) return false;
  for (let i = 0; i < 4; i++) {
    if (!Object.hasOwn(value, i)) return false;
    const point = value[i];
    if (!Array.isArray(point) || point.length !== 2) return false;
    for (let j = 0; j < 2; j++)
      if (!Object.hasOwn(point, j) || typeof point[j] !== "number") return false;
  }
  return true;
}
export class ScannerError extends Error {
  code;
  constructor(code, message) {
    super(message);
    this.name = "ScannerError";
    this.code = code;
  }
}
function integer(value, min, max, name) {
  if (!Number.isSafeInteger(value) || value < min || value > max)
    throw new ScannerError("invalid_input", `Invalid ${name}`);
  return value;
}
function status(code) {
  if (code !== 0)
    throw new ScannerError(`core_${code}`, `Independent scanner rejected operation (${code})`);
}
function parseFrame(text, count) {
  const v = JSON.parse(text);
  if (!v || typeof v !== "object")
    throw new ScannerError("invalid_output", "Expected frame object");
  const f = v;
  if (
    !Array.isArray(f.candidates) ||
    f.candidates.length !== count ||
    !Array.isArray(f.barcodes) ||
    typeof f.unfinished !== "boolean" ||
    !f.reconciliation
  )
    throw new ScannerError("invalid_output", "Invalid frame shape");
  for (let i = 0; i < f.candidates.length; i++) {
    const c = f.candidates[i];
    if (
      c.candidate_index !== i ||
      !Array.isArray(c.coverage) ||
      c.coverage.length !== 4 ||
      !Array.isArray(c.observations) ||
      !Array.isArray(c.detections) ||
      typeof c.error !== "boolean"
    )
      throw new ScannerError("invalid_output", "Invalid candidate shape");
  }
  for (const b of f.barcodes)
    if (
      !/^\d{13}$/.test(b.text) ||
      !Array.isArray(b.polygon) ||
      b.polygon.length !== 4 ||
      !Array.isArray(b.candidate_indices) ||
      b.candidate_indices.some((i) => !Number.isInteger(i) || i < 0 || i >= count)
    )
      throw new ScannerError("invalid_output", "Invalid barcode shape");
  return f;
}
/** Validate before asynchronous work; returned bytes are owned by the caller. */
export function snapshotImage(image) {
  if (!image || typeof image !== "object") throw new ScannerError("invalid_input", "Invalid image");
  const width = integer(image.width, 1, 0xffffffff, "width"),
    height = integer(image.height, 1, 0xffffffff, "height");
  if (![1, 3, 4].includes(image.channels) || !(image.data instanceof Uint8Array))
    throw new ScannerError("invalid_input", "Invalid image buffer/channels");
  const stride = integer(image.stride, width * image.channels, 0xffffffff, "stride");
  const required = integer(
    (height - 1) * stride + width * image.channels,
    1,
    128 * 1024 * 1024,
    "image length",
  );
  if (image.data.byteLength < required)
    throw new ScannerError("invalid_input", "Image buffer is too short");
  return {
    data: new Uint8Array(image.data.subarray(0, required)),
    width,
    height,
    channels: image.channels,
    stride,
  };
}
export function snapshotPolicy(policy) {
  if (!policy || typeof policy !== "object")
    throw new ScannerError("invalid_input", "Invalid policy");
  const copy = { ...policy };
  for (const key of [
    "transitionCleanup",
    "sourceIdentity",
    "interiorNormalization",
    "guardBias",
    "allowSingleRow",
  ])
    if (copy[key] !== undefined && typeof copy[key] !== "boolean")
      throw new ScannerError("invalid_input", `Invalid ${key}`);
  integer(copy.maxRetryPathsPerCandidate ?? 512, 0, 4096, "candidate budget");
  integer(copy.maxRetryPathsPerFrame ?? 8192, 0, 65536, "frame budget");
  integer(copy.maxAssociationChecks ?? 200000, 0, 2000000, "comparison budget");
  integer(copy.maxAssociationPixels ?? 2000000, 0, 16000000, "pixel budget");
  integer(copy.maxResults ?? 1024, 1, 4096, "result budget");
  return copy;
}
/** Uncalibrated support ordering; ties preserve spatial output order. */
export function rankBarcodes(barcodes) {
  return [...barcodes].sort((a, b) => b.support - a.support);
}

/** Maintained ABI profiles. Callers select the behavior their WASM recipe supports. */
export const runtimeHostOptions = Object.freeze({
  base: Object.freeze({
    maxProposals: 32,
    scanAbi: "basic",
    completion: false,
    retail: false,
  }),
  completion: Object.freeze({
    maxProposals: 32,
    scanAbi: "basic",
    completion: true,
    retail: false,
  }),
  wide: Object.freeze({
    maxProposals: 64,
    scanAbi: "basic",
    completion: false,
    retail: false,
  }),
  detail: Object.freeze({
    maxProposals: 64,
    scanAbi: "masked",
    completion: true,
    retail: true,
  }),
});

function validateHostOptions(options) {
  if (!options || typeof options !== "object")
    throw new ScannerError("invalid_input", "Invalid runtime host options");
  integer(options.maxProposals, 1, 64, "proposal limit");
  if (!["basic", "masked"].includes(options.scanAbi))
    throw new ScannerError("invalid_input", "Invalid scan ABI");
  if (typeof options.completion !== "boolean" || typeof options.retail !== "boolean")
    throw new ScannerError("invalid_input", "Invalid runtime host capability options");
  return Object.freeze({ ...options });
}

export class IndependentScanner {
  #exports;
  #handle;
  #options;
  constructor(exports, options) {
    this.#exports = exports;
    this.#options = options;
    if (exports.regions_version() !== 1)
      throw new ScannerError("abi_version", "Unsupported scanner ABI");
    this.#handle = exports.regions_new();
    if (!this.#handle) throw new ScannerError("capacity", "Scanner handle capacity exhausted");
  }
  static async create(bytes, options = runtimeHostOptions.base) {
    const selected = validateHostOptions(options);
    const module = await WebAssembly.compile(bytes);
    const instance = await WebAssembly.instantiate(module, {});
    const exports = instance.exports;
    for (const name of [
      "regions_localize",
      "regions_version",
      "regions_new",
      "regions_destroy",
      "regions_prepare",
      "regions_input_ptr",
      "regions_input_len",
      "regions_quads_ptr",
      "regions_output_ptr",
      "regions_output_len",
      "regions_scan",
    ]) {
      if (typeof instance.exports[name] !== "function")
        throw new ScannerError("abi_shape", `Missing ${name}`);
    }
    if (!(exports.memory instanceof WebAssembly.Memory))
      throw new ScannerError("abi_shape", "Missing memory");
    return new IndependentScanner(exports, selected);
  }
  /** All supplied regions receive the cheap pass; successful reads do not end scanning. */
  scan(image, quads, policy = {}) {
    return this.#scan(image, quads, policy, false);
  }
  /** Synchronous transaction: upload once, localize, decode the same owned pixels. */
  scanLocalized(image, policy = {}, fitLimit = 8, fullFrame = false, transform = undefined) {
    const start = performance.now();
    if (!this.#handle) throw new ScannerError("disposed", "Scanner is disposed");
    if (!image || typeof image !== "object")
      throw new ScannerError("invalid_input", "Invalid image");
    const width = integer(image.width, 3, 0xffffffff, "width"),
      height = integer(image.height, 3, 0xffffffff, "height");
    if (![1, 3, 4].includes(image.channels) || !(image.data instanceof Uint8Array))
      throw new ScannerError("invalid_input", "Invalid image buffer/channels");
    const stride = integer(image.stride, width * image.channels, 0xffffffff, "stride");
    const required = integer(
      (height - 1) * stride + width * image.channels,
      1,
      128 * 1024 * 1024,
      "image length",
    );
    if (image.data.byteLength < required)
      throw new ScannerError("invalid_input", "Image buffer is too short");
    integer(fitLimit, 0, 8, "shear limit");
    const e = this.#exports,
      id = this.#handle;
    status(e.regions_prepare(id, width, height, image.channels, stride));
    if (e.regions_input_len(id) !== required)
      throw new ScannerError("abi_shape", "Input allocation mismatch");
    new Uint8Array(e.memory.buffer, e.regions_input_ptr(id), required).set(
      image.data.subarray(0, required),
    );
    status(e.regions_localize(id, fitLimit));
    let localization = JSON.parse(
      new TextDecoder().decode(
        new Uint8Array(e.memory.buffer, e.regions_output_ptr(id), e.regions_output_len(id)),
      ),
    );
    if (transform) localization = transform(localization);
    if (localization.retryMask) policy = { ...policy, retryMask: localization.retryMask };
    if (
      !Array.isArray(localization.proposals) ||
      localization.proposals.length > Math.min(this.#options.maxProposals, fullFrame ? 63 : 64) ||
      localization.proposals.some((p) => !isQuadShape(p.polygon))
    )
      throw new ScannerError("abi_shape", "Invalid localization");
    const searchWindows = fullFrame
      ? [
          {
            kind: "full_frame_search",
            polygon: [
              [0, 0],
              [width - 1, 0],
              [width - 1, height - 1],
              [0, height - 1],
            ],
            candidateIndex: localization.proposals.length,
          },
        ]
      : [];
    const localizationMs = performance.now() - start,
      decodeStart = performance.now();
    const scan = this.#scan(
      image,
      [...localization.proposals.map((p) => p.polygon), ...searchWindows.map((p) => p.polygon)],
      policy,
      true,
    );
    return {
      localization,
      searchWindows,
      scan,
      localizationMs,
      decodingMs: performance.now() - decodeStart,
      scanMs: performance.now() - start,
    };
  }
  #scan(image, quads, policy, prepared) {
    const start = performance.now();
    if (!this.#handle) throw new ScannerError("disposed", "Scanner is disposed");
    if (
      !image ||
      typeof image !== "object" ||
      !Array.isArray(quads) ||
      !policy ||
      typeof policy !== "object"
    )
      throw new ScannerError("invalid_input", "Invalid scan arguments");
    for (const key of [
      "transitionCleanup",
      "sourceIdentity",
      "interiorNormalization",
      "guardBias",
      "allowSingleRow",
    ])
      if (policy[key] !== undefined && typeof policy[key] !== "boolean")
        throw new ScannerError("invalid_input", `Invalid ${key}`);
    const e = this.#exports,
      id = this.#handle;
    const width = integer(image.width, 1, 0xffffffff, "width"),
      height = integer(image.height, 1, 0xffffffff, "height");
    if (![1, 3, 4].includes(image.channels) || !(image.data instanceof Uint8Array))
      throw new ScannerError("invalid_input", "Invalid image buffer/channels");
    const stride = integer(image.stride, width * image.channels, 0xffffffff, "stride");
    const required = integer(
      (height - 1) * stride + width * image.channels,
      1,
      128 * 1024 * 1024,
      "image length",
    );
    if (image.data.byteLength < required)
      throw new ScannerError("invalid_input", "Image buffer is too short");
    integer(quads.length, 0, 64, "candidate count");
    for (const q of quads)
      if (!isQuadShape(q)) throw new ScannerError("invalid_input", "Invalid quad shape");
    const perCandidate = integer(
      policy.maxRetryPathsPerCandidate ?? 512,
      0,
      4096,
      "candidate budget",
    );
    const perFrame = integer(policy.maxRetryPathsPerFrame ?? 8192, 0, 65536, "frame budget");
    const checks = integer(policy.maxAssociationChecks ?? 200000, 0, 2000000, "comparison budget");
    const pixels = integer(policy.maxAssociationPixels ?? 2000000, 0, 16000000, "pixel budget");
    const results = integer(policy.maxResults ?? 1024, 1, 4096, "result budget");
    if (
      this.#options.completion &&
      policy.finishCandidates &&
      e.regions_completion_supported?.() !== 1
    )
      throw new Error("This WASM build does not support finishCandidates");
    const flags =
      (policy.transitionCleanup ? 1 : 0) |
      (policy.sourceIdentity ? 2 : 0) |
      (policy.interiorNormalization ? 4 : 0) |
      (policy.guardBias ? 8 : 0) |
      (policy.allowSingleRow ? 16 : 0) |
      (this.#options.completion && policy.finishCandidates ? 32 : 0);
    if (!prepared) {
      status(e.regions_prepare(id, width, height, image.channels, stride));
      if (e.regions_input_len(id) !== required)
        throw new ScannerError("abi_shape", "Input allocation mismatch");
      new Uint8Array(e.memory.buffer, e.regions_input_ptr(id), required).set(
        image.data.subarray(0, required),
      );
    }
    const coordinates = new Float64Array(e.memory.buffer, e.regions_quads_ptr(id), 512);
    quads.forEach((q, i) =>
      q.forEach((p, j) => {
        coordinates[i * 8 + j * 2] = p[0];
        coordinates[i * 8 + j * 2 + 1] = p[1];
      }),
    );
    if (this.#options.retail && e.regions_retail)
      status(e.regions_retail(id, policy.retailMask ?? 1, policy.retailMask ? 3 : 0));
    if (this.#options.scanAbi === "masked") {
      const mask = policy.retryMask ?? [4294967295, 4294967295];
      if (!Array.isArray(mask) || mask.length !== 2)
        throw new ScannerError("invalid_input", "Invalid retry mask");
      for (const value of mask) integer(value, 0, 4294967295, "retry mask");
      // The pinned Low recovery decoder has no masked ABI. Preserve its basic
      // scan fallback when the caller did not request explicit retry scheduling.
      if (!e.regions_scan_mask && policy.retryMask)
        throw new ScannerError("abi_shape", "Missing retry scheduling ABI");
      if (e.regions_scan_mask)
        status(
          e.regions_scan_mask(
            id,
            quads.length,
            flags,
            perCandidate,
            perFrame,
            checks,
            pixels,
            results,
            mask[0],
            mask[1],
          ),
        );
      else
        status(
          e.regions_scan(id, quads.length, flags, perCandidate, perFrame, checks, pixels, results),
        );
    } else {
      status(
        e.regions_scan(id, quads.length, flags, perCandidate, perFrame, checks, pixels, results),
      );
    }
    // Scan can grow memory. Never reuse the earlier input/coordinate views.
    const output = new Uint8Array(
      e.memory.buffer,
      e.regions_output_ptr(id),
      e.regions_output_len(id),
    ).slice();
    const frame = parseFrame(new TextDecoder().decode(output), quads.length);
    return { ...frame, candidateTimingsAvailable: false, elapsedMs: performance.now() - start };
  }
  /** Separate convenience; it never changes find-all work or suppresses frame evidence. */
  best(result) {
    return rankBarcodes(result.barcodes)[0];
  }
  dispose() {
    if (this.#handle) {
      const id = this.#handle;
      this.#handle = 0;
      status(this.#exports.regions_destroy(id));
    }
  }
}
