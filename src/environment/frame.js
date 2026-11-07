// The battle publishes its surface facts once, advances all installed native
// owners once, then applies their outputs at the existing resource boundaries.
// Renderer passes and pass scheduling do not participate in this state phase.
import { SurfaceHost } from './surfaces.js';
import { SurfaceFrame as FRAME, SurfaceResult as RESULT } from './layout.js';

export class SurfaceFrame {
  constructor(owner, foliage, water, shore, weather, snow, atmosphere, sky, environment) {
    this.owner=owner;this.host=water?.host||new SurfaceHost(owner);
    this.foliage=foliage;this.water=water;this.shore=shore;this.weather=weather;
    this.snow=snow;this.atmosphere=atmosphere;this.sky=sky;this.environment=environment;
    // One stable actor slot replaces the frame's [soldier]/[] temporaries.
    // Native wake identity is independent of this slot or its current occupant.
    this.actors=[null];
    this.warned=false;
  }
  update(dt,camera,actor) {
    this.owner.refresh();this.actors[0]=actor;
    // Camera matrices have already been published by the camera phase. None
    // of these writers recomputes a transform, cloud, path, wave or lifetime.
    if(this.water)this.water.prepareFrame(dt,camera);else this.host.prepare(dt,camera);
    if(this.shore)this.shore.prepareFrame();else this.host.frame[FRAME.SHORE_QUALITY]=0;
    if(this.weather)this.weather.prepareFrame(dt,camera);
    if(this.snow)this.snow.prepareFrame(dt,camera);
    if(this.atmosphere)this.atmosphere.prepareFrame(dt,camera);
    if(this.sky)this.sky.prepareFrame(dt,camera);
    if(this.environment)this.environment.prepareFrame(dt,camera,this.actors);
    // Installed systems share memory; this call crosses the state boundary
    // once, rather than calling the weather/sky exports from their JS shells.
    this.owner.update();
    // A collision callback can grow memory during the native call.
    this.host.refresh();
    if(this.foliage)this.foliage.publishFrame(this.host.result,camera);
    if(this.host.result[RESULT.OVERFLOW]&&!this.warned) {
      this.warned=true;console.warn('Surface capacity reached; excess records were skipped');
    }
  }
  // Publication remains at the old Water.update point, before reflection.
  // The environment resources publish later, after posts and turret posing.
  // Keeping those boundaries preserves producer and cosmetic-birth ordering.
  publishWater(camera) {
    if(this.water)this.water.publishFrame(camera);
    if(this.shore)this.shore.publishFrame();
  }
  publishEnvironment() {
    if(this.weather)this.weather.publishFrame();
    if(this.snow)this.snow.publishFrame();
    if(this.atmosphere)this.atmosphere.publishFrame();
    if(this.sky)this.sky.publishFrame();
    if(this.environment)this.environment.publishFrame();
  }
}
