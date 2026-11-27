use bfe_sim::{ai::owned::load::Navigation, gen::AiNavSetup, store::Strings};

#[test]
fn classic_map_navigation_reader() {
    let strings = Strings::new();
    for row in include_str!("fixtures/navigation-reader.txt").lines() {
        let (json, expected) = row.split_once(' ').unwrap();
        let setup = AiNavSetup { json: json.as_bytes().into(), ..Default::default() };
        let answer = Navigation::read(&setup, &strings).err().unwrap_or_else(|| "OK".into());
        assert_eq!(answer, expected, "{json}");
    }
}
