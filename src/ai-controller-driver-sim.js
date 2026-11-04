// Callback frames preserve controller/goal identities across nested services.
export class NativeControllerDriver {
  constructor(sim) {
    this.sim=sim;this.frames=[];
    sim.bridge.register({aiControllerDriver:args=>{
      const op=args.op,number=args.number,frame=this.frames[this.frames.length-1],c=frame.controller,result={flag:false,number:0};
      if(op==='phase'){c.phase=frame.phase;result.flag=frame.phase===0;}
      else if(op==='clearRequests')c.clearRequests();
      else if(op==='driveCommands')c.driveCommands(frame.dt,Boolean(number));
      else if(op==='clearAlerts'){
        c.shot.pending=false;c.shot.known=false;c.danger.pending=false;c.vehicleNotes.length=0;c.command.pending=false;c.useTarget=null;c.clearAcoustic();
      } else if(op==='lowTicketExit')c.lowTicketExit();
      else if(op==='commands')frame.commands=c.commands;
      else if(op==='needsNext')result.flag=Boolean(frame.commands.needsNextGoal());
      else if(op==='count')result.flag=Boolean(frame.commands.count);
      else if(op==='pop')frame.goal=frame.commands.popFront();
      else if(op==='unlatch')frame.commands.latch=false;
      else if(op==='traceGoal'){const goal=frame.goal;c.trace('goal',{kind:goal.kind,mode:goal.mode,post:goal.post?.name??null});}
      else if(op==='goalKind'){const kind=frame.goal.kind;result.number=typeof kind==='number'?kind:NaN;}
      else if(op==='request')c.requestState(number);
      else if(op==='fsm')sim.driveFsm(c,0,frame.dt,Boolean(number));
      else if(op==='published'){if(number===0)c.published=false;else result.flag=Boolean(c.published);}
      else if(op==='runaway')frame.result=c.runaway();
      else if(op==='boarded'){frame.vehicles=c.ai.vehicles;result.flag=Boolean(frame.vehicles?.boarded?.(c));}
      else if(op==='lowTickets')result.flag=Boolean(frame.vehicles.lowTickets?.(c.unit.team));
      else if(op==='keepsSeat')result.flag=Boolean(frame.vehicles.pilotKeepsSeat?.(c));
      else if(op==='requestExit')c.requestExit();
      else if(op==='currentKind'){const kind=c.commands.current.kind;result.number=typeof kind==='number'?kind:NaN;}
      else throw Error(`unknown controller callback ${op}`);
      return result;
    }});
  }
  run(controller,op,dt,phase) {
    const frame={controller,dt,phase,result:undefined};this.frames.push(frame);
    try {
      if(op===0)this.sim.x.ai_controller_update();
      else if(op===1)this.sim.x.ai_controller_commands(Number(Boolean(phase)));
      else if(op===2)this.sim.x.ai_controller_low_ticket_exit();
      else if(op===3)this.sim.x.ai_controller_goal_state();
      return frame.result;
    } finally {this.frames.pop();}
  }
}
