//! Work units are 32-bit numbers.
//!
//! A script's budgets (a tick's, a load's, a call's) are far below 2^32 units, and so is the work
//! of any single operation the memory limit lets through. A count taken from a length is
//! therefore converted with [`Units::units`], which stops at the largest 32-bit number instead of
//! wrapping; a sum of units uses `saturating_add`.

/// A length or a count as work units.
pub(crate) trait Units {
    /// The number as units, stopping at `u32::MAX`.
    fn units(self) -> u32;
}

impl Units for usize {
    fn units(self) -> u32 {
        u32::try_from(self).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::Units;

    #[test]
    fn a_count_stops_at_the_largest_unit_number() {
        assert_eq!(0usize.units(), 0);
        assert_eq!(12345usize.units(), 12345);
        assert_eq!((u32::MAX as usize).units(), u32::MAX);
        #[cfg(target_pointer_width = "64")]
        assert_eq!((u32::MAX as usize + 1).units(), u32::MAX);
        assert_eq!(usize::MAX.units(), u32::MAX);
    }
}
