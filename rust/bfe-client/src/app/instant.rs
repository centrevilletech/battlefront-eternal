//! Family selection for local Instant Action. Online availability is independent.
use bfe_family::GameFamily;
use bfe_shell::launch::PlannedBattle;
use std::{borrow::Cow, path::Path};

pub(crate) fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("BFE_DEV_SWBF2_IA").as_deref() == Some(std::ffi::OsStr::new("1")))
}

pub(crate) fn check(battle: PlannedBattle<'_>) -> Result<(), bfe_family::UnavailableFamily> {
    if battle.family == GameFamily::Swbf2
        && matches!((battle.map, battle.era, battle.mode),
            ("tat2", Some("gcw"), "eli") | ("spa6", Some("cw"), "ass"))
        && enabled()
    {
        Ok(())
    } else {
        battle.family.require_available()
    }
}

pub(crate) fn content_root(root: &Path, family: GameFamily) -> Cow<'_, Path> {
    match family {
        GameFamily::Swbf1 => Cow::Borrowed(root),
        GameFamily::Swbf2 => Cow::Owned(root.join(family.as_str()).join(".content")),
    }
}
