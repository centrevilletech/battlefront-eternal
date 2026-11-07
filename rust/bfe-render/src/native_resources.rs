//! Native GPU image and target lifetime. The renderer frame schedule borrows
//! this owner directly; the client retains no parallel resource bank.
use crate::resource_policy::*;
use glow::HasContext;
use std::collections::HashMap;
fn parse<T: for<'a> serde::Deserialize<'a>>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

enum Pixels {
    Empty,
    Raw(Vec<u8>),
    Png(Vec<u8>),
    Blocks(Vec<u8>, Vec<Mip>),
}
struct Source {
    desc: ImageDesc,
    pixels: Pixels,
    version: u32,
    host_released: bool,
}
struct Physical {
    source: usize,
    sampling: Sampling,
    texture: Option<glow::Texture>,
    seen: u32,
    allocation: Option<(i32, i32, i32, u32, bool)>,
    levels: u32,
    users: usize,
    anisotropy_set: bool,
}
struct Sample {
    physical: usize,
    alive: bool,
    owned: bool,
}
struct Target {
    desc: TargetDesc,
    faces: Vec<glow::Framebuffer>,
    msaa: Option<glow::Framebuffer>,
    buffers: Vec<glow::Renderbuffer>,
}

/// The creating GL context stays current on this thread through cleanup.
/// Logical ids belong to this bank; retired physical rows carry no GL name.
pub struct GpuResources {
    gl: glow::Context,
    sources: Vec<Option<Source>>,
    physical: Vec<Physical>,
    free_physical: Vec<usize>,
    samples: Vec<Sample>,
    targets: Vec<Option<Target>>,
    cache: HashMap<(u32, String), usize>,
    current: Option<usize>,
    face: usize,
    level: i32,
    viewport: [i32; 4],
    scissor: [i32; 4],
    clipped: bool,
    epoch:u32,
    world_fallbacks: [Option<u32>; 2],
    pub canvas: Option<glow::Framebuffer>,
}
impl GpuResources {
    pub fn new(gl: glow::Context) -> Self {
        Self {
            gl,
            sources: Vec::new(),
            physical: Vec::new(),
            free_physical: Vec::new(),
            samples: Vec::new(),
            targets: Vec::new(),
            cache: HashMap::new(),
            current: None,
            face: 0,
            level: 0,
            viewport: [0; 4],
            scissor: [0; 4],
            clipped: false,epoch:0,
            canvas: None,
            world_fallbacks: [None; 2],
        }
    }
    pub fn epoch(&self)->u32{self.epoch}
    pub fn source(&mut self, descriptor: &str) -> Result<u32, String> {
        let desc: ImageDesc = parse(descriptor)?;
        self.validate(&desc)?;
        let id = self.sources.len();
        self.sources.push(Some(Source {
            desc,
            pixels: Pixels::Empty,
            version: 1,
            host_released: false,
        }));
        Ok(id as u32)
    }
    fn validate(&self, d: &ImageDesc) -> Result<(), String> {
        let max = unsafe { self.gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE) };
        if d.width < 1 || d.height < 1 || d.depth < 1 || d.width > max || d.height > max {
            return Err("invalid texture dimensions".into());
        }
        if ![
            glow::TEXTURE_2D,
            glow::TEXTURE_3D,
            glow::TEXTURE_CUBE_MAP,
            glow::TEXTURE_2D_ARRAY,
        ]
        .contains(&d.target)
        {
            return Err("unsupported texture target".into());
        }
        format(d)?;
        Ok(())
    }
    pub fn source_desc(&mut self, id: u32, descriptor: &str) -> Result<(), String> {
        self.epoch=self.epoch.wrapping_add(1);
        let desc = parse(descriptor)?;
        self.validate(&desc)?;
        let s = self.source_mut(id)?;
        s.desc = desc;
        s.version = s.version.wrapping_add(1);
        Ok(())
    }
    pub fn source_pixels(&mut self, id: u32, bytes: &[u8]) -> Result<(), String> {
        self.epoch=self.epoch.wrapping_add(1);
        let s = self.source_mut(id)?;
        match &mut s.pixels {
            Pixels::Raw(data) => {
                data.clear();
                data.extend_from_slice(bytes);
            }
            _ => s.pixels = Pixels::Raw(bytes.to_vec()),
        }
        s.version = s.version.wrapping_add(1);
        Ok(())
    }
    pub fn source_png(&mut self, id: u32, encoded: &[u8]) -> Result<(), String> {
        self.epoch=self.epoch.wrapping_add(1);
        let s = self.source_mut(id)?;
        s.pixels = Pixels::Png(encoded.to_vec());
        s.version = s.version.wrapping_add(1);
        Ok(())
    }
    pub fn source_blocks(&mut self, id: u32, data: &[u8], metadata: &str) -> Result<(), String> {
        self.epoch=self.epoch.wrapping_add(1);
        let mips: Vec<Mip> = parse(metadata)?;
        if mips.is_empty()
            || mips.iter().any(|m| {
                m.width < 1
                    || m.height < 1
                    || m.offset
                        .checked_add(m.length)
                        .is_none_or(|n| n > data.len())
            })
        {
            return Err("invalid mip extent".into());
        }
        let s = self.source_mut(id)?;
        s.pixels = Pixels::Blocks(data.to_vec(), mips);
        s.version = s.version.wrapping_add(1);
        Ok(())
    }
    pub fn sample(&mut self, source: u32, settings: &str) -> Result<u32, String> {
        let physical = self.physical(source, settings)?;
        self.physical[physical].users += 1;
        let id = self.samples.len();
        self.samples.push(Sample {
            physical,
            alive: true,
            owned: false,
        });
        Ok(id as u32)
    }
    fn physical(&mut self, source: u32, settings: &str) -> Result<usize, String> {
        self.source_ref(source)?;
        let key = (source, settings.to_string());
        if let Some(&id) = self.cache.get(&key) {
            return Ok(id);
        }
        let sampling = parse(settings)?;
        // SAFETY: resource admission runs on the owner's current context;
        // the returned name remains owned here until retirement or Drop.
        let texture = unsafe { self.gl.create_texture()? };
        // Logical sampler ids never recycle: a stale draw cannot acquire a
        // new image by aliasing an old id. Only unreferenced physical rows do.
        let row = Physical {
            source: source as usize,
            sampling,
            texture: Some(texture),
            seen: 0,
            allocation: None,
            levels: 0,
            users: 0,
            anisotropy_set: false,
        };
        let id = if let Some(id) = self.free_physical.pop() {
            self.physical[id] = row;
            id
        } else {
            let id = self.physical.len();
            self.free_physical
                .reserve(id + 1 - self.free_physical.len());
            self.physical.push(row);
            id
        };
        self.cache.insert(key, id);
        Ok(id)
    }
    pub fn sampling(&mut self, id: u32, settings: &str) -> Result<(), String> {
        self.epoch=self.epoch.wrapping_add(1);
        let old = self.sample_ref(id)?.physical;
        let source = self.physical[old].source as u32;
        let next: Sampling = parse(settings)?;
        let key = (source, settings.to_string());
        let render_target = self.source_ref(source)?.desc.render_target;
        // Render target attachments and sole filtering revisions retain their
        // GL name. The browser does the same; changing this invalidates pass
        // framebuffers and registered texture ids without a transport event.
        if render_target || self.physical[old].users == 1 && !self.cache.contains_key(&key) {
            if !render_target {
                self.cache.retain(|_, id| *id != old);
                self.cache.insert(key, old);
            }
            let p = &mut self.physical[old];
            let changed = p.sampling.flip != next.flip
                || p.sampling.premultiply != next.premultiply
                || p.sampling.mipmaps != next.mipmaps;
            p.sampling = next;
            let d = &self.sources[source as usize]
                .as_ref()
                .ok_or("disposed source")?
                .desc;
            unsafe {
                self.gl.bind_texture(d.target, p.texture);
                self.gl
                    .tex_parameter_i32(d.target, glow::TEXTURE_WRAP_S, wrap(p.sampling.wrap_s));
                self.gl
                    .tex_parameter_i32(d.target, glow::TEXTURE_WRAP_T, wrap(p.sampling.wrap_t));
                self.gl.tex_parameter_i32(
                    d.target,
                    glow::TEXTURE_MIN_FILTER,
                    filter(p.sampling.min),
                );
                self.gl.tex_parameter_i32(
                    d.target,
                    glow::TEXTURE_MAG_FILTER,
                    filter(p.sampling.mag),
                );
                if render_target {
                    let level = if p.sampling.mipmaps {
                        (d.width.max(d.height) as u32).ilog2()
                    } else {
                        0
                    };
                    self.gl
                        .tex_parameter_i32(d.target, glow::TEXTURE_MAX_LEVEL, level as i32);
                    p.levels = level as u32 + 1;
                }
            }
            if changed && !render_target {
                p.seen = 0;
            }
            return Ok(());
        }
        let next = self.physical(source, settings)?;
        if old != next {
            self.physical[next].users += 1;
            self.samples[id as usize].physical = next;
            self.release_physical(old)?;
        }
        Ok(())
    }
    // Browser add_texture/replace_sample_texture use this same logical
    // descriptor boundary. Both hosts resolve it through their existing bank.
    pub fn descriptor_texture(&self,id:u32)->Result<glow::Texture,String>{self.texture(id)}
    pub fn texture(&self, id: u32) -> Result<glow::Texture, String> {
        self.physical[self.sample_ref(id)?.physical]
            .texture
            .ok_or("disposed physical image".into())
    }
    fn sample_ref(&self, id: u32) -> Result<&Sample, String> {
        self.samples
            .get(id as usize)
            .filter(|s| s.alive)
            .ok_or_else(|| "unknown sampler".into())
    }
    fn source_ref(&self, id: u32) -> Result<&Source, String> {
        self.sources
            .get(id as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| "unknown image source".into())
    }
    fn source_mut(&mut self, id: u32) -> Result<&mut Source, String> {
        self.sources
            .get_mut(id as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| "unknown image source".into())
    }
    // Static descriptors pin their samplers after the loader's weak source
    // cache releases its handle. Match GpuResources::retain_sample ownership.
    pub fn retain_sample(&mut self, id: u32) -> Result<u32, String> {
        let physical = self.sample_ref(id)?.physical;
        self.physical[physical].users += 1;
        let id = self.samples.len() as u32;
        self.samples.push(Sample {
            physical,
            alive: true,
            owned: true,
        });
        Ok(id)
    }
    pub fn release_source(&mut self, id: u32) {
        let Some(source) = self.sources.get_mut(id as usize).and_then(Option::as_mut) else {
            return;
        };
        source.host_released = true;
        for i in 0..self.samples.len() {
            let s = &self.samples[i];
            if s.alive && !s.owned && self.physical[s.physical].source == id as usize {
                let _ = self.dispose_sample(i as u32);
            }
        }
        self.retire_source(id as usize);
    }
    fn retire_source(&mut self, id: usize) {
        if self
            .sources
            .get(id)
            .and_then(Option::as_ref)
            .is_some_and(|s| s.host_released)
            && !self
                .samples
                .iter()
                .any(|s| s.alive && s.owned && self.physical[s.physical].source == id)
        {
            self.sources[id] = None;
        }
    }
    // Texture.dispose and the browser's release_physical retire the last
    // reference, including cache identity. Repeated disposal remains harmless;
    // a bad reference count is a reported invariant failure, never saturation.
    fn release_physical(&mut self, id: usize) -> Result<(), String> {
        let p = self.physical.get_mut(id).ok_or("unknown physical image")?;
        p.users = p.users.checked_sub(1).ok_or("image reference underflow")?;
        if p.users == 0 {
            if let Some(texture) = p.texture.take() {
                // SAFETY: this owner created the live GL name on the current
                // context; taking it prevents a second delete during Drop.
                unsafe {
                    self.gl.delete_texture(texture);
                }
            }
            self.cache.retain(|_, row| *row != id);
            self.free_physical.push(id);
        }
        Ok(())
    }
    pub fn dispose_sample(&mut self, id: u32) -> Result<(), String> {
        self.epoch = self.epoch.wrapping_add(1);
        let s = self.samples.get_mut(id as usize).ok_or("unknown sampler")?;
        if s.alive {
            s.alive = false;
            let physical = s.physical;
            let source = self.physical[physical].source;
            self.release_physical(physical)?;
            self.retire_source(source);
        }
        Ok(())
    }
    // world_fallback and canopy_state use the browser owner's exact image
    // descriptors. Only their byte source changes from a JS view to a slice.
    pub fn world_fallback(&mut self, neutral: bool) -> Result<u32, String> {
        let slot = usize::from(neutral);
        if let Some(id) = self.world_fallbacks[slot] {
            return Ok(id);
        }
        let pixels = if neutral {
            [128, 128, 128, 255]
        } else {
            [0; 4]
        };
        let id = self.state_image(1, 1009, &pixels)?;
        self.world_fallbacks[slot] = Some(id);
        Ok(id)
    }
    fn state_image(&mut self, width: u32, data_type: u32, pixels: &[u8]) -> Result<u32, String> {
        let source = self.source(&serde_json::json!({"width":width,"height":1,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":data_type,"srgb":false}).to_string())?;
        self.source_pixels(source, pixels)?;
        let sample = self.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        self.upload(sample)?;
        Ok(sample)
    }
    pub fn canopy_state(&mut self, width: u32, pixels: &[u8]) -> Result<u32, String> {
        if width == 0 || width > 65536 || pixels.len() != width as usize * 16 {
            return Err("invalid canopy state capacity".into());
        }
        self.state_image(width, 1015, pixels)
    }
    pub fn update_canopy_state(&mut self, id: u32, pixels: &[u8]) -> Result<(), String> {
        let source = self.physical[self.sample_ref(id)?.physical].source as u32;
        if pixels.len() != self.source_ref(source)?.desc.width as usize * 16 {
            return Err("invalid canopy state update".into());
        }
        self.source_pixels(source, pixels)?;
        self.upload(id)
    }
    pub fn upload(&mut self, id: u32) -> Result<(), String> {
        let index = self.sample_ref(id)?.physical;
        let p = &mut self.physical[index];
        let s = self
            .sources
            .get_mut(p.source)
            .and_then(Option::as_mut)
            .ok_or("disposed source")?;
        if p.seen == s.version {
            return Ok(());
        }
        let d = &s.desc;
        let sampling = &p.sampling;
        let gl = &self.gl;
        let (base, ty, internal) = format(d)?;
        let immutable = !d.render_target || [1026, 1027].contains(&d.format);
        let allocation = (d.width, d.height, d.depth, internal, immutable);
        let allocate = p.allocation != Some(allocation);
        let levels = match &s.pixels {
            Pixels::Blocks(_, mips) => mips.len() as u32,
            _ if sampling.mipmaps => (d.width.max(d.height).max(d.depth) as u32).ilog2() + 1,
            _ => 1,
        };
        unsafe {
            if allocate && p.allocation.is_some_and(|old| immutable || old.4) {
                if let Some(texture) = p.texture.take() {
                    gl.delete_texture(texture);
                }
                p.texture = Some(gl.create_texture()?);
            }
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(d.target, p.texture);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, sampling.alignment);
            gl.tex_parameter_i32(d.target, glow::TEXTURE_WRAP_S, wrap(sampling.wrap_s));
            gl.tex_parameter_i32(d.target, glow::TEXTURE_WRAP_T, wrap(sampling.wrap_t));
            if matches!(d.target, glow::TEXTURE_3D | glow::TEXTURE_2D_ARRAY) {
                gl.tex_parameter_i32(d.target, glow::TEXTURE_WRAP_R, wrap(sampling.wrap_r));
            }
            gl.tex_parameter_i32(d.target, glow::TEXTURE_MIN_FILTER, filter(sampling.min));
            gl.tex_parameter_i32(d.target, glow::TEXTURE_MAG_FILTER, filter(sampling.mag));
            let aniso = gl
                .supported_extensions()
                .iter()
                .any(|s| s.ends_with("texture_filter_anisotropic"));
            if aniso
                && sampling.mag != 1003
                && [1005, 1008].contains(&sampling.min)
                && (sampling.anisotropy > 1.0 || p.anisotropy_set)
            {
                let max = gl.get_parameter_f32(0x84ff);
                gl.tex_parameter_f32(d.target, 0x84fe, sampling.anisotropy.max(1.0).min(max));
                p.anisotropy_set = true;
            }
            if sampling.compare.is_some() || [1026, 1027].contains(&d.format) {
                gl.tex_parameter_i32(
                    d.target,
                    glow::TEXTURE_COMPARE_MODE,
                    if sampling.compare.is_some() {
                        glow::COMPARE_REF_TO_TEXTURE
                    } else {
                        glow::NONE
                    } as i32,
                );
            }
            if let Some(compare) = sampling.compare {
                gl.tex_parameter_i32(d.target, glow::TEXTURE_COMPARE_FUNC, compare as i32);
            }
            if allocate && immutable {
                if matches!(d.target, glow::TEXTURE_3D | glow::TEXTURE_2D_ARRAY) {
                    gl.tex_storage_3d(
                        d.target,
                        levels as i32,
                        internal,
                        d.width,
                        d.height,
                        d.depth,
                    );
                } else {
                    gl.tex_storage_2d(d.target, levels as i32, internal, d.width, d.height);
                }
            }
            let mut decoded;
            let mut converted;
            let pixels = match &s.pixels {
                Pixels::Raw(bytes) => Some(bytes.as_slice()),
                Pixels::Png(encoded) => {
                    decoded =
                        crate::texture_image::decode_image(encoded, d.width.max(d.height) as u32)?
                            .rgba;
                    if sampling.premultiply {
                        for px in decoded.chunks_exact_mut(4) {
                            let alpha = px[3] as u32;
                            for c in &mut px[..3] {
                                *c = ((*c as u32 * alpha + 127) / 255) as u8;
                            }
                        }
                    }
                    Some(decoded.as_slice())
                }
                _ => None,
            };
            let pixels = if let Some(raw) = pixels {
                let components = match base {
                    glow::RGBA => 4,
                    glow::RGB => 3,
                    glow::RG => 2,
                    _ => 1,
                };
                let scalar = match ty {
                    glow::FLOAT | glow::UNSIGNED_INT | glow::INT | glow::UNSIGNED_INT_24_8 => 4,
                    glow::UNSIGNED_SHORT | glow::SHORT | glow::HALF_FLOAT => 2,
                    glow::UNSIGNED_SHORT_4_4_4_4
                    | glow::UNSIGNED_SHORT_5_5_5_1
                    | glow::UNSIGNED_SHORT_5_6_5 => 2,
                    _ => 1,
                };
                let packed = matches!(
                    ty,
                    glow::UNSIGNED_SHORT_4_4_4_4
                        | glow::UNSIGNED_SHORT_5_5_5_1
                        | glow::UNSIGNED_SHORT_5_6_5
                        | glow::UNSIGNED_INT_24_8
                );
                let row = d.width as usize * scalar * if packed { 1 } else { components };
                let pitch = row.div_ceil(sampling.alignment.max(1) as usize)
                    * sampling.alignment.max(1) as usize;
                let required = pitch
                    .checked_mul(d.height.saturating_sub(1) as usize)
                    .and_then(|n| n.checked_add(row))
                    .ok_or("pixel extent overflow")?;
                if raw.len() < required {
                    return Err(format!(
                        "pixel data has {} bytes, needs {required}",
                        raw.len()
                    ));
                }
                if sampling.flip
                    || sampling.premultiply
                        && matches!(s.pixels, Pixels::Raw(_))
                        && base == glow::RGBA
                        && ty == glow::UNSIGNED_BYTE
                {
                    converted = raw.to_vec();
                    if sampling.flip {
                        for y in 0..d.height as usize {
                            converted[y * pitch..y * pitch + row].copy_from_slice(
                                &raw[(d.height as usize - 1 - y) * pitch
                                    ..(d.height as usize - 1 - y) * pitch + row],
                            );
                        }
                    }
                    if sampling.premultiply
                        && matches!(s.pixels, Pixels::Raw(_))
                        && base == glow::RGBA
                        && ty == glow::UNSIGNED_BYTE
                    {
                        for y in 0..d.height as usize {
                            for px in converted[y * pitch..y * pitch + row].chunks_exact_mut(4) {
                                let a = px[3] as u32;
                                for c in &mut px[..3] {
                                    *c = ((*c as u32 * a + 127) / 255) as u8;
                                }
                            }
                        }
                    }
                    Some(converted.as_slice())
                } else {
                    Some(raw)
                }
            } else {
                None
            };
            if let Pixels::Blocks(bytes, mips) = &s.pixels {
                for (level, mip) in mips.iter().enumerate() {
                    let bytes = &bytes[mip.offset..mip.offset + mip.length];
                    if d.format > 30000 {
                        gl.compressed_tex_sub_image_2d(
                            d.target,
                            level as i32,
                            0,
                            0,
                            mip.width,
                            mip.height,
                            internal,
                            glow::CompressedPixelUnpackData::Slice(bytes),
                        );
                    } else {
                        gl.tex_sub_image_2d(
                            d.target,
                            level as i32,
                            0,
                            0,
                            mip.width,
                            mip.height,
                            base,
                            ty,
                            glow::PixelUnpackData::Slice(Some(bytes)),
                        );
                    }
                }
                gl.tex_parameter_i32(
                    d.target,
                    glow::TEXTURE_MAX_LEVEL,
                    mips.len().saturating_sub(1) as i32,
                );
            } else if matches!(d.target, glow::TEXTURE_3D | glow::TEXTURE_2D_ARRAY) {
                if allocate && !immutable {
                    gl.tex_image_3d(
                        d.target,
                        0,
                        internal as i32,
                        d.width,
                        d.height,
                        d.depth,
                        0,
                        base,
                        ty,
                        glow::PixelUnpackData::Slice(pixels),
                    );
                } else if pixels.is_some() {
                    gl.tex_sub_image_3d(
                        d.target,
                        0,
                        0,
                        0,
                        0,
                        d.width,
                        d.height,
                        d.depth,
                        base,
                        ty,
                        glow::PixelUnpackData::Slice(pixels),
                    );
                }
            } else {
                for face in 0..if d.target == glow::TEXTURE_CUBE_MAP {
                    6
                } else {
                    1
                } {
                    let target = if d.target == glow::TEXTURE_CUBE_MAP {
                        glow::TEXTURE_CUBE_MAP_POSITIVE_X + face
                    } else {
                        d.target
                    };
                    if allocate && !immutable {
                        gl.tex_image_2d(
                            target,
                            0,
                            internal as i32,
                            d.width,
                            d.height,
                            0,
                            base,
                            ty,
                            glow::PixelUnpackData::Slice(pixels),
                        );
                    } else if pixels.is_some() {
                        gl.tex_sub_image_2d(
                            target,
                            0,
                            0,
                            0,
                            d.width,
                            d.height,
                            base,
                            ty,
                            glow::PixelUnpackData::Slice(pixels),
                        );
                    }
                }
            }
            if !sampling.mipmaps && !matches!(s.pixels, Pixels::Blocks(..)) {
                gl.tex_parameter_i32(d.target, glow::TEXTURE_MAX_LEVEL, 0);
            }
            if sampling.mipmaps && ![1026, 1027].contains(&d.format) {
                gl.generate_mipmap(d.target);
            }
        }
        p.seen = s.version;
        p.allocation = Some(allocation);
        p.levels = levels;
        Ok(())
    }
    pub fn target(&mut self, descriptor: &str) -> Result<u32, String> {
        let id = self.targets.len();
        self.targets.push(None);
        self.make_target(id, parse(descriptor)?)?;
        Ok(id as u32)
    }
    pub fn resize_target(&mut self, id: u32, descriptor: &str) -> Result<(), String> {
        let current = self.current;
        self.dispose_target(id);
        self.make_target(id as usize, parse(descriptor)?)?;
        self.current = current;
        self.bind_current()
    }
    fn target_ref(&self, id: u32) -> Result<&Target, String> {
        self.targets
            .get(id as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| "unknown render target".into())
    }
    fn make_target(&mut self, id: usize, mut desc: TargetDesc) -> Result<(), String> {
        if desc.width < 1 || desc.height < 1 {
            return Err("invalid framebuffer size".into());
        }
        if desc.cube {
            desc.samples = 0;
        }
        desc.samples = desc
            .samples
            .clamp(0, unsafe { self.gl.get_parameter_i32(glow::MAX_SAMPLES) });
        self.upload(desc.color)?;
        if let Some(depth) = desc.depth {
            self.upload(depth)?;
        }
        let color = self.texture(desc.color)?;
        let source =
            self.source_ref(self.physical[self.sample_ref(desc.color)?.physical].source as u32)?;
        let (_, _, color_format) = format(&source.desc)?;
        let depth = desc.depth.map(|id| self.texture(id)).transpose()?;
        let depth_type = desc
            .depth
            .map(|id| {
                self.source_ref(self.physical[self.sample_ref(id)?.physical].source as u32)
                    .map(|s| s.desc.data_type)
            })
            .transpose()?;
        let depth_format = depth_format(desc.stencil, depth_type);
        let mut target = Target {
            desc: desc.clone(),
            faces: Vec::new(),
            msaa: None,
            buffers: Vec::new(),
        };
        unsafe {
            for face in 0..if desc.cube { 6 } else { 1 } {
                let fb = self.gl.create_framebuffer()?;
                target.faces.push(fb);
                self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
                self.gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    if desc.cube {
                        glow::TEXTURE_CUBE_MAP_POSITIVE_X + face
                    } else {
                        glow::TEXTURE_2D
                    },
                    Some(color),
                    0,
                );
                if let Some(depth) = depth {
                    self.gl.framebuffer_texture_2d(
                        glow::FRAMEBUFFER,
                        if desc.stencil {
                            glow::DEPTH_STENCIL_ATTACHMENT
                        } else {
                            glow::DEPTH_ATTACHMENT
                        },
                        glow::TEXTURE_2D,
                        Some(depth),
                        0,
                    );
                } else if desc.depth_buffer {
                    let rb = self.gl.create_renderbuffer()?;
                    target.buffers.push(rb);
                    self.gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rb));
                    self.gl.renderbuffer_storage(
                        glow::RENDERBUFFER,
                        depth_format,
                        desc.width,
                        desc.height,
                    );
                    self.gl.framebuffer_renderbuffer(
                        glow::FRAMEBUFFER,
                        if desc.stencil {
                            glow::DEPTH_STENCIL_ATTACHMENT
                        } else {
                            glow::DEPTH_ATTACHMENT
                        },
                        glow::RENDERBUFFER,
                        Some(rb),
                    );
                }
                if self.gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
                {
                    return Err("incomplete native render target".into());
                }
            }
            if desc.samples > 0 {
                let fb = self.gl.create_framebuffer()?;
                target.msaa = Some(fb);
                self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
                for (attachment, format) in [
                    (glow::COLOR_ATTACHMENT0, color_format),
                    (
                        if desc.stencil {
                            glow::DEPTH_STENCIL_ATTACHMENT
                        } else {
                            glow::DEPTH_ATTACHMENT
                        },
                        depth_format,
                    ),
                ] {
                    if attachment != glow::COLOR_ATTACHMENT0 && !desc.depth_buffer {
                        continue;
                    }
                    let rb = self.gl.create_renderbuffer()?;
                    target.buffers.push(rb);
                    self.gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rb));
                    self.gl.renderbuffer_storage_multisample(
                        glow::RENDERBUFFER,
                        desc.samples,
                        format,
                        desc.width,
                        desc.height,
                    );
                    self.gl.framebuffer_renderbuffer(
                        glow::FRAMEBUFFER,
                        attachment,
                        glow::RENDERBUFFER,
                        Some(rb),
                    );
                }
                if self.gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
                {
                    return Err("incomplete native multisample target".into());
                }
            }
        }
        *self.targets.get_mut(id).ok_or("invalid target id")? = Some(target);
        self.bind_current()
    }
    pub fn dispose_target(&mut self, id: u32) {
        if let Some(target) = self.targets.get_mut(id as usize).and_then(Option::take) {
            unsafe {
                for fb in target.faces {
                    self.gl.delete_framebuffer(fb);
                }
                if let Some(fb) = target.msaa {
                    self.gl.delete_framebuffer(fb);
                }
                for rb in target.buffers {
                    self.gl.delete_renderbuffer(rb);
                }
            }
        }
        if self.current == Some(id as usize) {
            self.current = None;
        }
    }
    fn bind_current(&self) -> Result<(), String> {
        unsafe {
            if let Some(id) = self.current {
                let t = self.target_ref(id as u32)?;
                let fb = t
                    .msaa
                    .or_else(|| t.faces.get(self.face).copied())
                    .ok_or("invalid target face")?;
                self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
                if t.desc.cube {
                    self.gl.framebuffer_texture_2d(
                        glow::FRAMEBUFFER,
                        glow::COLOR_ATTACHMENT0,
                        if t.desc.cube {
                            glow::TEXTURE_CUBE_MAP_POSITIVE_X + self.face as u32
                        } else {
                            glow::TEXTURE_2D
                        },
                        Some(self.texture(t.desc.color)?),
                        self.level,
                    );
                }
            } else {
                self.gl.bind_framebuffer(glow::FRAMEBUFFER, self.canvas);
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bind(
        &mut self,
        id: i32,
        face: u32,
        level: i32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        clipped: bool,
    ) -> Result<(), String> {
        self.current = if id < 0 { None } else { Some(id as usize) };
        self.face = face as usize;
        self.level = level;
        self.viewport = [x, y, w, h];
        self.scissor = [sx, sy, sw, sh];
        self.clipped = clipped;
        self.bind_current()?;
        unsafe {
            self.gl.viewport(x, y, w, h);
            self.gl.scissor(sx, sy, sw, sh);
            if clipped {
                self.gl.enable(glow::SCISSOR_TEST);
            } else {
                self.gl.disable(glow::SCISSOR_TEST);
            }
        }
        Ok(())
    }
    pub fn framebuffer(
        &self,
        id: u32,
        multisample: bool,
        face: usize,
    ) -> Result<glow::Framebuffer, String> {
        let t = self.target_ref(id)?;
        if multisample {
            if let Some(fb) = t.msaa {
                return Ok(fb);
            }
        }
        t.faces
            .get(face)
            .copied()
            .ok_or("unknown framebuffer face".into())
    }
    pub fn resolve(&mut self, id: u32) -> Result<(), String> {
        let t = self.target_ref(id)?;
        if let Some(msaa) = t.msaa {
            let d = &t.desc;
            let mut mask = glow::COLOR_BUFFER_BIT;
            if d.resolve_depth && d.depth_buffer {
                mask |= glow::DEPTH_BUFFER_BIT;
            }
            if d.resolve_depth && d.resolve_stencil && d.stencil {
                mask |= glow::STENCIL_BUFFER_BIT;
            }
            unsafe {
                self.gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(msaa));
                self.gl
                    .bind_framebuffer(glow::DRAW_FRAMEBUFFER, t.faces.first().copied());
                self.gl.blit_framebuffer(
                    0,
                    0,
                    d.width,
                    d.height,
                    0,
                    0,
                    d.width,
                    d.height,
                    mask,
                    glow::NEAREST,
                );
            }
        }
        self.bind_current()
    }
    pub fn copy(&mut self, source: u32, dest: u32, mask: u32) -> Result<(), String> {
        let a = self.target_ref(source)?;
        let b = self.target_ref(dest)?;
        unsafe {
            self.gl.bind_framebuffer(
                glow::READ_FRAMEBUFFER,
                a.msaa.or_else(|| a.faces.first().copied()),
            );
            self.gl
                .bind_framebuffer(glow::DRAW_FRAMEBUFFER, b.faces.first().copied());
            self.gl.blit_framebuffer(
                0,
                0,
                a.desc.width,
                a.desc.height,
                0,
                0,
                b.desc.width,
                b.desc.height,
                mask,
                glow::NEAREST,
            );
        }
        self.bind_current()
    }
    pub fn mipmap(&mut self, id: u32) -> Result<(), String> {
        let p = &self.physical[self.sample_ref(id)?.physical];
        let source = self.source_ref(p.source as u32)?;
        unsafe {
            self.gl.bind_texture(source.desc.target, p.texture);
            self.gl.tex_parameter_i32(
                source.desc.target,
                glow::TEXTURE_MAX_LEVEL,
                (source.desc.width.max(source.desc.height) as u32).ilog2() as i32,
            );
            self.gl.generate_mipmap(source.desc.target);
        }
        Ok(())
    }
}
impl Drop for GpuResources {
    fn drop(&mut self) {
        unsafe {
            for p in &self.physical {
                if let Some(texture) = p.texture {
                    self.gl.delete_texture(texture);
                }
            }
        }
        for id in 0..self.targets.len() {
            self.dispose_target(id as u32);
        }
    }
}

// The frame bank borrows the same target owner used at admission. Binding
// coordinates follow Graphics.setRenderTarget; negative ids name the canvas.
// Neither a view publication nor battle_frame creates a second target bank.
impl crate::frame::FrameTargets for GpuResources {
    fn bind(&mut self, row: &[f64]) -> Result<(), String> {
        use crate::frame_plan::layout::binding as b;
        if row.len() != b::LEN {
            return Err("invalid frame target binding length".into());
        }
        self.bind(
            row[b::TARGET] as i32,
            (row[b::FACE] as u32).min(5),
            row[b::LEVEL].max(0.).min(32.) as i32,
            row[b::X] as i32,
            row[b::Y] as i32,
            row[b::WIDTH].max(0.).min(32768.) as i32,
            row[b::HEIGHT].max(0.).min(32768.) as i32,
            row[b::SCISSOR] as i32,
            row[b::SCISSOR + 1] as i32,
            row[b::SCISSOR + 2].max(0.).min(32768.) as i32,
            row[b::SCISSOR + 3].max(0.).min(32768.) as i32,
            row[b::CLIPPED] != 0.,
        )
    }
    fn resolve(&mut self, id: i32) -> Result<(), String> {
        if id < 0 {
            Ok(())
        } else {
            self.resolve(id as u32)
        }
    }
    fn copy(&mut self, from: i32, to: i32, mask: u32) -> Result<(), String> {
        if from < 0 || to < 0 {
            Ok(())
        } else {
            self.copy(from as u32, to as u32, mask)
        }
    }
    fn mipmap(&mut self, sample: i32) -> Result<(), String> {
        if sample < 0 {
            Ok(())
        } else {
            self.mipmap(sample as u32)
        }
    }
}

#[cfg(test)]
#[path = "native_resources_tests.rs"]
mod tests;
