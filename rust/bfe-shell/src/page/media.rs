//! Texture inputs for MovieBackdrop and MapPreview (src/shell/backdrop.js).
//! Playback, decoding, frame recycling and texture upload belong to the media
//! host. Page samples its current outputs each draw, including an updated image
//! in the same texture or a replacement handle. It never opens a media file.
use bfe_render::interface::UV;

/// An admitted renderer texture index, with the provider's normalized crop.
/// This is the HUD/Core texture namespace, not a raw OpenGL object name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTexture {
    pub texture: u32,
    pub uv: [f64; 4],
}
impl FrameTexture {
    pub fn new(texture: u32) -> Self {
        Self { texture, uv: UV }
    }
    /// Compose the panel's fade/depth sample with the media provider's crop.
    /// The provider can implement movie aspect-cover without changing Page.
    pub fn sample(self, uv: [f64; 4]) -> [f64; 4] {
        let [x, y, right, bottom] = self.uv;
        [
            x + uv[0] * (right - x),
            y + uv[1] * (bottom - y),
            x + uv[2] * (right - x),
            y + uv[3] * (bottom - y),
        ]
    }
}
impl Default for FrameTexture {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Current left/right MovieBackdrop outputs and optional IA preview output.
/// A zero handle or absent preview draws no image; there is no still fallback
/// inside the renderer. The host may supply a composited movie crossfade.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MediaTextures {
    pub backdrops: [FrameTexture; 2],
    pub preview: Option<FrameTexture>,
}
