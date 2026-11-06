// Length-delimited source records for shader-corpus indexing. Only the declared
// kind is a runnable catalog variant; alternatives identify wrong-kind captures.
use bfe_render::{
    programs,
    shader::{self, Kind, Stage},
};
use std::io::{self, Write};

fn main() {
    let mut out = io::BufWriter::new(io::stdout().lock());
    if std::env::args().any(|arg| arg == "--instance-layout") {
        write!(out, "{{\"texels\":{},\"fields\":{{", shader::ROW_TEXELS).unwrap();
        let fields = [
            ("modelMatrix", shader::ROW_MODEL, 4, 4),
            ("modelViewMatrix", shader::ROW_MODEL_VIEW, 4, 4),
            ("normalMatrix", shader::ROW_NORMAL, 3, 3),
            ("bindMatrixInverse", shader::ROW_BIND_INVERSE, 4, 4),
            ("bindMatrix", shader::ROW_BIND, 4, 4),
            ("previousModel", shader::ROW_PREVIOUS_MODEL, 4, 4),
            ("previousBindInverse", shader::ROW_PREVIOUS_BIND_INVERSE, 4, 4),
        ];
        for (i, (name, texel, columns, rows)) in fields.iter().enumerate() {
            if i != 0 { write!(out, ",").unwrap(); }
            write!(out, "\"{name}\":{{\"at\":{},\"columns\":{columns},\"rows\":{rows}}}", texel * 4).unwrap();
        }
        writeln!(out, "}}}}").unwrap();
        return;
    }
    let args: Vec<_> = std::env::args().collect();
    let family = args.windows(2).find(|a| a[0] == "--family").map(|a| format!("{}/", a[1]));
    for spec in programs::specs() {
        if family.as_ref().is_some_and(|prefix| !spec.key.starts_with(prefix)) { continue; }
        let (vertex, fragment) = programs::sources(spec);
        let attributes = spec
            .attributes
            .iter()
            .map(|(name, at)| format!("{name}:{at}"))
            .collect::<Vec<_>>()
            .join(",");
        for kind in [
            Kind::Plain,
            Kind::World,
            Kind::Effect,
            Kind::Motion,
            Kind::Skinned,
            Kind::Rigid,
        ] {
            let (Ok(v), Ok(f)) = (
                shader::rewrite_lamps(&vertex, Stage::Vertex, kind, spec.lamps),
                shader::rewrite_lamps(&fragment, Stage::Fragment, kind, spec.lamps),
            ) else {
                continue;
            };
            writeln!(
                out,
                "{}\t{:?}\t{}\t{}\t{}\t{}",
                spec.key,
                kind,
                kind == spec.kind,
                v.len(),
                f.len(),
                attributes
            )
            .unwrap();
            out.write_all(v.as_bytes()).unwrap();
            out.write_all(f.as_bytes()).unwrap();
        }
    }
}
