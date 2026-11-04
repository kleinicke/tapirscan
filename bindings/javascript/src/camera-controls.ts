/** Track controls shared by CameraScanner and applications that own their capture loop. */

/** Zoom limits reported by the open camera. Values use the device's zoom scale. */
export interface CameraZoomRange {
  readonly min: number;
  readonly max: number;
  readonly step: number;
}

/** A point in the full, unmirrored video image: top-left (0,0), bottom-right (1,1). */
export interface CameraFocusPoint {
  readonly x: number;
  readonly y: number;
}

type Capabilities = MediaTrackCapabilities & {
  torch?: boolean | boolean[];
  focusMode?: string[];
  zoom?: CameraZoomRange;
};
type Settings = MediaTrackSettings & { zoom?: number; pointsOfInterest?: CameraFocusPoint[] };
type Controls = MediaTrackConstraintSet & {
  torch?: boolean;
  focusMode?: string;
  zoom?: number;
  pointsOfInterest?: CameraFocusPoint[];
};

/**
 * @experimental Controls for an application-owned video track. Constructing this
 * helper changes no camera settings. Use one helper per track and route control
 * changes through it so requests preserve each other. It owns no camera stream.
 * Browser capabilities are best effort and cannot guarantee macro lens switching.
 */
export class CameraControls {
  readonly #track: MediaStreamTrack;
  #disposed = false;
  #queue: Promise<unknown> = Promise.resolve();

  constructor(track: MediaStreamTrack) {
    this.#track = track;
  }

  #isActive(): boolean {
    return !this.#disposed && this.#track.readyState === "live";
  }

  get #capabilities(): Capabilities | undefined {
    if (!this.#isActive()) return undefined;
    // Some browsers expose streams but no getCapabilities().
    try {
      return (this.#track as Partial<MediaStreamTrack>).getCapabilities?.();
    } catch {
      // Optional controls must not prevent capture when a browser refuses capability queries.
      return undefined;
    }
  }

  /** Whether the camera advertises a controllable torch. */
  get torchAvailable(): boolean {
    const torch = this.#capabilities?.torch;
    return torch === true || (Array.isArray(torch) && torch.includes(true));
  }

  /** Whether continuous autofocus can be requested. */
  get continuousFocusAvailable(): boolean {
    return this.#capabilities?.focusMode?.includes("continuous") === true;
  }

  /**
   * Whether point constraints are recognized and this track advertises autofocus.
   * This is permission to attempt focusAt(), not evidence of successful focusing.
   */
  get pointFocusAvailable(): boolean {
    const devices = controlDevices();
    const supported: (MediaTrackSupportedConstraints & { pointsOfInterest?: boolean }) | undefined =
      devices?.getSupportedConstraints?.();
    const modes = this.#capabilities?.focusMode;
    return (
      supported?.pointsOfInterest === true &&
      (modes?.includes("continuous") === true || modes?.includes("single-shot") === true)
    );
  }

  /** Device zoom scale, or undefined when unsupported or fixed. */
  get zoomRange(): CameraZoomRange | undefined {
    const range = this.#capabilities?.zoom;
    if (
      !range ||
      ![range.min, range.max, range.step].every(Number.isFinite) ||
      range.min <= 0 ||
      range.max <= range.min ||
      range.step < 0
    )
      return undefined;
    return { min: range.min, max: range.max, step: range.step };
  }

  /** Zoom reported by the track; the device may round requested zoom values. */
  get zoom(): number | undefined {
    return this.#isActive() ? (this.#track.getSettings() as Settings).zoom : undefined;
  }

  /** Restore continuous autofocus, clearing point requests if present. */
  async setContinuousFocus(): Promise<boolean> {
    if (!this.continuousFocusAvailable) return false;
    return this.#apply(() => {
      const constraints = this.#track.getConstraints();
      const settings = this.#track.getSettings() as Settings;
      const hasPoint =
        "pointsOfInterest" in constraints ||
        constraints.advanced?.some((entry) => "pointsOfInterest" in entry) === true ||
        (settings.pointsOfInterest?.length ?? 0) > 0;
      return hasPoint
        ? { focusMode: "continuous", pointsOfInterest: [] }
        : { focusMode: "continuous" };
    });
  }

  /**
   * Request focus at normalized image coordinates. Continuous mode keeps tracking
   * that area when available; otherwise uses single-shot. Map preview crop, rotation
   * and mirroring before calling. Points may also influence exposure/white balance.
   * True means getSettings() reports this point (within 0.001 rounding), not a sharp
   * image. False means unsupported, unconfirmed, ended or disposed. Device refusal
   * rejects. There is no automatic timeout; setContinuousFocus() resets the area.
   */
  async focusAt(point: CameraFocusPoint): Promise<boolean> {
    validateFocusPoint(point);
    const requested = { x: point.x, y: point.y };
    if (!this.pointFocusAvailable) return false;
    return this.#apply(
      () => ({
        focusMode: this.continuousFocusAvailable ? "continuous" : "single-shot",
        pointsOfInterest: [requested],
      }),
      () => {
        const settings = this.#track.getSettings() as Settings;
        return (
          settings.pointsOfInterest?.some(
            (value) =>
              Math.abs(value.x - requested.x) <= 0.001 && Math.abs(value.y - requested.y) <= 0.001,
          ) === true
        );
      },
    );
  }

  /** Request a zoom inside zoomRange; invalid values and device refusal reject. */
  async setZoom(value: number): Promise<boolean> {
    validateZoom(value);
    const range = this.zoomRange;
    if (!range) return false;
    if (value < range.min || value > range.max)
      throw new RangeError("zoom must be within zoomRange");
    return this.#apply(() => ({ zoom: value }));
  }

  /** Request torch on/off; false when unavailable, ended or disposed. */
  async setTorch(on: boolean): Promise<boolean> {
    if (!this.torchAvailable) return false;
    return this.#apply(() => ({ torch: on }));
  }

  /** Invalidate pending results and queued requests. Does not stop or reset the track. */
  dispose(): void {
    this.#disposed = true;
  }

  async #apply(patch: () => Controls, confirmed: () => boolean = () => true): Promise<boolean> {
    const apply = async () => {
      if (!this.#isActive()) return false;
      const values = patch();
      const keys = Object.keys(values);
      const constraints = withoutControls(this.#track.getConstraints(), keys);
      const advanced = (constraints.advanced ?? [])
        .map((entry) => withoutControls(entry, keys))
        .filter((entry) => Object.keys(entry).length > 0);
      try {
        await this.#track.applyConstraints({ ...constraints, advanced: [...advanced, values] });
      } catch (error) {
        if (!this.#isActive()) return false;
        throw error;
      }
      return this.#isActive() && confirmed();
    };
    const result = this.#queue.then(apply);
    this.#queue = result.catch(() => undefined);
    return result;
  }
}

function withoutControls(
  constraints: MediaTrackConstraints,
  keys: string[],
): MediaTrackConstraints {
  return Object.fromEntries(Object.entries(constraints).filter(([key]) => !keys.includes(key)));
}

export function validateFocusPoint(point: CameraFocusPoint): void {
  const input = point as CameraFocusPoint | null | undefined;
  if (
    !input ||
    ![input.x, input.y].every((value) => Number.isFinite(value) && value >= 0 && value <= 1)
  )
    throw new RangeError("focus point coordinates must be finite numbers between 0 and 1");
}

export function validateZoom(value: number): void {
  if (!Number.isFinite(value) || value <= 0)
    throw new RangeError("zoom must be a positive finite number");
}

function controlDevices(): Partial<MediaDevices> | undefined {
  return typeof navigator === "undefined" ? undefined : navigator.mediaDevices;
}
