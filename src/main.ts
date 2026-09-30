import './style.css';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { Viewer } from './viewer';
import { AnimationBar } from './ui/animation';
import { InfoPanel } from './ui/info';
import { SettingsPanel } from './ui/settings';

/** Mirrors `Session` in src-tauri/src/viewer_window.rs. */
interface Session {
  /** glTF/GLB files in the folder, in Explorer's view order. */
  files: string[];
  index: number;
  /** Changes when the current file is modified on disk. */
  stamp: string;
}

// Custom protocol registered in src-tauri/src/protocol.rs (Windows form of `model://`).
const MODEL_ORIGIN = 'http://model.localhost';
const isTauri = '__TAURI_INTERNALS__' in window;

const viewer = new Viewer(document.getElementById('canvas-host')!);
const animationBar = new AnimationBar(viewer);
const infoPanel = new InfoPanel();
const settingsPanel = new SettingsPanel(viewer);

const statusEl = document.getElementById('status')!;
const fileNameEl = document.getElementById('file-name')!;
const filePosEl = document.getElementById('file-pos')!;

let session: Session | null = null;
let loadedKey = '';
let loadToken = 0;

function toModelUrl(path: string): string {
  let normalized = path.replace(/\\/g, '/');
  if (normalized.startsWith('//')) normalized = 'UNC/' + normalized.slice(2);
  return `${MODEL_ORIGIN}/${normalized.split('/').map(encodeURIComponent).join('/')}`;
}

function baseName(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}

function showStatus(html: string | null, isError = false) {
  statusEl.classList.toggle('hidden', html === null);
  statusEl.classList.toggle('error', isError);
  if (html !== null) statusEl.innerHTML = html;
}

async function openSession(next: Session) {
  session = next;
  const path = next.files[next.index];
  if (!path) return;
  const name = baseName(path);
  fileNameEl.textContent = name;
  filePosEl.textContent = next.files.length > 1 ? `${next.index + 1} / ${next.files.length}` : '';
  document.title = `${name} – IVAR glTF Viewer`;

  // Reopening an unchanged file (e.g. Space toggling) keeps the loaded scene.
  const key = `${path}|${next.stamp}`;
  if (key === loadedKey) return;
  await loadModel(isTauri ? toModelUrl(path) : path, name, key);
}

async function loadModel(url: string, name: string, key: string) {
  const token = ++loadToken;
  const isCurrent = () => token === loadToken;
  loadedKey = '';
  viewer.clear();
  animationBar.reset();
  infoPanel.setModel(null);
  showStatus('<div><div class="spinner"></div>Loading…</div>');

  try {
    const res = await fetch(url);
    if (!res.ok) throw new Error(res.status === 404 ? 'File not found.' : `Could not read file (HTTP ${res.status}).`);
    const buffer = await res.arrayBuffer();
    if (!isCurrent()) return;

    const resourcePath = url.slice(0, url.lastIndexOf('/') + 1);
    const gltf = await viewer.parse(buffer, resourcePath);
    if (!isCurrent()) return;

    viewer.setContent(gltf);
    animationBar.reset();
    loadedKey = key;
    showStatus(null);

    const isGlb = new TextDecoder().decode(new Uint8Array(buffer, 0, Math.min(4, buffer.byteLength))) === 'glTF';
    infoPanel.setModel({ fileName: name, byteLength: buffer.byteLength, format: isGlb ? 'GLB' : 'glTF', stats: viewer.stats() });
    void infoPanel.validate(buffer, url, isCurrent);
  } catch (err) {
    if (!isCurrent()) return;
    console.error(err);
    showStatus(`Couldn't open ${escapeHtml(name)}\n\n${escapeHtml(errorMessage(err))}`, true);
  }
}

async function navigate(delta: number) {
  if (!session || session.files.length < 2) return;
  const next = await invoke<Session | null>('navigate', { delta });
  if (next) await openSession(next);
}

async function hideViewer() {
  if (!isTauri) return;
  const win = getCurrentWindow();
  if (await win.isFullscreen()) await setFullscreen(false);
  await invoke('hide_viewer');
}

async function setFullscreen(on: boolean) {
  await getCurrentWindow().setFullscreen(on);
  document.body.classList.toggle('fullscreen', on);
}

async function toggleFullscreen() {
  if (!isTauri) return;
  await setFullscreen(!(await getCurrentWindow().isFullscreen()));
}

function togglePanel(which: 'info' | 'settings') {
  const infoOpen = which === 'info' ? infoPanel.toggle() : infoPanel.toggle(false);
  const settingsOpen = which === 'settings' ? settingsPanel.toggle() : settingsPanel.toggle(false);
  document.getElementById('btn-info')!.classList.toggle('active', infoOpen);
  document.getElementById('btn-settings')!.classList.toggle('active', settingsOpen);
}

// ---------- Input ----------

window.addEventListener('keydown', (e) => {
  const target = e.target as HTMLElement;
  if (target.closest('input:not([type=range]), select, textarea')) {
    if (e.key === 'Escape') target.blur();
    return;
  }
  if (e.ctrlKey || e.altKey || e.metaKey) return;
  let handled = true;
  switch (e.key) {
    case ' ':
      void hideViewer();
      break;
    case 'Escape':
      if (document.body.classList.contains('fullscreen')) void setFullscreen(false);
      else void hideViewer();
      break;
    case 'ArrowRight':
    case 'ArrowDown':
      void navigate(1);
      break;
    case 'ArrowLeft':
    case 'ArrowUp':
      void navigate(-1);
      break;
    case 'f':
    case 'F':
      void toggleFullscreen();
      break;
    case 'i':
    case 'I':
      togglePanel('info');
      break;
    case 's':
    case 'S':
      togglePanel('settings');
      break;
    case 'Home':
      viewer.frame();
      break;
    default:
      handled = false;
  }
  if (handled) e.preventDefault();
});

document.getElementById('btn-close')!.addEventListener('click', () => void hideViewer());
document.getElementById('btn-fullscreen')!.addEventListener('click', () => void toggleFullscreen());
document.getElementById('btn-info')!.addEventListener('click', () => togglePanel('info'));
document.getElementById('btn-settings')!.addEventListener('click', () => togglePanel('settings'));
document.getElementById('issues-badge')!.addEventListener('click', () => togglePanel('info'));
// Keep keyboard shortcuts working after clicking title bar buttons.
document.querySelectorAll('button').forEach((b) => b.addEventListener('mouseup', () => b.blur()));

// ---------- Tauri wiring ----------

function errorMessage(err: unknown) {
  return err instanceof Error ? err.message : String(err);
}

function escapeHtml(text: string) {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

async function initTauri() {
  const log = (message: string) => void invoke('frontend_log', { message }).catch(() => {});
  window.addEventListener('error', (e) => log(`${e.message} @ ${e.filename}:${e.lineno}`));
  window.addEventListener('unhandledrejection', (e) => log(`Unhandled rejection: ${errorMessage(e.reason)}`));
  document.addEventListener('securitypolicyviolation', (e) => log(`CSP blocked ${e.violatedDirective}: ${e.blockedURI}`));
  for (const level of ['error', 'warn'] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      original(...args);
      log(`console.${level}: ${args.map((a) => (a instanceof Error ? a.message : String(a))).join(' ')}`);
    };
  }

  await listen<Session>('open-session', (e) => void openSession(e.payload));

  await getCurrentWebview().onDragDropEvent(async (e) => {
    if (e.payload.type !== 'drop') return;
    const path = e.payload.paths.find((p) => /\.(glb|gltf)$/i.test(p));
    if (!path) return;
    try {
      await openSession(await invoke<Session>('open_path', { path }));
    } catch (err) {
      showStatus(escapeHtml(errorMessage(err)), true);
    }
  });

  const initial = await invoke<Session | null>('get_session');
  if (initial) await openSession(initial);
  else showStatus('Drop a .glb or .gltf file here');
}

if (isTauri) {
  void initTauri();
} else {
  // Plain-browser dev mode: http://localhost:1420/?model=/some/file.glb
  const model = new URLSearchParams(location.search).get('model');
  if (model) void openSession({ files: [model], index: 0, stamp: '' });
  else showStatus('Dev mode: add ?model=&lt;url&gt; to load a file');
}
