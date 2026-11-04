//! Every captured AI combat service and agent entry point, with exact
//! state, boundary arguments and random-stream order checks.
#![cfg(feature = "replay")]
use bfe_sim::ai::combat::replay::replay_frame;
use sim_replay::{Corpus, Event, Frame, Node, Outcome, Report, Value};

#[test]
fn ai_combat_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let only = std::env::var("BFE_REPLAY_ONLY").ok();
    let tick = std::env::var("BFE_REPLAY_TICK").ok().map(|s| s.parse::<u32>().unwrap());
    let fail_fast = std::env::var_os("BFE_REPLAY_FAIL_FAST").is_some();
    let mut reports: Vec<Report> = Vec::new();
    for capture in corpus.captures() {
        let capture = capture.unwrap();
        for spec in capture.header.root().get("frames").items() {
            let name = spec.get("name").str().unwrap();
            if !(name.starts_with("aiCombat.") || name.starts_with("UnitCombatAgent.")) || only.as_ref().is_some_and(|only| !name.contains(only)) { continue; }
            let index = reports.iter().position(|r| r.name == name).unwrap_or_else(|| { reports.push(Report::new(name)); reports.len()-1 });
            let report = &mut reports[index]; report.captures += 1;
            for frame in capture.frames(name) {
                let frame = frame.unwrap();
                if tick.is_some_and(|t| t != frame.tick) { continue; }
                let outcome = match replay_frame(&capture,&frame) { Ok(()) => Outcome::Same, Err(e) => Outcome::Failed(e) };
                report.add(&capture,&frame,outcome);
                if fail_fast { report.assert_clean(); }
            }
        }
    }
    assert!(reports.iter().any(|r| r.replayed() > 0), "no matching AI combat frames");
    for report in &reports { eprintln!("{}",report.summary()); }
    if let Some(path) = std::env::var_os("BFE_REPLAY_FAILURES") {
        let failures: Vec<_> = reports.iter().flat_map(|r| r.failures.iter()).map(String::as_str).collect();
        std::fs::write(path,failures.join("\n\n")).unwrap();
    }
    for report in &reports { report.assert_clean(); }
}

fn move_field(frame: &mut Frame, key: &str, zero_only: bool) -> bool {
    for node in &mut frame.exit.arena.nodes {
        let Node::Object {fields,..}=node else {continue};
        for (k,v) in fields {
            if k!=key {continue}
            let Value::Num(n)=v else {continue};
            if zero_only && *n!=0.0 {continue}
            *n=if zero_only {-*n} else if n.is_finite() {f64::from_bits(n.to_bits()+1)} else {0.0};
            return true;
        }
    }
    false
}

/// Mutate the actual captured evidence independently of the port. Draw
/// order, exact floats, shared caches and served-call preconditions must
/// each reject a mismatch.
#[test]
fn ai_combat_replay_mutations_are_caught() {
    let Some(corpus)=Corpus::from_env().unwrap() else {return};
    let mut caught=[false;10];
    for capture in corpus.captures() {
        let capture=capture.unwrap();
        for name in ["UnitCombatAgent.tick","aiCombat.consumeFire","aiCombat.aim","aiCombat.scan","aiCombat.ingest"] {
            for frame in capture.frames(name) {
                let frame=frame.unwrap();
                if replay_frame(&capture,&frame).is_err() {continue}
                for (probe,key,zero) in [(4,"roll",false),(5,"aimState",true)] {
                    if caught[probe] {continue}
                    let mut m=frame.clone();
                    if move_field(&mut m,key,zero) {assert!(replay_frame(&capture,&m).is_err(),"mutation {probe} replayed clean");caught[probe]=true;}
                }
                for (probe,table,key) in [(6,"reservations","at"),(7,"clusters","scanAt")] {
                    if caught[probe] {continue}
                    let mut m=frame.clone();let item=m.after().get(table).at(0);
                    let Some(n)=item.get(key).opt_num() else {continue};
                    let object=item.value.clone();let n=if n.is_finite(){f64::from_bits(n.to_bits()+1)}else{0.0};
                    m.exit.arena.assign(&object,key,Value::Num(n));
                    assert!(replay_frame(&capture,&m).is_err(),"mutation {probe} replayed clean");caught[probe]=true;
                }
                let draws:Vec<_>=frame.events.iter().enumerate().filter_map(|(i,e)|matches!(e,Event::Draw(_)).then_some(i)).collect();
                for &i in &draws {
                    if caught[0] {break}
                    let mut m=frame.clone();
                    if let Event::Draw(d)=&mut m.events[i] {d.value=((d.value as u32)^0x8000_0000) as f64;}
                    caught[0]=replay_frame(&capture,&m).is_err();
                }
                if let Some(&i)=draws.first() {
                    if !caught[1] {let mut m=frame.clone();m.events.insert(i,m.events[i].clone());assert!(replay_frame(&capture,&m).is_err(),"extra draw replayed clean");caught[1]=true;}
                    if !caught[2] {let mut m=frame.clone();if let Event::Draw(d)=&mut m.events[i] {d.stream="wrong-stream".into();}assert!(replay_frame(&capture,&m).is_err(),"wrong stream replayed clean");caught[2]=true;}
                }
                if !caught[3] {
                    let calls:Vec<_>=frame.events.iter().enumerate().filter_map(|(i,e)|if let Event::Call(c)=e {Some((i,c.name.as_str()))} else {None}).collect();
                    if let Some(&(a,name))=calls.first() {if let Some(&(b,_))=calls.iter().find(|(_,n)|*n!=name) {
                        let mut m=frame.clone();m.events.swap(a,b);assert!(replay_frame(&capture,&m).is_err(),"swapped calls replayed clean");caught[3]=true;
                    }}
                }
                if !caught[8] {
                    let mut m=frame.clone();
                    for event in &mut m.events {
                        let Event::Call(call)=event else {continue};
                        let c=call.extra().get("before").get("ctls").at(0);
                        let Some(roll)=c.get("roll").opt_num() else {continue};
                        let object=c.value.clone();call.ret.arena.assign(&object,"roll",Value::Num(f64::from_bits(roll.to_bits()+1)));
                        caught[8]=true;break;
                    }
                    if caught[8] {assert!(replay_frame(&capture,&m).is_err(),"foreign call precondition replayed clean");}
                }
                if !caught[9] && name=="aiCombat.ingest" {
                    let mut m=frame.clone();let root=m.exit.root().value.clone();m.exit.arena.assign(&root,"r",Value::Bool(false));
                    assert!(replay_frame(&capture,&m).is_err(),"changed void return replayed clean");caught[9]=true;
                }
                if caught.iter().all(|x|*x) {eprintln!("AI combat mutation probes: 10 rejected");return}
            }
        }
    }
    assert!(caught.iter().all(|x|*x),"missing mutation coverage: {caught:?}");
}
