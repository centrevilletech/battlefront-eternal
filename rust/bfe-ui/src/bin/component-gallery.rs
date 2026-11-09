use slint::ComponentHandle;
use std::{cell::Cell, rc::Rc, time::Duration};

// Admission-only test frames exercise the renderer-image slot without game
// files. Timer ticks borrow prebuilt frames, so the gallery's animation does
// not allocate a new pixel buffer every time it presents one.
fn demo_frames() -> Vec<slint::Image> {
    (0..16)
        .map(|phase| {
            let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(128, 96);
            for (i, pixel) in pixels
                .make_mut_bytes()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .enumerate()
            {
                let x = i % 128;
                let y = i / 128;
                pixel.copy_from_slice(&[
                    ((x + phase * 8) % 128 * 2) as u8,
                    (y * 2) as u8,
                    ((x + y + phase * 4) % 128 * 2) as u8,
                    255,
                ]);
            }
            slint::Image::from_rgba8(pixels)
        })
        .collect()
}

fn main() -> Result<(), slint::PlatformError> {
    let gallery = bfe_ui::ComponentGallery::new()?;
    bfe_ui::configure(&gallery, |sound| eprintln!("menu sound: {sound:?}"));
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--theme" => {
                let value = args.next().expect("--theme requires bf1, bf2 or eternal");
                assert!(
                    ["bf1", "bf2", "eternal"].contains(&value.as_str()),
                    "unknown menu theme"
                );
                gallery
                    .global::<bfe_ui::Theme>()
                    .set_menu_theme(value.into());
            }
            "--section" => {
                let value = args.next().expect("--section requires 0..3");
                let value: i32 = value.parse().expect("section must be an integer");
                assert!((0..=3).contains(&value), "unknown gallery section");
                gallery.set_section(value);
            }
            _ => panic!("unknown gallery argument: {arg}"),
        }
    }
    let frames = Rc::new(demo_frames());
    gallery.set_movie_frame(frames[0].clone());
    let weak = gallery.as_weak();
    let phase = Cell::new(0);
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            let Some(gallery) = weak.upgrade() else {
                return;
            };
            let index = (phase.get() + 1) % frames.len();
            phase.set(index);
            gallery.set_movie_frame(frames[index].clone());
        },
    );
    gallery.run()
}
