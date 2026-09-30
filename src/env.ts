import * as THREE from 'three';
import { HDRLoader } from 'three/examples/jsm/loaders/HDRLoader.js';
import { RoomEnvironment } from 'three/examples/jsm/environments/RoomEnvironment.js';

export interface EnvironmentDef {
  id: string;
  name: string;
  /** Bundled HDR under public/hdr, or null for a procedural/none environment. */
  path: string | null;
}

// HDRs are CC0 from Poly Haven (https://polyhaven.com), 1k resolution.
export const ENVIRONMENTS: EnvironmentDef[] = [
  { id: 'neutral', name: 'Neutral', path: null },
  { id: 'studio', name: 'Studio', path: '/hdr/studio_small_09.hdr' },
  { id: 'photo', name: 'Photo studio', path: '/hdr/brown_photostudio_02.hdr' },
  { id: 'sunset', name: 'Venice sunset', path: '/hdr/venice_sunset.hdr' },
  { id: 'outdoor', name: 'Outdoor sky', path: '/hdr/kloofendal_48d_partly_cloudy_puresky.hdr' },
  { id: 'none', name: 'None', path: null },
];

/** Builds and caches prefiltered (PMREM) environment maps. */
export class EnvironmentManager {
  private pmrem: THREE.PMREMGenerator;
  private cache = new Map<string, Promise<THREE.Texture | null>>();

  constructor(renderer: THREE.WebGLRenderer) {
    this.pmrem = new THREE.PMREMGenerator(renderer);
    this.pmrem.compileEquirectangularShader();
  }

  get(id: string): Promise<THREE.Texture | null> {
    let entry = this.cache.get(id);
    if (!entry) {
      entry = this.build(id);
      this.cache.set(id, entry);
      // Allow a retry if loading failed.
      entry.catch(() => this.cache.delete(id));
    }
    return entry;
  }

  private async build(id: string): Promise<THREE.Texture | null> {
    const def = ENVIRONMENTS.find((e) => e.id === id);
    if (!def || def.id === 'none') return null;
    if (def.id === 'neutral') {
      const room = new RoomEnvironment();
      const texture = this.pmrem.fromScene(room, 0.04).texture;
      room.dispose();
      return texture;
    }
    const equirect = await new HDRLoader().loadAsync(def.path!);
    const texture = this.pmrem.fromEquirectangular(equirect).texture;
    equirect.dispose();
    return texture;
  }
}
