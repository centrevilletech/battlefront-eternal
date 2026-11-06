#!/usr/bin/env python3
"""Generate the Rust resource journal from the WebGL registration boundary."""
from pathlib import Path
import re
import sys

# This generator owns the remember hooks and the complete Rust replay module.
# It does not infer a renderer from frame packets or save browser GL objects.
# Registration signatures are the resource boundary: exported calls carry the
# exact owned arguments needed to reconstruct one lost GPU allocation.
# Keep parsing and emitting deterministic so --check works on a clean checkout.
# Changing comments here must regenerate recovery.rs just like changing a hook.
check = "--check" in sys.argv
dirty = []
# Check mode compares the desired text without rewriting either output.
# A stale hook and a stale replay body are reported together to the build tool.
# Normal generation writes only changed files, preserving unrelated timestamps.
def emit(path, text):
    if not path.exists() or path.read_text() != text:
        dirty.append(str(path))
        if not check: path.write_text(text)
root = Path(__file__).resolve().parents[1]
web = root / 'rust/bfe-render/src/web'
methods = []
# Draw calls and shared-memory access are transient. Resource registrations and
# state updates must survive a context loss, with their logical handles intact.
# Borrowed texture handles are supplied afresh by the caller; only their slots
# are recorded separately. Frame/pointer/query methods must never be replayed.
# Replacing a sampled handle is likewise external ownership, not a GL payload.
# hud_model_* owns CPU-only presentation state outside Renderer. It survives
# replacement directly; replaying its commands would duplicate clocks/events.
skip = {'new', 'add_texture', 'near_begin', 'transparent_segment', 'foliage_selected', 'records_at', 'record_stride', 'motion_record_stride', 'motion_records_at', 'transparent_object_stride', 'far_items', 'rebuild', 'frame', 'replace_sample_texture'}
# Files and declarations define a reproducible API-tag order for coalescing.
# Never parse the generated replay implementation as another registration API.
# Existing hooks are stripped first, otherwise repeated generation would add
# another remember call and turn every resource write into duplicate history.
for path in sorted(web.glob('*.rs')):
    if path.stem == 'recovery': continue
    text = path.read_text()
    text = re.sub(r'\s*self\.remember_\w+\([^\n]*\); // recovery\s*', ' ', text)
    edits = []
    # This boundary uses ordinary exported Rust functions with scalar/slice
    # arguments. The parser is intentionally confined to those declarations;
    # it must not rewrite arbitrary Rust expressions or private helpers.
    for m in re.finditer(r'pub fn (\w+)\((.*?)\)\s*(?:->\s*([^\{]+))?\{', text, re.S):
        name, args, ret = m.group(1), m.group(2), (m.group(3) or '').strip()
        if name.startswith('hud_model_') or name in skip or name.endswith(('_ptr', '_len', '_frame')) or '&mut self' not in args: continue
        parsed = []
        for arg in args.split(','):
            arg = arg.strip()
            if not arg or arg == '&mut self': continue
            aname, ty = arg.split(':', 1)
            parsed.append((aname.strip(), ty.strip()))
        methods.append((name, parsed, ret))
        edits.append((m.end(), '\n        self.remember_' + name + '(' + ', '.join(n for n,t in parsed) + '); // recovery\n'))
    # Insert from the end so earlier source offsets remain valid. The emitted
    # hook runs before its resource operation, preserving call chronology.
    for pos, add in reversed(edits): text = text[:pos] + add + text[pos:]
    emit(path, text)

# Every retained API needs an explanation here. Keeping these beside the
# emitter makes a new resource method fail generation until its replay
# dependency is documented; the generated Rust is never edited by hand.
method_notes = dict(line.split('|', 1) for line in '''canopies_mesh|Recreate the canopy VAO and instance capacity; leaf rows come from the retained mirror.
fx_reserve|Retain effects capacity so moving the old instance mirror cannot exceed the new allocation.
fx_add_owner|Allocate the analytic owner's track binding before batches refer to its owner id.
fx_add_batch|Allocate the particle batch with its shader bindings and authored blend state.
fx_batch_texture|Restore the batch's primary sampler using a logical texture id, never an old GL handle.
fx_batch_values|Retain the material value layout; actual frame values remain in effects input memory.
fx_batch_interleaved|Preserve the interleaved instance layout expected by the batch's VAO.
fx_batch_side|Preserve single-sided particle culling rather than defaulting to a corner quad's state.
fx_batch_depth|Restore the non-default depth test/write flags and comparison function.
fx_batch_sampler|Restore an extra named 2D sampler on the same texture unit as its shader.
fx_batch_sampler_3d|Restore volume-noise sampling with a 3D binding rather than a 2D texture alias.
fx_batch_base|Keep the custom base vertices and indices used instead of a particle corner quad.
fx_add_mesh|Allocate the dynamic mesh's attribute/value layout before its shader passes are attached.
fx_mesh_pass|Attach one shader and side to a mesh; returned pass ids must retain their original order.
fx_mesh_texture|Restore the dynamic mesh's current primary texture without borrowing the old handle.
fx_add_model|Allocate the model's material layout and culling sphere before its draw passes.
fx_model_pass|Attach a model pass to its program and geometry in authored side order.
fx_model_texture|Restore the model's primary texture slot after the texture owner recreates its handle.
fx_model_sampler|Restore the model's extra named sampler and texture-unit assignment.
far_table|Recreate the far material's uniform layout before its items reference the table.
far_item|Allocate the far item with its geometry, sort key, depth and blend configuration.
far_source|Retain a far uniform source, including the shared-memory offset used on the next frame.
far_range|Restore the far item's element range; geometry allocation alone does not select its draw.
far_base|Restore the base vertex offset used by the far item's retained geometry.
far_instanced|Recreate a far instance binding and capacity before live instance rows are uploaded.
far_cells|Retain the placement/index data that builds the far item's cell selection structure.
far_terrain|Recreate the far terrain's retained index groups and cell bounds at its authored level.
far_range_input|Reconnect the far item's draw range to its shared input offset.
far_refresh|Retain the dirty-buffer source descriptor; reading frame vertices is deferred until drawing.
foliage_class|Allocate the foliage class from its base mesh, placements, bounds and material values.
hud_init|Recreate the HUD's static index buffer before its live vertex mirrors are reuploaded.
interface_buffer_init|Recreate the interface vertex mirror and its geometry binding before restoring live bytes.
inst_add_blend|Allocate the instanced material's depth/blend state and ordered shader passes.
set_shadow_maps|Restore logical cascade texture ids after the browser recreates the depth textures.
set_far_map|Reconnect the near scene's far-image sampler to its stable logical texture slot.
add_program|Relink a rewritten program from retained sources and explicit attribute/uniform bindings.
add_program_lamps|Relink a lit program with the lamp-table bindings required by its shader rewrite.
set_lamps|Retain the latest lamp table; rebuilding must not reset dynamic lighting to empty values.
add_owned_program|Relink the catalog key through the same cache so repeated keys keep their program id.
add_geometry|Allocate a geometry slot before buffer and pointer commands refer to that slot.
geometry_buffer|Allocate a geometry buffer with its initial bytes before configuring its attribute pointers.
geometry_pointer|Append a VAO attribute binding; a geometry can have several independently bound attributes.
geometry_index|Recreate the element buffer and its scalar type before range-based draws can resume.
geometry_group|Append one element group; collapsing groups would change bucket indices and draw order.
geometry_primitive|Restore line topology, indexed/array selection, first vertex and authored line width.
motion_extra|Allocate the extra motion geometry and layout before its old shared input rows are moved.
post_target|Recreate the framebuffer wrapper around the caller's newly restored target texture.
geometry_buffer_data|Keep the latest full vertex-buffer contents; the initial allocation may now be obsolete.
geometry_index_data|Keep the latest full element-buffer contents and corresponding draw capacity.
geometry_buffer_sub_data|Retain the exact byte range; a shorter write must not erase an earlier write's suffix.
geometry_index_sub_data|Retain the exact element-byte range so overlapping index edits replay in write order.
transparent_bucket|Allocate the blended bucket with its bounds, depth state and point-light policy.
transparent_bucket_pass|Restore a bucket's shader and side using the original program and pass ids.
transparent_bucket_texture|Restore the bucket's sampler unit without retaining the lost context's texture object.
transparent_bucket_uniform|Retain the bucket pass's latest named float/integer material value.
transparent_object|Allocate the object draw range and authored blend/depth state before adding its passes.
transparent_object_pass|Append an object pass in side order; double-sided rendering needs both entries.
transparent_object_texture|Restore the object's primary sampler through its logical texture id.
water_surface|Allocate the water surface with the same geometry, blend/depth state and side.
water_rings|Recreate ring indices and material state before the old ring vertex mirrors are reuploaded.
water_set|Recreate water-instance capacity and the attribute sizes expected by the live instance mirror.
water_instanced|Reconnect a water surface to its instance set, row and shader attribute location.
add_world|Allocate the world bucket with its program, bounds, sort order and raster flags.
world_texture|Restore the world bucket's sampler unit after native texture ownership has been rebuilt.
world_uniform|Retain the world's named material value with its integer/float interpretation intact.
world_cells|Recreate world placement cells from retained positions, indices and cell size.
sort|Retain the renderer's sort policy so restoration cannot change opaque submission ordering.
world_runs|Restore the world's visible index runs and associated culling boxes.
world_shown|Preserve authored world visibility rather than reviving a hidden bucket during restoration.
world_lamps|Restore the world bucket's lamp selection independently of the global lamp table.'''.splitlines())
assert {name for name, _, _ in methods} == set(method_notes), 'Document every resource registration in method_notes'

# Borrowed registration arguments must outlive the call: the journal owns
# strings and slices, but scalar handles keep their original numeric value.
def owned(ty):
    if ty == '&str': return 'String'
    if ty.startswith('&['): return 'Vec<' + ty[2:-1] + '>'
    return ty

# Only borrowed payloads need a first owned copy. Later updates reuse the
# saved String/Vec capacity rather than allocating a new frame-sized value.
def make(n,t): return n + '.to_owned()' if t.startswith('&') else n

# Replay borrows saved vectors/strings and dereferences copied scalars.
# These calls go through the same API as initial registration, with journaling
# disabled, so id allocation and shader validation follow one implementation.
def ref(n,t): return n if t.startswith('&') else '*' + n

out = [
       '// Generated by tools/renderer-recovery.py; edit the emitter, never this file.',
       '//',
       '// GPU reconstruction for RustRender.rebuild in src/rust-render/index.js.',
       '// The browser invalidates programs, buffers, VAOs and framebuffer objects',
       '// on context loss. This journal keeps CPU descriptions, not those handles.',
       '// The Rust resource bank first recreates textures and targets, then',
       '// passes their stable resource ids in the old logical slot order.',
       '//',
       '// Constructors remain append-only: replaying the same allocation calls',
       '// in the same order recreates program/geometry/batch ids. Frame records',
       '// and sampler references therefore need no remapping after restoration.',
       '// Scalar, string and range selectors identify replaceable state writes.',
       '// A repeated key reuses its saved payload storage and gets a new clock.',
       '// Different keys may overlap, so vector position is NOT replay order.',
       '// In particular, full-buffer writes and overlapping partial writes must',
       '// replay by their last-write clocks or stale bytes can overwrite newer ones.',
       '//',
       '// Never journal frame submission, shared-memory pointers, GPU queries,',
       '// fences or callbacks. Never draw or advance presentation during replay.',
       '// Newly created resources are incomplete until live CPU mirrors are moved',
       '// back and dynamic GPU buffers are reuploaded. Old GL handles must never',
       '// be rebound, and the reconstruction calls must not journal themselves.',
       '// A failed reconstruction is fatal to this battle; the caller must not',
       '// render with a partially rebuilt resource table or choose another drawer.',
       '//',
       'use super::BfeRenderer;', 'use wasm_bindgen::prelude::*;',
       'use std::collections::HashMap;', 'use std::hash::{Hash, Hasher};',
       '#[derive(Default)]', 'pub(super) struct Recovery {',
       '    // Entries retain owned arguments; their clocks survive in-place updates.',
       '    commands: Vec<(u64, Command)>,',
       '    // API tag, primary resource id and selector hash locate reusable entries.',
       '    slots: HashMap<(u16, u32, u64), usize>,',
       '    // Monotonic write order is independent of a coalesced entry\'s vector slot.',
       '    clock: u64,',
       '    // Texture ids belong to the caller; only their slot count is retained.',
       '    pub textures: u32,', '}',
       '// Typed variants keep registration payloads separate from transient frame data.',
       '// Handles inside a variant refer to the append-only tables recreated by replay.',
       'enum Command {',
       '    // Only the slot is saved: a restored texture object comes from the caller.',
       '    Texture(u32),']
for name,args,ret in methods:
    out.append('    ' + name + ' { ' + ', '.join(n+': '+owned(t) for n,t in args) + ' },')
out += ['}', '#[allow(non_camel_case_types)]', 'impl BfeRenderer {',
        '    // add_texture has an external object, so remember its position rather than',
        '    // retaining a WebGlTexture that becomes unusable with the lost context.',
        '    pub(super) fn remember_texture(&mut self) {',
        '        if self.rebuilding { return; }',
        '        let slot = self.recovery.textures;',
        '        self.recovery.textures += 1;',
        '        self.recovery.clock += 1;',
        '        self.recovery.commands.push((self.recovery.clock, Command::Texture(slot)));', '    }']
# Returned ids and append-only lists cannot be compacted: subsequent resource
# descriptions refer to their positions. Setters can replace only an identical
# selector key; writes with different keys still retain independent clocks.
for ident,(name,args,ret) in enumerate(methods):
    append = ('u32' in ret and not ret.startswith('bool')) or name in {'geometry_pointer', 'geometry_group', 'transparent_object_pass'}
    signature = ', '.join(n+': '+t for n,t in args)
    out += [f'    // BfeRenderer::{name}: {method_notes[name]}',
            ('    // Keep every call: later registrations depend on the returned id or append order.' if append else '    // Coalesce only the same selector key; other writes retain their own chronology.'),
            f'    pub(super) fn remember_{name}(&mut self'+(', '+signature if signature else '')+') {',
            '        if self.rebuilding { return; }', '        self.recovery.clock += 1;']
    # The first integer is the primary resource; later scalar/string selectors
    # distinguish its binding, unit, pass or range. Payload slices are excluded
    # so changed bytes replace the saved data instead of growing the journal.
    if not append:
        first = args[0][0] if args and args[0][1] in ('u32','i32') else '0'
        out += ['        let '+('mut ' if any(t in ('&str', 'u32', 'i32', 'bool') for n,t in args[1:]) else '')+'hash = std::collections::hash_map::DefaultHasher::new();']
        # Preserve selector distinctions even when they currently have the same
        # payload. A sampler name and an element offset are not interchangeable.
        for n,t in args[1:]:
            if t in ('&str', 'u32', 'i32', 'bool'): out.append(f'        {n}.hash(&mut hash);')
        # Only a write of the identical range supersedes a previous partial
        # write. A shorter update at the same offset leaves its suffix intact.
        if name in {'geometry_buffer_sub_data', 'geometry_index_sub_data'}:
            out.append('        data.len().hash(&mut hash);')
        out += [f'        let key = ({ident}u16, {first} as u32, hash.finish());',
                '        if let Some(&recovery_at) = self.recovery.slots.get(&key) {',
                f'            if let Command::{name} {{ '+', '.join(n+': old_'+n for n,t in args)+' } = &mut self.recovery.commands[recovery_at].1 {']
        # Reuse payload storage on a matching key, then move its timestamp.
        # The timestamp matters even if the replacement bytes happen to match:
        # an overlapping write to another key may have occurred in between.
        for n,t in args:
            if t.startswith('&['): out.append(f'                old_{n}.clear(); old_{n}.extend_from_slice({n});')
            elif t == '&str': out.append(f'                old_{n}.clear(); old_{n}.push_str({n});')
            else: out.append(f'                *old_{n} = {n};')
        out += ['            }',
                '            // Move this write after resources or overlapping writes added since it.',
                '            // Updating the payload without its clock would replay stale ordering.',
                '            self.recovery.commands[recovery_at].0 = self.recovery.clock;', '            return;', '        }',
                '        self.recovery.slots.insert(key, self.recovery.commands.len());']
    out += [f'        self.recovery.commands.push((self.recovery.clock, Command::{name} {{ '+', '.join(n+': '+make(n,t) for n,t in args)+' }));', '    }']
# Reconstruction is deliberately a cold path. It allocates a replay order,
# but the ordinary remember setters reuse their buffers on the frame path.
# Errors propagate to the required renderer's fatal GPU-labelled error screen;
# no frame may use the new renderer before all replay operations succeed.
out += ['}', '#[wasm_bindgen]', 'impl BfeRenderer {',
        '    // Called by RustRender.rebuild after the Rust image bank has rebuilt its textures.',
        '    // No frame, callbacks, GPU queries or fences may run until this returns Ok.',
        '    pub fn rebuild(&mut self, context: web_sys::WebGl2RenderingContext, resources: &crate::resources::GpuResources, textures: &[u32]) -> Result<(), JsValue> {',
        '        // Detach the journal before replacing GPU ownership. The old renderer',
        '        // still owns CPU mirrors needed below, but none of its GL handles is valid.',
        '        let mut saved = std::mem::take(&mut self.recovery);',
        '        let mut old = std::mem::replace(&mut self.r, BfeRenderer::new(context).r);',
        '        // Replay uses the public registration methods, whose remember hooks',
        '        // must stay inert rather than recursively appending the replay itself.',
        '        self.rebuilding = true;',
        '        let mut order: Vec<usize> = (0..saved.commands.len()).collect();',
        '        // Coalescing changes clocks in place; sorting by vector index would',
        '        // bind new dependencies too early and corrupt overlapping buffer edits.',
        '        order.sort_by_key(|&i| saved.commands[i].0);',
        '        for recovery_at in order { match &saved.commands[recovery_at].1 {',
        '            Command::Texture(slot) => { let texture = *textures.get(*slot as usize).ok_or_else(|| JsValue::from_str("Missing restored texture id"))?; self.add_texture(resources,texture)?; },']
for name,args,ret in methods:
    invoke = f'self.{name}('+', '.join(ref(n,t) for n,t in args)+')' + ('?' if ret.startswith('Result') else '')
    out.append('            Command::'+name+' { '+', '.join(n for n,t in args)+' } => { let _ = '+invoke+'; },')
out += ['        }}',
        '        // Allocations and bindings now have their original logical ids.',
        '        // Move, do not clone, the authoritative CPU mirrors across the reset.',
        '        // Published WASM views are rebound by the JS caller after this returns.']
# Persistent shared-memory mirrors are retained across GPU reconstruction.
# Frame inputs are live state, not registration commands. Moving each mirror
# preserves its values and allocation without replaying a draw that reads it.
# GPU-backed mirrors need an explicit upload after all resource ids exist.
# Adding another mirror here requires its owner to define that upload ordering.
fields = {'core':['input','bones','shadow_input'], 'effects':['input','data','matrices'], 'far':['input','vertices'], 'hud':['input','position','uv','mask_uv','tint'], 'interface':['input','streams'], 'canopies':['input','leaves'], 'post':['input','states'], 'motion':['input','bones','previous','extras_input','extra_values'], 'water':['input','rings_position','rings_uv','rings_alpha','instances'], 'foliage':['input'], 'view':['input'], 'shadow':['input'], 'transparent':['keys','objects']}
for owner,names in fields.items():
    out.append(f'        // Preserve {owner} input storage; replay must not reset the live frame values.')
    for name in names: out.append(f'        self.r.{owner}.{name} = std::mem::take(&mut old.{owner}.{name});')
out += ['        // These passes own dynamic GPU buffers backed by the moved mirrors.',
        '        // Populate them before the first restored frame can sample their VAOs.']
out += [f'        self.r.{name}.reupload(&mut self.r.core);' for name in ('hud','interface','canopies','water','far')]
out += ['        // Keep the same history for another loss. Only successful completion',
        '        // enables recording again; the caller treats any earlier error as fatal.',
        '        self.recovery = saved;', '        self.rebuilding = false;', '        Ok(())', '    }', '}']
# Emit one canonical file, including its explanation of recovery invariants.
# Variant spelling follows the public ABI so a missing replay arm is obvious
# when comparing a registration signature with the generated command list.
text='\n'.join(out)+'\n'
text=text.replace('enum Command {','#[allow(non_camel_case_types)]\nenum Command {').replace('        let mut saved =','        let saved =')
emit(web/'recovery.rs', text)
print('Generated',len(methods),'resource journal entries')

# Leave the build nonzero when generated text differs; callers must regenerate
# rather than resolving generated output by merging individual lines.
if check and dirty:
    raise SystemExit("Renderer recovery journal is stale: " + ", ".join(dirty))
