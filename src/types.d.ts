declare module 'gltf-validator' {
  export interface ValidationOptions {
    uri?: string;
    format?: 'glb' | 'gltf';
    maxIssues?: number;
    ignoredIssues?: string[];
    externalResourceFunction?: (uri: string) => Promise<Uint8Array>;
  }
  export function validateBytes(data: Uint8Array, options?: ValidationOptions): Promise<unknown>;
  export function version(): string;
}
