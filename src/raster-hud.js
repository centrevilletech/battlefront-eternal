import { contextLimits } from "./rust-render/browser.js";
// Keep original art and glyph coverage. Generated lossless atlases share the
// source UVs; missing upgrades simply retain the imported native bitmap.
import { rendererFailure } from "./rust-render/failure.js";
import { pngImage } from "./texture-images.js";
import {publishImage} from "./interface/images.js";
export class RasterHudAssets {
  constructor(renderer) {
    this.renderer = renderer;
    this.queue = Promise.resolve();
    this.ready = fetch("/common/raster-hud/manifest.json")
      // Absence is a content fact: a 404, or the development server's HTML
      // page for an unknown path. A present but broken index stays a failure.
      .then((response) => {if(response.status===404||/text\/html/.test(response.headers.get("content-type")||""))return null;if(!response.ok)throw Error(`HUD atlas index: ${response.status}`);return response.json();});
    this.ready.catch(cause=>{this.renderer.resources.failure=rendererFailure(this.renderer,cause);});
  }

  upgrade(texture, url) {
    this.queue = this.queue.then(async () => {
      const manifest = await this.ready;
      const path = new URL(url, location.href).pathname;
      if (!path.startsWith("/common/")) return;
      const entry = manifest?.textures?.[path.slice("/common/".length)];
      if (!entry || texture.rasterSource === entry.file) return;
      const width = entry.width * manifest.scale, height = entry.height * manifest.scale;
      // An oversized optional atlas keeps its already loaded native bitmap.
      // Check before decode so it cannot allocate unusable full-size pixels.
      if (Math.max(width, height) > contextLimits(this.renderer).maxTextureSize) return;
      const atlasUrl = `/common/raster-hud/${entry.file}`;
      const image = await pngImage(atlasUrl, contextLimits(this.renderer).maxTextureSize);
      try {
      if (image.width !== width || image.height !== height) throw Error(`HUD atlas dimensions: ${atlasUrl}`);
      await new Promise((resolve) => requestAnimationFrame(resolve));
      // Layout retains native bitmap dimensions while the native source bank
      // replaces its larger allocation behind the same logical sampler id.
      await publishImage(this.renderer,texture,image,entry.width,entry.height);
      texture.rasterSource=entry.file;
      } finally {image.release();}
    }).catch(cause=>{this.renderer.resources.failure=rendererFailure(this.renderer,cause);});
    return this.queue;
  }
}
