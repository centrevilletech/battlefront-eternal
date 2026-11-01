import * as THREE from "three";
import { KTX2Loader } from "three/addons/loaders/KTX2Loader.js";
import transcoderScript from "three/addons/libs/basis/basis_transcoder.js?url";
import transcoderBinary from "three/addons/libs/basis/basis_transcoder.wasm?url";
import { rendererName, unpacksBc7 } from "./gpu-info.js";

// The game's textures by URL, as GPU-compressed KTX2 where the import made
// one (tools/ktx2.py) and as the extracted PNG otherwise.
//
// Each textures folder has a ktx2.json listing its PNGs that have a KTX2
// twin: Basis UASTC with its full mip chain, transcoded in workers to what
// this GPU samples directly (BC7 on desktop, ASTC or ETC2 on mobile; BC1 and
// BC3 on GPUs that only appear to take BC7, gpu-info.js unpacksBc7). That
// is a quarter of the memory and bandwidth of RGBA8, and nothing to build
// mipmaps from on the main thread. The values are stored raw, as the PNGs
// are: the game lit and blended in gamma space.
//
// Normal maps (*_bump) stay PNG: UASTC's colour metrics leave visible blocks
// in their lighting. A texture is returned at once and filled in when its
// data arrives, as TextureLoader does; a KTX2 that fails falls back to its
// PNG. userData.png keeps the PNG for code that reads pixels on the CPU
// (texture-pixels.js).
const KEEP_PNG = /_bump\.png$/i;

export class TextureLibrary {
  constructor({ renderer, manager, compressed = true }) {
    this.manager = manager;
    this.images = new THREE.TextureLoader(manager);
    this.twins = new Map();
    this.cache = new Map();
    this.stats = { compressed: 0, png: 0, failed: 0 };
    if (compressed && renderer) {
      // KTX2Loader fetches its transcoder by these names from one folder;
      // Vite serves (and fingerprints) them wherever it likes.
      const modify = manager.urlModifier;
      manager.setURLModifier((url) => {
        if (url.endsWith("basis_transcoder.js")) return transcoderScript;
        if (url.endsWith("basis_transcoder.wasm")) return transcoderBinary;
        return modify ? modify(url) : url;
      });
      this.ktx2 = new KTX2Loader(manager).setTranscoderPath("/basis/").detectSupport(renderer);
      // BC7 the driver would unpack to RGBA8: BC1/BC3, which the GPU reads
      // itself, instead (the next format in KTX2Loader's order here).
      if (this.ktx2.workerConfig.bptcSupported && unpacksBc7(rendererName(renderer.getContext()))) this.ktx2.workerConfig.bptcSupported = false;
    }
  }

  // Read the manifests of the folders these PNG URLs live in.
  async prepare(urls) {
    if (!this.ktx2) return;
    const folders = new Set([...urls].map((url) => url.slice(0, url.lastIndexOf("/"))));
    await Promise.all([...folders].map(async (folder) => {
      try {
        const response = await fetch(`${folder}/ktx2.json`);
        if (!response.ok) return;
        const manifest = await response.json();
        for (const [png, entry] of Object.entries(manifest.files || {})) {
          if (!KEEP_PNG.test(png)) this.twins.set(`${folder}/${png}`, `${folder}/${entry.ktx2}`);
        }
      } catch {
        // No manifest (or none readable): this folder's PNGs as they are.
      }
    }));
  }

  load(url) {
    let texture = this.cache.get(url);
    if (texture) return texture;
    const twin = this.twins.get(url);
    if (twin) {
      texture = new THREE.Texture();
      texture.userData.png = url;
      // A copy (clone(), as the droideka's shield makes for its own wrap)
      // shares the image but not what fill() sets beside it: the kind of
      // texture and the mip chain. Copies made before the data arrives are
      // filled with it too; later ones keep the kind clone() leaves out.
      // Without this a copy uploads the compressed placeholder as an image.
      const copies = new Set();
      texture.clone = function () {
        const copy = THREE.Texture.prototype.clone.call(this);
        if (this.userData.filled) kind(copy, this);
        else copies.add(copy);
        return copy;
      };
      this.manager.itemStart(twin);
      this.ktx2.load(twin, (loaded) => {
        fill(texture, loaded);
        for (const copy of copies) fill(copy, loaded);
        copies.clear();
        this.stats.compressed++;
        this.manager.itemEnd(twin);
      }, undefined, (error) => {
        console.warn(`KTX2 ${twin}: ${error?.message || error}; using the PNG`);
        this.stats.failed++;
        // The PNG is an ordinary image, which copies share as they are.
        copies.clear();
        this.images.load(url, (image) => {
          texture.image = image.image;
          texture.needsUpdate = true;
        });
        this.manager.itemEnd(twin);
      });
    } else {
      texture = this.images.load(url);
      this.stats.png++;
    }
    this.cache.set(url, texture);
    return texture;
  }

  dispose() {
    this.ktx2?.dispose();
  }
}

// The kind of texture a transcoded one is, which decides how three uploads
// it (compressed blocks, or raw rows as data) rather than as an image.
function kind(texture, loaded) {
  texture.isCompressedTexture = Boolean(loaded.isCompressedTexture);
  texture.isDataTexture = Boolean(loaded.isDataTexture);
}

// The transcoded data into the texture the materials already hold.
function fill(texture, loaded) {
  kind(texture, loaded);
  texture.userData.filled = true;
  texture.image = loaded.image;
  texture.mipmaps = loaded.mipmaps;
  texture.format = loaded.format;
  texture.type = loaded.type;
  texture.generateMipmaps = false;
  texture.minFilter = loaded.minFilter;
  texture.magFilter = loaded.magFilter;
  texture.premultiplyAlpha = loaded.premultiplyAlpha;
  // Compressed data can't be flipped on upload; KTX2 rows are stored as the
  // PNG's, which the game draws unflipped too.
  texture.flipY = false;
  texture.colorSpace = THREE.NoColorSpace;
  texture.needsUpdate = true;
}
