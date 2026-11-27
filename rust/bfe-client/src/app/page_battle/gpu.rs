//! Page stage geometry submitted through the source-owned InterfacePass.
//! Only the holomap render target is device plumbing here; materials, uniform
//! layouts, skinning and draw policy stay in the client's shared renderer.
use super::{
    appearance::{self, Appearance},
    rig::Rig,
    textures,
};
use crate::render_bridge::Bridge;
use bfe_render::interface_resource_layout::look as L;
use glow::HasContext;
use three::{Matrix4, Quaternion, Vector3 as V};
type Mesh = (i32, i32);
struct Mote {
    angle: f64,
    radius: f64,
    height: f64,
    speed: f64,
}
struct Logo {
    name: String,
    appearance: Appearance,
}
pub(super) struct Gpu {
    pub target: glow::Framebuffer,
    vignette: Appearance,
    /// The 2004 look's hologram room; absent where the content has no grid texture.
    room: Option<Appearance>,
    floor: Appearance,
    ring: Appearance,
    inner: Appearance,
    glow: Appearance,
    column: Appearance,
    face: Appearance,
    back: Appearance,
    rim: Appearance,
    beam: Appearance,
    base: Appearance,
    cone: Appearance,
    lamp: Appearance,
    pool: Appearance,
    band: Appearance,
    pin: Appearance,
    pin_dot: Appearance,
    pin_ring: Appearance,
    pin_wave: Appearance,
    logos: Vec<Logo>,
    motes: [Mote; 40],
    mote_vertices: [f32; 40 * 8],
    mote_geometry: u32,
    mote_appearance: Appearance,
    random: jsrt::rng::ClassicRandom,
}
fn expanded(vertices: &[f32], mesh: Mesh) -> Vec<f32> {
    let mut out = Vec::with_capacity(mesh.1 as usize * 8);
    for t in vertices[mesh.0 as usize * 5..(mesh.0 + mesh.1) as usize * 5].chunks_exact(15) {
        let mut a = V::new(
            (t[5] - t[0]) as f64,
            (t[6] - t[1]) as f64,
            (t[7] - t[2]) as f64,
        );
        let b = V::new(
            (t[10] - t[0]) as f64,
            (t[11] - t[1]) as f64,
            (t[12] - t[2]) as f64,
        );
        a.cross(&b).normalize();
        for p in t.chunks_exact(5) {
            out.extend_from_slice(&p[..3]);
            out.extend([a.x as f32, a.y as f32, a.z as f32]);
            out.extend_from_slice(&p[3..]);
        }
    }
    out
}
fn additive(program: &str) -> bfe_render::interface_resources::Style {
    let mut s = appearance::style(program);
    s.transparent = Some(true);
    s.blending = Some(2);
    s
}
fn vertex(out: &mut Vec<f32>, p: [f64; 3], uv: [f64; 2]) {
    out.extend(p.map(|v| v as f32));
    out.extend(uv.map(|v| v as f32));
}
fn triangle(out: &mut Vec<f32>, p: [[f64; 3]; 3], uv: [[f64; 2]; 3]) {
    for i in 0..3 {
        vertex(out, p[i], uv[i]);
    }
}
fn cylinder(out: &mut Vec<f32>, top: f64, bottom: f64, height: f64, n: usize, caps: bool) -> Mesh {
    let first = out.len() / 5;
    for i in 0..n {
        let a = i as f64 / n as f64 * std::f64::consts::TAU;
        let b = (i + 1) as f64 / n as f64 * std::f64::consts::TAU;
        // THREE.CylinderGeometry's vertex at angle t is (r sin t, y, r cos t): a hexagon's corner faces +z.
        let p = [
            [a.sin() * bottom, 0., a.cos() * bottom],
            [b.sin() * bottom, 0., b.cos() * bottom],
            [a.sin() * top, height, a.cos() * top],
            [b.sin() * top, height, b.cos() * top],
        ];
        let u = i as f64 / n as f64;
        let v = (i + 1) as f64 / n as f64;
        triangle(out, [p[0], p[2], p[1]], [[u, 0.], [u, 1.], [v, 0.]]);
        triangle(out, [p[1], p[2], p[3]], [[v, 0.], [u, 1.], [v, 1.]]);
        if caps {
            triangle(
                out,
                [[0., height, 0.], p[3], p[2]],
                [[0.5, 0.5], [1., 1.], [0., 1.]],
            );
            triangle(out, [[0.; 3], p[0], p[1]], [[0.5, 0.5], [0., 0.], [1., 0.]]);
        }
    }
    (first as i32, (out.len() / 5 - first) as i32)
}
// THREE.RingGeometry in the xy plane: `segments` sectors over `sweep` radians from angle 0.
fn ring(out: &mut Vec<f32>, inner: f64, outer: f64, segments: usize, sweep: f64, turn: f64) -> Mesh {
    let first = out.len() / 5;
    let uv = |x: f64, y: f64| [(x / outer + 1.) * 0.5, (y / outer + 1.) * 0.5];
    for i in 0..segments {
        let a = turn + i as f64 / segments as f64 * sweep;
        let b = turn + (i + 1) as f64 / segments as f64 * sweep;
        let p = [
            [a.cos() * inner, a.sin() * inner, 0.],
            [a.cos() * outer, a.sin() * outer, 0.],
            [b.cos() * inner, b.sin() * inner, 0.],
            [b.cos() * outer, b.sin() * outer, 0.],
        ];
        let t = p.map(|q| uv(q[0], q[1]));
        triangle(out, [p[0], p[1], p[2]], [t[0], t[1], t[2]]);
        triangle(out, [p[2], p[1], p[3]], [t[2], t[1], t[3]]);
    }
    (first as i32, (out.len() / 5 - first) as i32)
}
/// One post's pin on the hologram, in the disc's own units (radius 1).
pub(super) struct Pin {
    pub x: f64,
    pub y: f64,
    pub height: f64,
    pub picked: bool,
    pub wave: f64,
    pub color: [f64; 3],
}
/// The projector's parts that light with the hologram: ground glow, band and lamp.
fn depth_tested(mut s: bfe_render::interface_resources::Style) -> bfe_render::interface_resources::Style {
    s.depth_test = Some(true);
    s
}

/// The command post hologram that stands for a side.
pub(super) fn logo_for(side: &str) -> &'static str {
    match side {
        "cis" => "com_icon_cis",
        "imp" => "com_icon_imperial",
        "all" => "com_icon_alliance",
        _ => "com_icon_republic",
    }
}

impl Gpu {
    /// A side's emblem as a flat picture, for content without the hologram
    /// models (a 2005 battle): it turns and takes the team's colour as they do.
    pub fn add_flat_logo(&mut self, bridge: &mut Bridge, side: &str, texture: i32) -> Result<(), String> {
        let name = logo_for(side);
        if self.logos.iter().any(|l| l.name == name) {
            return Ok(());
        }
        let mut vertices = Vec::with_capacity(48);
        for (x, y, u, v) in [
            (-0.5, -0.5, 0., 1.),
            (0.5, -0.5, 1., 1.),
            (0.5, 0.5, 1., 0.),
            (-0.5, -0.5, 0., 1.),
            (0.5, 0.5, 1., 0.),
            (-0.5, 0.5, 0., 0.),
        ] {
            vertices.extend([x, y, 0., 0., 0., 1., u, v]);
        }
        let mut style = appearance::style("model");
        style.transparent = Some(true);
        style.depth_test = Some(true);
        style.depth_write = Some(false);
        let appearance = Appearance::mesh(bridge, &vertices, style, &[texture])?;
        self.logos.push(Logo {
            name: name.into(),
            appearance,
        });
        Ok(())
    }

    pub fn new(
        bridge: &mut Bridge,
        models: &[crate::menu_models::Model],
        grid: Option<u32>,
    ) -> Result<Self, String> {
        // Plane, CircleGeometry(96), the six-sided projector, the open beam
        // and column, and GridRoom's box use the metre dimensions.
        let mut vertices = Vec::new();
        triangle(
            &mut vertices,
            [[-1., -1., 0.], [1., -1., 0.], [-1., 1., 0.]],
            [[0., 0.], [1., 0.], [0., 1.]],
        );
        triangle(
            &mut vertices,
            [[1., -1., 0.], [1., 1., 0.], [-1., 1., 0.]],
            [[1., 0.], [1., 1.], [0., 1.]],
        );
        let plane = (0, 6);
        let first = vertices.len() / 5;
        for i in 0..96 {
            let a = i as f64 / 96. * std::f64::consts::TAU;
            let b = (i + 1) as f64 / 96. * std::f64::consts::TAU;
            triangle(
                &mut vertices,
                [[0.; 3], [a.cos(), a.sin(), 0.], [b.cos(), b.sin(), 0.]],
                [
                    [0.5, 0.5],
                    [(a.cos() + 1.) * 0.5, (a.sin() + 1.) * 0.5],
                    [(b.cos() + 1.) * 0.5, (b.sin() + 1.) * 0.5],
                ],
            );
        }
        let circle = (first as i32, 288);
        let base = cylinder(&mut vertices, 0.3, 0.36, 0.07, 6, true);
        let cone = cylinder(&mut vertices, 0., 0.26, 0.34, 6, true);
        let beam = cylinder(&mut vertices, 1., 0.05, 1., 48, false);
        let column = cylinder(&mut vertices, 0.3, 0.5, 2.35, 64, false);
        let first = vertices.len() / 5;
        for (a, b, c, d) in [
            (
                [-11., 0., -13.],
                [11., 0., -13.],
                [-11., 9., -13.],
                [11., 9., -13.],
            ),
            (
                [-11., 0., 13.],
                [-11., 0., -13.],
                [-11., 9., 13.],
                [-11., 9., -13.],
            ),
            (
                [11., 0., -13.],
                [11., 0., 13.],
                [11., 9., -13.],
                [11., 9., 13.],
            ),
            (
                [-11., 0., 13.],
                [11., 0., 13.],
                [-11., 0., -13.],
                [11., 0., -13.],
            ),
            (
                [-11., 9., -13.],
                [11., 9., -13.],
                [-11., 9., 13.],
                [11., 9., 13.],
            ),
        ] {
            triangle(&mut vertices, [a, b, c], [[0., 0.], [1., 0.], [0., 1.]]);
            triangle(&mut vertices, [b, d, c], [[1., 0.], [1., 1.], [0., 1.]]);
        }
        let room = (first as i32, (vertices.len() / 5 - first) as i32);
        let pin_ring = ring(&mut vertices, 0.075, 0.09, 40, std::f64::consts::TAU * 0.8, 0.);
        let pin_wave = ring(&mut vertices, 0.08, 0.095, 48, std::f64::consts::TAU, 0.);
        // RingGeometry(0.2, 0.26, 6) turned a twelfth so its corners meet the hexagonal base's.
        let band = ring(&mut vertices, 0.2, 0.26, 6, std::f64::consts::TAU, std::f64::consts::FRAC_PI_6);
        let texture = {
            let gl = &bridge.r.core.gl;
            unsafe {
                let t = bridge.r.core.hold_texture(gl.create_texture()?);
                gl.bind_texture(glow::TEXTURE_2D, Some(t));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    1024,
                    1024,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                for key in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, key, glow::LINEAR as i32);
                }
                for key in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D, key, glow::CLAMP_TO_EDGE as i32);
                }
                t
            }
        };
        let target = unsafe {
            let gl = &bridge.r.core.gl;
            // Deleted with the battle's renderer, also when a later step fails.
            let f = bridge.r.core.hold_framebuffer(gl.create_framebuffer()?);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err("spawn map framebuffer is incomplete".into());
            }
            f
        };
        let map = bridge.r.core.add_texture(texture) as i32;
        let stage = textures::stage();
        let ring_texture = appearance::texture(bridge, 512, 512, &stage.ring)? as i32;
        let rim_texture = appearance::texture(bridge, 1024, 1024, &stage.rim)? as i32;
        let dot = appearance::texture(bridge, 256, 256, &stage.dot)? as i32;
        let pin_texture = appearance::texture(bridge, 64, 64, &stage.pin)? as i32;
        let mut mesh =
            |m, style, maps: &[i32]| Appearance::mesh(bridge, &expanded(&vertices, m), style, maps);
        let vignette = mesh(
            plane,
            {
                let mut s = appearance::style("vignette");
                s.transparent = Some(true);
                s
            },
            &[],
        )?;
        let room = grid
            .map(|grid| mesh(room, appearance::style("room"), &[grid as i32]))
            .transpose()?;
        let floor = mesh(circle, additive("floor"), &[])?;
        let ring = mesh(plane, additive("basic"), &[ring_texture])?;
        let inner = ring.clone();
        let glow = mesh(plane, additive("basic"), &[dot])?;
        let column = mesh(column, additive("column"), &[])?;
        let face = mesh(circle, additive("face"), &[map])?;
        let back = mesh(
            circle,
            {
                let mut s = appearance::style("basic");
                s.transparent = Some(true);
                s
            },
            &[],
        )?;
        let rim = mesh(plane, additive("basic"), &[rim_texture])?;
        let beam = mesh(beam, additive("beam"), &[])?;
        // The projector is solid: depth-tested and depth-writing, so its near faces hide its far ones.
        let metal = || {
            let mut s = appearance::style("metal");
            s.depth_test = Some(true);
            s.depth_write = Some(true);
            s
        };
        let base = mesh(base, metal(), &[])?;
        let cone = mesh(cone, metal(), &[])?;
        let lamp = mesh(plane, depth_tested(additive("basic")), &[dot])?;
        let pool = lamp.clone();
        let band = mesh(band, depth_tested(additive("basic")), &[])?;
        let pin = mesh(plane, additive("basic"), &[pin_texture])?;
        let pin_dot = mesh(plane, additive("basic"), &[dot])?;
        let pin_ring = mesh(pin_ring, additive("basic"), &[])?;
        let pin_wave = mesh(pin_wave, additive("basic"), &[])?;
        let mut random = jsrt::rng::ClassicRandom::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0., |t| t.as_nanos() as u32 as f64),
        );
        let motes = std::array::from_fn(|i| Mote {
            angle: random.uniform() * std::f64::consts::TAU,
            radius: 0.15 + random.uniform() * 0.4,
            height: i as f64 / 40. * 2.35,
            speed: 0.25 + random.uniform() * 0.45,
        });
        let mote_vertices = [0.; 40 * 8];
        let mut mote_style = additive("points");
        mote_style.size = Some(0.06);
        mote_style.depth_test = Some(true);
        let mote_appearance = Appearance::mesh(bridge, &mote_vertices, mote_style, &[dot])?;
        let mote_geometry =
            bridge.r.interface.resources.descriptors[mote_appearance.handle].geometry;
        let mut logos = Vec::new();
        for model in models {
            // PageView.ensureLogos dresses each authored part with the
            // interface's lit, normal-alpha material, including depth writes.
            let mut style = appearance::style("model");
            style.transparent = Some(true);
            style.depth_test = Some(true);
            style.depth_write = Some(true);
            style.scroll = Some(model.scroll.map(f64::from));
            let appearance =
                Appearance::mesh(bridge, &model.vertices, style, &[model.texture as i32])?;
            logos.push(Logo {
                name: model.name.clone(),
                appearance,
            });
        }
        Ok(Self {
            target,
            vignette,
            room,
            floor,
            ring,
            inner,
            glow,
            column,
            face,
            back,
            rim,
            beam,
            base,
            cone,
            lamp,
            pool,
            band,
            pin,
            pin_dot,
            pin_ring,
            pin_wave,
            logos,
            motes,
            mote_vertices,
            mote_geometry,
            mote_appearance,
            random,
        })
    }
    fn submit(bridge: &mut Bridge, count: usize) {
        bridge.r.core.state.reset();
        bridge.r.interface.draw(&mut bridge.r.core, count);
    }
    pub fn before(
        &self,
        bridge: &mut Bridge,
        rig: &Rig,
        aspect: f64,
        look: usize,
        strength: f64,
        dip: f64,
    ) {
        let mut count = 0;
        let identity = Matrix4::default();
        if let (0, Some(room)) = (look, &self.room) {
            let mut l = room.look;
            l[L::TIME] = rig.time;
            l[L::SCROLL] = 0.0625;
            l[L::STRENGTH] = strength;
            l[L::TINT..L::TINT + 3].fill(1.);
            l[L::PULSE..L::PULSE + 3].copy_from_slice(&[130. / 255., 205. / 255., 1.]);
            room.simple(
                bridge,
                &mut count,
                &identity,
                &rig.inverse,
                &rig.projection,
                &l,
            );
        } else {
            let mut l = self.vignette.look;
            l[L::STRENGTH] = strength;
            l[L::ASPECT] = aspect;
            let pivot = rig.pivot_clip();
            l[L::CENTER..L::CENTER + 2]
                .copy_from_slice(&[(pivot[0] + 1.) * 0.5, (pivot[1] + 1.) * 0.5]);
            l[L::DIP] = dip * strength;
            l[L::SHADE..L::SHADE + 3].copy_from_slice(&if look == 2 {
                [2. / 255., 3. / 255., 6. / 255.]
            } else {
                [4. / 255., 8. / 255., 18. / 255.]
            });
            l[L::POOL..L::POOL + 3].copy_from_slice(&if look == 2 {
                [30. / 255., 18. / 255., 8. / 255.]
            } else {
                [16. / 255., 28. / 255., 54. / 255.]
            });
            self.vignette
                .simple(bridge, &mut count, &identity, &identity, &identity, &l);
            let mut floor = Matrix4::default();
            floor.make_rotation_x(-std::f64::consts::FRAC_PI_2);
            floor.scale(&V::new(7.5, 7.5, 7.5));
            floor.elements[13] = -0.005;
            let mut l = self.floor.look;
            // The looks' --bfe-spawn-floor: rgb(255,150,50) for Eternal, rgb(110,160,255) for BF2.
            l[L::COLOR..L::COLOR + 3].copy_from_slice(&if look == 2 {
                [1., 150. / 255., 50. / 255.]
            } else {
                [110. / 255., 160. / 255., 1.]
            });
            l[L::TIME] = rig.time;
            l[L::STRENGTH] = strength;
            self.floor.simple(
                bridge,
                &mut count,
                &floor,
                &rig.inverse,
                &rig.projection,
                &l,
            );
        }
        Self::submit(bridge, count);
    }
    pub fn projector(
        &mut self,
        bridge: &mut Bridge,
        rig: &Rig,
        look: usize,
        strength: f64,
        pulse: f64,
        dt: f64,
    ) {
        let mut count = 0;
        let tint = if look == 2 {
            [1., 160. / 255., 50. / 255.]
        } else {
            [90. / 255., 180. / 255., 1.]
        };
        for (appearance, size, y, opacity, turn) in [
            (&self.glow, 2.8, 0.004, 0.5 * (1. + 1.4 * pulse), 0.),
            (
                &self.ring,
                1.5,
                0.008,
                0.9 * (1. + 1.4 * pulse).min(1.6),
                rig.time * 0.35,
            ),
            (&self.inner, 1.5 * 0.62, 0.012, 0.55, -rig.time * 0.35 * 1.7),
        ] {
            let mut m = Matrix4::default();
            m.make_rotation_x(-std::f64::consts::FRAC_PI_2);
            let mut spin = Matrix4::default();
            spin.make_rotation_z(turn);
            m.multiply(&spin).scale(&V::new(size * 0.5, size * 0.5, 1.));
            m.elements[13] = y;
            let mut l = appearance.look;
            l[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
            l[L::OPACITY] = opacity * strength;
            appearance.simple(bridge, &mut count, &m, &rig.inverse, &rig.projection, &l);
        }
        let mut l = self.column.look;
        l[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
        l[L::TIME] = rig.time;
        l[L::STRENGTH] = strength * (0.5 + 0.9 * pulse);
        self.column.simple(
            bridge,
            &mut count,
            &Matrix4::default(),
            &rig.inverse,
            &rig.projection,
            &l,
        );
        // SpawnStage.update changes only forty retained point positions.
        // The renderer draws them as one PointsMaterial submission.
        for (i, mote) in self.motes.iter_mut().enumerate() {
            mote.height += mote.speed * dt * (1. + 2. * pulse);
            if mote.height > 2.35 * 0.9 {
                mote.height = 0.;
                mote.angle = self.random.uniform() * std::f64::consts::TAU;
            }
            mote.angle += dt * 0.4;
            self.mote_vertices[i * 8] = (mote.angle.cos() * mote.radius) as f32;
            self.mote_vertices[i * 8 + 1] = mote.height as f32;
            self.mote_vertices[i * 8 + 2] = (mote.angle.sin() * mote.radius) as f32;
        }
        bridge.r.core.geometry_buffer_sub_data(
            self.mote_geometry,
            0,
            0,
            appearance::float_bytes(&self.mote_vertices),
        );
        let mut look = self.mote_appearance.look;
        look[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
        look[L::OPACITY] = 0.85 * strength;
        self.mote_appearance.simple(
            bridge,
            &mut count,
            &Matrix4::default(),
            &rig.inverse,
            &rig.projection,
            &look,
        );
        Self::submit(bridge, count);
    }
    /// The hologram: the projector rising out of the floor, its beam, the disc unfolding and
    /// the posts' pins, in one submission.
    #[allow(clippy::too_many_arguments)]
    pub fn map(
        &self,
        bridge: &mut Bridge,
        rig: &Rig,
        look: usize,
        strength: f64,
        flare: f64,
        pins: &[Pin],
    ) {
        let mut count = 0;
        let (v, p) = (&rig.inverse, &rig.projection);
        let s = strength;
        let t = rig.time;
        let tint = if look == 2 {
            [1., 178. / 255., 90. / 255.]
        } else if look == 0 {
            [205. / 255., 232. / 255., 1.]
        } else {
            [130. / 255., 185. / 255., 1.]
        };
        let foot = V::new(rig.center.x, 0., rig.center.z + rig.radius * 0.25);
        // The projector group rises out of the floor as it lights: scale y 0.05..1.
        let rise = 0.05 + 0.95 * s;
        let at = |y: f64| {
            let mut m = Matrix4::default();
            m.make_translation(foot.x, y * rise, foot.z);
            m
        };
        let squash = |m: &mut Matrix4| {
            m.scale(&V::new(1., rise, 1.));
        };
        let mut l = self.pool.look;
        l[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
        l[L::OPACITY] = 0.35 * s;
        let mut pool = at(0.003);
        let mut flat = Matrix4::default();
        flat.make_rotation_x(-std::f64::consts::FRAC_PI_2);
        pool.multiply(&flat).scale(&V::new(0.8, 0.8, 1.));
        self.pool.simple(bridge, &mut count, &pool, v, p, &l);
        let mut metal = self.base.look;
        metal[L::COLOR..L::COLOR + 3].copy_from_slice(&[0.2, 0.23, 0.27]);
        metal[L::RIM..L::RIM + 3].copy_from_slice(&tint);
        let mut base = at(0.);
        squash(&mut base);
        self.base.simple(bridge, &mut count, &base, v, p, &metal);
        let cone = look == 1;
        let lamp = if cone { 0.42 } else { 0.09 };
        if cone {
            let mut m = at(0.07);
            squash(&mut m);
            self.cone.simple(bridge, &mut count, &m, v, p, &metal);
        }
        let mut l = self.band.look;
        l[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
        l[L::OPACITY] = (0.6 + 0.3 * (t * 2.3).sin()) * s;
        let mut band = at(0.072);
        band.multiply(&flat);
        self.band.simple(bridge, &mut count, &band, v, p, &l);
        let source = V::new(foot.x, lamp, foot.z);
        let mut up = rig.center;
        up.sub(&source);
        let length = up.length();
        up.normalize();
        let mut q = Quaternion::default();
        q.set_from_unit_vectors(&V::new(0., 1., 0.), &up);
        let mut beam = Matrix4::default();
        beam.compose(
            &source,
            &q,
            &V::new(rig.radius * 0.9, length, rig.radius * 0.9),
        );
        let mut l = self.beam.look;
        l[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
        l[L::TIME] = t;
        l[L::STRENGTH] = s * (1. + 1.5 * flare);
        self.beam.simple(bridge, &mut count, &beam, v, p, &l);
        // The disc unfolds as it lights: 0.6..1 of its radius.
        let mut disc = rig.map;
        let unfold = 0.6 + 0.4 * s;
        disc.scale(&V::new(unfold, unfold, unfold));
        let mut l = self.back.look;
        l[L::COLOR..L::COLOR + 3].copy_from_slice(&if look == 2 {
            [12. / 255., 8. / 255., 4. / 255.]
        } else if look == 0 {
            [3. / 255., 12. / 255., 36. / 255.]
        } else {
            [6. / 255., 14. / 255., 34. / 255.]
        });
        l[L::OPACITY] = if look == 0 {
            0.7
        } else if look == 2 {
            0.62
        } else {
            0.55
        };
        self.back.simple(bridge, &mut count, &disc, v, p, &l);
        let mut l = self.face.look;
        l[L::TINT..L::TINT + 3].copy_from_slice(&tint);
        l[L::TIME] = t;
        l[L::STRENGTH] = s * (1. + 0.35 * flare);
        let lift = |m: &Matrix4, z: f64| {
            let mut out = *m;
            let mut tz = Matrix4::default();
            tz.make_translation(0., 0., z);
            out.multiply(&tz);
            out
        };
        self.face.simple(bridge, &mut count, &lift(&disc, 0.002), v, p, &l);
        let mut rim = lift(&disc, 0.004);
        let mut turn = Matrix4::default();
        turn.make_rotation_z(-t * 0.08);
        rim.multiply(&turn).scale(&V::new(1. / 0.86, 1. / 0.86, 1.));
        let mut l = self.rim.look;
        l[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
        l[L::OPACITY] = 0.75 * s;
        self.rim.simple(bridge, &mut count, &rim, v, p, &l);
        let mut light = Matrix4::default();
        let lamp_at = V::new(source.x, lamp * rise, source.z);
        light
            .look_at(&rig.position, &lamp_at, &V::new(0., 1., 0.))
            .scale(&V::new(0.21, 0.21 * rise, 1.))
            .set_position_v3(&lamp_at);
        let mut l = self.lamp.look;
        l[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
        l[L::OPACITY] = (0.8 + 0.2 * (t * 3.1).sin()) * s;
        self.lamp.simple(bridge, &mut count, &light, v, p, &l);
        for pin in pins {
            self.pin(bridge, &mut count, rig, &disc, pin);
        }
        Self::submit(bridge, count);
    }
    // addPin/pinPosts: two crossed planes of light standing out of the disc (its +z), a soft dot
    // at the foot, and for the picked post a turning ring; a ripple running out when it is picked.
    fn pin(&self, bridge: &mut Bridge, count: &mut usize, rig: &Rig, disc: &Matrix4, pin: &Pin) {
        let (v, p) = (&rig.inverse, &rig.projection);
        let t = rig.time;
        let mut root = *disc;
        let mut at = Matrix4::default();
        at.make_translation(pin.x, pin.y, 0.006);
        root.multiply(&at);
        let color = |look: &[f64; L::LEN], opacity: f64| {
            let mut l = *look;
            l[L::COLOR..L::COLOR + 3].copy_from_slice(&pin.color);
            l[L::OPACITY] = opacity;
            l
        };
        let pulse = if pin.picked { 1. + 0.18 * (t * 5.).sin() } else { 1. };
        let half_w = 0.02 * if pin.picked { 1.5 } else { 1. };
        let h = (pin.height * pulse).max(1e-3);
        let look = color(&self.pin.look, 1.);
        // The plane's x across, its y (the light's foot to its tip) out of the disc.
        for across in [[half_w, 0., 0.], [0., half_w, 0.]] {
            let mut m = Matrix4::default();
            m.elements = [
                across[0], across[1], 0., 0.,
                0., 0., h * 0.5, 0.,
                -across[1] / half_w, across[0] / half_w, 0., 0.,
                0., 0., h * 0.5, 1.,
            ];
            let mut world = root;
            world.multiply(&m);
            self.pin.simple(bridge, count, &world, v, p, &look);
        }
        let dot = if pin.picked { 0.08 } else { 0.045 };
        let mut m = root;
        m.scale(&V::new(dot, dot, 1.));
        self.pin_dot.simple(bridge, count, &m, v, p, &color(&self.pin_dot.look, 1.));
        if pin.picked {
            let mut m = root;
            let mut turn = Matrix4::default();
            turn.make_rotation_z(t * 1.6);
            m.multiply(&turn);
            self.pin_ring.simple(bridge, count, &m, v, p, &color(&self.pin_ring.look, 0.9));
        }
        if pin.wave < 0.7 {
            let k = pin.wave / 0.7;
            let mut m = root;
            let grow = 1. + 6. * k;
            m.scale(&V::new(grow, grow, 1.));
            self.pin_wave
                .simple(bridge, count, &m, v, p, &color(&self.pin_wave.look, (1. - k) * (1. - k)));
        }
    }
    pub fn logos(
        &self,
        bridge: &mut Bridge,
        rig: &Rig,
        l: bfe_shell::page::SpawnLayout,
        own: u8,
        spin: f64,
        grow: f64,
        woken: Option<usize>,
        sides: [&str; 2],
    ) {
        let mut count = 0;
        let mut projection = Matrix4::default();
        // InterfaceCamera: its plane is 1024/(2*tan(pi/6)) out and
        // 768 units tall. A perspective view also preserves the bevels
        // and foreshortening of the original common models at the edges.
        let fov_half = (0.75 * (std::f64::consts::PI / 6.).tan()).atan();
        let near = 1.;
        let top = near * fov_half.tan();
        let aspect = l.width / l.height;
        projection.make_perspective(
            -top * aspect,
            top * aspect,
            top,
            -top,
            near,
            20000.,
            three::CoordinateSystem::WebGL,
        );
        unsafe {
            bridge.r.core.gl.depth_mask(true);
            bridge.r.core.gl.clear(glow::DEPTH_BUFFER_BIT);
        }
        for (i, side) in sides.iter().enumerate() {
            let name = logo_for(side);
            let picked = i as u8 + 1 == own;
            let plate = l.teams[i];
            // The emblem's box beside its plate; the model fills it, the other team's smaller, and both grow
            // in over the swap (renderAfter: size * (own ? 1 : 0.8) * (0.8 + 0.2 * min(t, 1))).
            let boxed = (l.height * 0.074).clamp(46., 80.);
            let awake = woken == Some(i);
            let size = boxed
                * if picked {
                    1.
                } else if awake {
                    0.92
                } else {
                    0.8
                }
                * (0.8 + 0.2 * grow.min(1.));
            let x = if i == 0 {
                plate[0] - l.width * 0.007 - boxed * 0.5
            } else {
                plate[2] + l.width * 0.007 + boxed * 0.5
            };
            let y = (plate[1] + plate[3]) * 0.5;
            let mut model = Matrix4::default();
            model.make_rotation_y(spin * if picked { 1. } else if awake { -2.5 } else { -0.6 });
            let size = size * 768. / l.height;
            model
                .scale(&V::new(size, size, size))
                .set_position_v3(&V::new(
                    (x / l.width * 1024. - 512.) * 0.75 * aspect,
                    (0.5 - y / l.height) * 768.,
                    -1024. / (2. * (std::f64::consts::PI / 6.).tan()),
                ));
            for logo in &self.logos {
                if logo.name != name {
                    continue;
                }
                let mut look = logo.appearance.look;
                look[L::TIME] = rig.time;
                look[L::COLOR..L::COLOR + 3].copy_from_slice(&if picked {
                    [32. / 255., 223. / 255., 32. / 255.]
                } else {
                    [223. / 255., 32. / 255., 32. / 255.]
                });
                look[L::OPACITY] = if picked || awake { 1. } else { 0.55 };
                logo.appearance.simple(
                    bridge,
                    &mut count,
                    &model,
                    &Matrix4::default(),
                    &projection,
                    &look,
                );
            }
        }
        Self::submit(bridge, count);
    }
}
