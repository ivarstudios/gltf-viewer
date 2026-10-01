// Runs the Khronos glTF-Validator off the UI thread. It is CPU-bound, synchronous Dart code
// behind an async wrapper; on the page it froze input and rendering for seconds on big files.
import { validateBytes } from 'gltf-validator';

export interface ValidateRequest {
  id: number;
  buffer: ArrayBuffer;
  /** Absolute URL of the model; external resources resolve against it. */
  url: string;
  /** File name, shown in the report. */
  uri: string;
}

export interface ValidateResponse {
  id: number;
  report?: unknown;
  error?: string;
}

self.addEventListener('message', async (e: MessageEvent<ValidateRequest>) => {
  const { id, buffer, url, uri } = e.data;
  let response: ValidateResponse;
  try {
    const report = await validateBytes(new Uint8Array(buffer), {
      uri,
      maxIssues: 500,
      externalResourceFunction: async (ref: string) => {
        const res = await fetch(new URL(ref, new URL(url, self.location.href)));
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return new Uint8Array(await res.arrayBuffer());
      },
    });
    response = { id, report };
  } catch (err) {
    response = { id, error: String(err) };
  }
  self.postMessage(response);
});
