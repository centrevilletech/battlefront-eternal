//! Build the live collision tree in its existing native storage. The host
//! copies roots out only for the JavaScript traversal and debug fallback.
use super::Corners;
use bfe_content::bvh::{build, Positions};

impl super::State {
    pub fn build_tree(&mut self, id: u32) {
        let state = self;
        let tree = &mut state.trees[id as usize];
        let positions = match &state.positions[tree.positions] {
            Corners::F32(values) => Positions::F32(values),
            Corners::F64(values) => Positions::F64(values),
        };
        let result = build(positions, &tree.index);
        tree.roots = result.roots;
        tree.indirect = result.indirect;
        tree.indexed = true;
    }
}
pub(crate) fn tree_roots_in(state: &super::State, id: u32) -> u32 { state.trees[id as usize].roots.len() as u32 }

pub(crate) fn tree_root_at_in(state: &super::State, id: u32, root: u32) -> &[u32] { &state.trees[id as usize].roots[root as usize] }

pub(crate) fn tree_root_len_in(state: &super::State, id: u32, root: u32) -> u32 { state.trees[id as usize].roots[root as usize].len() as u32 }

#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn tree_build(id: u32) { crate::abi::facade::cold().collision.state.build_tree(id) }
    #[no_mangle]
    pub extern "C" fn tree_roots(id: u32) -> u32 { tree_roots_in(&crate::abi::facade::cold().collision.state, id) }
    #[no_mangle]
    pub extern "C" fn tree_root_at(id: u32, root: u32) -> *const u32 { tree_root_at_in(&crate::abi::facade::cold().collision.state, id, root).as_ptr() }
    #[no_mangle]
    pub extern "C" fn tree_root_len(id: u32, root: u32) -> u32 { tree_root_len_in(&crate::abi::facade::cold().collision.state, id, root) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{tree_build, tree_roots, tree_root_at, tree_root_len};
