/** Immutable research configuration tags; changing bytes requires a new tag. */
export type SharedCorePolicy = {
  transitionCleanup: boolean;
  sourceIdentity: boolean;
  interiorNormalization: boolean;
  guardBias: boolean;
  allowSingleRow: boolean;
  maxRetryPathsPerCandidate: number;
  maxRetryPathsPerFrame: number;
  maxAssociationChecks: number;
  maxAssociationPixels: number;
  maxResults: number;
};
export type SharedCoreSettings = {
  /** A frozen host module. New hosts must keep at least the 64-proposal ABI. */
  host: "extrema-frozen-20260911" | "shared-core-64-20260911";
  decoder: string;
  localizer: string;
  policy: SharedCorePolicy;
  localization: {
    fitLimit: number;
    fullFrame: boolean;
    maxProposals: number;
    maxLocalizedProposals: number;
  };
};
const extremaShared = {
  host: "extrema-frozen-20260911",
  decoder: "extrema-20260911.wasm",
  localizer: "extrema-20260911.wasm",
  policy: {
    transitionCleanup: true,
    sourceIdentity: true,
    interiorNormalization: true,
    guardBias: true,
    maxRetryPathsPerCandidate: 512,
    maxRetryPathsPerFrame: 8192,
    maxAssociationChecks: 200000,
    maxAssociationPixels: 2000000,
    maxResults: 1024,
    allowSingleRow: false,
  },
  localization: { fitLimit: 8, fullFrame: true, maxProposals: 64, maxLocalizedProposals: 32 },
} as const satisfies SharedCoreSettings;
const balancedShared = (mode: "fast" | "quality" | "quality-guarded" | "fast-inline") => ({
  ...extremaShared,
  decoder: `${mode}-20260911.wasm`,
  localizer: `${mode}-20260911.wasm`,
});
export const defaultVersion = "very-fast-v2-lint-20260913" as const;
export const versions = {
  "nano-20260912": {
    label: "Historical Low effort · before lint cleanup",
    decoder: "nano-20260912.wasm",
    localizer: "nano-20260912.wasm",
    oriented: true,
    sharedCore: {
      ...extremaShared,
      decoder: "nano-20260912.wasm",
      localizer: "nano-20260912.wasm",
      localization: { ...extremaShared.localization, fitLimit: 0 },
    },
  },
  "nano-lint-20260913": {
    label: "Low effort",
    decoder: "nano-lint-20260913.wasm",
    localizer: "nano-lint-20260913.wasm",
    oriented: true,
    sharedCore: {
      ...extremaShared,
      decoder: "nano-lint-20260913.wasm",
      localizer: "nano-lint-20260913.wasm",
      localization: { ...extremaShared.localization, fitLimit: 0 },
    },
  },
  "very-fast-v3-20260912": {
    label: "Historical Very Fast · fewer refinements",
    decoder: "very-fast-v3-20260912.wasm",
    localizer: "very-fast-v3-20260912.wasm",
    oriented: true,
    sharedCore: {
      ...extremaShared,
      decoder: "very-fast-v3-20260912.wasm",
      localizer: "very-fast-v3-20260912.wasm",
      localization: { ...extremaShared.localization, fitLimit: 2 },
    },
  },
  "very-fast-v2-20260912": {
    label: "Historical Medium effort · before lint cleanup",
    decoder: "very-fast-v2-20260912.wasm",
    localizer: "very-fast-v2-20260912.wasm",
    oriented: true,
    sharedCore: {
      ...extremaShared,
      decoder: "very-fast-v2-20260912.wasm",
      localizer: "very-fast-v2-20260912.wasm",
    },
  },
  "very-fast-v2-lint-20260913": {
    label: "Medium effort · default",
    decoder: "very-fast-v2-lint-20260913.wasm",
    localizer: "very-fast-v2-lint-20260913.wasm",
    oriented: true,
    sharedCore: {
      ...extremaShared,
      decoder: "very-fast-v2-lint-20260913.wasm",
      localizer: "very-fast-v2-lint-20260913.wasm",
    },
  },
  "very-fast-20260912": {
    label: "Historical Fast · previous generation",
    decoder: "very-fast-20260912.wasm",
    localizer: "very-fast-20260912.wasm",
    oriented: true,
    sharedCore: {
      ...extremaShared,
      decoder: "very-fast-20260912.wasm",
      localizer: "very-fast-20260912.wasm",
    },
  },
  "fast-inline-20260911": {
    label: "Historical Fast · previous snapshot",
    decoder: "fast-inline-20260911.wasm",
    localizer: "fast-inline-20260911.wasm",
    oriented: true,
    sharedCore: balancedShared("fast-inline"),
  },
  "fast-20260911": {
    label: "Historical Fast · earlier snapshot",
    decoder: "fast-20260911.wasm",
    localizer: "fast-20260911.wasm",
    oriented: true,
    sharedCore: balancedShared("fast"),
  },
  "quality-guarded-20260911": {
    label: "Historical High effort · before lint cleanup",
    decoder: "quality-guarded-20260911.wasm",
    localizer: "quality-guarded-20260911.wasm",
    oriented: true,
    sharedCore: balancedShared("quality-guarded"),
  },
  "quality-guarded-lint-20260913": {
    label: "High effort",
    decoder: "quality-guarded-lint-20260913.wasm",
    localizer: "quality-guarded-lint-20260913.wasm",
    oriented: true,
    sharedCore: {
      ...extremaShared,
      decoder: "quality-guarded-lint-20260913.wasm",
      localizer: "quality-guarded-lint-20260913.wasm",
    },
  },
  "very-high-20260912": {
    label: "Historical Very high effort · before lint cleanup",
    decoder: "very-high-20260912.wasm",
    localizer: "very-high-20260912.wasm",
    oriented: true,
    sharedCore: {
      ...extremaShared,
      host: "shared-core-64-20260911",
      decoder: "very-high-20260912.wasm",
      localizer: "very-high-20260912.wasm",
      localization: { ...extremaShared.localization, maxLocalizedProposals: 63 },
    },
  },
  "very-high-lint-20260913": {
    label: "Very high effort",
    decoder: "very-high-lint-20260913.wasm",
    localizer: "very-high-lint-20260913.wasm",
    oriented: true,
    sharedCore: {
      ...extremaShared,
      host: "shared-core-64-20260911",
      decoder: "very-high-lint-20260913.wasm",
      localizer: "very-high-lint-20260913.wasm",
      localization: { ...extremaShared.localization, maxLocalizedProposals: 63 },
    },
  },
  "quality-20260911": {
    label: "Historical Quality · earlier snapshot",
    decoder: "quality-20260911.wasm",
    localizer: "quality-20260911.wasm",
    oriented: true,
    sharedCore: balancedShared("quality"),
  },
  "legacy-027-017": {
    label: "Legacy · previously deployed",
    decoder: "decoder.wasm",
    localizer: "localizer.wasm",
    oriented: false,
  },
  "general-031-058": {
    label: "General · historical",
    decoder: "decoder-058.wasm",
    localizer: "localizer-031.wasm",
    oriented: false,
  },
  "extrema-20260911": {
    label: "Extrema · historical snapshot",
    decoder: "extrema-20260911.wasm",
    localizer: "extrema-20260911.wasm",
    oriented: true,
    sharedCore: extremaShared,
  },
  "rotation-069-036": {
    label: "Rotation · experimental",
    decoder: "decoder-036.wasm",
    localizer: "localizer-069.wasm",
    oriented: true,
  },
} as const;
export type ScannerVersion = keyof typeof versions;
export function scannerVersion(value: unknown): ScannerVersion {
  return typeof value === "string" && Object.hasOwn(versions, value)
    ? (value as ScannerVersion)
    : defaultVersion;
}
export function sharedCoreSettings(version: ScannerVersion): SharedCoreSettings | undefined {
  const value = versions[version];
  return "sharedCore" in value ? value.sharedCore : undefined;
}
/** Reject incomplete metadata before a future shared-core tag reaches the worker. */
export function assertSharedCoreSettings(settings: SharedCoreSettings): void {
  if (settings.localization.maxProposals < 64)
    throw Error("Shared-core host must support at least 64 proposals");
  if (
    !Number.isSafeInteger(settings.localization.fitLimit) ||
    settings.localization.fitLimit < 0 ||
    settings.localization.fitLimit > 8
  )
    throw Error("Invalid shared-core fit limit");
  if (
    !Number.isSafeInteger(settings.localization.maxLocalizedProposals) ||
    settings.localization.maxLocalizedProposals < 0 ||
    settings.localization.maxLocalizedProposals > settings.localization.maxProposals
  )
    throw Error("Invalid shared-core localized proposal bound");
  if (
    settings.localization.fullFrame &&
    settings.localization.maxLocalizedProposals >= settings.localization.maxProposals
  )
    throw Error("Shared-core full-frame search needs one candidate slot");
  for (const key of [
    "transitionCleanup",
    "sourceIdentity",
    "interiorNormalization",
    "guardBias",
    "allowSingleRow",
  ] as const)
    if (typeof settings.policy[key] !== "boolean") throw Error(`Invalid shared-core ${key}`);
  for (const [key, min, max] of [
    ["maxRetryPathsPerCandidate", 0, 4096],
    ["maxRetryPathsPerFrame", 0, 65536],
    ["maxAssociationChecks", 0, 2000000],
    ["maxAssociationPixels", 0, 16000000],
    ["maxResults", 1, 4096],
  ] as const) {
    const value = settings.policy[key];
    if (!Number.isSafeInteger(value) || value < min || value > max)
      throw Error(`Invalid shared-core ${key}`);
  }
}
