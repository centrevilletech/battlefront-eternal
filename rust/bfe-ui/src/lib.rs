//! Shared compiled page components. Screens bind their existing Rust models
//! to this library rather than selecting independent native widget styles.
slint::include_modules!();

/// Install the shared casing and audio hooks on a window importing Theme.
/// The game passes its existing menu sound player; the gallery records calls.
pub fn configure<C>(component: &C, sound: impl Fn(MenuSound) + 'static)
where
    C: slint::ComponentHandle,
    for<'a> Theme<'a>: slint::Global<'a, C>,
{
    let theme = component.global::<Theme>();
    theme.on_sound_requested(sound);
    theme.on_format_label(|text, uppercase| format_label(&text, uppercase).into());
}

/// page/index.js lowercases button source labels, then looks.css applies
/// uppercase or CSS capitalize. Keep punctuation and whitespace intact.
pub fn format_label(text: &str, uppercase: bool) -> String {
    if uppercase {
        return text.to_uppercase();
    }
    let mut result = String::with_capacity(text.len());
    let mut start = true;
    for c in text.to_lowercase().chars() {
        if start {
            result.extend(c.to_uppercase());
        } else {
            result.push(c);
        }
        start = !c.is_alphanumeric() && c != '\'';
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn button_casing_keeps_browser_word_boundaries() {
        assert_eq!(
            super::format_label("STEAM & DISCORD", false),
            "Steam & Discord"
        );
        assert_eq!(super::format_label("1ST/3RD VIEW", false), "1st/3rd View");
        assert_eq!(super::format_label("Enter/exit", true), "ENTER/EXIT");
    }
}
