//! Validate the near-capture protocol before registration reaches a GL owner.
//! Counts describe allocations in call order, so a reference to a later object
//! is rejected just like a missing object. No driver/context is needed here.
use bfe_render::instances as I;
use serde_json::Value;
#[derive(Default)]
struct Geometry {
    buffers: Vec<usize>,
    indices: usize,
    groups: usize,
}
struct Capture<'a> {
    meta: &'a Value,
    bytes: &'a [u8],
}
impl Capture<'_> {
    fn error(&self, message: impl std::fmt::Display) -> String {
        format!("invalid renderer capture: {message}")
    }
    fn integer(&self, v: &Value, label: &str) -> Result<usize, String> {
        let n = v
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= u32::MAX as f64)
            .ok_or_else(|| self.error(format!("{label} must be an unsigned integer")))?;
        Ok(n as usize)
    }
    fn id(&self, v: &Value, count: usize, label: &str) -> Result<usize, String> {
        let id = self.integer(v, label)?;
        if id >= count {
            return Err(self.error(format!("{label} {id} is not registered (count {count})")));
        }
        Ok(id)
    }
    fn blob(&self, v: &Value, width: usize) -> Result<&[u8], String> {
        let id = v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| self.error("blob id must be an unsigned JSON integer"))?;
        let span = self.meta["blobs"]
            .as_array()
            .and_then(|b| b.get(id))
            .ok_or_else(|| self.error(format!("unknown blob {id}")))?;
        let at = span[0]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| self.error("invalid blob offset"))?;
        let len = span[1]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| self.error("invalid blob length"))?;
        if len % width != 0 {
            return Err(self.error(format!("blob {id} length is not a multiple of {width}")));
        }
        self.bytes
            .get(
                at..at
                    .checked_add(len)
                    .ok_or_else(|| self.error("blob extent overflow"))?,
            )
            .ok_or_else(|| self.error(format!("blob {id} exceeds the capture file")))
    }
    fn result(&self, call: &Value, id: usize, label: &str) -> Result<(), String> {
        if self.integer(&call[2], label)? != id {
            return Err(self.error(format!("{label} differs from registration order")));
        }
        Ok(())
    }
    fn finite(&self, v: &Value, label: &str) -> Result<f64, String> {
        v.as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| self.error(format!("{label} must be finite")))
    }
}

/// This is the one admission check used by both native hosts. Binary payloads
/// are included because index bounds and frame references cannot be established
/// from blob spans alone. Failure occurs before any resource is allocated.
pub fn validate_capture(meta: &Value, bytes: &[u8]) -> Result<(), String> {
    let c = Capture { meta, bytes };
    let blobs = meta["blobs"]
        .as_array()
        .ok_or_else(|| c.error("missing blob table"))?;
    for i in 0..blobs.len() {
        c.blob(&Value::from(i), 1)?;
    }
    let textures = meta["textures"]
        .as_array()
        .ok_or_else(|| c.error("missing texture table"))?;
    for (i, t) in textures.iter().enumerate() {
        if t.is_null() {
            continue;
        }
        let w = t["width"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| c.error("texture width must be an unsigned JSON integer"))?;
        let h = t["height"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| c.error("texture height must be an unsigned JSON integer"))?;
        let data = c.blob(&t["blob"], 4)?;
        if w == 0
            || h == 0
            || w > 16384
            || h > 16384
            || w.checked_mul(h).and_then(|n| n.checked_mul(4)) != Some(data.len())
        {
            return Err(c.error(format!("texture {i} has invalid RGBA8 dimensions")));
        }
    }
    let mut texture_count = 0;
    let mut programs: Vec<Option<String>> = vec![];
    let mut geometries: Vec<Geometry> = vec![];
    let mut worlds = 0;
    for (call_index, call) in meta["calls"]
        .as_array()
        .ok_or_else(|| c.error("missing calls"))?
        .iter()
        .enumerate()
    {
        let name = call[0]
            .as_str()
            .ok_or_else(|| c.error("missing call name"))?;
        let args = call[1]
            .as_array()
            .ok_or_else(|| c.error(format!("call {call_index} has no arguments")))?;
        // The replay ignores unrelated registration families. Validate exactly
        // the arguments it consumes, without pretending to admit those passes.
        let pattern = match name {
            "add_texture" | "add_geometry" => "",
            "set_far_map" => "n",
            "add_owned_program" => "s",
            "add_program" => "nssss",
            "geometry_buffer" => "nx",
            "geometry_pointer" => "nnnnnbnn",
            "geometry_index" => "nxn",
            "geometry_group" => "nnn",
            "add_world" => "nnnnd",
            "world_texture" => "nnn",
            "world_uniform" => "nsfb",
            "world_cells" => "nfxn",
            "sort" => "",
            _ => "",
        };
        for (i, kind) in pattern.chars().enumerate() {
            let v = args
                .get(i)
                .ok_or_else(|| c.error(format!("{name} argument {i} is missing")))?;
            match kind {
                'n' => {
                    c.finite(v, name)?;
                }
                's' => {
                    v.as_str()
                        .ok_or_else(|| c.error(format!("{name} requires a string")))?;
                }
                'b' => {
                    v.as_bool()
                        .ok_or_else(|| c.error(format!("{name} requires a boolean")))?;
                }
                'x' | 'f' | 'd' => {
                    c.blob(
                        &v["blob"],
                        if kind == 'd' {
                            8
                        } else if kind == 'f' {
                            4
                        } else {
                            1
                        },
                    )?;
                }
                _ => unreachable!(),
            }
        }
        match name {
            "add_texture" => {
                if textures.get(texture_count).is_none_or(|t| t.is_null()) {
                    return Err(c.error(format!("texture {texture_count} has no payload")));
                }
                c.result(call, texture_count, "texture id")?;
                texture_count += 1;
            }
            "set_far_map" => {
                c.id(&args[0], texture_count, "far texture id")?;
            }
            "add_owned_program" => {
                let key = args[0].as_str().unwrap();
                bfe_render::programs::spec(key).map_err(|e| c.error(e))?;
                let id = if let Some(i) = programs.iter().position(|p| p.as_deref() == Some(key)) {
                    i
                } else {
                    let i = programs.len();
                    programs.push(Some(key.into()));
                    i
                };
                c.result(call, id, "program id")?;
            }
            "add_program" => {
                if c.integer(&args[0], "program kind")? > 4 {
                    return Err(c.error("unknown program kind"));
                }
                for a in args[3]
                    .as_str()
                    .unwrap()
                    .split(',')
                    .filter(|s| !s.is_empty())
                {
                    if a.split_once(':')
                        .is_none_or(|(_, v)| v.parse::<u32>().is_err())
                    {
                        return Err(c.error("invalid attribute location"));
                    }
                }
                c.result(call, programs.len(), "program id")?;
                programs.push(None);
            }
            "add_geometry" => {
                c.result(call, geometries.len(), "geometry id")?;
                geometries.push(Geometry::default());
            }
            "geometry_buffer" => {
                let g = c.id(&args[0], geometries.len(), "geometry id")?;
                let data = c.blob(&args[1]["blob"], 1)?;
                if data.len() > i32::MAX as usize {
                    return Err(c.error("vertex buffer is too long"));
                }
                c.result(call, geometries[g].buffers.len(), "buffer id")?;
                geometries[g].buffers.push(data.len());
            }
            "geometry_pointer" => {
                let g = c.id(&args[0], geometries.len(), "geometry id")?;
                let b = c.id(&args[1], geometries[g].buffers.len(), "buffer id")?;
                let size = c.integer(&args[3], "attribute size")?;
                let stride = c.integer(&args[6], "attribute stride")?;
                let offset = c.integer(&args[7], "attribute offset")?;
                let width = match c.integer(&args[4], "attribute type")? as u32 {
                    glow::BYTE | glow::UNSIGNED_BYTE => 1,
                    glow::SHORT | glow::UNSIGNED_SHORT | glow::HALF_FLOAT => 2,
                    glow::INT | glow::UNSIGNED_INT | glow::FLOAT => 4,
                    _ => return Err(c.error("unsupported attribute scalar")),
                };
                if c.integer(&args[2], "attribute location")? > 31
                    || !(1..=4).contains(&size)
                    || stride > i32::MAX as usize
                    || (geometries[g].buffers[b] != 0 || offset != 0)
                        && offset
                            .checked_add(size * width)
                            .is_none_or(|n| n > geometries[g].buffers[b])
                {
                    return Err(c.error("invalid vertex attribute extent"));
                }
            }
            "geometry_index" => {
                let g = c.id(&args[0], geometries.len(), "geometry id")?;
                let width = match c.integer(&args[2], "index type")? as u32 {
                    glow::UNSIGNED_SHORT => 2,
                    glow::UNSIGNED_INT => 4,
                    _ => return Err(c.error("unsupported index scalar")),
                };
                let n = c.blob(&args[1]["blob"], width)?.len();
                if n > i32::MAX as usize {
                    return Err(c.error("index buffer is too long"));
                }
                geometries[g].buffers.push(n);
                geometries[g].indices = n / width;
            }
            "geometry_group" => {
                let g = c.id(&args[0], geometries.len(), "geometry id")?;
                let start = c.integer(&args[1], "group start")?;
                let count = c.integer(&args[2], "group count")?;
                if start
                    .checked_add(count)
                    .is_none_or(|n| n > geometries[g].indices)
                {
                    return Err(c.error("geometry group exceeds its index buffer"));
                }
                geometries[g].groups += 1;
            }
            "add_world" => {
                c.id(&args[0], geometries.len(), "world geometry id")?;
                c.id(&args[1], programs.len(), "world program id")?;
                c.integer(&args[2], "world order")?;
                c.integer(&args[3], "world flags")?;
                let bounds = c.blob(&args[4]["blob"], 8)?;
                if bounds.len() != 10 * 8
                    || bounds
                        .chunks_exact(8)
                        .any(|b| !f64::from_le_bytes(b.try_into().unwrap()).is_finite())
                {
                    return Err(c.error("add_world bounds require ten finite values"));
                }
                c.result(call, worlds, "world id")?;
                worlds += 1;
            }
            "world_texture" => {
                c.id(&args[0], worlds, "world id")?;
                if c.integer(&args[1], "texture unit")? >= bfe_render::core::UNITS {
                    return Err(c.error("world texture unit exceeds renderer storage"));
                }
                c.id(&args[2], texture_count, "world texture id")?;
            }
            "world_uniform" => {
                c.id(&args[0], worlds, "world id")?;
                let data = c.blob(&args[2]["blob"], 4)?;
                if data.is_empty() || data.len() > 4 * 4 {
                    return Err(c.error("world uniform requires one to four values"));
                }
            }
            "world_cells" => {
                c.id(&args[0], worlds, "world id")?;
                let positions = c.blob(&args[1]["blob"], 12)?;
                let index = c.blob(&args[2]["blob"], 12)?;
                if c.finite(&args[3], "cell size")? <= 0.0
                    || index.chunks_exact(4).any(|b| {
                        u32::from_le_bytes(b.try_into().unwrap()) as usize >= positions.len() / 12
                    })
                {
                    return Err(c.error("world cell index exceeds its positions"));
                }
            }
            "sort" => {
                if args.is_empty() {
                    return Err(c.error("sort argument is missing"));
                }
            }
            _ => {}
        }
    }
    let frames = meta["frames"]
        .as_array()
        .ok_or_else(|| c.error("missing frames"))?;
    if frames.is_empty() {
        return Err(c.error("capture has no frames"));
    }
    for (index, f) in frames.iter().enumerate() {
        let records = f["records"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| c.error("frame record count must be an unsigned JSON integer"))?
            .min(I::MAX_RECORDS);
        let input = c.blob(&f["input"], 8)?;
        let bones = c.blob(&f["bones"], 4)?;
        if input.len() / 8 > I::INPUT_LEN
            || bones.len() / 4 > I::BONES_LEN
            || records > 0 && input.len() / 8 < I::IN_RECORDS + records * I::REC_STRIDE
        {
            return Err(c.error(format!(
                "frame {index} exceeds or truncates renderer storage"
            )));
        }
        let scalar = |at: usize| f64::from_le_bytes(input[at * 8..at * 8 + 8].try_into().unwrap());
        for r in 0..records {
            let at = I::IN_RECORDS + r * I::REC_STRIDE;
            let value = |offset| Value::from(scalar(at + offset));
            let kind = c.integer(&value(I::REC_KIND), "instance kind")?;
            if !(1..=4).contains(&kind) {
                return Err(c.error("unknown instance kind"));
            }
            let geometry = c.id(
                &value(I::REC_GEOMETRY),
                geometries.len(),
                "instance geometry id",
            )?;
            if kind <= 2 {
                c.id(
                    &value(I::REC_PROGRAM),
                    programs.len(),
                    "instance program id",
                )?;
            }
            let groups = c.integer(&value(I::REC_GROUPS), "instance group count")?;
            if groups > 4
                || groups > 0
                    && geometries[geometry].groups > 0
                    && groups > geometries[geometry].groups
            {
                return Err(c.error("instance group count exceeds geometry groups"));
            }
            for group in 0..groups.max(1) {
                let t = scalar(at + I::REC_TEXTURES + group);
                if t >= 0.0 {
                    c.id(&Value::from(t), texture_count, "instance texture id")?;
                } else if !t.is_finite() {
                    return Err(c.error("invalid instance texture id"));
                }
            }
            if kind == 1 || kind == 3 {
                let count = c.integer(&value(I::REC_BONES), "bone count")?;
                let first = c.integer(&value(I::REC_BONE_AT), "bone offset")?;
                if count > I::MAX_BONES
                    || first
                        .checked_add(count)
                        .and_then(|n| n.checked_mul(16 * 4))
                        .is_none_or(|n| n > bones.len())
                {
                    return Err(c.error("instance bone range exceeds captured bones"));
                }
            }
        }
    }
    Ok(())
}
