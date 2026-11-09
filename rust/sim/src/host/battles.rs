//! Authority battle teardown, matching local-server.js dispose: old handles
//! cannot enter the new world's admission, even if a caller retains an alias.
use crate::abi::context::{Context, Error};

/// Dispose explicitly before a host admits the next battle. Dropping just
/// the caller's handle would leave any aliases pointing at a live old world.
pub fn finish(context: &Context) -> Result<(), Error> {
    context.dispose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_aliases_are_invalid_after_a_new_battle_starts() {
        let old = Context::new();
        let alias = old.clone();
        finish(&old).unwrap();
        let next = Context::new();
        assert!(alias.string_count().is_err());
        assert!(next.string_count().is_ok());
    }
}
