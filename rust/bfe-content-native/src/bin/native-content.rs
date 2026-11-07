//! Asset census for native hosts. Reports resource counts and immutable hashes,
//! never embeds content bytes in a distribution or generated source file.
use bfe_content_native::Loader;
fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .ok_or("usage: native-content CONTENT_DIR MAP [QUERY]")?;
    let map = args.next().ok_or("missing map")?;
    let query = args.next().unwrap_or_default();
    let mut loader = Loader::new(root);
    loader.compressed = false;
    let battle = loader.load(&map, &query)?;
    println!("map {map}: world {} models {} actor bodies {} textures {} building nodes {} terrain nodes {}",
        battle.world.len(),battle.models.len(),battle.actors.len(),battle.textures.images.len(),
        battle.buildings_bvh.roots.iter().map(Vec::len).sum::<usize>()/8,
        battle.terrain_bvh.as_ref().map_or(0,|b|b.roots.iter().map(Vec::len).sum::<usize>()/8));
    Ok(())
}
