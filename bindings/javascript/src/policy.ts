export const policy = Object.freeze({
  transitionCleanup: true,
  sourceIdentity: true,
  interiorNormalization: true,
  guardBias: true,
  allowSingleRow: false,
  maxRetryPathsPerCandidate: 512,
  maxRetryPathsPerFrame: 8192,
  maxAssociationChecks: 200000,
  maxAssociationPixels: 2000000,
  maxResults: 1024,
});
