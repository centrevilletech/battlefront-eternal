// Detached fixtures still require a native body owner. Real battles supply
// their own collision module through Droideka's normal constructor.
import {Droideka as Body} from '../../src/droideka.js';
import {owner} from './body.js';
export class Droideka extends Body {
  constructor(world,def,clips,actor){super(world,def,clips,actor,owner(world));}
}
export {DroidekaAnimator,goneUp} from '../../src/droideka.js';
