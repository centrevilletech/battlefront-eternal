//! Native static-world asset descriptors. Ports world.js::geometryFor and
//! materials.js::describe/materialFor. Shared native sources feed colour and
//! depth; cold emitter discovery consumes authored arrays without draw shells.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub texture: String,
    pub tree_canopies: Option<crate::static_special::Trees>,
    pub vertex_count: usize,
    pub stride: usize,
    pub attributes: Vec<String>,
    pub flags: u32,
    pub color_kind: Option<String>,
    pub dynamic: bool,
    pub far: bool,
    pub terrain: bool,
    pub bump: Option<String>,
    pub scroll: Option<[f64; 2]>,
    pub specular: Option<[f64; 4]>,
    pub scales: Vec<f64>,
    pub layers: Vec<String>,
    pub low_res_scale: Option<f64>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute { pub name: String, pub size: i32, pub offset: i32 }
pub struct Geometry {
    pub vertices: Vec<f32>,
    pub index: Vec<u32>,
    pub index_width: u32,
    pub interleaved: bool,
    pub fabric: bool,
    pub tree_groups: Option<Vec<f32>>,
    pub attributes: Vec<Attribute>,
    pub stride: i32,
    pub bounds: [f64; 10],
}
impl Geometry {
    // geometryFor's layout is authored, but component types, byte strides and
    // bounds are native policy. Check before any index or multiplication can
    // reach the GPU; malformed cold assets produce one load error.
    pub fn new(record: &Record, vertices: &[f32], index: &[u32]) -> Result<Self, &'static str> {
        if record.vertex_count == 0 || record.vertex_count > 8_388_608 || record.stride > 32 || record.stride < 3
            || vertices.len() != record.vertex_count.checked_mul(record.stride).ok_or("world vertex overflow")?
            || index.len() > 50_331_648 || !index.len().is_multiple_of(3) || index.iter().any(|v| *v as usize >= record.vertex_count)
            || vertices.chunks_exact(record.stride).any(|row|row[..3].iter().any(|v|!v.is_finite())) { return Err("invalid world stream capacity or index"); }
        // geometryFor used one authored interleaved stream. Attribute order
        // determines byte offsets; shader locations come only from the catalog.
        let mut attributes = Vec::with_capacity(record.attributes.len());
        let mut offset = 0;
        for name in &record.attributes {
            let size = match name.as_str() { "position" | "normal" | "tangent" | "binormal" => 3, "uv" => 2, "color" => 4, _ => return Err("unknown world attribute") };
            if attributes.iter().any(|a: &Attribute| &a.name == name) { return Err("duplicate world attribute"); }
            attributes.push(Attribute { name: name.clone(), size, offset: offset * 4 });
            offset += size;
        }
        if offset as usize != record.stride || attributes.first().is_none_or(|a| a.name != "position") { return Err("invalid world vertex layout"); }
        // geometryFor forwards authored UV bits, including NaNs on unused
        // atlas edges. Validate positions for bounds, preserving other streams
        // byte for byte rather than repairing values the GPU already consumed.
        let mut geometry = Self { vertices: vertices.to_vec(), index: index.to_vec(), index_width: 4, interleaved: true, fabric: false, tree_groups: None, attributes, stride: (record.stride*4) as i32, bounds: [0.;10] };
        geometry.update_bounds();
        if !record.far && !record.terrain && record.texture.split('_').any(|s| ["banner","flag","cloth","fabric"].iter().any(|n|s.eq_ignore_ascii_case(n))) { crate::static_special::fabric(&mut geometry)?; }
        crate::static_special::trees(&mut geometry,record)?;
        Ok(geometry)
    }

    // BufferGeometry.computeBoundingSphere centres on the box, then finds
    // the farthest vertex. Compute in f64 over the retained f32 positions.
    pub fn update_bounds(&mut self) {
        let stride=self.stride as usize/4;
        let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];
        for row in self.vertices.chunks_exact(stride) {for k in 0..3 {lo[k]=lo[k].min(row[k] as f64);hi[k]=hi[k].max(row[k] as f64);}}
        let center=[(lo[0]+hi[0])*0.5,(lo[1]+hi[1])*0.5,(lo[2]+hi[2])*0.5];let mut radius2:f64=0.;
        for row in self.vertices.chunks_exact(stride) {let x=row[0] as f64-center[0];let y=row[1] as f64-center[1];let z=row[2] as f64-center[2];radius2=radius2.max(x*x+y*y+z*z);}
        self.bounds=[center[0],center[1],center[2],radius2.sqrt(),lo[0],lo[1],lo[2],hi[0],hi[1],hi[2]];
    }
    pub fn positions(&self) -> Vec<f32> { self.vertices.chunks_exact(self.stride as usize / 4).flat_map(|r| r[..3].iter().copied()).collect() }
    pub fn cells(&self) -> bool { !self.attributes.iter().any(|a|a.name=="fabricAnchor") && self.index.len() >= 1536 && (self.bounds[7]-self.bounds[4] >= 48. || self.bounds[9]-self.bounds[6] >= 48.) }
}

#[derive(Deserialize, Debug)]
pub struct Selector {
    pub key: String, pub defines: Vec<(String,String)>, pub side: u32, pub shadows: u32, pub points: u32,
    pub csm: bool, pub fabric: bool, pub tree: bool, pub map: bool, pub lamps: bool,
}
// The catalog is immutable generated input. Parse it once on cold admission,
// never during frame selection or after each material settings change.
fn selectors() -> &'static [Selector] {
    static TABLE: OnceLock<Vec<Selector>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(include_str!("world-selectors.json")).expect("generated world selectors"))
}
// A retained draw owns values, sampler ids and its catalog key. No browser
// Material or named uniform value is consulted after this cold compilation.
pub struct Material {
    pub key: String,
    pub flags: u32,
    pub receive_shadow: bool,
    pub values: BTreeMap<&'static str, Vec<f32>>,
    pub samplers: Vec<(&'static str, u32, u32)>,
}
impl Material {
    // describe/materialFor: flag precedence and f64 exponent clamping match
    // the old constructors, including hard alpha clearing normal blending.
    pub fn new(record: &Record, geometry: &Geometry, weights: &[u8], samplers: &[i32], features: &[u32]) -> Result<Self, String> {
        if features.len() != crate::static_layout::FEATURES_LENGTH || samplers.len() < 2 { return Err("invalid world material inputs".into()); }
        // describe gives hard alpha precedence over normal blending. Blended
        // and sky families still belong to their separate resource owners.
        let f = if record.terrain { 1 } else { record.flags };
        if f & 0x80 != 0 || f & 4 != 0 && f & 2 == 0 { return Err("blended world belongs to transparent descriptors".into()); }
        let kind = if record.terrain { "terrain" } else if record.color_kind.as_deref() == Some("lit") || f & 0x40 != 0 { "baked" }
            else if f & 1 == 0 { "unlit" } else if record.dynamic { "dynamic" } else { "lit" };
        // Closed catalog selection replaces material defines/version scans.
        // Keep zero-light capacity separate from actual map light counts.
        let mut defines: Vec<(String,String)> = vec![("ZERO_LIGHTS".into(), "16".into()), ("WORLD_SPACE".into(), "".into())];
        for (yes, name) in [(kind=="dynamic","DYNAMIC"),(kind=="baked","BAKED"),(kind=="unlit","UNLIT"),(kind=="terrain","TERRAIN"),
            (record.color_kind.is_some() || record.terrain,"HAS_COLOR"),(record.color_kind.as_deref()==Some("tint"),"TINT"),
            (f&1==0 && !record.terrain,"NO_POINT"),(samplers[1]>=0 && !record.terrain,"BUMP"),(record.scroll.is_some(),"SCROLL"),
            (f&0x20!=0 && record.specular.is_some_and(|s| s[1..].iter().any(|v| *v>0.)),"SPECULAR"),(f&2!=0,"ALPHA_TEST"),(record.far,"FAR")] {
            if yes { defines.push((name.into(), "".into())); }
        }
        // Terrain relighting detects a nearly uniform baked colour. The
        // threshold is the old paintedLayers/materialFor pixel policy.
        let mut lit = [0.;3];
        let mask = terrain_layer_mask(record, weights);
        if record.terrain {
            let color = geometry.attributes.iter().find(|a|a.name=="color").ok_or("terrain lacks baked colour")?.offset as usize / 4;
            for row in geometry.vertices.chunks_exact(record.stride) { for k in 0..3 { lit[k]=f64::max(lit[k],row[color+k] as f64); } }
            let flat = geometry.vertices.chunks_exact(record.stride).filter(|row| (0..3).all(|k|(row[color+k] as f64-lit[k]).abs()<0.02)).count();
            let relight = flat as f64 > record.vertex_count as f64 * 0.85;
            if relight { defines.push(("TERRAIN_RELIGHT".into(),"".into())); }
            if !record.far && mask!=0 { defines.push(("TERRAIN_LAYERS".into(),(32-mask.leading_zeros()).to_string())); }
        }
        defines.sort();
        let side = if f & 0x86 != 0 { 2 } else { 0 };
        let lamps = features[3] != 0 && kind == "dynamic" && f & 1 != 0 && !record.far;
        let fabric = geometry.attributes.iter().any(|a|a.name=="fabricAnchor");
        let tree = geometry.attributes.iter().any(|a|a.name=="treeGroup");
        // All supported shader variants are generated from the source catalog.
        // Unknown cold combinations fail admission rather than drawing via JS.
        let selector = selectors().iter().find(|s| s.defines == defines && s.side == side && s.shadows == features[0] && s.points == features[1]
            && s.csm == (features[2]!=0) && s.fabric==fabric && s.tree==tree && s.map == (samplers[0]>=0) && s.lamps == lamps)
            .ok_or_else(|| format!("unowned native world variant {defines:?}, side={side}, features={features:?}"))?;
        // worldMaterial clamps only the specular exponent. RGB strengths and
        // scrolling rates retain their original final Float32 assignments.
        let specular = if f&0x20!=0 { record.specular.unwrap_or([0.;4]) } else { [0.;4] };
        let scroll = record.scroll.unwrap_or([0.;2]);
        let mut values = BTreeMap::new();
        values.insert("sunScale", vec![1.]); values.insert("brightness", vec![1.]); values.insert("ambientPull", vec![0.,0.]);
        values.insert("scroll", scroll.map(|v|v as f32).to_vec());
        values.insert("specular", vec![specular[0].clamp(10.,100.) as f32,specular[1] as f32,specular[2] as f32,specular[3] as f32]);
        // Sampler units match the existing shader bindings. They are logical
        // resource ids, so uploads and loss recovery can replace GPU handles.
        let mut textures = vec![];
        if samplers[0]>=0 { textures.push(("map",0,samplers[0] as u32)); }
        else if record.far {
            // FarPass.emptyTexture supplied a zero texel even for a catalog
            // variant without an authored map; near-world binding is unchanged.
            if let Some(&empty)=samplers.get(crate::static_layout::SAMPLERS_EMPTY) {if empty>=0 {textures.push(("map",0,empty as u32));}}
        }
        if samplers[1]>=0 { textures.push(("bumpMap",2,samplers[1] as u32)); }
        if tree {
            if let Some(&id)=samplers.get(crate::static_layout::SAMPLERS_TREE) { if id>=0 { textures.push(("treeStates",3,id as u32)); } }
            values.insert("treeStateCount",vec![record.tree_canopies.as_ref().map_or(0,|t|t.instances.len()) as f32]);
        }
        if record.terrain {
            if samplers.len()!=crate::static_layout::SAMPLERS_LENGTH { return Err("terrain sampler count".into()); }
            textures.push(("layer1Map",4,samplers[2] as u32)); textures.push(("detailMap",5,samplers[3] as u32));
            for (i,name) in ["layerMap0","layerMap1","layerMap2","layerMap3","layerMap4","layerMap5","layerMap6","layerMap7"].into_iter().enumerate() { textures.push((name,6+i as u32,samplers[4+i] as u32)); }
            // Low-resolution far terrain repeats one whole-map texture while
            // near terrain retains each authored layer scale and baked light.
            let scales = |i| record.scales.get(i).copied().unwrap_or(8.) as f32;
            if record.far {
                let repeat=record.low_res_scale.filter(|v|*v>0.).unwrap_or(1.);
                values.insert("farOrigin",vec![geometry.bounds[4] as f32,geometry.bounds[6] as f32]);
                values.insert("layerScales",vec![((geometry.bounds[7]-geometry.bounds[4])/repeat) as f32,((geometry.bounds[9]-geometry.bounds[6])/repeat) as f32]);
                values.insert("bakedLit",vec![(145./255.) as f32,(120./255.) as f32,(90./255.) as f32]);
            } else {
                values.insert("farOrigin",vec![0.,0.]); values.insert("layerScales",vec![scales(0),scales(1)]);
                values.insert("bakedLit",lit.map(|v|v as f32).to_vec());
            }
            values.insert("layerScale0",(0..4).map(scales).collect()); values.insert("layerScale1",(4..8).map(scales).collect());
        }
        Ok(Self { key: selector.key.clone(), flags: side | if f&2!=0 { 4 } else { 0 } | 8 | if f&1!=0 && kind!="unlit" && !record.far && !record.terrain { 16 } else { 0 } | if record.terrain { 32 } else { 0 },
            receive_shadow: kind!="baked" && kind!="unlit", values, samplers: textures })
    }
}

// TerrainPatches' immutable bins and reduced level. Cells retain source triangle
// order within each bin; changing that order changes cutout/MSAA edge pixels.
pub struct Terrain { pub full: Level, pub reduced: Level, pub sphere: [f64;4] }
pub struct Level { pub index: Vec<u32>, pub cells: Vec<[f64;8]> }
impl Terrain {
    pub fn new(geometry: &Geometry) -> Self {
        let stride = geometry.stride as usize / 4;
        let p = |v: u32,k:usize| geometry.vertices[v as usize*stride+k] as f64;
        let lo = &geometry.bounds[4..7]; let hi = &geometry.bounds[7..10];
        // TerrainPatches partitions the whole map into bounded 128 m cells.
        // Its rounded cell count is observable at the outer terrain seams.
        let nx = ((hi[0]-lo[0])/128.).round().clamp(1.,32.) as usize;
        let nz = ((hi[2]-lo[2])/128.).round().clamp(1.,32.) as usize;
        let sx = ((hi[0]-lo[0])/nx as f64).max(f64::MIN_POSITIVE);
        let sz = ((hi[2]-lo[2])/nz as f64).max(f64::MIN_POSITIVE);
        let bin = |source:&[u32]| {
            // Bin by triangle centroid, retaining the authored triangle order
            // inside each cell. Full and reduced levels use the same policy.
            let mut bins = vec![Vec::new();nx*nz];
            for t in source.as_chunks::<3>().0 {
                let x = (p(t[0],0)+p(t[1],0)+p(t[2],0))/3.;
                let z = (p(t[0],2)+p(t[1],2)+p(t[2],2))/3.;
                let cx = ((x-lo[0])/sx).floor().clamp(0.,(nx-1) as f64) as usize;
                let cz = ((z-lo[2])/sz).floor().clamp(0.,(nz-1) as f64) as usize;
                bins[cz*nx+cx].extend_from_slice(t);
            }
            let mut index = Vec::with_capacity(source.len()); let mut cells = vec![];
            for b in bins { if b.is_empty() { continue; }
                // Per-cell boxes enclose referenced vertices rather than the
                // centroid grid, so a tall edge is not incorrectly culled.
                let start = index.len(); let mut min = [f64::INFINITY;3]; let mut max = [f64::NEG_INFINITY;3];
                for &v in &b { for k in 0..3 { min[k]=min[k].min(p(v,k));max[k]=max[k].max(p(v,k)); } }
                index.extend_from_slice(&b);
                cells.push([start as f64,index.len() as f64,min[0],min[1],min[2],max[0],max[1],max[2]]);
            }
            Level { index, cells }
        };
        let full = bin(&geometry.index);
        // level(true)'s most common horizontal edge, tied by first appearance.
        // Keep the JS Map insertion rule rather than sorting equal counts.
        let mut lengths: Vec<(f64,usize)> = vec![];
        for t in full.index[..full.index.len().min(30000)].as_chunks::<3>().0 { for k in 0..3 {
            let a=t[k];let b=t[(k+1)%3];
            let d=(((p(a,0)-p(b,0)).abs().max((p(a,2)-p(b,2)).abs()))*100.).round()/100.;
            if d>0. { if let Some((_,n))=lengths.iter_mut().find(|(v,_)|*v==d) { *n+=1; } else { lengths.push((d,1)); } }
        }}
        let (mut spacing,mut most) = (0.,0);
        for (d,n) in lengths { if n>most {most=n;spacing=d;} }
        let reduced = if spacing<=0. { bin(&full.index) } else {
            // level(true) snaps each source vertex to a twice-spaced grid.
            // The quarter-cell offset preserves its original tie boundaries.
            let step=spacing*2.;let count=geometry.vertices.len()/stride;
            let mut keys=vec![0i64;count];let mut best=std::collections::HashMap::new();
            for (v,key) in keys.iter_mut().enumerate() {
                let cx=((p(v as u32,0)-lo[0])/step+0.25).floor();let cz=((p(v as u32,2)-lo[2])/step+0.25).floor();
                *key=(cz*65536.+cx) as i64;
                let x=(*key as f64)%65536.;let z=(*key as f64/65536.).floor();
                let dx=p(v as u32,0)-(lo[0]+x*step);let dz=p(v as u32,2)-(lo[2]+z*step);let d=dx*dx+dz*dz;
                let held=best.entry(*key).or_insert((v as u32,d));if d<held.1 {*held=(v as u32,d);}
            }
            // Choose the nearest original vertex, preserving its height and
            // other attributes instead of interpolating a different surface.
            let rep:Vec<_>=keys.iter().map(|k|best[k].0).collect();
            let area=|a,b,c|(p(b,0)-p(a,0))*(p(c,2)-p(a,2))-(p(b,2)-p(a,2))*(p(c,0)-p(a,0));
            let mut kept=vec![];let mut seen=std::collections::HashSet::new();
            // Collapsed or reversed triangles disappear. Duplicate keys use
            // the original numeric arithmetic, including its f64 rounding.
            for t in full.index.as_chunks::<3>().0 {
                let a=rep[t[0] as usize];let b=rep[t[1] as usize];let c=rep[t[2] as usize];
                if a==b||b==c||a==c||area(t[0],t[1],t[2])*area(a,b,c)<=0. {continue;}
                let mut sorted=[a,b,c];sorted.sort();let [x,y,z]=sorted.map(|v|v as f64);
                if seen.insert(((x*count as f64+y)*count as f64+z).to_bits()) { kept.extend_from_slice(&[a,b,c]); }
            }
            bin(&kept)
        };
        // TerrainPatches used the box diagonal for its conservative sphere,
        // distinct from geometryFor's farthest-vertex sphere.
        let x=hi[0]-lo[0];let y=hi[1]-lo[1];let z=hi[2]-lo[2];
        Self { full,reduced,sphere:[geometry.bounds[0],geometry.bounds[1],geometry.bounds[2],(x*x+y*y+z*z).sqrt()*0.5] }
    }
}

// world.js::paintedLayers used eight normalized bytes per vertex. A malformed
// optional weight file keeps the old two-layer terrain instead of indexing it.
pub fn terrain_layer_mask(record:&Record, weights:&[u8])->u32 {
    if record.vertex_count.checked_mul(8)!=Some(weights.len()) {return 0;}
    let mut mask=0;
    for i in 0..record.layers.len().min(8) { if weights.iter().skip(i).step_by(8).any(|v|*v!=0) {mask|=1<<i;} }
    mask
}

// Shared upload policy for colour and depth descriptors. Fabric keeps its
// separate streams; ordinary buckets retain the authored interleave and tree
// masks retain their additional scalar stream. No host geometry is inspected.
pub fn upload_geometry<C:glow::HasContext,M:crate::core::MultiDraw>(core:&mut crate::core::Core<C,M>,id:u32,g:&Geometry,attributes:&[(&str,u32)]) -> Result<(),String> {
    // Fabric's previous constructor produced separate Float32 attributes.
    // Preserve that representation as well as its values for depth replay.
    let buffer=if g.interleaved {Some(core.geometry_buffer(id,crate::core::bytes(&g.vertices)))} else {None};
    for (name,location) in attributes {
        // Weight streams are attached by the terrain compiler after this
        // shared upload; no other family can manufacture normalized weights.
        if *name=="weights0"||*name=="weights1" {continue;}
        let a=g.attributes.iter().find(|a|a.name==*name).ok_or_else(||format!("missing static attribute {name}"))?;
        if *name=="treeGroup" {
            // Original-tree ids are a separate scalar buffer, not a stride
            // extension that could shift the authored normal/UV offsets.
            let groups=g.tree_groups.as_ref().ok_or("missing static tree groups")?;
            let slot=core.geometry_buffer(id,crate::core::bytes(groups));core.geometry_pointer(id,slot,*location,1,glow::FLOAT,false,0,0);
        } else if !g.interleaved {
            let from=a.offset as usize/4;let width=g.stride as usize/4;
            let values=g.vertices.chunks_exact(width).flat_map(|row|row[from..from+a.size as usize].iter().copied()).collect::<Vec<_>>();
            let slot=core.geometry_buffer(id,crate::core::bytes(&values));core.geometry_pointer(id,slot,*location,a.size,glow::FLOAT,false,0,0);
        } else {core.geometry_pointer(id,buffer.unwrap(),*location,a.size,glow::FLOAT,false,g.stride,a.offset);}
    }
    // BufferGeometry.setIndex chose sixteen bits for subdivided small cloth;
    // imported world buckets and merged static casters retain their u32 index.
    if g.index_width==2 {let indices=g.index.iter().map(|v|*v as u16).collect::<Vec<_>>();core.geometry_index(id,crate::core::bytes(&indices),glow::UNSIGNED_SHORT);}
    else {core.geometry_index(id,crate::core::bytes(&g.index),glow::UNSIGNED_INT);}
    Ok(())
}
