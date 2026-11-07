// Resource facts for the native mount-history phase. The existing network
// presentation call samples every hull; this host never interpolates angles.
import {NetLeaseKind as K,NetMountFrame as F,NetMountInput as I,NetMountOutput as O,NetLimits as L} from './network-layout.js';
export class NetworkMounts{
  constructor(network){this.network=network;this.vehicles=null;}
  clear(){this.vehicles=null;}
  // Identity and lease values are transport facts. Rust decides which exact
  // correction owns each seat, including reused ids and released leases.
  prepare(game,previous){
    const n=this.network;n.refresh();const f=n.mountFrame,own=game.playerUnit,lease=own.seat,mount=lease?.vehicle||lease?.turret;
    this.vehicles=game.vehicles?.vehicles;const vehicles=this.vehicles;
    f[F.COUNT]=vehicles?.length||0;
    f[F.OWN_ID]=own.id;f[F.OWN_GENERATION]=own.generation;f[F.HAS_PREVIOUS]=previous?1:0;
    f[F.PREVIOUS_ID]=previous?.id||0;f[F.PREVIOUS_GENERATION]=previous?.generation||0;
    f[F.VEHICLE_ID]=previous?.vehicleId||0;f[F.VEHICLE_GENERATION]=previous?.vehicleGeneration||0;
    f[F.MOUNT_ID]=previous?.mountId||0;f[F.MOUNT_GENERATION]=previous?.mountGeneration||0;f[F.MOUNT_SEAT]=previous?.mountSeat||0;
    f[F.LEASE_KIND]=lease?.vehicle?K.VEHICLE:lease?.turret?K.TURRET:K.NONE;f[F.LEASE_ID]=mount?.aiTarget?.id||0;
    f[F.LEASE_GENERATION]=mount?.aiTarget?.generation||0;f[F.LEASE_SEAT]=lease?.index||0;f[F.LEASE_RELEASED]=lease?.released?1:0;
    // Input order is the current resource list. Native output carries id and
    // generation too, so publication cannot pair independently compacted slots
    // or write the last occupant's sampled angles onto a newly reused resource.
    const input=n.mountInput;
    for(let i=0;i<(vehicles?.length||0)&&i<L.SLOTS;i++){
      const v=vehicles[i],at=i*I.STRIDE;input[at+I.ID]=v.aiTarget.id;input[at+I.GENERATION]=v.aiTarget.generation;
      input[at+I.ALIVE]=v.alive?1:0;input[at+I.WALK_LENGTH]=v.walkLength;input[at+I.SEAT_COUNT]=v.seats.length;
      input[at+I.TILT]=v.tilt;input[at+I.ROLL]=v.roll;input[at+I.WALK_PHASE]=v.walkPhase;input[at+I.WALK_BLEND]=v.walkBlend;
      let seed=0;for(let j=0;j<v.seats.length&&j<L.MOUNT_SEATS;j++){
        const seat=v.seats[j];input[at+I.SEAT_ANGLES+j*2]=seat.aimYaw;input[at+I.SEAT_ANGLES+j*2+1]=seat.pitch;
        if(seat.lastYaw===null)seed|=1<<j;
      }input[at+I.LAST_YAW_NULL]=seed;
    }
  }
  // Source fields still feed collision/readers and prediction prerequisites.
  // Publish sampled numbers only; never reconstruct a sampled pose facade.
  publish(){
    const n=this.network;n.refresh();const output=n.mountOutput,vehicles=this.vehicles;
    for(let i=0;i<(vehicles?.length||0)&&i<L.SLOTS;i++){
      const v=vehicles[i],at=i*O.STRIDE;
      if(!output[at+O.VALID]||v.aiTarget.id!==output[at+O.ID]||v.aiTarget.generation!==output[at+O.GENERATION])continue;
      // The ownership mask also protects lean and walk state for a pilot.
      // A passenger's exact seat is skipped while the hull and every other
      // seat still receive the native sample for this displayed clock.
      if(!(output[at+O.OWN_MASK]&1)){
        v.tilt=v.previousTilt=output[at+O.TILT];v.roll=v.previousRoll=output[at+O.ROLL];
        v.walkPhase=output[at+O.WALK_PHASE];v.walkBlend=output[at+O.WALK_BLEND];
      }
      const count=output[at+O.SEAT_COUNT],own=output[at+O.OWN_MASK],seed=output[at+O.SEED_LAST_YAW];
      for(let j=0;j<count;j++){
        if(own&(1<<j))continue;const seat=v.seats[j];
        seat.aimYaw=output[at+O.SEAT_ANGLES+j*2];seat.pitch=output[at+O.SEAT_ANGLES+j*2+1];
        if(seed&(1<<j))seat.lastYaw=seat.aimYaw;
      }
    }
  }
}
