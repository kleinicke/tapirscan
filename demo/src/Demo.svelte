<script lang="ts">
  import { capabilities, videoFrame, scanDimensions } from "./lib/camera";
  import { onMount, tick } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import {
    comparisonOptions,
    visibleResults,
    type ComparisonSpec,
    type ComparisonEntry,
  } from "./lib/comparison";
  import type { Result } from "./lib/types";
  import {
    retailFormats,
    commonFormats,
    commonLinearFormats,
    linearFormats,
    matrixFormats,
    type Format,
  } from "tapirscan";
  import SpringSlider from "./SpringSlider.svelte";
  import { DoubleTap } from "./lib/taps";
  import { LabelLayout } from "./lib/labels";
  import { version } from "../package.json";
  import scannerVersions from "./lib/scanner-versions.json";
  let releaseVersion = scannerVersions.default;
  let tapirscanRevision = 0;
  function changeVersion() {
    tapirscanRevision++;
    const ids = new Set(
      options.filter((option) => option.engine === "classical").map((option) => option.id),
    );
    entries = entries.filter((entry) => !ids.has(entry.id));
    for (const id of ids) {
      pending.get(id)?.(new Error("Tapirscan version changed"));
      workers.get(id)?.terminate();
      workers.delete(id);
    }
    if ((source || live) && selected.some((id) => ids.has(id))) requestScan(0);
  }

  let pdfDocument: import("pdfjs-dist").PDFDocumentProxy | null = null;
  let pdfName = "";
  let pdfPage = 1;
  let pdfBusy = false;
  let isPdf = false;
  function clearPdf() {
    const previous = pdfDocument;
    pdfDocument = null;
    isPdf = false;
    pdfBusy = false;
    if (previous) void previous.loadingTask.destroy().catch(() => {});
  }
  async function showPdfPage(number: number, token = ++loadId) {
    const document = pdfDocument;
    if (!document) return;
    pdfBusy = true;
    invalidate();
    entries = [];
    source?.close();
    source = null;
    status = `Rendering PDF page ${number}…`;
    try {
      const { renderPdfPage } = await import("./lib/pdf");
      const blob = await renderPdfPage(document, number);
      if (token !== loadId || disposed) return;
      await loadImage(blob, token, 0, 1, true);
      pdfPage = number;
      captureInfo = `${pdfName} · Page ${number} of ${document.numPages} · ${mediaWidth} × ${mediaHeight}`;
    } catch (reason) {
      if (token === loadId) {
        error = `Could not open PDF page: ${String(reason)}`;
        status = "PDF page unavailable";
      }
    } finally {
      if (token === loadId) pdfBusy = false;
    }
  }
  async function loadPdf(file: File, token: number) {
    clearPdf();
    stopCamera();
    isPdf = true;
    pdfBusy = true;
    showAdjust = false;
    pointers.clear();
    gestureBounds = null;
    invalidate();
    entries = [];
    source?.close();
    source = null;
    error = "";
    status = "Opening PDF…";
    try {
      const { openPdf } = await import("./lib/pdf");
      if (token !== loadId || disposed) return;
      const document = await openPdf(file);
      if (token !== loadId || disposed) {
        await document.loadingTask.destroy();
        return;
      }
      pdfDocument = document;
      pdfName = file.name;
      await showPdfPage(1, token);
    } catch (reason) {
      if (token === loadId) {
        clearPdf();
        error = `This PDF could not be opened. Password-protected PDFs must be unlocked first. ${String(reason)}`;
        status = "PDF unavailable";
      }
    } finally {
      if (token === loadId) pdfBusy = false;
    }
  }

  let detection: "ean13" | "retail" | "common1d" | "common" | "matrix" | "all" = "retail";
  $: formats =
    detection === "all"
      ? [...linearFormats, ...matrixFormats]
      : detection === "matrix"
        ? matrixFormats
        : detection === "common1d"
          ? commonLinearFormats
          : detection === "common"
            ? commonFormats
            : detection === "retail"
              ? retailFormats
              : (["EAN13"] as const);

  function qrOnlyUnavailable(id: string) {
    return id === "jsqr" && !formats.includes("QRCode");
  }
  function changeDetection() {
    invalidate();
    entries = [];
    for (const reject of pending.values()) reject(new Error("Detection selection changed"));
    for (const worker of workers.values()) worker.terminate();
    workers.clear();
    if (source || live) requestScan(0);
  }

  let viewer: HTMLDivElement;
  let fullscreenButton: HTMLButtonElement;
  let expanded = false;
  let showAdjust = false;
  let nativeFullscreen = false;
  let orientationLocked = false;
  let savedOverflow = "";
  let stageWidth = 800;
  let windowWidth = window.innerWidth,
    windowHeight = window.innerHeight;
  function restoreViewer() {
    expanded = false;
    nativeFullscreen = false;
    if (orientationLocked) screen.orientation.unlock();
    orientationLocked = false;
    document.body.style.overflow = savedOverflow;
    fullscreenButton?.focus();
  }
  async function exitViewer() {
    if (document.fullscreenElement === viewer) await document.exitFullscreen();
    if (expanded) restoreViewer();
  }
  async function enterViewer() {
    const orientation = screen.orientation?.type;
    savedOverflow = document.body.style.overflow;
    expanded = true;
    document.body.style.overflow = "hidden";
    try {
      await viewer.requestFullscreen();
      nativeFullscreen = true;
    } catch {
      /* A viewport-filling viewer also works without native fullscreen. */
    }
    if (!expanded) return;
    const lockable = screen.orientation as ScreenOrientation & {
      lock?: (_type: string) => Promise<void>;
    };
    try {
      if (!orientation || !lockable?.lock) throw new Error("Unavailable");
      await lockable.lock(orientation);
      orientationLocked = true;
      if (!expanded) {
        screen.orientation.unlock();
        orientationLocked = false;
      }
    } catch {
      // Orientation locking is optional; keep the viewer usable when unavailable.
    }
    await tick();
    viewer.querySelector<HTMLButtonElement>(".exit-viewer")?.focus();
  }
  function fullscreenChanged() {
    if (nativeFullscreen && document.fullscreenElement !== viewer) restoreViewer();
  }
  function viewerKey(event: KeyboardEvent) {
    if (!expanded) return;
    if (event.key === "Escape") {
      event.preventDefault();
      void exitViewer();
    }
    if (event.key === "Tab") {
      const controls = [...viewer.querySelectorAll<HTMLElement>("button, input, [tabindex='0']")];
      const index = controls.indexOf(document.activeElement as HTMLElement);
      event.preventDefault();
      controls[(index + (event.shiftKey ? -1 : 1) + controls.length) % controls.length]?.focus();
    }
  }
  const options = [
    "fast",
    "turbo",
    "turbo2",
    "turbo4",
    "turbo8",
    "turbo16",
    "zxing",
    "zxingdefault",
    "zbar",
    "nano",
    "quality",
    "veryhigh",
    "jsqr",
    "native",
    "quagga",
    "zxingjs",
    "zxingjsdefault",
  ].map((id) => comparisonOptions.find((spec) => spec.id === id)!);
  let selected = ["fast", "turbo", "zxing", "zbar"];
  let visibleScanners = [...selected];
  const demoImages = [
    { file: "synthetic-barcode.png", label: "Synthetic barcode" },
    { file: "pesto.jpg", label: "Pesto" },
    { file: "pills.jpg", label: "Pills" },
    { file: "sauce.jpg", label: "Sauce" },
    { file: "sunscreen.jpg", label: "Sunscreen" },
  ];
  let selectedDemo = "pesto.jpg";
  type ViewEntry = ComparisonEntry & {
    viewRevision: number;
    contentRevision: number;
    angle: number;
    scale: number;
    width: number;
    height: number;
  };
  let entries: ViewEntry[] = [];
  let video: HTMLVideoElement,
    preview: HTMLCanvasElement,
    surface: HTMLCanvasElement,
    detail: HTMLCanvasElement;
  let detailReady = false,
    detailRevision = -1;
  let livePreview: HTMLCanvasElement;
  let livePreviewFrame = 0;
  let lastPreviewTime = 0;
  let lastScanImage: ImageData | null = null;
  let lastScanContent = -1;
  let saving = false;
  let cameraPaused = false;
  let cameraFrame = false;
  let captureMode: "video" | "photos" = "video";
  let resolution = "1080";
  let captureInfo = "";
  let cameraNote = "";
  let photoInput: HTMLInputElement;
  let cameras: MediaDeviceInfo[] = [];
  let selectedCamera = "";
  let cameraZoom = 1;
  let zoomRange: { min: number; max: number; step: number } | undefined;
  let source: ImageBitmap | null = null;
  let stream: MediaStream | null = null;
  let live = false,
    opening = false,
    busy = false,
    showAreas = false,
    torch = false,
    hasTorch = false;
  let error = "",
    status = "Choose a photo or start your camera.";
  let centerX = 0,
    centerY = 0;
  let scale = 1,
    angle = 0;
  let frameWidth = 1920,
    frameHeight = 1080,
    mediaWidth = 1920,
    mediaHeight = 1080;
  let gestureFrame = 0;
  let gestureBounds: DOMRect | null = null;
  let preparationMs = 0,
    totalMs = 0;
  let contentRevision = 0;
  let revision = 0,
    loadId = 0,
    cameraId = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  const workers = new SvelteMap<string, Worker>();
  const pending = new SvelteMap<string, (_error: Error) => void>();
  const input = document.createElement("canvas");
  input.width = 1920;
  input.height = 1080;
  const ctx = input.getContext("2d", { willReadFrequently: true })!;
  // The preview includes a 240px surround on every side of the actual scan frame.
  const margin = 240;
  $: W = frameWidth + margin * 2;
  $: H = frameHeight + margin * 2;
  $: fit = Math.min(frameWidth / mediaWidth, frameHeight / mediaHeight);
  $: frameOnly = expanded || live || cameraFrame || isPdf;
  $: viewWidth = frameOnly ? frameWidth : W;
  $: viewHeight = frameOnly ? frameHeight : H;
  $: viewOffset = frameOnly ? margin : 0;
  $: displayWidth = ((mediaWidth * fit) / viewWidth) * 100;
  $: displayHeight = ((mediaHeight * fit) / viewHeight) * 100;
  function setFrame(width: number, height: number) {
    mediaWidth = width;
    mediaHeight = height;
    const limit = resolution === "original" ? Infinity : (Number(resolution) * 16) / 9;
    [frameWidth, frameHeight] = scanDimensions(width, height, limit);
  }
  $: chosen = options.filter((s) => selected.includes(s.id));
  $: overlayEntries = visibleResults(
    entries,
    options.map((option) => option.id),
    selected,
    contentRevision,
  );
  $: overlaysUpdating = overlayEntries.some((entry) => entry.viewRevision !== revision);
  function overlayTransform(entry: ViewEntry, currentAngle: number, currentScale: number) {
    return `translate(${entry.width / 2} ${entry.height / 2}) rotate(${currentAngle - entry.angle}) scale(${currentScale / entry.scale}) translate(${-entry.width / 2} ${-entry.height / 2})`;
  }
  $: labelUnit = viewWidth / Math.max(stageWidth, 1);
  const labelLayout = new LabelLayout();
  $: labels = labelLayout.update(
    overlayEntries.flatMap((entry) => {
      const radians = ((angle - entry.angle) * Math.PI) / 180;
      const ratio = scale / entry.scale;
      return (entry.result?.regions ?? [])
        .filter((region) => region.text && region.polygon.length)
        .map((region) => ({
          text: region.text,
          scanner: entry.id,
          scanMs: entry.result!.scanMs,
          polygon: region.polygon.map(([x, y]) => {
            const dx = (x - entry.width / 2) * ratio,
              dy = (y - entry.height / 2) * ratio;
            return [
              margin -
                viewOffset +
                entry.width / 2 +
                dx * Math.cos(radians) -
                dy * Math.sin(radians),
              margin -
                viewOffset +
                entry.height / 2 +
                dx * Math.sin(radians) +
                dy * Math.cos(radians),
            ];
          }),
        }));
    }),
    chosen.map((spec) => ({
      id: spec.id,
      label: spec.label,
      color: spec.color,
      pending: !entries.some((entry) => entry.id === spec.id && entry.viewRevision === revision),
    })),
    viewWidth,
    viewHeight,
    labelUnit,
    JSON.stringify([
      loadId,
      cameraId,
      detection,
      expanded,
      viewWidth,
      viewHeight,
      stageWidth,
      centerX,
      centerY,
      selected.join(","),
    ]),
    performance.now(),
  );
  function invalidate(preserveOverlays = false) {
    revision++;
    if (!preserveOverlays) contentRevision++;
    detailReady = false;
    status = selected.length ? "Waiting to scan…" : "Select at least one scanner.";
  }
  function requestScan(delay = 180, keepPending = false) {
    if (keepPending && timer !== undefined) return;
    clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      void scan();
    }, delay);
  }
  function toggleVisibility(id: string) {
    if (visibleScanners.includes(id)) {
      visibleScanners = visibleScanners.filter((value) => value !== id);
      if (selected.includes(id)) toggle(id);
    } else {
      visibleScanners = [...visibleScanners, id];
      if (!selected.includes(id)) toggle(id);
    }
  }
  function toggle(id: string) {
    selected = selected.includes(id) ? selected.filter((s) => s !== id) : [...selected, id];
    // Selection changes do not change the pixels or invalidate cached results.
    status = selected.length ? "Scan complete" : "Select at least one scanner.";
    if ((source || live) && selected.includes(id)) requestScan();
  }
  // Rasterize a small preview once; gestures only change its compositor transform.
  function preparePreview(image: ImageBitmap | HTMLVideoElement, width: number, height: number) {
    const ratio = Math.min(1, 1600 / Math.max(width, height));
    preview.width = Math.round(width * ratio);
    preview.height = Math.round(height * ratio);
    preview.getContext("2d")!.drawImage(image, 0, 0, preview.width, preview.height);
  }
  function renderDetailedPreview() {
    if (!source || detailRevision === revision) return;
    if (detail.width !== W) detail.width = W;
    if (detail.height !== H) detail.height = H;
    const context = detail.getContext("2d", { willReadFrequently: true })!;
    context.fillStyle = "#142421";
    context.fillRect(0, 0, W, H);
    context.save();
    context.translate(W / 2, H / 2);
    context.rotate((angle * Math.PI) / 180);
    context.scale(fit * scale, fit * scale);
    context.drawImage(source, -mediaWidth / 2 - centerX, -mediaHeight / 2 - centerY);
    context.restore();
    detailRevision = revision;
    detailReady = true;
  }
  function transform() {
    if (!gestureFrame) {
      invalidate(true);
      gestureFrame = requestAnimationFrame(() => {
        gestureFrame = 0;
        requestScan(150, true);
      });
    }
  }
  let centerHint = false;
  let centerHintTimer: ReturnType<typeof setTimeout> | undefined;
  function flashCenter(duration = 900) {
    clearTimeout(centerHintTimer);
    centerHint = true;
    centerHintTimer = setTimeout(() => (centerHint = false), duration);
  }
  function recenter(event: Pick<MouseEvent, "clientX" | "clientY">) {
    if (!source || live || isPdf) return;
    const bounds = surface.getBoundingClientRect();
    const dx = ((event.clientX - bounds.left) / bounds.width - 0.5) * viewWidth;
    const dy = ((event.clientY - bounds.top) / bounds.height - 0.5) * viewHeight;
    const radians = (angle * Math.PI) / 180;
    centerX += (dx * Math.cos(radians) + dy * Math.sin(radians)) / (fit * scale);
    centerY += (-dx * Math.sin(radians) + dy * Math.cos(radians)) / (fit * scale);
    flashCenter();
    invalidate();
    requestScan(0);
  }
  function reset() {
    centerX = centerY = 0;
    invalidate();
    scale = 1;
    angle = 0;
    transform();
  }
  const doubleTap = new DoubleTap();
  let lastTouchTime = -Infinity;
  function mouseRecenter(event: MouseEvent) {
    // Some browsers also emit dblclick after touch; never recenter twice.
    if (performance.now() - lastTouchTime > 800) recenter(event);
  }
  const pointers = new SvelteMap<number, { x: number; y: number }>();
  const clampZoom = (value: number) => Math.max(0.25, Math.min(12, value));
  const wrapAngle = (value: number) => ((((value + 180) % 360) + 360) % 360) - 180;
  function point(event: PointerEvent) {
    const bounds = gestureBounds ?? surface.getBoundingClientRect();
    return {
      x: event.clientX - bounds.left - bounds.width / 2,
      y: event.clientY - bounds.top - bounds.height / 2,
    };
  }
  function suppressImageSelection(node: HTMLElement) {
    const prevent = (event: Event) => event.preventDefault();
    const events = ["contextmenu", "selectstart", "dragstart"];
    for (const event of events) node.addEventListener(event, prevent);
    return {
      destroy() {
        for (const event of events) node.removeEventListener(event, prevent);
      },
    };
  }
  function pointerDown(event: PointerEvent) {
    if (isPdf || !source || event.button !== 0) return;
    if (event.pointerType === "touch") {
      lastTouchTime = performance.now();
      if (!pointers.size) doubleTap.down(event);
      else doubleTap.cancel();
    } else doubleTap.cancel();
    if (!pointers.size) gestureBounds = surface.getBoundingClientRect();
    surface.setPointerCapture(event.pointerId);
    const position = point(event);
    pointers.set(event.pointerId, position);
  }
  function pointerMove(event: PointerEvent) {
    const previous = pointers.get(event.pointerId);
    if (isPdf || !previous || !source) return;
    if (event.pointerType === "touch" && pointers.size === 1 && doubleTap.move(event)) return;
    const next = point(event);
    const other = [...pointers.entries()].find(([id]) => id !== event.pointerId)?.[1];
    if (other) {
      const before = Math.hypot(previous.x - other.x, previous.y - other.y);
      const after = Math.hypot(next.x - other.x, next.y - other.y);
      if (before > 8) {
        scale = clampZoom((scale * after) / before);
        angle = wrapAngle(
          angle +
            wrapAngle(
              ((Math.atan2(next.y - other.y, next.x - other.x) -
                Math.atan2(previous.y - other.y, previous.x - other.x)) *
                180) /
                Math.PI,
            ),
        );
      }
    } else {
      const before = Math.hypot(previous.x, previous.y);
      const after = Math.hypot(next.x, next.y);
      if (before >= 12 && after >= 12) {
        scale = clampZoom((scale * after) / before);
        angle = wrapAngle(
          angle +
            wrapAngle(
              ((Math.atan2(next.y, next.x) - Math.atan2(previous.y, previous.x)) * 180) / Math.PI,
            ),
        );
      }
    }
    pointers.set(event.pointerId, next);
    transform();
  }
  function pointerUp(event: PointerEvent) {
    if (!pointers.has(event.pointerId)) return;
    if (event.pointerType === "touch") {
      lastTouchTime = performance.now();
      if (event.type === "pointerup" && pointers.size === 1) {
        if (doubleTap.up(event)) recenter(event);
      } else doubleTap.cancel();
    }
    pointers.delete(event.pointerId);
    if (!pointers.size) {
      gestureBounds = null;
      requestScan(220);
    }
  }
  function wheel(event: WheelEvent) {
    if (!source || isPdf) return;
    event.preventDefault();
    const delta =
      event.deltaY *
      (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? surface.clientHeight : 1);
    flashCenter(450);
    if (event.shiftKey) angle = wrapAngle(angle + delta * 0.15);
    else scale = clampZoom(scale * Math.exp(-delta * 0.002));
    transform();
  }
  function keyTransform(event: KeyboardEvent) {
    if (
      isPdf ||
      !source ||
      !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)
    )
      return;
    event.preventDefault();
    if (event.key === "ArrowLeft") angle = wrapAngle(angle - 5);
    if (event.key === "ArrowRight") angle = wrapAngle(angle + 5);
    if (event.key === "ArrowUp") scale = clampZoom(scale * 1.1);
    if (event.key === "ArrowDown") scale = clampZoom(scale / 1.1);
    transform();
  }
  function run(
    spec: ComparisonSpec,
    image: ImageData,
    scanFormats: readonly Format[],
  ): Promise<Result> {
    let worker = workers.get(spec.id);
    if (!worker) {
      worker =
        spec.engine === "classical" || spec.engine === "turbo"
          ? new Worker(new URL("./lib/scan.worker.ts", import.meta.url), { type: "module" })
          : spec.engine === "quagga"
            ? new Worker(new URL("./lib/quagga.worker.ts", import.meta.url), { type: "module" })
            : new Worker(new URL("./lib/reference.worker.ts", import.meta.url), { type: "module" });
      workers.set(spec.id, worker);
    }
    const current = worker;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => fail(new Error("Scanner timed out. Try again.")), 60000);
      const clean = () => {
        clearTimeout(timeout);
        pending.delete(spec.id);
      };
      const fail = (reason: Error) => {
        clean();
        current.terminate();
        workers.delete(spec.id);
        reject(reason);
      };
      pending.set(spec.id, fail);
      current.onerror = (event) => fail(new Error(event.message || "Scanner failed"));
      current.onmessage = ({
        data,
      }: MessageEvent<{ type?: string; result: Result; error?: string }>) => {
        if (data.type === "initializing") {
          status = `Loading ${spec.label}…`;
          return;
        }
        if (data.error) {
          fail(new Error(data.error));
          return;
        }
        clean();
        resolve(data.result);
      };
      const buffer = image.data.slice().buffer;
      current.postMessage(
        {
          id: 1,
          engine: spec.engine === "turbo" ? spec.id : spec.engine,
          scannerVersion: spec.version,
          releaseVersion: spec.releaseVersion ?? releaseVersion,
          searchFurther: true,
          finishCandidates: false,
          formats: scanFormats,
          engineBaseUrl: new URL(`${import.meta.env.BASE_URL}engines/`, document.baseURI).href,
          width: image.width,
          height: image.height,
          buffer,
        },
        [buffer],
      );
    });
  }
  function captureScanImage(): ImageData {
    if (source) {
      renderDetailedPreview();
      return detail
        .getContext("2d", { willReadFrequently: true })!
        .getImageData(margin, margin, frameWidth, frameHeight);
    }
    if (input.width !== frameWidth) input.width = frameWidth;
    if (input.height !== frameHeight) input.height = frameHeight;
    ctx.fillStyle = "#142421";
    ctx.fillRect(0, 0, frameWidth, frameHeight);
    ctx.save();
    ctx.translate(frameWidth / 2, frameHeight / 2);
    ctx.scale(fit, fit);
    ctx.drawImage(video, -mediaWidth / 2, -mediaHeight / 2);
    ctx.restore();
    return ctx.getImageData(0, 0, frameWidth, frameHeight);
  }
  async function saveImage() {
    if (saving || (!source && !live)) return;
    saving = true;
    try {
      const image = live ? lastScanImage : captureScanImage();
      if (!image || (live && lastScanContent !== contentRevision))
        throw new Error("Wait for a camera frame to be analyzed.");
      const canvas = document.createElement("canvas");
      canvas.width = image.width;
      canvas.height = image.height;
      canvas.getContext("2d")!.putImageData(image, 0, 0);
      const blob = await new Promise<Blob>((resolve, reject) =>
        canvas.toBlob(
          (blob) => (blob ? resolve(blob) : reject(new Error("PNG export failed"))),
          "image/png",
        ),
      );
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `tapirscan-input-${image.width}x${image.height}-${Date.now()}.png`;
      document.body.append(link);
      link.click();
      link.remove();
      setTimeout(() => URL.revokeObjectURL(url), 60000);
    } catch (reason) {
      error = `Could not save image: ${String(reason)}`;
    } finally {
      saving = false;
    }
  }
  // Paint the stream ourselves so iOS video-layer sizing cannot shrink the preview.
  // This canvas is display-only; scanning and PNG export retain the selected resolution.
  function paintLivePreview(time: number) {
    if (!live || disposed) return;
    if (!document.hidden && !source && video.readyState >= 2 && time - lastPreviewTime >= 33) {
      cameraResize();
      const ratio = Math.min(1, 1600 / Math.max(video.videoWidth, video.videoHeight));
      const width = Math.max(1, Math.round(video.videoWidth * ratio));
      const height = Math.max(1, Math.round(video.videoHeight * ratio));
      if (livePreview.width !== width) livePreview.width = width;
      if (livePreview.height !== height) livePreview.height = height;
      livePreview.getContext("2d")!.drawImage(video, 0, 0, width, height);
      lastPreviewTime = time;
    }
    livePreviewFrame = requestAnimationFrame(paintLivePreview);
  }
  async function scan() {
    if (disposed || (!source && !live)) return;
    if (busy || !selected.length || (live && document.hidden)) return;
    busy = true;
    const scanFormats = formats;
    let token = revision;
    let contentToken = contentRevision;
    let frameFailed = false;
    const start = performance.now();
    if (!live || !entries.length) status = "Scanning…";
    try {
      if (live && captureMode === "photos") {
        const next = await videoFrame(video);
        if (token !== revision || !live || disposed) {
          next.close();
          return;
        }
        source?.close();
        source = next;
        centerX = centerY = 0;
        scale = 1;
        angle = 0;
        const sameSize = next.width === mediaWidth && next.height === mediaHeight;
        setFrame(next.width, next.height);
        captureInfo = `Video snapshot · ${next.width} × ${next.height}`;
        preparePreview(next, next.width, next.height);
        invalidate(sameSize);
        token = revision;
        contentToken = contentRevision;
        await tick();
      }
      const image = captureScanImage();
      lastScanImage = image;
      lastScanContent = contentToken;
      const view = {
        viewRevision: token,
        contentRevision: contentToken,
        angle,
        scale,
        width: frameWidth,
        height: frameHeight,
      };
      const prep = performance.now() - start;
      const batch: ViewEntry[] = [];
      for (const spec of chosen) {
        if (contentToken !== contentRevision || disposed) break;
        if (!selected.includes(spec.id)) continue;
        if (!live && entries.some((entry) => entry.id === spec.id && entry.viewRevision === token))
          continue;
        const settingsRevision = tapirscanRevision;
        try {
          if (!workers.has(spec.id)) {
            status = `Warming up ${spec.label}…`;
            await run(spec, image, scanFormats);
            // A new worker's first scan warms its runtime; display the second scan.
            if (
              contentToken !== contentRevision ||
              disposed ||
              !selected.includes(spec.id) ||
              (spec.engine === "classical" && settingsRevision !== tapirscanRevision)
            )
              continue;
          }
          batch.push({ ...spec, ...view, result: await run(spec, image, scanFormats) });
        } catch (reason) {
          batch.push({ ...spec, ...view, error: String(reason) });
        }
        if (spec.engine === "classical" && settingsRevision !== tapirscanRevision) {
          batch.pop();
          continue;
        }
        if (contentToken !== contentRevision || disposed) break;
        // Publish this decoder immediately, without waiting for later decoders.
        const completed = batch[batch.length - 1];
        entries = [...entries.filter((entry) => entry.id !== spec.id), completed];
      }
      if (token === revision && !disposed) {
        if (batch.length) {
          preparationMs = prep;
          totalMs = performance.now() - start;
        }
        status = !selected.length
          ? "Select at least one scanner."
          : entries.some(
                (e) =>
                  e.viewRevision === token &&
                  selected.includes(e.id) &&
                  e.error &&
                  !qrOnlyUnavailable(e.id),
              )
            ? "Some scanners failed. See results below."
            : "Scan complete";
      }
    } catch (reason) {
      frameFailed = true;
      if (token === revision) {
        if (live && captureMode === "photos") stopCamera();
        error = `Capture or scan failed: ${String(reason)}. Try live video or Take photo.`;
      }
    } finally {
      busy = false;
      const missing = selected.some(
        (id) => !entries.some((entry) => entry.id === id && entry.viewRevision === revision),
      );
      if (!disposed && (live || token !== revision || (missing && !frameFailed)))
        requestScan(live ? Math.max(0, 500 - (performance.now() - start)) : 220);
    }
  }
  function stopCamera() {
    cancelAnimationFrame(livePreviewFrame);
    lastScanImage = null;
    doubleTap.cancel();
    clearTimeout(timer);
    timer = undefined;
    invalidate();
    zoomRange = undefined;
    cameraId++;
    opening = false;
    live = false;
    stream?.getTracks().forEach((track) => track.stop());
    stream = null;
    if (video) video.srcObject = null;
    torch = false;
    hasTorch = false;
  }
  async function stopVideo() {
    selectedDemo = "";
    if (source && captureMode === "photos") {
      stopCamera();
      requestScan();
    } else if (live && video.videoWidth) {
      const token = cameraId;
      const next = await createImageBitmap(video);
      if (token !== cameraId || disposed) {
        next.close();
        return;
      }
      stopCamera();
      source?.close();
      source = next;
      setFrame(next.width, next.height);
      preparePreview(next, next.width, next.height);
      captureInfo = `Frozen video frame · ${next.width} × ${next.height}`;
      reset();
    } else stopCamera();
    status = "Camera stopped · frame ready to compare";
  }
  function takeSinglePhoto() {
    stopCamera();
    photoInput.click();
  }
  async function changeMode() {
    if (stream) await startCamera();
  }
  async function changeResolution() {
    if (live && captureMode === "video") {
      await startCamera();
    } else if (source) {
      setFrame(source.width, source.height);
      invalidate();
      requestScan();
    }
  }
  async function setZoom() {
    try {
      const track = stream?.getVideoTracks()[0];
      await track?.applyConstraints({
        advanced: [{ zoom: cameraZoom } as globalThis.MediaTrackConstraintSet],
      });
    } catch {
      cameraNote = "This camera could not apply the requested zoom.";
    }
  }
  function cameraResize() {
    if (
      live &&
      captureMode === "video" &&
      video.videoWidth &&
      (video.videoWidth !== mediaWidth || video.videoHeight !== mediaHeight)
    ) {
      setFrame(video.videoWidth, video.videoHeight);
      invalidate();
      requestScan();
    }
  }
  async function startCamera() {
    clearPdf();
    stopCamera();
    const token = ++cameraId;
    ++loadId;
    opening = true;
    error = "";
    cameraNote = "";
    try {
      const next = await navigator.mediaDevices.getUserMedia({
        audio: false,
        video: {
          ...(selectedCamera
            ? { deviceId: { exact: selectedCamera } }
            : { facingMode: { ideal: "environment" } }),
          width: {
            ideal: resolution === "original" ? 10000 : Math.round((Number(resolution) * 16) / 9),
          },
          height: { ideal: resolution === "original" ? 10000 : Number(resolution) },
          ...(resolution === "original" ? { resizeMode: { ideal: "none" } } : {}),
          frameRate: { ideal: 30 },
        },
      });
      if (token !== cameraId || disposed) {
        next.getTracks().forEach((t) => t.stop());
        return;
      }
      stream = next;
      video.srcObject = stream;
      await video.play();
      if (token !== cameraId || disposed) return;
      source?.close();
      source = null;
      cameraPaused = false;
      detailReady = false;
      centerX = centerY = 0;
      scale = 1;
      angle = 0;
      setFrame(video.videoWidth, video.videoHeight);
      invalidate();
      selectedDemo = "";
      cameraFrame = true;
      live = true;
      await tick();
      if (token !== cameraId || disposed) return;
      cameraResize();
      lastPreviewTime = 0;
      livePreviewFrame = requestAnimationFrame(paintLivePreview);
      const track = next.getVideoTracks()[0];
      const caps = capabilities(track);
      hasTorch = !!caps.torch;
      zoomRange = caps.zoom;
      cameraZoom =
        (track.getSettings() as globalThis.MediaTrackSettings & { zoom?: number }).zoom ??
        caps.zoom?.min ??
        1;
      if (caps.focusMode?.includes("continuous")) {
        try {
          await track.applyConstraints({
            advanced: [{ focusMode: "continuous" } as globalThis.MediaTrackConstraintSet],
          });
        } catch {
          cameraNote = "Using the camera’s default focus settings.";
        }
      }
      if (token !== cameraId || disposed) return;
      try {
        cameras = (await navigator.mediaDevices.enumerateDevices()).filter(
          (device) => device.kind === "videoinput",
        );
      } catch {
        cameras = [];
      }
      if (token !== cameraId || disposed) return;
      selectedCamera = track.getSettings().deviceId ?? "";
      captureInfo = `Video · ${video.videoWidth} × ${video.videoHeight} · requested ${resolution === "original" ? "original" : resolution === "2160" ? "4K" : "Full HD"}`;
      requestScan();
    } catch (reason) {
      if (token === cameraId) {
        stopCamera();
        error = `Camera unavailable: ${String(reason)}. You can load an image instead.`;
      }
    } finally {
      if (token === cameraId) opening = false;
    }
  }
  async function light() {
    try {
      await stream
        ?.getVideoTracks()[0]
        .applyConstraints({ advanced: [{ torch: !torch } as { torch: boolean; width?: number }] });
      torch = !torch;
    } catch (reason) {
      error = `Could not change the light: ${String(reason)}`;
    }
  }
  async function loadImage(
    blob: Blob,
    token: number,
    initialAngle = 0,
    initialScale = 1,
    preservePdf = false,
  ) {
    if (token !== loadId || disposed) return;
    invalidate();
    status = "Opening image…";
    let next: ImageBitmap;
    try {
      next = await createImageBitmap(blob);
    } catch {
      if (token === loadId) error = "This image could not be opened. Try a JPEG, PNG, or WebP.";
      return;
    }
    if (token !== loadId || disposed) {
      next.close();
      return;
    }
    if (!preservePdf) clearPdf();
    stopCamera();
    source?.close();
    source = next;
    cameraFrame = false;
    captureInfo = `Photo · ${next.width} × ${next.height}`;
    cameraPaused = false;
    setFrame(next.width, next.height);
    preparePreview(next, next.width, next.height);
    error = "";
    centerX = centerY = 0;
    scale = initialScale;
    angle = initialAngle;
    transform();
  }
  async function demo() {
    clearPdf();
    selectedDemo ||= "pesto.jpg";
    const token = ++loadId;
    invalidate();
    status = "Loading demo image…";
    try {
      const response = await fetch(`${import.meta.env.BASE_URL}images/${selectedDemo}`);
      if (!response.ok) throw new Error("Demo image unavailable");
      await loadImage(
        await response.blob(),
        token,
        selectedDemo === "sauce.jpg" ? 34 : selectedDemo === "pills.jpg" ? 45 : 0,
        selectedDemo === "sauce.jpg"
          ? 2.21
          : selectedDemo === "pills.jpg"
            ? 2.33
            : selectedDemo === "pesto.jpg"
              ? 1.2
              : 1,
      );
    } catch (reason) {
      if (token === loadId) error = String(reason);
    }
  }
  function upload(event: Event) {
    const target = event.currentTarget as HTMLInputElement;
    const file = target.files?.[0];
    if (file) {
      selectedDemo = "";
      const token = ++loadId;
      if (file.type === "application/pdf" || /\.pdf$/i.test(file.name)) void loadPdf(file, token);
      else {
        clearPdf();
        void loadImage(file, token);
      }
    }
    target.value = "";
  }
  onMount(() => {
    document.addEventListener("fullscreenchange", fullscreenChanged);
    void demo();
    // Some mobile browsers pause an offscreen video without ending its stream.
    const resumePreview = () => {
      if (live && document.visibilityState === "visible") {
        requestScan();
        void video.play().catch(() => {
          status = "Camera preview paused · stop and restart video to retry";
        });
      }
    };
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting) resumePreview();
    });
    observer.observe(video);
    document.addEventListener("visibilitychange", resumePreview);
    return () => {
      document.removeEventListener("fullscreenchange", fullscreenChanged);
      if (expanded) {
        void exitViewer();
        restoreViewer();
      }
      observer.disconnect();
      document.removeEventListener("visibilitychange", resumePreview);
      clearTimeout(centerHintTimer);
      disposed = true;
      clearPdf();
      ++loadId;
      stopCamera();
      clearTimeout(timer);
      cancelAnimationFrame(gestureFrame);
      source?.close();
      for (const reject of pending.values()) reject(new Error("Demo closed"));
      for (const worker of workers.values()) worker.terminate();
    };
  });
</script>

<svelte:window
  bind:innerWidth={windowWidth}
  bind:innerHeight={windowHeight}
  on:keydown={viewerKey}
/>

<div class="demo">
  <div class="heading">
    <a href={import.meta.env.BASE_URL} class="wordmark">▥ <span>Tapirscan</span></a>
    <div class="heading-links">
      <span class="private">Images stay in your browser</span>
      <a class="github" href="https://github.com/kleinicke/tapirscan">
        <svg viewBox="0 0 16 16" aria-hidden="true"
          ><path
            d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.37A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"
          /></svg
        ><span><span class="github-prefix">View on&nbsp;</span>GitHub</span>
      </a>
    </div>
  </div>
  <main>
    <p class="scanner-key">TS = Tapirscan · Low, Med, High and VHigh indicate scan effort.</p>
    <p class="hint">
      TS-Low is the former Turbo reader. Optional Turbo experiments are under More scanners; their
      numbers target multiples of TS-Low's Common1D speed. Actual gains vary by image and device,
      and faster tiers miss more difficult codes. Common includes QR and Data Matrix; 2D selects
      only matrix formats. All Turbo tiers share the same 2D search. All checks every format and
      takes longer. TS-Low uses the selected public Low build. TS-Low Classic and Turbo experiments
      use fixed builds.
    </p>
    <div class="scanner-buttons" aria-label="Scanners">
      {#each options.filter((option) => visibleScanners.includes(option.id)) as option (option.id)}
        {@const active = selected.includes(option.id)}
        <!-- Keep the last completed outcome visible until this scanner finishes again. -->
        {@const entry = entries.find((value) => value.id === option.id)}
        {@const count = new Set(
          entry?.result?.regions.filter((region) => region.text).map((region) => region.text) ?? [],
        ).size}
        {@const outcome = !active
          ? "Paused"
          : entry?.error
            ? "Failed"
            : entry?.result
              ? count
                ? `✓ Found ${count}`
                : "Found 0"
              : busy
                ? "Scanning…"
                : "Ready"}
        <button
          aria-pressed={active}
          aria-label={`${option.label}: ${outcome}`}
          class:chosen={active}
          class:has-reads={active && !!entry?.result && count > 0}
          data-found-tier={active ? Math.min(count, 3) : 0}
          class:failed={active && !!entry?.error}
          style:--scanner-color={option.color}
          on:click={() => toggle(option.id)}
        >
          <span class="scanner-label"><i></i>{option.label}</span>
          <span class="scanner-outcome">{outcome}</span>
          <span class="scanner-time"
            >{active && entry?.result ? `${entry.result.scanMs.toFixed(1)} ms` : " "}</span
          >
        </button>
      {/each}
    </div>
    <details class="extra-scanners">
      <summary>More scanners ({visibleScanners.length} shown)</summary>
      <div class="scanner-menu">
        {#each options as option (option.id)}
          <label>
            <input
              type="checkbox"
              checked={visibleScanners.includes(option.id)}
              on:change={() => toggleVisibility(option.id)}
            />
            <span style:color={option.color}>{option.label}</span>
          </label>
        {/each}
      </div>
    </details>
    <div class="viewer" class:expanded bind:this={viewer}>
      <div
        class="stage"
        bind:clientWidth={stageWidth}
        use:suppressImageSelection
        style:aspect-ratio={`${viewWidth} / ${viewHeight}`}
        style:max-width={expanded
          ? `${Math.min(windowWidth, (windowHeight * viewWidth) / viewHeight)}px`
          : `min(100%, calc(66svh * ${viewWidth} / ${viewHeight}))`}
      >
        <video
          bind:this={video}
          class:visible={live && !source}
          muted
          playsinline
          aria-label="Live camera"
          on:loadedmetadata={cameraResize}
          on:resize={cameraResize}
        ></video>
        <canvas
          bind:this={livePreview}
          class="live-preview"
          class:visible={live && !source}
          aria-label="Live camera preview"
        ></canvas>
        <canvas
          bind:this={preview}
          class="image-preview"
          class:visible={(!!source && !detailReady) || cameraPaused}
          style:width={`${displayWidth}%`}
          style:height={`${displayHeight}%`}
          style:transform={`translate(-50%, -50%) rotate(${angle}deg) scale(${scale}) translate(${(-centerX / mediaWidth) * 100}%, ${(-centerY / mediaHeight) * 100}%)`}
          aria-label="Image preview"
        ></canvas>
        <canvas
          bind:this={detail}
          class="detail-preview"
          style:width={`${(W / viewWidth) * 100}%`}
          style:height={`${(H / viewHeight) * 100}%`}
          style:left={`${(-viewOffset / viewWidth) * 100}%`}
          style:top={`${(-viewOffset / viewHeight) * 100}%`}
          class:visible={!!source && detailReady}
          aria-label="Captured image and surrounding context"
        ></canvas>
        <canvas
          width="1"
          height="1"
          class="gesture-surface"
          style:pointer-events={source && !live && !isPdf ? "auto" : "none"}
          style:touch-action={source && !isPdf ? "none" : "pan-y"}
          bind:this={surface}
          tabindex={isPdf ? -1 : 0}
          aria-hidden={isPdf}
          aria-label="Image controls. Drag toward the center to zoom out, away to zoom in, or around it to rotate. Scroll to zoom. Shift-scroll to rotate. Double-click or double-tap a point to center it without changing zoom."
          on:pointerdown={pointerDown}
          on:pointermove={pointerMove}
          on:pointerup={pointerUp}
          on:pointercancel={pointerUp}
          on:lostpointercapture={pointerUp}
          on:dblclick={mouseRecenter}
          on:wheel|nonpassive={wheel}
          on:keydown={keyTransform}
          on:contextmenu|preventDefault={() => {}}
          on:dragstart|preventDefault={() => {}}
        ></canvas>
        <span
          class="center-marker"
          class:shown={!!source && !live && (pointers.size > 0 || centerHint)}
          aria-hidden="true"
        ></span>
        <svg
          class="scan-overlay"
          viewBox={`${viewOffset} ${viewOffset} ${viewWidth} ${viewHeight}`}
          aria-label="Analysis frame and barcode results"
        >
          {#if !frameOnly}<path
              d={`M0 0H${W}V${H}H0Z M${margin} ${margin}V${margin + frameHeight}H${margin + frameWidth}V${margin}Z`}
              fill="#071512"
              fill-opacity=".62"
              fill-rule="evenodd"
            />
            <rect
              x="240"
              y="240"
              width={frameWidth}
              height={frameHeight}
              fill="none"
              stroke="#edf5df"
              stroke-width="1.5"
              vector-effect="non-scaling-stroke"
            />
            <text x="240" y="195" fill="#edf5df" font-size="30"
              >SCAN · {frameWidth} × {frameHeight}</text
            >{/if}
          <svg
            x={margin}
            y={margin}
            width={frameWidth}
            height={frameHeight}
            viewBox={`0 0 ${frameWidth} ${frameHeight}`}
            overflow="hidden"
          >
            {#each overlayEntries as entry (entry.id)}
              <g transform={overlayTransform(entry, angle, scale)}>
                {#if showAreas}
                  {#each entry.result?.searchWindows ?? [] as area, i (i)}
                    <polygon
                      points={area.polygon.map((p) => p.join(",")).join(" ")}
                      fill={entry.color}
                      fill-opacity=".07"
                      stroke={entry.color}
                      stroke-width="1"
                      stroke-dasharray="6 4"
                      vector-effect="non-scaling-stroke"
                    />
                  {/each}
                  {#each entry.result?.proposals ?? [] as area, i (i)}
                    <polygon
                      points={area.polygon.map((p) => p.join(",")).join(" ")}
                      fill={entry.color}
                      fill-opacity=".1"
                      stroke={entry.color}
                      stroke-width="1"
                      stroke-dasharray="4 3"
                      vector-effect="non-scaling-stroke"
                    />
                  {/each}
                {/if}
                {#each (entry.result?.regions ?? []).filter((region) => region.text || showAreas) as region, i (i)}
                  <polygon
                    points={region.polygon.map((p) => p.join(",")).join(" ")}
                    fill="none"
                    stroke={entry.color}
                    stroke-width="2.5"
                    stroke-dasharray={region.text ? undefined : "5 4"}
                    vector-effect="non-scaling-stroke"
                  >
                    <title>{entry.label}: {region.text || "Located, unreadable"}</title>
                  </polygon>
                {/each}
              </g>
            {/each}
          </svg>
          {#each labels.placed as label (label.id)}
            <g transform={`translate(${viewOffset} ${viewOffset})`} aria-label={label.text}>
              <title>{label.text}</title>
              <rect
                x={label.x}
                y={label.y}
                width={label.width}
                height={label.height}
                rx={3 * labelUnit}
                fill="#102724"
                stroke="#b7c9c0"
                vector-effect="non-scaling-stroke"
              />
              <text
                class="region-label"
                x={label.x + 6 * labelUnit}
                y={label.y + 16 * labelUnit}
                fill="white"
                style:font-size={`${12 * labelUnit}px`}>{label.displayText}</text
              >
              {#each label.rows as row (row.id)}
                <text
                  class="region-label"
                  x={label.x + row.offset * labelUnit}
                  y={label.y + 29 * labelUnit}
                  fill={row.color}
                  opacity={row.found ? 1 : 0}
                  aria-hidden={!row.found}
                  style:font-size={`${9 * labelUnit}px`}>{row.label}</text
                >
              {/each}
            </g>
          {/each}
        </svg>
        {#if showAdjust && source && !live && !isPdf}
          <div
            class="edge-adjust"
            style:--adjust-track-height={`${Math.max(24, Math.min(65, ((stageWidth * viewHeight) / viewWidth - 148) / 2))}px`}
          >
            <SpringSlider
              label="Rotate"
              value={`${Math.round(angle)}°`}
              negative="↶"
              positive="↷"
              move={(amount) => {
                angle = wrapAngle(angle + amount * 90);
                transform();
              }}
            />
            <SpringSlider
              label="Zoom"
              value={`${scale.toFixed(2)}×`}
              negative="−"
              positive="+"
              move={(amount) => {
                scale = clampZoom(scale * Math.exp(amount));
                transform();
              }}
            />
          </div>
        {/if}
        {#if !source && !live && !cameraPaused && !entries.length}<div class="welcome">
            <h1>{opening ? "Opening your camera…" : "A clearer view of every barcode."}</h1>
            <p>Try a photo, or scan with your camera.</p>
            <button class="primary" on:click={() => void demo()}>Try demo image</button>
          </div>{/if}
        {#if busy || overlaysUpdating}<span class="scanning"
            >{overlaysUpdating ? "Previous outlines · updating…" : status}</span
          >{/if}
      </div>
      {#if expanded}<div class="viewer-header">
          <button
            class="exit-viewer"
            on:click={() => void exitViewer()}
            aria-label="Exit fullscreen">← Back</button
          >
          {#if source && !live && !isPdf}<button
              class="adjust-button"
              aria-pressed={showAdjust}
              on:click={() => (showAdjust = !showAdjust)}>Adjust</button
            >{/if}
          <div class="runtime-strip" aria-label="Scanner runtimes">
            {#each chosen as spec (spec.id)}
              {@const entry = entries.find(
                (item) => item.id === spec.id && item.contentRevision === contentRevision,
              )}
              <span style:color={spec.color}
                ><span>{spec.label}</span><strong
                  >{entry?.result
                    ? `${entry.result.scanMs.toFixed(1)} ms`
                    : entry?.error
                      ? "Failed"
                      : "—"}</strong
                ></span
              >
            {/each}
          </div>
        </div>{/if}
    </div>
    {#if pdfDocument}
      <div class="viewer-actions" aria-label="PDF pages">
        <button disabled={pdfBusy || pdfPage <= 1} on:click={() => void showPdfPage(pdfPage - 1)}
          >Previous page</button
        >
        <span>Page {pdfPage} of {pdfDocument.numPages}{pdfBusy ? " · Loading…" : ""}</span>
        <button
          disabled={pdfBusy || pdfPage >= pdfDocument.numPages}
          on:click={() => void showPdfPage(pdfPage + 1)}>Next page</button
        >
        <span class="hint">Whole-page scanning · no zoom or rotation</span>
      </div>
    {/if}
    <div class="viewer-actions">
      <button bind:this={fullscreenButton} on:click={() => void enterViewer()}>Fullscreen</button>
      {#if source && !live && !isPdf}<button
          aria-pressed={showAdjust}
          on:click={() => (showAdjust = !showAdjust)}>Adjust</button
        >{/if}
      <button
        on:click={() => void saveImage()}
        disabled={saving ||
          (!source && !live) ||
          (live && (!lastScanImage || lastScanContent !== contentRevision))}
        title="Save the scanner input as PNG, without overlays"
        >{saving ? "Saving…" : "Save image"}</button
      >
      {#if source && !isPdf}<span class="gesture-hint"
          >Drag to rotate and zoom · Double-tap / double-click to center</span
        >{/if}
      <label class="resolution-control"
        >Detect<select
          bind:value={detection}
          on:change={changeDetection}
          aria-describedby="detection-note"
        >
          <option value="ean13">EAN-13</option>
          <option value="retail">Retail</option>
          <option value="common1d">Common1D</option>
          <option value="common">Common</option>
          <option value="matrix">2D</option>
          <option value="all">All</option>
        </select></label
      >
      <label class="resolution-control"
        >Read up to<select
          aria-label="Resolution"
          bind:value={resolution}
          on:change={changeResolution}
          disabled={opening}
        >
          <option value="1080">Full HD</option><option value="2160">4K</option><option
            value="original">Original</option
          >
        </select></label
      >
    </div>
    {#if labels.hidden}<p class="hint">
        {labels.hidden} labels hidden to avoid overlap; see Results.
      </p>{/if}
    <div class="controls essential-controls">
      <div class="control-group examples-group">
        <span class="control-heading" id="examples-heading">Examples</span>
        <div class="demo-images" aria-label="Demo images">
          {#each demoImages as image (image.file)}
            <button
              aria-pressed={selectedDemo === image.file && !!source && !live}
              on:click={() => {
                selectedDemo = image.file;
                void demo();
              }}>{image.label}</button
            >
          {/each}
        </div>
      </div>
      <div class="control-group own-group" role="group" aria-labelledby="own-heading">
        <span class="control-heading" id="own-heading">Try your own</span>
        <div class="own-buttons">
          <label class="upload"
            >Load image / PDF<input
              type="file"
              accept="image/*,application/pdf,.pdf"
              on:change={upload}
            /></label
          >
          <button on:click={live ? stopVideo : startCamera} disabled={opening}
            >{opening ? "Opening camera…" : live ? "Pause / freeze" : "Use camera"}</button
          >
        </div>
      </div>
    </div>
    <p class="hint" id="detection-note">
      {formats.join(", ")}.{#if detection === "common" || detection === "all"}
        ZBar scans its supported formats only; it skips Data Matrix{detection === "all"
          ? ", PDF417, Aztec and MaxiCode"
          : ""}.{/if}
    </p>
    <input
      bind:this={photoInput}
      type="file"
      accept="image/*"
      capture="environment"
      on:change={upload}
      hidden
      aria-label="Take photo with device camera"
    />
    <section class="scan-results" aria-label="Scan results">
      <div class="runtime-heading">
        <h2>Results</h2>
        <span role="status">{selected.length ? status : "Select at least one scanner."}</span>
      </div>
      <div class="runtimes">
        {#each chosen as spec (spec.id)}
          {@const entry = overlayEntries.find((e) => e.id === spec.id)}
          {@const reads = entry?.result?.regions.filter((r) => r.text) ?? []}
          <div
            class="runtime"
            class:found={reads.length > 0}
            data-scanner={spec.id}
            style:--scanner-color={spec.color}
          >
            <div class="result-title">
              <span>{options.find((o) => o.id === spec.id)?.label ?? spec.label}</span><strong
                >{entry?.result ? entry.result.scanMs.toFixed(1) : "—"}<small> ms</small></strong
              >
            </div>
            <div class="barcode-values">
              {#each reads as region, i (i)}<code>{region.text}</code>{:else}<span
                  >{qrOnlyUnavailable(spec.id)
                    ? "Not available for barcodes — QR only"
                    : entry?.error
                      ? "Scan failed"
                      : entry?.result
                        ? "No barcode found"
                        : busy
                          ? "Scanning…"
                          : "Ready"}</span
                >{/each}
            </div>
            {#if entry && entry.viewRevision !== revision}<p class="hint">
                Previous result · updating…
              </p>{/if}
            {#if entry?.error && !qrOnlyUnavailable(spec.id)}<p class="error">{entry.error}</p>{/if}
          </div>
        {/each}
      </div>
    </section>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <p class="hint camera-info">
      {captureInfo}{captureInfo
        ? ` · scanning ${frameWidth} × ${frameHeight}`
        : "One resolution setting for video and photos."}
    </p>
    {#if cameraNote}<p class="hint" role="status">{cameraNote}</p>{/if}
    <details class="more-options">
      <summary>More options</summary>
      <p class="hint">
        Compare readers on the same pixels. ZBar scans the supported subset of selected formats.
        Scanner times exclude initialization, the one-time warm-up scan, and transfers.
        {#if totalMs}Preparation {preparationMs.toFixed(1)} ms · Total {totalMs.toFixed(1)} ms.{/if}
      </p>
      <div class="camera-settings">
        <label class="version-picker"
          >Tapirscan version
          <select
            aria-label="Tapirscan version"
            bind:value={releaseVersion}
            on:change={changeVersion}
          >
            {#each scannerVersions.versions as release (release.version)}<option
                value={release.version}>{release.label}</option
              >{/each}
          </select>
        </label>

        <label
          >Capture mode<select
            aria-label="Capture mode"
            bind:value={captureMode}
            on:change={changeMode}
            disabled={opening}
          >
            <option value="video">Live video</option><option value="photos"
              >Repeated snapshots</option
            >
          </select></label
        >
        {#if cameras.length > 1}<label
            >Camera<select
              aria-label="Camera"
              bind:value={selectedCamera}
              on:change={changeMode}
              disabled={opening}
            >
              {#each cameras as camera, index (camera.deviceId || index)}<option
                  value={camera.deviceId}>{camera.label || `Camera ${index + 1}`}</option
                >{/each}
            </select></label
          >{/if}
        {#if live && zoomRange && zoomRange.max > zoomRange.min}<label
            >Camera zoom <output>{cameraZoom.toFixed(1)}×</output><input
              aria-label="Camera zoom"
              type="range"
              min={zoomRange.min}
              max={zoomRange.max}
              step={zoomRange.step || 0.1}
              bind:value={cameraZoom}
              on:change={setZoom}
            /></label
          >{/if}
      </div>
      {#if showAreas}
        <section aria-label="Analyzed area counts">
          <h3>Analyzed areas</h3>
          {#each overlayEntries.filter((entry) => entry.engine === "classical" && entry.result?.areaCounts) as entry (entry.id)}
            {@const counts = entry.result!.areaCounts!}
            <p>
              <strong>{entry.label}</strong>: {counts.proposed} proposed areas · {counts.checked} primary
              candidates checked · {counts.withoutRead} without a primary EAN13/UPCA read · {counts.omitted ??
                "not reported"} omitted by the localization limit.
            </p>
          {/each}
          <p class="hint">
            These are reported search areas, not barcode counts. Primary candidate counts cover the
            EAN13/UPCA search. EAN8, UPCE and other selected formats may still be decoded in these
            areas. Fast-discarded area counts are not exposed by these scanner builds; no primary
            read does not mean the area was quickly rejected.
          </p>
        </section>
      {/if}
      <div class="controls">
        <button on:click={takeSinglePhoto} disabled={opening}>Take photo</button>
        {#if live && hasTorch}<button on:click={light} aria-pressed={torch}
            >{torch ? "Light on" : "Light"}</button
          >{/if}
        <button on:click={() => (showAreas = !showAreas)} aria-pressed={showAreas}
          >Show analyzed areas</button
        >
      </div>
      <p class="hint">
        Original keeps the image’s pixels, up to the 32 MP scan limit. For video it requests the
        highest available resolution; actual capture dimensions are shown above. Smaller images are
        never upscaled.
      </p>
      {#if captureMode === "photos"}<p class="hint">
          Capture one video frame, compare all selected scanners, then capture the next. For a
          separate photo from your phone’s camera, use Take photo.
        </p>{/if}
      {#if source && !live && !isPdf}
        <div class="image-controls">
          <label
            >Zoom <output>{scale.toFixed(2)}×</output><input
              aria-label="Zoom"
              type="range"
              min="0.25"
              max="12"
              step="0.01"
              bind:value={scale}
              on:input={transform}
              on:dblclick={() => {
                scale = 1;
                transform();
              }}
              title="Double-click to reset zoom to 1×"
            /></label
          >
          <label
            >Rotation <output>{Math.round(angle)}°</output><input
              aria-label="Rotation"
              type="range"
              min="-180"
              max="180"
              step="1"
              bind:value={angle}
              on:input={transform}
              on:dblclick={() => {
                angle = 0;
                transform();
              }}
              title="Double-click to reset rotation to 0°"
            /></label
          >
          <button
            on:click={() => {
              angle = ((angle + 270) % 360) - 180;
              transform();
            }}>Rotate 90°</button
          ><button on:click={reset}>Reset image</button>
        </div>
        <p class="hint">
          Drag outward to zoom in, inward to zoom out, or around the center to rotate. Scroll to
          zoom · Shift-scroll to rotate · Double-click a slider to reset it.
        </p>
      {/if}
    </details>
    {#if showAreas}<p class="hint">
        Dashed outlines show reported candidate regions and search windows. An unfinished search
        does not guarantee that every visible symbol was examined.
      </p>{/if}
    <footer>
      <span>Local processing. No image or PDF uploads.</span>
      <nav aria-label="Project and legal links">
        <a href="https://www.npmjs.com/package/tapirscan">npm</a>
        <a href="https://pypi.org/project/tapirscan/">PyPI</a>
        <a href="https://crates.io/crates/tapirscan">crates.io</a>
        <a href="https://github.com/kleinicke/tapirscan">GitHub</a>
        <a href="https://f-kleinicke.de/">About</a>
        <a href="https://f-kleinicke.de/impressum">Impressum</a>
        <a href={`${import.meta.env.BASE_URL}THIRD_PARTY_NOTICES.txt`}>Third-party licenses</a>
      </nav>
      <span>Tapirscan · v{version}</span>
    </footer>
  </main>
</div>

<style>
  .extra-scanners {
    position: relative;
    margin: 10px 0;
  }
  .extra-scanners summary {
    cursor: pointer;
    font-weight: 600;
  }
  .scanner-menu {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 16px;
    padding: 10px 12px;
    background: #173633;
    color: white;
    border-radius: 12px;
    margin-top: 8px;
  }
  .scanner-menu label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
    font-size: 13px;
    white-space: nowrap;
    cursor: pointer;
  }

  .demo {
    max-width: 1120px;
    margin: auto;
  }
  .heading {
    padding: 24px 32px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .wordmark {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 23px;
    font-weight: 700;
    text-decoration: none;
    letter-spacing: -0.6px;
  }
  .heading-links {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .github {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 9px 16px;
    border-radius: 9px;
    background: #173633;
    color: #fff;
    font-size: 14px;
    font-weight: 600;
    text-decoration: none;
    white-space: nowrap;
  }
  .github:hover {
    background: #2a5550;
  }
  .github svg {
    width: 18px;
    height: 18px;
    fill: currentColor;
  }
  .private,
  .hint {
    font-size: 12px;
    color: #61776b;
  }
  main {
    padding: 12px 32px 24px;
  }
  .controls.essential-controls {
    align-items: flex-end;
    gap: 16px;
  }
  .control-group {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .control-heading {
    font-size: 12px;
    font-weight: 600;
    color: #52665f;
  }
  .own-group {
    margin-left: auto;
  }
  .own-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .demo-images button {
    background: #e7eae7;
    border-color: #cbd1cc;
  }
  .own-buttons button,
  .own-buttons .upload {
    background: #e3eff8;
    border-color: #bacfdf;
    color: #234e6d;
  }
  .gesture-hint {
    margin: 0;
    flex: 1;
    font-size: 12px;
    color: #61776b;
  }
  .demo-images {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .resolution-control {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    margin-left: 0;
  }
  .resolution-control select {
    min-height: 44px;
    padding: 8px;
    border: 1px solid #cdd6cf;
    border-radius: 9px;
    background: white;
    color: #173633;
  }
  .more-options {
    margin: 12px 0;
  }
  .more-options summary {
    cursor: pointer;
    font-size: 13px;
    color: #526c60;
  }
  .camera-settings {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin-top: 16px;
  }
  .camera-settings label {
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-size: 12px;
  }
  .camera-settings select {
    max-width: 100%;
  }
  .camera-info {
    min-height: 20px;
  }
  .scanner-key {
    margin: 0 0 8px;
    font-size: 12px;
    color: #52675d;
  }
  .scanner-buttons {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 10px;
  }
  .scanner-buttons button {
    padding: 8px 6px;
    height: 76px;
    min-height: 76px;
    min-width: 0;
    width: 100%;
    overflow: hidden;
    font-size: 13px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
  }
  .scanner-label {
    white-space: nowrap;
    flex: 0 0 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
  }
  .scanner-outcome,
  .scanner-time {
    display: block;
    width: 100%;
    height: 14px;
    flex: 0 0 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 11px;
    line-height: 14px;
    font-weight: 500;
    opacity: 0.8;
  }
  .scanner-time {
    font-variant-numeric: tabular-nums;
  }
  .scanner-buttons i {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #b6c3b9;
  }
  .scanner-buttons .chosen {
    background: #193e34;
    color: white;
    border-color: #193e34;
  }
  .scanner-buttons .chosen.has-reads {
    background: #285b43;
    border-color: #77bb8f;
    box-shadow: inset 0 -3px #8ddba7;
  }
  .scanner-buttons .chosen.has-reads[data-found-tier="2"] {
    background: #25635a;
    border-color: #78c4b2;
    box-shadow: inset 0 -3px #98e2cf;
  }
  .scanner-buttons .chosen.has-reads[data-found-tier="3"] {
    background: #295f70;
    border-color: #83bfce;
    box-shadow: inset 0 -3px #a1ddeb;
  }
  .scanner-buttons .has-reads[data-found-tier="2"] .scanner-outcome {
    color: #d0f7ed;
  }
  .scanner-buttons .has-reads[data-found-tier="3"] .scanner-outcome {
    color: #d4f3fa;
  }
  .scanner-buttons .has-reads .scanner-outcome {
    color: #c4f6d3;
    opacity: 1;
  }
  .scanner-buttons .failed .scanner-outcome {
    color: #ffcf9b;
    opacity: 1;
  }
  .chosen i {
    background: var(--scanner-color);
  }
  .hint {
    line-height: 1.6;
    margin: 10px 0 18px;
  }
  .viewer {
    position: relative;
  }
  .viewer-header {
    position: absolute;
    top: max(12px, env(safe-area-inset-top));
    left: max(12px, env(safe-area-inset-left));
    right: max(12px, env(safe-area-inset-right));
    z-index: 3;
    display: flex;
    align-items: flex-start;
    gap: 12px;
    pointer-events: none;
  }
  .runtime-strip {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
    padding: 8px 10px;
    background: #102724cc;
    border-radius: 8px;
    font-size: 11px;
    min-width: 0;
  }
  .runtime-strip > span {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .runtime-strip strong {
    min-width: 9ch;
    text-align: right;
    font: inherit;
    font-variant-numeric: tabular-nums;
  }
  .viewer.expanded {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: #071512;
    width: 100%;
    height: 100dvh;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .expanded .stage {
    margin: 0;
    border-radius: 0;
    flex-shrink: 0;
  }
  .adjust-button {
    pointer-events: auto;
    flex-shrink: 0;
  }
  .exit-viewer {
    flex-shrink: 0;
    pointer-events: auto;
  }
  .viewer-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    margin-top: 10px;
  }
  .scan-overlay .region-label {
    letter-spacing: 0;
    font-family: ui-monospace, monospace;
  }
  .stage {
    -webkit-user-select: none;
    user-select: none;
    -webkit-touch-callout: none;
    position: relative;
    background: #142421;
    border-radius: 16px;
    overflow: hidden;
    width: 100%;
    margin: 14px auto 0;
    isolation: isolate;
  }
  .stage canvas,
  .stage video {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    visibility: hidden;
    pointer-events: none;
    -webkit-user-select: none;
    user-select: none;
    -webkit-touch-callout: none;
  }
  .stage video {
    inset: 0;
    width: 100%;
    height: 100%;
    transform: none;
    object-fit: contain;
  }
  .stage canvas {
    will-change: transform;
  }
  .stage .visible {
    visibility: visible;
  }
  .stage .live-preview,
  .stage .detail-preview {
    left: 0;
    top: 0;
    width: 100%;
    height: 100%;
    transform: none;
    will-change: auto;
  }
  .stage .gesture-surface {
    left: 0;
    top: 0;
    width: 100%;
    height: 100%;
    transform: none;
    visibility: visible;
    pointer-events: auto;
    will-change: auto;
    position: absolute;
    inset: 0;
    touch-action: none;
    cursor: move;
  }
  .edge-adjust {
    position: absolute;
    right: 10px;
    top: 50%;
    transform: translateY(-50%);
    z-index: 4;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .center-marker {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 26px;
    height: 26px;
    transform: translate(-50%, -50%);
    z-index: 2;
    pointer-events: none;
    opacity: 0;
    transition: opacity 220ms ease-out;
    filter: drop-shadow(0 0 1px #071512) drop-shadow(0 1px 1px #071512);
  }
  .center-marker.shown {
    opacity: 0.85;
    transition-duration: 80ms;
  }
  .center-marker::before,
  .center-marker::after {
    content: "";
    position: absolute;
    background: white;
    border-radius: 1px;
  }
  .center-marker::before {
    left: 12px;
    top: 0;
    width: 2px;
    height: 26px;
  }
  .center-marker::after {
    top: 12px;
    left: 0;
    height: 2px;
    width: 26px;
  }
  @media (prefers-reduced-motion: reduce) {
    .center-marker {
      transition: none;
    }
  }
  .gesture-surface:focus-visible {
    outline: 3px solid #edf5df;
    outline-offset: -4px;
  }
  .scan-overlay {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .scan-overlay text {
    font-size: 30px;
    letter-spacing: 4px;
  }
  .welcome {
    position: absolute;
    inset: 20% 12%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    color: #edf5df;
  }
  .welcome h1 {
    font-size: clamp(22px, 4vw, 42px);
    letter-spacing: -1px;
  }
  .welcome p {
    font-size: 14px;
  }
  .scanning {
    position: absolute;
    bottom: 12px;
    left: 16px;
    font-size: 12px;
    color: white;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .controls {
    margin: 14px 0;
  }
  .controls button,
  .upload {
    font-size: 12px;
  }

  .controls [aria-pressed="true"] {
    background: #193e34;
    color: white;
  }
  .upload {
    position: relative;
    padding: 12px 15px;
    border: 1px solid #cdd6cf;
    border-radius: 9px;
    background: white;
    font-weight: 600;
    cursor: pointer;
    overflow: hidden;
  }
  .upload input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
    width: 100%;
  }
  .upload:focus-within {
    outline: 3px solid #348778;
    outline-offset: 3px;
  }
  .image-controls {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 16px;
    background: #e8ede2;
    border-radius: 12px;
    flex-wrap: wrap;
  }
  .image-controls label {
    flex: 1;
    min-width: 140px;
    font-size: 12px;
  }
  .image-controls output {
    float: right;
    font-variant-numeric: tabular-nums;
  }
  .image-controls input {
    display: block;
    margin: 12px 0 0;
  }
  .image-controls button {
    font-size: 12px;
  }
  .runtime-heading {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin: 14px 0 10px;
    align-items: center;
  }
  .runtime-heading span {
    font-size: 12px;
    color: #61776b;
  }
  .runtimes {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
    gap: 10px;
  }
  .runtime {
    min-width: 0;
    min-height: 112px;
    border: 1px solid #d7dfd2;
    border-top: 3px solid var(--scanner-color);
    border-radius: 10px;
    padding: 12px;
    background: white;
    display: flex;
    flex-direction: column;
  }
  .runtime.found {
    background: color-mix(in srgb, var(--scanner-color) 12%, white);
  }
  .result-title {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
  }
  .result-title span {
    font-size: 12px;
    font-weight: 600;
  }
  .result-title strong {
    font-size: 18px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .runtime small {
    font-size: 11px;
    color: #61776b;
    font-weight: 400;
  }
  .barcode-values {
    margin-top: 10px;
    flex: 1;
    font-size: 12px;
    line-height: 1.5;
  }
  .barcode-values code {
    display: block;
    font-size: 14px;
    overflow-wrap: anywhere;
    margin-bottom: 4px;
    user-select: all;
  }
  .runtime .error {
    overflow: auto;
    margin: 4px 0 0;
    padding: 4px;
  }
  footer {
    margin-top: 28px;
    flex-wrap: wrap;
    align-items: center;
  }
  footer nav {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 16px;
  }
  footer a {
    display: inline-flex;
    align-items: center;
    min-height: 44px;
    font-size: 12px;
  }
  @media (max-width: 600px) {
    .heading {
      padding: 20px 16px;
    }
    .private,
    .github-prefix {
      display: none;
    }
    main {
      padding: 8px 12px 20px;
    }
    .scanner-buttons {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 5px;
    }
    .scanner-buttons button {
      padding: 8px 3px;
      font-size: 11px;
      gap: 2px;
      height: 70px;
      min-height: 70px;
    }

    .welcome p {
      font-size: 12px;
      margin: 8px 0;
    }
    .welcome h1 {
      font-size: 22px;
    }
    .examples-group {
      width: 100%;
    }
    .own-group {
      margin-left: 0;
      margin-right: auto;
    }
    .demo-images {
      width: 100%;
      display: grid;
      grid-template-columns: repeat(5, minmax(0, 1fr));
      gap: 4px;
    }
    .demo-images button {
      padding: 8px 2px;
      font-size: 11px;
      min-width: 0;
    }
    .runtime {
      min-height: 112px;
    }
    .runtime-heading {
      align-items: flex-start;
    }
    .runtime-heading span {
      text-align: right;
    }
  }
</style>
