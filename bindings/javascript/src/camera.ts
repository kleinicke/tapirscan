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
   * Extra video constraints, merged over the defaults of 1280×720 ideal size.
   * Smaller frames scan faster; full sensor resolution rarely helps.
   */
  constraints?: MediaTrackConstraints;
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

  /** Whether a camera is open or opening. */
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
   * only one camera at a time. Rejects when the camera cannot be opened (permission
   * denied, no camera, an insecure page); resolves early if `stop()` cancels it.
   */
  async start(selection: CameraSelection = {}): Promise<void> {
    this.#release();
    const session = ++this.#session;
    this.#selection = { ...selection };
    if (this.#options.pauseWhenHidden !== false && typeof document !== "undefined")
      document.addEventListener("visibilitychange", this.#onVisibility);
    try {
      await this.#open(session);
    } catch (error) {
      if (session === this.#session) this.stop();
      throw error;
    }
  }

  /** Release the camera and stop scanning. Results of a scan in progress are dropped. */
  stop(): void {
    this.#session++;
    this.#selection = undefined;
    if (typeof document !== "undefined")
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

  async #open(session: number): Promise<void> {
    const { deviceId, facingMode = "environment" } = this.#selection ?? {};
    const stream = await mediaDevices().getUserMedia({
      audio: false,
      video: {
        width: { ideal: 1280 },
        height: { ideal: 720 },
        // `ideal` lets laptops without a back camera fall back to their webcam.
        ...(deviceId === undefined
          ? { facingMode: { ideal: facingMode } }
          : { deviceId: { exact: deviceId } }),
        ...this.#options.constraints,
      },
    });
    if (session !== this.#session) {
      // Stopped or restarted while the permission prompt was open.
      stopTracks(stream);
      return;
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
    video.setAttribute("playsinline", "");
    video.srcObject = stream;
    try {
      await video.play();
    } catch (error) {
      // A later start or stop replaced the stream and interrupted play().
      if (session !== this.#session) return;
      throw error;
    }
    if (session !== this.#session) return;
    void this.#loop(session);
  }

  async #loop(session: number): Promise<void> {
    try {
      while (session === this.#session) {
        await nextFrame(this.#video);
        if (session !== this.#session) return;
        const result = await this.#scanner.scan(this.#video, this.#options.scanOptions);
        if (session !== this.#session) return;
        this.#options.onScan?.(result);
      }
    } catch (error) {
      if (session === this.#session)
        this.#fail(error instanceof Error ? error : new Error(String(error)));
    }
  }

  #paused(): boolean {
    return this.#options.pauseWhenHidden !== false && document.visibilityState === "hidden";
  }

  #visibilityChanged(): void {
    if (this.#selection === undefined) return;
    if (this.#paused()) {
      this.#session++;
      this.#release();
      return;
    }
    if (this.#stream) return;
    const session = ++this.#session;
    this.#open(session).catch((error: unknown) => {
      if (session === this.#session)
        this.#fail(error instanceof Error ? error : new Error(String(error)));
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

/** Resolve once the video shows a frame that was not scanned yet. */
function nextFrame(video: HTMLVideoElement): Promise<void> {
  // Typed as always present, but missing in older Firefox.
  if (typeof (video as Partial<HTMLVideoElement>).requestVideoFrameCallback === "function")
    return new Promise((resolve) => {
      video.requestVideoFrameCallback(() => {
        resolve();
      });
    });
  // Fallback: poll each animation frame for a new, decodable frame.
  const seen = video.currentTime;
  return new Promise((resolve) => {
    const check = () => {
      if (video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA && video.currentTime !== seen)
        resolve();
      else requestAnimationFrame(check);
    };
    requestAnimationFrame(check);
  });
}
