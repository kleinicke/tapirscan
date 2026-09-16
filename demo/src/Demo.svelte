<script lang="ts">
  import { capabilities, videoFrame, scanDimensions } from "./lib/camera";
  import { onMount, tick } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { comparisonOptions, type ComparisonSpec, type ComparisonEntry } from "./lib/comparison";
  import type { Result } from "./lib/types";

  const options = [
    ["veryhigh", "Very high"],
    ["quality", "High"],
    ["fast", "Medium"],
    ["nano", "Low"],
    ["zxing", "ZXing"],
    ["zbar", "ZBar"],
  ].map(([id, label]) => ({ ...comparisonOptions.find((s) => s.id === id)!, label }));
  let selected = ["fast", "zxing", "zbar"];
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
  let cameraPaused = false;
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
  $: displayWidth = ((mediaWidth * fit) / W) * 100;
  $: displayHeight = ((mediaHeight * fit) / H) * 100;
  function setFrame(width: number, height: number) {
    mediaWidth = width;
    mediaHeight = height;
    const limit = resolution === "original" ? Infinity : (Number(resolution) * 16) / 9;
    [frameWidth, frameHeight] = scanDimensions(width, height, limit);
  }
  $: chosen = options.filter((s) => selected.includes(s.id));
  $: overlayEntries = entries.filter(
    (entry) =>
      selected.includes(entry.id) &&
      (entry.viewRevision === revision ||
        (!!source && !live && entry.contentRevision === contentRevision)),
  );
  $: overlaysUpdating = overlayEntries.some((entry) => entry.viewRevision !== revision);
  function overlayTransform(entry: ViewEntry, currentAngle: number, currentScale: number) {
    return `translate(${entry.width / 2} ${entry.height / 2}) rotate(${currentAngle - entry.angle}) scale(${currentScale / entry.scale}) translate(${-entry.width / 2} ${-entry.height / 2})`;
  }
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
    context.drawImage(source, -mediaWidth / 2, -mediaHeight / 2);
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
  function reset() {
    scale = 1;
    angle = 0;
    transform();
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
    if (!source || event.button !== 0) return;
    if (!pointers.size) gestureBounds = surface.getBoundingClientRect();
    surface.setPointerCapture(event.pointerId);
    pointers.set(event.pointerId, point(event));
  }
  function pointerMove(event: PointerEvent) {
    const previous = pointers.get(event.pointerId);
    if (!previous || !source) return;
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
      // Polar movement around the fixed view center: radius controls zoom,
      // angle controls rotation. Ignore the tiny center where angle is undefined.
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
    pointers.delete(event.pointerId);
    if (!pointers.size) {
      gestureBounds = null;
      requestScan(220);
    }
  }
  function wheel(event: WheelEvent) {
    if (!source) return;
    event.preventDefault();
    const delta =
      event.deltaY *
      (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? surface.clientHeight : 1);
    if (event.shiftKey) angle = wrapAngle(angle + delta * 0.15);
    else scale = clampZoom(scale * Math.exp(-delta * 0.002));
    transform();
  }
  function keyTransform(event: KeyboardEvent) {
    if (!source || !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
    event.preventDefault();
    if (event.key === "ArrowLeft") angle = wrapAngle(angle - 5);
    if (event.key === "ArrowRight") angle = wrapAngle(angle + 5);
    if (event.key === "ArrowUp") scale = clampZoom(scale * 1.1);
    if (event.key === "ArrowDown") scale = clampZoom(scale / 1.1);
    transform();
  }
  function run(spec: ComparisonSpec, image: ImageData): Promise<Result> {
    let worker = workers.get(spec.id);
    if (!worker) {
      worker =
        spec.engine === "classical"
          ? new Worker(new URL("./lib/scan.worker.ts", import.meta.url), { type: "module" })
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
          engine: spec.engine,
          scannerVersion: spec.version,
          searchFurther: true,
          formats: ["EAN13"],
          engineBaseUrl: new URL(`${import.meta.env.BASE_URL}engines/`, document.baseURI).href,
          width: image.width,
          height: image.height,
          buffer,
        },
        [buffer],
      );
    });
  }
  async function scan() {
    if (disposed || (!source && !live)) return;
    if (busy || !selected.length || (live && document.hidden)) return;
    busy = true;
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
        scale = 1;
        angle = 0;
        setFrame(next.width, next.height);
        captureInfo = `Video snapshot · ${next.width} × ${next.height}`;
        preparePreview(next, next.width, next.height);
        invalidate();
        token = revision;
        contentToken = contentRevision;
        await tick();
      }
      if (source) renderDetailedPreview();
      let image: ImageData;
      if (source) {
        // All selected scanners receive the same captured pixels.
        image = detail
          .getContext("2d", { willReadFrequently: true })!
          .getImageData(margin, margin, frameWidth, frameHeight);
      } else {
        if (input.width !== frameWidth) input.width = frameWidth;
        if (input.height !== frameHeight) input.height = frameHeight;
        ctx.fillStyle = "#142421";
        ctx.fillRect(0, 0, frameWidth, frameHeight);
        ctx.save();
        ctx.translate(frameWidth / 2, frameHeight / 2);
        ctx.scale(fit, fit);
        ctx.drawImage(video, -mediaWidth / 2, -mediaHeight / 2);
        ctx.restore();
        image = ctx.getImageData(0, 0, frameWidth, frameHeight);
      }
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
        try {
          if (!workers.has(spec.id)) {
            status = `Warming up ${spec.label}…`;
            await run(spec, image);
            // A new worker's first scan warms its runtime; display the second scan.
            if (contentToken !== contentRevision || disposed || !selected.includes(spec.id))
              continue;
          }
          batch.push({ ...spec, ...view, result: await run(spec, image) });
        } catch (reason) {
          batch.push({ ...spec, ...view, error: String(reason) });
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
          : entries.some((e) => e.viewRevision === token && selected.includes(e.id) && e.error)
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
      scale = 1;
      angle = 0;
      setFrame(video.videoWidth, video.videoHeight);
      invalidate();
      selectedDemo = "";
      live = true;
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
  async function loadImage(blob: Blob, token: number, initialAngle = 0, initialScale = 1) {
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
    stopCamera();
    source?.close();
    source = next;
    captureInfo = `Photo · ${next.width} × ${next.height}`;
    cameraPaused = false;
    setFrame(next.width, next.height);
    preparePreview(next, next.width, next.height);
    error = "";
    scale = initialScale;
    angle = initialAngle;
    transform();
  }
  async function demo() {
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
      void loadImage(file, ++loadId);
    }
    target.value = "";
  }
  onMount(() => {
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
      observer.disconnect();
      document.removeEventListener("visibilitychange", resumePreview);
      disposed = true;
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

<svelte:head
  ><title>Tapirscan</title><meta
    name="description"
    content="Compare barcode scanners on your camera or an image. Everything runs in your browser."
  /></svelte:head
>
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
    <div class="scanner-buttons" aria-label="Scanners">
      {#each options as option (option.id)}
        {@const active = selected.includes(option.id)}
        <!-- Keep the last completed outcome visible until this scanner finishes again. -->
        {@const entry = entries.find((value) => value.id === option.id)}
        {@const count = entry?.result?.regions.filter((region) => region.text).length ?? 0}
        {@const outcome = !active
          ? "Not selected"
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
          class:failed={active && !!entry?.error}
          style:--scanner-color={option.color}
          on:click={() => toggle(option.id)}
        >
          <span class="scanner-label"><i></i>{option.label}</span>
          <span class="scanner-outcome">{outcome}</span>
          <span class="scanner-time"
            >{active && entry?.result ? `${entry.result.scanMs.toFixed(1)} ms` : "—"}</span
          >
        </button>
      {/each}
    </div>
    <div
      class="stage"
      use:suppressImageSelection
      style:aspect-ratio={`${W} / ${H}`}
      style:max-width={`min(100%, calc(66svh * ${W} / ${H}))`}
    >
      <video
        bind:this={video}
        class:visible={live && !source}
        muted
        playsinline
        aria-label="Live camera"
        on:resize={cameraResize}
        style:width={`${displayWidth}%`}
        style:height={`${displayHeight}%`}
      ></video>
      <canvas
        bind:this={preview}
        class="image-preview"
        class:visible={(!!source && !detailReady) || cameraPaused}
        style:width={`${displayWidth}%`}
        style:height={`${displayHeight}%`}
        style:transform={`translate(-50%, -50%) rotate(${angle}deg) scale(${scale})`}
        aria-label="Image preview"
      ></canvas>
      <canvas
        bind:this={detail}
        class="detail-preview"
        class:visible={!!source && detailReady}
        aria-label="Captured image and surrounding context"
      ></canvas>
      <canvas
        width="1"
        height="1"
        class="gesture-surface"
        style:pointer-events={source && !live ? "auto" : "none"}
        style:touch-action={source ? "none" : "pan-y"}
        bind:this={surface}
        tabindex="0"
        aria-label="Image controls. Drag toward the center to zoom out, away to zoom in, or around it to rotate. Scroll to zoom. Shift-scroll to rotate."
        on:pointerdown={pointerDown}
        on:pointermove={pointerMove}
        on:pointerup={pointerUp}
        on:pointercancel={pointerUp}
        on:lostpointercapture={pointerUp}
        on:wheel|nonpassive={wheel}
        on:keydown={keyTransform}
        on:contextmenu|preventDefault={() => {}}
        on:dragstart|preventDefault={() => {}}
      ></canvas>
      <svg
        class="scan-overlay"
        viewBox={`0 0 ${W} ${H}`}
        aria-label="Analysis frame and barcode results"
      >
        <path
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
        >
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
      </svg>
      {#if !source && !live && !cameraPaused && !entries.length}<div class="welcome">
          <h1>{opening ? "Opening your camera…" : "A clearer view of every barcode."}</h1>
          <p>Try a photo, or scan with your camera.</p>
          <button class="primary" on:click={() => void demo()}>Try demo image</button>
        </div>{/if}
      {#if busy || overlaysUpdating}<span class="scanning"
          >{overlaysUpdating ? "Previous outlines · updating…" : status}</span
        >{/if}
    </div>
    {#if source}
      <p class="gesture-hint">Drag with your mouse to rotate and zoom</p>
    {/if}
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
            >Load image<input type="file" accept="image/*" on:change={upload} /></label
          >
          <button on:click={live ? stopVideo : startCamera} disabled={opening}
            >{opening ? "Opening camera…" : live ? "Pause / freeze" : "Use camera"}</button
          >
        </div>
      </div>
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
          {@const entry = entries.find((e) => e.id === spec.id && e.viewRevision === revision)}
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
                  >{entry?.error
                    ? "Scan failed"
                    : entry?.result
                      ? "No barcode found"
                      : busy
                        ? "Scanning…"
                        : "Ready"}</span
                >{/each}
            </div>
            {#if entry?.error}<p class="error">{entry.error}</p>{/if}
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
        Compare EAN-13 readers on the same pixels. Scanner times exclude initialization, the
        one-time warm-up scan, and transfers.
        {#if totalMs}Preparation {preparationMs.toFixed(1)} ms · Total {totalMs.toFixed(1)} ms.{/if}
      </p>
      <div class="camera-settings">
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
      {#if source && !live}
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
      <span>Local processing. No image uploads.</span>
      <nav aria-label="Project and legal links">
        <a href="https://github.com/kleinicke/tapirscan">GitHub</a>
        <a href="https://f-kleinicke.de/">About</a>
        <a href="https://f-kleinicke.de/impressum">Impressum</a>
        <a href={`${import.meta.env.BASE_URL}THIRD_PARTY_NOTICES.txt`}>Third-party licenses</a>
      </nav>
      <span>Tapirscan · v1.1.0</span>
    </footer>
  </main>
</div>

<style>
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
    margin: 8px 0 0;
    text-align: center;
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
  .scanner-buttons {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: 10px;
  }
  .scanner-buttons button {
    height: 76px;
    min-height: 76px;
    min-width: 0;
    width: 100%;
    overflow: hidden;
    font-size: 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
  }
  .scanner-label {
    white-space: nowrap;
    flex: 0 0 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
  }
  .scanner-outcome {
    display: block;
    width: 100%;
    height: 15px;
    flex: 0 0 15px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 11px;
    line-height: 15px;
    font-weight: 500;
    opacity: 0.8;
  }
  .scanner-time {
    height: 14px;
    flex: 0 0 14px;
    font-size: 10px;
    line-height: 14px;
    font-variant-numeric: tabular-nums;
    opacity: 0.8;
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
  .stage canvas {
    will-change: transform;
  }
  .stage .visible {
    visibility: visible;
  }
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
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 5px;
    }
    .scanner-buttons button {
      padding: 8px 3px;
      font-size: 12px;
      gap: 3px;
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
