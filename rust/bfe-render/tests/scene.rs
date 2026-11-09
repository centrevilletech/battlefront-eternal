// Synthetic scene facts cover the decision before shader replay. These tests
// exercise the production registry, including mutation and stale generations.
use bfe_render::scene::{SceneRegistry, layout::{scene_node as n, scene_view as v,
    scene_flags as f, scene_family as family, scene_mode as mode, scene_selection as s,scene_group as g}};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
struct Count;
// Per thread: tests run in parallel, and only the tracked frame counts.
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
fn tracking() -> bool {
    TRACK.try_with(Cell::get).unwrap_or(false)
}
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if tracking() {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if tracking() {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static ALLOCATOR: Count = Count;

const IDENTITY: [f64; 16] = [1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];

#[test]
fn canopy_selection_keeps_ties_ranges_cache_layers_and_resource_generations() {
    use bfe_render::{scene_canopies::CanopySelection, scene::layout::{
        scene_canopy_view as v, scene_canopy_header as h, scene_canopy_selection as s}};
    let mut canopy = CanopySelection::default();
    canopy.reserve(40, 1);
    for sphere in canopy.bounds.as_chunks_mut::<4>().0 { sphere.copy_from_slice(&[0., 0., 0., 1.]); }
    canopy.view[v::GENERATION] = 1.; canopy.view[v::ENABLED] = 1.;
    canopy.view[v::LAYERS] = 1.; canopy.view[v::NODE_LAYERS] = 1.;
    for at in [v::PROJECTION, v::VIEW, v::WORLD] { canopy.view[at..at + 16].copy_from_slice(&IDENTITY); }
    assert_eq!(canopy.select(), 12);
    for i in 0..12 {
        let at = h::STRIDE + i * s::STRIDE;
        assert_eq!(canopy.output[at + s::ID], i as f64, "source-order distance tie");
        assert_eq!(canopy.output[at + s::FIRST], (i * 912) as f64);
        assert_eq!(canopy.output[at + s::COUNT], 912.);
        assert_eq!(canopy.output[at + s::CHANGED], 1.);
    }
    assert_eq!(&canopy.output[h::DIRTY..h::DIRTY + 2], &[0., 10944.]);
    assert_eq!(canopy.output[h::VERSION], 1.);
    assert_eq!(canopy.states.iter().filter(|&&v| v == 1.).count(), 12);
    assert_eq!(canopy.select(), 12); assert_eq!(canopy.output[h::CHANGED], 0.);
    assert_eq!(canopy.output[h::STRIDE + s::CHANGED], 0.);
    canopy.view[v::LAYERS] = 2.; canopy.select(); assert_eq!(canopy.output[h::SHOWN], 0.);
    canopy.view[v::NODE_LAYERS] = -1.; canopy.select(); assert_eq!(canopy.output[h::SHOWN], 1.);
    canopy.view[v::GENERATION] = 2.; assert_eq!(canopy.select(), 0);
    canopy.view[v::GENERATION] = 1.; assert_eq!(canopy.select(), 12);
    canopy.view[v::ENABLED] = 0.; assert_eq!(canopy.select(), 0);
    assert!(canopy.states.iter().all(|&v| v == 0.));
    canopy.view[v::ENABLED] = 1.; assert_eq!(canopy.select(), 12);
    assert_eq!(canopy.output[h::STRIDE + s::CHANGED], 1.);
    canopy.reserve(40, 2); canopy.view[v::GENERATION] = 2.; canopy.view[v::ENABLED] = 0.;
    assert_eq!(canopy.select(), 0); assert!(canopy.states.iter().all(|&v| v == 0.));
}

#[test]
fn canopy_queries_over_content_capacity_allocate_nothing_during_views() {
    use bfe_render::{scene_canopies::CanopySelection, scene::layout::scene_canopy_view as v};
    let mut canopy = CanopySelection::default();
    canopy.reserve(100_000, 1);
    assert_eq!(canopy.bounds.len(), 65536 * 4);
    for sphere in canopy.bounds.as_chunks_mut::<4>().0 { sphere.copy_from_slice(&[0., 0., 0., 1.]); }
    canopy.view[v::GENERATION] = 1.; canopy.view[v::ENABLED] = 1.;
    for at in [v::PROJECTION, v::VIEW, v::WORLD] { canopy.view[at..at + 16].copy_from_slice(&IDENTITY); }
    ALLOCS.with(|n| n.set(0)); TRACK.with(|n| n.set(true));
    let mut selected = 0;
    for camera in 0..64 { canopy.view[v::CAMERA] = camera as f64; selected += canopy.select(); }
    TRACK.with(|n| n.set(false));
    assert_eq!(selected, 64 * 12); assert_eq!(ALLOCS.with(Cell::get), 0);
}

#[derive(serde::Deserialize)]
struct Replay { records: Vec<Fact> }
#[derive(serde::Deserialize)]
struct Fact { name: String, rows: Vec<Vec<f64>>, view: Vec<f64>, expected: Vec<Vec<f64>> }

#[test]
fn frozen_preselection_facts_keep_ids_keys_ranges_and_command_order() {
    // Captured before NearPass.walk on the frozen source. Expected records are
    // that producer's output, including its exact no-divide clip-Z arithmetic.
    let replay: Replay = serde_json::from_str(include_str!("fixtures/scene-facts.json")).unwrap();
    let mut scene = SceneRegistry::new(128);
    for fact in replay.records {
        assert_eq!(fact.view.len(), v::STRIDE, "{} view layout", fact.name);
        for (i, row) in fact.rows.iter().enumerate() {
            assert_eq!(row.len(), n::STRIDE, "{} node layout", fact.name);
            scene.changes[i * n::STRIDE..(i + 1) * n::STRIDE].copy_from_slice(row);
        }
        scene.apply(fact.rows.len()); scene.view.copy_from_slice(&fact.view);
        assert_eq!(scene.select(), fact.expected.len(), "{} selected count", fact.name);
        for (i, expected) in fact.expected.iter().enumerate() {
            assert_eq!(&scene.output[i*s::STRIDE..(i+1)*s::STRIDE], expected, "{} record {}", fact.name, i);
        }
    }
}

fn node(id: usize, parent: i32, generation: u32, flags: usize) -> [f64; n::STRIDE] {
    let mut r = [0.; n::STRIDE];
    r[n::SLOT] = id as f64; r[n::GENERATION] = generation as f64;
    r[n::PARENT] = parent as f64; r[n::PARENT_GENERATION] = 1.;
    r[n::FLAGS] = (f::ALIVE | f::VISIBLE | f::MATERIAL_VISIBLE | flags) as f64;
    r[n::SIBLING] = id as f64; r[n::OBJECT_ID] = (100 + id) as f64;
    r[n::LAYERS] = 1.; r[n::FAMILIES] = family::NEAR as f64;
    r[n::WORLD..n::WORLD + 16].copy_from_slice(&IDENTITY);
    r[n::RANGE + 1] = 30.; r[n::RANGE + 2] = 30.; r
}
fn apply(scene: &mut SceneRegistry, rows: &[[f64; n::STRIDE]]) {
    for (i, row) in rows.iter().enumerate() { scene.changes[i*n::STRIDE..(i+1)*n::STRIDE].copy_from_slice(row); }
    scene.apply(rows.len());
}
fn view(scene: &mut SceneRegistry, mode: usize) {
    scene.view[v::ROOT] = 0.; scene.view[v::GENERATION] = 1.;
    scene.view[v::LAYERS] = 1.; scene.view[v::FAMILIES] = family::NEAR as f64;
    scene.view[v::MODE] = mode as f64;
    scene.view[v::PROJECTION..v::PROJECTION+16].copy_from_slice(&IDENTITY);
    scene.view[v::VIEW..v::VIEW+16].copy_from_slice(&IDENTITY);
}
fn ids(scene: &mut SceneRegistry) -> Vec<usize> {
    let count = scene.select();
    (0..count).map(|i| scene.output[i*s::STRIDE+s::SLOT] as usize).collect()
}

#[test]
fn hidden_parent_suppresses_children_but_layer_mismatch_does_not() {
    let mut scene = SceneRegistry::new(16);
    let mut root = node(0,-1,1,0); root[n::FAMILIES] = 0.;
    let mut group = node(1,0,1,f::GROUP); group[n::LAYERS] = 2.; group[n::RENDER_ORDER] = 7.;
    apply(&mut scene, &[root, group, node(2,1,1,0)]);
    view(&mut scene,mode::SCENE); assert_eq!(ids(&mut scene), [2]);
    assert_eq!(scene.output[s::GROUP_ORDER], 0.);
    view(&mut scene,mode::VIEW); assert_eq!(ids(&mut scene), [2]);
    assert_eq!(scene.output[s::GROUP_ORDER], 7.);
    group[n::FLAGS] = (f::ALIVE | f::GROUP) as f64;
    apply(&mut scene, &[group]); assert!(ids(&mut scene).is_empty());
}

#[test]
fn reparent_detach_sibling_order_and_stale_generations() {
    let mut scene = SceneRegistry::new(16);
    let mut root = node(0,-1,1,0); root[n::FAMILIES] = 0.;
    let mut first = node(1,0,1,0); let mut second = node(2,0,1,0);
    apply(&mut scene,&[root,first,second]); view(&mut scene,mode::SCENE);
    assert_eq!(ids(&mut scene), [1,2]);
    first[n::PARENT] = -1.; apply(&mut scene,&[first]); assert_eq!(ids(&mut scene), [2]);
    first[n::PARENT] = 0.; first[n::SIBLING] = 5.; apply(&mut scene,&[first]); assert_eq!(ids(&mut scene), [2,1]);
    second[n::GENERATION] = 2.; apply(&mut scene,&[second]);
    let mut stale = second; stale[n::GENERATION] = 1.; stale[n::FLAGS] = 0.;
    apply(&mut scene,&[stale]); assert_eq!(ids(&mut scene), [2,1]); assert_eq!(scene.rejected,1);
    // A parent's reused slot must not adopt children from the previous life.
    root[n::GENERATION] = 2.; apply(&mut scene,&[root]); scene.view[v::GENERATION] = 2.;
    assert!(ids(&mut scene).is_empty());
}

#[test]
fn exact_clip_z_ties_and_draw_ranges() {
    let mut scene = SceneRegistry::new(16);
    let mut root = node(0,-1,1,0); root[n::FAMILIES] = 0.;
    let mut first = node(1,0,1,f::TRANSPARENT); let mut second = node(2,0,1,f::TRANSPARENT);
    first[n::WORLD+14] = 0.25; second[n::WORLD+14] = 0.75;
    second[n::RANGE] = 3.; second[n::RANGE+1] = 12.; second[n::RANGE+2] = 10.;
    apply(&mut scene,&[root,first,second]); view(&mut scene,mode::TRANSPARENT);
    scene.view[v::PROJECTION+11] = 0.5;
    assert_eq!(ids(&mut scene), [2,1]);
    assert_eq!(scene.output[s::DEPTH], 0.75); // Vector4: never divide by clip W.
    assert_eq!(&scene.output[s::FIRST..s::COUNT+1], &[3.,7.]);
    first[n::WORLD+14] = 0.75; apply(&mut scene,&[first]); assert_eq!(ids(&mut scene), [1,2]);
}

#[test]
fn content_overflow_and_invalid_boundary_rows_do_not_allocate_or_panic() {
    let mut scene = SceneRegistry::new(8);
    let mut root = node(0,-1,1,0); root[n::FAMILIES] = 0.;
    apply(&mut scene,&[root,node(1,0,1,0)]); view(&mut scene,mode::SCENE);
    let mut bad = node(99,0,1,0); apply(&mut scene,&[bad]);
    bad[n::SLOT] = f64::NAN; apply(&mut scene,&[bad]);
    bad = node(1,1,1,0); apply(&mut scene,&[bad]);
    assert_eq!(ids(&mut scene),[1]); assert_eq!(scene.rejected,3);
    let changes = scene.changes.as_ptr(); let output = scene.output.as_ptr();
    for _ in 0..10000 { scene.apply(usize::MAX); scene.select(); }
    assert_eq!(scene.changes.as_ptr(),changes); assert_eq!(scene.output.as_ptr(),output);
}

#[test]
fn oversized_scene_and_paused_change_bursts_keep_reserved_storage() {
    let count = 16384;
    let mut scene = SceneRegistry::new(count);
    for i in 0..count {
        let mut row = node(i, if i == 0 { -1 } else { 0 }, 1, f::TRANSPARENT);
        if i == 0 { row[n::FAMILIES] = 0.; }
        scene.changes[i*n::STRIDE..(i+1)*n::STRIDE].copy_from_slice(&row);
    }
    scene.apply(count); view(&mut scene, mode::TRANSPARENT); scene.select();
    assert_eq!(scene.selected, count-1);
    // More than a map's rows and a paused producer's repeated observations are
    // bounded by the imported block. Selection never extends a retained Vec.
    ALLOCS.with(|n| n.set(0)); TRACK.with(|n| n.set(true));
    for _ in 0..4 { scene.apply(usize::MAX); scene.select(); }
    TRACK.with(|n| n.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert_eq!(scene.selected, count-1);
    scene.changes[n::SLOT] = (count + 1000) as f64;
    scene.apply(1);
    assert_eq!(scene.rejected, 1);
}

#[test]
fn near_output_clips_to_native_draw_capacity_without_losing_scene_facts() {
    let count = bfe_render::transparent::MAX_OBJECTS*2;
    let mut scene = SceneRegistry::new(count + 1);
    for i in 0..=count {
        let mut row = node(i, if i == 0 { -1 } else { 0 }, 1, f::TRANSPARENT);
        if i == 0 { row[n::FAMILIES] = 0.; }
        scene.changes[i*n::STRIDE..(i+1)*n::STRIDE].copy_from_slice(&row);
    }
    scene.apply(count + 1);view(&mut scene,mode::SCENE);
    // NearPass formerly filled its retained pool in traversal order. Preserve
    // that prefix while the full registry remains available to other views.
    let expected=bfe_render::transparent::MAX_OBJECTS;
    assert_eq!(scene.select(),expected);assert_eq!(scene.clipped,(count-expected) as u64);
    assert_eq!(scene.output[s::SLOT],1.);
    assert_eq!(scene.output[(expected-1)*s::STRIDE+s::SLOT],expected as f64);
    view(&mut scene,mode::TRANSPARENT);
    assert_eq!(scene.select(),count);
}

#[test]
fn material_groups_keep_visibility_ranges_order_and_node_generation() {
    let mut scene=SceneRegistry::new(16);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;
    let mut mesh=node(1,0,1,f::MATERIAL_ARRAY|f::CAST_SHADOW);
    mesh[n::GROUP_COUNT]=3.;mesh[n::RANGE]=3.;mesh[n::RANGE+1]=9.;mesh[n::RANGE+2]=12.;
    apply(&mut scene,&[root,mesh]);
    for i in 0..3 {
        let at=i*g::STRIDE;let r=&mut scene.group_changes[at..at+g::STRIDE];
        r[g::SLOT]=i as f64;r[g::OWNER]=1.;r[g::GENERATION]=1.;
        r[g::MATERIAL_ID]=(21+i) as f64;r[g::MATERIAL_INDEX]=i as f64;
        r[g::VISIBLE]=if i==1{0.}else{1.};r[g::FIRST]=(i*6) as f64;r[g::COUNT]=6.;
    }
    // The last group overlaps only the end of the mesh's draw range.
    scene.group_changes[2*g::STRIDE+g::FIRST]=9.;scene.apply_groups(3);
    view(&mut scene,mode::SHADOW);assert_eq!(ids(&mut scene),[1,1]);
    assert_eq!(&scene.output[s::FIRST..s::COUNT+1],&[3.,3.]);
    assert_eq!(&scene.output[s::STRIDE+s::FIRST..s::STRIDE+s::COUNT+1],&[9.,3.]);
    assert_eq!(scene.output[s::GROUP],0.);assert_eq!(scene.output[s::STRIDE+s::GROUP],2.);
    assert_eq!(scene.output[s::STRIDE+s::MATERIAL_ID],23.);
    // Motion's locked mask uses the same authored groups and range selection.
    view(&mut scene,mode::LOCKED);assert_eq!(ids(&mut scene),[1,1]);
    scene.view[v::OUTPUT_LIMIT]=1.;assert_eq!(scene.select(),1);assert_eq!(scene.clipped,1);
    mesh[n::GENERATION]=2.;apply(&mut scene,&[mesh]);
    scene.view[v::OUTPUT_LIMIT]=0.;scene.apply_groups(3);
    assert_eq!(scene.select(),0);assert_eq!(scene.rejected,3);
    for i in 0..3 {scene.group_changes[i*g::STRIDE+g::GENERATION]=2.;}
    scene.apply_groups(3);assert_eq!(scene.select(),2);
}

// Line uniforms are packed after the other scene clients have selected their
// rows. A later query must not overwrite the retained line ids or painter keys.
#[test]
fn line_selection_survives_other_queries_and_clears_an_invalid_view() {
    use bfe_render::scene::layout::scene_line_header as h;
    let mut scene=SceneRegistry::new(8);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;
    let mut line=node(1,0,1,f::TRANSPARENT);line[n::FAMILIES]=family::LINE as f64;
    line[n::RESOURCE]=17.;line[n::RENDER_ORDER]=9.;
    let mut pending=node(2,0,1,0);pending[n::FAMILIES]=family::LINE as f64;pending[n::RESOURCE] = -1.;
    apply(&mut scene,&[root,line,pending]);view(&mut scene,mode::VIEW);
    scene.view[v::FAMILIES]=family::LINE as f64;
    assert_eq!(scene.select(),1);assert_eq!(scene.line_output[h::COUNT],1.);
    assert_eq!(scene.line_output[h::STRIDE+s::RESOURCE],17.);
    assert_eq!(scene.line_output[h::STRIDE+s::FLAGS] as usize&f::TRANSPARENT,f::TRANSPARENT);
    let retained=scene.line_output.clone();
    view(&mut scene,mode::SHADOW);assert_eq!(scene.select(),0);
    assert_eq!(scene.line_output,retained);
    scene.view[v::FAMILIES]=family::LINE as f64;scene.view[v::ROOT]=f64::NAN;
    assert_eq!(scene.select(),0);assert_eq!(scene.line_output[h::COUNT],0.);
}

// Static depth descriptors supply their own spatial bounds. Their Object3D
// anchor can be outside a cascade while some of its retained cells intersect.
#[test]
fn native_depth_anchors_keep_their_owners_spatial_selection() {
    let mut scene=SceneRegistry::new(4);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;
    let mut anchor=node(1,0,1,f::CAST_SHADOW|f::CULLED|f::NATIVE_CULL);
    anchor[n::FAMILIES]=family::SHADOW as f64;anchor[n::WORLD+12]=1000.;
    let mut mesh=anchor;mesh[n::SLOT]=2.;mesh[n::FLAGS]=(f::ALIVE|f::VISIBLE|f::MATERIAL_VISIBLE|f::CAST_SHADOW|f::CULLED) as f64;
    apply(&mut scene,&[root,anchor,mesh]);view(&mut scene,mode::SHADOW);
    scene.view[v::FAMILIES]=family::SHADOW as f64;
    assert_eq!(ids(&mut scene),[1]);
}

// ShadowPass.walk never substitutes geometry for an unculled caster's bounds.
// Some authored buffers have NaN centres; they still retain their draw range
// and scene identity. Frustum's comparisons also keep those legacy semantics.
#[test]
fn nonfinite_unused_bounds_keep_caster_identity_ranges_and_nan_keys() {
    let mut scene=SceneRegistry::new(4);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;
    let mut caster=node(1,0,1,f::CAST_SHADOW|f::CULLED);
    caster[n::FAMILIES]=family::SHADOW as f64;caster[n::SPHERE]=f64::NAN;
    apply(&mut scene,&[root,caster]);view(&mut scene,mode::SHADOW);
    scene.view[v::FAMILIES]=family::SHADOW as f64;
    assert_eq!(ids(&mut scene),[1]);assert!(scene.output[s::DEPTH].is_nan());
    assert_eq!(&scene.output[s::FIRST..s::COUNT+1],&[0.,30.]);
}

// MotionVectors.prepare updates nearby histories even outside the frustum.
// Only its source's default layer matters; ancestor layers never prune it.
#[test]
fn motion_sources_select_before_frustum_and_emit_bounded_lifecycle_events() {
    use bfe_render::scene::layout::{scene_motion_header as h,scene_motion_event as e,scene_motion_kind as k};
    let mut scene=SceneRegistry::new(8);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;
    let mut group=node(1,0,1,f::GROUP);group[n::FAMILIES]=0.;group[n::LAYERS]=2.;
    let mut source=node(2,1,1,0);source[n::WORLD+12]=2.;
    let mut motion=node(3,-1,1,0);motion[n::FAMILIES]=0.;
    let mut proxy=node(4,3,1,f::MOTION_SOURCE|f::CULLED);proxy[n::FLAGS]-=f::VISIBLE as f64;
    proxy[n::FAMILIES]=family::MOTION as f64;proxy[n::MOTION_SOURCE]=2.;proxy[n::MOTION_SOURCE+1]=1.;proxy[n::MOTION_FAR]=60.;proxy[n::WORLD+12]=900.;
    apply(&mut scene,&[root,group,source,motion,proxy]);
    view(&mut scene,mode::MOTION_PREPARE);scene.view[v::ROOT]=3.;scene.view[v::FAMILIES]=family::MOTION as f64;
    scene.view[v::MOTION_ROOT]=0.;scene.view[v::MOTION_ROOT+1]=1.;
    assert_eq!(ids(&mut scene),[4]);assert_eq!(scene.motion_events[h::EVENTS],0.);
    scene.view[v::MODE]=mode::PROXY as f64;assert_eq!(scene.select(),0);
    scene.view[v::MODE]=mode::MOTION_PREPARE as f64;
    source[n::WORLD+12]=61.;apply(&mut scene,&[source]);assert_eq!(scene.select(),0);
    assert_eq!(scene.motion_events[h::EVENTS],1.);assert_eq!(scene.motion_events[h::STRIDE+e::KIND],k::CLEAR as f64);
    assert_eq!(scene.select(),0);assert_eq!(scene.motion_events[h::EVENTS],0.);
    source[n::WORLD+12]=2.;apply(&mut scene,&[source]);assert_eq!(ids(&mut scene),[4]);
    // Hidden or non-default-layer pooled bodies never age as detached.
    source[n::LAYERS]=2.;source[n::PARENT] = -1.;apply(&mut scene,&[source]);
    scene.select();
    for _ in 0..700 {assert_eq!(scene.select(),0);assert_eq!(scene.motion_events[h::EVENTS],0.);}
    source[n::LAYERS]=1.;apply(&mut scene,&[source]);
    ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
    for _ in 0..600 {assert_eq!(scene.select(),0);assert_eq!(scene.motion_events[h::EVENTS],0.);}
    assert_eq!(scene.select(),0);TRACK.with(|n|n.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
    assert_eq!(scene.motion_events[h::EVENTS],1.);assert_eq!(scene.motion_events[h::STRIDE+e::KIND],k::RELEASE as f64);
    assert_eq!(scene.motion_events[h::STRIDE+e::SLOT],4.);assert_eq!(scene.motion_events[h::STRIDE+e::GENERATION],1.);
    // A fresh proxy life never inherits the expired life or a stale source.
    proxy[n::GENERATION]=2.;source[n::GENERATION]=2.;source[n::PARENT]=1.;apply(&mut scene,&[source,proxy]);
    assert_eq!(scene.select(),0);assert_eq!(scene.motion_events[h::EVENTS],0.);
    proxy[n::MOTION_SOURCE+1]=2.;apply(&mut scene,&[proxy]);assert_eq!(ids(&mut scene),[4]);
}

#[test]
fn signed_layer_masks_keep_enable_all_and_the_high_bit() {
    let mut scene=SceneRegistry::new(4);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;root[n::LAYERS] = -1.;
    let mut mesh=node(1,0,1,0);mesh[n::LAYERS]=i32::MIN as f64;
    apply(&mut scene,&[root,mesh]);view(&mut scene,mode::SCENE);
    scene.view[v::LAYERS]=i32::MIN as f64;assert_eq!(ids(&mut scene),[1]);
    scene.view[v::LAYERS] = -1.;assert_eq!(ids(&mut scene),[1]);
    mesh[n::LAYERS] = -1.;apply(&mut scene,&[mesh]);scene.view[v::LAYERS]=1.;assert_eq!(ids(&mut scene),[1]);
}

#[test]
fn cached_views_keep_pose_changes_sparse_and_authored_visibility_current() {
    let mut scene=SceneRegistry::new(64);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;
    let mut group=node(1,0,1,f::GROUP);group[n::FAMILIES]=0.;group[n::RENDER_ORDER]=7.;
    let mut mesh=node(2,1,1,f::CULLED);mesh[n::SPHERE+3]=0.1;
    apply(&mut scene,&[root,group,mesh]);view(&mut scene,mode::SCENE);
    assert_eq!(ids(&mut scene),[2]);assert_eq!(scene.output[s::GROUP_ORDER],7.);
    let builds=scene.visibility_builds;let bounds=scene.bounds_updates;
    // Camera motion changes keys and culling, never authored visibility.
    scene.view[v::VIEW+14] = -0.25;
    assert_eq!(ids(&mut scene),[2]);assert_eq!(scene.output[s::DEPTH],-0.25);
    assert_eq!(scene.visibility_builds,builds);assert_eq!(scene.bounds_updates,bounds);
    mesh[n::WORLD+12]=4.;apply(&mut scene,&[mesh]);
    assert!(ids(&mut scene).is_empty());assert_eq!(scene.visibility_builds,builds);
    assert_eq!(scene.bounds_updates,bounds+1);
    mesh[n::WORLD+12]=0.;apply(&mut scene,&[mesh]);assert_eq!(ids(&mut scene),[2]);
    group[n::FLAGS]=(f::ALIVE|f::GROUP)as f64;apply(&mut scene,&[group]);
    assert!(ids(&mut scene).is_empty());assert!(scene.visibility_builds>builds);
    group[n::FLAGS]=(f::ALIVE|f::VISIBLE|f::GROUP)as f64;group[n::LAYERS]=2.;apply(&mut scene,&[group]);
    assert_eq!(ids(&mut scene),[2]);assert_eq!(scene.output[s::GROUP_ORDER],0.);
    scene.view[v::MODE]=mode::VIEW as f64;
    assert_eq!(ids(&mut scene),[2]);assert_eq!(scene.output[s::GROUP_ORDER],7.);
}

#[test]
fn cached_views_evict_without_allocation_and_keep_query_root_lives_separate() {
    let mut scene=SceneRegistry::new(64);
    for id in 0..32 {apply(&mut scene,&[node(id,-1,1,0)]);}
    view(&mut scene,mode::SCENE);
    // More camera/root policies than the retained cache has slots must clip
    // neither commands nor scene facts, and cannot allocate during selection.
    ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
    for id in 0..32 {
        scene.view[v::ROOT]=id as f64;
        assert_eq!(scene.select(),1);assert_eq!(scene.output[s::SLOT],id as f64);
    }
    TRACK.with(|n|n.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
    scene.view[v::ROOT]=0.;scene.view[v::GENERATION]=2.;assert_eq!(scene.select(),0);
    let mut replacement=node(0,-1,2,0);replacement[n::OBJECT_ID]=999.;apply(&mut scene,&[replacement]);
    assert_eq!(scene.select(),1);assert_eq!(scene.output[s::OBJECT_ID],999.);
}

// Terrain/effect shown queries need ancestor membership without forcing every
// camera to rewrite a scattered f64 row for each transform-only bone.
#[test]
fn cached_membership_keeps_transform_nodes_and_streams_only_drawable_keys() {
    let mut scene=SceneRegistry::new(2048);
    for id in 0..2048 {
        let mut r=node(id,if id==0{-1}else{0},1,0);
        if id!=2047 {r[n::FAMILIES]=0.;}
        scene.changes[id*n::STRIDE..(id+1)*n::STRIDE].copy_from_slice(&r);
    }
    scene.apply(2048);view(&mut scene,mode::SCENE);
    assert_eq!(ids(&mut scene),[2047]);
    assert!(scene.visible.iter().all(|&word|word==u32::MAX));
    assert_eq!(scene.visibility_visits,2048);assert_eq!(scene.drawable_visits,1);
    let builds=scene.visibility_builds;
    ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
    for _ in 0..32 {assert_eq!(scene.select(),1);}
    TRACK.with(|n|n.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
    // Retained queries stream only the family's drawables; the walk is not repeated.
    assert_eq!(scene.visibility_builds,builds);assert_eq!(scene.drawable_visits,33);
    assert_eq!(scene.visibility_visits,2048);
    // Invalid roots clear the previous membership rather than lending a prior
    // camera's shown state to a detached or recycled scene identity.
    scene.view[v::GENERATION]=2.;assert_eq!(scene.select(),0);
    assert!(scene.visible.iter().all(|&word|word==0));
    scene.view[v::GENERATION]=1.;assert_eq!(scene.select(),1);
    let mut hidden=node(0,-1,1,0);hidden[n::FAMILIES]=0.;hidden[n::FLAGS]=f::ALIVE as f64;
    apply(&mut scene,&[hidden]);assert_eq!(scene.select(),0);
    assert!(scene.visible.iter().all(|&word|word==0));
}

#[test]
fn stable_lod_choices_reuse_visits_and_crossings_keep_hysteresis() {
    let mut scene=SceneRegistry::new(8);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;
    let mut lod=node(1,0,1,0);lod[n::FAMILIES]=0.;lod[n::LOD_AUTO]=1.;
    let mut close=node(2,1,1,0);close[n::LOD_INDEX]=0.;
    let mut far=node(3,1,1,0);far[n::LOD_INDEX]=1.;far[n::LOD_DISTANCE]=10.;far[n::LOD_HYSTERESIS]=0.1;
    far[n::FLAGS]-=f::VISIBLE as f64;
    apply(&mut scene,&[root,lod,close,far]);view(&mut scene,mode::SCENE);scene.view[v::ZOOM]=1.;
    assert_eq!(ids(&mut scene),[2]);let builds=scene.visibility_builds;
    for distance in [0.,5.,9.5,9.99] {scene.view[v::EYE]=distance;assert_eq!(ids(&mut scene),[2]);}
    assert_eq!(scene.visibility_builds,builds,"an existing LOD is not a forced full-tree rebuild");
    scene.view[v::EYE]=10.;assert_eq!(ids(&mut scene),[3]);
    let builds=scene.visibility_builds;
    scene.view[v::EYE]=9.5;assert_eq!(ids(&mut scene),[3]);
    assert_eq!(scene.visibility_builds,builds);
    scene.view[v::EYE]=8.99;assert_eq!(ids(&mut scene),[2]);
    assert_eq!(scene.visible[0]&(1<<2),1<<2);assert_eq!(scene.visible[0]&(1<<3),0);
}

// Different native families share one retained walk; property flags still
// select independently, while sparse poses, camera bits and generations
// move bounds without expiring it.
#[test]
fn sparse_pose_lives_move_bounds_without_expiring_the_retained_walk() {
    use bfe_render::scene::layout::scene_world_change as w;
    let mut scene=SceneRegistry::new(64);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;
    let mut mesh=node(1,0,1,f::CULLED|f::CAST_SHADOW);mesh[n::SPHERE+3]=0.1;
    mesh[n::FAMILIES]=(family::NEAR|family::SHADOW) as f64;
    apply(&mut scene,&[root,mesh]);view(&mut scene,mode::SCENE);
    assert_eq!(ids(&mut scene),[1]);let builds=scene.visibility_builds;
    scene.view[v::MODE]=mode::SHADOW as f64;scene.view[v::FAMILIES]=family::SHADOW as f64;
    assert_eq!(ids(&mut scene),[1]);assert_eq!(scene.visibility_builds,builds);
    scene.world_changes[w::SLOT]=1.;scene.world_changes[w::GENERATION]=1.;
    scene.world_changes[w::WORLD..w::WORLD+16].copy_from_slice(&IDENTITY);
    scene.world_changes[w::WORLD+12]=4.;scene.apply_worlds(1);
    assert!(ids(&mut scene).is_empty());assert_eq!(scene.visibility_builds,builds);
    // The same unchanged pose does no work and a stale life cannot move it.
    let bounds=scene.bounds_updates;scene.apply_worlds(1);assert_eq!(scene.bounds_updates,bounds);
    scene.world_changes[w::GENERATION]=0.;scene.world_changes[w::WORLD+12]=0.;scene.apply_worlds(1);
    assert!(ids(&mut scene).is_empty());assert_eq!(scene.rejected,1);
    scene.world_changes[w::GENERATION]=1.;scene.apply_worlds(1);
    assert_eq!(ids(&mut scene),[1]);scene.view[v::VIEW+12]=4.;
    assert!(ids(&mut scene).is_empty());
    // Arbitrary camera motion remains bounded and allocates no frame data.
    ALLOCS.with(|n|n.set(0));TRACK.with(|v|v.set(true));
    for i in 0..80 {scene.view[v::VIEW+12]=i as f64;scene.select();}
    TRACK.with(|v|v.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
    // Oversized transport clamps to the reserved block instead of indexing it.
    scene.apply_worlds(usize::MAX);assert!(scene.rejected>1);
}

// Shadow and far owners stream only their family. Water and effects still
// read membership and the centre fact after any query, because membership
// is the walk's and the centre is published when bounds move; near, instance,
// line and sea preparation stamp a frustum receipt on every drawable.
#[test]
fn selection_streams_only_requested_families_and_keeps_contributor_facts() {
    use bfe_render::scene::layout::{scene_fact as fact,scene_facts_header as h};
    let mut scene=SceneRegistry::new(64);
    let mut root=node(0,-1,1,0);root[n::FAMILIES]=0.;
    let mut caster=node(1,0,1,f::CULLED|f::CAST_SHADOW);caster[n::FAMILIES]=family::SHADOW as f64;
    let mut effect=node(2,0,1,f::CULLED);effect[n::FAMILIES]=family::EFFECT as f64;effect[n::WORLD+12]=0.25;
    apply(&mut scene,&[root,caster,effect]);view(&mut scene,mode::SHADOW);
    scene.view[v::FAMILIES]=family::SHADOW as f64;assert_eq!(ids(&mut scene),[1]);
    assert_eq!(scene.drawable_visits,1);
    scene.view[v::FAMILIES]=family::FAR as f64;scene.view[v::MODE]=mode::FAR as f64;
    assert!(ids(&mut scene).is_empty());assert_eq!(scene.drawable_visits,1);
    // Sea preparation stamps the receipt every drawable it visits.
    scene.view[v::FAMILIES]=family::SEA as f64;
    assert!(ids(&mut scene).is_empty());assert_eq!(scene.drawable_visits,3);
    assert_eq!(scene.visible[0]&(1<<2),1<<2,"effect membership after a sea query");
    let effect_at=h::STRIDE+2*fact::STRIDE;
    assert_eq!(scene.facts[effect_at+fact::CENTER],0.25);
    effect[n::WORLD+12]=0.5;apply(&mut scene,&[effect]);
    assert_eq!(scene.facts[effect_at+fact::CENTER],0.5,"the centre fact follows bounds, not queries");
    scene.view[v::FAMILIES]=family::NEAR as f64;scene.view[v::MODE]=mode::SCENE as f64;
    assert!(ids(&mut scene).is_empty());assert_eq!(scene.drawable_visits,5);
    assert_eq!(scene.visible[0]&(1<<2),1<<2);
    assert_eq!(scene.facts[effect_at+fact::EPOCH],scene.facts[h::EPOCH]);
    assert_eq!(scene.facts[effect_at+fact::IN_VIEW],1.);
}
