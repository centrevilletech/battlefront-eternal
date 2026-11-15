//! Native GL submission adapter. Image and target lifetime belongs to bfe-render.
pub use bfe_render::resources::GpuResources as Resources;
use glow::HasContext;
use std::{cell::{Cell,RefCell},ffi::c_void};
type MultiElements=unsafe extern "system" fn(u32,*const i32,u32,*const *const c_void,i32);
thread_local!{static MULTI_ELEMENTS:Cell<Option<MultiElements>>=const{Cell::new(None)};}

/// Sorted refraction copies must preserve host framebuffer handles. GLES has
/// no multi-draw entrypoint in glow; issuing the retained ranges is equivalent.
pub struct Ranges {
    gl: glow::Context,
    multi: Option<MultiElements>,
    offsets:RefCell<Vec<*const c_void>>,
    saved: (Option<glow::Framebuffer>, Option<glow::Framebuffer>),
}
impl Ranges {
    pub fn install_multi_draw(gl:&glow::Context,address:*const c_void){
        let function=if (!gl.version().is_embedded||gl.supported_extensions().contains("GL_EXT_multi_draw_arrays"))&&!address.is_null(){Some(unsafe{std::mem::transmute::<*const c_void,MultiElements>(address)})}else{None};
        MULTI_ELEMENTS.set(function);eprintln!("native multi draw: {}",function.is_some());
    }
    pub fn new(gl: glow::Context) -> Self {
        Self {
            gl, multi:MULTI_ELEMENTS.get(),offsets:RefCell::new(vec![std::ptr::null();4096]),
            saved: (None, None),
        }
    }
}
impl bfe_render::core::MultiDraw for Ranges {
    fn multi_draw(&self, mode: u32, counts: &mut [i32], ty: u32, offsets: &mut [i32]) {
        if let Some(draw)=self.multi{
            let mut pointers=self.offsets.borrow_mut();
            let n=counts.len().min(offsets.len());
            if n<=pointers.len(){
                for(i,offset)in offsets[..n].iter().enumerate(){pointers[i]=*offset as usize as *const c_void;}
                unsafe{draw(mode,counts.as_ptr(),ty,pointers.as_ptr(),n as i32);}return;
            }
        }
        for (&count, &offset) in counts.iter().zip(offsets.iter()) {
            unsafe {
                self.gl.draw_elements(mode, count, ty, offset);
            }
        }
    }
    fn save_framebuffers(&mut self) -> bool {
        unsafe {
            self.saved = (
                self.gl
                    .get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
                self.gl
                    .get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
            );
        }
        true
    }
    fn read_saved_draw_framebuffer(&self) {
        unsafe {
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, self.saved.0);
        }
    }
    fn restore_framebuffers(&self) {
        unsafe {
            self.gl
                .bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.saved.0);
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, self.saved.1);
        }
    }
}
