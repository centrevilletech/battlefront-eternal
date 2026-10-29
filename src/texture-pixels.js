// A texture's pixels, for the few things that read them on the CPU (lamp
// detection, a laser's hue). A GPU-compressed texture (textures.js) can't be
// read back; it names its PNG in userData.png, fetched on first request.
//
// Returns { data, width, height } (RGBA bytes), or null while not ready.
const entries = new WeakMap();

function read(image) {
  const canvas = document.createElement("canvas");
  canvas.width = image.width;
  canvas.height = image.height;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  context.drawImage(image, 0, 0);
  return { data: context.getImageData(0, 0, image.width, image.height).data, width: image.width, height: image.height };
}

export function texturePixels(texture) {
  if (!texture) return null;
  const known = entries.get(texture);
  if (known) return known.pixels;
  const png = texture.userData?.png;
  if (png) {
    const entry = { pixels: null };
    entries.set(texture, entry);
    fetch(png)
      .then((response) => (response.ok ? response.blob() : Promise.reject(new Error(response.statusText))))
      .then((blob) => createImageBitmap(blob, { premultiplyAlpha: "none", colorSpaceConversion: "none" }))
      .then((bitmap) => { entry.pixels = read(bitmap); bitmap.close?.(); })
      .catch((error) => console.warn(`texture pixels: ${png}: ${error.message}`));
    return null;
  }
  const image = texture.image;
  if (!image?.width || !image?.height || texture.isCompressedTexture) return null;
  const pixels = image.data ? { data: image.data, width: image.width, height: image.height } : read(image);
  entries.set(texture, { pixels });
  return pixels;
}
