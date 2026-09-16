import type { Mode } from "tapirscan";
import type { Result } from "./types";
export type ComparisonSpec = {
  id: string;
  label: string;
  color: string;
  engine: "classical" | "zxing" | "zbar";
  version: Mode;
};
export type ComparisonEntry = ComparisonSpec & { result?: Result; error?: string };
export const comparisonOptions: ComparisonSpec[] = [
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
