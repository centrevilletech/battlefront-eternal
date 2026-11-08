//! Cold settings changes preserve descriptor identity and GPU storage. Both
//! variants use the catalog's explicit attribute locations; only uniforms move.
use crate::core::{Core,MultiDraw};
use glow::HasContext;

pub fn key(current:&str,shadows:bool)->Option<String>{
    if !current.starts_with("effects/") {return None;}
    let base=current.replace("-shadow","");
    // Descriptor::new deliberately excludes view-space and captured volumes.
    // Flat far water has its own unshadowed reflection profile as well.
    if base.starts_with("effects/view-")||base.starts_with("effects/volume-s")||base.starts_with("effects/jet")||base.contains("-far-far-water"){return None;}
    let next=if !shadows{base}else if let Some(s)=base.strip_suffix("-premultiplied"){format!("{s}-shadow-premultiplied")}else if let Some(s)=base.strip_suffix("-mesh"){format!("{s}-shadow-mesh")}else{format!("{base}-shadow")};
    (next!=current&&crate::programs::spec(&next).is_ok()).then_some(next)
}

pub(crate) fn location<C:HasContext,M:MultiDraw>(core:&Core<C,M>,program:usize,name:&str)->Option<C::UniformLocation>{
    unsafe{core.gl.get_uniform_location(core.programs[program].program,name)}
}

pub(crate) fn program<C:HasContext,M:MultiDraw>(core:&mut Core<C,M>,old:usize,shadows:bool)->Result<usize,String>{
    let Some((current,_))=core.owned_programs.iter().find(|(_,id)|*id as usize==old)else{return Ok(old)};
    let Some(next)=key(current,shadows)else{return Ok(old)};
    let previous=crate::programs::spec(current)?;
    let spec=crate::programs::spec(&next)?;
    let program=core.add_owned_program(&next)? as usize;
    // A descriptor keeps its material owner when settings select its twin.
    // Preserve existing owners of that target program as well.
    let owners=core.programs[old].material_owners.get();
    core.programs[program].material_owners.set(core.programs[program].material_owners.get() | owners);
    // Sampler assignments are descriptor-specific. Preserve their units when
    // switching variants, rather than relying on link-time zero defaults.
    unsafe{
        let gl=&core.gl;gl.use_program(Some(core.programs[program].program));
        for (name,ty) in previous.uniforms {
            if !matches!(ty,32|33|34|35){continue;}
            if let Some(from)=location(core,old,name){
                let mut unit=[0];gl.get_uniform_i32(core.programs[old].program,&from,&mut unit);
                gl.uniform_1_i32(location(core,program,name).as_ref(),unit[0]);
            }
        }
    }
    if previous.key.starts_with("effects/fountain-water-") {
        // These two basin values are immutable and have no live input slot.
        unsafe{let gl=&core.gl;
            if let Some(from)=location(core,old,"reflectionMatrix"){
                let mut value=[0.;16];gl.get_uniform_f32(core.programs[old].program,&from,&mut value);
                gl.uniform_matrix_4_f32_slice(location(core,program,"reflectionMatrix").as_ref(),false,&value);
            }
        }
    }
    debug_assert_eq!(previous.attributes,spec.attributes);
    Ok(program)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn effect_twins_preserve_vertex_bindings_and_round_trip(){
        let mut pairs=0;
        for spec in crate::programs::specs(){
            if spec.key.contains("-shadow"){continue;}
            if let Some(next)=key(spec.key,true){
                let twin=crate::programs::spec(&next).unwrap();
                assert_eq!(spec.attributes,twin.attributes,"{}",spec.key);
                assert_eq!(key(&next,false).as_deref(),Some(spec.key));pairs+=1;
            }
        }
        assert!(pairs>30);
        for s in ["effects/view-basic-s0","effects/volume-s1","effects/jets-s2","effects/water-textured-far-far-water-s2"]{assert!(key(s,true).is_none());}
    }
}
