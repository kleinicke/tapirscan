// tapirscan/camera: an optional camera loop for any tapirscan/browser scanner.
// It owns acquisition, frame scheduling and cleanup; decoding stays in the scanner.
import { freeze } from "./freeze.js";
import { handOver } from "./handover.js";
import type { Barcode, Quad, ScanOptions, ScanResult } from "./index.js";

/**
 * Any tapirscan/browser `Scanner`; the controller never disposes it. With
 * `region`, `scan` receives an `ImageBitmap` of the region instead of the video;
 * a tapirscan/browser `Scanner` takes it over instead of copying it.
 */
export interface FrameScanner {
  scan(source: HTMLVideoElement | ImageBitmap, options?: ScanOptions): Promise<ScanResult>;
}

/** Part of the video frame, in fractions (0 to 1) of its width and height. */
export interface ScanRegion {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Which camera to open. Without a `deviceId`, the browser picks by `facingMode`. */
export interface CameraSelection {
  /** A `deviceId` from `CameraScanner.cameras()`; required to match. */
  deviceId?: string;
  /** Preferred direction when no `deviceId` is given; defaults to "environment". */
  facingMode?: "environment" | "user";
}

/** @experimental Options of the `CameraScanner` preview. */
export interface CameraScannerOptions {
  /** Called with every scanned frame; `result.barcodes` is empty when nothing was read. */
  onScan?(result: ScanResult): void;
  /**
   * Called once per appearance of a barcode, after `confirmations` consecutive
   * scans read it with the same format and text. It counts as gone, and is
   * reported again when it returns, only after it is missing from at least 3
   * consecutive scans and for at least 500 ms, so a blurred frame does not repeat
   * it. It is also reported again when the camera reopens. Barcodes with the same
   * format and text count as one: `onDetect` fires once while any of them stays
   * in view.
   */
  onDetect?(barcode: Barcode): void;
  /**
   * Consecutive scans that must read a barcode before `onDetect`. Default 1. A scan
   * without a barcode that was not yet reported restarts its count.
   */
  confirmations?: number;
  /**
   * Minimum milliseconds from the start of one scan to the start of the next.
   * Default 0: scan the next frame as soon as a scan finishes.
   */
  scanInterval?: number;
  /**
   * Scan only this part of each frame, such as the area inside a viewfinder.
   * Results keep the coordinates of the whole video frame.
   */
  region?: ScanRegion;
  /**
   * Called when scanning fails after `start()` resolved: the scanner failed or was
   * disposed, the camera ended, the video lost its stream, or resuming after the
   * page was hidden failed. Scanning has stopped and the camera is released.
   * Without `onError`, the error goes to the global error handler (`reportError`).
   */
  onError?(error: Error): void;
  /** Options for every frame, such as a smaller `formats` selection. */
  scanOptions?: ScanOptions;
  /**
   * Extra video constraints. A `width` or `height` replaces the default 1280×720
   * ideal size; smaller frames scan faster. Choose the camera with `start()`.
   */
  constraints?: Omit<MediaTrackConstraints, "deviceId" | "facingMode">;
  /** Release the camera while the page is hidden and reopen it when shown. Default true. */
  pauseWhenHidden?: boolean;
}

/** `torch` is missing from the DOM library's constraint and capability types. */
type TorchCapabilities = MediaTrackCapabilities & { torch?: boolean };

/**
 * @experimental Preview: may change in minor releases.
 *
 * Scans a live camera into a `<video>` element with a tapirscan/browser scanner.
 * One frame is scanned at a time, so slow devices skip frames instead of queueing
 * them. After `stop()`, no further results arrive, and a camera that is granted
 * only after `stop()` is released at once.
 */
export class CameraScanner {
  readonly #video: HTMLVideoElement;
  readonly #scanner: FrameScanner;
  readonly #options: CameraScannerOptions;
  /**
   * Increments on every start, stop and visibility change; work from an older
   * session is dropped.
   */
  #session = 0;
  #stream: MediaStream | undefined;
  /**
   * The pending camera request, if any. It settles once the request does; a stream
   * granted after its session ended is stopped by then.
   */
  #request: Promise<void> | undefined;
  /** Removes the open stream's video listeners. */
  #watching: AbortController | undefined;
  /** The camera to reopen when the page is shown again; undefined when stopped. */
  #selection: CameraSelection | undefined;
  readonly #onVisibility = () => {
    this.#visibilityChanged();
  };

  constructor(video: HTMLVideoElement, scanner: FrameScanner, options: CameraScannerOptions = {}) {
    if (typeof HTMLVideoElement === "undefined" || !(video instanceof HTMLVideoElement))
      throw new TypeError("CameraScanner needs a <video> element");
    const input: unknown = scanner;
    if (typeof (input as Partial<FrameScanner> | null)?.scan !== "function")
      throw new TypeError("CameraScanner needs a tapirscan/browser Scanner");
    const { constraints = {}, confirmations = 1, scanInterval = 0, region } = options;
    if ("deviceId" in constraints || "facingMode" in constraints)
      throw new TypeError("Pass deviceId and facingMode to start(), not constraints");
    if (!Number.isInteger(confirmations) || confirmations < 1)
      throw new TypeError("confirmations must be a positive integer");
    if (!Number.isFinite(scanInterval) || scanInterval < 0)
      throw new TypeError("scanInterval must be a non-negative number of milliseconds");
    if (region !== undefined && !validRegion(region))
      throw new TypeError("region must lie within the frame, with a positive width and height");
    this.#video = video;
    this.#scanner = scanner;
    this.#options = { ...options, ...(region ? { region: { ...region } } : {}) };
  }

  /**
   * Video inputs with their `deviceId` and `label`. Browsers hide labels (and on
   * some, all but one device) until camera permission is granted, so list cameras
   * after `start()` to build a camera picker.
   */
  static async cameras(): Promise<MediaDeviceInfo[]> {
    const devices = await mediaDevices().enumerateDevices();
    return devices.filter((device) => device.kind === "videoinput");
  }

  /** Whether `start()` is in effect; the camera is released while the page is hidden. */
  get running(): boolean {
    return this.#selection !== undefined;
  }

  /** The open camera track, for settings such as zoom; undefined while stopped or paused. */
  get track(): MediaStreamTrack | undefined {
    return this.#stream?.getVideoTracks()[0];
  }

  /** Whether the open camera reports a controllable torch. */
  get torchAvailable(): boolean {
    // getCapabilities is missing in older Firefox.
    const track = this.track as Partial<MediaStreamTrack> | undefined;
    const capabilities: TorchCapabilities | undefined = track?.getCapabilities?.();
    return capabilities?.torch === true;
  }

  /**
   * Open a camera and scan its frames until `stop()`. Calling it while running
   * switches cameras: the current one is released first, because many phones open
   * only one camera at a time. Resolves true once this call's camera is open, and
   * false when `stop()` or another `start()` cancelled it or the page is hidden
   * (the camera then opens when the page is shown). Rejects when the camera cannot
   * be opened: permission denied, no camera, an insecure page or a browser without
   * `requestVideoFrameCallback`. A camera request from an earlier `start()` that is
   * still pending, such as an unanswered permission prompt, is waited for first,
   * without a timeout; `stop()` makes this call resolve false once it settles.
   */
  async start(selection: CameraSelection = {}): Promise<boolean> {
    if (!("requestVideoFrameCallback" in this.#video))
      throw new Error("CameraScanner needs requestVideoFrameCallback (Firefox 132 or later)");
    this.stop();
    const session = this.#session;
    this.#selection = { ...selection };
    if (this.#options.pauseWhenHidden !== false)
      document.addEventListener("visibilitychange", this.#onVisibility);
    if (this.#paused()) return false;
    try {
      return await this.#open(session);
    } catch (error) {
      if (session !== this.#session) return false;
      this.stop();
      throw error;
    }
  }

  /** Release the camera and stop scanning. Results of a scan in progress are dropped. */
  stop(): void {
    this.#session++;
    this.#selection = undefined;
    document.removeEventListener("visibilitychange", this.#onVisibility);
    this.#release();
  }

  /**
   * Turn the torch on or off. Resolves false when the camera has no controllable
   * torch, for example a front camera, most laptops and some browsers. The torch
   * turns off whenever the camera is reopened.
   */
  async setTorch(on: boolean): Promise<boolean> {
    const track = this.track;
    if (!track || !this.torchAvailable) return false;
    await track.applyConstraints({ advanced: [{ torch: on } as MediaTrackConstraintSet] });
    return true;
  }

  /** Open the selected camera for `session`; false if the session ended meanwhile. */
  async #open(session: number): Promise<boolean> {
    const { deviceId, facingMode = "environment" } = this.#selection ?? {};
    const { constraints = {} } = this.#options;
    const sized = "width" in constraints || "height" in constraints;
    // Many phones open one camera at a time, so an earlier request settles first.
    if (this.#request) {
      await this.#request;
      if (session !== this.#session) return false;
    }
    const request = mediaDevices().getUserMedia({
      audio: false,
      video: {
        ...(sized ? {} : { width: { ideal: 1280 }, height: { ideal: 720 } }),
        ...constraints,
        // `ideal` lets laptops without a back camera fall back to their webcam.
        // Before permission, device ids can be empty strings.
        ...(deviceId ? { deviceId: { exact: deviceId } } : { facingMode: { ideal: facingMode } }),
      },
    });
    const settled = request.then(
      (stream) => {
        if (session !== this.#session) stopTracks(stream);
      },
      () => undefined,
    );
    this.#request = settled;
    void settled.then(() => {
      if (this.#request === settled) this.#request = undefined;
    });
    const stream = await request;
    if (session !== this.#session) {
      // Stopped, restarted or hidden while the permission prompt was open.
      stopTracks(stream);
      return false;
    }
    this.#stream = stream;
    for (const track of stream.getVideoTracks())
      track.addEventListener("ended", () => {
        if (session !== this.#session || this.#paused()) return;
        this.#fail(new Error("The camera stopped"));
      });
    const video = this.#video;
    // iOS Safari plays inline, muted video without a user gesture only.
    video.muted = true;
    video.playsInline = true;
    video.srcObject = stream;
    try {
      await video.play();
    } catch (error) {
      // A later start or stop replaced the stream and interrupted play().
      if (session !== this.#session) return false;
      throw error;
    }
    if (session !== this.#session) return false;
    this.#watch(session, stream);
    void this.#loop(session);
    return true;
  }

  async #loop(session: number): Promise<void> {
    const video = this.#video;
    const { scanInterval = 0 } = this.#options;
    let seen: Reads = new Map();
    while (session === this.#session) {
      await new Promise((resolve) => video.requestVideoFrameCallback(resolve));
      if (session !== this.#session) return;
      const started = performance.now();
      let result: ScanResult | undefined;
      try {
        result = await this.#scanFrame();
      } catch (error) {
        if (session === this.#session) this.#fail(toError(error));
        return;
      }
      if (session !== this.#session) return;
      if (result) seen = this.#report(result, seen, session, started);
      const wait = started + scanInterval - performance.now();
      if (wait > 0) await new Promise((resolve) => setTimeout(resolve, wait));
    }
  }

  /** Scan the current frame, or only its region; undefined before the first frame. */
  async #scanFrame(): Promise<ScanResult | undefined> {
    const { region, scanOptions } = this.#options;
    const video = this.#video;
    if (!region) return this.#scanner.scan(video, scanOptions);
    const { videoWidth, videoHeight } = video;
    // Before the first frame, or below the scanner's minimum image size.
    if (videoWidth < 3 || videoHeight < 3) return undefined;
    const [left, width] = span(region.left, region.width, videoWidth);
    const [top, height] = span(region.top, region.height, videoHeight);
    const crop = await createImageBitmap(video, left, top, width, height);
    handOver(crop);
    try {
      return offset(await this.#scanner.scan(crop, scanOptions), left, top);
    } finally {
      crop.close();
    }
  }

  /**
   * Call the app's handlers until one stops `session`; returns the barcodes to
   * track in the next scan. `now` is when this scan started.
   */
  #report(result: ScanResult, seen: Reads, session: number, now: number): Reads {
    const options = this.#options;
    safely(() => options.onScan?.(result));
    if (!options.onDetect || session !== this.#session) return seen;
    const { confirmations = 1 } = options;
    const next: Reads = new Map();
    for (const barcode of result.barcodes) {
      const key = `${barcode.format}\n${barcode.text}`;
      if (next.has(key)) continue;
      const previous = seen.get(key);
      const reads = (previous?.reads ?? 0) + 1;
      const reported = previous?.reported === true || reads >= confirmations;
      next.set(key, { reads, reported, missed: 0, lastRead: now });
      if (reported && !previous?.reported) {
        safely(() => options.onDetect?.(barcode));
        if (session !== this.#session) return next;
      }
    }
    // A reported barcode stays until it is missing from FORGET_SCANS scans and
    // for FORGET_MS: the count covers fast scanning, the time slow scanning. An
    // unreported one is dropped at once, so confirmations stay consecutive.
    for (const [key, entry] of seen) {
      if (next.has(key) || !entry.reported) continue;
      const missed = entry.missed + 1;
      if (missed < FORGET_SCANS || now - entry.lastRead < FORGET_MS)
        next.set(key, { ...entry, missed });
    }
    return next;
  }

  #paused(): boolean {
    return this.#options.pauseWhenHidden !== false && document.visibilityState === "hidden";
  }

  /** Release the camera while hidden; reopen it when shown. */
  #visibilityChanged(): void {
    if (this.#selection === undefined) return;
    const hidden = this.#paused();
    if (!hidden && this.#stream) return;
    const session = ++this.#session;
    this.#release();
    if (hidden) return;
    this.#open(session).catch((error: unknown) => {
      if (session === this.#session) this.#fail(toError(error));
    });
  }

  /**
   * Stop `session` when the app replaces the video's stream. A paused video only
   * pauses scanning: requestVideoFrameCallback waits until it plays again.
   */
  #watch(session: number, stream: MediaStream): void {
    const video = this.#video;
    const watching = new AbortController();
    this.#watching = watching;
    video.addEventListener(
      "emptied",
      () => {
        if (session === this.#session && video.srcObject !== stream)
          this.#fail(new Error("The video element's source was replaced"));
      },
      { signal: watching.signal },
    );
  }

  /** Stop and report `error` to onError, or to the global error handler without one. */
  #fail(error: Error): void {
    this.stop();
    const options = this.#options;
    if (options.onError)
      safely(() => {
        options.onError?.(error);
      });
    else reportError(error);
  }

  #release(): void {
    this.#watching?.abort();
    this.#watching = undefined;
    const stream = this.#stream;
    this.#stream = undefined;
    if (!stream) return;
    stopTracks(stream);
    if (this.#video.srcObject === stream) this.#video.srcObject = null;
  }
}

/**
 * Each tracked barcode: its consecutive reads, whether `onDetect` has seen it, the
 * scans since it was last read and when that scan started.
 */
type Reads = Map<string, { reads: number; reported: boolean; missed: number; lastRead: number }>;

/** A reported barcode counts as gone once it is missing from this many consecutive scans */
const FORGET_SCANS = 3;
/** and has not been read for this many milliseconds. */
const FORGET_MS = 500;

/** An error in the app's handlers is reported, not treated as a camera failure. */
function safely(handler: () => void): void {
  try {
    handler();
  } catch (error) {
    reportError(error);
  }
}

/**
 * One side of a region in pixels: the offset and length of its rounded edges,
 * widened and moved back inside the frame to the scanner's minimum of 3 pixels.
 */
function span(start: number, size: number, extent: number): [number, number] {
  const from = Math.round(start * extent);
  const length = Math.max(3, Math.min(Math.round((start + size) * extent), extent) - from);
  return [Math.min(from, extent - length), length];
}

function validRegion(region: ScanRegion): boolean {
  const { left, top, width, height } = region;
  return (
    [left, top, width, height].every(Number.isFinite) &&
    left >= 0 &&
    top >= 0 &&
    width > 0 &&
    height > 0 &&
    left + width <= 1 &&
    top + height <= 1
  );
}

/** Move a result scanned from a cropped region back into whole-frame coordinates. */
function offset(result: ScanResult, left: number, top: number): ScanResult {
  if (result.barcodes.length === 0 || (left === 0 && top === 0)) return result;
  const barcodes = result.barcodes.map((barcode) => ({
    ...barcode,
    polygon: barcode.polygon.map(([x, y]) => [x + left, y + top]) as unknown as Quad,
    rect: { ...barcode.rect, left: barcode.rect.left + left, top: barcode.rect.top + top },
  }));
  // `best` is one of `barcodes`; the worker's structured clone keeps that identity.
  const best = result.best && barcodes[result.barcodes.indexOf(result.best)];
  return freeze({ ...result, barcodes, best });
}

function mediaDevices(): MediaDevices {
  const devices = typeof navigator === "undefined" ? undefined : navigator.mediaDevices;
  if (!devices?.getUserMedia)
    throw new Error("Camera access needs a browser page served over HTTPS or from localhost");
  return devices;
}

function stopTracks(stream: MediaStream): void {
  for (const track of stream.getTracks()) track.stop();
}

function toError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
