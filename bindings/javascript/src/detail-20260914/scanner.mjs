import { IndependentScanner } from "./host.mjs";
import { evidencePlan } from "./source-evidence.mjs";
import { recoverDirectSeed } from "./direct-recovery.mjs";

/** Workbench composition. Each crop keeps its own candidate-index namespace. */
export class DetailScanner {
  constructor(primary, recovery, directions) {
    this.primary = primary;
    this.recovery = recovery;
    this.directions = directions;
  }

  static async create(primaryBytes, recoveryBytes, directions) {
    if (![1, 2, 3].includes(directions)) throw new Error("Invalid recovery direction budget");
    const primary = await IndependentScanner.create(primaryBytes);
    try {
      return new DetailScanner(primary, await IndependentScanner.create(recoveryBytes), directions);
    } catch (error) {
      primary.dispose();
      throw error;
    }
  }

  scanLocalized(image, policy, fitLimit, fullFrame) {
    // The source samplers use tightly packed RGBA, as supplied by the camera worker.
    if (image.channels !== 4 || image.stride !== image.width * 4)
      throw new Error("Detail discovery requires tightly packed RGBA");
    const start = performance.now();
    const found = this.primary.scanLocalized(image, policy, fitLimit, fullFrame, (localization) =>
      evidencePlan(image, localization, 48),
    );
    const recovery = recoverDirectSeed(
      image,
      this.recovery,
      policy,
      found,
      64,
      3,
      true,
      true,
      1,
      this.directions,
    );
    return {
      ...found,
      // Do not concatenate crop-local candidate indices into the primary frame.
      // Consumers use detailRegions; raw frames and coordinate transforms stay inspectable.
      recovery,
      detailRegions: detailRegions(found, recovery),
      scanMs: performance.now() - start,
    };
  }

  dispose() {
    this.primary.dispose();
    this.recovery.dispose();
  }
}

/** Preserve undecoded geometry without assigning crop indices to primary proposals. */
export function detailRegions(found, recovery) {
  const decoded = new Set(found.scan.barcodes.flatMap((barcode) => barcode.candidate_indices));
  const regions = recovery.barcodes.map((barcode) => ({
    text: barcode.text,
    polygon: barcode.polygon,
    support: barcode.support,
  }));
  found.localization.proposals.forEach((proposal, index) => {
    if (!decoded.has(index))
      regions.push({ text: "", polygon: proposal.polygon, score: proposal.score });
  });
  for (const attempt of recovery.attempts) {
    const accepted = new Set(attempt.reads.flatMap((barcode) => barcode.candidate_indices));
    attempt.proposals.forEach((proposal, index) => {
      if (!accepted.has(index))
        regions.push({ text: "", polygon: proposal.polygon, score: proposal.score });
    });
  }
  return regions;
}
