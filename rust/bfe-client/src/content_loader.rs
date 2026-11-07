//! Cold prepared-content admission. Device capabilities select one decoded
//! representation before scene registration; the frame loop retains handles.
pub use bfe_content_native::{BattleContent, Loader as NativeContentLoader};

pub trait ContentLoader {
    type Battle;
    fn load_content(&self, map: &str, query: &str) -> Result<Self::Battle, String>;
}
impl ContentLoader for NativeContentLoader {
    type Battle = BattleContent;
    fn load_content(&self, map: &str, query: &str) -> Result<BattleContent, String> {
        self.load(map, query)
    }
}
