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
    room: Appearance,
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
    pin: Appearance,
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
        let p = [
            [a.cos() * bottom, 0., a.sin() * bottom],
            [b.cos() * bottom, 0., b.sin() * bottom],
            [a.cos() * top, height, a.sin() * top],
            [b.cos() * top, height, b.sin() * top],
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

impl Gpu {
    pub fn new(
        bridge: &mut Bridge,
        models: &[crate::menu_models::Model],
        grid: u32,
    ) -> Result<Self, String> {
        // Plane, CircleGeometry(96), the six-sided projector, the open beam
        // and column, and GridRoom's box use the browser's metre dimensions.
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
        let texture = {
            let gl = &bridge.r.core.gl;
            unsafe {
                let t = gl.create_texture()?;
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
            let f = gl.create_framebuffer()?;
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
        let ring_texture =
            appearance::texture(bridge, 512, 512, &textures::ring(512, false))? as i32;
        let rim_texture =
            appearance::texture(bridge, 1024, 1024, &textures::ring(1024, true))? as i32;
        let dot = appearance::texture(bridge, 256, 256, &textures::dot(256))? as i32;
        let pin_texture = appearance::texture(bridge, 64, 64, &textures::pin(64))? as i32;
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
        let room = mesh(room, appearance::style("room"), &[grid as i32])?;
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
        let base = mesh(base, appearance::style("metal"), &[])?;
        let cone = mesh(cone, appearance::style("metal"), &[])?;
        let lamp = glow.clone();
        let pin = mesh(plane, additive("basic"), &[pin_texture])?;
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
            pin,
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
    pub fn before(&self, bridge: &mut Bridge, rig: &Rig, aspect: f64, look: usize, strength: f64) {
        let mut count = 0;
        let identity = Matrix4::default();
        let tint = if look == 2 {
            [1., 160. / 255., 50. / 255.]
        } else {
            [90. / 255., 180. / 255., 1.]
        };
        if look == 0 {
            let mut l = self.room.look;
            l[L::TIME] = rig.time;
            l[L::SCROLL] = 0.0625;
            l[L::STRENGTH] = strength;
            l[L::TINT..L::TINT + 3].fill(1.);
            l[L::PULSE..L::PULSE + 3].copy_from_slice(&[130. / 255., 205. / 255., 1.]);
            self.room.simple(
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
            l[L::CENTER..L::CENTER + 2].copy_from_slice(&[0.5, 0.42]);
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
            l[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
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
    pub fn map(&self, bridge: &mut Bridge, rig: &Rig, _aspect: f64, look: usize, strength: f64) {
        let mut count = 0;
        let tint = if look == 2 {
            [1., 178. / 255., 90. / 255.]
        } else if look == 0 {
            [205. / 255., 232. / 255., 1.]
        } else {
            [130. / 255., 185. / 255., 1.]
        };
        let foot = V::new(rig.center.x, 0., rig.center.z + rig.radius * 0.25);
        let mut base = Matrix4::default();
        base.make_translation_v3(&foot);
        let mut metal = self.base.look;
        metal[L::COLOR..L::COLOR + 3].copy_from_slice(&[0.2, 0.23, 0.27]);
        metal[L::RIM..L::RIM + 3].copy_from_slice(&tint);
        self.base.simple(
            bridge,
            &mut count,
            &base,
            &rig.inverse,
            &rig.projection,
            &metal,
        );
        let cone = look == 1;
        let lamp = if cone { 0.42 } else { 0.09 };
        if cone {
            let mut m = base;
            m.elements[13] = 0.07;
            self.cone.simple(
                bridge,
                &mut count,
                &m,
                &rig.inverse,
                &rig.projection,
                &metal,
            );
        }
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
        l[L::TIME] = rig.time;
        l[L::STRENGTH] = strength;
        self.beam
            .simple(bridge, &mut count, &beam, &rig.inverse, &rig.projection, &l);
        let mut l = self.back.look;
        l[L::COLOR..L::COLOR + 3].copy_from_slice(&if look == 2 {
            [12. / 255., 8. / 255., 4. / 255.]
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
        self.back.simple(
            bridge,
            &mut count,
            &rig.map,
            &rig.inverse,
            &rig.projection,
            &l,
        );
        let mut l = self.face.look;
        l[L::TINT..L::TINT + 3].copy_from_slice(&tint);
        l[L::TIME] = rig.time;
        l[L::STRENGTH] = strength;
        let mut face = rig.map;
        let mut translate = Matrix4::default();
        translate.make_translation(0., 0., 0.002);
        face.multiply(&translate);
        self.face
            .simple(bridge, &mut count, &face, &rig.inverse, &rig.projection, &l);
        let mut rim = rig.map;
        rim.scale(&V::new(1. / 0.86, 1. / 0.86, 1.));
        let mut l = self.rim.look;
        l[L::COLOR..L::COLOR + 3].copy_from_slice(&tint);
        l[L::OPACITY] = strength;
        self.rim
            .simple(bridge, &mut count, &rim, &rig.inverse, &rig.projection, &l);
        let mut light = Matrix4::default();
        light
            .look_at(&rig.position, &source, &V::new(0., 1., 0.))
            .scale(&V::new(0.21, 0.21, 1.))
            .set_position_v3(&source);
        self.lamp.simple(
            bridge,
            &mut count,
            &light,
            &rig.inverse,
            &rig.projection,
            &l,
        );
        Self::submit(bridge, count);
    }
    pub fn pin(
        &self,
        bridge: &mut Bridge,
        rig: &Rig,
        x: f64,
        y: f64,
        height: f64,
        color: [f32; 4],
    ) {
        let mut p = V::new(x, y, 0.);
        p.apply_matrix4(&rig.map);
        let mut m = Matrix4::default();
        m.look_at(&rig.position, &p, &V::new(0., 1., 0.));
        m.scale(&V::new(rig.radius * 0.02, rig.radius * height * 0.5, 1.));
        p.y += rig.radius * height * 0.5;
        m.set_position_v3(&p);
        let mut l = self.pin.look;
        for i in 0..3 {
            l[L::COLOR + i] = color[i] as f64;
        }
        l[L::OPACITY] = color[3] as f64;
        let mut count = 0;
        self.pin
            .simple(bridge, &mut count, &m, &rig.inverse, &rig.projection, &l);
        Self::submit(bridge, count);
    }
    pub fn logos(
        &self,
        bridge: &mut Bridge,
        rig: &Rig,
        l: bfe_shell::page::SpawnLayout,
        own: u8,
        spin: f64,
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
            let name = match *side {
                "cis" => "com_icon_cis",
                "imp" => "com_icon_imperial",
                "all" => "com_icon_alliance",
                _ => "com_icon_republic",
            };
            let picked = i as u8 + 1 == own;
            let plate = l.teams[i];
            let size = (l.height * 0.074).clamp(46., 80.) * if picked { 1. } else { 0.8 };
            let x = if i == 0 {
                plate[0] - l.width * 0.007 - size * 0.5
            } else {
                plate[2] + l.width * 0.007 + size * 0.5
            };
            let y = (plate[1] + plate[3]) * 0.5;
            let mut model = Matrix4::default();
            model.make_rotation_y(spin * if picked { 1. } else { -0.6 });
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
                look[L::OPACITY] = if picked { 1. } else { 0.55 };
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
    pub fn dispose(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_framebuffer(self.target);
        }
    }
}
