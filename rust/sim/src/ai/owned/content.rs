//! Load-time class projections from generated rows. Scalar types and absence
//! survive the boundary; control and combat retain their different role rules,
//! and navigation keeps numeric size masks distinct from goal size names.
use crate::{
    ai::{
        combat::world::ClassView,
        control::world::ClassSnap,
        nav::units::{ClassDef, SizeType},
    },
    combat::defs::Js,
    gen::{AiClassScalar as Scalar, AiUnitClass},
    store::{DefId, StrId, Strings},
};
use std::rc::Rc;

pub struct UnitClass {
    pub id: DefId,
    pub name: Rc<str>,
    pub policy: ClassSnap,
    pub combat: ClassView,
    pub navigation: ClassDef,
    pub vehicle: crate::ai::vehicles::world::ClassView,
    pub label: Option<Rc<str>>,
}
fn text(strings: &Strings, id: StrId) -> String {
    String::from_utf16_lossy(strings.units(id))
}
fn js(strings: &Strings, value: Scalar) -> Js {
    match value {
        Scalar::Undefined => Js::Undefined,
        Scalar::Null => Js::Null,
        Scalar::Bool(v) => Js::Bool(v),
        Scalar::F64(v) => Js::Num(v),
        Scalar::Str(id) => Js::Str(text(strings, id).into()),
    }
}
fn number(strings: &Strings, value: Scalar) -> Option<f64> {
    let value = js(strings, value);
    (!value.is_nullish()).then(|| value.num())
}
/// classes.quantity reads a present service table before the named fields.
/// Keep empty tables distinct from absence, including rebuild/build health.
pub fn item(row:&crate::gen::AiItemDefinition,strings:&Strings,items:&mut super::items::Items) {
    let values=|input:&[Scalar]|std::array::from_fn(|i|input.get(i).and_then(|value|number(strings,value.clone())));
    let quantity=crate::ai::control::world::QtySnap {
        svc_health:row.service_health.as_ref().map(|input|values(input)),
        svc_ammo:row.service_ammo.as_ref().map(|input|values(input)),
        named_health:values(&row.health),named_ammo:values(&row.ammo),bare:values(&row.bare),
    };
    items.class(row.def,row.base.map(|id|Rc::from(text(strings,id))),row.class_name.map(|id|Rc::from(text(strings,id))),quantity);
}
fn numeric(value: Scalar) -> Option<f64> {
    if let Scalar::F64(v) = value {
        Some(v)
    } else {
        None
    }
}
fn string(strings: &Strings, value: Scalar) -> Option<String> {
    if let Scalar::Str(id) = value {
        Some(text(strings, id))
    } else {
        None
    }
}
/// objectiveVehicleMask uses navigation's trimmed string/integer rule over
/// the authored settings, independently of the target's raw class policy.
pub fn traversal_size(value:Scalar,strings:&Strings)->u32 {
    let size=match value {Scalar::Str(id)=>SizeType::Text(text(strings,id).into()),Scalar::F64(value)=>SizeType::Number(value),_=>SizeType::Missing};
    let mut unit=crate::ai::nav::units::NavUnit::new(0,0.,Default::default());
    unit.class_def=Some(ClassDef {ai_size_type:size,..Default::default()});crate::ai::nav::units::traversal_mask(&unit)
}
impl UnitClass {
    pub fn read(row: &AiUnitClass, strings: &Strings) -> Self {
        let size = match row.size_type {
            Scalar::Str(id) => SizeType::Text(text(strings, id).into()),
            Scalar::F64(v) => SizeType::Number(v),
            _ => SizeType::Missing,
        };
        let bounds = row.bounds.map(|b| [[b[0], b[1], b[2]], [b[3], b[4], b[5]]]);
        let policy = ClassSnap {
            role: numeric(row.role),
            unit_type: string(strings, row.unit_type),
            ai_size_type: size.clone(),
            bounds,
            max_speed: number(strings, row.max_speed),
            max_strafe_speed: number(strings, row.max_strafe_speed),
            strafe_speed: number(strings, row.strafe_speed),
            max_turn_speed: number(strings, row.max_turn_speed),
            max_pitch_speed: number(strings, row.max_pitch_speed),
            forward_speed: number(strings, row.forward_speed),
            turn_rate: number(strings, row.turn_rate),
            pitch_rate: number(strings, row.pitch_rate),
            max_health: number(strings, row.max_health),
            health_type_num: numeric(row.health_type),
            health_type_name: string(strings, row.health_type),
            base: string(strings, row.base),
            ..Default::default()
        };
        let navigation = ClassDef {
            ai_size_type: size,
            bounds,
            max_speed: policy.max_speed,
            max_strafe_speed: policy.max_strafe_speed,
            max_turn_speed: policy.max_turn_speed,
            max_pitch_speed: policy.max_pitch_speed,
            forward_speed: policy.forward_speed,
            turn_rate: policy.turn_rate,
            jet_fuel_min_border: number(strings, row.jet_fuel_min_border),
            jet_fuel_recharge_rate: number(strings, row.jet_fuel_recharge_rate),
        };
        let combat = ClassView {
            fields:std::rc::Rc::new(crate::ai::combat::world::ClassFields {
            forward_speed: js(strings, row.forward_speed),
            max_speed: js(strings, row.max_speed),
            max_health: js(strings, row.max_health),
            health_type: js(strings, row.health_type),
            jet_jump: js(strings, row.jet_jump),
            jet_fuel_min_border: js(strings, row.jet_fuel_min_border),
            unit_type: js(strings, row.unit_type),
            role: js(strings, row.role),
            ai_role: js(strings, row.ai_role),
            }),
            prepared_role: None,
            prepared_category: None,
        };
        let no_enter = match row.no_enter_vehicles {
            Scalar::Undefined | Scalar::Null => 0.,
            Scalar::F64(n) => {
                if n == 0. {
                    0.
                } else {
                    n
                }
            }
            Scalar::Str(id) => bfe_content::js::parse_float(&text(strings, id)),
            Scalar::Bool(_) => f64::NAN,
        };
        let mut vehicle = vehicle_class(&policy, no_enter);
        // vehicleClassView carries the cooked flyer/walker/hover fields that
        // are absent from the infantry control policy. Keep zero and false
        // distinct from a missing authored value, especially moving turns.
        vehicle.moving_turn_only=match row.moving_turn_only {Scalar::Undefined|Scalar::Null=>None,value=>Some(js(strings,value).truthy())};
        vehicle.min_speed=number(strings,row.min_speed);vehicle.max_yaw_speed=number(strings,row.max_yaw_speed);
        vehicle.vehicle_type=match row.vehicle_type {
            Scalar::Undefined|Scalar::Null=>None,Scalar::Str(id)=>Some(text(strings,id).into()),
            Scalar::Bool(value)=>Some(value.to_string().into()),Scalar::F64(value)=>Some(bfe_content::js::number_to_string(value).into()),
        };
        vehicle.landing_time=number(strings,row.landing_time);vehicle.landing_speed=number(strings,row.landing_speed);vehicle.landed_height=number(strings,row.landed_height);
        Self {
            id: row.def,
            name: text(strings, row.name).into(),
            policy,
            combat,
            navigation,
            vehicle,
            label: row.label.map(|id| text(strings, id).into()),
        }
    }
    pub fn install(
        self,
        units: &mut super::units::Units,
        combat: &mut super::combat::Units,
        navigation: &mut super::navigation::Navigation,
    ) -> usize {
        units.class(self.id, self.policy, self.name);
        units.vehicle_class(self.id, self.vehicle, self.label);
        combat.class_view(self.id, self.combat);
        navigation.class(self.navigation)
    }
}

/// Static vehicle-service fields of a raw class. Seat construction can supply
/// its cooked class instead; registry entries retain their original raw class.
pub fn vehicle_class(policy: &ClassSnap, no_enter: f64) -> crate::ai::vehicles::world::ClassView {
    crate::ai::vehicles::world::ClassView {
        ai_size_type: match &policy.ai_size_type {
            SizeType::Text(t) => Some(t.clone()),
            SizeType::Number(n) => Some(bfe_content::js::number_to_string(*n).into()),
            _ => None,
        },
        forward_speed: policy.forward_speed,
        strafe_speed: policy.strafe_speed,
        turn_rate: policy.turn_rate,
        pitch_rate: policy.pitch_rate,
        max_speed: policy.max_speed,
        max_turn_speed: policy.max_turn_speed,
        max_pitch_speed: policy.max_pitch_speed,
        bounds: policy.bounds,
        no_enter_vehicles: no_enter,
        role: crate::ai::control::controller::role_of(policy.role, policy.unit_type.as_deref()),
        ..Default::default()
    }
}
