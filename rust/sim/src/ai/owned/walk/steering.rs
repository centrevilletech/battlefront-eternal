//! A reusable single-waypoint executor for the collision survey.
use crate::ai::nav::{
    analogs::analogs,
    control::MovementRequest,
    executor::{ExecutorHost, PathExecutor},
    math::Vec3,
    planner::{PathNode, PathStore},
    units::{kind, NavUnit, SoldierView},
};
use crate::movement::soldier::{Input, Mover, V3};

#[derive(Default)]
struct Control {
    request: MovementRequest,
    side: f64,
}
impl ExecutorHost for Control {
    fn consume_segment(&mut self) {}
    fn avoid_side(&mut self) -> &mut f64 {
        &mut self.side
    }
    fn write_request(
        &mut self,
        target: Vec3,
        point: Vec3,
        direction: Vec3,
        mode: f64,
        low: f64,
        high: f64,
    ) {
        self.request
            .write(target, point, direction, mode, low, high);
    }
}

pub(super) struct Steering {
    store: PathStore,
    path: usize,
    unit: NavUnit,
    control: Control,
}

impl Steering {
    pub fn new() -> Self {
        let mut store = PathStore::new(1., 1.);
        let path = store.make();
        store.append(
            path,
            PathNode {
                x: 0.,
                y: 0.,
                z: 0.,
                flag: 0,
            },
        );
        let mut unit = NavUnit::new(0, kind::SOLDIER, Vec3::default());
        unit.soldier = Some(SoldierView::default());
        Self {
            store,
            path,
            unit,
            control: Control::default(),
        }
    }
    pub fn start(&mut self, start: Vec3, end: Vec3, mover: &Mover) -> PathExecutor {
        self.store.paths[self.path].nodes[0] = PathNode {
            x: end.x,
            y: end.y,
            z: end.z,
            flag: 0,
        };
        self.unit.position = start;
        self.unit.velocity = Vec3::new(mover.velocity.x, mover.velocity.y, mover.velocity.z);
        self.unit.soldier = Some(SoldierView {
            max_speed: Some(mover.max_speed),
            max_strafe: Some(mover.max_strafe),
            max_turn: Some(mover.max_turn),
            max_pitch: Some(mover.max_pitch),
            ..Default::default()
        });
        self.control = Control::default();
        PathExecutor::new(
            &mut self.store,
            &mut self.control,
            self.path,
            0,
            &mut self.unit,
        )
    }
    pub fn tick(&mut self, executor: &mut PathExecutor, mover: &mut Mover) -> Input {
        self.tick_aim(executor, mover, None, true)
    }
    pub fn tick_aim(
        &mut self,
        executor: &mut PathExecutor,
        mover: &mut Mover,
        aim: Option<Vec3>,
        hold: bool,
    ) -> Input {
        self.unit.position = Vec3::new(mover.position.x, mover.position.y, mover.position.z);
        self.unit.velocity = Vec3::new(mover.velocity.x, mover.velocity.y, mover.velocity.z);
        let soldier = self.unit.soldier.as_mut().unwrap();
        soldier.aim_yaw = Some(mover.aim_yaw);
        soldier.aim_pitch = Some(mover.aim_pitch);
        soldier.avoidance = Some(Vec3::new(
            mover.avoidance.x,
            mover.avoidance.y,
            mover.avoidance.z,
        ));
        executor.hold = true;
        executor.tick(&mut self.store, &mut self.control, 1. / 60., &mut self.unit);
        let intent = analogs(
            &self.unit,
            Some(&self.control.request),
            aim,
            1. / 60.,
            true,
            Some(hold),
        );
        mover.avoidance = V3::ZERO;
        Input {
            forward: intent.forward,
            right: intent.right,
            turn: intent.turn,
            pitch: intent.pitch,
            ..Default::default()
        }
    }
}
