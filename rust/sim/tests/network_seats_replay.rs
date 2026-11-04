#![cfg(feature = "replay")]
//! Replay live field/callback transcripts whose complete results, control
//! calls, queues and hull fields were compared with the source execution.
use bfe_sim::{gen::NetworkSeatsOp, seats::network_module::{run, Transport}};
use sim_replay::{format::Reader, same_number, V, Value};
use std::{collections::BTreeMap, path::Path};

struct Replay<'a> { calls: Vec<V<'a>>, cursor: usize }
fn numbers(v: V) -> Vec<f64> { v.items().map(|x| x.num()).collect() }
fn same(a: &[f64], b: &[f64]) -> bool { a.len() == b.len() && a.iter().zip(b).all(|(&a,&b)| same_number(a,b)) }
impl Transport for &mut Replay<'_> {
    fn call(&mut self, op: NetworkSeatsOp, args: Vec<f64>) -> Result<Vec<f64>, ()> {
        let call = self.calls.get(self.cursor).expect("an expected callback"); self.cursor += 1;
        assert_eq!(call.get("op").str(), Some(NetworkSeatsOp::NAMES[op as usize]), "callback {}", self.cursor);
        assert!(same(&args,&numbers(call.get("args"))), "callback {} arguments: {args:?} != {}", self.cursor, call.get("args").show());
        if call.get("ok").bool() { Ok(numbers(call.get("values"))) } else { Err(()) }
    }
}
fn scalar(v: V, out: &mut Vec<f64>) {
    match v.value {
        Value::Undefined => out.extend([0.0,0.0,0.0]),
        Value::Null => out.extend([1.0,0.0,0.0]),
        Value::Bool(false) => out.extend([2.0,0.0,0.0]),
        Value::Bool(true) => out.extend([3.0,0.0,1.0]),
        Value::Num(x) => out.extend([4.0,*x,if *x != 0.0 && !x.is_nan() {1.0} else {0.0}]),
        _ => panic!("a primitive network control value: {}",v.show()),
    }
}
fn expected(v: V, cycle: bool) -> Vec<f64> {
    if cycle { return vec![if v.bool() {1.0} else {0.0}]; }
    let mut out=Vec::new();
    if !v.get("steeringAssist").is_undefined() {
        assert_eq!(v.get("steeringAssist").opt_bool(),Some(false));
        out.extend([1.0,v.get("forward").num(),v.get("right").num()]);
        let stick=!v.get("stickX").is_undefined();out.push(if stick {1.0} else {0.0});
        if stick {out.extend([v.get("stickX").num(),v.get("stickY").num()]);}
        for key in ["boost","rollLock"] {out.push(if v.get(key).bool() {1.0} else {0.0});}
        for key in ["takeoff","trick","immelmann","targetLock","responseCurve"] {scalar(v.get(key),&mut out);}
    } else {
        out.push(0.0);for key in ["forward","right","turn","brake"] {scalar(v.get(key),&mut out);}
    }
    out
}

#[test]
fn live_network_transcripts_match_source_results_and_callbacks() {
    let Ok(root)=std::env::var("BFE_NETWORK_SEATS_REPLAY") else { eprintln!("BFE_NETWORK_SEATS_REPLAY unset: nothing replayed"); return; };
    let mut paths=std::fs::read_dir(Path::new(&root)).unwrap().map(|e|e.unwrap().path())
        .filter(|p|p.file_name().unwrap().to_string_lossy().ends_with(".native.bfecap")).collect::<Vec<_>>();paths.sort();
    assert!(!paths.is_empty(),"fresh native transcripts");
    let mut totals=BTreeMap::<String,usize>::new();let mut total=0;let mut callbacks=0;
    for path in paths {
        let bytes=std::fs::read(&path).unwrap();let mut reader=Reader::new(&bytes).unwrap();let mut frames=0;
        while let Some(record)=reader.next_record() {
            let record=record.unwrap();
            if record.name=="network.header" {assert!(record.root().get("v8").str().unwrap().starts_with("13.6."));continue;}
            if record.name!="network.live" {continue;}
            let v=record.root();assert!(v.get("sourceVerified").bool());
            let cycle=v.get("cycle").bool();let mut replay=Replay{calls:v.get("queries").items().collect(),cursor:0};
            let result=run(&mut replay,cycle).expect("a successful source invocation");
            let golden=expected(v.get("expected"),cycle);
            assert!(same(&result,&golden),"{} tick {}: {result:?} != {golden:?}",path.display(),record.tick);
            assert_eq!(replay.cursor,replay.calls.len(),"unused callbacks");
            callbacks+=replay.cursor;frames+=1;
            *totals.entry(v.get("mode").str().unwrap().to_string()).or_default()+=1;
        }
        assert!(frames>0,"{} has no frames",path.display());total+=frames;
        eprintln!("{}: {frames} exact",path.file_name().unwrap().to_string_lossy());
    }
    for (mode,n) in totals {eprintln!("  {mode}: {n}");}
    eprintln!("network replay: {total}/{total} exact, {callbacks} callbacks, zero different or not replayed");
}

#[test]
fn a_failed_import_aborts_without_later_callbacks() {
    struct Fail(usize);
    impl Transport for &mut Fail {fn call(&mut self,_:NetworkSeatsOp,_:Vec<f64>)->Result<Vec<f64>,()>{self.0+=1;Err(())}}
    for cycle in [false,true] {let mut f=Fail(0);assert_eq!(run(&mut f,cycle),Err(()));assert_eq!(f.0,1);}
}
