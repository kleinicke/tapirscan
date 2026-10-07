<script lang="ts">
  import { version } from "../package.json";
  import { onDestroy, tick } from "svelte";
  import { retailFormats } from "tapirscan";
  import {
    BenchmarkRunner,
    methods,
    statistics,
    repeats,
    uniqueCodes,
    snapshot,
    variations,
    renderVariation,
    stressNote,
    benchmarkVersion,
    type Report,
    type Scene,
    type Method,
  } from "./lib/benchmark";
  export let releaseVersion = version;
  export let source: ImageBitmap | null = null;
  export let example = "";
  export let sourceRevision = 0;
  let galleryNav: HTMLDivElement;
  let viewer: HTMLDialogElement;
  let viewerCanvas: HTMLCanvasElement;
  let inspected: Scene | null = null;
  let inspectionError = "";
  let inspectionBusy = false;
  let inspectionToken = 0;
  let inspectScale = "fit";
  let showOverlay = true;
  async function changePage(next: number) {
    page = next;
    await tick();
    galleryNav.scrollIntoView({ block: "start", behavior: "instant" });
  }
  function closeInspection() {
    inspectionToken++;
    inspected = null;
    viewer?.close();
  }
  async function inspect(scene: Scene) {
    if (!report) return;
    const current = ++inspectionToken;
    const runToken = token;
    const savedReport = report;
    const index = savedReport.scenes.indexOf(scene);
    inspected = scene;
    inspectionError = "";
    inspectionBusy = true;
    inspectScale = "fit";
    await tick();
    viewer.showModal();
    try {
      let original: HTMLCanvasElement;
      let bitmap: ImageBitmap | null = null;
      try {
        if (origin === "Precomputed example") {
          const response = await fetch(
            `${import.meta.env.BASE_URL}images/${encodeURIComponent(savedReport.source)}`,
          );
          if (!response.ok) throw new Error("Could not load the example image");
          bitmap = await createImageBitmap(await response.blob());
          original = snapshot(bitmap, savedReport.limit);
        } else if (currentImageRun) {
          if (!source) throw new Error("The original image is no longer available");
          original = snapshot(source, savedReport.limit);
        } else {
          const file = files[savedReport.kind === "batch" ? index : 0];
          if (!file) throw new Error("The original upload is no longer available");
          bitmap = await createImageBitmap(file);
          original = snapshot(bitmap, savedReport.limit);
        }
      } finally {
        bitmap?.close();
      }
      const image =
        savedReport.kind === "stress" && index > 0
          ? renderVariation(original, variations[index - 1], savedReport.scenes[0])
          : original;
      if (current !== inspectionToken || runToken !== token) return;
      if (image.width !== scene.width || image.height !== scene.height)
        throw new Error("The recreated image dimensions do not match the saved scan");
      inspectionBusy = false;
      await tick();
      viewerCanvas.width = image.width;
      viewerCanvas.height = image.height;
      viewerCanvas.getContext("2d")?.drawImage(image, 0, 0);
    } catch (error) {
      if (current === inspectionToken) {
        inspectionBusy = false;
        inspectionError = String(error);
      }
    }
  }
  let currentImageRun = false;
  let open = false;
  let busy = false;
  let message = "";
  let report: Report | null = null;
  let origin = "";
  let files: File[] = [];
  let mode: "batch" | "stress" = "batch";
  let limit = 1920;
  let runner: BenchmarkRunner | null = null;
  let token = 0;
  let method: Method = "medium";
  let filter = "all";
  let tag = "all";
  let page = 0;
  let savedExample = "";
  $: if (sourceRevision >= 0) {
    resetSource(sourceRevision, releaseVersion);
  }
  $: if (open && example && savedExample !== example && !busy) {
    void loadSaved(example);
  }
  $: scenes = report?.scenes ?? [];
  $: tags = [...new Set(scenes.flatMap((scene) => scene.tags))];
  $: filtered = scenes.filter((scene) => {
    const observation = scene.observations[method];
    return (
      (tag === "all" || scene.tags.includes(tag)) &&
      (filter === "all" ||
        (filter === "read" && !!observation?.regions.length) ||
        (filter === "one" && !!observation && uniqueCodes(observation.regions) === 1) ||
        (filter === "multiple" && !!observation && uniqueCodes(observation.regions) >= 2) ||
        (filter === "miss" && !!observation && !observation.error && !observation.regions.length) ||
        (filter === "repeat" && !!observation && repeats(observation.regions) > 0) ||
        (filter === "error" && !!observation?.error))
    );
  });
  $: visible = filtered.slice(page * 12, page * 12 + 12);
  function cancel() {
    if (busy && report) report = { ...report, status: "cancelled" };
    token++;
    runner?.cancel();
    runner = null;
    busy = false;
  }
  function resetSource(_revision: number, _version: string) {
    closeInspection();
    cancel();
    report = null;
    origin = "";
    savedExample = "";
    page = 0;
    message = "";
  }
  onDestroy(cancel);
  async function loadSaved(file: string) {
    closeInspection();
    savedExample = file;
    const current = ++token;
    message = "Loading saved benchmark…";
    try {
      const root = `${import.meta.env.BASE_URL}benchmarks/`;
      const [response, manifestResponse] = await Promise.all([
        fetch(`${root}${file}.json`),
        fetch(`${root}manifest.json`),
      ]);
      if (!response.ok || !manifestResponse.ok)
        throw new Error("No saved benchmark for this example. Run it on this device.");
      const result = (await response.json()) as Report;
      const manifest = (await manifestResponse.json()) as { identity: string };
      if (
        result.engines?.Tapirscan !== releaseVersion ||
        result.version !== benchmarkVersion ||
        result.identity !== manifest.identity
      )
        throw new Error(
          "Saved benchmark does not match this benchmark recipe. Run it on this device.",
        );
      if (current !== token) return;
      page = 0;
      filter = "all";
      tag = "all";
      report = result;
      origin = "Precomputed example";
      message = "";
    } catch (error) {
      if (current === token) {
        report = null;
        message = error instanceof Error ? error.message : String(error);
      }
    }
  }
  function choose(event: Event) {
    closeInspection();
    cancel();
    files = Array.from((event.target as HTMLInputElement).files ?? []);
    report = null;
    origin = "";
    message = "";
    page = 0;
    if (files.length > 1) mode = "batch";
  }
  async function run(useCurrent: boolean) {
    closeInspection();
    currentImageRun = useCurrent;
    cancel();
    const current = token;
    const selected = useCurrent ? [] : [...files];
    const kind = useCurrent ? "stress" : mode;
    if (useCurrent && !source) return;
    if (!useCurrent && (!selected.length || (kind === "stress" && selected.length !== 1))) return;
    const active = new BenchmarkRunner([...retailFormats], releaseVersion);
    runner = active;
    busy = true;
    filter = "all";
    tag = "all";
    page = 0;
    report = {
      version: benchmarkVersion,
      created: new Date().toISOString(),
      browser: navigator.userAgent,
      source: useCurrent
        ? example || "Current image"
        : selected.length === 1
          ? selected[0]!.name
          : `${selected.length} uploaded images`,
      kind,
      status: "running",
      totalInputs: useCurrent ? 1 : selected.length,
      engines: {
        Tapirscan: releaseVersion,
        ZXing: "3.1.1",
        ZBar: "0.11.0",
        "Native browser": "BarcodeDetector (see browser version)",
      },
      formats: [...retailFormats],
      limit,
      scenes: [],
      note: "",
    };
    origin = "Measured on this device";
    const output = report;
    const add = async (image: HTMLCanvasElement, name: string, labels: string[]) => {
      const index = output.scenes.length;
      return active.scene(image, name, labels, (scene) => {
        if (current !== token) return;
        output.scenes[index] = scene;
        report = { ...output, scenes: [...output.scenes] };
      });
    };
    try {
      const count = useCurrent ? 1 : selected.length;
      for (let index = 0; index < count; index++) {
        if (current !== token) break;
        const file = selected[index];
        const name = file?.name ?? (example || "Current image");
        message = `Image ${index + 1} / ${count}: ${name}`;
        let image: HTMLCanvasElement;
        try {
          if (useCurrent) image = snapshot(source!, limit);
          else {
            const bitmap = await createImageBitmap(file!);
            try {
              image = snapshot(bitmap, limit);
            } finally {
              bitmap.close();
            }
          }
        } catch (error) {
          if (current !== token) break;
          output.scenes.push({
            name,
            tags: ["load error"],
            width: 0,
            height: 0,
            thumbnail: "",
            observations: Object.fromEntries(
              methods.map((item) => [
                item.id,
                { regions: [], error: `Image load failed: ${String(error)}` },
              ]),
            ),
          });
          report = { ...output, scenes: [...output.scenes] };
          continue;
        }
        const baseline = await add(image, name, ["original"]);
        if (kind === "stress") {
          output.note = stressNote;
          for (let v = 0; v < variations.length; v++) {
            if (current !== token) break;
            const variation = variations[v]!;
            message = `Variation ${v + 1} / ${variations.length}: ${variation.name}`;
            await add(renderVariation(image, variation, baseline), variation.name, variation.tags);
          }
        }
      }
      if (current === token) {
        output.status = "complete";
        report = { ...output };
        message = `Completed ${output.scenes.length} images.`;
      }
    } catch (error) {
      if (current === token) {
        output.status = "failed";
        report = { ...output };
        message = error instanceof Error ? error.message : String(error);
      }
    } finally {
      active.cancel();
      if (current === token) {
        runner = null;
        busy = false;
      }
    }
  }
  function download() {
    if (!report) return;
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(report, null, 2)], { type: "application/json" }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = "tapirscan-benchmark.json";
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  const ms = (value: number | null | undefined) => (value == null ? "—" : value.toFixed(1));
</script>

<section class="benchmark" aria-label="Image benchmark">
  <button class="toggle" aria-expanded={open} on:click={() => (open = !open)}
    >Image benchmark {open ? "−" : "+"}</button
  >
  {#if open}
    <p>
      Compare Native browser (BarcodeDetector), ZXing, ZBar, Tapirscan Low and Tapirscan Medium.
      Batch statistics for any number of images, or a stress test of one complete image. Images stay
      in your browser. The current-image test starts from the original pixels, independent of the
      scanner preview’s zoom and rotation.
    </p>
    <div class="actions">
      <button disabled={!source || busy} on:click={() => void run(true)}
        >Stress-test current image</button
      >
      {#if example}<button disabled={busy} on:click={() => void loadSaved(example)}
          >Show precomputed example</button
        >{/if}
      <label
        >Upload benchmark images<input
          aria-label="Upload benchmark images"
          type="file"
          accept="image/*"
          multiple
          on:change={choose}
          disabled={busy}
        /></label
      >
    </div>
    {#if files.length}
      <div class="actions">
        <span>{files.length} files selected</span>
        <label
          >Experiment<select aria-label="Benchmark experiment" bind:value={mode} disabled={busy}>
            <option value="batch">Scan every uploaded image</option><option
              value="stress"
              disabled={files.length !== 1}>Generate variations of one image</option
            >
          </select></label
        >
        <button
          disabled={busy || (mode === "stress" && files.length !== 1)}
          on:click={() => void run(false)}>Run uploaded images</button
        >
      </div>
    {/if}
    <div class="actions">
      <label
        >Maximum image side<select
          aria-label="Benchmark image limit"
          bind:value={limit}
          disabled={busy}
          ><option value={1920}>1920 px</option><option value={3840}>3840 px</option><option
            value={100000}>Original resolution</option
          ></select
        ></label
      >
      {#if busy}<button
          on:click={() => {
            cancel();
            message = "Stopped. Partial results retained.";
          }}>Stop benchmark</button
        >{/if}
      {#if report}<button on:click={download}>Download results JSON</button>{/if}
      <span role="status">{message}</span>
    </div>
    <p class="note">
      Retail formats: {retailFormats.join(", ")}. Timing is one warm scan per image, excluding
      engine loading, image preparation and worker transfer. Native browser includes detector
      construction; ZBar uses fresh scan state. Small timing differences are not rankings.
      Unsupported methods are errors, never fallback results.
    </p>
    {#if report}
      <h3>{origin} · {report.source}</h3>
      <p class="note">
        {report.created} · {report.status} · {report.scenes.length} images · maximum side {report.limit ===
        100000
          ? "original"
          : `${report.limit}px`}<br />{report.browser}<br />{report.note}
      </p>
      {#if report.engines}<p class="note">
          {Object.entries(report.engines)
            .map(([name, version]) => `${name}: ${version}`)
            .join(" · ")}
        </p>{/if}
      <p class="note">
        The image columns group successfully processed images by distinct decoded values: zero,
        exactly one, or two and more. Repeated detections of the same value count as one unique code
        in that image. Total codes detected still includes repeated detections. Errors are separate.
        These counts are not verified accuracy. Repeated values may be duplicate detections or
        separate physical copies. Outlines show each method’s returned geometry; ZBar outlines are
        hulls of sample points. Missing outlines do not prove no barcode exists.
      </p>
      <div class="actions">
        <label
          >Method / overlay<select
            aria-label="Benchmark method"
            bind:value={method}
            on:change={() => (page = 0)}
            >{#each methods as item (item.id)}<option value={item.id}>{item.label}</option
              >{/each}</select
          ></label
        >
        <label
          >Filter<select
            aria-label="Benchmark filter"
            bind:value={filter}
            on:change={() => (page = 0)}
            ><option value="all">All images</option><option value="read">Any code detected</option
            ><option value="one">Exactly 1 unique code</option><option value="multiple"
              >2+ unique codes</option
            ><option value="miss">No read</option><option value="repeat"
              >Repeated value (2+ reads)</option
            ><option value="error">Error / unavailable</option></select
          ></label
        >
        <label
          >Variation<select aria-label="Benchmark tag" bind:value={tag} on:change={() => (page = 0)}
            ><option value="all">All variations</option>{#each tags as value (value)}<option {value}
                >{value}</option
              >{/each}</select
          ></label
        >
      </div>
      <p>
        {filtered.length} / {scenes.length} images shown. Statistics below follow these filters.
      </p>
      <div class="table-wrap">
        <table>
          <thead
            ><tr
              ><th>Method</th><th>Images: 0 unique codes</th><th>Images: exactly 1 unique code</th
              ><th>Images: 2+ unique codes</th><th>Total codes detected</th><th
                >Repeated images / extra reads</th
              ><th>Errors</th><th>Mean ms</th><th>Median ms</th><th>P95 ms</th><th>Total ms</th></tr
            ></thead
          ><tbody>
            {#each methods as item (item.id)}{@const stats = statistics(filtered, item.id)}<tr
                ><th style:--method-color={item.color}>{item.label}</th><td>{stats.zeroUnique}</td
                ><td>{stats.oneUnique}</td><td>{stats.multipleUnique}</td><td>{stats.reads}</td><td
                  >{stats.repeated} / {stats.extras}</td
                ><td>{stats.errors}</td><td>{ms(stats.mean)}</td><td>{ms(stats.median)}</td><td
                  >{ms(stats.p95)}</td
                ><td>{ms(stats.total)}</td></tr
              >{/each}
          </tbody>
        </table>
      </div>
      <div class="actions pagination" bind:this={galleryNav}>
        <button disabled={page === 0} on:click={() => void changePage(page - 1)}>Previous</button>
        <span>Page {page + 1} / {Math.max(1, Math.ceil(filtered.length / 12))}</span>
        <button
          disabled={(page + 1) * 12 >= filtered.length}
          on:click={() => void changePage(page + 1)}>Next</button
        >
      </div>
      <div class="gallery">
        {#each visible as scene, sceneIndex (page * 12 + sceneIndex)}
          <article>
            <h4>{scene.name}</h4>
            <p class="note">{scene.tags.join(" · ")} · {scene.width} × {scene.height}</p>
            {#if scene.thumbnail}<div class="image">
                <img
                  src={scene.thumbnail}
                  alt={scene.name}
                  width={scene.width}
                  height={scene.height}
                  loading="lazy"
                /><svg
                  viewBox={`0 0 ${scene.width} ${scene.height}`}
                  aria-label={`${methods.find((item) => item.id === method)?.label} detection locations`}
                >
                  {#each scene.observations[method]?.regions ?? [] as region, index (index)}
                    <polygon
                      points={region.polygon.map((point) => point.join(",")).join(" ")}
                      fill="none"
                      stroke={methods.find((item) => item.id === method)?.color}
                      stroke-width="3"
                      vector-effect="non-scaling-stroke"
                    />
                    {#if region.polygon[0]}<text
                        x={region.polygon[0][0]}
                        y={Math.max(20, region.polygon[0][1])}
                        font-size={Math.max(scene.width, scene.height) / 24}
                        fill="black"
                        stroke="white"
                        stroke-width="2"
                        paint-order="stroke">{index + 1}</text
                      >{/if}
                  {/each}
                </svg>
              </div>
              <button class="inspect-button" on:click={() => void inspect(scene)}
                >Inspect full resolution</button
              >{/if}
            {#each methods as item (item.id)}{@const observation = scene.observations[item.id]}
              <details>
                <summary style:--method-color={item.color}
                  >{item.label}: {observation?.error
                    ? "error"
                    : observation
                      ? `${observation.regions.length} codes · ${ms(observation.scanMs)} ms${repeats(observation.regions) ? " · repeated" : ""}`
                      : "pending"}</summary
                >
                {#if observation?.error}<p>{observation.error}</p>{/if}
                {#each observation?.regions ?? [] as region, index (index)}<p>
                    {index + 1}. <code>{region.text}</code>
                  </p>{/each}
              </details>{/each}
          </article>
        {/each}
      </div>
      <div class="actions">
        <button disabled={page === 0} on:click={() => void changePage(page - 1)}>Previous</button
        ><span>Page {page + 1} / {Math.max(1, Math.ceil(filtered.length / 12))}</span><button
          disabled={(page + 1) * 12 >= filtered.length}
          on:click={() => void changePage(page + 1)}>Next</button
        >
      </div>
    {/if}
  {/if}
</section>

<dialog
  bind:this={viewer}
  on:close={() => {
    inspectionToken++;
    inspected = null;
  }}
  aria-label="Full-resolution benchmark image"
>
  {#if inspected}
    <div class="actions viewer-toolbar">
      <strong>{inspected.name} · {inspected.width} × {inspected.height}</strong>
      <label
        >View<select aria-label="Image zoom" bind:value={inspectScale}
          ><option value="fit">Fit</option><option value="1">100% (1:1)</option><option value="2"
            >200%</option
          ></select
        ></label
      >
      <label
        >Overlay<select aria-label="Inspection method" bind:value={method}
          >{#each methods as item (item.id)}<option value={item.id}>{item.label}</option
            >{/each}</select
        ></label
      >
      <label><input type="checkbox" bind:checked={showOverlay} /> Show detections</label>
      <button on:click={closeInspection}>Close image</button>
    </div>
    <p class="note">
      Recreated from the original image at the scan resolution, without thumbnail compression. A
      returned value is a detection, not independently verified correctness.
    </p>
    {#if inspectionBusy}<p role="status">
        Preparing full-resolution image…
      </p>{:else if inspectionError}<p role="alert">{inspectionError}</p>{:else}
      <div class="viewer-scroll">
        <div
          class="viewer-image"
          style:width={inspectScale === "fit"
            ? `min(100%, ${inspected.width}px, ${(65 * inspected.width) / inspected.height}vh)`
            : `${inspected.width * Number(inspectScale)}px`}
        >
          <canvas bind:this={viewerCanvas} aria-label="Full-resolution scan pixels"></canvas>
          {#if showOverlay}<svg
              viewBox={`0 0 ${inspected.width} ${inspected.height}`}
              aria-label="Full-resolution detections"
            >
              {#each inspected.observations[method]?.regions ?? [] as region, index (index)}
                <polygon
                  points={region.polygon.map((point) => point.join(",")).join(" ")}
                  fill="none"
                  stroke={methods.find((item) => item.id === method)?.color}
                  stroke-width="2"
                  vector-effect="non-scaling-stroke"
                />
                {#if region.polygon[0]}<text
                    x={region.polygon[0][0]}
                    y={Math.max(20, region.polygon[0][1])}
                    font-size="24"
                    fill="black"
                    stroke="white"
                    stroke-width="2"
                    paint-order="stroke">{index + 1}</text
                  >{/if}
              {/each}
            </svg>{/if}
        </div>
      </div>
      <p>
        {#each inspected.observations[method]?.regions ?? [] as region, index (index)}<span
            >{index + 1}. <code>{region.text}</code>
          </span>{/each}
      </p>
    {/if}
  {/if}
</dialog>

<style>
  .benchmark {
    overflow-anchor: none;
    margin: 1rem 0;
    border: 1px solid #444;
    border-radius: 12px;
    padding: 1rem;
  }
  .toggle {
    width: 100%;
    text-align: left;
    font-size: 1.1rem;
    font-weight: 650;
  }
  p {
    line-height: 1.5;
  }
  .note {
    font-size: 0.8rem;
    color: #586b65;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.7rem;
    margin: 0.8rem 0;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.8rem;
  }
  input {
    max-width: 260px;
  }
  .table-wrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8rem;
  }
  th,
  td {
    text-align: left;
    padding: 0.6rem;
    border-bottom: 1px solid #444;
    white-space: nowrap;
  }
  .pagination {
    scroll-margin-top: 16px;
  }
  .inspect-button {
    margin: 0.5rem 0;
    width: 100%;
  }
  dialog {
    width: min(96vw, 1500px);
    max-height: 94vh;
    border: 1px solid #cdd6cf;
    border-radius: 12px;
    background: #f2f3ec;
    color: #173633;
  }
  dialog::backdrop {
    background: #0009;
  }
  .viewer-toolbar {
    position: sticky;
    top: 0;
    background: #f2f3ec;
    z-index: 2;
  }
  .viewer-scroll {
    overflow: auto;
    max-height: 65vh;
    background: #dce1da;
  }
  .viewer-image {
    position: relative;
    margin: 0 auto;
  }
  .viewer-image canvas {
    display: block;
    width: 100%;
    height: auto;
  }
  .gallery {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 290px), 1fr));
    gap: 1rem;
    margin: 1rem 0;
  }
  article {
    border: 1px solid #444;
    border-radius: 8px;
    padding: 0.8rem;
    min-width: 0;
  }
  h4 {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .image {
    aspect-ratio: 1;
    position: relative;
    background: white;
  }
  img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  th[style],
  summary {
    border-left: 4px solid var(--method-color);
    padding-left: 0.6rem;
  }
  summary {
    cursor: pointer;
    padding: 0.4rem 0;
    font-size: 0.8rem;
  }
  details p {
    overflow-wrap: anywhere;
  }
</style>
