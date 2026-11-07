import * as THREE from "three";
import { PngTextureLoader } from "./texture-images.js";
import { basisImage } from "./texture-decode.js";
import { rendererModule } from "./rust-render/load.js";
import { rendererFailure } from "./rust-render/failure.js";

// Imported textures are raw gamma-space values. Rust selects a GPU format and
// transcodes every authored mip; normal maps retain their lossless PNG source.
// Clones share pixels while keeping their own wrap and filtering descriptors.
const KEEP_PNG = /_bump\.png$/i;

export class TextureLibrary {
  constructor({ renderer, manager, compressed = true }) {
    this.manager = manager;
    this.renderer = renderer;
    this.images = new PngTextureLoader(manager, renderer);
    this.compressed = compressed;
    this.twins = new Map();
    this.cache = new Map();
    this.stats = { compressed: 0, png: 0, failed: 0 };
    // Discovery uses the actual drawing context. Decoder workers receive only
    // Rust's capability mask and never create an unrelated context of their own.
    this.capabilities = compressed ? rendererModule().then(({ module }) => module.texture_capabilities(renderer.getContext())) : Promise.resolve(0);
  }

  // The optional folder index maps PNG names to alternate encoded assets.
  // Its absence leaves the original PNG source, decoded by the same Rust owner.
  // Discovery never changes sampler settings or transfers a texture payload.
  async prepare(urls) {
    if (!this.compressed) return;
    await this.capabilities;
    const folders = new Set([...urls].map(url => url.slice(0, url.lastIndexOf("/"))));
    await Promise.all([...folders].map(async folder => {
        const response = await fetch(`${folder}/ktx2.json`);
        // Only absence is a content choice. Corrupt indexes and transport or
        // decoder failures must never silently multiply GPU memory with PNGs.
        if (response.status === 404) return;
        if (!response.ok) throw new Error(`KTX2 index ${folder}: ${response.status}`);
        const manifest = await response.json();
        // A present index must describe its files. Treating malformed metadata
        // as an empty folder would silently select PNG for every declared twin.
        if(!manifest || !manifest.files || typeof manifest.files!=="object" || Array.isArray(manifest.files))throw Error(`Invalid KTX2 index ${folder}`);
        for (const [png, entry] of Object.entries(manifest.files)) {
          if(!entry || typeof entry.ktx2!=="string" || !entry.ktx2.length)throw Error(`Invalid KTX2 entry ${folder}/${png}`);
          if (!KEEP_PNG.test(png)) this.twins.set(`${folder}/${png}`, `${folder}/${entry.ktx2}`);
        }
    })).catch(cause => { throw rendererFailure(this.renderer,cause); });
  }

  // Port of the former TextureLoader/KTX2Loader loading boundary. Return a
  // descriptor immediately so materials can share it before bytes arrive.
  // Completion includes Rust upload, not merely network delivery/transcoding.
  load(url) {
    let texture = this.cache.get(url);
    if (texture) return texture;
    const twin = this.twins.get(url);
    if (twin) {
      texture = new THREE.Texture();
      texture.userData.png = url;
      const copies = new Set();
      // Texture.clone does not copy compressed/data upload-kind markers. Keep
      // both early and late sampler clones consistent with their shared source.
      texture.clone = function () {
        const copy = THREE.Texture.prototype.clone.call(this);
        if (this.userData.filled) kind(copy, this);
        else copies.add(copy);
        return copy;
      };
      this.manager.itemStart(twin);
      // One transferred payload backs every authored mip view. Rust retains
      // those blocks once per Source after upload; cloned sampler descriptors
      // must not retain another encoded KTX container or decoded RGBA chain.
      this.capabilities.then(bits => basisImage(twin, this.renderer.capabilities.maxTextureSize, bits)).then(async image => {
        try {
        const mips = image.mips.map(m => ({ width: m.width, height: m.height,
          data: new Uint8Array(image.data.buffer, image.data.byteOffset + m.offset, m.length) }));
        const loaded = { isCompressedTexture: image.format !== THREE.RGBAFormat, isDataTexture: image.format === THREE.RGBAFormat,
          image: { width: image.width, height: image.height, data: image.format === THREE.RGBAFormat ? mips[0].data : undefined }, mipmaps: mips,
          format: image.format, type: THREE.UnsignedByteType, minFilter: mips.length > 1 ? THREE.LinearMipmapLinearFilter : THREE.LinearFilter,
          magFilter: THREE.LinearFilter, premultiplyAlpha: image.premultiplied };
        fill(texture, loaded);
        for (const copy of copies) fill(copy, loaded);
        await Promise.all([this.renderer.resources.enqueue(texture), ...Array.from(copies, copy => this.renderer.resources.publishStatic(copy))]);
        copies.clear();
        this.stats.compressed++;
        } finally { image.release(); }
      }).catch(cause => {
        this.stats.failed++;
        // The loading screen reads this same sticky failure as PNG loading.
        this.images.failure ??= rendererFailure(this.renderer,cause);
        this.renderer.resources.failure=this.images.failure;
        this.manager.itemError(twin);
        copies.clear();
      }).finally(() => this.manager.itemEnd(twin));
    } else {
      texture = this.images.load(url, image => this.renderer.resources.enqueue(image));
      this.stats.png++;
    }
    this.cache.set(url, texture);
    return texture;
  }

  dispose() { this.cache.clear(); }
}

// The scene descriptors describe the bytes Rust produced. Neither marker
// decides a format or transcodes pixels in JavaScript.
function kind(texture, loaded) {
  texture.isCompressedTexture = Boolean(loaded.isCompressedTexture);
  texture.isDataTexture = Boolean(loaded.isDataTexture);
}

// Port of KTX2Loader's completed texture publication. Container alpha flags
// remain explicit while source dimensions and mip views are shared by clones.
// Marking the descriptor changed publishes its generation to the resource bank.
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
  // Compressed rows cannot be flipped by upload. Imported KTX2 and PNG rows
  // already share the game's unflipped orientation and raw colour values.
  texture.flipY = false;
  texture.colorSpace = THREE.NoColorSpace;
  texture.needsUpdate = true;
}
