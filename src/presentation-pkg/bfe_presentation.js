export class PagePresentation {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PagePresentationFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagepresentation_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    draws_len() {
        const ret = wasm.pagepresentation_draws_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    draws_ptr() {
        const ret = wasm.pagepresentation_draws_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    effects_len() {
        const ret = wasm.pagepresentation_effects_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    effects_ptr() {
        const ret = wasm.pagepresentation_effects_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    events_len() {
        const ret = wasm.pagepresentation_events_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    events_ptr() {
        const ret = wasm.pagepresentation_events_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Live effect ids the page may hold at once (id 0 is never issued).
     * @returns {number}
     */
    handles_len() {
        const ret = wasm.pagepresentation_handles_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    header_ptr() {
        const ret = wasm.pagepresentation_header_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagepresentation_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagepresentation_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} content
     */
    constructor(content) {
        const ptr0 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagepresentation_new(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        PagePresentationFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    present_frame() {
        wasm.pagepresentation_present_frame(this.__wbg_ptr);
    }
}
if (Symbol.dispose) PagePresentation.prototype[Symbol.dispose] = PagePresentation.prototype.free;

/**
 * The page's animated props and log traps: registration at load, then one
 * frame call over fixed records (src/presentation/props.js).
 */
export class PageProps {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PagePropsFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pageprops_free(ptr, 0);
    }
    /**
     * @param {string} data
     * @returns {number}
     */
    add_prop(data) {
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageprops_add_prop(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {string} data
     * @returns {number}
     */
    add_trap(data) {
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageprops_add_trap(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {number}
     */
    bodies_at() {
        const ret = wasm.pageprops_bodies_at(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {boolean} trap
     * @param {number} index
     * @returns {number}
     */
    bone_start(trap, index) {
        const ret = wasm.pageprops_bone_start(this.__wbg_ptr, trap, index);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    bones_len() {
        const ret = wasm.pageprops_bones_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    bones_ptr() {
        const ret = wasm.pageprops_bones_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    events_ptr() {
        const ret = wasm.pageprops_events_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    feet_at() {
        const ret = wasm.pageprops_feet_at(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    header_ptr() {
        const ret = wasm.pageprops_header_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pageprops_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pageprops_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} frames
     * @param {number} fps
     * @param {boolean} root_motion
     * @param {Int32Array | number[]} tracks
     * @param {Float32Array | number[]} data
     * @returns {number}
     */
    load_clip(frames, fps, root_motion, tracks, data) {
        const val3 = +frames;
        const val4 = +fps;
        const val5 = +root_motion;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(tracks, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(data, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.pageprops_load_clip(this.__wbg_ptr, val3, val4, val5, ptr0, len0, ptr1, len1);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pageprops_new();
        this.__wbg_ptr = ret;
        PagePropsFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    present_props() {
        wasm.pageprops_present_props(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    prop_input_ptr() {
        const ret = wasm.pageprops_prop_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    prop_output_ptr() {
        const ret = wasm.pageprops_prop_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    trap_input_ptr() {
        const ret = wasm.pageprops_trap_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    trap_output_ptr() {
        const ret = wasm.pageprops_trap_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageProps.prototype[Symbol.dispose] = PageProps.prototype.free;

/**
 * Thin browser binding for the unit phase. Registration owns allocations;
 * two record batches are split by the host camera phase, without re-entry.
 */
export class PageUnits {
    static __wrap(ptr) {
        const obj = Object.create(PageUnits.prototype);
        obj.__wbg_ptr = ptr;
        PageUnitsFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageUnitsFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pageunits_free(ptr, 0);
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    actor_node_start(id) {
        const ret = wasm.pageunits_actor_node_start(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    actor_skin_start(id) {
        const ret = wasm.pageunits_actor_skin_start(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @param {number} content
     * @param {string} inverses
     * @param {string} attachments
     * @returns {number}
     */
    add_actor(content, inverses, attachments) {
        const val3 = +content;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(inverses, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(attachments, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.pageunits_add_actor(this.__wbg_ptr, val3, ptr0, len0, ptr1, len1);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {string} data
     * @param {string} attachments
     * @returns {number}
     */
    add_view(data, attachments) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(attachments, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.pageunits_add_view(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {number}
     */
    changed_len() {
        const ret = wasm.pageunits_changed_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    changed_ptr() {
        const ret = wasm.pageunits_changed_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    commands_len() {
        const ret = wasm.pageunits_commands_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    commands_ptr() {
        const ret = wasm.pageunits_commands_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    header_ptr() {
        const ret = wasm.pageunits_header_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pageunits_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pageunits_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} data
     * @returns {number}
     */
    load_content(data) {
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageunits_load_content(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    constructor() {
        const ret = wasm.pageunits_new();
        this.__wbg_ptr = ret;
        PageUnitsFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    nodes_len() {
        const ret = wasm.pageunits_nodes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    nodes_ptr() {
        const ret = wasm.pageunits_nodes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    output_len() {
        const ret = wasm.pageunits_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    output_ptr() {
        const ret = wasm.pageunits_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    place_actors(count) {
        wasm.pageunits_place_actors(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    places_len() {
        const ret = wasm.pageunits_places_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    places_ptr() {
        const ret = wasm.pageunits_places_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    present_first_person() {
        wasm.pageunits_present_first_person(this.__wbg_ptr);
    }
    present_units() {
        wasm.pageunits_present_units(this.__wbg_ptr);
    }
    /**
     * @returns {PageUnits}
     */
    static preview() {
        const ret = wasm.pageunits_preview();
        return PageUnits.__wrap(ret);
    }
    /**
     * @param {number} id
     */
    refresh_actor(id) {
        wasm.pageunits_refresh_actor(this.__wbg_ptr, id);
    }
    /**
     * @returns {number}
     */
    skin_len() {
        const ret = wasm.pageunits_skin_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    skin_ptr() {
        const ret = wasm.pageunits_skin_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    view_node_start(id) {
        const ret = wasm.pageunits_view_node_start(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    weapons_len() {
        const ret = wasm.pageunits_weapons_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    weapons_ptr() {
        const ret = wasm.pageunits_weapons_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageUnits.prototype[Symbol.dispose] = PageUnits.prototype.free;

/**
 * The page's hulls: rigs registered once, then two calls a frame over
 * fixed records (src/presentation/vehicles.js).
 */
export class PageVehicles {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageVehiclesFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagevehicles_free(ptr, 0);
    }
    /**
     * @param {string} data
     * @returns {number}
     */
    add_rig(data) {
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagevehicles_add_rig(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {number}
     */
    entries_capacity() {
        const ret = wasm.pagevehicles_entries_capacity(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    header_ptr() {
        const ret = wasm.pagevehicles_header_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagevehicles_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagevehicles_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} frames
     * @param {number} fps
     * @param {boolean} root_motion
     * @param {Int32Array | number[]} tracks
     * @param {Float32Array | number[]} data
     * @returns {number}
     */
    load_clip(frames, fps, root_motion, tracks, data) {
        const val3 = +frames;
        const val4 = +fps;
        const val5 = +root_motion;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(tracks, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(data, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.pagevehicles_load_clip(this.__wbg_ptr, val3, val4, val5, ptr0, len0, ptr1, len1);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagevehicles_new();
        this.__wbg_ptr = ret;
        PageVehiclesFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    place_hulls() {
        wasm.pagevehicles_place_hulls(this.__wbg_ptr);
    }
    pose_hulls() {
        wasm.pagevehicles_pose_hulls(this.__wbg_ptr);
    }
    /**
     * @param {number} rig
     * @param {number} slot
     * @returns {number}
     */
    slot_input(rig, slot) {
        const ret = wasm.pagevehicles_slot_input(this.__wbg_ptr, rig, slot);
        return ret >>> 0;
    }
    /**
     * @param {number} rig
     * @param {number} slot
     * @returns {number}
     */
    slot_output(rig, slot) {
        const ret = wasm.pagevehicles_slot_output(this.__wbg_ptr, rig, slot);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    stream_in_len() {
        const ret = wasm.pagevehicles_stream_in_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    stream_in_ptr() {
        const ret = wasm.pagevehicles_stream_in_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    stream_out_len() {
        const ret = wasm.pagevehicles_stream_out_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    stream_out_ptr() {
        const ret = wasm.pagevehicles_stream_out_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageVehicles.prototype[Symbol.dispose] = PageVehicles.prototype.free;
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_throw_4c88d8bf264fd984: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbindgen_generic_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            table.set(0, undefined);
            table.set(1024, undefined);
            table.set(1025, null);
            table.set(1026, true);
            table.set(1027, false);
        },
    };
    return {
        __proto__: null,
        "./bfe_presentation_bg.js": import0,
    };
}

const PagePresentationFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagepresentation_free(ptr, 1));
const PagePropsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageprops_free(ptr, 1));
const PageUnitsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageunits_free(ptr, 1));
const PageVehiclesFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagevehicles_free(ptr, 1));

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function passArray32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getUint32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getFloat32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(bytes) {
    numBytesDecoded += bytes.length;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true });
        cachedTextDecoder.decode();
        numBytesDecoded = bytes.length;
    }
    return cachedTextDecoder.decode(bytes);
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedFloat32ArrayMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

let __wbg_init_promise;
async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;
    if (__wbg_init_promise === undefined) {
        __wbg_init_promise = __wbg_init_once(module_or_path);
        __wbg_init_promise.catch(() => { __wbg_init_promise = undefined; });
    }
    return __wbg_init_promise;
}

async function __wbg_init_once(module_or_path) {

    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('bfe_presentation_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
