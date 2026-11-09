//! Synthetic phase regressions preserve the event rules formerly checked by
//! the deleted JS policy tests. No comparison implementation is kept alive.
use audio_control::{Collision, Hit, Point};
use audio_control::engine::{Ask, Entry, Sample, Sound, NONE, SILENT, EFFECTS};
use audio_control::layout::*;
use audio_control::phase::Phase;
struct Ground;
impl Collision for Ground {
    fn cast(&mut self, p: Point, d: Point, reach: f64) -> Option<Hit> {
        let distance = -p[1] / d[1];
        (d[1] < 0.0 && distance >= 0.0 && distance <= reach).then_some(Hit {distance,point:[p[0],0.0,p[2]],terrain:true})
    }
}
fn phase() -> Phase {
    let mut p = Phase::new(123); p.started = true; p.engine.running = true;
    let mut row = [f64::NAN; sound::STRIDE]; row[sound::KIND] = 1.0; row[sound::BUS] = -1.0; row[sound::STREAM] = -1.0;
    row[sound::MODE3D] = 1.0; row[sound::MIN_DISTANCE] = 3.0; row[sound::MAX_DISTANCE] = 500.0; row[sound::MUTE_DISTANCE] = 500.0;
    row[sound::GAIN] = 0.5; row[sound::BIAS] = 0.0001;
    for _ in 0..16 { p.engine.sounds.push(Sound::load(&row,vec![Entry {id:0,weight:1.0,interval:0.0,dev:0.0,next:f64::NEG_INFINITY}])); }
    p.engine.samples.push(Sample {render_id:0,duration:0.5,rate:48000.0,scale:1.0,available:true}); p.engine.enhanced_samples.push(None);
    p.classes.push([f64::NAN;class::STRIDE]); for field in 0..class::OVERHEAT_PITCH { p.classes[0][field] = -1.0; }
    for field in class::COLLISION..class::STRIDE { p.classes[0][field] = -1.0; }
    p.reaches.push(f64::NAN); p.class_properties.push(Vec::new()); p.foot_definitions.push(Vec::new());
    p.layers.definitions.resize_with(16,||audio_control::layers::Definition {layers:Vec::new()});
    p.frame[frame::RUNNING] = 1.0; p.frame[frame::RATE] = 48000.0; p.frame[frame::MAX_VOICES] = 32.0; p.frame[frame::VIRTUAL_VOICES] = 64.0;
    p.frame[frame::TEAM] = 1.0; p.frame[frame::OWN] = -1.0; p.frame[frame::FZ] = -1.0; p.frame[frame::UY] = 1.0; p.frame[frame::TICKETS1] = 100.0; p.frame[frame::TICKETS2] = 100.0;
    p.frame[frame::WATER_LEVEL] = f64::NEG_INFINITY;
    for key in [frame::EFFECTS_VOLUME,frame::AMBIENCE_VOLUME,frame::MUSIC_VOLUME,frame::VOICE_VOLUME,frame::INTERFACE_VOLUME] {p.frame[key] = 1.0;}
    p
}
fn actor(p: &mut Phase, id: usize, kind: usize) {
    p.entity_count = p.entity_count.max(id+1); let r = &mut p.emitters[id]; r[emitter::KIND] = kind as f64; r[emitter::CLASS] = 0.0; r[emitter::ALIVE] = 1.0;
    r[emitter::HEALTH] = 100.0; r[emitter::MAX_HEALTH] = 100.0; r[emitter::Z] = -3.0; r[emitter::GROUNDED] = 1.0; r[emitter::SHIELD] = -1.0;
    r[emitter::FOOT0] = f64::NAN; r[emitter::FOOT1] = f64::NAN; r[emitter::FOOT2] = f64::NAN; r[emitter::LIFE] = 10.0;
}
fn weapon(p: &mut Phase, id: usize, owner: usize) {
    p.weapon_count = p.weapon_count.max(id+1); let r = &mut p.weapons[id]; r[weapon::OWNER] = owner as f64; r[weapon::CLASS] = 0.0; r[weapon::SELECTED] = 1.0; r[weapon::THRESHOLD] = 1.0; r[weapon::MAX_PRESSED] = 1.0;
}
fn event(kind: usize, owner: usize, weapon: usize) -> [f64;event::STRIDE] {
    let mut r = [0.0;event::STRIDE]; r[event::KIND] = kind as f64; r[event::OWNER] = owner as f64; r[event::WEAPON] = weapon as f64; r
}
fn tick(p: &mut Phase, dt: f64, events: &[[f64;event::STRIDE]]) {
    p.engine.commands.clear(); p.engine.placements.clear(); p.streams.requests.clear(); p.frame[frame::DT] = dt; p.frame[frame::NOW] += dt; p.frame[frame::WALL_NOW] = p.frame[frame::NOW]; p.step(&mut Ground,events);
}
fn started(p: &Phase, sound: usize) -> usize { p.engine.voices.iter().filter(|v|v.serial>0 && v.sound==sound).count() }
fn live(p: &Phase, sound: usize) -> usize { p.engine.voices.iter().filter(|v|!v.done && v.sound==sound).count() }

#[test]
fn pending_release_tails_keep_new_voices_virtual_until_slots_return() {
    let mut p = phase();
    // Two batches can replace all real voices before a slow or suspended
    // device clock reaches the old voices' release deadlines.
    for _ in 0..64 {
        let handle = p.play(Ask::new(0, EFFECTS));
        assert_ne!(handle, SILENT);
        let id = p.engine.voices.iter().position(|v| !v.done).unwrap();
        p.engine.set_real(id, true);
        p.stop(handle, 0.03);
    }
    let handle = p.play(Ask::new(0, EFFECTS));
    assert_ne!(handle, SILENT);
    let id = p.engine.voices.iter().position(|v| !v.done).unwrap();
    let commands = p.engine.commands.len();
    p.engine.set_real(id, true);
    assert!(!p.engine.voices[id].real);
    assert_eq!(p.engine.voices[id].slot, NONE);
    assert_eq!(p.engine.commands.len(), commands);
    // Keep the logical voice alive and admit it once the device catches up.
    p.engine.wall_now = 1.0;
    p.engine.reclaim();
    p.engine.set_real(id, true);
    assert!(p.engine.voices[id].real);
    assert_ne!(p.engine.voices[id].slot, NONE);
}

#[test]
fn salvo_is_once_per_frame_and_fire_stop_cuts_the_preceding_shot() {
    let mut p=phase(); actor(&mut p,0,emitter_kind::SOLDIER); weapon(&mut p,0,0); p.classes[0][class::FIRE]=0.0; p.classes[0][class::FIRE_STOP]=1.0;
    let fire=event(event_kind::ACTOR_FIRE,0,0); tick(&mut p,1.0/60.0,&[fire,fire]); assert_eq!(live(&p,0),1);
    tick(&mut p,1.0/60.0,&[fire]); assert_eq!(live(&p,0),1); assert_eq!(p.engine.stats[2],2);
}
#[test]
fn repair_loop_restarts_and_stops_when_firing_ends() {
    let mut p=phase(); actor(&mut p,0,1); weapon(&mut p,0,0); p.classes[0][class::FIRE_LOOP]=1.0; p.weapons[0][weapon::FIRING]=1.0;
    let mut fire=event(event_kind::ACTOR_FIRE,0,0); fire[event::VALUE]=1.0; tick(&mut p,0.01,&[fire]); assert_eq!(live(&p,1),1);
    tick(&mut p,0.01,&[fire]); assert_eq!(live(&p,1),1); assert_eq!(p.engine.stats[2],2);
    p.weapons[0][weapon::FIRING]=0.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),0);
}
#[test]
fn reload_and_heat_edges_are_separate_for_own_and_other_weapons() {
    let mut p=phase(); actor(&mut p,0,1); actor(&mut p,1,1); weapon(&mut p,0,0); weapon(&mut p,1,1); p.frame[frame::OWN]=0.0;
    p.classes[0][class::RELOAD]=1.0; p.classes[0][class::OVERHEAT]=2.0; p.classes[0][class::OVERHEAT_PITCH]=0.5;
    p.weapons[0][weapon::RELOADING]=1.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),1);
    tick(&mut p,0.01,&[]); assert_eq!(p.engine.stats[2],1);
    p.weapons[0][weapon::RELOADING]=0.0; p.weapons[1][weapon::OVERHEATED]=1.0; p.weapons[1][weapon::HEAT]=2.0; tick(&mut p,0.01,&[]);
    assert_eq!(live(&p,1),0); let v=p.engine.voices.iter().find(|v|!v.done&&v.sound==2).unwrap(); assert_eq!(v.pitch,0.75);
    p.emitters[1][emitter::ALIVE]=0.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,2),0);
}
#[test]
fn charge_first_pitch_and_continued_pitch_keep_their_different_clamps() {
    let mut p=phase(); actor(&mut p,0,1); weapon(&mut p,0,0); p.frame[frame::OWN]=0.0; p.classes[0][class::CHARGE]=3.0; p.classes[0][class::CHARGE_PITCH]=0.5;
    p.weapons[0][weapon::CHARGING]=1.0; tick(&mut p,0.01,&[]); assert_eq!(p.engine.voices.iter().find(|v|!v.done&&v.sound==3).unwrap().pitch,0.5);
    p.weapons[0][weapon::HELD]=0.7; tick(&mut p,0.01,&[]); assert_eq!(p.engine.voices.iter().find(|v|!v.done&&v.sound==3).unwrap().pitch,0.4);
    p.weapons[0][weapon::CHARGING]=0.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,3),0);
}
#[test]
fn wounds_use_remaining_health_and_death_always_replaces_the_voice() {
    let mut p=phase(); actor(&mut p,0,1); p.classes[0][class::HURT]=4.0; p.classes[0][class::DEATH]=5.0;
    let mut hurt=event(event_kind::ACTOR_HURT,0,0); hurt[event::HEALTH]=100.0; hurt[event::AMOUNT]=2.0; tick(&mut p,0.01,&[hurt]); assert_eq!(live(&p,4),0);
    hurt[event::AMOUNT]=2.1; tick(&mut p,0.01,&[hurt]); assert_eq!(live(&p,4),1);
    let dead=event(event_kind::ACTOR_DEATH,0,0); tick(&mut p,0.01,&[dead]); assert_eq!(live(&p,4),0); assert_eq!(live(&p,5),1);
}
#[test]
fn far_one_shots_do_not_steal_but_loops_retain_virtual_time() {
    let mut p=phase(); let mut ask=Ask::new(0,EFFECTS); ask.position=Some([1000.0,0.0,0.0]); assert_eq!(p.play(ask),SILENT); assert_eq!(p.engine.stats[5],1);
    ask.looping=Some(true); let h=p.play(ask); tick(&mut p,0.1,&[]); tick(&mut p,0.1,&[]); assert!(p.playing(h)); assert_eq!(p.engine.stats[0],0); assert!(p.engine.voices.iter().any(|v|!v.done&&v.elapsed>=0.1));
}
#[test]
fn post_charge_dispute_and_discharge_use_raw_occupancy() {
    let mut p=phase(); p.classes[0][class::CHARGE]=1.0; p.classes[0][class::DISPUTE]=2.0; p.classes[0][class::DISCHARGE]=3.0;
    p.post_count=1; p.posts[0].fill(-1.0); p.posts[0][post::PRESENT]=1.0; p.posts[0][post::CLASS]=0.0; p.posts[0][post::ACTIVE]=1.0; p.posts[0][post::TEAM]=0.0; p.posts[0][post::CONTESTED]=0.0; p.posts[0][post::PROGRESS]=0.5; p.post_counts[0][1]=1.0;
    tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),1); assert_eq!(p.engine.voices.iter().find(|v|!v.done&&v.sound==1).unwrap().pitch,0.8);
    p.posts[0][post::CONTESTED]=1.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),0); assert_eq!(live(&p,2),1);
    p.posts[0][post::TEAM]=2.0; p.posts[0][post::NEUTRALIZE]=0.5; tick(&mut p,0.01,&[]); assert_eq!(live(&p,2),0); assert_eq!(live(&p,3),1);
    p.posts[0][post::ACTIVE]=0.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,3),0);
}
#[test]
fn mine_landing_is_once_and_ticks_follow_whole_seconds() {
    let mut p=phase(); p.classes[0][class::TICK]=1.0; p.classes[0][class::TICK_PITCH]=0.5; p.classes[0][class::BOUNCE]=2.0; p.item_count=1;
    let r=&mut p.items[0]; r[item::PRESENT]=1.0; r[item::MINE]=1.0; r[item::MINE_BASE]=1.0; r[item::LANDING_SOURCE]=1.0; r[item::LANDED]=1.0; r[item::SPAN]=2.0; r[item::LIFE]=2.0;
    tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),1); assert_eq!(live(&p,2),1); let n=p.engine.stats[2]; tick(&mut p,0.01,&[]); assert_eq!(p.engine.stats[2],n);
    p.items[0][item::LIFE]=0.5; tick(&mut p,0.01,&[]); assert_eq!(p.engine.stats[2],n+1); assert_eq!(p.engine.voices.iter().rfind(|v|!v.done&&v.sound==1).unwrap().pitch,0.625);
}
#[test]
fn round_loop_stops_when_the_published_round_disappears() {
    let mut p=phase(); actor(&mut p,0,emitter_kind::ROUND); p.classes[0][class::ORDNANCE]=1.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),1);
    p.emitters[0][emitter::KIND]=0.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),0);
}
#[test]
fn hull_pilot_and_flight_edges_preserve_engine_lifetime() {
    let mut p=phase(); actor(&mut p,0,emitter_kind::HULL); p.classes[0][class::ENGINE]=1.0; p.classes[0][class::TURN_ON]=2.0; p.classes[0][class::TURNING_OFF]=3.0; p.classes[0][class::TURN_OFF]=4.0; p.classes[0][class::TURN_OFF_TIME]=0.1;
    tick(&mut p,0.01,&[]); p.emitters[0][emitter::PILOTED]=1.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),1); assert_eq!(live(&p,2),1);
    p.emitters[0][emitter::PILOTED]=0.0; p.emitters[0][emitter::FLYING]=1.0; tick(&mut p,0.2,&[]); assert_eq!(live(&p,1),1); assert_eq!(started(&p,3),0);
    p.emitters[0][emitter::FLYING]=0.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,3),1); tick(&mut p,0.11,&[]); tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),0); assert_eq!(live(&p,4),1);
}
#[test]
fn hull_damage_death_and_crash_use_distinct_rules() {
    let mut p=phase(); actor(&mut p,0,3); p.classes[0][class::HURT]=1.0; p.classes[0][class::DEATH]=2.0; p.classes[0][class::COLLISION]=3.0; p.emitters[0][emitter::COLLISION_DROP]=5.0; p.emitters[0][emitter::VZ]=20.0;
    tick(&mut p,0.01,&[]); p.emitters[0][emitter::HEALTH]=90.0; p.emitters[0][emitter::VZ]=0.0; tick(&mut p,1.0/60.0,&[]); assert_eq!(live(&p,1),1); assert_eq!(live(&p,3),1);
    p.emitters[0][emitter::ALIVE]=0.0; p.emitters[0][emitter::HEALTH]=0.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,2),1);
}
#[test]
fn engine_reach_culling_returns_without_another_turn_on() {
    let mut p=phase(); actor(&mut p,0,3); p.classes[0][class::ENGINE]=1.0; p.classes[0][class::TURN_ON]=2.0; p.emitters[0][emitter::PILOTED]=1.0;
    tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),1); let on=started(&p,2); p.emitters[0][emitter::Z] = -1000.0; tick(&mut p,0.01,&[]); assert_eq!(live(&p,1),0);
    p.emitters[0][emitter::Z] = -3.0; tick(&mut p,1.01,&[]); assert_eq!(live(&p,1),1); assert_eq!(started(&p,2),on);
}
fn stream_bank(p:&mut Phase) {
    p.engine.sounds[8].kind=4; p.engine.sounds[8].stream=0;
    p.engine.sounds[9].kind=4; p.engine.sounds[9].stream=0;
    p.streams.definitions.push(audio_control::streams::Stream{slots:1,channels:1});
    p.streams.segments.push(audio_control::streams::Segment{stream:0,duration:0.1,channels:1,variants:[NONE;4]}); p.streams.configure();
}
#[test]
fn stream_slots_gate_queued_speech_until_the_source_retires() {
    use audio_control::streams::Options;
    let mut p=phase();stream_bank(&mut p);let first=p.streams.play(&mut p.engine,Options::new(8,6));assert!(p.streams.playing(first));
    for _ in 0..6{assert!(p.streams.queue(&mut p.engine,Options::new(9,6)));}
    tick(&mut p,0.01,&[]);assert!(p.streams.requests.is_empty(),"pending source still holds its slot");
    assert!(!p.streams.ready(&mut p.engine,0,99,0,0,0.1,1));
    assert!(p.streams.ready(&mut p.engine,0,1,0,0,0.1,1));
    tick(&mut p,0.11,&[]);assert!(!p.streams.playing(first));assert_eq!(p.streams.requests.len(),1,"next queued line starts after retirement");
}
#[test]
fn stream_queue_retains_far_bias_and_respects_the_shared_interval() {
    use audio_control::streams::Options;
    let mut p=phase();stream_bank(&mut p);p.engine.sounds[8].interval=1.0;p.engine.sounds[8].bias=0.001;
    let mut line=Options::new(8,6);line.ask.position=Some([1000.0,0.0,0.0]);line.spatial=true;
    assert!(p.streams.queue(&mut p.engine,line));assert!(!p.streams.queue(&mut p.engine,line));tick(&mut p,0.01,&[]);assert_eq!(p.streams.requests.len(),1);
    assert!(!p.streams.busy(usize::MAX,&p.engine),"a loading source does not duck");
}
#[test]
fn layout_selects_enhanced_stream_assets_in_the_native_phase() {
    use audio_control::streams::Options;
    for (channels,want) in [(2,10),(6,11),(8,12)] {
        let mut p=phase();stream_bank(&mut p);p.engine.enhanced=true;p.engine.channels=channels;p.streams.segments[0].variants=[10,11,12,10];
        p.streams.play(&mut p.engine,Options::new(8,6));assert_eq!(p.streams.requests[0].variant,want);
    }
    let mut p=phase();stream_bank(&mut p);p.engine.channels=8;p.streams.segments[0].variants=[10,11,12,10];p.streams.play(&mut p.engine,Options::new(8,6));assert_eq!(p.streams.requests[0].variant,NONE,"Classic retains original bytes");
}
#[test]
fn music_thresholds_use_team_two_last_and_change_after_minimum_play() {
    use audio_control::director::{Track,Slot};
    let mut p=phase();stream_bank(&mut p);
    p.director.tracks.push(Track::new(8,1.0,0.0,1.0,0.0,0.0));p.director.tracks.push(Track::new(9,0.0,0.0,2.0,0.0,0.0));
    p.director.slots[1][0]=Some(Slot{track:0,threshold:100.0});p.director.slots[1][1]=Some(Slot{track:1,threshold:50.0});
    p.director.slots[2][0]=Some(Slot{track:0,threshold:100.0});p.director.slots[2][1]=Some(Slot{track:1,threshold:50.0});
    tick(&mut p,0.01,&[]);assert_eq!(p.director.stage,0);assert_eq!(p.streams.requests.len(),1);
    p.frame[frame::TICKETS1]=40.0;tick(&mut p,0.01,&[]);assert_eq!(p.director.stage,0,"team two selects stage last");
    p.frame[frame::TICKETS2]=40.0;tick(&mut p,0.01,&[]);assert_eq!(p.director.stage,1);assert!(p.streams.requests.is_empty(),"minimum playback keeps current source");
    tick(&mut p,1.01,&[]);assert_eq!(p.streams.requests.len(),1,"new stage starts after minimum playback");
}
#[test]
fn battle_end_waits_two_seconds_and_uses_own_team_outcome() {
    use audio_control::director::Track;
    let mut p=phase();stream_bank(&mut p);p.director.tracks.push(Track::new(8,0.0,0.0,1.0,0.0,0.0));p.director.defeat[1]=0;p.frame[frame::OVER]=1.0;p.frame[frame::TICKETS1]=0.0;
    tick(&mut p,1.99,&[]);assert!(p.streams.requests.is_empty());tick(&mut p,0.02,&[]);assert_eq!(p.streams.requests.len(),1);
}
#[test]
fn objective_info_lines_stop_when_the_post_is_inactive() {
    let mut p=phase();stream_bank(&mut p);p.post_count=1;p.posts[0].fill(-1.0);p.posts[0][post::PRESENT]=1.0;p.posts[0][post::ACTIVE]=1.0;p.posts[0][post::BUILDING_ALIVE]=1.0;p.posts[0][post::TEAM]=2.0;p.posts[0][post::CLASS]=0.0;p.posts[0][post::INFO12]=8.0;p.frame[frame::HUMAN]=1.0;
    tick(&mut p,0.01,&[]);assert_eq!(p.streams.requests.len(),1);let seed=p.engine.random.0;p.posts[0][post::ACTIVE]=0.0;tick(&mut p,0.01,&[]);assert_eq!(p.engine.random.0,seed,"inactive objectives ask for no further line");
}
#[test]
fn leaving_boundary_uses_the_current_listener_team() {
    let mut p=phase();stream_bank(&mut p);p.director.bounds[1]=8;p.director.bounds[2]=9;
    tick(&mut p,0.01,&[event(event_kind::LEAVING,0,0)]);assert_eq!(p.streams.requests.len(),1);
    p.frame[frame::TEAM]=2.0;let seed=p.engine.random.0;tick(&mut p,0.01,&[event(event_kind::LEAVING,0,0)]);assert_ne!(p.engine.random.0,seed,"team change selects and queues its own line");
}
#[test]
fn probability_failure_uses_interval_and_cycle_picks_only_draw_once() {
    let mut p=phase();let sound=&mut p.engine.sounds[0];sound.interval=1.0;sound.probability=0.0;
    assert_eq!(p.play(Ask::new(0,0)),SILENT);let seed=p.engine.random.0;assert_eq!(p.play(Ask::new(0,0)),SILENT);assert_eq!(p.engine.random.0,seed);
    p.engine.time=2.0;p.engine.sounds[0].probability=1.0;p.engine.sounds[0].cycle = -1;
    let _=p.play(Ask::new(0,0));assert_eq!(p.engine.sounds[0].cycle,0);
    let seed=p.engine.random.0;let mut random=audio_control::Random(seed);random.next();random.spread();random.spread();p.engine.time=4.0;
    let _=p.play(Ask::new(0,0));assert_ne!(p.engine.random.0,seed);
}
#[test]
fn listener_offset_and_distance_curve_keep_the_original_boundaries() {
    let mut p=phase();p.engine.listener.set([0.0,0.0,0.0],[0.0,0.0,-1.0],[0.0,1.0,0.0]);let (distance,span,local)=p.engine.listener.place([0.0,0.0,-3.0]);assert_eq!((distance,span,local),(1.0,3.0,[0.0,0.0,-1.0]));
    let c=audio_control::engine::Curve{roll_in:0.0,min:3.0,max:90.0,mute:80.0,roll:0.3};assert_eq!(c.gain(3.0),1.0);assert_eq!(c.gain(80.0),0.0);assert_eq!(c.openal(90.0),0.5);
}
#[test]
fn doppler_has_a_strict_half_speed_limit_and_classic_glides_twice() {
    let mut p=phase();let mut ask=Ask::new(0,0);ask.position=Some([0.0,0.0,-3.0]);p.play(ask);p.engine.enhanced=true;
    p.engine.voices[0].radial=171.0;assert_eq!(p.engine.rate(0),1.0+171.0/344.0);
    p.engine.voices[0].radial=172.0;assert_eq!(p.engine.rate(0),1.0);
    p.engine.enhanced=false;p.engine.voices[0].radial=0.0;assert_eq!(p.engine.rate(0),1.0);p.engine.voices[0].radial=100.0;
    let first=p.engine.rate(0);let second=p.engine.rate(0);assert!(second>first&&second<1.0+100.0/344.0);
}
#[test]
fn an_end_command_cancels_delayed_layer_children() {
    use audio_control::layers::{Layer,Definition};
    let mut p=phase();p.engine.sounds[10].kind=2;p.layers.definitions[10]=Definition{layers:vec![Layer{sound:0,delay:1.0,deviation:0.0,bindings:Vec::new()}]};
    let handle=p.play(Ask::new(10,0));tick(&mut p,0.1,&[event(event_kind::END,0,0)]);assert!(!p.playing(handle));tick(&mut p,2.0,&[]);assert_eq!(p.engine.stats[2],0);
}
#[test]
fn parameterized_layers_multiply_graph_outputs_and_raw_idle_inputs() {
    use audio_control::layers::{Layer,Definition,Binding};
    let mut p=phase();p.engine.sounds[10].kind=3;
    p.layers.definitions[10]=Definition{layers:vec![Layer{sound:1,delay:0.0,deviation:0.0,bindings:vec![Binding{input:0,pitch:false,points:vec![[0.0,0.0],[1.0,1.0]]},Binding{input:2,pitch:false,points:vec![[0.0,0.0],[1.0,1.0]]},Binding{input:1,pitch:true,points:vec![[0.0,0.5],[1.0,1.0]]}]}]};
    let h=p.parameterized(Ask::new(10,3));let mut inputs=[(1.0,true,true);7];inputs[0]=(0.5,false,true);inputs[2]=(0.8,false,true);inputs[1]=(0.4,false,true);p.inputs(h,&inputs,false);
    let voice=p.engine.voices.iter().find(|v|!v.done&&v.sound==1).unwrap();assert_eq!(voice.gain_scale,0.4);assert_eq!(voice.pitch_scale,0.7);
    inputs[0]=(1.0,true,true);p.inputs(h,&inputs,false);assert_eq!(p.engine.voices.iter().find(|v|!v.done&&v.sound==1).unwrap().gain_scale,0.8);
}
#[test]
fn delayed_layers_wait_for_their_clock_before_starting_children() {
    use audio_control::layers::{Layer,Definition};
    let mut p=phase();p.engine.sounds[10].kind=2;p.layers.definitions[10]=Definition{layers:vec![Layer{sound:1,delay:0.1,deviation:0.0,bindings:Vec::new()}]};p.play(Ask::new(10,0));
    tick(&mut p,0.05,&[]);tick(&mut p,0.05,&[]);assert_eq!(live(&p,1),0);tick(&mut p,0.05,&[]);assert_eq!(live(&p,1),1);
}
#[test]
fn an_empty_trigger_clicks_only_on_a_new_press() {
    let mut p=phase();actor(&mut p,0,1);weapon(&mut p,0,0);p.frame[frame::OWN]=0.0;p.classes[0][class::EMPTY]=1.0;p.weapons[0][weapon::EMPTY]=1.0;p.emitters[0][emitter::FIRING_PRIMARY]=1.0;
    tick(&mut p,0.01,&[]);let first=p.engine.stats[2];assert_eq!(first,1);tick(&mut p,0.01,&[]);assert_eq!(p.engine.stats[2],first);
    p.emitters[0][emitter::FIRING_PRIMARY]=0.0;tick(&mut p,0.01,&[]);p.emitters[0][emitter::FIRING_PRIMARY]=1.0;tick(&mut p,0.01,&[]);assert_eq!(p.engine.stats[2],first+1);
}
#[test]
fn stride_starts_right_and_enhanced_air_suppresses_steps() {
    let mut p=phase();actor(&mut p,0,1);p.frame[frame::OWN]=0.0;p.classes[0][class::RUSTLE]=3.0;
    let mut fx=[-1.0;foley::STRIDE];fx[foley::CLASS]=0.0;fx[foley::MATERIAL]=0.0;fx[foley::LEFT]=1.0;fx[foley::RIGHT]=2.0;p.foley.push(fx);p.emitters[0][emitter::VX]=3.5;
    tick(&mut p,0.26,&[]);assert_eq!(live(&p,3),1);tick(&mut p,0.26,&[]);assert_eq!(live(&p,2),1);assert_eq!(started(&p,1),0);
    p.frame[frame::ENHANCED]=1.0;p.emitters[0][emitter::GROUNDED]=0.0;p.emitters[0][emitter::AIR]=1.0;let before=p.engine.stats[2];for _ in 0..8{tick(&mut p,0.1,&[]);}assert_eq!(p.engine.stats[2],before);
}
#[test]
fn recon_countdown_selects_first_strike_weapon_by_arsenal_order() {
    let mut p=phase();actor(&mut p,0,emitter_kind::DROID);weapon(&mut p,0,0);p.weapons[0][weapon::STRIKE]=1.0;p.weapons[0][weapon::ORDINAL]=2.0;p.classes[0][class::CHARGE]=1.0;p.classes[0][class::CHARGE_PITCH]=0.5;
    p.emitters[0][emitter::COUNTDOWN]=0.2;tick(&mut p,0.01,&[]);assert_eq!(live(&p,1),1);assert_eq!(p.engine.voices.iter().find(|v|!v.done&&v.sound==1).unwrap().pitch,0.5);
    tick(&mut p,0.01,&[]);assert_eq!(p.engine.voices.iter().find(|v|!v.done&&v.sound==1).unwrap().pitch,0.4);p.emitters[0][emitter::SALVO]=1.0;tick(&mut p,0.01,&[]);assert_eq!(live(&p,1),0);
}
#[test]
fn other_droidekas_share_soldier_reload_and_overheat_rules() {
    let mut p=phase();actor(&mut p,0,emitter_kind::DROIDEKA);weapon(&mut p,0,0);p.classes[0][class::RELOAD]=1.0;p.classes[0][class::OVERHEAT]=2.0;p.weapons[0][weapon::RELOADING]=1.0;
    tick(&mut p,0.01,&[]);assert_eq!(live(&p,1),1);
    p.weapons[0][weapon::RELOADING]=0.0;p.weapons[0][weapon::OVERHEATED]=1.0;p.weapons[0][weapon::HEAT]=2.0;tick(&mut p,0.01,&[]);assert_eq!(live(&p,1),0);assert_eq!(live(&p,2),1);
    p.emitters[0][emitter::ALIVE]=0.0;tick(&mut p,0.01,&[]);assert_eq!(live(&p,2),0);
}
#[test]
fn placed_layer_children_keep_their_own_roll_in_and_roll_off() {
    use audio_control::layers::{Layer,Definition};use audio_control::engine::Curve;
    let mut p=phase();p.engine.sounds[0].curve.roll_in=2.0;p.engine.sounds[0].curve.roll=0.5;p.engine.sounds[10].kind=2;p.layers.definitions[10]=Definition{layers:vec![Layer{sound:0,delay:0.0,deviation:0.0,bindings:Vec::new()}]};
    let mut ask=Ask::new(10,4);ask.position=Some([0.0,0.0,-17.0]);ask.looping=Some(true);ask.curve=Some(Curve{roll_in:0.0,min:10.0,max:20.0,mute:20.0,roll:0.0});p.play(ask);tick(&mut p,0.01,&[]);
    let voice=p.engine.voices.iter().find(|v|!v.done&&v.sound==0).unwrap();assert_eq!(voice.curve.roll_in,2.0);assert_eq!(voice.curve.roll,0.5);assert!((voice.audible-1.0/21.0).abs()<1e-15,"distance bounds override the child without replacing its curve");
}
#[test]
fn negative_loop_and_doppler_tags_are_not_enabled() {
    let mut row=[f64::NAN;sound::STRIDE];row[sound::LOOP] = -1.0;row[sound::DOPPLER] = -1.0;let sound=Sound::load(&row,Vec::new());assert!(!sound.looping);assert!(!sound.doppler);
}
#[test]
fn release_tails_recycle_on_wall_time_while_the_device_clock_is_suspended() {
    let mut p=phase();let h=p.play(Ask::new(0,0));tick(&mut p,0.01,&[]);let slot=p.engine.voices.iter().find(|v|!v.done).unwrap().slot;p.stop(h,0.03);
    p.engine.wall_now=0.2;p.engine.reclaim();assert_eq!(p.engine.now,0.01);assert_eq!(p.engine.reserve_slot(),slot,"a suspended device must not exhaust released effect slots");
}
#[test]
fn finished_stream_slots_recycle_after_wall_time_without_advancing_pcm() {
    use audio_control::streams::Options;
    let mut p=phase();stream_bank(&mut p);let h=p.streams.play(&mut p.engine,Options::new(8,6));p.streams.stop(&mut p.engine,h,0.05);p.engine.wall_now=3.1;
    p.streams.requests.clear();p.streams.update(&mut p.engine,&p.acoustics,0.0);assert_eq!(p.engine.now,0.0);assert!(p.streams.requests.iter().any(|r|r.operation==request_kind::RELEASE));
}
