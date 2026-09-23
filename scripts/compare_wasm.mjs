// Pixel-stream worker for compare_scanners.py. Compilation and input decoding are outside timing.
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createInterface } from "node:readline";
import { performance } from "node:perf_hooks";
import { isDeepStrictEqual } from "node:util";

const config = JSON.parse(readFileSync(process.argv[2], "utf8"));
const api = await import(
  pathToFileURL(path.join(config.candidate, "bindings/javascript/dist/index.js"))
);
const timingKeys = new Set([
  "elapsedMs",
  "extraMs",
  "elapsed_ms",
  "sampling_ms",
  "interpretation_ms",
  "support_ms",
]);
function clean(value) {
  if (Array.isArray(value)) return value.map(clean);
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value)
        .filter(([key]) => !timingKeys.has(key))
        .map(([key, item]) => [key, clean(item)]),
    );
  return value;
}
function variants(image) {
  const selections = image.formats ? [image.formats] : config.formats;
  return selections.flatMap((selection) =>
    config.budgets.map((budget) => ({
      label: JSON.stringify(selection),
      formats: selection === "retail" ? api.retailFormats : selection,
      extendedBudget: budget === "extended",
    })),
  );
}
const browser =
  config.backend === "browser"
    ? await (await import("./compare_browser.mjs")).browserComparison(config)
    : null;
const scanners = new Map();
async function pair(mode, addon) {
  const key = `${mode}/${addon}`;
  if (!scanners.has(key)) {
    if (browser) {
      const instances = await browser.pair(mode, addon);
      scanners.set(key, instances);
      return instances;
    }
    const instances = [];
    for (const label of ["baseline", "candidate"]) {
      const manifest = JSON.parse(readFileSync(config[`${label}Wasm`], "utf8"));
      const entry = manifest.modes.find((entry) => entry.mode === mode);
      const bytes = readFileSync(path.join(config[`${label}Assets`], entry.file));
      const host = await import(
        pathToFileURL(path.join(config[label], "bindings/javascript/dist/index.js"))
      );
      instances.push(
        await host.Scanner.create({
          mode,
          formats: [...host.linearFormats, ...host.matrixFormats],
          eanAddOnPolicy: addon,
          loadWasm: async () => bytes,
        }),
      );
    }
    scanners.set(key, instances);
  }
  return scanners.get(key);
}
async function scan(scanner, image, variant, debug) {
  if (scanner.scanTimed) {
    const [elapsed, result] = await scanner.scanTimed(image.image, {
      formats: variant.formats,
      extendedBudget: variant.extendedBudget,
      debug,
    });
    return [elapsed, clean(result)];
  }
  const start = performance.now();
  const result = scanner.scan(image.image, {
    formats: variant.formats,
    extendedBudget: variant.extendedBudget,
    debug,
  });
  const elapsed = performance.now() - start;
  return [elapsed, clean(result)];
}
const differences = path.join(config.output, "differences.jsonl");
writeFileSync(differences, "");
let differing = 0,
  comparisons = 0,
  timedComparisons = 0,
  count = 0;
function compare(before, after, context) {
  if (!isDeepStrictEqual(before, after)) {
    differing++;
    if (differing <= config.maxDifferences)
      appendFileSync(
        differences,
        JSON.stringify({ ...context, baseline: before, candidate: after }) + "\n",
      );
  }
}
const retained = [],
  samples = [];
const timingIndices = new Set(config.timingIndices);
try {
  for await (const line of createInterface({ input: process.stdin })) {
    const image = JSON.parse(line);
    image.image = {
      data: new Uint8Array(Buffer.from(image.data, "base64")),
      width: image.width,
      height: image.height,
      channels: image.channels,
      stride: image.stride,
    };
    delete image.data;
    for (const mode of config.modes) {
      const [before, after] = await pair(mode, image.addon);
      for (const variant of variants(image)) {
        const a = (await scan(before, image, variant, config.diagnostics))[1];
        const b = (await scan(after, image, variant, config.diagnostics))[1];
        if (config.saveResults)
          appendFileSync(
            path.join(config.output, "results.jsonl"),
            JSON.stringify({
              index: image.index,
              case: image.name,
              mode,
              selection: variant.label,
              extendedBudget: variant.extendedBudget,
              baseline: a,
              candidate: b,
            }) + "\n",
          );
        compare(a, b, {
          case: image.name,
          mode,
          selection: variant.label,
          extendedBudget: variant.extendedBudget,
          phase: "parity",
        });
        comparisons++;
      }
    }
    if (timingIndices.has(image.index)) retained.push(image);
    if (++count % 50 === 0) console.error(`${count} images compared`);
  }
  // The complete pixel stream is consumed before measurements, avoiding feeder contention.
  for (const mode of config.modes) {
    console.error(
      `Starting ${mode} timing (${retained.length} images, ${config.repeats} repetitions)`,
    );
    for (const image of retained.slice(0, config.warmup))
      for (const variant of variants(image))
        for (const scanner of await pair(mode, image.addon))
          await scan(scanner, image, variant, false);
    for (let repetition = 0; repetition < config.repeats; repetition++) {
      for (const [index, image] of retained.entries()) {
        const [before, after] = await pair(mode, image.addon);
        for (const variant of variants(image)) {
          const order = [
            ["baseline", before],
            ["candidate", after],
          ];
          if ((index + repetition) % 2) order.reverse();
          const measured = {};
          for (const [label, scanner] of order)
            measured[label] = await scan(scanner, image, variant, false);
          const group = `${mode}/${variant.label}/${variant.extendedBudget ? "extended" : "default"}/${image.addon}`;
          compare(measured.baseline[1], measured.candidate[1], {
            case: image.name,
            group,
            phase: "timing",
            repetition,
          });
          timedComparisons++;
          samples.push({
            case: image.name,
            group,
            repetition,
            baseline: measured.baseline[0],
            candidate: measured.candidate[0],
          });
        }
      }
      console.error(`Finished ${mode} timing repetition ${repetition + 1}/${config.repeats}`);
    }
  }
} finally {
  for (const instances of scanners.values()) for (const scanner of instances) scanner.dispose();
  await browser?.close();
}
writeFileSync(
  path.join(config.output, "timings.jsonl"),
  samples.map((row) => JSON.stringify(row) + "\n").join(""),
);
writeFileSync(
  path.join(config.output, "wasm-result.json"),
  JSON.stringify({
    comparisons,
    timedComparisons,
    differences: differing,
    runtime: browser?.runtime ?? { node: process.version, v8: process.versions.v8 },
  }),
);
