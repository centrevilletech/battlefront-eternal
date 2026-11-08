//! Recorded session transport. All JSON, range validation and scalar decoding
//! happens at admission; frame submission borrows retained native commands.
use serde_json::Value;
use std::path::Path;

/// Scalar banks are plain Rust numbers with initialized bytes. The capture
/// protocol uses little endian and refuses other hosts at admission.
mod scalar {
    pub trait Sealed {}
    impl Sealed for f64 {}
    impl Sealed for f32 {}
    impl Sealed for u32 {}
    impl Sealed for i32 {}
    impl Sealed for u16 {}
    impl Sealed for u8 {}
}
// Sealed to initialized, padding-free numbers. Copy alone would also admit
// structs with uninitialized padding, which cannot safely become byte slices.
pub trait Scalar: Copy + scalar::Sealed {}
impl Scalar for f64 {}
impl Scalar for f32 {}
impl Scalar for u32 {}
impl Scalar for i32 {}
impl Scalar for u16 {}
impl Scalar for u8 {}
pub fn bytes_mut<T: Scalar>(values: &mut [T]) -> &mut [u8] {
    unsafe {
        std::slice::from_raw_parts_mut(values.as_mut_ptr().cast(), std::mem::size_of_val(values))
    }
}
pub fn bytes<T: Scalar>(values: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
}
// Shared network banks retain their RefCell loan only while a host copy runs.
// The remaining owners lend slices directly; neither form creates a mirror.
pub enum Bank<'a> {
    Slice(&'a [u8]),
    Shared(std::cell::Ref<'a, [u8]>),
}
pub enum BankMut<'a> {
    Slice(&'a mut [u8]),
    Shared(std::cell::RefMut<'a, [u8]>),
}
impl std::ops::Deref for Bank<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Slice(v) => v,
            Self::Shared(v) => v,
        }
    }
}
impl AsRef<[u8]> for Bank<'_> {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
impl std::ops::Deref for BankMut<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Slice(v) => v,
            Self::Shared(v) => v,
        }
    }
}
impl std::ops::DerefMut for BankMut<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Slice(v) => v,
            Self::Shared(v) => v,
        }
    }
}
pub struct BlobStore {
    data: Vec<u8>,
    spans: Vec<(usize, usize)>,
    assets: std::collections::BTreeMap<String, Vec<u8>>,
}
impl BlobStore {
    pub fn load(dir: &Path, meta: &Value) -> Result<Self, String> {
        if !cfg!(target_endian = "little") {
            return Err("session requires a little endian host".into());
        }
        let data = std::fs::read(dir.join("blobs.bin")).map_err(|e| e.to_string())?;
        let mut spans = Vec::new();
        for span in meta["blobs"].as_array().ok_or("missing blob table")? {
            let at = span[0].as_u64().ok_or("invalid blob offset")? as usize;
            let len = span[1].as_u64().ok_or("invalid blob length")? as usize;
            if at.checked_add(len).is_none_or(|end| end > data.len()) {
                return Err("blob span exceeds archive".into());
            }
            spans.push((at, len));
        }
        Ok(Self {
            data,
            spans,
            assets: Default::default(),
        })
    }
    pub fn get(&self, id: usize) -> Result<&[u8], String> {
        let &(at, len) = self.spans.get(id).ok_or("unknown blob")?;
        Ok(&self.data[at..at + len])
    }
}
pub struct Arguments<'a> {
    args: &'a [Value],
    blobs: &'a BlobStore,
}
impl<'a> Arguments<'a> {
    pub fn new(args: &'a [Value], blobs: &'a BlobStore) -> Self {
        Self { args, blobs }
    }
    fn arg(&self, i: usize) -> Result<&Value, String> {
        self.args
            .get(i)
            .ok_or_else(|| format!("missing argument {i}"))
    }
    pub fn string(&self, i: usize) -> Result<String, String> {
        Ok(self.arg(i)?.as_str().ok_or("expected string")?.into())
    }
    pub fn bool(&self, i: usize) -> Result<bool, String> {
        let a = self.arg(i)?;
        Ok(a.as_bool().unwrap_or_else(|| a.as_i64().unwrap_or(0) != 0))
    }
    pub fn f64(&self, i: usize) -> Result<f64, String> {
        self.arg(i)?
            .as_f64()
            .ok_or_else(|| "expected finite number".into())
    }
    pub fn f32(&self, i: usize) -> Result<f32, String> {
        Ok(self.f64(i)? as f32)
    }
    pub fn u32(&self, i: usize) -> Result<u32, String> {
        u32::try_from(self.arg(i)?.as_u64().ok_or("expected unsigned number")?)
            .map_err(|_| "integer exceeds u32".into())
    }
    pub fn i32(&self, i: usize) -> Result<i32, String> {
        i32::try_from(self.arg(i)?.as_i64().ok_or("expected signed number")?)
            .map_err(|_| "integer exceeds i32".into())
    }
    pub fn usize(&self, i: usize) -> Result<usize, String> {
        Ok(self.u32(i)? as usize)
    }
    pub fn owner(&self, i: usize) -> Result<usize, String> {
        Ok(self.arg(i)?["owner"]
            .as_u64()
            .ok_or("expected owner reference")? as usize)
    }
    pub fn raw(&self, i: usize) -> Result<&[u8], String> {
        if let Some(asset) = self.arg(i)?["asset"].as_str() {
            self.blobs
                .assets
                .get(asset)
                .map(Vec::as_slice)
                .ok_or_else(|| format!("unknown native asset {asset}"))
        } else {
            self.blobs.get(
                self.arg(i)?["blob"]
                    .as_u64()
                    .ok_or("expected blob reference")? as usize,
            )
        }
    }
}
macro_rules! arrays{($($method:ident,$ty:ty,$n:literal);*)=>{$(impl Arguments<'_>{pub fn $method(&self,i:usize)->Result<Vec<$ty>,String>{let data=self.raw(i)?;if data.len()%$n!=0{return Err("unaligned scalar blob".into());}Ok(data.chunks_exact($n).map(|b|<$ty>::from_le_bytes(b.try_into().unwrap())).collect())}})*};}
arrays!(f64s,f64,8;f32s,f32,4;u32s,u32,4;i32s,i32,4;u16s,u16,2;u8s,u8,1);

use crate::presentation_bridge::*;
// Cold owner links borrow distinct resources in their native store. Sorting
// three stack indices avoids allocation and never aliases mutable owners.
fn three_owners(rows:&mut [PresentationOwner],a:usize,b:usize,c:usize)->Result<(&mut PresentationOwner,&mut PresentationOwner,&mut PresentationOwner),String>{
    let mut ids=[a,b,c];ids.sort_unstable();
    if ids[0]==ids[1]||ids[1]==ids[2]||ids[2]>=rows.len(){return Err("invalid source owner indices".into());}
    let (low,high)=rows.split_at_mut(ids[2]);let z=&mut high[0];
    let (low,mid)=low.split_at_mut(ids[1]);let y=&mut mid[0];let x=&mut low[ids[0]];
    Ok(if a==ids[0]{if b==ids[1]{(x,y,z)}else{(x,z,y)}}
        else if a==ids[1]{if b==ids[0]{(y,x,z)}else{(y,z,x)}}
        else if b==ids[0]{(z,x,y)}else{(z,y,x)})
}
pub enum PresentationOwner {
    Wake(PageWake),
    Jetpack(PageJetpack),
    JetVolume(PageJetVolume),
    Trail(PageTrail),
    Shield(PageShield),
    DeathTrace(PageDeathTrace),
    GunnerView(PageGunnerView),
    SoldierView(PageSoldierView),
    PostEffects(PagePostEffects),
    VehicleEffects(PageVehicleEffects),
    AnalyticEffects(PageAnalyticEffects),

    Effects(PagePresentation),
    Units(PageUnits),
    Props(PageProps),
    Vehicles(PageVehicles),
    Items(PageItems),
    Ordnance(PageOrdnance),
    Combat(PageCombat),
    Feet(PageFeet),
    Sniper(PageSniper),
    Lightning(PageLightning),
    ArcCamera(PageArcCamera),
    Network(PageNetwork),
    Warning(PageWarning),
    Lights(PageLights),
    ShellMusic(PageShellMusic),
    Interface(PageInterfaceBatch),
    Timeline(bfe_pose::timeline::Timeline),
}
pub enum PresentationCommand {
    Wake(PageWakeCommand),
    Jetpack(PageJetpackCommand),
    JetVolume(PageJetVolumeCommand),
    Trail(PageTrailCommand),
    Shield(PageShieldCommand),
    DeathTrace(PageDeathTraceCommand),
    GunnerView(PageGunnerViewCommand),
    SoldierView(PageSoldierViewCommand),
    PostEffects(PagePostEffectsCommand),
    VehicleEffects(PageVehicleEffectsCommand),
    AnalyticEffects(PageAnalyticEffectsCommand),

    Effects(PagePresentationCommand),
    Units(PageUnitsCommand),
    Props(PagePropsCommand),
    Vehicles(PageVehiclesCommand),
    Items(PageItemsCommand),
    Ordnance(PageOrdnanceCommand),
    Combat(PageCombatCommand),
    Feet(PageFeetCommand),
    Sniper(PageSniperCommand),
    Lightning(PageLightningCommand),
    ArcCamera(PageArcCameraCommand),
    Network(PageNetworkCommand),
    Warning(PageWarningCommand),
    Lights(PageLightsCommand),
    ShellMusic(PageShellMusicCommand),
    Interface(PageInterfaceBatchCommand),
    Push,
    Present,
    Reset(f64),
    Remove(f64),
}
macro_rules! owners {($($case:ident,$class:literal,$cmd:ident);*)=>{
impl PresentationOwner {
    pub fn class(&self)->&'static str{match self{$(Self::$case(_)=>$class,)*Self::Network(_)=>"pagenetwork",Self::Timeline(_)=>"timeline"}}
    pub fn bank(&mut self,name:&str)->Result<BankMut<'_>,String>{match self{$(Self::$case(p)=>p.bank(name).map(BankMut::Slice),)*Self::Network(p)=>p.bank(name),Self::Timeline(t)=>match name{"input"=>Ok(BankMut::Slice(bytes_mut(&mut t.input))),"output"=>Ok(BankMut::Slice(bytes_mut(&mut t.output))),_=>Err("unknown timeline bank".into())}}}
    pub fn read_bank(&self,name:&str)->Result<Bank<'_>,String>{match self{$(Self::$case(p)=>p.read_bank(name).map(Bank::Slice),)*Self::Network(p)=>p.read_bank(name),Self::Timeline(t)=>match name{"input"=>Ok(Bank::Slice(bytes(&t.input))),"output"=>Ok(Bank::Slice(bytes(&t.output))),_=>Err("unknown timeline bank".into())}}}
    pub fn lengths(&self)->Vec<(&'static str,usize)>{match self{$(Self::$case(p)=>p.bank_lengths(),)*Self::Network(p)=>p.bank_lengths(),Self::Timeline(t)=>vec![("input",t.input.len()),("output",t.output.len())]}}
    pub fn decode(&self,name:&str,args:&[Value],blobs:&BlobStore)->Result<PresentationCommand,String>{Ok(match self{$(Self::$case(_)=>PresentationCommand::$case($cmd::decode(name,args,blobs)?),)*Self::Network(_)=>PresentationCommand::Network(PageNetworkCommand::decode(name,args,blobs)?),Self::Timeline(_)=>match name{"push"=>PresentationCommand::Push,"present"=>PresentationCommand::Present,"reset"=>PresentationCommand::Reset(Arguments::new(args,blobs).f64(0)?),"remove"=>PresentationCommand::Remove(Arguments::new(args,blobs).f64(0)?),_=>return Err("unknown timeline command".into())}})}
}
impl PresentationCommand {pub fn execute(&self,owner:&mut PresentationOwner)->Result<(),String>{match(self,owner){$((Self::$case(c),PresentationOwner::$case(p))=>c.execute(p),)*(Self::Network(c),PresentationOwner::Network(p))=>c.execute(p),(Self::Push,PresentationOwner::Timeline(t))=>{t.push();Ok(())},(Self::Present,PresentationOwner::Timeline(t))=>{t.present();Ok(())},(Self::Reset(e),PresentationOwner::Timeline(t))=>{t.reset(*e);Ok(())},(Self::Remove(e),PresentationOwner::Timeline(t))=>{t.remove(*e);Ok(())},_=>Err("presentation owner type mismatch".into())}}}
};}
owners!(Combat,"pagecombat",PageCombatCommand;Wake,"pagewake",PageWakeCommand;Jetpack,"pagejetpack",PageJetpackCommand;JetVolume,"pagejetvolume",PageJetVolumeCommand;Trail,"pagetrail",PageTrailCommand;Shield,"pageshield",PageShieldCommand;DeathTrace,"pagedeathtrace",PageDeathTraceCommand;GunnerView,"pagegunnerview",PageGunnerViewCommand;SoldierView,"pagesoldierview",PageSoldierViewCommand;PostEffects,"pageposteffects",PagePostEffectsCommand;VehicleEffects,"pagevehicleeffects",PageVehicleEffectsCommand;AnalyticEffects,"pageanalyticeffects",PageAnalyticEffectsCommand;Lightning,"pagelightning",PageLightningCommand;ArcCamera,"pagearccamera",PageArcCameraCommand;Feet,"pagefeet",PageFeetCommand;Sniper,"pagesniper",PageSniperCommand;Effects,"pagepresentation",PagePresentationCommand;Units,"pageunits",PageUnitsCommand;Props,"pageprops",PagePropsCommand;Vehicles,"pagevehicles",PageVehiclesCommand;Items,"pageitems",PageItemsCommand;Ordnance,"pageordnance",PageOrdnanceCommand;Warning,"pagewarning",PageWarningCommand;Lights,"pagelights",PageLightsCommand;ShellMusic,"pageshellmusic",PageShellMusicCommand;Interface,"pageinterfacebatch",PageInterfaceBatchCommand);
impl PresentationOwner {
    pub fn new(class: &str, content: &str, preview: bool) -> Result<Self, String> {
        Ok(match class {
            "pagewake" => Self::Wake(PageWake::new()),
            "pagejetpack" => Self::Jetpack(PageJetpack::new(content)?),
            "pagejetvolume" => Self::JetVolume(PageJetVolume::new()),
            "pagetrail" => Self::Trail(PageTrail::new(content)),
            "pageshield" => Self::Shield(PageShield::new()),
            "pagedeathtrace" => Self::DeathTrace(PageDeathTrace::new()),
            "pagegunnerview" => Self::GunnerView(PageGunnerView::new()),
            "pagesoldierview" => Self::SoldierView(PageSoldierView::new()),
            "pageposteffects" => Self::PostEffects(PagePostEffects::new(content)?),
            "pagevehicleeffects" => Self::VehicleEffects(PageVehicleEffects::new()),
            "pageanalyticeffects" => Self::AnalyticEffects(PageAnalyticEffects::new(content)?),
            "pagepresentation" => Self::Effects(PagePresentation::new(content)?),
            "pageunits" => Self::Units(if preview {
                PageUnits::preview()
            } else {
                PageUnits::new()
            }),
            "pageprops" => Self::Props(PageProps::new()),
            "pagevehicles" => Self::Vehicles(PageVehicles::new()),
            "pageitems" => Self::Items(PageItems::new()),
            "pageordnance" => Self::Ordnance(PageOrdnance::new()),
            "pagecombat" => Self::Combat(PageCombat::new(content.parse().map_err(|_| "invalid combat capacity")?)),
            "pagefeet" => Self::Feet(PageFeet::new()),
            "pagelightning" => Self::Lightning(PageLightning::new()),
            "pagearccamera" => Self::ArcCamera(PageArcCamera::new()),
            "pagesniper" => Self::Sniper(PageSniper::new()),
            "pagenetwork" => Self::Network(PageNetwork::new()),
            "pagewarning" => Self::Warning(PageWarning::new()),
            "pagelights" => Self::Lights(PageLights::new(
                content.parse().map_err(|_| "invalid light capacity")?,
            )),
            "pageshellmusic" => Self::ShellMusic(PageShellMusic::new(content)?),
            "pageinterfacebatch" => Self::Interface(PageInterfaceBatch::new()),
            "timeline" => Self::Timeline(bfe_pose::timeline::Timeline::new()),
            _ => return Err(format!("unknown presentation class {class}")),
        })
    }
}
pub fn presentation_schema() -> Value {
    let mut map = serde_json::Map::new();
    for class in [
        "pagewake",
        "pagejetpack",
        "pagejetvolume",
        "pagetrail",
        "pageshield",
        "pagedeathtrace",
        "pagegunnerview",
        "pagesoldierview",
        "pageposteffects",
        "pagevehicleeffects",
        "pageanalyticeffects",
        "pagepresentation",
        "pageunits",
        "pageprops",
        "pagevehicles",
        "pageitems",
        "pageordnance",
        "pagecombat",
        "timeline",
        "pagenetwork",
        "pagewarning",
        "pagelights",
        "pageshellmusic",
        "pageinterfacebatch",
        "pagelightning",
        "pagearccamera",
    ] {
        // Empty authored effects still constructs the same bounded scalar banks.
        if let Ok(mut p) = PresentationOwner::new(
            class,
            if class == "pagelights" || class == "pagecombat" {
                "0"
            } else if ["pageshellmusic","pagejetpack","pageanalyticeffects"].contains(&class) {
                "[]"
            } else {
                r#"{"definitions":[],"groups":[]}"#
            },
            false,
        ) {
            if let PresentationOwner::Network(n) = &mut p {
                n.receive_setup();
            }
            map.insert(
                class.into(),
                p.lengths()
                    .into_iter()
                    .map(|(n, l)| (n.to_string(), Value::from(l)))
                    .collect::<serde_json::Map<_, _>>()
                    .into(),
            );
        }
    }
    map.insert("instance".into(),serde_json::json!({"world":bfe_render::instances::REC_WORLD,"inverse":bfe_render::instances::REC_BIND_INVERSE,"boneAt":bfe_render::instances::REC_BONE_AT,"bones":bfe_render::instances::REC_BONES,"nodeStride":bfe_presentation::layout::unit_node::STRIDE,"nodeWorld":bfe_presentation::layout::unit_node::WORLD,"outputStride":bfe_presentation::layout::unit_output::STRIDE,"outputInverse":bfe_presentation::layout::unit_output::BIND_INVERSE}));
    map.insert("stats".into(),serde_json::json!({"prefix":bfe_render::core::Stats::default().values().len()-bfe_render::core::PASSES,"names":["world","instances","far","terrain","transparent","effects","foliage","view","post","hud","water","shadow","motion","canopies","screen"]}));
    map.into()
}

use crate::{
    render_bridge::{Bridge, Command},
    resources::Resources,
};
use glow::HasContext;
use sha2::{Digest, Sha256};
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Clear {
    mask: u32,
    color: [f32; 4],
    depth: f32,
    stencil: i32,
    color_mask: [bool; 4],
    depth_mask: bool,
    scissor: bool,
    #[serde(rename = "box")]
    rect: [i32; 4],
}
enum ResourceCommand {
    Source(String),
    Retain(u32),
    WorldFallback(bool),
    Canopy(u32, Vec<u8>),
    UpdateCanopy(u32, Vec<u8>),
    Desc(u32, String),
    Pixels(u32, Vec<u8>),
    Png(u32, Vec<u8>),
    Blocks(u32, Vec<u8>, String),
    Release(u32),
    Sample(u32, String),
    Sampling(u32, String),
    Dispose(u32),
    Upload(u32),
    Target(String),
    Resize(u32, String),
    DisposeTarget(u32),
    Bind(i32, u32, i32, [i32; 4], [i32; 4], bool),
    Resolve(u32),
    Copy(u32, u32, u32),
    Mipmap(u32),
}
impl ResourceCommand {
    fn decode(name: &str, args: &[Value], b: &BlobStore) -> Result<Self, String> {
        let a = Arguments::new(args, b);
        Ok(match name {
            "retain_sample" => Self::Retain(a.u32(0)?),
            "world_fallback" => Self::WorldFallback(a.bool(0)?),
            "canopy_state" => Self::Canopy(a.u32(0)?, a.raw(1)?.to_vec()),
            "update_canopy_state" => Self::UpdateCanopy(a.u32(0)?, a.raw(1)?.to_vec()),
            "source" => Self::Source(a.string(0)?),
            "source_desc" => Self::Desc(a.u32(0)?, a.string(1)?),
            "source_pixels" | "source_canvas" => Self::Pixels(a.u32(0)?, a.raw(1)?.to_vec()),
            "source_png" => Self::Png(a.u32(0)?, a.raw(1)?.to_vec()),
            "source_blocks" => Self::Blocks(a.u32(0)?, a.raw(1)?.to_vec(), a.string(2)?),
            "release_source" => Self::Release(a.u32(0)?),
            "sample" => Self::Sample(a.u32(0)?, a.string(1)?),
            "sampling" => Self::Sampling(a.u32(0)?, a.string(1)?),
            "dispose_sample" => Self::Dispose(a.u32(0)?),
            "upload" => Self::Upload(a.u32(0)?),
            "target" => Self::Target(a.string(0)?),
            "resize_target" => Self::Resize(a.u32(0)?, a.string(1)?),
            "dispose_target" => Self::DisposeTarget(a.u32(0)?),
            "resolve" => Self::Resolve(a.u32(0)?),
            "copy" => Self::Copy(a.u32(0)?, a.u32(1)?, a.u32(2)?),
            "mipmap" => Self::Mipmap(a.u32(0)?),
            "bind" => Self::Bind(
                a.i32(0)?,
                a.u32(1)?,
                a.i32(2)?,
                [a.i32(3)?, a.i32(4)?, a.i32(5)?, a.i32(6)?],
                [a.i32(7)?, a.i32(8)?, a.i32(9)?, a.i32(10)?],
                a.bool(11)?,
            ),
            _ => return Err(format!("unknown resource call {name}")),
        })
    }
    fn execute(&self, r: &mut Resources) -> Result<(), String> {
        match self {
            Self::Retain(id) => {
                r.retain_sample(*id)?;
            }
            Self::WorldFallback(n) => {
                r.world_fallback(*n)?;
            }
            Self::Canopy(w, b) => {
                r.canopy_state(*w, b)?;
            }
            Self::UpdateCanopy(id, b) => r.update_canopy_state(*id, b)?,
            Self::Source(d) => {
                r.source(d)?;
            }
            Self::Desc(id, d) => r.source_desc(*id, d)?,
            Self::Pixels(id, b) => r.source_pixels(*id, b)?,
            Self::Png(id, b) => r.source_png(*id, b)?,
            Self::Blocks(id, b, m) => r.source_blocks(*id, b, m)?,
            Self::Release(id) => r.release_source(*id),
            Self::Sample(id, s) => {
                r.sample(*id, s)?;
            }
            Self::Sampling(id, s) => r.sampling(*id, s)?,
            Self::Dispose(id) => r.dispose_sample(*id)?,
            Self::Upload(id) => r.upload(*id)?,
            Self::Target(d) => {
                r.target(d)?;
            }
            Self::Resize(id, d) => r.resize_target(*id, d)?,
            Self::DisposeTarget(id) => r.dispose_target(*id),
            Self::Resolve(id) => r.resolve(*id)?,
            Self::Copy(a, b, m) => r.copy(*a, *b, *m)?,
            Self::Mipmap(id) => r.mipmap(*id)?,
            Self::Bind(id, f, l, v, s, c) => r.bind(
                *id, *f, *l, v[0], v[1], v[2], v[3], s[0], s[1], s[2], s[3], *c,
            )?,
        }
        Ok(())
    }
}
struct ArgumentBinding {
    argument: usize,
    source: usize,
    bank: String,
    offset: usize,
    len: usize,
}
fn bindings(args: &[Value], owners: &[Option<usize>]) -> Result<Vec<ArgumentBinding>, String> {
    args.iter()
        .enumerate()
        .filter_map(|(argument, a)| {
            a["source"].as_u64().map(|id| {
                Ok(ArgumentBinding {
                    argument,
                    source: owners
                        .get(id as usize)
                        .and_then(|r| *r)
                        .ok_or("argument source is not presentation")?,
                    bank: a["sourceBank"]
                        .as_str()
                        .ok_or("missing source bank")?
                        .into(),
                    offset: a["sourceOffset"].as_u64().ok_or("missing source offset")? as usize,
                    len: a["length"].as_u64().ok_or("missing source length")? as usize,
                })
            })
        })
        .collect()
}
enum Operation {
    External(ExternalCommand),
    BindNetwork(usize, usize, usize, f64, f64, f64),
    HeldRider(usize, usize, usize),
    BindSources(usize, usize, usize),
    PreparedView(
        usize,
        Option<bfe_presentation::unit_content::FirstPersonInput>,
        String,
    ),
    PreparedUnit(usize, Option<bfe_presentation::unit_content::Input>),
    Copy(String, usize, usize, String, usize, usize),
    Render(Command, Vec<ArgumentBinding>, Vec<(String, Vec<u8>)>),
    Resource(usize, ResourceCommand, Vec<ArgumentBinding>),
    Present(usize, PresentationCommand),
    Patch(usize, String, usize, Vec<u8>),
    Clear(Clear),
    Checkpoint,
}
/// A session uses the same phase owners as a live native battle. Resource and
/// owner ids are resolved at admission; no JSON or file IO reaches a frame.
fn context_key(meta: &Value) -> Result<String, String> {
    Ok(meta["context"]
        .as_u64()
        .ok_or("missing context")?
        .to_string())
}
pub struct Session {
    pub renderer: Box<Bridge>,
    pub canvas: crate::canvas::Canvas,
    resources: Vec<Resources>,
    presentation: Vec<PresentationOwner>,
    draws: [u64; bfe_render::core::PASSES],
    check_gl: bool,
    check_inputs: bool,
    frames: Vec<std::ops::Range<usize>>,
    verification: Vec<(usize, String, String)>,
    ops: Vec<Operation>,
    labels: Vec<String>,
    owners: Vec<Option<usize>>,
    render_owner: usize,
    pub size: [i32; 2],
    reference: Vec<u8>,
    pub content: bfe_content_native::BattleContent,
}
impl Session {
    pub fn load(dir: &Path, root: &Path, load: impl Fn() -> glow::Context) -> Result<Self, String> {
        let meta: Value = serde_json::from_slice(
            &std::fs::read(dir.join("session.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        crate::session_format::admit(&meta)?;
        if !meta["errors"]
            .as_array()
            .ok_or("missing capture coverage")?
            .is_empty()
        {
            return Err(format!("capture coverage failure: {}", meta["errors"]));
        }
        if meta["externalDraws"]
            .as_u64()
            .ok_or("missing draw coverage")?
            != 0
        {
            return Err("session contains draws outside the renderer".into());
        }
        {
            let expected = meta["apiHash"]
                .as_str()
                .ok_or("missing session ABI digest")?;
            let mut hash = Sha256::new();
            hash.update(include_bytes!(
                "../../../tools/native-session/renderer-api.json"
            ));
            hash.update(include_bytes!(
                "../../../tools/native-session/presentation-api.json"
            ));
            hash.update(serde_json::to_vec(&presentation_schema()).map_err(|e| e.to_string())?);
            if format!("{hash:x}", hash = hash.finalize()) != expected {
                return Err(
                    "session ABI differs; record with the current generated transport".into(),
                );
            }
        }
        let mut blobs = BlobStore::load(dir, &meta)?;
        let map = meta["map"].as_str().ok_or("missing session map")?;
        let mut loader = bfe_content_native::Loader::new(root);
        loader.compressed = meta["settings"]["textureCompression"]
            .as_bool()
            .unwrap_or(true);
        loader.texture_capabilities = meta["capabilities"][context_key(&meta)?]
            .as_u64()
            .unwrap_or(0) as u32;
        let mut content = loader.load(map, meta["query"].as_str().unwrap_or(""))?;
        let requests: Vec<crate::session_assets::TextureRequest> =
            serde_json::from_value(meta["textureRequests"].clone())
                .map_err(|e| format!("texture requests: {e}"))?;
        crate::session_assets::prepare(&mut content, &requests)?;
        blobs.assets = crate::session_assets::Assets::new(&content, &requests)?.data;
        for call in meta["calls"].as_array().ok_or("missing calls")? {
            if let Some(args) = call["args"].as_array() {
                for a in args {
                    if let Some(asset) = a["asset"].as_str() {
                        let data = blobs
                            .assets
                            .get(asset)
                            .ok_or_else(|| format!("missing native asset {asset}"))?;
                        if let Some(expected) = a["sha256"].as_str() {
                            if format!("{:x}", Sha256::digest(data)) != expected {
                                return Err(format!(
                                    "prepared asset {asset} differs from recorded content"
                                ));
                            }
                        }
                    }
                }
            }
        }

        eprintln!(
            "native content: {} world {} models {} actors {} textures",
            content.world.len(),
            content.models.len(),
            content.actors.len(),
            content.textures.images.len()
        );
        let context = meta["context"].as_u64().ok_or("missing battle context")?;
        let render_owner = meta["renderer"].as_u64().ok_or("missing battle renderer")? as usize;
        let rows = meta["owners"].as_array().ok_or("missing owner table")?;
        if rows.len() > 4096 {
            return Err("session has too many owners".into());
        }
        let mut gl = load();
        unsafe {
            eprintln!(
                "native GL: {} / {}",
                gl.get_parameter_string(glow::VERSION),
                gl.get_parameter_string(glow::RENDERER)
            );
        }
        if std::env::var_os("BFE_SESSION_GL_CHECK").is_some() {
            unsafe {
                gl.enable(glow::DEBUG_OUTPUT);
                gl.debug_message_callback(|_, kind, id, severity, message| {
                    if kind == glow::DEBUG_TYPE_ERROR {
                        eprintln!("GL {id} severity {severity:x}: {message}");
                    }
                });
            }
        }
        let size = [
            meta["size"][0].as_i64().ok_or("missing width")? as i32,
            meta["size"][1].as_i64().ok_or("missing height")? as i32,
        ];
        let mut result = Self {
            canvas: crate::canvas::Canvas::new(load(), size)?,
            renderer: Bridge::new_boxed(gl, crate::resources::Ranges::new(load())),
            resources: Vec::new(),
            presentation: Vec::new(),
            draws: [0; bfe_render::core::PASSES],
            check_gl: std::env::var_os("BFE_SESSION_GL_CHECK").is_some(),
            check_inputs: std::env::var_os("BFE_SESSION_VALIDATE").is_some(),
            frames: Vec::new(),
            verification: Vec::new(),
            ops: Vec::new(),
            labels: Vec::new(),
            owners: vec![None; rows.len()],
            render_owner,
            size: [
                meta["size"][0].as_i64().ok_or("missing width")? as i32,
                meta["size"][1].as_i64().ok_or("missing height")? as i32,
            ],
            reference: blobs
                .get(meta["reference"].as_u64().ok_or("missing reference")? as usize)?
                .to_vec(),
            content,
        };
        result.renderer.r.core.canvas_framebuffer = Some(result.canvas.framebuffer);
        let prepared_units = std::mem::take(&mut result.content.units);
        let mut unit_names = std::collections::BTreeMap::new();
        let mut resource_ids = vec![None; rows.len()];
        for row in meta["calls"].as_array().ok_or("missing calls")? {
            let kind = row["kind"].as_str().ok_or("missing operation kind")?;
            let owner = row["owner"].as_u64().unwrap_or(0) as usize;
            let previous = result.ops.len();
            match kind {
                "new" => {
                    let class = row["class"].as_str().ok_or("missing owner class")?;
                    if class == "bferenderer" {
                        continue;
                    }
                    if class == "gpuresources" {
                        if row["context"] == context {
                            let id = result.resources.len();
                            let mut resources = Resources::new(load());
                            resources.canvas = Some(result.canvas.framebuffer);
                            result.resources.push(resources);
                            *resource_ids.get_mut(owner).ok_or("owner exceeds table")? = Some(id);
                        }
                        continue;
                    }
                    let native_effects = if class == "pagepresentation" {
                        let captured: Value = serde_json::from_str(
                            row["args"][0].as_str().ok_or("missing effect descriptor")?,
                        )
                        .map_err(|e| e.to_string())?;
                        let mut difference = None;
                        let index = result
                            .content
                            .effects
                            .iter()
                            .position(|(name, e)| {
                                if let Some(path) =
                                    json_difference(&e.descriptor, &captured, "effects")
                                {
                                    difference = Some(format!(
                                        "native effect library {name} differs at {path}"
                                    ));
                                    false
                                } else {
                                    true
                                }
                            })
                            .ok_or_else(|| {
                                difference.unwrap_or("native effect content unavailable".into())
                            })?;
                        Some(result.content.effects[index].1.descriptor.to_string())
                    } else {
                        None
                    };
                    let ctor_text =
                        row["args"][0]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                if row["args"][0].is_number() {
                                    row["args"][0].to_string()
                                } else {
                                    "{}".into()
                                }
                            });
                    let p = PresentationOwner::new(
                        class,
                        native_effects.as_deref().unwrap_or(&ctor_text),
                        row["preview"].as_bool().unwrap_or(false),
                    )?;
                    let id = result.presentation.len();
                    result.presentation.push(p);
                    *result.owners.get_mut(owner).ok_or("owner exceeds table")? = Some(id);
                }
                "call" => {
                    let name = row["name"].as_str().ok_or("missing method")?;
                    let mut args = row["args"].as_array().ok_or("missing arguments")?.clone();
                    let op = if owner == render_owner {
                        for a in &mut args {
                            if let Some(id) = a["owner"].as_u64() {
                                let index = resource_ids
                                    .get(id as usize)
                                    .and_then(|r| *r)
                                    .ok_or("resource belongs to another context")?;
                                *a = serde_json::json!({"owner":index});
                            }
                        }
                        Operation::Render(
                            Command::decode(name, &args, &blobs)
                                .map_err(|e| format!("{name}: {e}"))?,
                            bindings(&args, &result.owners)?,
                            row["checks"]
                                .as_array()
                                .unwrap_or(&Vec::new())
                                .iter()
                                .map(|c| {
                                    Ok((
                                        c["bank"]
                                            .as_str()
                                            .ok_or("missing renderer check bank")?
                                            .into(),
                                        blobs
                                            .get(
                                                c["data"]["blob"]
                                                    .as_u64()
                                                    .ok_or("missing renderer check bytes")?
                                                    as usize,
                                            )?
                                            .to_vec(),
                                    ))
                                })
                                .collect::<Result<Vec<_>, String>>()?,
                        )
                    } else if let Some(id) = resource_ids.get(owner).and_then(|r| *r) {
                        Operation::Resource(
                            id,
                            ResourceCommand::decode(name, &args, &blobs)?,
                            bindings(&args, &result.owners)?,
                        )
                    } else if let Some(id) = result.owners.get(owner).and_then(|r| *r) {
                        if name == "bind_sources" {
                            let a=Arguments::new(&args,&blobs);
                            let vehicle=result.owners.get(a.owner(0)?).and_then(|x|*x).ok_or("unknown vehicle source")?;
                            let props=result.owners.get(a.owner(1)?).and_then(|x|*x).ok_or("unknown prop source")?;
                            Operation::BindSources(id,vehicle,props)
                        } else if name == "add_unit_rider" {
                            let a=Arguments::new(&args,&blobs);
                            let source=result.owners.get(a.owner(0)?).and_then(|x|*x).ok_or("unknown unit owner")?;
                            Operation::HeldRider(id,source,a.usize(1)?)
                        } else if name == "bind_network" {
                            let a = Arguments::new(&args, &blobs);
                            let source = a.owner(0)?;
                            Operation::BindNetwork(
                                id,
                                result
                                    .owners
                                    .get(source)
                                    .and_then(|x| *x)
                                    .ok_or("unknown network owner")?,
                                a.usize(1)?,
                                a.f64(2)?,
                                a.f64(3)?,
                                a.f64(4)?,
                            )
                        } else if name == "load_content"
                            && result.presentation[id].class() == "pageunits"
                        {
                            let captured: Value = serde_json::from_str(
                                args[0].as_str().ok_or("missing unit descriptor")?,
                            )
                            .map_err(|e| e.to_string())?;
                            let names = captured["bones"]
                                .as_array()
                                .ok_or("missing unit bones")?
                                .iter()
                                .map(|b| b["name"].as_str().unwrap_or(""))
                                .collect::<Vec<_>>();
                            let mut difference = None;
                            let at = prepared_units
                                .iter()
                                .position(|(asset, u)| {
                                    if !u
                                        .bones
                                        .iter()
                                        .map(|b| b.name.as_str())
                                        .eq(names.iter().copied())
                                        || u.clips.len()
                                            != captured["clips"].as_array().map_or(0, Vec::len)
                                    {
                                        return false;
                                    }
                                    let native = bfe_content_native::units::descriptor(u);
                                    let delta = json_difference(&native, &captured, "unit");
                                    if let Some(path) = delta {
                                        difference = Some(format!(
                                            "native unit content {asset} differs at {path}"
                                        ));
                                        false
                                    } else {
                                        true
                                    }
                                })
                                .ok_or_else(|| {
                                    difference.unwrap_or(
                                        "no native unit content matches the recorded skeleton"
                                            .into(),
                                    )
                                })?;
                            let (asset, unit) = &prepared_units[at];
                            unit_names.insert(id, asset.clone());
                            Operation::PreparedUnit(
                                id,
                                Some(bfe_content_native::units::copy_input(unit)),
                            )
                        } else if name == "add_view"
                            && result.presentation[id].class() == "pageunits"
                        {
                            let input = result
                                .content
                                .first_person
                                .as_ref()
                                .map(bfe_content_native::units::copy_first_person)
                                .ok_or("native first-person content unavailable")?;
                            let captured: Value = serde_json::from_str(
                                args[0].as_str().ok_or("missing first-person descriptor")?,
                            )
                            .map_err(|e| e.to_string())?;
                            if let Some(path) = json_difference(
                                &bfe_content_native::units::first_person_descriptor(&input),
                                &captured,
                                "firstPerson",
                            ) {
                                return Err(format!(
                                    "native first-person content differs at {path}"
                                ));
                            }
                            Operation::PreparedView(
                                id,
                                Some(input),
                                args[1]
                                    .as_str()
                                    .ok_or("missing first-person attachments")?
                                    .into(),
                            )
                        } else {
                            Operation::Present(
                                id,
                                result.presentation[id].decode(name, &args, &blobs)?,
                            )
                        }
                    } else {
                        continue;
                    };
                    result.ops.push(op);
                }
                "patch" => {
                    if owner != render_owner && result.owners.get(owner).and_then(|r| *r).is_none()
                    {
                        continue;
                    }
                    result.ops.push(Operation::Patch(
                        owner,
                        row["bank"].as_str().ok_or("missing bank")?.into(),
                        row["offset"].as_u64().ok_or("missing patch offset")? as usize,
                        blobs
                            .get(
                                row["data"]["blob"].as_u64().ok_or("missing patch bytes")? as usize
                            )?
                            .to_vec(),
                    ));
                }
                "copy" => {
                    if owner != render_owner {
                        continue;
                    }
                    let source = row["source"].as_u64().ok_or("missing copy source")? as usize;
                    result.ops.push(Operation::Copy(
                        row["bank"].as_str().ok_or("missing copy bank")?.into(),
                        row["offset"].as_u64().ok_or("missing copy offset")? as usize,
                        result
                            .owners
                            .get(source)
                            .and_then(|r| *r)
                            .ok_or("copy source is not presentation")?,
                        row["sourceBank"]
                            .as_str()
                            .ok_or("missing source bank")?
                            .into(),
                        row["sourceOffset"]
                            .as_u64()
                            .ok_or("missing source offset")? as usize,
                        row["length"].as_u64().ok_or("missing copy length")? as usize,
                    ));
                }
                "gl" => {
                    if row["context"] == context {
                        result.ops.push(Operation::External(ExternalCommand::decode(
                            row["name"].as_str().ok_or("missing GL method")?,
                            row["args"].as_array().ok_or("missing GL arguments")?,
                            &blobs,
                            &resource_ids,
                        )?));
                    }
                }
                "clear" => {
                    if row["context"] == context {
                        result.ops.push(Operation::Clear(
                            serde_json::from_value(row.clone()).map_err(|e| e.to_string())?,
                        ));
                    }
                }
                "frame" => {
                    let end = result.ops.len();
                    if let Some(last) = result.frames.last_mut() {
                        last.end = end;
                    }
                    result.frames.push(end..end);
                }
                "checkpoint" => result.ops.push(Operation::Checkpoint),
                _ => return Err(format!("unknown session operation {kind}")),
            }
            if result.ops.len() > previous {
                result
                    .labels
                    .push(row["name"].as_str().unwrap_or(kind).into());
            }
        }
        if let Some(last) = result.frames.last_mut() {
            last.end = result.ops.len();
        }
        for v in meta["verification"].as_array().unwrap_or(&Vec::new()) {
            result.verification.push((
                result
                    .owners
                    .get(v["owner"].as_u64().ok_or("invalid verification owner")? as usize)
                    .and_then(|r| *r)
                    .ok_or("verification owner is not presentation")?,
                v["bank"]
                    .as_str()
                    .ok_or("missing verification bank")?
                    .into(),
                v["sha256"]
                    .as_str()
                    .ok_or("missing verification digest")?
                    .into(),
            ));
        }
        if !(1..=8192).contains(&result.size[0])
            || !(1..=8192).contains(&result.size[1])
            || result.reference.len() != result.size[0] as usize * result.size[1] as usize * 4
        {
            return Err("invalid reference extent".into());
        }
        Ok(result)
    }
    pub fn world(&self) -> RecordedWorld {
        let mut ranges = Vec::with_capacity(self.frames.len() + 1);
        let first = self.frames.first().map_or(self.ops.len(), |r| r.start);
        ranges.push(0..first);
        ranges.extend(self.frames.iter().cloned());
        RecordedWorld::new(ranges)
    }
    pub fn play(&mut self) -> Result<(), String> {
        self.play_range(0..self.ops.len())
    }
    pub fn play_range(&mut self, range: std::ops::Range<usize>) -> Result<(), String> {
        for (index, op) in self.ops[range.clone()].iter_mut().enumerate() {
            let outcome = match op {
                Operation::BindSources(id,vehicles,props)=>{
                    let (units,vehicles,props)=three_owners(&mut self.presentation,*id,*vehicles,*props)?;
                    let (PresentationOwner::Units(units),PresentationOwner::Vehicles(vehicles),PresentationOwner::Props(props))=(units,vehicles,props)
                        else{return Err("invalid rider source owners".into())};
                    units.bind_sources(vehicles,props);Ok(())
                }
                Operation::HeldRider(id,source,actor)=>{
                    if id==source {return Err("held source cannot be its own unit owner".into());}
                    let (target,source)=if *id<*source {let(a,b)=self.presentation.split_at_mut(*source);(&mut a[*id],&b[0])}
                        else{let(a,b)=self.presentation.split_at_mut(*id);(&mut b[0],&a[*source])};
                    let (PresentationOwner::Props(props),PresentationOwner::Units(units))=(target,source) else{return Err("invalid held source owners".into())};
                    props.add_unit_rider(units,*actor);Ok(())
                }
                Operation::BindNetwork(id, source, slot, entity, generation, kind) => {
                    if id == source {
                        return Err("network cannot bind itself".into());
                    }
                    let (target, network) = if *id < *source {
                        let (a, b) = self.presentation.split_at_mut(*source);
                        (&mut a[*id], &b[0])
                    } else {
                        let (a, b) = self.presentation.split_at_mut(*id);
                        (&mut b[0], &a[*source])
                    };
                    let PresentationOwner::Network(network) = network else {
                        return Err("binding source is not a network".into());
                    };
                    match target {
                        PresentationOwner::Units(p) => {
                            p.bind_network(network, *slot, *entity, *generation, *kind)
                        }
                        PresentationOwner::Vehicles(p) => {
                            p.bind_network(network, *slot, *entity, *generation, *kind)
                        }
                        _ => return Err("unsupported network binding target".into()),
                    };
                    Ok(())
                }
                Operation::External(c) => c.execute(
                    &self.renderer.r.core.gl,
                    &self.resources,
                    self.canvas.framebuffer,
                ),
                Operation::PreparedView(id, input, attachments) => {
                    if let PresentationOwner::Units(p) = &mut self.presentation[*id] {
                        p.view_prepared(
                            input
                                .take()
                                .ok_or("first-person content already admitted")?,
                            attachments,
                        )?;
                        Ok(())
                    } else {
                        Err("first-person content requires a unit owner".into())
                    }
                }
                Operation::PreparedUnit(id, unit) => {
                    if let PresentationOwner::Units(p) = &mut self.presentation[*id] {
                        p.load_prepared(unit.take().ok_or("prepared content already admitted")?);
                        Ok(())
                    } else {
                        Err("native unit content requires a unit owner".into())
                    }
                }
                Operation::Render(c, bindings, checks) => {
                    for b in bindings {
                        let bank = self.presentation[b.source].read_bank(&b.bank)?;
                        let source = bank
                            .get(
                                b.offset
                                    ..b.offset
                                        .checked_add(b.len)
                                        .ok_or("argument range overflow")?,
                            )
                            .ok_or("argument source exceeds bank")?;
                        c.patch_argument(b.argument, source)?;
                    }
                    if self.check_inputs {
                        for (bank, expected) in checks {
                            let actual = self.renderer.bank(bank)?;
                            if let Some(at) =
                                actual.iter().zip(expected.iter()).position(|(a, b)| a != b)
                            {
                                let word = at / 8;
                                let value = |data: &[u8]| {
                                    data.get(word * 8..word * 8 + 8)
                                        .map(|w| f64::from_le_bytes(w.try_into().unwrap()))
                                };
                                return Err(format!("renderer submission {} {} word {word} differs: native {:?}, browser {:?}",index+range.start,bank,value(actual),value(expected)));
                            }
                        }
                    }
                    if let Some(stats) = c.execute(&mut self.renderer, &mut self.resources)? {
                        for (total, count) in self.draws.iter_mut().zip(stats.pass_draws) {
                            *total += count as u64;
                        }
                    }
                    Ok(())
                }
                Operation::Resource(id, c, bindings) => {
                    for b in bindings {
                        let bank = self.presentation[b.source].read_bank(&b.bank)?;
                        let source = bank
                            .get(
                                b.offset
                                    ..b.offset
                                        .checked_add(b.len)
                                        .ok_or("argument range overflow")?,
                            )
                            .ok_or("argument source exceeds bank")?;
                        match c {
                            ResourceCommand::Canopy(_, data)
                            | ResourceCommand::UpdateCanopy(_, data)
                            | ResourceCommand::Pixels(_, data)
                            | ResourceCommand::Blocks(_, data, _)
                                if b.argument == 1 && data.len() == source.len() =>
                            {
                                data.copy_from_slice(source)
                            }
                            _ => return Err("unsupported resource presentation binding".into()),
                        }
                    }
                    c.execute(&mut self.resources[*id])
                }
                Operation::Present(id, c) => c.execute(&mut self.presentation[*id]),
                Operation::Patch(owner, name, offset, data) => {
                    let mut bank = if *owner == self.render_owner {
                        BankMut::Slice(self.renderer.bank(name)?)
                    } else {
                        self.presentation
                            [self.owners[*owner].ok_or("missing presentation owner")?]
                        .bank(name)?
                    };
                    let dest = bank
                        .get_mut(*offset..offset.checked_add(data.len()).ok_or("patch overflow")?)
                        .ok_or_else(|| format!("patch {name} exceeds native bank"))?;
                    dest.copy_from_slice(data);
                    Ok(())
                }
                Operation::Copy(bank, offset, source, source_bank, source_offset, len) => {
                    let source_data = self.presentation[*source].read_bank(source_bank)?;
                    let data = source_data
                        .get(
                            *source_offset
                                ..source_offset.checked_add(*len).ok_or("copy overflow")?,
                        )
                        .ok_or("copy exceeds source bank")?;
                    let dest = self
                        .renderer
                        .bank(bank)?
                        .get_mut(*offset..offset.checked_add(*len).ok_or("copy overflow")?)
                        .ok_or("copy exceeds destination bank")?;
                    dest.copy_from_slice(data);
                    Ok(())
                }
                Operation::Clear(c) => {
                    let gl = &self.renderer.r.core.gl;
                    unsafe {
                        gl.clear_color(c.color[0], c.color[1], c.color[2], c.color[3]);
                        gl.clear_depth_f32(c.depth);
                        gl.clear_stencil(c.stencil);
                        gl.color_mask(
                            c.color_mask[0],
                            c.color_mask[1],
                            c.color_mask[2],
                            c.color_mask[3],
                        );
                        gl.depth_mask(c.depth_mask);
                        gl.scissor(c.rect[0], c.rect[1], c.rect[2], c.rect[3]);
                        if c.scissor {
                            gl.enable(glow::SCISSOR_TEST);
                        } else {
                            gl.disable(glow::SCISSOR_TEST);
                        }
                        gl.clear(c.mask);
                    }
                    Ok(())
                }
                Operation::Checkpoint => Ok(()),
            };
            outcome.map_err(|e| {
                format!(
                    "session operation {} {}: {e}",
                    index + range.start,
                    self.labels[index + range.start]
                )
            })?;
            if self.check_gl {
                let error = unsafe { self.renderer.r.core.gl.get_error() };
                if error != glow::NO_ERROR {
                    return Err(format!(
                        "session operation {} {}: GL error {error:x}",
                        index + range.start,
                        self.labels[index + range.start]
                    ));
                }
            }
        }
        Ok(())
    }
    /// One cold readback yields precise differences; reference bytes never
    /// become a texture or a render input. Coordinates use GL's lower origin.
    pub fn compare(&self) -> Result<Value, String> {
        let gl = &self.renderer.r.core.gl;
        let mut pixels = vec![0u8; self.reference.len()];
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvas.framebuffer));
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                0,
                0,
                self.size[0],
                self.size[1],
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
        }
        let mut changed = 0;
        let mut channels = 0;
        let mut max = 0;
        let mut squared = 0u64;
        let mut first = None;
        let mut thresholds = [0u64; 4];
        let mut tiles = vec![[0u64; 3]; 16 * 9];
        let mut largest = None;
        for (i, (a, b)) in pixels
            .chunks_exact(4)
            .zip(self.reference.chunks_exact(4))
            .enumerate()
        {
            if a != b {
                changed += 1;
                if first.is_none() {
                    first = Some(
                        serde_json::json!({"x":i%self.size[0] as usize,"y":i/self.size[0] as usize,"native":a,"browser":b}),
                    );
                }
            }
            for (&a, &b) in a.iter().zip(b) {
                let d = a.abs_diff(b);
                if d > 0 {
                    channels += 1;
                }
                max = max.max(d);
                squared += d as u64 * d as u64;
            }
            let d = a
                .iter()
                .zip(b)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            for (n, t) in [1, 8, 32, 128].iter().enumerate() {
                if d > *t {
                    thresholds[n] += 1;
                }
            }
            let tile = (i / self.size[0] as usize * 9 / self.size[1] as usize) * 16
                + (i % self.size[0] as usize * 16 / self.size[0] as usize);
            if d > 0 {
                tiles[tile][0] += 1;
            }
            if d > 8 {
                tiles[tile][1] += 1;
            }
            tiles[tile][2] = tiles[tile][2].max(d as u64);
            if d == max && d > 0 {
                largest = Some(
                    serde_json::json!({"x":i%self.size[0] as usize,"y":i/self.size[0] as usize,"native":a,"browser":b}),
                );
            }
        }
        let mut phase_differences = Vec::new();
        for (id, bank, expected) in &self.verification {
            let actual = format!(
                "{:x}",
                Sha256::digest(self.presentation[*id].read_bank(bank)?)
            );
            if &actual != expected {
                phase_differences.push(serde_json::json!({"owner":id,"class":self.presentation[*id].class(),"bank":bank,"native":actual,"browser":expected}));
            }
        }
        Ok(
            serde_json::json!({"native":format!("{:x}",Sha256::digest(&pixels)),"browser":format!("{:x}",Sha256::digest(&self.reference)),"pixels":pixels.len()/4,"changed":changed,"channels":channels,"max":max,"over1_8_32_128":thresholds,"tiles16x9":tiles,"largest":largest,"rms":(squared as f64/pixels.len() as f64).sqrt(),"first":first,"operations":self.ops.len(),"passDraws":self.draws,"presentationOwners":self.presentation.len(),"phaseDifferences":phase_differences,"phaseChecks":self.verification.len()}),
        )
    }
}

fn json_difference(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) if a.as_f64() == b.as_f64() => None,
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b)
            .enumerate()
            .find_map(|(i, (a, b))| json_difference(a, b, &format!("{path}[{i}]"))),
        (Value::Object(a), Value::Object(b)) if a.len() == b.len() => a
            .iter()
            .find_map(|(key, a)| json_difference(a, &b[key], &format!("{path}.{key}"))),
        _ if a == b => None,
        _ => {
            let describe = |v: &Value| match v {
                Value::Array(a) => format!("array of {}", a.len()),
                Value::Object(a) => format!("object of {}", a.len()),
                _ => v.to_string(),
            };
            Some(format!(
                "{path}: native {}, browser {}",
                describe(a),
                describe(b)
            ))
        }
    }
}

/// Recorded authority/presentation batches share the host clock boundary with
/// a future live world. They retain the snapshot timestamps inside each phase
/// input; display wall time never changes interpolation or RNG decisions.
pub struct RecordedWorld {
    ranges: Vec<std::ops::Range<usize>>,
    at: usize,
    started: bool,
}
impl RecordedWorld {
    fn new(ranges: Vec<std::ops::Range<usize>>) -> Self {
        Self {
            ranges,
            at: 0,
            started: false,
        }
    }
    pub fn len(&self) -> usize {
        self.ranges.len()
    }
}
impl crate::host::World for RecordedWorld {
    type Frame = std::ops::Range<usize>;
    fn step(
        &mut self,
        _: &crate::generated_input::ClientCommand,
        _: &crate::generated_input::ClientLook,
        _: &crate::generated_input::ClientFlightCommand,
        _: &crate::input::ViewActions,
        _: f64,
    ) -> Result<(), String> {
        if self.started {
            self.at = (self.at + 1).min(self.ranges.len().saturating_sub(1));
        }
        self.started = true;
        Ok(())
    }
    fn present(&mut self, _: f64) -> &Self::Frame {
        &self.ranges[self.at]
    }
}

/// The host's viewport/scissor/reset operations surround the Rust phases.
/// Capturing these scalar calls is essential: they affect a draw even though
/// they contain no world data and are absent from the WASM render API.
enum ExternalCommand {
    Enable(u32, bool),
    Viewport([i32; 4]),
    Scissor([i32; 4]),
    Color([bool; 4]),
    Depth(bool),
    DepthFunc(u32),
    Stencil(u32),
    Polygon(f32, f32),
    Line(f32),
    Equation(u32, u32),
    Blend(u32, u32, u32, u32),
    Front(u32),
    Cull(u32),
    Framebuffer(u32, Option<(usize, u32, bool, usize)>),
}
impl ExternalCommand {
    fn decode(
        name: &str,
        args: &[Value],
        b: &BlobStore,
        resources: &[Option<usize>],
    ) -> Result<Self, String> {
        let a = Arguments::new(args, b);
        Ok(match name {
            "enable" | "disable" => Self::Enable(a.u32(0)?, name == "enable"),
            "viewport" => Self::Viewport([a.i32(0)?, a.i32(1)?, a.i32(2)?, a.i32(3)?]),
            "scissor" => Self::Scissor([a.i32(0)?, a.i32(1)?, a.i32(2)?, a.i32(3)?]),
            "colorMask" => Self::Color([a.bool(0)?, a.bool(1)?, a.bool(2)?, a.bool(3)?]),
            "depthMask" => Self::Depth(a.bool(0)?),
            "depthFunc" => Self::DepthFunc(a.u32(0)?),
            "stencilMask" => Self::Stencil(a.u32(0)?),
            "polygonOffset" => Self::Polygon(a.f32(0)?, a.f32(1)?),
            "lineWidth" => Self::Line(a.f32(0)?),
            "blendEquation" => Self::Equation(a.u32(0)?, a.u32(0)?),
            "blendEquationSeparate" => Self::Equation(a.u32(0)?, a.u32(1)?),
            "blendFunc" => Self::Blend(a.u32(0)?, a.u32(1)?, a.u32(0)?, a.u32(1)?),
            "blendFuncSeparate" => Self::Blend(a.u32(0)?, a.u32(1)?, a.u32(2)?, a.u32(3)?),
            "frontFace" => Self::Front(a.u32(0)?),
            "cullFace" => Self::Cull(a.u32(0)?),
            "bindFramebuffer" => Self::Framebuffer(
                a.u32(0)?,
                if args[1].is_null() {
                    None
                } else {
                    let v = &args[1];
                    Some((
                        resources
                            .get(v["owner"].as_u64().ok_or("missing framebuffer owner")? as usize)
                            .and_then(|r| *r)
                            .ok_or("framebuffer context mismatch")?,
                        v["target"].as_u64().ok_or("missing framebuffer target")? as u32,
                        v["multisample"].as_bool().unwrap_or(false),
                        v["face"].as_u64().ok_or("missing framebuffer face")? as usize,
                    ))
                },
            ),
            _ => return Err(format!("unknown external GL state {name}")),
        })
    }
    fn execute(
        &self,
        gl: &glow::Context,
        resources: &[Resources],
        canvas: glow::Framebuffer,
    ) -> Result<(), String> {
        unsafe {
            match self {
                Self::Enable(cap, on) => {
                    if *on {
                        gl.enable(*cap)
                    } else {
                        gl.disable(*cap)
                    }
                }
                Self::Viewport(v) => gl.viewport(v[0], v[1], v[2], v[3]),
                Self::Scissor(v) => gl.scissor(v[0], v[1], v[2], v[3]),
                Self::Color(v) => gl.color_mask(v[0], v[1], v[2], v[3]),
                Self::Depth(on) => gl.depth_mask(*on),
                Self::DepthFunc(f) => gl.depth_func(*f),
                Self::Stencil(m) => gl.stencil_mask(*m),
                Self::Polygon(a, b) => gl.polygon_offset(*a, *b),
                Self::Line(w) => gl.line_width(*w),
                Self::Equation(a, b) => gl.blend_equation_separate(*a, *b),
                Self::Blend(a, b, c, d) => gl.blend_func_separate(*a, *b, *c, *d),
                Self::Front(f) => gl.front_face(*f),
                Self::Cull(f) => gl.cull_face(*f),
                Self::Framebuffer(target, fb) => gl.bind_framebuffer(
                    *target,
                    fb.map(|(owner, id, multi, face)| {
                        resources[owner].framebuffer(id, multi, face)
                    })
                    .transpose()?
                    .or(Some(canvas)),
                ),
            }
        }
        Ok(())
    }
}
