// Keep original art and glyph coverage. Generated lossless atlases share the
// source UVs; missing upgrades simply retain the imported native bitmap.
export class RasterHudAssets {
  constructor(renderer) {
    this.renderer = renderer;
    this.queue = Promise.resolve();
    this.ready = fetch("/common/raster-hud/manifest.json")
      .then((response) => response.ok ? response.json() : null).catch(() => null);
  }

  upgrade(texture, url) {
    this.queue = this.queue.then(async () => {
      const manifest = await this.ready;
      const path = new URL(url, location.href).pathname;
      if (!path.startsWith("/common/")) return;
      const entry = manifest?.textures?.[path.slice("/common/".length)];
      if (!entry || texture.userData.rasterSource === entry.file) return;
      // A decoded ImageBitmap uploads without the image element's
      // conversion work on the main thread (each upgrade used to be a hitch).
      const response = await fetch(`/common/raster-hud/${entry.file}`);
      if (!response.ok) return;
      const image = await createImageBitmap(await response.blob(), { premultiplyAlpha: "none", colorSpaceConversion: "none" });
      if (image.width !== entry.width * manifest.scale || image.height !== entry.height * manifest.scale ||
          Math.max(image.width, image.height) > this.renderer.capabilities.maxTextureSize) return;
      await new Promise((resolve) => requestAnimationFrame(resolve));
      // Texture storage is immutable in WebGL2; retain the shared Texture
      // object and its sampling settings while reallocating its GPU storage.
      texture.dispose();
      texture.userData.nativeWidth = entry.width;
      texture.userData.nativeHeight = entry.height;
      texture.image = image;
      texture.userData.rasterSource = entry.file;
      texture.needsUpdate = true;
      this.renderer.initTexture(texture);
    }).catch((error) => console.warn("HUD raster fallback:", error.message));
    return this.queue;
  }
}
