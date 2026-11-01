// Workflow state only: no camera, decoder, DOM or persistent storage dependency.
import type { Barcode } from "./index.js";
import { formatBits, hasKey } from "./multiformat/formats.js";

/** A decoded identity. Format and text together distinguish list entries. */
export type ScanIdentity = Pick<Barcode, "format" | "text">;

/** Accepted units, not an automatic count of physical objects in view. */
export interface ScanEntry extends ScanIdentity {
  readonly quantity: number;
}

/** @experimental Options for the scan-session preview. */
export interface ScanSessionOptions {
  /** "single" finishes on the first acceptance; "list" finishes explicitly. Default "single". */
  mode?: "single" | "list";
}

/**
 * @experimental A camera-independent acceptance workflow for decoded results.
 *
 * Repeated format/text pairs are ignored for the entire session unless the user
 * explicitly asks to add another unit. No physical tracking or storage is implied.
 * The application owns camera start/stop, feedback and persistence.
 */
export class ScanSession {
  readonly #mode: "single" | "list";
  #finished = false;
  #entries = new Map<string, ScanEntry>();
  #history: string[] = [];

  constructor({ mode = "single" }: ScanSessionOptions = {}) {
    const input: unknown = mode;
    if (input !== "single" && input !== "list")
      throw new TypeError('session mode must be "single" or "list"');
    this.#mode = mode;
  }

  /** Workflow selected at construction. */
  get mode(): "single" | "list" {
    return this.#mode;
  }

  /** Immutable snapshot in first-acceptance order; safe to retain after reset(). */
  get entries(): readonly ScanEntry[] {
    return Object.freeze([...this.#entries.values()]);
  }

  /** True after finish(), or after the first acceptance in single mode. */
  get finished(): boolean {
    return this.#finished;
  }

  /** Whether the last accepted unit can be undone. Finished sessions are locked. */
  get canUndo(): boolean {
    return !this.#finished && this.#history.length > 0;
  }

  /**
   * Accept a decoded identity. Returns false for duplicates or a finished session.
   * `repeat: true` explicitly adds one more unit of an existing entry. Connect
   * that option to a user action, never an every-frame callback.
   */
  accept(barcode: ScanIdentity, { repeat = false }: { repeat?: boolean } = {}): boolean {
    if (this.#finished) return false;
    // Validate plain JavaScript callers too; keep no reference to the input.
    const input = barcode as { format?: unknown; text?: unknown } | null;
    if (typeof input?.format !== "string" || !input.format || typeof input.text !== "string")
      throw new TypeError("A scan identity needs format and text strings");
    if (!hasKey(formatBits, input.format))
      throw new TypeError(`Unsupported barcode format: ${input.format}`);
    if (typeof repeat !== "boolean") throw new TypeError("repeat must be a boolean");
    const { format, text } = input as ScanIdentity;
    const key = JSON.stringify([format, text]);
    const previous = this.#entries.get(key);
    if (previous && !repeat) return false;
    const quantity = (previous?.quantity ?? 0) + 1;
    if (!Number.isSafeInteger(quantity))
      throw new RangeError("quantity exceeds the safe integer range");
    this.#entries.set(key, Object.freeze({ format, text, quantity }));
    this.#history.push(key);
    if (this.mode === "single") this.#finished = true;
    return true;
  }

  /** Remove the last accepted unit; ignores duplicate attempts. False when locked or empty. */
  undo(): boolean {
    if (!this.canUndo) return false;
    const key = this.#history.pop();
    if (key === undefined) return false;
    const entry = this.#entries.get(key);
    if (!entry) return false;
    if (entry.quantity === 1) this.#entries.delete(key);
    else this.#entries.set(key, Object.freeze({ ...entry, quantity: entry.quantity - 1 }));
    return true;
  }

  /** Lock acceptance and return an immutable snapshot. Does not stop a camera. */
  finish(): readonly ScanEntry[] {
    this.#finished = true;
    return this.entries;
  }

  /** Begin an empty session in the same mode. Previously returned snapshots remain unchanged. */
  reset(): void {
    this.#entries.clear();
    this.#history = [];
    this.#finished = false;
  }
}
