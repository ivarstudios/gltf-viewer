import GUI from 'lil-gui';
import { ENVIRONMENTS } from '../env';
import { DEFAULT_SETTINGS, type ToneMappingName, type Viewer, type ViewerSettings } from '../viewer';

const STORAGE_KEY = 'ivar-gltf-viewer.settings';

/** lil-gui display settings, persisted per machine in WebView2's localStorage. */
export class SettingsPanel {
  private panel = document.getElementById('settings-panel')!;

  constructor(viewer: Viewer) {
    const settings = loadSettings();
    viewer.applySettings(settings);

    const gui = new GUI({ container: this.panel, title: 'Display' });
    const apply = () => {
      viewer.applySettings(settings);
      saveSettings(settings);
    };
    const envOptions = Object.fromEntries(ENVIRONMENTS.map((e) => [e.name, e.id]));
    const toneOptions: ToneMappingName[] = ['Neutral', 'AgX', 'ACES Filmic', 'Linear'];

    gui.add(settings, 'environment', envOptions).name('Environment').onChange(apply);
    gui.add(settings, 'background').name('Show environment').onChange(apply);
    gui.addColor(settings, 'bgColor').name('Background').onChange(apply);
    gui.add(settings, 'exposure', -4, 4, 0.1).name('Exposure (EV)').onChange(apply);
    gui.add(settings, 'toneMapping', toneOptions).name('Tone mapping').onChange(apply);
    gui.add(settings, 'punctualLights').name('Fill lights').onChange(apply);
    gui.add(settings, 'autoRotate').name('Auto-rotate').onChange(apply);
    gui.add({ frame: () => viewer.frame() }, 'frame').name('Reset camera');
    gui.add(
      {
        reset: () => {
          Object.assign(settings, DEFAULT_SETTINGS);
          gui.controllersRecursive().forEach((c) => c.updateDisplay());
          apply();
        },
      },
      'reset',
    ).name('Reset settings');

    // lil-gui leaves keyboard focus on whatever was used last, so the viewer's shortcuts
    // (Space, Esc, arrows) would keep going to that control. Hand focus back once a change
    // is made or a button/checkbox/slider has been clicked; text fields keep it while typing.
    this.panel.addEventListener('change', () => this.releaseFocus());
    this.panel.addEventListener('pointerup', () => {
      const active = document.activeElement;
      if (active instanceof HTMLSelectElement || active instanceof HTMLInputElement) return;
      this.releaseFocus();
    });
  }

  toggle(force?: boolean) {
    return !this.panel.classList.toggle('hidden', force === undefined ? undefined : !force);
  }

  private releaseFocus() {
    const active = document.activeElement;
    if (active instanceof HTMLElement && this.panel.contains(active)) active.blur();
  }
}

function loadSettings(): ViewerSettings {
  try {
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
    return { ...DEFAULT_SETTINGS, ...stored };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function saveSettings(settings: ViewerSettings) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Settings just won't persist.
  }
}
