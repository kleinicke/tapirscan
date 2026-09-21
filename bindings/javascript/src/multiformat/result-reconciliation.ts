import { mergeLinearDuplicates } from "../runtime-multiformat/linear-duplicates.js";
import { distinctReads, polygonOverlap } from "../runtime-multiformat/geometry.js";
import type { Image } from "../runtime-host.mjs";
import type { Barcode, EanAddOnSymbol } from "./scanner.js";

export function reconcileResults(
  barcodes: Barcode[],
  regions: Barcode[],
  image: Image,
  reconcileAdditional: boolean,
  eanAddOnSymbol: EanAddOnSymbol,
): void {
  if (reconcileAdditional) {
    propagateSupplements(barcodes);
    if (eanAddOnSymbol === "Require") requireSupplements(barcodes, regions);
    const distinct = distinctReads(barcodes);
    barcodes.splice(0, barcodes.length, ...distinct);
    const remaining = regions.filter(
      (region) =>
        !barcodes.some((barcode) => polygonOverlap(region.polygon, barcode.polygon).a >= 0.65),
    );
    regions.splice(0, regions.length, ...distinctReads(remaining));
  }
  barcodes.splice(0, barcodes.length, ...mergeLinearDuplicates(barcodes, image));
  barcodes.sort((a, b) => b.support - a.support);
  barcodes.forEach((barcode, index) => {
    barcode.rank = index + 1;
  });
}

function propagateSupplements(barcodes: Barcode[]): void {
  for (const read of barcodes.filter((barcode) => barcode.eanAddOn)) {
    for (const base of barcodes) {
      if (
        base.format === read.format &&
        base.text === read.text &&
        !base.eanAddOn &&
        polygonOverlap(base.polygon, read.polygon).smaller >= 0.65
      )
        base.eanAddOn = read.eanAddOn;
    }
  }
}

function requireSupplements(barcodes: Barcode[], regions: Barcode[]): void {
  for (let index = barcodes.length - 1; index >= 0; index--) {
    const barcode = barcodes[index];
    if (["EAN13", "UPCA", "EAN8", "UPCE"].includes(barcode.format) && !barcode.eanAddOn) {
      regions.push({ ...barcode, text: "" });
      barcodes.splice(index, 1);
    }
  }
}
