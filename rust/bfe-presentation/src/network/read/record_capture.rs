//! networkRecord's explicit fixture capture owns semantic values.
//! These values already came from the live receiver; capture performs no
//! wire decoding, baseline reconstruction or display interpolation.
// Explicit fixture capture owns ordinary semantic values. Live transport lends
// native leases and never calls this serializer or reconstructs these objects.
pub(super) fn capture(row:&[f64],bytes:&[u8])->serde_json::Value {
    use crate::network_layout::{net_record as r,net_kind as k};
    use serde_json::json;
    let kind=row[r::KIND]as u32;
    // Ownership facts carry both generations, including an explicit null
    // when the original seatChangesFor publication released a body.
    if kind==k::SEAT{return json!({"id":row[r::ID],"generation":row[r::GENERATION],"seat":if row[r::SEAT_PRESENT]!=0.{
        Some(json!({"vehicleId":row[r::SEAT_VEHICLE_ID],"vehicleGeneration":row[r::SEAT_VEHICLE_GENERATION],"seatIndex":row[r::SEAT_INDEX]}))
    }else{None}});}
    let names=["soldier","droideka","vehicle","turret","item","remote","mount","tombstone","summary","correction","combat"];
    let mut value=json!({"kind":names[kind as usize],"id":row[r::ID],"generation":row[r::GENERATION],"tick":row[r::TICK]});
    let fields=value.as_object_mut().unwrap();
    // Opaque fields retain their original bytes. A capture owns them so
    // later packets and scratch decoders cannot change a fixture result.
    if kind>=k::SUMMARY{fields.insert("schema".into(),json!(row[r::SCHEMA]));fields.insert("payload".into(),json!(bytes));return value;}
    if kind==k::TOMBSTONE{return value;}
    // Mount quantities are already dequantized by mountRecordFromEntry.
    // Keeping that conversion in the live owner prevents a diagnostic twin.
    if kind==k::MOUNT{
        for (name,at) in [("tilt",r::TILT),("roll",r::MOUNT_ROLL),("walkPhase",r::WALK_PHASE),("walkBlend",r::WALK_BLEND)]{fields.insert(name.into(),json!(row[at]));}
        fields.insert("slot".into(),if row[r::SLOT]<0.{json!(null)}else{json!(row[r::SLOT])});
        fields.insert("seats".into(),json!((0..(row[r::SEAT_COUNT]as usize).min(16)).map(|i|json!({"yaw":row[r::SEAT_ANGLES+i*2],"pitch":row[r::SEAT_ANGLES+i*2+1]})).collect::<Vec<_>>()));return value;
    }
    fields.insert("team".into(),json!(row[r::TEAM]));
    // Turret labels are protocol names; building identity replaces class.
    // Numeric aim still comes from the retained native semantic row.
    if kind==k::TURRET{
        fields.insert("building".into(),json!(row[r::BUILDING]));
        fields.insert("state".into(),json!(["unbuilt","built","destroyed"].get(row[r::STATE]as usize)));
        for (name,at) in [("yaw",r::YAW),("pitch",r::PITCH),("gunYaw",r::GUN_YAW),("gunPitch",r::GUN_PITCH),("health",r::HEALTH)]{fields.insert(name.into(),json!(row[at]));}return value;
    }
    fields.insert("classId".into(),json!(row[r::CLASS_ID]));
    // Inactive resources deliberately omit transform and definition fields.
    // Their consumers use absence to distinguish retirement from placement.
    if kind==k::ITEM||kind==k::REMOTE{
        fields.insert("active".into(),json!(row[r::ACTIVE]!=0.));if row[r::ACTIVE]==0.{return value;}
        fields.insert("definitionId".into(),json!(row[r::DEFINITION_ID]));
    }
    fields.insert("position".into(),json!(&row[r::POSITION..r::POSITION+3]));
    fields.insert("velocity".into(),json!(&row[r::VELOCITY..r::VELOCITY+3]));
    // Hull quaternion normalization happened once in the production owner.
    // Capturing the stored result preserves its multiplication order and bits.
    if kind==k::VEHICLE{
        fields.insert("quaternion".into(),json!(&row[r::QUATERNION..r::QUATERNION+4]));
        for (name,at) in [("health",r::HEALTH),("state",r::STATE),("power",r::POWER)]{fields.insert(name.into(),json!(row[at]));}return value;
    }
    fields.insert("yaw".into(),json!(row[r::YAW]));
    if kind==k::ITEM{fields.insert("up".into(),json!(&row[r::UP..r::UP+3]));return value;}
    fields.insert("pitch".into(),json!(row[r::PITCH]));fields.insert("health".into(),json!(row[r::HEALTH]));
    if kind==k::REMOTE{return value;}
    for (name,at) in [("posture",r::POSTURE),("state",r::STATE),("weapon",r::WEAPON),("fireCounter",r::FIRE_COUNTER)]{fields.insert(name.into(),json!(row[at]));}
    if kind==k::SOLDIER{fields.insert("charge".into(),json!(row[r::CHARGE]));}else{
        for (name,at) in [("mode",r::MODE),("time",r::TIME),("roll",r::ROLL),("lean",r::LEAN),("legs",r::LEGS),("shield",r::SHIELD)]{fields.insert(name.into(),json!(row[at]));}
        for (name,at) in [("reverse",r::REVERSE),("dead",r::DEAD),("shieldOn",r::SHIELD_ON)]{fields.insert(name.into(),json!(row[at]!=0.));}
    }
    value
}
