// Pure planner fixtures model the acknowledged effects of outside body
// commands. Actual movement, contact and controller publication run through
// the native phase in the f3-* and f4-* fixtures.
export const unitCommands={
  bindController(controller){if(this.f3)this.f3.controller(this,controller);else this.controller=controller;},
  // Fleet-only planner fixtures already copied the draw onto each weapon;
  // their native fleet input imports it at its next owning boundary.
  bindMountedRandom(lease,draw){if(this.f3)this.f3.mountedRandom(lease,draw);},
  consumeContact(){if(this.f3)this.f3.consumeContact(this);else this.soldier.avoidance.set(0,0,0);},
};
