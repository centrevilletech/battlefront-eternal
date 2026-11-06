// Rust-owned KTX2 decode inputs and output buffers. The C++ transcoder borrows
// encoded bytes; no JavaScript heap, worker callback or GL object crosses it.
// Its handle is closed before the backing bytes are dropped, even on errors.
use std::{ffi::c_void, ops::Range, ptr::NonNull, sync::Once};
// Link the bare-WASM allocator and libc forwarders used by the C++ archive.
#[cfg(target_arch = "wasm32")]
use basisu_wasm_libcxx as _;

extern "C" {
    // Named scalar accessors avoid a duplicated C/Rust structure layout.
    // Buffer lengths use fixed-width integers on both native and WASM targets.
    fn bfe_basis_init();
    fn bfe_basis_open(bytes: *const u8, size: u32) -> *mut c_void;
    fn bfe_basis_close(decoder: *mut c_void);
    fn bfe_basis_width(d: *const c_void) -> u32;
    fn bfe_basis_height(d: *const c_void) -> u32;
    fn bfe_basis_levels(d: *const c_void) -> u32;
    fn bfe_basis_faces(d: *const c_void) -> u32;
    fn bfe_basis_layers(d: *const c_void) -> u32;
    fn bfe_basis_alpha(d: *const c_void) -> u32;
    fn bfe_basis_premultiplied(d: *const c_void) -> u32;
    fn bfe_basis_uastc(d: *const c_void) -> u32;
    fn bfe_basis_size(format: u32, width: u32, height: u32) -> u32;
    fn bfe_basis_transcode(d: *mut c_void, level: u32, layer: u32, face: u32, format: u32, out: *mut u8, bytes: u32) -> u32;
}

#[derive(Clone, Copy, Debug)]
pub struct Info {
    // Zero layers denotes an ordinary 2D image; iteration treats it as one.
    // Faces remain explicit so cubemaps do not masquerade as array layers.
    pub width: u32, pub height: u32, pub levels: u32,
    pub faces: u32, pub layers: u32, pub alpha: bool, pub uastc: bool, pub premultiplied: bool,
}

pub struct Mip {
    // Ranges address the single payload allocation in Transcoded, in bytes.
    // They never refer back into a decoder's temporary Zstandard workspace.
    pub level: u32, pub layer: u32, pub face: u32,
    pub width: u32, pub height: u32, pub bytes: Range<usize>,
}

// All mip payloads share one allocation. Moving this result to an uploader
// cannot accidentally duplicate the source's full mip chain per sampler.
pub struct Transcoded { pub info: Info, pub mips: Vec<Mip>, pub data: Vec<u8> }

pub struct Decoder {
    handle: NonNull<c_void>,
    // Kept for the C++ object's borrowed source view, not for a second decode.
    _encoded: Box<[u8]>,
    info: Info,
}

impl Decoder {
    // Port of KTX2Loader's KTX2File initialization and startTranscoding.
    // Each worker has its own decoder instance; only immutable lookup-table
    // initialization is shared, protected by Rust's one-time initialization.
    pub fn new(encoded: Vec<u8>, max_size: u32) -> Result<Self, String> {
        static INIT: Once = Once::new();
        INIT.call_once(|| unsafe { bfe_basis_init() });
        if encoded.len()>64<<20{return Err("KTX2 encoded input exceeds 64 MiB".into());}
        let size = u32::try_from(encoded.len()).map_err(|_| "KTX2 input is too large")?;
        // The boxed allocation cannot move while the C++ decoder borrows it.
        // Construct the owner immediately so validation errors close it too.
        let encoded = encoded.into_boxed_slice();
        let handle = NonNull::new(unsafe { bfe_basis_open(encoded.as_ptr(), size) }).ok_or("Invalid Basis KTX2 input")?;
        let d = handle.as_ptr();
        let info = unsafe { Info {
            width: bfe_basis_width(d), height: bfe_basis_height(d), levels: bfe_basis_levels(d),
            faces: bfe_basis_faces(d), layers: bfe_basis_layers(d),
            alpha: bfe_basis_alpha(d) != 0, uastc: bfe_basis_uastc(d) != 0, premultiplied: bfe_basis_premultiplied(d) != 0,
        } };
        let decoder = Self { handle, _encoded: encoded, info };
        // Validate before allocating output, including containers whose tiny
        // encoded stream advertises a very large texture or excessive levels.
        if info.width == 0 || info.height == 0 || info.width > max_size || info.height > max_size || info.levels == 0 || info.levels > 32 || info.layers>256 || ![1, 6].contains(&info.faces) {
            return Err("KTX2 dimensions exceed texture limits".into());
        }
        Ok(decoder)
    }

    pub fn info(&self) -> Info { self.info }

    // Port of KTX2Loader.transcodeImage. Level-major traversal decompresses a
    // supercompressed level once for its layers and faces. Each upload keeps
    // explicit layer/face metadata, so traversal order does not change images.
    pub fn transcode(&mut self, format: u32) -> Result<Transcoded, String> {
        if ![0, 1, 2, 3, 6, 8, 9, 10, 13].contains(&format) { return Err("Unsupported Basis output format".into()); }
        let info = self.info;
        let mut mips = Vec::new();
        let mut total = 0usize;
        for level in 0..info.levels {
            let width = (info.width >> level).max(1);
            let height = (info.height >> level).max(1);
            let bytes = unsafe { bfe_basis_size(format, width, height) } as usize;
            if bytes == 0 { return Err("Invalid Basis output dimensions".into()); }
            // Bound the complete level before iterating attacker-controlled
            // layer metadata. A small compressed input cannot request a GiB
            // RGBA fallback or an unbounded vector in the 32-bit worker heap.
            let count=info.layers.max(1) as usize*info.faces as usize;
            let level_bytes=bytes.checked_mul(count).ok_or("KTX2 output size overflow")?;
            if total.checked_add(level_bytes).is_none_or(|n|n>128<<20){return Err("KTX2 output exceeds 128 MiB".into());}
            for layer in 0..info.layers.max(1) { for face in 0..info.faces {
                let end = total.checked_add(bytes).ok_or("KTX2 output size overflow")?;
                mips.push(Mip { level, layer, face, width, height, bytes: total..end });
                total = end;
            } }
        }
        // Allocate after all checked additions. There is no growing output
        // buffer whose address could change during an individual transcode.
        let mut data = vec![0; total];
        for mip in &mips {
            // The slice and capacity passed to C++ describe the same range.
            // Its block-count argument is derived there from this byte length.
            let out = &mut data[mip.bytes.clone()];
            if unsafe { bfe_basis_transcode(self.handle.as_ptr(), mip.level, mip.layer, mip.face, format, out.as_mut_ptr(), out.len() as u32) } == 0 {
                return Err(format!("Basis transcode failed: mip {} layer {} face {} format {format}", mip.level, mip.layer, mip.face));
            }
        }
        Ok(Transcoded { info, mips, data })
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // Closing before _encoded is freed is the lifetime invariant at the
        // FFI boundary. Rust field-drop order runs after this destructor body.
        unsafe { bfe_basis_close(self.handle.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    // Golden blocks were recorded from the original KTX2 loader over two
    // synthetic patterns. Hashes cover the entire ordered mip chain, including
    // terminal blocks; a correct base level alone cannot satisfy this check.
    #[test]
    fn all_formats_match_authored_fixture_blocks() {
        // Opaque and fractional-alpha inputs exercise separate format choices.
        // PVRTC terminal mips also check its minimum physical storage size.
        let expected: serde_json::Value = serde_json::from_str(include_str!("../../../tests/fixtures/renderer-textures/expected.json")).unwrap();
        for row in expected.as_array().unwrap() {
            let bytes: &[u8] = match row["file"].as_str().unwrap() {
                "alpha.ktx2" => include_bytes!("../../../tests/fixtures/renderer-textures/alpha.ktx2"),
                "opaque.ktx2" => include_bytes!("../../../tests/fixtures/renderer-textures/opaque.ktx2"),
                "opaque-etc1s.ktx2"=>include_bytes!("../../../tests/fixtures/renderer-textures/opaque-etc1s.ktx2"),
                "alpha-etc1s.ktx2"=>include_bytes!("../../../tests/fixtures/renderer-textures/alpha-etc1s.ktx2"),
                "alpha-odd.ktx2"=>include_bytes!("../../../tests/fixtures/renderer-textures/alpha-odd.ktx2"),
                _ => panic!("Unexpected fixture"),
            };
            let mut decoder = Decoder::new(bytes.to_vec(), 16).unwrap();
            let info = decoder.info();
            assert_eq!((info.width, info.height, info.levels, info.faces, info.layers), (row["width"].as_u64().unwrap() as u32,row["height"].as_u64().unwrap() as u32,row["levels"].as_u64().unwrap() as u32,1,0));
            assert_eq!(info.alpha, row["alpha"].as_bool().unwrap());
            assert_eq!(info.uastc,row["uastc"].as_bool().unwrap());
            for (name, format) in [("bc1", 2), ("bc3", 3), ("bc7", 6), ("astc", 10), ("etc1", 0), ("etc2", 1), ("rgba", 13), ("pvrtc-rgb", 8), ("pvrtc-rgba", 9)] {
                if row["formats"][name].is_null(){continue;}
                let image = decoder.transcode(format).unwrap();
                assert_eq!(image.mips.len(),info.levels as usize);
                let golden = &row["formats"][name];
                assert_eq!(image.data.len() as u64, golden["bytes"].as_u64().unwrap(), "{name}");
                assert_eq!(format!("{:x}", Sha256::digest(&image.data)), golden["hash"].as_str().unwrap(), "{name}");
            }
            // Rejecting a bad output request must not consume the decoder.
            // The next valid request still reads its original borrowed source.
            assert!(decoder.transcode(999).is_err());
            assert!(!decoder.transcode(13).unwrap().data.is_empty());
            assert!(Decoder::new(bytes.to_vec(), 8).is_err());
        }
    }

    // The output budget is checked before any level/face allocation or C++
    // transcode call. Mutating only metadata isolates that boundary from file
    // validation and cannot accidentally allocate the claimed giant image.
    #[test]
    fn expanded_output_is_bounded_before_allocation(){
        let bytes=include_bytes!("../../../tests/fixtures/renderer-textures/alpha.ktx2");
        let mut decoder=Decoder::new(bytes.to_vec(),16384).unwrap();
        decoder.info.width=16384;decoder.info.height=16384;decoder.info.levels=15;
        assert!(decoder.transcode(13).err().unwrap().contains("128 MiB"));
    }

    #[test]
    fn malformed_inputs_do_not_produce_an_upload() {
        // Failed initialization owns no handle and exposes no partial payload.
        assert!(Decoder::new(Vec::new(), 1024).is_err());
        let bytes = include_bytes!("../../../tests/fixtures/renderer-textures/alpha.ktx2");
        assert!(Decoder::new(bytes[..40].to_vec(), 1024).is_err());
    }
}
