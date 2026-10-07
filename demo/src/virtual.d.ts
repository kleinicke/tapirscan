declare module "virtual:tapirscan-releases" {
  /** Released npm versions, oldest first; the last is the main release. */
  export const releaseVersions: string[];
  export const latestRelease: string;
}
declare module "virtual:tapirscan-hosts" {
  /** Minimal shape shared by the library hosts of every released version. */
  export interface Host {
    Scanner: {
      create(options: Record<string, unknown>): Promise<{
        dispose(): void;
        scan?(input: unknown, options: Record<string, unknown>): unknown;
        inspect?(input: unknown, options: Record<string, unknown>): unknown;
      }>;
    };
  }
  /** Each released package's own host, keyed by version. */
  export const hosts: Record<string, Host>;
}
