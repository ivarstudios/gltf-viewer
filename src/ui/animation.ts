import * as THREE from 'three';
import type { Viewer } from '../viewer';

const ALL = '__all__';

/** Bottom bar: clip selection, play/pause, scrubbing and playback speed. */
export class AnimationBar {
  private bar = document.getElementById('anim-bar')!;
  private playBtn = document.getElementById('anim-play') as HTMLButtonElement;
  private clipSelect = document.getElementById('anim-clip') as HTMLSelectElement;
  private scrub = document.getElementById('anim-scrub') as HTMLInputElement;
  private timeLabel = document.getElementById('anim-time')!;
  private speedSelect = document.getElementById('anim-speed') as HTMLSelectElement;

  private actions: THREE.AnimationAction[] = [];
  private duration = 0;
  private playing = false;
  private scrubbing = false;

  constructor(private viewer: Viewer) {
    this.playBtn.addEventListener('click', () => this.toggle());
    this.clipSelect.addEventListener('change', () => {
      this.selectClip(this.clipSelect.value);
      this.clipSelect.blur();
    });
    this.speedSelect.addEventListener('change', () => this.speedSelect.blur());
    this.scrub.addEventListener('pointerdown', () => (this.scrubbing = true));
    this.scrub.addEventListener('pointerup', () => {
      this.scrubbing = false;
      this.scrub.blur();
    });
    this.scrub.addEventListener('input', () => this.seek(Number(this.scrub.value) * this.duration));
    viewer.onFrame = (dt) => this.update(dt);
  }

  /** Call after the viewer's content changes. */
  reset() {
    const { clips } = this.viewer;
    this.actions = [];
    this.clipSelect.innerHTML = '';
    this.bar.classList.toggle('hidden', clips.length === 0);
    if (!clips.length) {
      this.setPlaying(false);
      return;
    }
    clips.forEach((clip, i) => this.clipSelect.add(new Option(clip.name || `Animation ${i + 1}`, String(i))));
    if (clips.length > 1) this.clipSelect.add(new Option('All clips', ALL));
    this.clipSelect.disabled = clips.length === 1;
    this.selectClip('0');
    this.setPlaying(true);
  }

  toggle() {
    if (!this.actions.length) return;
    this.setPlaying(!this.playing);
    this.playBtn.blur();
  }

  private selectClip(value: string) {
    const { mixer, clips } = this.viewer;
    if (!mixer) return;
    mixer.stopAllAction();
    const chosen = value === ALL ? clips : [clips[Number(value)]];
    this.actions = chosen.map((clip) => mixer.clipAction(clip).reset().play());
    this.duration = Math.max(...chosen.map((c) => c.duration), 0);
    mixer.setTime(0);
    this.viewer.invalidate();
    this.syncUi();
  }

  private seek(time: number) {
    this.viewer.mixer?.setTime(time);
    this.viewer.invalidate();
    this.syncUi();
  }

  private setPlaying(playing: boolean) {
    this.playing = playing;
    this.bar.classList.toggle('playing', playing);
  }

  private update(dt: number) {
    const mixer = this.viewer.mixer;
    if (!mixer || !this.playing || this.scrubbing) return;
    mixer.update(dt * Number(this.speedSelect.value));
    this.viewer.invalidate();
    this.syncUi();
  }

  private syncUi() {
    const time = this.actions[0]?.time ?? 0;
    const progress = this.duration > 0 ? (time % this.duration) / this.duration : 0;
    if (!this.scrubbing) this.scrub.value = String(progress);
    this.timeLabel.textContent = `${time.toFixed(2)} / ${this.duration.toFixed(2)}`;
  }
}
