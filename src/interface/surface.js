import * as THREE from "three";
import { RasterHudAssets } from "../raster-hud.js";
import { Batch } from "./batch.js";
import { Fonts } from "./fonts.js";
import { IW, anchorMatrix, interfaceCamera, interfaceHeight, planePoint } from "./space.js";

// What every interface screen draws with, with or without a battle behind
// it: the renderer, the textures by name (each upgraded to its 4x atlas
// where the raster-hud pipeline made one, src/raster-hud.js), the game's
// bitmap fonts, the localized strings, a quad batch, and the interface
// camera and space (space.js). The battle HUD (src/hud.js) is one; the
// front end makes its own on a page with no map loaded.
export class InterfaceSurface {
  // renderer: a WebGLRenderer; url(name): a texture's file, or null; fonts
  // and strings: common.json's.
  constructor(renderer, { url = () => null, fonts = null, strings = null } = {}) {
    this.renderer = renderer;
    this.url = url;
    this.strings = strings || {};
    this.loader = new THREE.TextureLoader();
    this.textures = new Map();
    this.solid = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1);
    this.solid.needsUpdate = true;
    this.rasterAssets = new RasterHudAssets(renderer);
    this.batch = new Batch();
    this.scene = new THREE.Scene();
    this.scene.add(this.batch.mesh);
    this.camera = interfaceCamera();
    this.fonts = new Fonts(fonts, (name) => this.texture(name));
    for (const font of Object.values(fonts || {})) this.preload(font.pages);
  }

  // Start loading textures before they're first drawn.
  preload(names) {
    for (const name of names) if (name) this.texture(name);
  }

  load(url) {
    if (!this.textures.has(url)) {
      const texture = this.loader.load(url, (t) => {
        t.userData.loaded = true;
        this.rasterAssets.upgrade(t, url);
      });
      texture.flipY = false;
      texture.wrapS = THREE.ClampToEdgeWrapping;
      texture.wrapT = THREE.ClampToEdgeWrapping;
      this.textures.set(url, texture);
    }
    return this.textures.get(url);
  }

  // A texture by name, or null when there is no such texture.
  texture(name) {
    const url = name ? this.url(name) : null;
    return url ? this.load(url) : null;
  }

  // A localized string, with %s filled in order.
  text(key, ...args) {
    let text = this.strings[key];
    if (!text) return args.join(" ");
    for (const arg of args) text = text.replace("%s", arg);
    return text;
  }

  // Interface height for the window.
  get height() {
    const canvas = this.renderer.domElement;
    return interfaceHeight(canvas.clientWidth, canvas.clientHeight);
  }

  // The matrix from an anchor's space (x right, y down, in units) to the
  // interface camera's.
  anchor(ax, ay, safe = true) {
    return anchorMatrix(ax, ay, safe, this.height);
  }

  // Canvas pixels (from its top left) to interface units.
  toInterface(x, y) {
    const canvas = this.renderer.domElement;
    return { x: (x / Math.max(canvas.clientWidth, 1)) * IW, y: (y / Math.max(canvas.clientHeight, 1)) * this.height };
  }

  // Where a point of a group (x, y, z under m) shows, in interface units.
  toPlane(m, x, y, z = 0) {
    return planePoint(m, x, y, z, this.height);
  }

  // One frame of quads over whatever the canvas holds: draw(batch) adds
  // them. For a page that is only an interface (the front end); the battle
  // HUD has its own render with its layers.
  drawFrame(draw) {
    const batch = this.batch;
    batch.begin();
    draw(batch);
    batch.end();
    const renderer = this.renderer;
    renderer.setRenderTarget(null);
    const autoClear = renderer.autoClear;
    renderer.autoClear = false;
    renderer.render(this.scene, this.camera);
    renderer.autoClear = autoClear;
  }
}

// A url(name) for common.json's textures, which live under /common/.
export function commonTextureUrl(common, root = "/common") {
  const paths = new Map(Object.entries(common?.textures || {}).map(([name, path]) => [name.toLowerCase(), `${root}/${path}`]));
  return (name) => paths.get(String(name).toLowerCase()) || null;
}
