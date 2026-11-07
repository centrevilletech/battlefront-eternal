// Raw input, body and lease facts for the native own-preview frame. These
// retained rows join network sampling; the host only publishes the results.
import {Vector3} from 'three';
import {NetOutbound as R} from './network-layout.js';
import {NetPreviewPosture as P,NetPreviewAir as A,NetPreviewFrame as F,NetPreviewCommand as C,NetPreviewLook as L,NetPreviewOutput as O,NetLimits as N} from './network-layout.js';
const NONE=Object.freeze([]),POSTURES={stand:P.STAND,crouch:P.CROUCH,prone:P.PRONE},AIR={jump:A.JUMP,fall:A.FALL,thrown:A.THROWN,jet:A.JET};
class CommandAngles{constructor(){this.aimYaw=this.aimPitch=.1;this.aimYaw=this.aimPitch=0;}}
export class NetworkPreview{
  constructor(network){this.network=network;this.client=null;this.enabled=false;this.aimFrom=new Vector3();this.aimDirection=new Vector3();this.angles=new CommandAngles();}
  // The command looks occupy a prefix; loose DOM looks occupy the tail.
  // Reserving both before copying prevents a large mouse burst aliasing an
  // outstanding command. Excess records keep a bounded prefix, without growth.
  // Input encoding and display borrow the same facts at different points in
  // the frame. Preparing them performs no crossing or numerical integration;
  // outbound processing computes fire aim in its already-required native call.
  prepareFacts(game,enabled=true,alpha=1,lockstep=false,frozen=false){
    const n=this.network;n.refresh();const a=n.previewFrame,c=game.client,s=game.soldier,combat=game.combat;
    this.client=c;this.enabled=Boolean(enabled&&c&&s&&combat);a[F.ENABLED]=this.enabled?1:0;
    if(!this.enabled)return;
    a[F.FROZEN]=frozen?1:0;a[F.LOCKSTEP]=lockstep?1:0;a[F.PREDICTED]=c.predicted?1:0;
    a[F.DEAD]=combat.dead?1:0;a[F.WALKER]=game.playerUnit.walker?1:0;
    a[F.OWN_YAW]=s.aimYaw;a[F.OWN_PITCH]=s.aimPitch;a[F.MAX_TURN]=s.maxTurn;a[F.LOCK]=s.lock;
    a[F.POSTURE]=POSTURES[s.posture]??P.STAND;a[F.AIR]=AIR[s.air]??A.NONE;a[F.DIVE]=s.dive;
    a[F.PITCH_LOW]=s.pitchLimits?.[0]??-Infinity;a[F.PITCH_HIGH]=s.pitchLimits?.[1]??Infinity;
    const position=s.position,camera=combat.hudCamera||combat.camera,sections=combat.def?.cameras;
    a[F.POSITION]=a[F.FEET]=position?.x??0;a[F.POSITION+1]=a[F.FEET+1]=position?.y??0;a[F.POSITION+2]=a[F.FEET+2]=position?.z??0;
    a[F.EYE_HEIGHT]=s.eyeHeight??0;a[F.ROLLED]=s.mode>=11?1:0;a[F.HAS_CAMERA]=camera?1:0;a[F.USES_EYE]=combat.usesEyeView?1:0;
    if(camera){a[F.CAMERA]=camera.position.x;a[F.CAMERA+1]=camera.position.y;a[F.CAMERA+2]=camera.position.z;}
    a[F.TRACK_OFFSETS]=sections?.STAND?.TrackOffset?.[2]??3.5;
    a[F.TRACK_OFFSETS+1]=sections?.CROUCH?.TrackOffset?.[2]??sections?.STAND?.TrackOffset?.[2]??3.5;
    a[F.TRACK_OFFSETS+2]=sections?.PRONE?.TrackOffset?.[2]??sections?.STAND?.TrackOffset?.[2]??3.5;
    a[F.ZOOM]=combat.zoom;a[F.SCALE]=game.settings.mouseScale;a[F.INVERT]=game.settings.get('invertMouse')?1:0;
    a[F.ALPHA]=alpha;a[F.TICK_YAW]=game.playerUnit.presentation.tickYaw;
    a[F.PRESENT_ALPHA]=game.previewPresentAlpha??1;a[F.INPUT_ALPHA]=c.inputClock?.alpha??1;
    a[F.HAS_LOOSE]=frozen||lockstep?0:1;
    a[F.INPUT_TURRET_YAW]=game.gunner?.previewYaw??0;a[F.INPUT_TURRET_PITCH]=game.gunner?.previewPitch??0;
    const t=game.gunner?.turret;a[F.TURRET_ACTIVE]=game.gunner?.active&&t?1:0;
    a[F.TURRET_ID]=t?.aiTarget?.id||0;a[F.TURRET_GENERATION]=t?.aiTarget?.generation||0;
    if(t){a[F.TURRET_YAW]=t.yaw;a[F.TURRET_PITCH]=t.pitch;a[F.YAW_VEL]=t.yawVel;a[F.PITCH_VEL]=t.pitchVel;
      a[F.TURN_RATE]=t.turnRate;a[F.FILTER]=t.filter;a[F.HAS_YAW_ARC]=t.yawLimits?1:0;
      a[F.ARC_LOW]=t.yawLimits?.[0]??0;a[F.ARC_HIGH]=t.yawLimits?.[1]??0;a[F.WRAP_YAW]=t.wrapYaw?1:0;
      a[F.TURRET_PITCH_LOW]=t.pitchLimits?.[0]??-Infinity;a[F.TURRET_PITCH_HIGH]=t.pitchLimits?.[1]??Infinity;a[F.FACING]=t.facing;
    }
    // Lease and preview identity comparisons are host resource facts. The
    // native transport phase uses them to select a current canonical seat or
    // its previously published reticle, without another per-seat WASM call.
    const v=game.driver?.vehicle;a[F.VEHICLE_ACTIVE]=game.driver?.active&&v?1:0;
    a[F.INPUT_VEHICLE_PREVIEW]=v&&game.driver.previewVehicle===v&&game.driver.previewSeat===v.seatIndex?1:0;
    a[F.INPUT_VEHICLE_YAW]=game.driver?.previewYaw??0;a[F.INPUT_VEHICLE_PITCH]=game.driver?.previewPitch??0;
    if(v){const seat=v.seat,view=seat.view;a[F.FLYING]=v.flying?1:0;a[F.SEAT_INDEX]=v.seatIndex;a[F.HULL_YAW]=v.yaw;
      a[F.SEAT_YAW]=v.aimYaw;a[F.SEAT_PITCH]=v.pitch;a[F.HAS_LOOK_YAW]=v.lookYaw!=null?1:0;a[F.HAS_LOOK_PITCH]=v.lookPitch!=null?1:0;
      a[F.LOOK_YAW]=v.lookYaw??0;a[F.LOOK_PITCH]=v.lookPitch??0;
      a[F.VEHICLE_PITCH_LOW]=view.pitchLimits[0];a[F.VEHICLE_PITCH_HIGH]=view.pitchLimits[1];
      a[F.VEHICLE_HAS_YAW_ARC]=view.yawLimits?1:0;a[F.VEHICLE_ARC_LOW]=view.yawLimits?.[0]??0;a[F.VEHICLE_ARC_HIGH]=view.yawLimits?.[1]??0;a[F.VEHICLE_WRAP_YAW]=view.wrapYaw?1:0;
    }
  }
  // The native command batch publishes ray requests for every fire event.
  // Answer those external world/unit queries together, then converge once;
  // the individual transport callbacks read retained completed angles only.
  resolveAim(game,count){
    const n=this.network,combat=game.combat;let requested=false;n.refresh();
    for(let i=0;i<count;i++){
      const at=i*R.STRIDE,x=n.outboundResults;if(!x[at+R.NEEDS_REACH]||!x[at+R.FIRE_COUNT])continue;
      this.aimFrom.fromArray(x,at+R.AIM_FROM);this.aimDirection.fromArray(x,at+R.AIM_DIRECTION);
      const reach=combat.crosshairReach(this.aimFrom,this.aimDirection);n.refresh();n.outboundResults[at+R.REACH]=reach;requested=true;
    }
    if(requested)n.outboundAim(count);
  }
  // convergedAim restored the corrected gun basis after previewing pointer
  // input. Publish that source basis without reconstructing its geometry.
  commandAim(game,at){
    const n=this.network;n.refresh();const x=n.outboundResults,combat=game.combat;
    if(x[at+R.NEEDS_REACH]){const basis=at+R.SOURCE_BASIS;
      combat.origin.fromArray(x,basis);combat.pivot.fromArray(x,basis+3);combat.left.fromArray(x,basis+6);combat.up.fromArray(x,basis+9);combat.aimDir.fromArray(x,basis+12);
    }
    const aim=this.angles;aim.aimYaw=x[at+R.BODY_YAW];aim.aimPitch=x[at+R.BODY_PITCH];return aim;
  }
  prepare(game,enabled=true,alpha=1,lockstep=false,frozen=false){
    this.prepareFacts(game,enabled,alpha,lockstep,frozen);
    game.client?.smoother?.prepare(this.network,0);game.client?.vehicleSmoother?.prepare(this.network,1);
    if(!this.enabled)return;
    const n=this.network,a=n.previewFrame,c=game.client;
    const pending=frozen||lockstep?NONE:c.pending||NONE,loose=frozen||lockstep?NONE:game.controls.looks||NONE;
    // Copy event facts verbatim. Keep per-command optional settings distinct
    // from current defaults, including explicit false and a zero sensitivity.
    // Native code chooses the clock, integrates servos and clamps each event.
    const looseCount=Math.min(loose.length,N.PREVIEW_LOOKS),looks=n.previewLooks;
    a[F.LOOKS]=looseCount;
    for(let i=0;i<looseCount;i++){const at=(N.PREVIEW_LOOKS-looseCount+i)*L.STRIDE;looks[at+L.X]=loose[i][0];looks[at+L.Y]=loose[i][1];}
    const commands=n.previewCommands,count=Math.min(pending.length,N.PREVIEW_COMMANDS);a[F.COMMANDS]=count;
    let used=0;
    for(let i=0;i<count;i++){
      const command=pending[i],source=command.look||NONE,at=i*C.STRIDE,keep=Math.min(source.length,N.PREVIEW_LOOKS-looseCount-used);
      commands[at+C.LOOK_AT]=used;commands[at+C.LOOK_COUNT]=keep;
      commands[at+C.HAS_SCALE]=command.mouseScale===undefined||command.mouseScale===null?0:1;commands[at+C.SCALE]=command.mouseScale??0;
      commands[at+C.HAS_INVERT]=command.invertMouse===undefined||command.invertMouse===null?0:1;commands[at+C.INVERT]=command.invertMouse?1:0;
      commands[at+C.LOOK_SENT]=command.lookSent===false?0:1;
      for(let j=0;j<keep;j++){looks[used*L.STRIDE+L.X]=source[j][0];looks[used*L.STRIDE+L.Y]=source[j][1];used++;}
    }
  }
  // Derived angles never feed another JS controller. Resource getters consume
  // these scalar publications, and body/camera phases use the same values.
  // Scalar publication preserves the existing resource identities. Turret
  // camera/body and vehicle reticle readers observe the same native preview;
  // none reconstructs or advances a controller after this publication.
  publish(game){
    const n=this.network;n.refresh();const c=this.client;
    c?.smoother?.publish(n,0);c?.vehicleSmoother?.publish(n,1);
    if(!this.enabled)return;
    const x=n.previewOutput,view=game.ownPreview;
    view.yaw=x[O.PREVIEW_YAW];view.pitch=x[O.PREVIEW_PITCH];view.hudYaw=x[O.HUD_YAW];
    view.renderedYaw=x[O.RENDERED_YAW];view.renderedPitch=x[O.RENDERED_PITCH];view.ready=true;
    const driver=game.driver;
    if(driver){driver.previewVehicle=x[O.VEHICLE_VALID]?driver.vehicle:null;driver.previewSeat=driver.vehicle?.seatIndex;
      driver.previewYaw=x[O.VEHICLE_YAW];driver.previewPitch=x[O.VEHICLE_PITCH];}
    const gunner=game.gunner;
    if(gunner){gunner.previewYaw=x[O.TURRET_PREVIEW_YAW];gunner.previewPitch=x[O.TURRET_PREVIEW_PITCH];
      const state=gunner.previewState;state.yaw=x[O.TURRET_YAW];state.pitch=x[O.TURRET_PITCH];state.yawVel=x[O.TURRET_YAW_VEL];state.pitchVel=x[O.TURRET_PITCH_VEL];
      gunner.shownYaw=x[O.SHOWN_YAW];gunner.shownPitch=x[O.SHOWN_PITCH];}
  }
}
