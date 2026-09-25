import type { Mode } from "tapirscan";
export interface Region {
  polygon: readonly (readonly [number, number])[];
  text: string;
  score?: number;
  support?: number;
}
export interface Result {
  areaCounts?: { proposed: number; checked: number; withoutRead: number; omitted: number | null };
  searchWindows?: {
    polygon: readonly (readonly number[])[];
    kind: string;
    candidateIndex: number;
  }[];
  regions: Region[];
  proposals: Region[];
  width: number;
  height: number;
  scanMs: number;
  unfinished: boolean;
  scannerVersion?: Mode;
}
