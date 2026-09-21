import type { Mode } from "tapirscan";
import type { Result } from "./types";
export type ComparisonSpec = {
  id: string;
  label: string;
  color: string;
  engine: "classical" | "zxing" | "zbar" | "jsqr" | "native" | "quagga" | "zxingjs";
  version: Mode;
};
export type ComparisonEntry = ComparisonSpec & { result?: Result; error?: string };
export const comparisonOptions: ComparisonSpec[] = [
  { id: "zxingjs", label: "ZXing-JS", color: "#80d4f4", engine: "zxingjs", version: "medium" },
  { id: "quagga", label: "Quagga2", color: "#f2a8cd", engine: "quagga", version: "medium" },
  { id: "jsqr", label: "jsQR", color: "#69d6b0", engine: "jsqr", version: "medium" },
  { id: "native", label: "Native", color: "#dfcf78", engine: "native", version: "medium" },
  { id: "zxing", label: "ZXing", color: "#58b9ff", engine: "zxing", version: "medium" },
  { id: "zbar", label: "ZBar", color: "#b7a4ff", engine: "zbar", version: "medium" },
  { id: "nano", label: "TS-Low", color: "#8be56f", engine: "classical", version: "low" },
  { id: "fast", label: "TS-Med", color: "#ff6f61", engine: "classical", version: "medium" },
  { id: "quality", label: "TS-High", color: "#ffbb66", engine: "classical", version: "high" },
  {
    id: "veryhigh",
    label: "TS-VHigh",
    color: "#ef70b5",
    engine: "classical",
    version: "very-high",
  },
];

/** Keep each selected reader's last result for this source, in a fixed display order. */
export function visibleResults<T extends { id: string; contentRevision: number }>(
  entries: T[],
  order: readonly string[],
  selected: readonly string[],
  contentRevision: number,
): T[] {
  return order.flatMap((id) =>
    entries.filter(
      (entry) =>
        entry.id === id && selected.includes(id) && entry.contentRevision === contentRevision,
    ),
  );
}
