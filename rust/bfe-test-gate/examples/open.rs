//! A binary that uses the open gate: the release check's proof that it finds
//! the marker in a binary that can open the 2005 game.
fn main() {
    let open = std::hint::black_box(bfe_test_gate::OPEN);
    println!("{open:?} plays the 2005 game: {}", open.plays(bfe_family::GameFamily::Swbf2));
}
