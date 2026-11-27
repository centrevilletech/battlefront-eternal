//! The FNV-1a hash the Space state digest folds its parts into.
//!
//! Every value goes in as the eight little-endian bytes of one word, so a flag, a count, a
//! sign-extended team and the bits of a double all contribute exactly what a 64-bit word of
//! the same value does; the digest of a battle does not depend on how the caller holds a value.

use bfe_wide::Wide;

pub(super) struct Fnv(bfe_wide::Fnv);

impl Fnv {
    /// The hash of nothing yet.
    pub(super) fn start() -> Self {
        Self(bfe_wide::Fnv::new())
    }

    /// Continues from the hash of what came before.
    pub(super) fn new(hash: Wide) -> Self {
        Self(bfe_wide::Fnv::from_hash(hash))
    }

    pub(super) fn finish(self) -> Wide {
        self.0.finish()
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.0.bytes(bytes);
    }

    /// A whole word: a generator state, an identity carried as bits.
    pub(super) fn word(&mut self, word: Wide) {
        self.0.word(word);
    }

    /// The eight little-endian bytes of a word held in another form.
    pub(super) fn le(&mut self, bytes: [u8; 8]) {
        self.bytes(&bytes);
    }

    /// The bits of a double.
    pub(super) fn bits(&mut self, value: f64) {
        self.bytes(&value.to_le_bytes());
    }

    /// A flag, a generation, a slot or a small enumeration, zero-extended.
    pub(super) fn small(&mut self, value: impl Into<u32>) {
        self.bytes(&value.into().to_le_bytes());
        self.bytes(&[0; 4]);
    }

    /// A length or an index, as `small`; above `u32::MAX` (never) it saturates.
    pub(super) fn count(&mut self, n: usize) {
        self.small(u32::try_from(n).unwrap_or(u32::MAX));
    }

    /// A signed number sign-extended to a whole word.
    pub(super) fn signed(&mut self, value: i32) {
        self.bytes(&value.to_le_bytes());
        self.bytes(&[if value < 0 { 0xff } else { 0 }; 4]);
    }

    /// The all-ones word, the digest's mark for "none".
    pub(super) fn none(&mut self) {
        self.bytes(&[0xff; 8]);
    }

    /// A small number, or the mark for none.
    pub(super) fn option_small(&mut self, value: Option<impl Into<u32>>) {
        match value {
            Some(value) => self.small(value),
            None => self.none(),
        }
    }

    /// An index, or the mark for none.
    pub(super) fn option_count(&mut self, value: Option<usize>) {
        match value {
            Some(n) => self.count(n),
            None => self.none(),
        }
    }

    /// The bits of a double, or the mark for none.
    pub(super) fn option_bits(&mut self, value: Option<f64>) {
        match value {
            Some(v) => self.bits(v),
            None => self.none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Fnv, Wide};

    #[test]
    fn every_kind_of_value_feeds_the_eight_bytes_of_its_word() {
        let mut m = Fnv::start();
        m.word(Wide::new(0x0123_4567, 0x89ab_cdef));
        m.small(7u8);
        m.small(true);
        m.small(u32::MAX);
        m.count(300);
        m.signed(5);
        m.signed(-1);
        m.signed(i32::MIN);
        m.none();
        m.option_small(Some(9u8));
        m.option_small(None::<u8>);
        m.option_count(Some(4));
        m.option_count(None);
        m.option_bits(Some(1.5));
        m.option_bits(None);
        let mut expected = Fnv::start();
        for bytes in [
            [0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01],
            [7, 0, 0, 0, 0, 0, 0, 0],
            [1, 0, 0, 0, 0, 0, 0, 0],
            [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0],
            [0x2c, 0x01, 0, 0, 0, 0, 0, 0],
            [5, 0, 0, 0, 0, 0, 0, 0],
            [0xff; 8],
            [0, 0, 0, 0x80, 0xff, 0xff, 0xff, 0xff],
            [0xff; 8],
            [9, 0, 0, 0, 0, 0, 0, 0],
            [0xff; 8],
            [4, 0, 0, 0, 0, 0, 0, 0],
            [0xff; 8],
            1.5f64.to_le_bytes(),
            [0xff; 8],
        ] {
            expected.bytes(&bytes);
        }
        assert_eq!(m.finish(), expected.finish());
    }

    #[test]
    fn a_small_number_and_the_same_word_hash_alike() {
        let (mut a, mut b) = (Fnv::new(Wide::from_u32(11)), Fnv::new(Wide::from_u32(11)));
        a.small(70_000u32);
        b.word(Wide::from_u32(70_000));
        assert_eq!(a.finish(), b.finish());
    }
}
