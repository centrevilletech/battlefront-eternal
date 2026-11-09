//! Entity-event admission and resource queries. Browser objects remain opaque;
//! the owner selects indices and returns ordered presentation commands.
use serde_json::{json,Value};
fn n(v:&Value,k:&str)->f64{v[k].as_f64().unwrap_or(0.0)}
fn b(v:&Value,k:&str)->bool{v[k].as_bool().unwrap_or(false)}
fn s<'a>(v:&'a Value,k:&str)->&'a str{v[k].as_str().unwrap_or("")}
fn list(v:&Value)->&[Value]{v.as_array().map(Vec::as_slice).unwrap_or(&[])}
fn truth(v:&Value)->bool{match v{Value::Null=>false,Value::Bool(v)=>*v,Value::Number(v)=>v.as_f64().is_some_and(|v|v!=0.0),Value::String(v)=>!v.is_empty(),_=>true}}
// This cheap probe precedes cold descriptor construction. A pending event can
// retry missing resources each displayed frame without allocating a plan.
#[allow(clippy::too_many_arguments)]
pub fn ready(kind:&str,owner:f64,generation:f64,present:bool,actual:f64,mount:f64,mount_generation:f64,mount_present:bool,mount_actual:f64)->bool{
    kind=="lightning"||((owner==0.0||present&&generation==actual)&&(mount==0.0||mount_present&&mount_generation==mount_actual))
}
// Weapon order is vehicle seat, turret, then infantry arsenal. The host sends
// every candidate, including its seat, rather than choosing a fallback itself.
fn weapon(e:&Value,f:&Value)->Option<usize>{
    let rows=list(&f["weapons"]);let name=&f["name"];
    rows.iter().position(|w|s(w,"role")=="vehicle"&&w["seat"]==e["seat"]&&w["name"]==*name)
        .or_else(||rows.iter().position(|w|s(w,"role")=="turret"))
        .or_else(||rows.iter().position(|w|s(w,"role")=="combat"&&w["name"]==*name))
}
pub fn plan(text:&str,facts:&str)->Result<String,String>{
    let e:Value=serde_json::from_str(text).map_err(|e|e.to_string())?;
    let f:Value=serde_json::from_str(facts).map_err(|e|e.to_string())?;
    let kind=s(&e,"presentationKind");let mut out=Vec::new();
    let mut emit=|v:Value|out.push(v);
    let fail=||Ok(json!({"accepted":false,"commands":[]}).to_string());
    let point=e["position"].is_array();let own=b(&f,"own");
    match kind {
        // Lightning is already placed by the authority. An unknown shooter
        // leaves its start fixed instead of suppressing the visible bolt.
        "lightning"=>{
            if !b(&f,"lightning")||!e["from"].is_array()||!e["to"].is_array(){return fail();}
            emit(json!(["lightning",!e["channel"].is_null()&&b(&f,"combat")&&n(&e,"ownerGeneration")==n(&f,"generation")]));
        }
        "effect"=>{if !truth(&e["name"])||!point{return fail();}emit(json!(["effect"]));if s(&e,"name")=="headshot"{emit(json!(["headshot"]));}}
        // Only authored action/posture pairs reach the animator. The prepare
        // view uses the first busy weapon in the selected animation bank.
        "weaponAction"|"endWeaponAction"=>{
            if !b(&f,"combat"){return fail();}
            if kind=="endWeaponAction"{emit(json!(["endAction"]));}else{
                if !["reload","charge","shoot","shoot2","shoot3","shoot4","shoot5"].contains(&s(&e,"action"))||!["stand","crouch","prone"].contains(&s(&e,"posture")){return Err("Invalid weapon animation".into());}
                emit(json!(["action"]));
                if b(&e,"prepare")&&b(&f,"firstPerson"){
                    if let Some(i)=list(&f["weapons"]).iter().position(|w|s(w,"role")=="combat"&&w["bank"]==e["bank"]&&b(w,"busy")){emit(json!(["prepare",i]));}
                }
            }
        }
        // The HUD sees an accepted shot before the foreign prediction claim.
        // A successful claim ends command dispatch before sound and flashes.
        "shot"=>{
            let Some(i)=weapon(&e,&f)else{return fail()};let w=&f["weapons"][i];
            emit(json!(["hud",i]));if b(&f,"owner"){emit(json!(["claim"]));}
            // Mounted fire keeps vehicle/turret audio precedence. Infantry
            // repairs and ordinary shots invoke their respective view resource.
            let mounted=b(&f,"mount");emit(json!(["sound",i,if mounted{"turretFire"}else if own{"fire"}else{"actorFire"},if b(&f,"vehicle"){0}else if b(&f,"turret"){1}else{2}]));
            if !mounted&&b(&e,"repair"){emit(json!(["repair",i]));}
            else if !mounted&&!b(&e,"melee"){emit(json!(["projectile",i]));}
            else if b(&f,"vehicle")&&point{
                emit(json!(["kick",i]));if truth(&w["flash"]){emit(json!(["flashEffect",w["flash"]]));}emit(json!(["flash",[1.0,0.55,0.2],5.0,0.06]));
            }
            if b(&e,"melee"){emit(json!(["melee"]));}
        }
        // Droid guns are queried within their registered droid definition;
        // those individual gun classes have no independent wire identities.
        "remoteShot"=>{
            if s(&f,"base")!="droid"||!point{return fail();}
            let Some(w)=list(&f["droidWeapons"]).iter().find(|w|n(w,"channel")==n(&e,"channel"))else{return fail()};
            emit(json!(["flash",[1.0,0.75,0.75],n(w,"FlashLightRadius"),n(w,"FlashLightDuration")]));
        }
        // The viewer decides whether a voice is positional. Remote voices
        // always use the source position, regardless of the sender flag.
        "vo"=>emit(json!(["vo",own,if own{e["positional"].clone()}else{json!(true)}])),
        "squad"=>{if own{emit(json!(["squad"]));}}
        "shieldHit"|"deathBlast"|"blowApart"|"remoteBlast"=>emit(json!(["present",kind=="remoteBlast",n(&e,"amount"),if truth(&e["hitKind"]){e["hitKind"].clone()}else{Value::Null}])),
        "bounce"|"pickup"=>{if !b(&f,"definition")||!point{return fail();}emit(json!(["objectSound"]));}
        // A supply station can arrive after its event. Failed resource
        // admission remains retryable without advancing a presentation clock.
        "serve"=>{if !b(&f,"station"){return fail();}emit(json!(["serve",own]));}
        // Tow sounds select a weapon across all seats in their stored order.
        // Their resource host receives the definition, not its state facade.
        "towAttach"|"towRelease" if b(&f,"ownerVehicle")=>{
            if let Some(i)=list(&f["towWeapons"]).iter().position(|w|w["name"]==f["name"]){emit(json!(["tow",i]));}
        }
        _=>{}
    }
    Ok(json!({"accepted":true,"commands":out}).to_string())
}
// attachPresentationCapture describes the producer's semantic event. Native
// selection preserves the producer channel and the matching trigger identity.
pub fn capture(text:&str)->Result<String,String>{
    fn ordered(fields:&[(&str,Value)])->String{
        let mut out=String::from("{");
        for(i,(k,v))in fields.iter().enumerate(){if i>0{out.push(',');}out.push_str(&serde_json::to_string(k).unwrap());out.push(':');out.push_str(&v.to_string());}
        out.push('}');out
    }
    let f:Value=serde_json::from_str(text).map_err(|e|e.to_string())?;let kind=s(&f,"kind");
    let owner=||json!(n(&f,"ownerId"));let generation=||json!(n(&f,"ownerGeneration"));
    // Capture never infers simulation results from presentation resources.
    // It only describes the semantic event the simulation already emitted.
    let fields:Vec<(&str,Value)>=match kind {
        "presentation"=>return Ok(f["event"].to_string()),
        "vo"=>vec![("kind",json!(kind)),("ownerId",owner()),("ownerGeneration",generation()),("team",json!(n(&f,"team"))),("position",f["position"].clone()),("human",json!(b(&f,"human"))),("property",json!(f["property"].as_str().unwrap_or(""))),("positional",json!(f["positional"]!=false))],
        "shot"=>{
            // The trigger identity follows the selected gun channel, so
            // secondary fire cannot consume the primary prediction claim.
            let index=weapon(&json!({"seat":f["seat"]}),&f);let channel=index.map(|i|n(&f["weapons"][i],"channel")).unwrap_or(0.0);
            let fire=if f["shots"].is_array(){list(&f["shots"]).iter().find(|e|n(e,"button")==channel).map(|e|n(e,"id")).unwrap_or(0.0)}else{n(&f,"fireId")};
            vec![("kind",json!(kind)),("ownerId",owner()),("ownerGeneration",generation()),("mountId",json!(n(&f,"mountId"))),("mountGeneration",json!(n(&f,"mountGeneration"))),("seat",json!(n(&f,"seat"))),("fireId",json!(fire)),("channel",json!(channel)),("weaponId",json!(n(&f,"weaponId"))),("position",f["position"].clone()),("repair",json!(b(&f,"repair"))),("melee",json!(b(&f,"melee"))),("swing",json!(n(&f,"swing")))]
        }
        "remoteShot"=>vec![("kind",json!(kind)),("definitionId",json!(n(&f,"definitionId"))),("channel",json!(if n(&f,"channel")==1.0{1}else{0})),("position",f["position"].clone())],
        "bounce"|"pickup"=>vec![("kind",json!(kind)),("definitionId",json!(n(&f,"definitionId"))),("position",f["position"].clone()),("amount",json!(n(&f,"amount")))],
        // Match supply objects by host identity, retaining the first unit
        // just as the producer iterator did before this query moved here.
        "serve"=>{let Some(building)=list(&f["buildings"]).iter().position(|v|v==true)else{return Ok("null".into())};let u=list(&f["units"]).iter().find(|u|b(u,"supply"));vec![("kind",json!(kind)),("building",json!(building)),("ownerId",json!(u.map(|u|n(u,"id")).unwrap_or(0.0))),("ownerGeneration",json!(u.map(|u|n(u,"generation")).unwrap_or(0.0)))]}
        "towAttach"|"towRelease"=>{if !list(&f["vehicles"]).iter().any(|v|v==true){return Ok("null".into());}vec![("kind",json!(kind)),("ownerId",owner()),("ownerGeneration",generation()),("weaponId",json!(n(&f,"weaponId"))),("reason",if truth(&f["reason"]){f["reason"].clone()}else{Value::Null})]}
        _=>return Ok("null".into())
    };Ok(ordered(&fields))
}
