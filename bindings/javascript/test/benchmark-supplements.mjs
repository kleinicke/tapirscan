// Warm-process creation and paired scan timing, matching benchmark_supplements.py.
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { platform, arch, cpus } from "node:os";
import assert from "node:assert/strict";
import { Scanner } from "../dist/index.js";
const manifest = process.argv[2];
const count = Number(process.argv[3] ?? 25);
assert.ok(Number.isInteger(count) && count > 0);
const fixtures = JSON.parse(await readFile(manifest, "utf8"));
const policies = ["Ignore", "Read", "Require"];
const scenes = ["EAN13-none", "EAN13-12", "EAN13-51234", "different-supplements"];
function summary(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return {
    medianMs: sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2,
    minMs: sorted[0],
    maxMs: sorted.at(-1),
  };
}
async function measure(mode, scene) {
  const cases = Object.fromEntries(
    policies.map((p) => [p, fixtures.find((f) => f.name === `${scene}-${p}`)]),
  );
  const f = cases.Ignore;
  const image = {
    data: new Uint8Array(await readFile(join(dirname(manifest), f.file))),
    width: f.width,
    height: f.height,
    channels: 1,
  };
  const creation = Object.fromEntries(policies.map((p) => [p, []]));
  const scans = Object.fromEntries(policies.map((p) => [p, []]));
  const order = (i) => [...policies.slice(i % 3), ...policies.slice(0, i % 3)];
  for (let i = 0; i < count + 5; i++) {
    for (const policy of order(i)) {
      const start = performance.now();
      const scanner = await Scanner.create({ mode, eanAddOnPolicy: policy });
      const elapsed = performance.now() - start;
      scanner.dispose();
      if (i >= 5) creation[policy].push(elapsed);
    }
  }
  const scanners = {};
  try {
    for (const policy of policies)
      scanners[policy] = await Scanner.create({ mode, eanAddOnPolicy: policy });
    for (let i = 0; i < count + 5; i++) {
      for (const policy of order(i)) {
        const start = performance.now();
        const result = scanners[policy].scan(image);
        const elapsed = performance.now() - start;
        assert.deepEqual(
          result.barcodes.map((b) => [b.text, b.eanAddOn ?? ""]).sort(),
          cases[policy].expected.map((b) => [b.text, b.eanAddOn ?? ""]).sort(),
        );
        if (i >= 5) scans[policy].push(elapsed);
      }
    }
  } finally {
    for (const scanner of Object.values(scanners)) scanner.dispose();
  }
  return policies.map((policy) => ({
    mode,
    scene,
    policy,
    width: f.width,
    height: f.height,
    create: summary(creation[policy]),
    scan: summary(scans[policy]),
    createSamplesMs: creation[policy],
    scanSamplesMs: scans[policy],
  }));
}
const results = [];
for (const mode of ["low", "medium", "high", "very-high"])
  for (const scene of scenes) results.push(...(await measure(mode, scene)));
console.log(
  JSON.stringify({
    runtime: process.version,
    platform: `${platform()} ${arch()}`,
    cpu: cpus()[0]?.model,
    warmup: 5,
    samples: count,
    results,
  }),
);
