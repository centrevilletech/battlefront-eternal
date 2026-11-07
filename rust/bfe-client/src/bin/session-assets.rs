//! Emit digests of native resources for a recording's cold asset references.
fn main() {
    let mut args = std::env::args().skip(1);
    let mut loader = bfe_content_native::Loader::new(args.next().expect("content directory"));
    let map = args.next().expect("map");
    let query = args.next().unwrap_or_default();
    loader.texture_capabilities = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let requests: Vec<bfe_client::session_assets::TextureRequest> =
        serde_json::from_str(&args.next().unwrap_or("[]".into())).expect("texture requests");
    loader.compressed = args.next().as_deref() != Some("false");
    let mut content = loader.load(&map, &query).expect("prepared content");
    bfe_client::session_assets::prepare(&mut content, &requests).expect("native texture sources");
    let assets =
        bfe_client::session_assets::Assets::new(&content, &requests).expect("asset catalog");
    println!("{}", serde_json::to_string(&assets.hashes()).unwrap());
}
