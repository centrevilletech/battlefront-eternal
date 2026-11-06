//! CombatWorld.fireRound and dispense (combat-world.js), using prepared F1
//! handles. The caller commits its current unit before entering the context,
//! so instant hits and depth-first death effects observe the canonical shooter.
use crate::gen::{BeamSource, ItemOwner, OrdnanceOwner};
use crate::ordnance::{
    world::{Shot, World},
    Cx,
};
use crate::store::{DefId, Ref, StrId, Vec3};

pub struct WeaponService {
    pub ordnance: Option<DefId>,
    pub item: Option<DefId>,
    pub channel: f64,
    pub lock: super::targeting::Lock,
    // Text ids are installed by the content loader before a phase begins.
    // Presentation never interns a bank or lightning name in a weapon loop.
    pub bank: StrId,
    pub lightning: StrId,
    pub has_lightning: bool,
}

impl WeaponService {
    #[allow(clippy::too_many_arguments)]
    pub fn fire<W: World + ?Sized>(
        &self,
        cx: &mut Cx<'_, W>,
        from: Vec3,
        direction: Vec3,
        owner: OrdnanceOwner,
        target_id: f64,
        target_generation: f64,
    ) -> Option<Shot> {
        let def = self.ordnance.expect("a fired weapon has prepared ordnance");
        cx.fire_round(
            def,
            from,
            direction,
            owner,
            None,
            None,
            BeamSource::Null,
            self.channel,
            Some(target_id),
            Some(target_generation),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dispense<W: World + ?Sized>(
        &self,
        cx: &mut Cx<'_, W>,
        key: StrId,
        work: &mut Vec<Ref<crate::gen::Item>>,
        spare: &mut Vec<Ref<crate::gen::Item>>,
        most: f64,
        origin: Vec3,
        velocity: Vec3,
        yaw: f64,
        owner: ItemOwner,
        team: f64,
    ) -> bool {
        let def = self.item.expect("a dispenser has a prepared item");
        cx.dispense_prepared(key, work, spare, most, def, origin, velocity, yaw, owner, team)
            .is_some()
    }
}
