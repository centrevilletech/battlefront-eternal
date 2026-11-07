//! Cold content admission boundary. Native map/asset loaders can implement this
//! without making the window loop know filenames or allocate per entity.
use crate::host::World;

pub trait ContentLoader {
    type Battle: World;
    fn load(&mut self, map: &str) -> Result<Self::Battle, String>;
}

/// Map construction awaits the shared native world owner and asset loaders.
pub struct NativeContentLoader;
impl ContentLoader for NativeContentLoader {
    type Battle = crate::host::CaptureWorld;
    fn load(&mut self, _map: &str) -> Result<Self::Battle, String> {
        Err("native map loading is not available; supply a renderer capture".into())
    }
}
