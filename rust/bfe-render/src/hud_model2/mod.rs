//! Compile the prepared viewport once. Coordinates use the authored 4:3 interface plane.
//!
//! Wide screens keep the plane at its true size. Each top-level group of the definition is
//! pinned by where the definition puts it: a group in the left third of the plane follows the
//! left screen edge, one in the right third the right edge, the rest stay centred; the same
//! thirds pin it to the top, the middle or the bottom.
pub mod catalog;
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, io::Read};

pub const PLANE: [f64; 2] = [1024., 768.];

/// The screen edge a point of the plane is pinned to on each axis: 0 = left or top, 0.5 =
/// centre, 1 = right or bottom.
pub fn pin(at: [f64; 2]) -> [f64; 2] {
    std::array::from_fn(|i| {
        let third = PLANE[i] / 3.;
        if at[i] < third {
            0.
        } else if at[i] > 2. * third {
            1.
        } else {
            0.5
        }
    })
}

#[derive(Deserialize)]
struct Tree {
    file: String,
    entries: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    name: String,
    values: Vec<Value>,
    children: Option<Vec<Node>>,
}

impl Node {
    fn property(&self, key: &str) -> &[Value] {
        self.children
            .as_deref()
            .unwrap_or_default()
            .iter()
            .find(|n| n.children.is_none() && n.name.eq_ignore_ascii_case(key))
            .map_or(&[], |n| n.values.as_slice())
    }

    fn word(&self, key: &str) -> &str {
        self.property(key)
            .first()
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    fn numbers<const N: usize>(&self, key: &str, default: [f64; N]) -> Result<[f64; N], String> {
        let values = self.property(key);
        if values.is_empty() {
            return Ok(default);
        }
        if values.len() < N {
            return Err(format!("incomplete {key}"));
        }
        let mut out = default;
        for (v, slot) in values.iter().zip(&mut out) {
            *slot = v
                .as_f64()
                .filter(|v| v.is_finite() && v.abs() <= 1e7)
                .ok_or_else(|| format!("invalid {key}"))?;
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub bitmap: String,
    pub size: [f64; 2],
    pub color: [f64; 4],
    pub has_color: bool,
}

#[derive(Clone, Debug)]
pub struct Widget {
    pub name: String,
    pub kind: String,
    pub at: [f64; 2],
    pub scale: [f64; 2],
    pub rect: [f64; 4],
    pub uv: [f64; 4],
    pub color: [f64; 4],
    pub bitmap: String,
    pub mesh: String,
    pub mask: String,
    /// Authored static text key or literal on a text widget.
    pub text: String,
    /// A text widget's box (`TextBox`) on the plane, or zero.
    pub text_box: [f64; 2],
    /// Colour pulse rate in cycles per second when authored (`ColorPulseRate`).
    pub pulse: f64,
    /// Fade in, hold and fade out in seconds following the enable event.
    pub fade: [f64; 3],
    /// Additional sustain time (`FadeSustainTime`) in seconds before fade out.
    pub sustain: f64,
    pub font: String,
    pub align: u32,
    pub value: String,
    pub enable: String,
    pub tint: String,
    pub segments: u16,
    pub angles: [f64; 2],
    /// True when the widget is mirrored horizontally (180 degree Y rotation).
    pub mirror_x: bool,
    pub segment: Option<Segment>,
    pub z: f64,
    /// The model widget a `MeshInfo` placement belongs to.
    pub owner: String,
    /// The enclosing element, by index (see [`Layout::siblings`]).
    pub parent: Option<usize>,
    /// The product of the enclosing elements' colours, which tints this element's own or
    /// event-driven colour.
    pub inherit: [f64; 3],
    /// Vertical text alignment (0 top, 1 bottom, 2 centre).
    pub valign: u32,
    /// The text is drawn with a drop shadow (`TextStyle Shadow`).
    pub shadow: bool,
    /// The screen edges this widget follows, from its top-level group (see [`pin`]).
    pub pin: [f64; 2],
    /// A bordered box's `Border`: how far its background reaches past its rect on each side,
    /// on the plane (the background is then drawn as nine pieces, see `Painter::bordered`).
    pub border: [f64; 2],
    /// A multiline list's `NumLines`: the rows it shows, or 0 when not authored.
    pub lines: u16,
}

#[derive(Clone, Copy)]
struct Placement {
    at: [f64; 2],
    scale: [f64; 2],
    alpha: f64,
    pin: [f64; 2],
    rgb: [f64; 3],
}

fn measure(values: &[Value], unit: usize, v: [f64; 2]) -> [f64; 2] {
    if values
        .get(unit)
        .and_then(Value::as_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("Viewport"))
    {
        [v[0] * PLANE[0], v[1] * PLANE[1]]
    } else {
        v
    }
}

fn alignment(word: Option<&str>) -> f64 {
    match word {
        Some(s) if s.eq_ignore_ascii_case("center") => 0.5,
        Some(s) if s.eq_ignore_ascii_case("right") || s.eq_ignore_ascii_case("bottom") => 1.,
        _ => 0.,
    }
}

fn tree(reader: impl Read, file: &str) -> Result<Tree, String> {
    let mut bytes = Vec::new();
    reader
        .take((4 << 20) + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 << 20 {
        return Err("HUD tree exceeds size limit".into());
    }
    let t: Tree = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if t.file != file {
        return Err(format!("expected {file}"));
    }
    Ok(t)
}

// Names and event identifiers in authored definitions are ASCII case-insensitive.
fn folded(a: &str, b: &str) -> std::cmp::Ordering {
    a.bytes()
        .map(|b| b.to_ascii_lowercase())
        .cmp(b.bytes().map(|b| b.to_ascii_lowercase()))
}

#[derive(Clone, Copy)]
struct Binding {
    widget: usize,
    enable: bool,
}

impl Binding {
    fn signal(self, widget: &Widget) -> &str {
        if self.enable {
            &widget.enable
        } else {
            &widget.value
        }
    }
}

#[derive(Default)]
pub struct Layout {
    widgets: Vec<Widget>,
    by_name: Vec<usize>,
    bindings: Vec<Binding>,
    ramps: HashMap<String, Vec<[f64; 4]>>,
    output_ramps: Vec<(String, Vec<[f64; 4]>)>,
    mesh_maps: HashMap<String, HashMap<String, String>>,
    pub spawn_map: Option<[f64; 2]>,
}

impl Layout {
    pub fn read(view: impl Read, transforms: impl Read) -> Result<Self, String> {
        let mut layout = Self::default();
        let origin = Placement {
            at: [0.; 2],
            scale: [1.; 2],
            alpha: 1.,
            pin: [0.5; 2],
            rgb: [1.; 3],
        };
        for node in &tree(view, "1playerhud")?.entries {
            layout.visit(node, origin, "", 0)?;
        }
        for node in &tree(transforms, "hudtransforms")?.entries {
            layout.transform(node, 0)?;
        }
        let widgets = &layout.widgets;
        layout.by_name = (0..widgets.len()).collect();
        layout
            .by_name
            .sort_by(|&a, &b| folded(&widgets[a].name, &widgets[b].name));
        // Value bindings precede enable bindings. Ties follow file order.
        for enable in [false, true] {
            for (widget, w) in widgets.iter().enumerate() {
                let entry = Binding { widget, enable };
                if !entry.signal(w).is_empty() {
                    layout.bindings.push(entry);
                }
            }
        }
        layout.bindings.sort_by(|a, b| {
            let (wa, wb) = (&widgets[a.widget], &widgets[b.widget]);
            folded(&wa.kind, &wb.kind).then_with(|| folded(a.signal(wa), b.signal(wb)))
        });
        Ok(layout)
    }

    fn visit(
        &mut self,
        node: &Node,
        parent: Placement,
        font: &str,
        depth: u32,
    ) -> Result<(), String> {
        self.visit_in(node, parent, font, depth, "", None)
    }

    fn visit_in(
        &mut self,
        node: &Node,
        parent: Placement,
        font: &str,
        depth: u32,
        owner: &str,
        up: Option<usize>,
    ) -> Result<(), String> {
        let Some(children) = node.children.as_deref() else {
            return Ok(());
        };
        if depth > 48 || self.widgets.len() >= 4096 {
            return Err("too many HUD elements".into());
        }
        let offset = measure(
            node.property("position"),
            3,
            node.numbers("position", [0.; 2])?,
        );
        let at = std::array::from_fn(|i| parent.at[i] + offset[i] * parent.scale[i]);
        let local = node.numbers("Scale", [1.; 2])?;
        let scale = std::array::from_fn(|i| parent.scale[i] * local[i]);
        let rgb = node
            .numbers("color", [255.; 3])?
            .map(|c| (c / 255.).clamp(0., 1.));
        let alpha = (node.numbers("alpha", [1.])?[0] * parent.alpha).clamp(0., 1.);
        let pin = if depth == 0 { pin(at) } else { parent.pin };
        let mut placement = Placement {
            at,
            scale,
            alpha,
            pin,
            rgb: std::array::from_fn(|i| parent.rgb[i] * rgb[i]),
        };
        let key = if node.property("BitmapRect").is_empty() {
            "Rect"
        } else {
            "BitmapRect"
        };
        let rect = node.property(key);
        let raw_size = measure(rect, 4, node.numbers(key, [0.; 2])?);
        let rot_y = node.numbers("rotation", [0.; 3])?[1];
        let mirror_x = (rot_y.rem_euclid(360.) - 180.).abs() < 90.;
        let size = [raw_size[0] * scale[0], raw_size[1] * scale[1]];
        let mut xy = at;
        if node.name == "BarSegmented" {
            // Segmented rings scale and mirror around their aligned unscaled top-left corner.
            let ax = alignment(rect.get(2).and_then(Value::as_str));
            let ay = alignment(rect.get(3).and_then(Value::as_str));
            let sign_x = if mirror_x { -1. } else { 1. };
            let cx = parent.at[0]
                + (offset[0] + (-ax + sign_x * 0.5 * local[0]) * raw_size[0]) * parent.scale[0];
            let cy = parent.at[1]
                + (offset[1] + (-ay + 0.5 * local[1]) * raw_size[1]) * parent.scale[1];
            xy = [cx - 0.5 * size[0], cy - 0.5 * size[1]];
        } else {
            for i in 0..2 {
                xy[i] -= alignment(rect.get(2 + i).and_then(Value::as_str)) * size[i];
            }
            // A container's children start at its aligned rect's corner.
            if !rect.is_empty() && ["group", "ObjectiveList", "BorderedBox"].contains(&node.name.as_str()) {
                placement.at = xy;
            }
        }
        let segment = if node.name == "BarSegmented" {
            if let Some(seg) = children
                .iter()
                .find(|c| c.children.is_some() && c.name == "Segment")
            {
                let s_rect = seg.property("BitmapRect");
                let s_raw = measure(s_rect, 4, seg.numbers("BitmapRect", [0.; 2])?);
                let s_local = seg.numbers("Scale", [1.; 2])?;
                let has_color = !seg.property("color").is_empty();
                let s_rgb = if has_color {
                    seg.numbers("color", [255.; 3])?
                        .map(|c| (c / 255.).clamp(0., 1.))
                } else {
                    rgb
                };
                let s_alpha = (seg.numbers("alpha", [1.])?[0] * alpha).clamp(0., 1.);
                Some(Segment {
                    bitmap: seg.word("Bitmap").to_owned(),
                    size: [
                        s_raw[0] * scale[0] * s_local[0],
                        s_raw[1] * scale[1] * s_local[1],
                    ],
                    color: [s_rgb[0], s_rgb[1], s_rgb[2], s_alpha],
                    has_color,
                })
            } else {
                None
            }
        } else {
            None
        };
        let font = if node.word("TextFont").is_empty() {
            font
        } else {
            node.word("TextFont")
        };
        let value = [
            "EventValue",
            "EventNumber",
            "EventText",
            "EventBitmap",
            "EventMesh",
        ]
        .iter()
        .map(|key| node.word(key))
        .find(|v| !v.is_empty())
        .unwrap_or("");
        let name = node.values.first().and_then(Value::as_str).unwrap_or("");
        let index = self.widgets.len();
        let vertical = node.property("TextAlignment").get(1).and_then(Value::as_str);
        if node.name == "map" && !node.property("PositionSpawn").is_empty() {
            self.spawn_map = Some(measure(
                node.property("PositionSpawn"),
                3,
                node.numbers("PositionSpawn", [0.; 2])?,
            ));
        }
        self.widgets.push(Widget {
            name: name.to_owned(),
            kind: node.name.clone(),
            at,
            scale,
            rect: [xy[0], xy[1], size[0], size[1]],
            uv: node.numbers("texcoords", [0., 0., 1., 1.])?,
            color: [
                rgb[0] * parent.rgb[0],
                rgb[1] * parent.rgb[1],
                rgb[2] * parent.rgb[2],
                alpha,
            ],
            // A bordered box names its picture `background`.
            bitmap: if node.name == "BorderedBox" {
                node.word("background")
            } else {
                node.word("Bitmap")
            }
            .to_owned(),
            mesh: node.word("Mesh").to_owned(),
            mask: node.word("Mask").to_owned(),
            text: node.word("text").to_owned(),
            text_box: {
                let [w, h] = node.numbers("TextBox", [0.; 2])?;
                [w * PLANE[0] * scale[0], h * PLANE[1] * scale[1]]
            },
            pulse: node.numbers("ColorPulseRate", [0.])?[0].max(0.),
            fade: [
                node.numbers("FadeInTime", [0.])?[0].max(0.),
                node.numbers("FadeHoldTime", [0.])?[0].max(0.),
                node.numbers("FadeOutTime", [0.])?[0].max(0.),
            ],
            sustain: node.numbers("FadeSustainTime", [0.])?[0].max(0.),
            font: font.to_owned(),
            align: match alignment(Some(node.word("TextAlignment"))) {
                1. => 1,
                0.5 => 2,
                _ => 0,
            },
            value: value.to_owned(),
            enable: node.word("EventEnable").to_owned(),
            tint: node.word("EventColor").to_owned(),
            segments: node.numbers("NumSegments", [0.])?[0].clamp(0., 128.) as u16,
            angles: [
                node.numbers("AngleStart", [0.])?[0],
                node.numbers("AngleEnd", [360.])?[0],
            ],
            mirror_x,
            segment,
            z: node.numbers("ZOrder", [0.])?[0],
            owner: owner.to_owned(),
            parent: up,
            inherit: parent.rgb,
            valign: match alignment(vertical) {
                1. => 1,
                0.5 => 2,
                _ => 0,
            },
            shadow: node.word("TextStyle").eq_ignore_ascii_case("Shadow"),
            pin,
            border: {
                let [bx, by] = node.numbers("Border", [0.; 2])?;
                [bx.max(0.) * scale[0], by.max(0.) * scale[1]]
            },
            lines: node.numbers("NumLines", [0.])?[0].clamp(0., 64.) as u16,
        });
        let owner = if node.name == "Model3D" { name } else { owner };
        for child in children {
            self.visit_in(child, placement, font, depth + 1, owner, Some(index))?;
        }
        Ok(())
    }

    fn transform(&mut self, node: &Node, depth: u32) -> Result<(), String> {
        if depth > 48 {
            return Err("too many transform levels".into());
        }
        let Some(children) = node.children.as_deref() else {
            return Ok(());
        };
        let output = node.word("EventOutput");
        if node.name == "TransformNumberColor" {
            let mut stops = Vec::new();
            for stop in children.iter().filter(|n| n.name == "NumberColor") {
                if stop.values.len() != 4 || stops.len() >= 256 {
                    return Err("invalid colour ramp".into());
                }
                let mut values = [0.; 4];
                for (dst, v) in values.iter_mut().zip(&stop.values) {
                    *dst = v.as_f64().ok_or("invalid colour stop")?;
                }
                stops.push(values);
            }
            stops.sort_by(|a, b| a[0].total_cmp(&b[0]));
            // Several inputs can drive the same output, and one input can drive distinct outputs.
            if !output.is_empty() {
                if let Some((_, existing)) = self
                    .output_ramps
                    .iter_mut()
                    .find(|(key, _)| key.eq_ignore_ascii_case(output))
                {
                    existing.clone_from(&stops);
                } else {
                    self.output_ramps.push((output.into(), stops.clone()));
                }
            }
            self.ramps.insert(node.word("EventInput").into(), stops);
        } else if node.name == "TransformNameMesh" {
            let map = self.mesh_maps.entry(output.into()).or_default();
            for pair in children.iter().filter(|n| n.name == "NameMesh") {
                if let [Value::String(name), Value::String(mesh)] = &pair.values[..] {
                    map.insert(name.to_ascii_lowercase(), mesh.to_ascii_lowercase());
                }
            }
        }
        for child in children {
            self.transform(child, depth + 1)?;
        }
        Ok(())
    }

    pub fn binding(&self, kind: &str, signal: &str) -> Option<&Widget> {
        let compare = |entry: &Binding| {
            let w = &self.widgets[entry.widget];
            folded(&w.kind, kind).then_with(|| folded(entry.signal(w), signal))
        };
        let i = self
            .bindings
            .partition_point(|entry| compare(entry).is_lt());
        let entry = self
            .bindings
            .get(i)
            .filter(|entry| compare(entry).is_eq())?;
        Some(&self.widgets[entry.widget])
    }

    pub fn value_bindings<'a>(
        &'a self,
        kind: &'a str,
        signal: &'a str,
    ) -> impl Iterator<Item = &'a Widget> + 'a {
        let compare = move |entry: &Binding| {
            let w = &self.widgets[entry.widget];
            folded(&w.kind, kind).then_with(|| folded(entry.signal(w), signal))
        };
        let i = self
            .bindings
            .partition_point(|entry| compare(entry).is_lt());
        self.bindings[i..]
            .iter()
            .take_while(move |entry| compare(entry).is_eq())
            .filter(|entry| !entry.enable)
            .map(|entry| &self.widgets[entry.widget])
    }

    /// The other elements of `w`'s enclosing element, in definition order.
    pub fn siblings<'a>(&'a self, w: &'a Widget) -> impl Iterator<Item = &'a Widget> + 'a {
        self.widgets
            .iter()
            .filter(move |o| o.parent == w.parent && !std::ptr::eq(*o, w))
    }

    /// The element directly enclosing `w`.
    pub fn parent(&self, w: &Widget) -> Option<&Widget> {
        w.parent.and_then(|i| self.widgets.get(i))
    }

    /// The first element of kind `kind` directly inside `w`.
    pub fn child<'a>(&'a self, w: &Widget, kind: &str) -> Option<&'a Widget> {
        let index = self.widgets.iter().position(|o| std::ptr::eq(o, w))?;
        self.widgets
            .iter()
            .find(|o| o.parent == Some(index) && o.kind.eq_ignore_ascii_case(kind))
    }

    /// Where a model widget draws one of its meshes (`MeshInfo`).
    pub fn mesh_info(&self, owner: &str, mesh: &str) -> Option<&Widget> {
        self.widgets.iter().find(|w| {
            w.kind == "MeshInfo"
                && w.name.eq_ignore_ascii_case(mesh)
                && w.owner.eq_ignore_ascii_case(owner)
        })
    }

    pub fn named(&self, name: &str) -> Option<&Widget> {
        let i = self
            .by_name
            .partition_point(|&id| folded(&self.widgets[id].name, name).is_lt());
        self.by_name
            .get(i)
            .map(|&id| &self.widgets[id])
            .filter(|w| w.name.eq_ignore_ascii_case(name))
    }

    pub fn mesh<'a>(&'a self, binding: &str, tag: &'a str) -> &'a str {
        self.mesh_maps
            .get(binding)
            .and_then(|map| {
                map.get(tag).or_else(|| {
                    tag.bytes()
                        .any(|b| b.is_ascii_uppercase())
                        .then(|| {
                            map.iter()
                                .find(|(key, _)| key.eq_ignore_ascii_case(tag))
                                .map(|(_, value)| value)
                        })
                        .flatten()
                })
            })
            .map_or(tag, String::as_str)
    }

    /// Whether a colour ramp takes `input` as its input value.
    pub fn has_ramp(&self, input: &str) -> bool {
        self.ramps.get(input).is_some_and(|stops| !stops.is_empty())
    }

    pub fn color(&self, input: &str, value: f64, fallback: [f64; 4]) -> [f64; 4] {
        let Some(stops) = self
            .ramps
            .get(input)
            .or_else(|| {
                self.output_ramps
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(input))
                    .map(|(_, stops)| stops)
            })
            .filter(|s| !s.is_empty())
        else {
            return fallback;
        };
        let i = stops.partition_point(|s| s[0] < value).min(stops.len() - 1);
        let (a, b) = (stops[i.saturating_sub(1)], stops[i]);
        let t = if b[0] > a[0] {
            ((value - a[0]) / (b[0] - a[0])).clamp(0., 1.)
        } else {
            1.
        };
        let mut rgb = fallback;
        for i in 0..3 {
            rgb[i] = ((a[i + 1] + (b[i + 1] - a[i + 1]) * t) / 255.).clamp(0., 1.);
        }
        rgb
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn nested_viewport_and_pixel_positions_and_authored_ramp() {
        let view = json!({"file":"1playerhud","entries":[{"name":"group","values":["hinge"],"children":[
            {"name":"position","values":[0.25,0.5,0,"Viewport"]},{"name":"Scale","values":[0.5,2,1]},
            {"name":"BarBitmap","values":["ink"],"children":[{"name":"position","values":[12,-7,0,"Pixels"]},
                {"name":"BitmapRect","values":[40,8,"Right","Center","Pixels"]},{"name":"EventValue","values":["pen.level"]}]}]}]});
        let ramps = json!({"file":"hudtransforms","entries":[{"name":"TransformNumberColor","values":["chalk"],"children":[
            {"name":"EventInput","values":["pen.level"]},{"name":"NumberColor","values":[0,10,40,30]},
            {"name":"NumberColor","values":[1,30,60,70]}]}]});
        let hud = Layout::read(view.to_string().as_bytes(), ramps.to_string().as_bytes()).unwrap();
        assert_eq!(
            hud.binding("BarBitmap", "pen.level").unwrap().rect,
            [242., 362., 20., 16.]
        );
        assert_eq!(
            hud.color("pen.level", 0.5, [1.; 4]),
            [20. / 255., 50. / 255., 50. / 255., 1.]
        );
    }

    #[test]
    fn a_container_rect_places_children_and_each_group_takes_one_pin() {
        let view = json!({"file":"1playerhud","entries":[
            {"name":"group","values":["shelf"],"children":[
                {"name":"position","values":[0.05,1.2,0,"Viewport"]},
                {"name":"Rect","values":[0.5,0.8,"Left","Center","Viewport"]},
                {"name":"Bitmap","values":["jar"],"children":[{"name":"position","values":[0.1,-0.2,0,"Viewport"]}]}]},
            {"name":"text","values":["lamp"],"children":[{"name":"position","values":[0.62,0.31,0,"Viewport"]}]},
            {"name":"group","values":["vane"],"children":[{"name":"position","values":[0.9,0.5,0,"Viewport"]},
                {"name":"Bitmap","values":["cup"],"children":[{"name":"position","values":[-0.7,0.4,0,"Viewport"]}]}]}]});
        let hud = Layout::read(
            view.to_string().as_bytes(),
            br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
        )
        .unwrap();
        let jar = hud.named("jar").unwrap();
        assert!((jar.at[0] - 0.15 * 1024.).abs() < 1e-9);
        assert!((jar.at[1] - (1.2 - 0.4 - 0.2) * 768.).abs() < 1e-9);
        assert_eq!(hud.named("shelf").unwrap().pin, [0., 1.]);
        assert_eq!(jar.pin, [0., 1.]);
        assert_eq!(hud.named("lamp").unwrap().pin, [0.5, 0.]);
        // A child follows its group even when it is drawn in another third of the plane.
        assert_eq!(hud.named("cup").unwrap().pin, [1., 0.5]);
        // A group's colour tints what it encloses.
        let tinted = json!({"file":"1playerhud","entries":[{"name":"group","values":["dye"],"children":[
            {"name":"color","values":[51,102,255]},
            {"name":"Bitmap","values":["cloth"],"children":[{"name":"color","values":[255,255,51]}]}]}]});
        let hud = Layout::read(
            tinted.to_string().as_bytes(),
            br#"{"file":"hudtransforms","entries":[]}"#.as_slice(),
        )
        .unwrap();
        let cloth = hud.named("cloth").unwrap();
        assert_eq!(cloth.inherit, [0.2, 0.4, 1.]);
        assert!((cloth.color[0] - 0.2).abs() < 1e-12 && (cloth.color[2] - 0.2).abs() < 1e-12);
    }

    #[test]
    fn segmented_rings_keep_distinct_output_ramps_and_mirrored_centres() {
        let view = json!({"file":"1playerhud","entries":[{"name":"group","values":["dial"],"children":[
            {"name":"position","values":[0.4,0.45,0,"Viewport"]},{"name":"Scale","values":[0.6,0.6,1]},
            {"name":"BarSegmented","values":["left"],"children":[
                {"name":"Rect","values":[0.15,0.18,"Center","Center","Viewport"]},
                {"name":"NumSegments","values":[5]},{"name":"AngleStart","values":[-155]},{"name":"AngleEnd","values":[-95]},
                {"name":"EventValue","values":["dial.cool"]},{"name":"EventEnable","values":["dial.cool"]},
                {"name":"EventColor","values":["dial.warmTint"]},
                {"name":"Segment","values":["Segment"],"children":[
                    {"name":"Bitmap","values":["swatch"]},
                    {"name":"BitmapRect","values":[0.014,0.022,"Center","Center","Viewport"]},
                    {"name":"alpha","values":[0.65]}]}]},
            {"name":"BarSegmented","values":["right"],"children":[
                {"name":"position","values":[0.15,0,0,"Viewport"]},{"name":"rotation","values":[0,180,0]},
                {"name":"Rect","values":[0.15,0.18,"Center","Center","Viewport"]},
                {"name":"NumSegments","values":[5]},{"name":"AngleStart","values":[-155]},{"name":"AngleEnd","values":[-95]},
                {"name":"EventValue","values":["dial.cool"]},{"name":"EventEnable","values":["dial.cool"]}]}]}]});
        let ramps = json!({"file":"hudtransforms","entries":[
            {"name":"TransformNumberColor","values":["warm"],"children":[
                {"name":"EventInput","values":["dial.level"]},{"name":"EventOutput","values":["dial.warmTint"]},
                {"name":"NumberColor","values":[0,30,120,210]},{"name":"NumberColor","values":[1,190,80,50]}]},
            {"name":"TransformNumberColor","values":["alert"],"children":[
                {"name":"EventInput","values":["dial.level"]},{"name":"EventOutput","values":["dial.alertTint"]},
                {"name":"NumberColor","values":[0,20,200,100]},{"name":"NumberColor","values":[1,240,30,90]}]}]});
        let hud = Layout::read(view.to_string().as_bytes(), ramps.to_string().as_bytes()).unwrap();
        let bars: Vec<_> = hud.value_bindings("BarSegmented", "dial.cool").collect();
        assert_eq!(bars.len(), 2);
        for bar in &bars {
            let cx = bar.rect[0] + bar.rect[2] * 0.5;
            let cy = bar.rect[1] + bar.rect[3] * 0.5;
            assert!((cx - 0.4 * 1024.).abs() < 1e-6);
            assert!((cy - 0.45 * 768.).abs() < 1e-6);
        }
        assert!(!bars[0].mirror_x && bars[1].mirror_x);
        let seg = bars[0].segment.as_ref().unwrap();
        assert_eq!(seg.bitmap, "swatch");
        assert!((seg.size[0] - 0.014 * 1024. * 0.6).abs() < 1e-6);
        assert_eq!(
            hud.color("dial.warmTint", 0.5, [1.; 4]),
            [110. / 255., 100. / 255., 130. / 255., 1.]
        );
    }
}
