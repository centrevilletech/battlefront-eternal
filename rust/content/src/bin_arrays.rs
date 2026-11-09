//! A binary content file as JavaScript views it: `new Float32Array(buffer)`,
//! `new Uint32Array(buffer)` and so on, little-endian (every host the game
//! runs on is). A buffer whose length is not a whole number of elements
//! makes the JavaScript throw a RangeError; so does this.

#[derive(Debug, Clone, PartialEq)]
pub struct LengthError {
    pub bytes: usize,
    pub element: usize,
}

impl std::fmt::Display for LengthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "byte length {} is not a multiple of {}", self.bytes, self.element)
    }
}

impl std::error::Error for LengthError {}

fn check(bytes: &[u8], element: usize) -> Result<(), LengthError> {
    if bytes.len().is_multiple_of(element) {
        Ok(())
    } else {
        Err(LengthError { bytes: bytes.len(), element })
    }
}

pub fn f32s(bytes: &[u8]) -> Result<Vec<f32>, LengthError> {
    check(bytes, 4)?;
    Ok(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
}

pub fn u32s(bytes: &[u8]) -> Result<Vec<u32>, LengthError> {
    check(bytes, 4)?;
    Ok(bytes.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect())
}

pub fn u16s(bytes: &[u8]) -> Result<Vec<u16>, LengthError> {
    check(bytes, 2)?;
    Ok(bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect())
}
