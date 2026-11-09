//! Cold fabric and tree geometry formerly registered through world.js and
//! TreeCanopies.adapt. Depth casters may retain their own CPU source; these
//! colour descriptors always compile from immutable authored map records.
use crate::static_world::{Attribute, Geometry, Record};
use serde::Deserialize;
use std::collections::HashMap;
use three::{Box3, Matrix4, Vector3};

#[derive(Clone, Deserialize, Default)]
pub struct Trees { pub instances: Vec<Tree> }
#[derive(Clone, Deserialize)]
pub struct Tree { pub matrix: [f64;16], pub bounds: [[f64;3];2] }
fn root(parent:&mut [usize], mut i:usize)->usize {
    while parent[i]!=i { parent[i]=parent[parent[i]];i=parent[i]; }
    i
}
fn join(parent:&mut [usize],a:usize,b:usize) { let a=root(parent,a);let b=root(parent,b);parent[a]=b; }

// fabricGeometry welds only connectivity, then adds two subdivisions without
// splitting a UV/normal seam. Keep intermediate components f64 until the old
// Float32BufferAttribute conversion, including anchor interpolation.
pub fn fabric(g:&mut Geometry)->Result<(), &'static str> {
    if g.index.len()>3_145_728 { return Err("fabric subdivision capacity"); }
    let stride=g.stride as usize/4;
    let vertices=g.vertices.iter().map(|v|*v as f64).collect::<Vec<_>>();
    let (rows,index)=fabric_rows(&vertices,stride,&g.index)?;
    let width=stride+2;
    g.fabric=true;g.interleaved=false;
    g.vertices=rows.into_iter().map(|v|v as f32).collect();g.index=index;
    // Three's setIndex promotes at 65535, reserving that value rather than
    // selecting width from the number of vertices alone.
    g.index_width=if g.index.iter().any(|v|*v>=65535) {4} else {2};
    g.attributes.push(Attribute {name:"fabricAnchor".into(),size:2,offset:g.stride});g.stride=(width*4) as i32;
    g.update_bounds();
    // fabricGeometry gives wind displacement its existing conservative margin.
    for k in 4..7 {g.bounds[k]-=0.18;}for k in 7..10 {g.bounds[k]+=0.18;}g.bounds[3]+=0.18;
    Ok(())
}

// fabricGeometry's cold shell also serves blended map records. Share the
// subdivision implementation with static descriptors and keep normalized
// source attributes in f64 until the final Float32BufferAttribute boundary.
fn fabric_rows(vertices:&[f64],stride:usize,indices:&[u32])->Result<(Vec<f64>,Vec<u32>), &'static str> {
    if stride<3||!vertices.len().is_multiple_of(stride)||!indices.len().is_multiple_of(3)||indices.len()>3_145_728||indices.iter().any(|i|*i as usize>=vertices.len()/stride){return Err("invalid fabric stream");}
    let count=vertices.len()/stride;
    // fabricGeometry rounds weld positions to millimetres only to discover
    // connected cloth pieces; original UV and normal seams remain separate.
    let mut parent:Vec<_>=(0..count).collect();let mut welded=HashMap::new();
    for v in 0..count {
        let key = [0,1,2].map(|k|(vertices[v*stride+k]*1000.+0.5).floor() as i64);
        if let Some(&held)=welded.get(&key) { join(&mut parent,v,held); } else { welded.insert(key,v); }
    }
    for t in indices.as_chunks::<3>().0 {join(&mut parent,t[0] as usize,t[1] as usize);join(&mut parent,t[0] as usize,t[2] as usize);}
    let mut bounds:HashMap<usize,(f64,f64)>=HashMap::new();
    for v in 0..count { let id=root(&mut parent,v);let y=vertices[v*stride+1];
        let b=bounds.entry(id).or_insert((f64::INFINITY,f64::NEG_INFINITY));b.0=b.0.min(y);b.1=b.1.max(y);
    }
    // Every piece carries its own top and height. These anchors must be
    // interpolated along with positions through both subdivision rounds.
    let width=stride+2;let mut rows=Vec::with_capacity(count*width);
    for v in 0..count {
        rows.extend_from_slice(&vertices[v*stride..(v+1)*stride]);
        let b=bounds[&root(&mut parent,v)];rows.extend_from_slice(&[b.1,(b.1-b.0).max(0.01)]);
    }
    let mut index=indices.to_vec();
    for _ in 0..2 {
        let mut cache=HashMap::new();let mut next=Vec::with_capacity(index.len()*4);
        // Reuse each undirected edge once per subdivision round. The first
        // encounter fixes vertex order and therefore exact index bytes.
        let mut midpoint=|a:u32,b:u32| {
            let key=if a<b {(a,b)} else {(b,a)};
            if let Some(&v)=cache.get(&key) {return v;}
            let v=(rows.len()/width) as u32;
            for k in 0..width { rows.push((rows[a as usize*width+k]+rows[b as usize*width+k])*0.5); }
            cache.insert(key,v);v
        };
        for t in index.as_chunks::<3>().0 {let [a,b,c]=[t[0],t[1],t[2]];let ab=midpoint(a,b);let bc=midpoint(b,c);let ca=midpoint(c,a);
            next.extend_from_slice(&[a,ab,ca,ab,b,bc,ca,bc,c,ab,bc,ca]);
        }
        index=next;
    }
    Ok((rows,index))
}

#[derive(serde::Deserialize,serde::Serialize)]
pub struct ClothAttribute {pub name:String,pub size:usize,pub values:Vec<f64>}
#[derive(serde::Deserialize,serde::Serialize)]
pub struct Cloth {pub attributes:Vec<ClothAttribute>,pub index:Vec<u32>,#[serde(default)]pub bounds:[f64;10]}
impl Cloth {
    // fabricGeometry accepts arbitrary draw attributes, including normalized
    // colours. Reordering only the internal position stream leaves the host's
    // attribute identities and exact original component values intact.
    pub fn compile(mut self)->Result<Self,&'static str>{
        let position=self.attributes.iter().position(|a|a.name=="position"&&a.size==3).ok_or("fabric position missing")?;
        self.attributes.swap(0,position);let count=self.attributes[0].values.len()/3;
        let stride=self.attributes.iter().map(|a|a.size).sum::<usize>();
        if count>8_388_608||stride>64||self.attributes.iter().any(|a|a.size==0||a.values.len()!=count*a.size){return Err("invalid fabric attributes");}
        let mut vertices=Vec::with_capacity(count*stride);
        for i in 0..count{for a in &self.attributes{vertices.extend_from_slice(&a.values[i*a.size..(i+1)*a.size]);}}
        if self.index.is_empty(){self.index=(0..count as u32).collect();}
        let (rows,index)=fabric_rows(&vertices,stride,&self.index)?;let width=stride+2;
        let mut offset=0;for a in &mut self.attributes{a.values.clear();for row in rows.chunks_exact(width){a.values.extend(row[offset..offset+a.size].iter().map(|v|*v as f32 as f64));}offset+=a.size;}
        self.attributes.push(ClothAttribute{name:"fabricAnchor".into(),size:2,values:rows.chunks_exact(width).flat_map(|r|r[stride..].iter().map(|v|*v as f32 as f64)).collect()});
        let mut geometry=Geometry{vertices:rows.into_iter().map(|v|v as f32).collect(),index:vec![],index_width:4,interleaved:false,fabric:true,tree_groups:None,attributes:vec![],stride:(width*4) as i32,bounds:[0.;10]};
        geometry.update_bounds();for k in 4..7{geometry.bounds[k]-=0.18;}for k in 7..10{geometry.bounds[k]+=0.18;}geometry.bounds[3]+=0.18;
        self.bounds=geometry.bounds;self.index=index;self.attributes.swap(0,position);Ok(self)
    }
}

// TreeCanopies.adapt assigns each connected original billboard to its nearest
// containing canopy footprint. Equal distances preserve authored group order;
// small bushes and unmatched pieces keep -1 and remain visible in the shader.
pub fn trees(g:&mut Geometry, record:&Record)->Result<(), &'static str> {
    let Some(trees)=&record.tree_canopies else { return Ok(()); };
    if trees.instances.len()>65536 { return Err("tree group capacity"); }
    let name=record.texture.to_ascii_lowercase();let bush=name.contains("bush");
    if !bush && !name.contains("treebillboard") {return Ok(());}
    let stride=g.stride as usize/4;let count=g.vertices.len()/stride;
    let mut parent:Vec<_>=(0..count).collect();
    for t in g.index.as_chunks::<3>().0 {join(&mut parent,t[0] as usize,t[1] as usize);join(&mut parent,t[1] as usize,t[2] as usize);}
    let mut pieces:Vec<Vec<usize>>=vec![];let mut by_root=HashMap::new();
    for v in 0..count {let id=root(&mut parent,v);let next=pieces.len();let at=*by_root.entry(id).or_insert_with(|| {pieces.push(vec![]);next});pieces[at].push(v);}
    let vec=|a:[f64;3]|Vector3::new(a[0],a[1],a[2]);
    // TreeCanopies.adapt tests in each canopy's local footprint and compares
    // world-space centres. Reusing those spaces preserves rotated canopies.
    let groups=trees.instances.iter().map(|t| {
        let matrix=Matrix4 {elements:t.matrix};let mut inverse=matrix;inverse.invert();
        let local=Box3::new(vec(t.bounds[0]),vec(t.bounds[1]));let mut world=local;world.apply_matrix4(&matrix);
        let center=world.get_center(&mut Vector3::default());(inverse,local,center)
    }).collect::<Vec<_>>();
    let mut ids=vec![-1.;count];let mut assigned=false;
    for vertices in pieces {
        let mut bbox=Box3::default();
        for &v in &vertices {bbox.expand_by_point(&Vector3::new(g.vertices[v*stride] as f64,g.vertices[v*stride+1] as f64,g.vertices[v*stride+2] as f64));}
        let center=bbox.get_center(&mut Vector3::default());let mut size=Vector3::default();bbox.get_size(&mut size);
        if bush && size.x.max(size.y).max(size.z)<5.5 {continue;}
        let mut distance=f64::INFINITY;let mut best=None;
        for (i,(inverse,local,at)) in groups.iter().enumerate() {
            let mut point=center;point.apply_matrix4(inverse);if !local.contains_point(&point) {continue;}
            let d=center.distance_to_squared(at);if d<distance {best=Some(i);distance=d;}
        }
        if let Some(id)=best {assigned=true;for v in vertices {ids[v]=id as f32;}}
    }
    // No extra attribute or program variant is needed when all original
    // billboards remain unmatched; -1 still keeps a partial match visible.
    if assigned {
        g.tree_groups=Some(ids);
        g.attributes.push(Attribute {name:"treeGroup".into(),size:1,offset:0});
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Disconnected cards share an authored stream. Only the large card in
    // a canopy footprint should acquire a mask; small and distant ones stay.
    fn cards(texture: &str) -> (Record, Vec<f32>, Vec<u32>) {
        let mut vertices = vec![];
        let mut index = vec![];
        for (x, y, z, width, height) in [(0.,0.,-30.,7.,8.), (0.,1.,-29.,1.,1.), (40.,0.,-30.,8.,8.)] {
            let n = (vertices.len()/3) as u32;
            vertices.extend_from_slice(&[x-width/2.,y,z,x+width/2.,y,z,x-width/2.,y+height,z,x+width/2.,y+height,z]);
            index.extend_from_slice(&[n,n+1,n+2,n+2,n+1,n+3]);
        }
        let tree = Tree { matrix: Matrix4::default().elements, bounds: [[-4.,0.,-34.],[4.,12.,-26.]] };
        let record = Record { texture: texture.into(), vertex_count: 12, stride: 3,
            attributes: vec!["position".into()], tree_canopies: Some(Trees { instances: vec![tree.clone(),tree] }), ..Record::default() };
        (record,vertices,index)
    }

    #[test]
    fn connected_tree_masks_preserve_small_bushes_and_authored_streams() {
        let (record,vertices,index) = cards("jungle_bush");
        let geometry = Geometry::new(&record,&vertices,&index).unwrap();
        assert_eq!(geometry.vertices,vertices);
        assert_eq!(geometry.index,index);
        // Equal-distance overlaps select the first authored canopy.
        assert_eq!(geometry.tree_groups.unwrap(),[0.,0.,0.,0.,-1.,-1.,-1.,-1.,-1.,-1.,-1.,-1.]);
    }

    #[test]
    fn ordinary_materials_keep_their_original_layout() {
        let (record,vertices,index) = cards("stone_wall");
        let geometry = Geometry::new(&record,&vertices,&index).unwrap();
        assert!(geometry.tree_groups.is_none());
        assert_eq!(geometry.attributes.len(),1);
    }
}

#[cfg(test)]
mod cloth_tests {
    use super::*;
    #[test]
    fn subdivided_cloth_preserves_normalized_attributes_and_component_anchors(){
        // A single triangle has six edges after one split and sixteen
        // triangles after two. Normalized colour stays double until output.
        let cloth=Cloth{attributes:vec![
            ClothAttribute{name:"color".into(),size:1,values:vec![1./255.,2./255.,3./255.]},
            ClothAttribute{name:"position".into(),size:3,values:vec![0.,0.,0.,2.,0.,0.,0.,4.,0.]},
        ],index:vec![0,1,2],bounds:[0.;10]}.compile().unwrap();
        assert_eq!(cloth.index.len(),48);assert_eq!(&cloth.index[..12],&[0,6,8,6,3,7,8,7,5,6,7,8]);
        let p=&cloth.attributes[1].values;assert_eq!(p.len(),45);
        let anchor=&cloth.attributes[2].values;assert!(anchor.as_chunks::<2>().0.iter().all(|v|v==&[4.,4.]));
        assert_eq!(cloth.attributes[0].values[3],((1./255.+2./255.)*0.5) as f32 as f64);
        assert_eq!(cloth.bounds[4],-0.18);assert_eq!(cloth.bounds[8],4.18);
    }
}
