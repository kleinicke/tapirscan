// tapirscan/camera: an optional camera loop for any tapirscan/browser scanner.
// It owns acquisition, frame scheduling and cleanup; decoding stays in the scanner.
import type { ScanOptions, ScanResult } from "./index.js";

/** Any tapirscan/browser `Scanner`; the controller never disposes it. */
export interface FrameScanner {
  scan(source: HTMLVideoElement, options?: ScanOptions): Promise<ScanResult>;
}

/** Which camera to open. Without a `deviceId`, the browser picks by `facingMode`. */
export interface CameraSelection {
  /** A `deviceId` from `CameraScanner.cameras()`; required to match. */
  deviceId?: string;
  /** Preferred direction when no `deviceId` is given; defaults to "environment". */
  facingMode?: "environment" | "user";
}

export interface CameraScannerOptions {
  /** Called with every scanned frame; `result.barcodes` is empty when nothing was read. */
  onScan?(result: ScanResult): void;
  /**
   * Called when scanning fails after `start()` resolved: the scanner failed or was
   * disposed, the camera ended, or resuming after the page was hidden failed.
   * Scanning has stopped and the camera is released.
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
 * Scans a live camera into a `<video>` element with a tapirscan/browser scanner.
 * One frame is scanned at a time, so slow devices skip frames instead of queueing
 * them. After `stop()`, no further results arrive, and a camera that is granted
 * only after `stop()` is released at once.
 */
export class CameraScanner {
  readonly #video: HTMLVideoElement;
  readonly #scanner: FrameScanner;
  readonly #options: CameraScannerOptions;
  /** Increments on every start and stop; work from an older session is dropped. */
  #session = 0;
  #stream: MediaStream | undefined;
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
    const { constraints = {} } = options;
    if ("deviceId" in constraints || "facingMode" in constraints)
      throw new TypeError("Pass deviceId and facingMode to start(), not constraints");
    this.#video = video;
    this.#scanner = scanner;
    this.#options = { ...options };
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
   * `requestVideoFrameCallback`.
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
    const stream = await mediaDevices().getUserMedia({
      audio: false,
      video: {
        ...(sized ? {} : { width: { ideal: 1280 }, height: { ideal: 720 } }),
        ...constraints,
        // `ideal` lets laptops without a back camera fall back to their webcam.
        // Before permission, device ids can be empty strings.
        ...(deviceId ? { deviceId: { exact: deviceId } } : { facingMode: { ideal: facingMode } }),
      },
    });
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
    void this.#loop(session);
    return true;
  }

  async #loop(session: number): Promise<void> {
    const video = this.#video;
    while (session === this.#session) {
      await new Promise((resolve) => video.requestVideoFrameCallback(resolve));
      if (session !== this.#session) return;
      let result: ScanResult;
      try {
        result = await this.#scanner.scan(video, this.#options.scanOptions);
      } catch (error) {
        if (session === this.#session) this.#fail(toError(error));
        return;
      }
      if (session !== this.#session) return;
      // An error in the app's handler is reported, not treated as a camera failure.
      try {
        this.#options.onScan?.(result);
      } catch (error) {
        reportError(error);
      }
    }
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

  #fail(error: Error): void {
    this.stop();
    this.#options.onError?.(error);
  }

  #release(): void {
    const stream = this.#stream;
    this.#stream = undefined;
    if (!stream) return;
    stopTracks(stream);
    if (this.#video.srcObject === stream) this.#video.srcObject = null;
  }
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
