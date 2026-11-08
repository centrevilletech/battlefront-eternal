#!/usr/bin/env python3
"""Generate the native transport from the renderer's existing Rust API.

Only transport is emitted. The scene, material and draw implementations stay
in bfe-render; a native host never maintains a second version of a pass.
"""
from pathlib import Path
import json
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
WEB = ROOT / 'rust/bfe-render/src/web'
OUT = ROOT / 'rust/bfe-client/src/render_bridge.rs'
META = ROOT / 'tools/native-session/renderer-api.json'


def functions(text):
    # Restrict parsing to the ordinary scalar/slice binding declarations. The
    # renderer's generated recovery journal uses this same boundary contract.
    for match in re.finditer(r'pub fn (\w+)\((.*?)\)\s*(?:->\s*([^\{]+))?\{', text, re.S):
        if not re.search(r"&(?:mut )?self",match[2]): continue
        start = match.end()
        depth = 1
        pos = start
        while depth:
            if text[pos] == '{': depth += 1
            if text[pos] == '}': depth -= 1
            pos += 1
        args = []
        for arg in match[2].split(','):
            arg = arg.strip()
            if not arg or arg in ['&self', '&mut self']: continue
            name, kind = arg.split(':', 1)
            args.append((name.strip(), kind.strip()))
        yield match[1], args, (match[3] or '').strip(), text[start:pos-1]


methods = []
per_pass = []
static_modules = []

# Field type chain for nested renderer banks: the Renderer field's type, then
# each intermediate field's type, ending at the file declaring the leaf.
def struct_file(type_name):
    found = [f for f in sorted((ROOT / 'rust/bfe-render/src').rglob('*.rs'))
             if re.search(r'pub struct ' + type_name + r'\b', f.read_text())]
    if len(found) != 1: raise RuntimeError(f'Cannot resolve renderer type {type_name}')
    return found[0]
def field_type(text, field):
    declared = re.search(r'pub\s+' + field + r':\s*(?:crate::)?([\w:]+)', text)
    if not declared: raise RuntimeError(f'Cannot resolve renderer field {field}')
    return declared[1].split('::')[-1]
def nested_owner(path):
    text = (ROOT / 'rust/bfe-render/src/renderer.rs').read_text()
    for hop in path[:-1]:
        owner_file = struct_file(field_type(text, hop))
        text = owner_file.read_text()
    return owner_file, path[-1]

banks = []
imports = set()
for file in sorted(WEB.glob('*.rs')):
    if file.stem in ['recovery', 'images']: continue
    text = file.read_text()
    text = re.sub(r'\s*self\.remember_\w+\([^\n]*\); // recovery\s*', '\n', text)
    # Local pass constants remain authoritative; the native shim only borrows
    # them. Do not bring browser context types or recovery state into the host.
    for imp in re.findall(r'^use crate::[^;]+;', text, re.M):
        if file.stem not in ['static_world','static_depth']: imports.add(imp.replace('crate::', 'bfe_render::'))
    for name, args, result, body in functions(text):
        if name.endswith('_ptr'):
            field = re.search(r'self\.((?:r\.|hud_model\.|line_resources\.|actor_resources\.)[\w.]+)\.as_mut_ptr\(\)', body)
            if field and not args and name != 'transparent_segments_ptr':
                path = field[1].split('.')[1:] if field[1].startswith('r.') else field[1].split('.')
                if path[0]=='actor_resources':
                    owner_file=ROOT/('rust/bfe-render/src/actor_crowd.rs' if path[1]=='crowd' else 'rust/bfe-render/src/actor_depth.rs');member=path[-1]
                elif len(path) == 2:
                    module, member = path
                    module = {'interface':'interface_scene'}.get(module,module)
                    owner_file=ROOT / f'rust/bfe-render/src/{module}.rs'
                    if not owner_file.exists():owner_file=ROOT / f'rust/bfe-render/src/{module}/mod.rs'
                    # A pass whose field name is not its file name (frames) is
                    # resolved through its declared type like a nested bank.
                    if not owner_file.exists(): owner_file, member = nested_owner(path)
                else:
                    # A nested bank (self.r.frames.plan.facts) follows each
                    # field's declared type to the struct that owns the leaf,
                    # since leaf names such as `state` repeat across passes.
                    owner_file, member = nested_owner(path)
                owner = owner_file.read_text()
                declared = re.search(r'pub\s+' + member + r':\s*(?:Vec<|\[)([a-z0-9]+)', owner)
                if not declared: raise RuntimeError(f'Cannot resolve renderer bank {field[1]}')
                scalar = declared[1]
                key = name[:-4]
                # The storage type is declared on each pass, so fail compilation
                # if a future field changes without updating this transport map.
                banks.append((key, field[1], scalar))
            elif name != 'transparent_segments_ptr' and not args:
                raise RuntimeError(f'Unmapped renderer bank {name}')
            continue
        if name.endswith('_len') or name in ['new', 'rebuild', 'replay_program_draw']: continue
        if any('web_sys' in kind for _, kind in args): continue
        body = body.replace('crate::', 'bfe_render::')
        # Resource constructors share native topology and pixel math with the page.
        body = body.replace('pixel_math::Page', 'pixel_math::V8')
        body = body.replace('.unchecked_into()', '')
        body = body.replace('self.remember_texture();', '')
        body = body.replace('unsafe { self.r.core.gl.register_external_texture(texture) }', 'texture')
        body = body.replace('self.r.core.gl.unregister_external_texture(old);', '')
        body = re.sub(r'JsValue::from_str\(&e\)', 'e.to_string()', body)
        body = re.sub(r'JsValue::from_str\(&e\.to_string\(\)\)', 'e.to_string()', body)
        body = body.replace('JsValue::from_str','String::from').replace('bfe_render::core::bytes','crate::session::bytes').replace('self.r.core.textures.len()','self.r.core.native_texture_count()')
        result = result.replace('JsValue', 'String')
        # Stats::values constructs a Vec only for JavaScript. Native frame
        # commands retain Stats directly, keeping phase submission allocation-free.
        if result == 'Result<Vec<u32>, String>':
            result = 'Result<bfe_render::core::Stats, String>'
            body = body.replace('?.values()', '?')
        # Recordings submit sealed packets. Standalone interface batches belong
        # to browser menus, rather than the native battle command stream.
        if 'bfe_render::core::Stats' in result or name == 'interface_frame':
            per_pass.append(name)
            continue
        methods.append((name, args, result, body.strip()))

lines = ['// Generated by tools/native-render-bridge.py; do not edit.',
    '// Scalar transport shared with the browser API; all draw bodies remain in bfe-render.',
    '#![allow(clippy::all, unused_imports, unused_variables, non_camel_case_types)]',
    'use glow::HasContext;', 'use crate::resources::Ranges;',
    'use crate::session::{Arguments, BlobStore};',
    'use crate::resources::Resources;', *sorted(imports),
    'pub struct Bridge {', 'pub r: Renderer<glow::Context, Ranges>,',
    'pub hud_model: bfe_render::hud_model::HudModel,', 'actor_resources: bfe_render::actor_resources::Bank,', 'line_resources: bfe_render::line_resources::Lines,', 'water_sources: Vec<bfe_render::water_resources::WaterSource>, water_geometry: std::collections::HashMap<(u32,u32),u32>,', 'static_sources: Vec<static_world::StaticSource>, static_worlds: Vec<static_world::StaticSlot>, static_recipes: Vec<StaticRecipe>,', '}',
    'impl Bridge {',
    'pub fn new(gl: glow::Context, multi: Ranges) -> Self { Self { r: Renderer::new(gl,multi), hud_model: bfe_render::hud_model::HudModel::new(),actor_resources:bfe_render::actor_resources::Bank::default(), line_resources:bfe_render::line_resources::Lines::default(),water_sources:vec![],water_geometry:std::collections::HashMap::new(), static_sources:vec![],static_worlds:vec![],static_recipes:vec![] } }']
for name, args, result, body in methods:
    params = ', '.join(f'{name}: {kind.replace("&crate::resources::GpuResources", "&Resources")}' for name, kind in args)
    params = params.replace('&crate::resources::GpuResources', '&Resources')
    if name.startswith('static_'): continue
    lines.append(f'pub fn {name}(&mut self{", " if params else ""}{params}) {"-> " + result if result else ""} {{ {body} }}')
lines += ['pub fn bank(&mut self, name: &str) -> Result<&mut [u8], String> {', 'match name {']
for name, field, scalar in banks:
    lines.append(f'"{name}" => Ok(crate::session::bytes_mut::<{scalar}>(&mut self.{field})),')
lines += ['_ => { if let Some(index)=name.strip_prefix(\"interface_stream_\").and_then(|s|s.parse::<usize>().ok()) { return self.r.interface.streams.get_mut(index).map(|s|s.data.as_mut_slice()).ok_or(\"unknown interface stream\".into()); } Err(format!(\"unknown renderer bank {name}\")) },', '}', '}', '}']
lines += ['pub enum Command {']
for name, args, _, _ in methods:
    kinds = [('usize' if 'GpuResources' in t else 'String' if t == '&str' else 'Vec<' + t[2:-1] + '>' if t.startswith('&[') else t) for _, t in args]
    lines.append(name + ('(' + ','.join(kinds) + ')' if kinds else '') + ',')
lines += ['}', 'impl Command {', 'pub fn decode(name: &str, args: &[serde_json::Value], blobs: &BlobStore) -> Result<Self,String> {',
    'let a=Arguments::new(args,blobs);', 'Ok(match name {']
for name, args, _, _ in methods:
    decoded = []
    for index, (_, kind) in enumerate(args):
        accessor = 'owner' if 'GpuResources' in kind else 'string' if kind == '&str' else kind[2:-1]+'s' if kind.startswith('&[') else kind[4:-1]+'s' if kind.startswith('Vec<') else kind
        decoded.append(f'a.{accessor}({index})?')
    lines.append(f'"{name}" => Self::{name}' + ('(' + ','.join(decoded) + ')' if args else '') + ',')
pass_names=per_pass
lines += ['_ => return Err(format!("unknown renderer command {name}")),', '})', '}',
    'pub fn is_per_pass(name:&str)->bool {matches!(name,'+'|'.join(json.dumps(name) for name in pass_names)+')}',
    'pub fn is_known(name:&str)->bool {matches!(name,'+'|'.join(json.dumps(name) for name,_,_,_ in methods)+')}',
    'pub fn execute(&mut self, host: &mut Bridge, resource_bank: &mut [Resources]) -> Result<Option<bfe_render::core::Stats>,String> {', 'match self {']
for name, args, result, _ in methods:
    names = [n for n, _ in args]
    passed = []
    for n, kind in args:
        if 'GpuResources' in kind: passed.append(f'resource_bank.{"get_mut" if "&mut" in kind else "get"}(*{n}).ok_or("unknown resource owner")?')
        elif kind.startswith('&'): passed.append(n)
        elif kind.startswith('Vec<'): passed.append(f'std::mem::take({n})')
        else: passed.append('*'+n)
    call = f'host.{name}(' + ','.join(passed) + ')'
    if result.startswith('Result<'): call += '?'
    action=f'return Ok(Some({call}));' if 'bfe_render::core::Stats' in result else f'let _={call};'
    if name in ['battle_frame','frame_aux_frame']:
        # Stats stay in the renderer's generated role rows. Summarize them
        # without constructing the browser-only Vec result on each frame.
        stats=(ROOT/'rust/bfe-render/src/core.rs').read_text().split('pub struct Stats {',1)[1].split('}',1)[0]
        fields=re.findall(r'pub (\w+): u32',stats)
        update=''.join(f'stats.{field}=stats.{field}.saturating_add(values[bfe_render::frame_plan::layout::stats::{field.upper()}] as u32);' for field in fields)
        rows='host.r.frames.stats.chunks_exact(bfe_render::frame_plan::layout::stats::LEN)' + ('.skip(*role as usize).take(1)' if name=='frame_aux_frame' else '')
        action=f'if !{call} {{ return Err(host.frame_error_frame().unwrap_or("battle frame failed".into())); }} let mut stats=bfe_render::core::Stats::default(); for values in {rows} {{{update} for (out,value) in stats.pass_draws.iter_mut().zip(&values[bfe_render::frame_plan::layout::stats::PASS_DRAWS..]){{*out=out.saturating_add(*value as u32);}}}} return Ok(Some(stats));'
    lines.append(f'Self::{name}' + ('('+','.join(names)+')' if args else '') + f' => {{ {action} }},')
lines += ['}', 'Ok(None)', '}', 'pub fn patch_argument(&mut self,index:usize,source_bytes:&[u8])->Result<(),String>{match (self,index){']
for name,args,_,_ in methods:
    for i,(n,kind) in enumerate(args):
        if not kind.startswith('&['): continue
        names=[n if j==i else '_' for j,(n,_) in enumerate(args)]
        lines.append(f'(Self::{name}('+','.join(names)+f'),{i})=>{{let dest=crate::session::bytes_mut({n});if dest.len()!=source_bytes.len(){{return Err(\"presentation argument extent differs\".into());}}dest.copy_from_slice(source_bytes);Ok(())}},')
lines += ['_=>Err(\"argument is not a scalar buffer\".into()),}}','}']
lines.append('struct StaticRecipe {source:u32,far_view:bool,low_res_scale:f64,samplers:Vec<i32>,order:u32,slot:u32,object:u32}')
for module in ['static_world','static_depth']:
    source=(WEB/(module+'.rs')).read_text().replace('use super::BfeRenderer;','use super::Bridge as BfeRenderer;').replace('use wasm_bindgen::prelude::*;','').replace('#[wasm_bindgen]','').replace('crate::','bfe_render::')
    source=re.sub(r'\s*self\.remember_\w+\([^\n]*\); // recovery\s*','\n',source)
    source=source.replace('JsValue::from_str','String::from').replace('JsValue','String')
    if module=='static_world':
        # Browser loss resumes a previously journaled call. Native contexts
        # admit directly; both wrappers use the same descriptor compiler below.
        body=next(body for name,_,_,body in functions(source) if name=='static_world')
        source=source.replace(body,"""
        let resource=self.compile_static_world(source,far_view,low_res_scale,samplers,features,order,slot,object,None)?;
        self.static_worlds.push(resource);
        Ok(resource.item)
"""+"    ",1)
        source=source.replace('self.static_worlds.push(resource);','self.static_recipes.push(super::StaticRecipe{source,far_view,low_res_scale,samplers:samplers.to_vec(),order,slot,object}); self.static_worlds.push(resource);')
        start=source.index('        let saved=std::mem::take(&mut self.recovery);')
        end=source.index('        result',start)+len('        result')
        source=source[:start]+"""        let recipes=std::mem::take(&mut self.static_recipes);
        let result=(||{for (i,r) in recipes.iter().enumerate(){let old=self.static_worlds[i];self.compile_static_world(r.source,r.far_view,r.low_res_scale,&r.samplers,if old.far{far}else{near},r.order,r.slot,r.object,Some(old))?;}Ok(())})();
        self.static_recipes=recipes;
        result"""+source[end:]
    source=source.replace('self.r.core.textures.len()','self.r.core.native_texture_count()')
    source=source.replace('core.programs[program as usize].uniforms.iter().position(|(n,_,_)|n==name)', 'core.native_uniform_slot(program,name)')
    lines.append('mod '+module+' {\n'+source+'\n}')
text = '\n'.join(lines).rstrip() + '\n'
meta = {'class':'bferenderer','methods':[{ 'name':name,'args':args,'result':result } for name,args,result,_ in methods],
        'banks':[{'name':n,'type':t,'pointer':n+'_ptr','length':n+'_len'} for n,_,t in banks]}
outputs = [(OUT, text), (META, json.dumps(meta,indent=2)+'\n')]
for path, output in outputs:
    if '--check' in sys.argv:
        if not path.exists() or path.read_text() != output: raise SystemExit(f'{path.name} is stale')
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(output)
