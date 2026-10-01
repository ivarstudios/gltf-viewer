import type { SceneStats } from '../viewer';
import type { ValidateRequest, ValidateResponse } from '../validator.worker';

interface ValidatorMessage {
  code: string;
  message: string;
  severity: number;
  pointer?: string;
  offset?: number;
}

interface ValidatorResource {
  pointer: string;
  mimeType?: string;
  storage: string;
  uri?: string;
  byteLength?: number;
  image?: { width: number; height: number; format?: string };
}

interface ValidatorReport {
  issues: {
    numErrors: number;
    numWarnings: number;
    numInfos: number;
    numHints: number;
    messages: ValidatorMessage[];
    truncated: boolean;
  };
  info?: {
    version?: string;
    generator?: string;
    extensionsUsed?: string[];
    extensionsRequired?: string[];
    resources?: ValidatorResource[];
    drawCallCount?: number;
    totalVertexCount?: number;
    totalTriangleCount?: number;
    hasSkins?: boolean;
    hasMorphTargets?: boolean;
  };
}

export interface ModelInfo {
  fileName: string;
  byteLength: number;
  format: 'GLB' | 'glTF';
  stats: SceneStats;
}

const SEVERITY = ['Error', 'Warning', 'Info', 'Hint'];

/** Side panel with file info, scene stats and the Khronos glTF-Validator report. */
export class InfoPanel {
  private panel = document.getElementById('info-panel')!;
  private badge = document.getElementById('issues-badge')!;
  private model: ModelInfo | null = null;
  private report: ValidatorReport | null = null;
  private validationError: string | null = null;
  private validationSkipped: string | null = null;
  private validating = false;
  private validator = new ValidatorClient();

  setModel(model: ModelInfo | null) {
    this.model = model;
    this.report = null;
    this.validationError = null;
    this.validationSkipped = null;
    this.badge.className = 'badge hidden';
    this.render();
  }

  /** Runs the validator; `isCurrent` guards against a newer file having been opened meanwhile. */
  async validate(buffer: ArrayBuffer, url: string, isCurrent: () => boolean) {
    this.validating = true;
    this.render();
    try {
      const report = (await this.validator.validate(buffer, url, this.model?.fileName ?? 'model')) as ValidatorReport;
      if (!isCurrent()) return;
      this.report = report;
    } catch (err) {
      if (!isCurrent()) return;
      this.validationError = String(err);
    } finally {
      if (isCurrent()) {
        this.validating = false;
        this.updateBadge();
        this.render();
      }
    }
  }

  /** Records why validation did not run (e.g. file too large) and shows it in the panel. */
  skipValidation(reason: string) {
    this.validating = false;
    this.report = null;
    this.validationError = null;
    this.validationSkipped = reason;
    this.badge.className = 'badge hidden';
    this.render();
  }

  toggle(force?: boolean) {
    return !this.panel.classList.toggle('hidden', force === undefined ? undefined : !force);
  }

  private updateBadge() {
    const issues = this.report?.issues;
    if (!issues) {
      this.badge.className = 'badge hidden';
      return;
    }
    if (issues.numErrors) {
      this.badge.className = 'badge error';
      this.badge.textContent = `${issues.numErrors} error${issues.numErrors === 1 ? '' : 's'}`;
    } else if (issues.numWarnings) {
      this.badge.className = 'badge warning';
      this.badge.textContent = `${issues.numWarnings} warning${issues.numWarnings === 1 ? '' : 's'}`;
    } else {
      this.badge.className = 'badge ok';
      this.badge.textContent = 'Valid';
    }
  }

  private render() {
    const m = this.model;
    if (!m) {
      this.panel.innerHTML = '<p class="muted">No model loaded.</p>';
      return;
    }
    const info = this.report?.info;
    const s = m.stats;
    const rows: [string, string][] = [
      ['File', m.fileName],
      ['Size', formatBytes(m.byteLength)],
      ['Format', m.format],
    ];
    if (info?.version) rows.push(['glTF version', info.version]);
    if (info?.generator) rows.push(['Generator', info.generator]);

    const statRows: [string, string][] = [
      ['Triangles', fmt(info?.totalTriangleCount ?? s.triangles)],
      ['Vertices', fmt(info?.totalVertexCount ?? s.vertices)],
      ['Meshes', fmt(s.meshes)],
      ['Draw calls', info?.drawCallCount !== undefined ? fmt(info.drawCallCount) : '–'],
      ['Materials', fmt(s.materials)],
      ['Textures', fmt(s.textures)],
      ['Animations', fmt(s.animations)],
    ];
    if (info?.hasSkins) statRows.push(['Skinned', 'Yes']);
    if (info?.hasMorphTargets) statRows.push(['Morph targets', 'Yes']);

    let html = `<h3>File</h3>${table(rows)}<h3>Scene</h3>${table(statRows)}`;

    const extensions = info?.extensionsUsed ?? [];
    if (extensions.length) {
      const required = new Set(info?.extensionsRequired ?? []);
      html += `<h3>Extensions</h3>${table(extensions.map((e) => [e, required.has(e) ? 'required' : 'used']))}`;
    }

    const images = (info?.resources ?? []).filter((r) => r.image);
    if (images.length) {
      html += `<h3>Images</h3>${table(
        images.map((r) => [
          r.uri ? decodeSafe(r.uri) : r.pointer,
          `${r.image!.width}×${r.image!.height} ${shortMime(r.mimeType)}`,
        ]),
      )}`;
    }

    html += '<h3>Validation</h3>';
    if (this.validating) {
      html += '<p class="muted">Validating…</p>';
    } else if (this.validationSkipped) {
      html += `<p class="muted">${esc(this.validationSkipped)}</p>`;
    } else if (this.validationError) {
      html += `<p class="muted">Validator failed: ${esc(this.validationError)}</p>`;
    } else if (this.report) {
      const i = this.report.issues;
      html += table([
        ['Errors', fmt(i.numErrors)],
        ['Warnings', fmt(i.numWarnings)],
        ['Infos', fmt(i.numInfos)],
        ['Hints', fmt(i.numHints)],
      ]);
      html += i.messages
        .map(
          (msg) => `<div class="issue sev-${Number(msg.severity) || 0}">
            <div>${esc(msg.message)}</div>
            <code>${SEVERITY[msg.severity] ?? ''} · ${esc(msg.code)}</code>
            ${msg.pointer ? `<span class="pointer">${esc(msg.pointer)}</span>` : ''}
          </div>`,
        )
        .join('');
      if (i.truncated) html += '<p class="muted">Issue list truncated.</p>';
    }
    this.panel.innerHTML = html;
  }
}

/**
 * Runs the validator in a Web Worker so a large file does not freeze the page. If the
 * worker cannot start, validation falls back to the page itself (slower, same result).
 */
class ValidatorClient {
  private worker: Worker | null = null;
  private workerBroken = false;
  private nextId = 1;
  private pending = new Map<number, { resolve: (report: unknown) => void; reject: (err: unknown) => void }>();

  validate(buffer: ArrayBuffer, url: string, uri: string): Promise<unknown> {
    const worker = this.workerBroken ? null : this.ensureWorker();
    if (!worker) return validateOnPage(buffer, url, uri);
    const id = this.nextId++;
    // The model is parsed already; hand the worker its own copy so the page keeps the original.
    const request: ValidateRequest = { id, buffer: buffer.slice(0), url, uri };
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      worker.postMessage(request, [request.buffer]);
    });
  }

  private ensureWorker(): Worker | null {
    if (this.worker) return this.worker;
    try {
      const worker = new Worker(new URL('../validator.worker.ts', import.meta.url), { type: 'module' });
      worker.addEventListener('message', (e: MessageEvent<ValidateResponse>) => {
        const { id, report, error } = e.data;
        const call = this.pending.get(id);
        this.pending.delete(id);
        if (!call) return;
        if (error !== undefined) call.reject(new Error(error));
        else call.resolve(report);
      });
      worker.addEventListener('error', (e) => {
        console.warn('Validator worker failed; validating on the page instead', e.message);
        this.giveUp(new Error(e.message || 'validator worker failed'));
      });
      this.worker = worker;
      return worker;
    } catch (err) {
      console.warn('Validator worker could not start; validating on the page instead', err);
      this.workerBroken = true;
      return null;
    }
  }

  private giveUp(err: Error) {
    this.workerBroken = true;
    this.worker?.terminate();
    this.worker = null;
    for (const call of this.pending.values()) call.reject(err);
    this.pending.clear();
  }
}

async function validateOnPage(buffer: ArrayBuffer, url: string, uri: string): Promise<unknown> {
  const validator = await import('gltf-validator');
  return validator.validateBytes(new Uint8Array(buffer), {
    uri,
    maxIssues: 500,
    externalResourceFunction: async (ref: string) => {
      const res = await fetch(new URL(ref, new URL(url, location.href)));
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      return new Uint8Array(await res.arrayBuffer());
    },
  });
}

function table(rows: [string, string][]) {
  return `<table>${rows.map(([k, v]) => `<tr><td class="muted">${esc(k)}</td><td>${esc(v)}</td></tr>`).join('')}</table>`;
}

function fmt(n: number) {
  return n.toLocaleString('en-US');
}

function formatBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 ** 2).toFixed(1)} MB`;
}

function shortMime(mime?: string) {
  return mime ? mime.replace('image/', '').toUpperCase() : '';
}

function decodeSafe(uri: string) {
  try {
    return decodeURIComponent(uri);
  } catch {
    return uri;
  }
}

function esc(text: string) {
  return String(text).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}
