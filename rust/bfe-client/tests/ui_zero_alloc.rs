//! The browser's per-frame checks allocate nothing, for a 2004 row and for
//! rows that name another game: the real `ServerBrowser::matches_filters`, the
//! compatibility badge, the row's game reading, and the map filter's index in
//! the shell's map list.

use bfe_family::GameFamily;
use bfe_online::browser::{
    compatible, row_tag, FilterUpdate, ServerBrowser, ServerBrowserService, ServerContract, ServerEntry,
};
use bfe_shell::data::ShellMap;
use bfe_test_alloc::Counting;
use std::hint::black_box;

#[global_allocator]
static GLOBAL: Counting = Counting::new();

fn counted(mut frame: impl FnMut()) -> u32 {
    bfe_test_alloc::measure_all(|| {
        for _ in 0..1000 {
            frame();
        }
    })
    .1
    .allocations
}

#[test]
fn browser_rows_cost_no_allocation_per_frame() {
    let rows: Vec<ServerEntry> = [
        r#"{"id":"a","battle":{"map":"kam1","era":"cw"},"rotation":["swbf1/kam1:rep:conquest"]}"#,
        r#"{"id":"b","battle":{"family":"swbf9","map":"kam1"},"rotation":[{"family":"swbf9","map":"kam1"}]}"#,
        r#"{"id":"c","battle":{"family":"swbf2","map":"swbf2/kam1"},"rotation":["swbf2/kam1:gcw:hunt"],"query":"?map=swbf2/kam1"}"#,
        r#"{"id":"d","battle":{"family":"swbf1","map":"kam1"},"rotation":["swbf2/kam1:gcw:hunt"]}"#,
    ]
    .iter()
    .map(|row| serde_json::from_str(row).unwrap())
    .collect();
    let contract = ServerContract::current();
    let maps: Vec<ShellMap> = [(GameFamily::Swbf1, "bes2"), (GameFamily::Swbf2, "kam1"), (GameFamily::Swbf1, "kam1")]
        .into_iter()
        .map(|(family, id)| ShellMap { family, id: id.into(), name: id.into(), title_resolved: true, name_key: None, extracted: Some(true), eras: Default::default(), fly: None, load: None })
        .collect();
    for filter in ["all", "kam1", "swbf2/kam1", "swbf9/kam1"] {
        let mut browser = ServerBrowser::default();
        browser.set_filter(FilterUpdate::Map(filter.to_string()));
        for row in &rows {
            let allocations = counted(|| {
                black_box(browser.matches_filters(black_box(row)));
                black_box(compatible(black_box(row), &contract));
                black_box(row_tag(black_box(row), &contract));
                black_box(row.family_reading());
                black_box(maps.iter().position(|m| m.has_browser_id(black_box(filter))));
            });
            assert_eq!(allocations, 0, "filter {filter}, row {}", row.id);
        }
    }
}
