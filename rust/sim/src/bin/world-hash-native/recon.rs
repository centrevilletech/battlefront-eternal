//! Recorded cold recon classes enter the product host through its typed
//! content boundary, just as native admission supplies F3ReconClass rows.
use bfe_sim::abi::context::Context;
use bfe_sim::gen::F3ReconClass;
use bfe_sim::world::admit::Call;

pub fn admitted(context: &Context, export: &str, call: &Call) -> Result<(), String> {
    if export == "f3_recon_class" {
        let class = F3ReconClass::take(&call.row, &call.heap);
        context
            .world_host_recon_class(class.id, &class.value)
            .map_err(|e| e.message().to_string())?;
    }
    Ok(())
}

/// Game's remoteShot retains the authored class object and raw channel.
/// Its protocol presentation uses a definition id instead of this object.
pub fn event_fields(
    json: &mut super::json::Json,
    definitions: &bfe_sim::host::recon::Definitions,
    record: bfe_sim::host::recon::Record,
) {
    json.key("droid");
    json.raw(definitions.get(record.class).unwrap_or("null"));
    json.key("channel");
    json.number(record.channel);
    json.key("position");
    json.vec3(&record.position);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_definition_expands_as_an_object() {
        // The journal's class identity expands to the authored object,
        // while presentation uses its separate protocol definition id.
        let context = Context::new();
        let mut definitions = bfe_sim::host::recon::Definitions::default();
        definitions.bind(&context, 2, &[7.0, 0.0]).unwrap();
        let record = bfe_sim::host::recon::Record {
            class: 2,
            channel: 2.0,
            position: [1.0, -2.0, 3.5],
        };
        let mut json = super::super::json::Json::new();
        json.begin_object();
        event_fields(&mut json, &definitions, record);
        json.end_object();
        assert_eq!(
            json.text(),
            r#"{"droid":{},"channel":2,"position":[1,-2,3.5]}"#
        );
    }
}
