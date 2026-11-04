//! Build the live collision tree in its existing native storage. The host
//! copies roots out only for the JavaScript traversal and debug fallback.
use super::{state, Corners};
use bfe_content::bvh::{build, Positions};

#[no_mangle]
pub extern "C" fn tree_build(id: u32) {
    let state = state();
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

#[no_mangle]
pub extern "C" fn tree_roots(id: u32) -> u32 {
    state().trees[id as usize].roots.len() as u32
}
#[no_mangle]
pub extern "C" fn tree_root_at(id: u32, root: u32) -> *const u32 {
    state().trees[id as usize].roots[root as usize].as_ptr()
}
#[no_mangle]
pub extern "C" fn tree_root_len(id: u32, root: u32) -> u32 {
    state().trees[id as usize].roots[root as usize].len() as u32
}
