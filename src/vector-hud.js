// Rasterize vector masters once at display density; retain the batched GPU
// quads. Original bitmaps remain the fallback for imports without vectors.
export class VectorHudAssets {
  constructor(renderer) {
    this.renderer = renderer;
    this.entries = new Map();
    this.queue = Promise.resolve();
    this.ready = fetch("/common/vector-hud/manifest.json")
      .then((response) => response.ok ? response.json() : null).catch(() => null);
    this.scale = this.density();
    window.addEventListener("resize", () => this.refresh());
  }

  refresh() {
    const scale = this.density();
    if (scale === this.scale) return;
    this.scale = scale;
    for (const [texture, url] of this.entries) this.upgrade(texture, url);
  }

  density() {
    // The HUD spans 1024 interface units. Oversample for rotated panels and
    // enlarged score text, while bounding the atlas cache size.
    return Math.min(8, Math.max(2, Math.ceil(this.renderer.domElement.width / 1024 * 2)));
  }

  upgrade(texture, url) {
    this.entries.set(texture, url);
    // Serialize decoding/uploads instead of replacing every atlas together.
    this.queue = this.queue.then(async () => {
      const manifest = await this.ready;
      const path = new URL(url, location.href).pathname;
      if (!path.startsWith("/common/")) return;
      const entry = manifest?.textures?.[path.slice("/common/".length)];
      if (!entry) return;
      const scale = this.density();
      if (texture.userData.vectorScale === scale) return;
      const response = await fetch(`/common/vector-hud/${entry.svg}`);
      if (!response.ok) return;
      const document = new DOMParser().parseFromString(await response.text(), "image/svg+xml");
      const root = document.documentElement;
      root.setAttribute("width", String(entry.width * scale));
      root.setAttribute("height", String(entry.height * scale));
      const source = URL.createObjectURL(new Blob([new XMLSerializer().serializeToString(root)], { type: "image/svg+xml" }));
      try {
        const image = new Image(); image.src = source;
        await image.decode();
        await new Promise((resolve) => requestAnimationFrame(resolve));
        // WebGL2 texture storage is immutable. Reallocate the GPU handle
        // when replacing a native/previous-density atlas with larger pixels;
        // retain the Texture object referenced by every HUD batch.
        texture.dispose();
        texture.image = image;
        texture.userData.vectorScale = scale;
        texture.userData.vectorSource = entry.svg;
        texture.needsUpdate = true;
        this.renderer.initTexture(texture);
      } finally { URL.revokeObjectURL(source); }
    }).catch((error) => console.warn("HUD vector fallback:", error.message));
    return this.queue;
  }
}
