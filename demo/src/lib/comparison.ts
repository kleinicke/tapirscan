import type { Mode } from "tapirscan";
import type { Result } from "./types";
export type ComparisonSpec = {
  id: string;
  label: string;
  color: string;
  engine:
    | "turbo"
    | "classical"
    | "zxing"
    | "zxingdefault"
    | "zbar"
    | "jsqr"
    | "native"
    | "quagga"
    | "zxingjs"
    | "zxingjsdefault";
  version: Mode;
  /** Undefined: the latest published release. "next": the repository's current build. */
  releaseVersion?: string;
};
export type ComparisonEntry = ComparisonSpec & { result?: Result; error?: string };
/** The release name of the repository's current build, served from engines/next/. */
export const nextRelease = "next";

export const comparisonOptions: ComparisonSpec[] = [
  { id: "turbo", label: "TS-Low", color: "#55ddc5", engine: "classical", version: "low" },
  {
    id: "turbo2",
    label: "TS-Turbo2",
    color: "#55cce0",
    engine: "turbo",
    version: "low",
    releaseVersion: nextRelease,
  },
  {
    id: "turbo4",
    label: "TS-Turbo4",
    color: "#9bb5ff",
    engine: "turbo",
    version: "low",
    releaseVersion: nextRelease,
  },
  {
    id: "turbo8",
    label: "TS-Turbo8",
    color: "#d3a4ff",
    engine: "turbo",
    version: "low",
    releaseVersion: nextRelease,
  },
  {
    id: "turbo16",
    label: "TS-Turbo16",
    color: "#e3b878",
    engine: "turbo",
    version: "low",
    releaseVersion: nextRelease,
  },
  { id: "zxingjs", label: "ZXing-JS", color: "#80d4f4", engine: "zxingjs", version: "medium" },
  {
    id: "zxingjsdefault",
    label: "ZXing-JS default",
    color: "#a9deee",
    engine: "zxingjsdefault",
    version: "medium",
  },
  { id: "quagga", label: "Quagga2", color: "#f2a8cd", engine: "quagga", version: "medium" },
  { id: "jsqr", label: "jsQR", color: "#69d6b0", engine: "jsqr", version: "medium" },
  { id: "native", label: "Native browser", color: "#dfcf78", engine: "native", version: "medium" },
  { id: "zxing", label: "ZXing", color: "#58b9ff", engine: "zxing", version: "medium" },
  {
    id: "zxingdefault",
    label: "ZXing default",
    color: "#84cef5",
    engine: "zxingdefault",
    version: "medium",
  },
  { id: "zbar", label: "ZBar", color: "#b7a4ff", engine: "zbar", version: "medium" },
  { id: "fast", label: "TS-Med", color: "#ff6f61", engine: "classical", version: "medium" },
  { id: "quality", label: "TS-High", color: "#ffbb66", engine: "classical", version: "high" },
  {
    id: "veryhigh",
    label: "TS-VHigh",
    color: "#ef70b5",
    engine: "classical",
    version: "very-high",
  },
  {
    id: "ts-low-next",
    label: "TS-Low-next",
    color: "#b4f29c",
    engine: "classical",
    version: "low",
    releaseVersion: nextRelease,
  },
  {
    id: "ts-med-next",
    label: "TS-Med-next",
    color: "#ff947d",
    engine: "classical",
    version: "medium",
    releaseVersion: nextRelease,
  },
  {
    id: "ts-high-next",
    label: "TS-High-next",
    color: "#ffd08a",
    engine: "classical",
    version: "high",
    releaseVersion: nextRelease,
  },
  {
    id: "ts-vhigh-next",
    label: "TS-VHigh-next",
    color: "#ffa4d1",
    engine: "classical",
    version: "very-high",
    releaseVersion: nextRelease,
  },
];

const effortLevels: { mode: Mode; short: string; label: string; id: string }[] = [
  { mode: "low", short: "low", label: "TS-Low", id: "turbo" },
  { mode: "medium", short: "med", label: "TS-Med", id: "fast" },
  { mode: "high", short: "high", label: "TS-High", id: "quality" },
  { mode: "very-high", short: "vhigh", label: "TS-VHigh", id: "veryhigh" },
];

/** Readers for earlier published versions, reusing each effort level's color. */
export function previousReleaseOptions(versions: readonly string[]): ComparisonSpec[] {
  return versions.flatMap((releaseVersion) =>
    effortLevels.map(({ mode, short, label, id }) => ({
      id: `ts-${short}-${releaseVersion}`,
      label: `${label} ${releaseVersion}`,
      color: comparisonOptions.find((spec) => spec.id === id)!.color,
      engine: "classical" as const,
      version: mode,
      releaseVersion,
    })),
  );
}

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
