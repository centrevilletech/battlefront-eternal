//! Active vertex data and draw ranges of the released planet loading screen.
use bfe_render::interface::{Batch, Glyph};
use bfe_shell::loading::{LoadingTimeline, LookId};
use bfe_shell::loading2::loading_screen_for;
use bfe_shell::launch::GameFamily;
use bfe_wide::{Fnv, Wide};
use bfe_shell::render::{draw_loading_screen, ShellFonts, ShellTextures, StageTexture};

#[test]
fn planet_loading_batches() {
    let mut fonts = ShellFonts::default();
    for font in [&mut fonts.tiny, &mut fonts.medium] {
        font.height = 18.;
        for code in 32..=126 {
            font.glyphs.insert(code, Glyph {
                page: 2, advance: 7., bounds: [0., 1., 6., 15.], uv: [0.1, 0.2, 0.3, 0.4],
            });
        }
    }
    let mut textures = ShellTextures::default();
    for i in 0..4 {
        textures.loading_stages.push(StageTexture {
            texture: 3 + i, width: 256., height: 256.,
            target: (i < 3).then_some([32., 64., 96., 80.]),
        });
    }
    for (i, key) in ["load_edge_horz", "load_edge_vert", "load_rotating_disk",
                     "load_rotating_disk_eternal", "load_rotating_disk_bf2"].iter().enumerate() {
        textures.by_name.insert((*key).into(), 8 + i as u32);
    }
    let timeline = LoadingTimeline::new(3.5, 1., 0., vec![Some([0.125, 0.25, 0.375, 0.3125]); 4]);
    for (look, expected) in [
        (LookId::Bf1, Wide::new(0x9aa6_e9d0, 0x85b2_c3c9)),
        (LookId::Eternal, Wide::new(0x9554_333d, 0xc686_b5c1)),
        (LookId::Bf2, Wide::new(0xbdb7_4158, 0xd65f_ffd9)),
    ] {
        let mut hash = Fnv::new();
        let mut feed = |bytes: &[u8]| hash.bytes(bytes);
        let mut batch = Batch::new();
        for (w, h) in [(1024., 768.), (1280., 720.)] {
            for t in [0., 2.5, 3.6, 4.5, 8.4, 13.5] {
                draw_loading_screen(&mut batch, &fonts, &textures, loading_screen_for(GameFamily::Swbf1),
                                    &timeline.at(t), None, t, "WORLD",
                             "LOADING", look, w, h);
                for n in [batch.count, batch.start, batch.end, batch.draws.len()] {
                    feed(&(n as u32).to_le_bytes());
                }
                for draw in &batch.draws {
                    for n in [draw.first as u32, draw.count as u32, draw.texture, draw.mask] {
                        feed(&n.to_le_bytes());
                    }
                }
                let n = batch.count * 4;
                for data in [&batch.position[..n * 3], &batch.uv[..n * 2],
                             &batch.mask_uv[..n * 2], &batch.tint[..n * 4]] {
                    for value in data { feed(&value.to_le_bytes()); }
                }
            }
        }
        assert_eq!(hash.finish(), expected, "{look:?}");
    }
}
