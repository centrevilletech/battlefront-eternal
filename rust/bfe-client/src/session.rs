//! Byte views of scalar banks and the argument decoding the render bridge's
//! resource commands use.
use serde_json::Value;
use std::path::Path;

/// Scalar banks are plain Rust numbers with initialized bytes. The capture
/// protocol uses little endian and refuses other hosts at admission.
mod scalar {
    pub trait Sealed {}
    impl Sealed for f64 {}
    impl Sealed for f32 {}
    impl Sealed for u32 {}
    impl Sealed for i32 {}
    impl Sealed for u16 {}
    impl Sealed for u8 {}
}
// Sealed to initialized, padding-free numbers. Copy alone would also admit
// structs with uninitialized padding, which cannot safely become byte slices.
pub trait Scalar: Copy + scalar::Sealed {}
impl Scalar for f64 {}
impl Scalar for f32 {}
impl Scalar for u32 {}
impl Scalar for i32 {}
impl Scalar for u16 {}
impl Scalar for u8 {}
pub fn bytes_mut<T: Scalar>(values: &mut [T]) -> &mut [u8] {
    unsafe {
        std::slice::from_raw_parts_mut(values.as_mut_ptr().cast(), std::mem::size_of_val(values))
    }
}
pub fn bytes<T: Scalar>(values: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
}
// Shared network banks retain their RefCell loan only while a host copy runs.
// The remaining owners lend slices directly; neither form creates a mirror.
pub enum Bank<'a> {
    Slice(&'a [u8]),
    Shared(std::cell::Ref<'a, [u8]>),
}
pub enum BankMut<'a> {
    Slice(&'a mut [u8]),
    Shared(std::cell::RefMut<'a, [u8]>),
}
impl std::ops::Deref for Bank<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Slice(v) => v,
            Self::Shared(v) => v,
        }
    }
}
impl AsRef<[u8]> for Bank<'_> {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
impl std::ops::Deref for BankMut<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Slice(v) => v,
            Self::Shared(v) => v,
        }
    }
}
impl std::ops::DerefMut for BankMut<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Slice(v) => v,
            Self::Shared(v) => v,
        }
    }
}
pub struct BlobStore {
    data: Vec<u8>,
    spans: Vec<(usize, usize)>,
    assets: std::collections::BTreeMap<String, Vec<u8>>,
}
impl BlobStore {
    pub fn load(dir: &Path, meta: &Value) -> Result<Self, String> {
        if !cfg!(target_endian = "little") {
            return Err("session requires a little endian host".into());
        }
        let data = std::fs::read(dir.join("blobs.bin")).map_err(|e| e.to_string())?;
        let mut spans = Vec::new();
        for span in meta["blobs"].as_array().ok_or("missing blob table")? {
            let at = span[0].as_u64().ok_or("invalid blob offset")? as usize;
            let len = span[1].as_u64().ok_or("invalid blob length")? as usize;
            if at.checked_add(len).is_none_or(|end| end > data.len()) {
                return Err("blob span exceeds archive".into());
            }
            spans.push((at, len));
        }
        Ok(Self {
            data,
            spans,
            assets: Default::default(),
        })
    }
    pub fn get(&self, id: usize) -> Result<&[u8], String> {
        let &(at, len) = self.spans.get(id).ok_or("unknown blob")?;
        Ok(&self.data[at..at + len])
    }
}
pub struct Arguments<'a> {
    args: &'a [Value],
    blobs: &'a BlobStore,
}
impl<'a> Arguments<'a> {
    pub fn new(args: &'a [Value], blobs: &'a BlobStore) -> Self {
        Self { args, blobs }
    }
    fn arg(&self, i: usize) -> Result<&Value, String> {
        self.args
            .get(i)
            .ok_or_else(|| format!("missing argument {i}"))
    }
    pub fn string(&self, i: usize) -> Result<String, String> {
        Ok(self.arg(i)?.as_str().ok_or("expected string")?.into())
    }
    pub fn bool(&self, i: usize) -> Result<bool, String> {
        let a = self.arg(i)?;
        Ok(a.as_bool().unwrap_or_else(|| a.as_i64().unwrap_or(0) != 0))
    }
    pub fn f64(&self, i: usize) -> Result<f64, String> {
        self.arg(i)?
            .as_f64()
            .ok_or_else(|| "expected finite number".into())
    }
    pub fn f32(&self, i: usize) -> Result<f32, String> {
        Ok(self.f64(i)? as f32)
    }
    pub fn u32(&self, i: usize) -> Result<u32, String> {
        u32::try_from(self.arg(i)?.as_u64().ok_or("expected unsigned number")?)
            .map_err(|_| "integer exceeds u32".into())
    }
    pub fn i32(&self, i: usize) -> Result<i32, String> {
        i32::try_from(self.arg(i)?.as_i64().ok_or("expected signed number")?)
            .map_err(|_| "integer exceeds i32".into())
    }
    pub fn usize(&self, i: usize) -> Result<usize, String> {
        Ok(self.u32(i)? as usize)
    }
    pub fn owner(&self, i: usize) -> Result<usize, String> {
        Ok(self.arg(i)?["owner"]
            .as_u64()
            .ok_or("expected owner reference")? as usize)
    }
    pub fn raw(&self, i: usize) -> Result<&[u8], String> {
        if let Some(asset) = self.arg(i)?["asset"].as_str() {
            self.blobs
                .assets
                .get(asset)
                .map(Vec::as_slice)
                .ok_or_else(|| format!("unknown native asset {asset}"))
        } else {
            self.blobs.get(
                self.arg(i)?["blob"]
                    .as_u64()
                    .ok_or("expected blob reference")? as usize,
            )
        }
    }
}
// Scalar blobs must contain complete little-endian values. Checking the
// remainder also covers byte arrays, whose element width is exactly one.
macro_rules! arrays {
    ($($method:ident,$ty:ty,$n:literal);*) => {
        $(impl Arguments<'_> {
            pub fn $method(&self, i: usize) -> Result<Vec<$ty>, String> {
                let data = self.raw(i)?;
                let chunks = data.chunks_exact($n);
                if !chunks.remainder().is_empty() {
                    return Err("unaligned scalar blob".into());
                }
                Ok(chunks.map(|b| <$ty>::from_le_bytes(b.try_into().unwrap())).collect())
            }
        })*
    };
}
arrays!(f64s,f64,8;f32s,f32,4;u32s,u32,4;i32s,i32,4;u16s,u16,2;u8s,u8,1);
