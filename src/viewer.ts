// Core rendering, based on three-gltf-viewer by Don McCurdy (MIT):
// https://github.com/donmccurdy/three-gltf-viewer
import * as THREE from 'three';
import { GLTFLoader, type GLTF } from 'three/examples/jsm/loaders/GLTFLoader.js';
import { DRACOLoader } from 'three/examples/jsm/loaders/DRACOLoader.js';
import { KTX2Loader } from 'three/examples/jsm/loaders/KTX2Loader.js';
import { MeshoptDecoder } from 'three/examples/jsm/libs/meshopt_decoder.module.js';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { EnvironmentManager } from './env';

export type ToneMappingName = 'Neutral' | 'AgX' | 'ACES Filmic' | 'Linear';

const TONE_MAPPINGS: Record<ToneMappingName, THREE.ToneMapping> = {
  Neutral: THREE.NeutralToneMapping,
  AgX: THREE.AgXToneMapping,
  'ACES Filmic': THREE.ACESFilmicToneMapping,
  Linear: THREE.LinearToneMapping,
};

export interface ViewerSettings {
  environment: string;
  background: boolean;
  bgColor: string;
  exposure: number;
  toneMapping: ToneMappingName;
  punctualLights: boolean;
  autoRotate: boolean;
}

export const DEFAULT_SETTINGS: ViewerSettings = {
  environment: 'neutral',
  background: false,
  bgColor: '#191919',
  exposure: 0,
  toneMapping: 'Neutral',
  punctualLights: true,
  autoRotate: false,
};

export interface SceneStats {
  meshes: number;
  vertices: number;
  triangles: number;
  materials: number;
  textures: number;
  animations: number;
}

export class Viewer {
  readonly renderer: THREE.WebGLRenderer;
  readonly scene = new THREE.Scene();
  readonly camera: THREE.PerspectiveCamera;
  readonly controls: OrbitControls;

  mixer: THREE.AnimationMixer | null = null;
  clips: THREE.AnimationClip[] = [];
  /** Called every frame with the frame delta (seconds), e.g. to drive animation UI. */
  onFrame: ((dt: number) => void) | null = null;

  private settings: ViewerSettings = { ...DEFAULT_SETTINGS };
  private content: THREE.Object3D | null = null;
  private lights: THREE.Light[] = [];
  private envManager: EnvironmentManager;
  private envTexture: THREE.Texture | null = null;
  private envRequest = 0;
  private loader: GLTFLoader;
  private timer = new THREE.Timer();
  private dirty = true;

  constructor(private host: HTMLElement) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, powerPreference: 'high-performance' });
    this.renderer.setPixelRatio(window.devicePixelRatio);
    host.appendChild(this.renderer.domElement);

    this.camera = new THREE.PerspectiveCamera(45, 1, 0.01, 1000);
    this.camera.position.set(1, 0.5, 1.5);
    this.scene.add(this.camera);

    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.screenSpacePanning = true;
    this.controls.autoRotateSpeed = -4;
    this.controls.addEventListener('change', () => (this.dirty = true));

    // Decoders are bundled by Vite via three's import.meta.url references, so no network is needed.
    const draco = new DRACOLoader();
    const ktx2 = new KTX2Loader().detectSupport(this.renderer);
    this.loader = new GLTFLoader()
      .setCrossOrigin('anonymous')
      .setDRACOLoader(draco)
      .setKTX2Loader(ktx2)
      .setMeshoptDecoder(MeshoptDecoder);

    this.envManager = new EnvironmentManager(this.renderer);

    new ResizeObserver(() => this.resize()).observe(host);
    this.resize();
    this.applySettings(this.settings);
    this.renderer.setAnimationLoop((time) => this.tick(time));
  }

  /** Parses a glTF/GLB buffer without touching the current scene. */
  parse(buffer: ArrayBuffer, resourcePath: string): Promise<GLTF> {
    return this.loader.parseAsync(buffer, resourcePath);
  }

  setContent(gltf: GLTF) {
    this.clear();
    const object = gltf.scene ?? gltf.scenes[0];
    if (!object) throw new Error('This file contains no scene.');

    // Recenter inside a wrapper so the model's own transforms stay intact.
    const wrapper = new THREE.Group();
    wrapper.add(object);
    object.updateMatrixWorld(true);
    const box = new THREE.Box3().setFromObject(object);
    if (!box.isEmpty()) object.position.sub(box.getCenter(new THREE.Vector3()));

    this.scene.add(wrapper);
    this.content = wrapper;
    this.frame(box);

    this.clips = gltf.animations ?? [];
    this.mixer = this.clips.length ? new THREE.AnimationMixer(object) : null;
    this.updateLights();
    this.dirty = true;
  }

  /** Points the default camera at the whole model. */
  frame(box = this.content ? new THREE.Box3().setFromObject(this.content) : new THREE.Box3()) {
    const sphere = box.isEmpty() ? new THREE.Sphere(new THREE.Vector3(), 1) : box.getBoundingSphere(new THREE.Sphere());
    const radius = Math.max(sphere.radius, 1e-4);
    const fov = THREE.MathUtils.degToRad(this.camera.fov);
    const fitFov = this.camera.aspect < 1 ? 2 * Math.atan(Math.tan(fov / 2) * this.camera.aspect) : fov;
    const distance = (radius / Math.sin(fitFov / 2)) * 1.05;

    this.camera.near = distance / 100;
    this.camera.far = distance * 100;
    this.camera.updateProjectionMatrix();
    this.camera.position.set(1, 0.45, 1).normalize().multiplyScalar(distance);
    this.controls.target.set(0, 0, 0);
    this.controls.maxDistance = distance * 10;
    this.controls.update();
    this.dirty = true;
  }

  clear() {
    this.mixer?.stopAllAction();
    this.mixer = null;
    this.clips = [];
    if (!this.content) return;
    this.scene.remove(this.content);
    disposeObject(this.content);
    this.content = null;
    this.dirty = true;
  }

  stats(): SceneStats {
    const stats: SceneStats = { meshes: 0, vertices: 0, triangles: 0, materials: 0, textures: 0, animations: this.clips.length };
    const materials = new Set<THREE.Material>();
    const textures = new Set<THREE.Texture>();
    this.content?.traverse((node) => {
      const mesh = node as THREE.Mesh;
      if (!mesh.isMesh) return;
      stats.meshes++;
      const geometry = mesh.geometry;
      const instances = (mesh as THREE.InstancedMesh).isInstancedMesh ? (mesh as THREE.InstancedMesh).count : 1;
      const vertexCount = geometry.attributes.position?.count ?? 0;
      stats.vertices += vertexCount * instances;
      stats.triangles += ((geometry.index ? geometry.index.count : vertexCount) / 3) * instances;
      for (const material of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
        materials.add(material);
        for (const value of Object.values(material)) {
          if (value && (value as THREE.Texture).isTexture) textures.add(value as THREE.Texture);
        }
      }
    });
    stats.materials = materials.size;
    stats.textures = textures.size;
    stats.triangles = Math.round(stats.triangles);
    return stats;
  }

  getSettings(): ViewerSettings {
    return { ...this.settings };
  }

  applySettings(next: Partial<ViewerSettings>) {
    const prev = this.settings;
    this.settings = { ...prev, ...next };
    const s = this.settings;
    this.renderer.toneMapping = TONE_MAPPINGS[s.toneMapping] ?? THREE.NeutralToneMapping;
    this.renderer.toneMappingExposure = Math.pow(2, s.exposure);
    this.controls.autoRotate = s.autoRotate;
    if (s.environment !== prev.environment || !this.envRequest) {
      void this.loadEnvironment(s.environment);
    } else {
      this.updateBackground();
    }
    this.updateLights();
    this.dirty = true;
  }

  private async loadEnvironment(id: string) {
    const request = ++this.envRequest;
    let texture: THREE.Texture | null = null;
    try {
      texture = await this.envManager.get(id);
    } catch (err) {
      console.error('Failed to load environment', id, err);
    }
    if (request !== this.envRequest) return;
    this.envTexture = texture;
    this.scene.environment = texture;
    this.updateBackground();
    this.dirty = true;
  }

  private updateBackground() {
    const s = this.settings;
    if (s.background && this.envTexture) {
      this.scene.background = this.envTexture;
      this.scene.backgroundBlurriness = 0.4;
    } else {
      this.scene.background = new THREE.Color(s.bgColor);
      this.scene.backgroundBlurriness = 0;
    }
    this.dirty = true;
  }

  /** Default fill lights, skipped when the model brings its own (KHR_lights_punctual). */
  private updateLights() {
    let modelHasLights = false;
    this.content?.traverse((node) => {
      if ((node as THREE.Light).isLight) modelHasLights = true;
    });
    const wanted = this.settings.punctualLights && !modelHasLights;
    if (wanted && !this.lights.length) {
      const ambient = new THREE.AmbientLight(0xffffff, 0.3);
      const direct = new THREE.DirectionalLight(0xffffff, 0.8 * Math.PI);
      direct.position.set(0.5, 0, 0.866);
      // Attached to the camera so the key light follows the view.
      this.camera.add(ambient, direct);
      this.lights = [ambient, direct];
    } else if (!wanted && this.lights.length) {
      for (const light of this.lights) this.camera.remove(light);
      this.lights = [];
    }
  }

  invalidate() {
    this.dirty = true;
  }

  private resize() {
    const { clientWidth: w, clientHeight: h } = this.host;
    if (!w || !h) return;
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.dirty = true;
  }

  private tick(time: number) {
    this.timer.update(time);
    const dt = this.timer.getDelta();
    if (document.hidden) return;
    this.onFrame?.(dt);
    // Damping and auto-rotate need continuous updates; update() emits 'change' while moving.
    this.controls.update(dt);
    if (!this.dirty) return;
    this.dirty = false;
    this.renderer.render(this.scene, this.camera);
  }
}

function disposeObject(root: THREE.Object3D) {
  root.traverse((node) => {
    const mesh = node as THREE.Mesh;
    if (mesh.geometry) mesh.geometry.dispose();
    const material = mesh.material;
    if (!material) return;
    for (const m of Array.isArray(material) ? material : [material]) {
      for (const value of Object.values(m)) {
        if (value && (value as THREE.Texture).isTexture) (value as THREE.Texture).dispose();
      }
      m.dispose();
    }
  });
}
