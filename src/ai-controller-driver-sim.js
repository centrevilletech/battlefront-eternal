// Callback frames preserve controller/goal identities across nested services.
export class NativeControllerDriver {
  constructor(sim) {
    this.sim=sim;this.frames=[];
    sim.bridge.register({aiControllerDriver:args=>{
      const op=args.op,number=args.number,frame=this.frames[this.frames.length-1],c=frame.controller,a=frame.agent,result={flag:false,number:0};
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
      else if(op==='factory'){frame.factory=c.ai.agentFor(c.state,c);result.flag=Boolean(frame.factory);}
      else if(op==='createAgent'){const factory=frame.factory;c.agent=number!==0?factory(c,c.commands.current):null;}
      else if(op==='traceEnter')c.trace('enter',{state:c.state,agent:c.agent?.constructor.name??null});
      else if(op==='hasAgent')result.flag=Boolean(c.agent);
      else if(op==='exitAgent')c.agent.exit();
      else if(op==='disposeAgent')c.disposeAgent();
      else if(op==='tickAgent')result.flag=Boolean(c.agent.tick(frame.dt,c.phase));
      else if(op==='currentDone')c.commands.currentDone=Boolean(number);
      else if(op==='captureAgent'){frame.agent=c.agent;c.agent=null;}
      else if(op==='releaseAgent')a?.dispose?.();
      else if(op==='agentPhase')result.flag=frame.phase===0;
      else if(op==='agentUpdate')a.update(frame.dt,Boolean(number));
      else if(op==='agentState'){const state=a.state;result.number=typeof state==='number'?state:NaN;}
      else if(op==='controllerPhase'){const phase=a.controller.phase;result.number=typeof phase==='number'?phase:NaN;}
      else if(op==='rankTargets')a.ai.combat?.rankTargets?.(a);
      else if(op==='behave')a.behave(frame.dt);
      else if(op==='hasHelper')result.flag=Boolean(a.helper);
      else if(op==='stopHelper')(a.helper.terminate??a.helper.stop)?.call(a.helper);
      else if(op==='terminateAgent')a.terminate();
      else if(op==='aimStart'){frame.aimController=a.controller;frame.aimController.setStance(frame.stance);}
      else if(op==='combat')frame.combat=a.ai.combat;
      else if(op==='currentTarget'){frame.target=frame.combat?.currentTarget?.(a)??null;result.flag=Boolean(frame.target);}
      else if(op==='aimTarget'){const t=frame.target;frame.aimController.aimAt({unit:t,generation:t.generation});}
      else if(op==='memoryPoint'){frame.memory=frame.combat?.memoryPoint?.(a)??null;result.flag=Boolean(frame.memory);}
      else if(op==='hasLook')result.flag=Boolean(frame.look);
      else if(op==='aimLook')frame.aimController.lookAt(number===0?frame.memory:frame.look);
      else if(op==='aimAlong')frame.aimController.lookAlongPath();
      else if(op==='engageHook'){frame.engage=a.ai.combat?.canEngage;result.flag=Boolean(frame.engage);}
      else if(op==='engage'){const engage=frame.engage;result.flag=Boolean(engage(a.controller,frame.target));}
      else if(op==='boardHook'){frame.vehicles=a.ai.vehicles;result.flag=Boolean(frame.vehicles?.boardPolicy);}
      else if(op==='board')result.flag=Boolean(frame.vehicles.boardPolicy(a,frame.target));
      else if(op==='nominalSpeed'){const u=frame.target;result.number=u?(u.soldier?.maxSpeed??u.classDef?.MaxSpeed??u.classDef?.ForwardSpeed??0):0;}
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
      else this.sim.x.ai_controller_agent(op-4);
      return frame.result;
    } finally {this.frames.pop();}
  }
  agent(agent,op,options={}) {
    const frame={agent,...options};this.frames.push(frame);
    try {
      if(op===0)return Boolean(this.sim.x.ai_agent_tick());
      if(op===1)this.sim.x.ai_agent_on_tick();
      else if(op===2)this.sim.x.ai_agent_exit();
      else if(op===3)this.sim.x.ai_agent_aim();
      else if(op===4)return Boolean(this.sim.x.ai_agent_can_engage());
      else if(op===5)return Boolean(this.sim.x.ai_agent_board_policy());
    } finally {this.frames.pop();}
  }
}
