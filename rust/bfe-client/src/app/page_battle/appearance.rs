//! Typed admission and publication into the renderer's existing InterfacePass.
//! Program names, live fields, matrix packing and raster policy belong there.
use crate::render_bridge::Bridge;
use bfe_content_native::geometry::{Attribute, Geometry, Scalar};
use bfe_render::{
    interface_resource_layout::{kind, look as L, raw as R},
    interface_resources::{Descriptor, Policy, Style},
    interface_scene::layout as D,
};
use three::Matrix4;
#[derive(Clone)]
pub(crate) struct Appearance {
    pub handle: usize,
    pub first: usize,
    pub count: usize,
    pub look: [f64; L::LEN],
}
impl Appearance {
    pub fn new(
        bridge: &mut Bridge,
        geometry: u32,
        count: usize,
        style: Style,
        skin: bool,
        maps: &[i32],
    ) -> Result<Self, String> {
        let policy = Policy::new(&style, skin, maps.first().is_some_and(|m| *m >= 0))?;
        let program = bridge.add_owned_program(&policy.key)?;
        let mut look = [0.; L::LEN];
        look.copy_from_slice(&policy.defaults);
        let handle = bridge.r.interface.resources.descriptors.len();
        bridge
            .r
            .interface
            .resources
            .descriptors
            .push(Descriptor::new(
                program,
                geometry,
                count,
                handle as u32,
                policy,
                maps,
            )?);
        Ok(Self {
            handle,
            first: 0,
            count,
            look,
        })
    }
    pub fn mesh(
        bridge: &mut Bridge,
        vertices: &[f32],
        style: Style,
        maps: &[i32],
    ) -> Result<Self, String> {
        let policy = Policy::new(&style, false, maps.first().is_some_and(|m| *m >= 0))?;
        let count = vertices.len() / 8;
        let geometry = Geometry {
            buffers: vec![vertices.iter().flat_map(|v| v.to_le_bytes()).collect()],
            attributes: [("position", 3, 0), ("normal", 3, 12), ("uv", 2, 24)]
                .into_iter()
                .map(|(name, size, offset)| Attribute {
                    name: name.into(),
                    buffer: 0,
                    size,
                    scalar: Scalar::Float,
                    normalized: false,
                    stride: 32,
                    offset,
                })
                .collect(),
            indices: (0..count as u32).collect(),
            vertices: count,
            groups: Vec::new(),
        }
        .register(&mut bridge.r, &policy.key)?;
        Self::new(bridge, geometry, count, style, false, maps)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &self,
        bridge: &mut Bridge,
        count: &mut usize,
        world: &Matrix4,
        view: &Matrix4,
        projection: &Matrix4,
        bind: &[f64; 16],
        inverse: &[f64; 16],
        look: &[f64; L::LEN],
    ) {
        if *count == D::MAX_DRAWS {
            return;
        }
        let height = bridge.r.hud.input[bfe_render::hud::IN_VIEWPORT + 1];
        let row = &mut bridge.r.interface.input[*count * D::STRIDE..(*count + 1) * D::STRIDE];
        row.fill(0.);
        row[D::PROGRAM] = kind::INTERFACE as f64;
        row[D::GEOMETRY] = self.handle as f64;
        row[D::FIRST] = self.first as f64;
        row[D::COUNT] = self.count as f64;
        row[D::OBJECT] = *count as f64;
        row[D::DEPTH] = -world.elements[14];
        let raw = &mut row[D::VALUES..];
        for (at, matrix) in [
            (R::WORLD, &world.elements),
            (R::VIEW, &view.elements),
            (R::PROJECTION, &projection.elements),
            (R::BIND, bind),
            (R::INVERSE, inverse),
        ] {
            raw[at..at + 16].copy_from_slice(matrix);
        }
        raw[R::LOOK..R::LOOK + L::LEN].copy_from_slice(look);
        raw[R::PIXEL_RATIO] = 1.;
        raw[R::HEIGHT] = height;
        *count += 1;
    }
    pub fn simple(
        &self,
        bridge: &mut Bridge,
        count: &mut usize,
        world: &Matrix4,
        view: &Matrix4,
        projection: &Matrix4,
        look: &[f64; L::LEN],
    ) {
        let identity = Matrix4::default().elements;
        self.push(
            bridge, count, world, view, projection, &identity, &identity, look,
        );
    }
}
pub(crate) fn style(program: &str) -> Style {
    Style {
        program: program.into(),
        side: Some(2),
        depth_test: Some(false),
        depth_write: Some(false),
        ..Style::default()
    }
}
pub(super) fn texture(
    bridge: &mut Bridge,
    width: i32,
    height: i32,
    rgba: &[u8],
) -> Result<u32, String> {
    let texture =
        super::super::shell::ShellHost::upload_rgba(&bridge.r.core.gl, width, height, rgba)?;
    Ok(bridge.r.core.add_texture(texture))
}

/// Borrow the tightly packed float upload until its synchronous GL call returns.
pub(crate) fn float_bytes(values: &[f32]) -> &[u8] {
    // f32 has no padding; the returned slice cannot outlive its source.
    unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
    }
}
