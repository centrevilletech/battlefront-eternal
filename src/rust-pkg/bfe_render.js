export class BfeDepthMarker {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BfeDepthMarkerFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bfedepthmarker_free(ptr, 0);
    }
    /**
     * @param {Float64Array | number[]} cameras
     * @param {number} role
     */
    begin_camera(cameras, role) {
        const val2 = +role;
        const ptr0 = passArrayF64ToWasm0(cameras, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bfedepthmarker_begin_camera(this.__wbg_ptr, ptr0, len0, val2);
    }
    finish() {
        wasm.bfedepthmarker_finish(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    key_len() {
        const ret = wasm.bfedepthmarker_key_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    key_ptr() {
        const ret = wasm.bfedepthmarker_key_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.bfedepthmarker_new();
        this.__wbg_ptr = ret;
        BfeDepthMarkerFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    output_len() {
        const ret = wasm.bfedepthmarker_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    output_ptr() {
        const ret = wasm.bfedepthmarker_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {boolean}
     */
    request() {
        const ret = wasm.bfedepthmarker_request(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    screen_len() {
        const ret = wasm.bfedepthmarker_screen_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    screen_ptr() {
        const ret = wasm.bfedepthmarker_screen_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) BfeDepthMarker.prototype[Symbol.dispose] = BfeDepthMarker.prototype.free;

export class BfePoseHistory {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BfePoseHistoryFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bfeposehistory_free(ptr, 0);
    }
    commit_frame() {
        wasm.bfeposehistory_commit_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    current_len() {
        const ret = wasm.bfeposehistory_current_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    current_ptr() {
        const ret = wasm.bfeposehistory_current_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    flyer_frame() {
        const ret = wasm.bfeposehistory_flyer_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {boolean} stepped
     * @returns {number}
     */
    flyer_sample_frame(id, stepped) {
        const ret = wasm.bfeposehistory_flyer_sample_frame(this.__wbg_ptr, id, stepped);
        return ret >>> 0;
    }
    /**
     * @param {number} skin
     * @param {Float64Array | number[]} bind
     * @returns {number}
     */
    model_frame(skin, bind) {
        const val2 = +skin;
        const ptr0 = passArrayF64ToWasm0(bind, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bfeposehistory_model_frame(this.__wbg_ptr, val2, ptr0, len0);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.bfeposehistory_new();
        this.__wbg_ptr = ret;
        BfePoseHistoryFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    poses_len() {
        const ret = wasm.bfeposehistory_poses_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    poses_ptr() {
        const ret = wasm.bfeposehistory_poses_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    prepare_frame(count) {
        wasm.bfeposehistory_prepare_frame(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    records_len() {
        const ret = wasm.bfeposehistory_records_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    records_ptr() {
        const ret = wasm.bfeposehistory_records_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     */
    release_frame(id) {
        wasm.bfeposehistory_release_frame(this.__wbg_ptr, id);
    }
    /**
     * @param {number} id
     */
    reset_frame(id) {
        wasm.bfeposehistory_reset_frame(this.__wbg_ptr, id);
    }
    /**
     * @param {number} id
     * @param {number} skin
     */
    reskin_frame(id, skin) {
        wasm.bfeposehistory_reskin_frame(this.__wbg_ptr, id, skin);
    }
    /**
     * @returns {number}
     */
    selected_len() {
        const ret = wasm.bfeposehistory_selected_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    selected_ptr() {
        const ret = wasm.bfeposehistory_selected_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    sequence() {
        const ret = wasm.bfeposehistory_sequence(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     * @returns {number}
     */
    skin_frame(count) {
        const ret = wasm.bfeposehistory_skin_frame(this.__wbg_ptr, count);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    skin_start(id) {
        const ret = wasm.bfeposehistory_skin_start(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     * @param {Float64Array} input
     * @param {Float32Array} bones
     * @param {Float32Array} previous
     */
    write_frame(count, input, bones, previous) {
        const val4 = +count;
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passArrayF64ToWasm0(input, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(bones, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArrayF32ToWasm0(previous, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 8, 8); }
            throw e;
        }
        wasm.bfeposehistory_write_frame(this.__wbg_ptr, val4, ptr0, len0, input, ptr1, len1, bones, ptr2, len2, previous);
    }
}
if (Symbol.dispose) BfePoseHistory.prototype[Symbol.dispose] = BfePoseHistory.prototype.free;

export class BfeRenderer {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BfeRendererFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bferenderer_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    actor_bones_len() {
        const ret = wasm.bferenderer_actor_bones_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    actor_bones_ptr() {
        const ret = wasm.bferenderer_actor_bones_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    actor_crowd_frame() {
        wasm.bferenderer_actor_crowd_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    actor_crowd_input_len() {
        const ret = wasm.bferenderer_actor_crowd_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    actor_crowd_input_ptr() {
        const ret = wasm.bferenderer_actor_crowd_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} source
     * @param {string} authored
     * @param {number} skin
     * @param {number} map
     * @returns {number | undefined}
     */
    actor_depth(source, authored, skin, map) {
        const val2 = +source;
        const val3 = +skin;
        const val4 = +map;
        const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_actor_depth(this.__wbg_ptr, val2, ptr0, len0, val3, val4);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] === Number.MAX_SAFE_INTEGER ? undefined : ret[0];
    }
    actor_depth_frame() {
        wasm.bferenderer_actor_depth_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} source
     * @param {number} kind
     * @param {boolean} shadows
     * @param {boolean} view
     * @param {number} texture
     * @param {string} authored
     * @returns {Float64Array}
     */
    actor_effect(source, kind, shadows, view, texture, authored) {
        const val2 = +source;
        const val3 = +kind;
        const val4 = +shadows;
        const val5 = +view;
        const val6 = +texture;
        const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_actor_effect(this.__wbg_ptr, val2, val3, val4, val5, val6, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v7 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v7;
    }
    /**
     * @param {number} source
     * @param {number} flags
     * @param {boolean} fading
     * @param {number} capacity
     * @param {number} texture
     * @param {number} slot
     * @param {number} object
     * @param {boolean} shadows
     * @returns {number | undefined}
     */
    actor_flyer(source, flags, fading, capacity, texture, slot, object, shadows) {
        const ret = wasm.bferenderer_actor_flyer(this.__wbg_ptr, source, flags, fading, capacity, texture, slot, object, shadows);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] === Number.MAX_SAFE_INTEGER ? undefined : ret[0];
    }
    /**
     * @param {number} source
     * @param {number} capacity
     * @param {Float32Array | number[]} matrices
     * @returns {Float64Array}
     */
    actor_flyer_motion(source, capacity, matrices) {
        const val2 = +source;
        const val3 = +capacity;
        const ptr0 = passArrayF32ToWasm0(matrices, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_actor_flyer_motion(this.__wbg_ptr, val2, val3, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v4 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v4;
    }
    /**
     * @param {number} source
     * @param {string} key
     * @param {boolean} whole
     * @returns {number}
     */
    actor_geometry(source, key, whole) {
        const val2 = +source;
        const val3 = +whole;
        const ptr0 = passStringToWasm0(key, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_actor_geometry(this.__wbg_ptr, val2, ptr0, len0, val3);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} source
     * @param {string} authored
     * @param {boolean} skin
     * @param {number} cascades
     * @param {boolean} lamps
     * @param {boolean} view
     * @param {Int32Array | number[]} samplers
     * @returns {Float64Array}
     */
    actor_material(source, authored, skin, cascades, lamps, view, samplers) {
        const val3 = +source;
        const val4 = +skin;
        const val5 = +cascades;
        const val6 = +lamps;
        const val7 = +view;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(samplers, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_actor_material(this.__wbg_ptr, val3, ptr0, len0, val4, val5, val6, val7, ptr1, len1);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v8 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v8;
    }
    /**
     * @param {number} source
     * @param {boolean} skin
     * @param {boolean} locked
     * @returns {number | undefined}
     */
    actor_motion(source, skin, locked) {
        const ret = wasm.bferenderer_actor_motion(this.__wbg_ptr, source, skin, locked);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] === Number.MAX_SAFE_INTEGER ? undefined : ret[0];
    }
    /**
     * @param {number} cascades
     * @param {boolean} lamps
     */
    actor_settings(cascades, lamps) {
        const ret = wasm.bferenderer_actor_settings(this.__wbg_ptr, cascades, lamps);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} texture
     * @param {number} bones
     * @returns {number | undefined}
     */
    actor_skin(texture, bones) {
        const ret = wasm.bferenderer_actor_skin(this.__wbg_ptr, texture, bones);
        return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
    }
    /**
     * @returns {number}
     */
    actor_skin_input_len() {
        const ret = wasm.bferenderer_actor_skin_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    actor_skin_input_ptr() {
        const ret = wasm.bferenderer_actor_skin_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    actor_skin_offset(id) {
        const ret = wasm.bferenderer_actor_skin_offset(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     */
    actor_skin_release(id) {
        wasm.bferenderer_actor_skin_release(this.__wbg_ptr, id);
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    actor_skin_slot(id) {
        const ret = wasm.bferenderer_actor_skin_slot(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @param {number} bones
     * @returns {number}
     */
    actor_skin_words(bones) {
        const ret = wasm.bferenderer_actor_skin_words(this.__wbg_ptr, bones);
        return ret >>> 0;
    }
    actor_skins_frame() {
        wasm.bferenderer_actor_skins_frame(this.__wbg_ptr);
    }
    /**
     * @param {string} authored
     * @param {Float32Array | number[]} vertices
     * @param {Uint32Array | number[]} indices
     * @returns {number | undefined}
     */
    actor_source(authored, vertices, indices) {
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(vertices, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArray32ToWasm0(indices, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_actor_source(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] === Number.MAX_SAFE_INTEGER ? undefined : ret[0];
    }
    /**
     * @returns {number}
     */
    actor_source_crowd() {
        const ret = wasm.bferenderer_actor_source_crowd(this.__wbg_ptr);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} body
     * @param {number} bone_base
     * @param {number} bones
     * @param {Uint32Array | number[]} sources
     * @param {Uint32Array | number[]} entries
     * @param {Uint32Array | number[]} groups
     * @param {Float64Array | number[]} matrices
     * @returns {Float64Array}
     */
    actor_source_crowd_add(body, bone_base, bones, sources, entries, groups, matrices) {
        const val5 = +body;
        const val6 = +bone_base;
        const val7 = +bones;
        let ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3;
        try {
            ptr0 = passArray32ToWasm0(sources, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(entries, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArray32ToWasm0(groups, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
            ptr3 = passArrayF64ToWasm0(matrices, wasm.__wbindgen_malloc);
            len3 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr3) { wasm.__wbindgen_free(ptr3, len3 * 8, 8); }
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_actor_source_crowd_add(this.__wbg_ptr, val5, val6, val7, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v8 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v8;
    }
    /**
     * @param {number} source
     * @returns {string}
     */
    actor_source_facts(source) {
        let deferred2_0;
        let deferred2_1;
        try {
            const ret = wasm.bferenderer_actor_source_facts(this.__wbg_ptr, source);
            var ptr1 = ret[0];
            var len1 = ret[1];
            if (ret[3]) {
                ptr1 = 0; len1 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    add_geometry() {
        const ret = wasm.bferenderer_add_geometry(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} key
     * @returns {number}
     */
    add_owned_program(key) {
        const ptr0 = passStringToWasm0(key, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_add_owned_program(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} kind
     * @param {string} vs
     * @param {string} fs
     * @param {string} attributes
     * @param {string} uniforms
     * @returns {number}
     */
    add_program(kind, vs, fs, attributes, uniforms) {
        const val5 = +kind;
        let ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3;
        try {
            ptr0 = passStringToWasm0(vs, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(fs, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passStringToWasm0(attributes, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len2 = WASM_VECTOR_LEN;
            ptr3 = passStringToWasm0(uniforms, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len3 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr3) { wasm.__wbindgen_free(ptr3, len3 * 1, 1); }
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 1, 1); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_add_program(this.__wbg_ptr, val5, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} kind
     * @param {string} vs
     * @param {string} fs
     * @param {string} attributes
     * @param {string} uniforms
     * @returns {number}
     */
    add_program_lamps(kind, vs, fs, attributes, uniforms) {
        const val5 = +kind;
        let ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3;
        try {
            ptr0 = passStringToWasm0(vs, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(fs, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passStringToWasm0(attributes, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len2 = WASM_VECTOR_LEN;
            ptr3 = passStringToWasm0(uniforms, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len3 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr3) { wasm.__wbindgen_free(ptr3, len3 * 1, 1); }
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 1, 1); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_add_program_lamps(this.__wbg_ptr, val5, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {GpuResources} resources
     * @param {number} id
     * @returns {number}
     */
    add_texture(resources, id) {
        _assertClass(resources, GpuResources);
        const ret = wasm.bferenderer_add_texture(this.__wbg_ptr, resources.__wbg_ptr, id);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} geometry
     * @param {number} program
     * @param {number} order
     * @param {number} flags
     * @param {Float64Array | number[]} bounds
     * @returns {number}
     */
    add_world(geometry, program, order, flags, bounds) {
        const val2 = +geometry;
        const val3 = +program;
        const val4 = +order;
        const val5 = +flags;
        const ptr0 = passArrayF64ToWasm0(bounds, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_add_world(this.__wbg_ptr, val2, val3, val4, val5, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * @param {GpuResources} resources
     * @returns {boolean}
     */
    battle_frame(resources) {
        _assertClass(resources, GpuResources);
        const ret = wasm.bferenderer_battle_frame(this.__wbg_ptr, resources.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    bones_len() {
        const ret = wasm.bferenderer_bones_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    bones_ptr() {
        const ret = wasm.bferenderer_bones_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_acquire_frame() {
        const ret = wasm.bferenderer_camera_acquire_frame(this.__wbg_ptr);
        return ret;
    }
    /**
     * @param {number} role
     */
    camera_canopy_view_frame(role) {
        wasm.bferenderer_camera_canopy_view_frame(this.__wbg_ptr, role);
    }
    camera_cascades_frame() {
        wasm.bferenderer_camera_cascades_frame(this.__wbg_ptr);
    }
    /**
     * @returns {boolean}
     */
    camera_clipped() {
        const ret = wasm.bferenderer_camera_clipped(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {number} from
     * @param {number} to
     */
    camera_copy_frame(from, to) {
        wasm.bferenderer_camera_copy_frame(this.__wbg_ptr, from, to);
    }
    /**
     * @param {number} role
     */
    camera_depth_begin_frame(role) {
        wasm.bferenderer_camera_depth_begin_frame(this.__wbg_ptr, role);
    }
    camera_depth_finish_frame() {
        wasm.bferenderer_camera_depth_finish_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    camera_depth_key_len() {
        const ret = wasm.bferenderer_camera_depth_key_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_depth_key_ptr() {
        const ret = wasm.bferenderer_camera_depth_key_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_depth_marker_len() {
        const ret = wasm.bferenderer_camera_depth_marker_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_depth_marker_ptr() {
        const ret = wasm.bferenderer_camera_depth_marker_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {boolean}
     */
    camera_depth_request_frame() {
        const ret = wasm.bferenderer_camera_depth_request_frame(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {number} role
     */
    camera_derived_frame(role) {
        wasm.bferenderer_camera_derived_frame(this.__wbg_ptr, role);
    }
    /**
     * @returns {number}
     */
    camera_evicted() {
        const ret = wasm.bferenderer_camera_evicted(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_extra_handles_len() {
        const ret = wasm.bferenderer_camera_extra_handles_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_extra_handles_ptr() {
        const ret = wasm.bferenderer_camera_extra_handles_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} role
     */
    camera_foliage_view_frame(role) {
        wasm.bferenderer_camera_foliage_view_frame(this.__wbg_ptr, role);
    }
    camera_global_shader_frame() {
        wasm.bferenderer_camera_global_shader_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} handle
     * @returns {number}
     */
    camera_import_frame(handle) {
        const ret = wasm.bferenderer_camera_import_frame(this.__wbg_ptr, handle);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_input_len() {
        const ret = wasm.bferenderer_camera_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_input_ptr() {
        const ret = wasm.bferenderer_camera_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     */
    camera_invalidate_frame(index) {
        wasm.bferenderer_camera_invalidate_frame(this.__wbg_ptr, index);
    }
    /**
     * @param {number} command
     */
    camera_light_command_frame(command) {
        wasm.bferenderer_camera_light_command_frame(this.__wbg_ptr, command);
    }
    /**
     * @returns {number}
     */
    camera_light_input_len() {
        const ret = wasm.bferenderer_camera_light_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_light_input_ptr() {
        const ret = wasm.bferenderer_camera_light_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_light_output_len() {
        const ret = wasm.bferenderer_camera_light_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_light_output_ptr() {
        const ret = wasm.bferenderer_camera_light_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} handle
     */
    camera_lights_frame(handle) {
        wasm.bferenderer_camera_lights_frame(this.__wbg_ptr, handle);
    }
    /**
     * @returns {number}
     */
    camera_lights_len() {
        const ret = wasm.bferenderer_camera_lights_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_lights_ptr() {
        const ret = wasm.bferenderer_camera_lights_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {boolean}
     */
    camera_main_frame() {
        const ret = wasm.bferenderer_camera_main_frame(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {Float64Array | number[]} bounds
     */
    camera_mirror_bounds_frame(bounds) {
        const ptr0 = passArrayF64ToWasm0(bounds, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_camera_mirror_bounds_frame(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @returns {number}
     */
    camera_mirror_len() {
        const ret = wasm.bferenderer_camera_mirror_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_mirror_ptr() {
        const ret = wasm.bferenderer_camera_mirror_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    camera_mirror_visibility_frame() {
        wasm.bferenderer_camera_mirror_visibility_frame(this.__wbg_ptr);
    }
    /**
     * @returns {boolean}
     */
    camera_mirrored_frame() {
        const ret = wasm.bferenderer_camera_mirrored_frame(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    camera_output_len() {
        const ret = wasm.bferenderer_camera_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_output_ptr() {
        const ret = wasm.bferenderer_camera_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_pack_acquire_frame() {
        const ret = wasm.bferenderer_camera_pack_acquire_frame(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    camera_pack_evicted() {
        const ret = wasm.bferenderer_camera_pack_evicted(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_pack_handles_len() {
        const ret = wasm.bferenderer_camera_pack_handles_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_pack_handles_ptr() {
        const ret = wasm.bferenderer_camera_pack_handles_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_pack_rejected() {
        const ret = wasm.bferenderer_camera_pack_rejected(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} handle
     * @returns {boolean}
     */
    camera_pack_release_frame(handle) {
        const ret = wasm.bferenderer_camera_pack_release_frame(this.__wbg_ptr, handle);
        return ret !== 0;
    }
    /**
     * @param {number} handle
     */
    camera_pack_shader_frame(handle) {
        wasm.bferenderer_camera_pack_shader_frame(this.__wbg_ptr, handle);
    }
    /**
     * @returns {number}
     */
    camera_packs_len() {
        const ret = wasm.bferenderer_camera_packs_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_packs_ptr() {
        const ret = wasm.bferenderer_camera_packs_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    camera_placement_frame(count) {
        wasm.bferenderer_camera_placement_frame(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    camera_placement_len() {
        const ret = wasm.bferenderer_camera_placement_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_placement_ptr() {
        const ret = wasm.bferenderer_camera_placement_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} role
     */
    camera_post_facts_frame(role) {
        wasm.bferenderer_camera_post_facts_frame(this.__wbg_ptr, role);
    }
    /**
     * @param {number} slot
     */
    camera_probe_frame(slot) {
        wasm.bferenderer_camera_probe_frame(this.__wbg_ptr, slot);
    }
    camera_project_frame() {
        wasm.bferenderer_camera_project_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} role
     */
    camera_projection_view_frame(role) {
        wasm.bferenderer_camera_projection_view_frame(this.__wbg_ptr, role);
    }
    /**
     * @returns {number}
     */
    camera_projection_view_len() {
        const ret = wasm.bferenderer_camera_projection_view_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_projection_view_ptr() {
        const ret = wasm.bferenderer_camera_projection_view_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    camera_reflect_frame() {
        wasm.bferenderer_camera_reflect_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} handle
     * @returns {boolean}
     */
    camera_release_frame(handle) {
        const ret = wasm.bferenderer_camera_release_frame(this.__wbg_ptr, handle);
        return ret !== 0;
    }
    camera_restore_frame() {
        wasm.bferenderer_camera_restore_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} root
     * @param {number} generation
     * @param {number} role
     * @param {number} source
     * @param {number} source_generation
     * @param {boolean} view
     * @param {number} now
     * @param {number} handle
     */
    camera_scene_lights_frame(root, generation, role, source, source_generation, view, now, handle) {
        wasm.bferenderer_camera_scene_lights_frame(this.__wbg_ptr, root, generation, role, source, source_generation, view, now, handle);
    }
    /**
     * @param {number} role
     * @param {boolean} follow
     */
    camera_scene_sky_frame(role, follow) {
        wasm.bferenderer_camera_scene_sky_frame(this.__wbg_ptr, role, follow);
    }
    camera_scene_sky_restore_frame() {
        wasm.bferenderer_camera_scene_sky_restore_frame(this.__wbg_ptr);
    }
    camera_scene_sky_save_frame() {
        wasm.bferenderer_camera_scene_sky_save_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} id
     * @param {number} generation
     * @param {number} role
     */
    camera_scene_uniform_frame(id, generation, role) {
        wasm.bferenderer_camera_scene_uniform_frame(this.__wbg_ptr, id, generation, role);
    }
    /**
     * @param {number} role
     */
    camera_scene_view_frame(role) {
        wasm.bferenderer_camera_scene_view_frame(this.__wbg_ptr, role);
    }
    /**
     * @param {number} role
     */
    camera_shader_frame(role) {
        wasm.bferenderer_camera_shader_frame(this.__wbg_ptr, role);
    }
    camera_shadow_configure_frame() {
        wasm.bferenderer_camera_shadow_configure_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} index
     */
    camera_shadow_frame(index) {
        wasm.bferenderer_camera_shadow_frame(this.__wbg_ptr, index);
    }
    camera_shadow_shader_frame() {
        wasm.bferenderer_camera_shadow_shader_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} role
     */
    camera_shift_frame(role) {
        wasm.bferenderer_camera_shift_frame(this.__wbg_ptr, role);
    }
    /**
     * @param {number} role
     * @param {boolean} follow
     */
    camera_sky_frame(role, follow) {
        wasm.bferenderer_camera_sky_frame(this.__wbg_ptr, role, follow);
    }
    /**
     * @returns {number}
     */
    camera_sky_len() {
        const ret = wasm.bferenderer_camera_sky_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {Uint32Array | number[]} nodes
     * @param {number} single
     * @param {number} generation
     */
    camera_sky_nodes_frame(nodes, single, generation) {
        const val2 = +single;
        const val3 = +generation;
        const ptr0 = passArray32ToWasm0(nodes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_camera_sky_nodes_frame(this.__wbg_ptr, ptr0, len0, val2, val3);
    }
    /**
     * @returns {number}
     */
    camera_sky_ptr() {
        const ret = wasm.bferenderer_camera_sky_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     */
    camera_static_shadow_frame(index) {
        wasm.bferenderer_camera_static_shadow_frame(this.__wbg_ptr, index);
    }
    /**
     * @param {boolean} uv
     */
    camera_uniform_frame(uv) {
        wasm.bferenderer_camera_uniform_frame(this.__wbg_ptr, uv);
    }
    /**
     * @returns {number}
     */
    camera_uniform_input_len() {
        const ret = wasm.bferenderer_camera_uniform_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_uniform_input_ptr() {
        const ret = wasm.bferenderer_camera_uniform_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_uniform_output_len() {
        const ret = wasm.bferenderer_camera_uniform_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_uniform_output_ptr() {
        const ret = wasm.bferenderer_camera_uniform_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} role
     */
    camera_unit_view_frame(role) {
        wasm.bferenderer_camera_unit_view_frame(this.__wbg_ptr, role);
    }
    /**
     * @returns {number}
     */
    camera_unit_view_len() {
        const ret = wasm.bferenderer_camera_unit_view_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_unit_view_ptr() {
        const ret = wasm.bferenderer_camera_unit_view_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_windows_len() {
        const ret = wasm.bferenderer_camera_windows_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    camera_windows_ptr() {
        const ret = wasm.bferenderer_camera_windows_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    canopies_input_len() {
        const ret = wasm.bferenderer_canopies_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    canopies_input_ptr() {
        const ret = wasm.bferenderer_canopies_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    canopies_leaves_len() {
        const ret = wasm.bferenderer_canopies_leaves_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    canopies_leaves_ptr() {
        const ret = wasm.bferenderer_canopies_leaves_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} max
     */
    canopies_mesh(max) {
        const ret = wasm.bferenderer_canopies_mesh(this.__wbg_ptr, max);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {number}
     */
    caster_input_len() {
        const ret = wasm.bferenderer_caster_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    caster_input_ptr() {
        const ret = wasm.bferenderer_caster_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {boolean} tier
     * @param {Float64Array | number[]} basins
     * @param {Float32Array | number[]} vertices
     * @param {Uint32Array | number[]} indices
     * @param {boolean} wide
     * @param {boolean} shadows
     * @param {Int32Array | number[]} samplers
     * @returns {Float64Array}
     */
    effect_basin(tier, basins, vertices, indices, wide, shadows, samplers) {
        const val5 = +tier;
        const val6 = +wide;
        const val7 = +shadows;
        let ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3;
        try {
            ptr0 = passArrayF64ToWasm0(basins, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(vertices, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArray32ToWasm0(indices, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
            ptr3 = passArray32ToWasm0(samplers, wasm.__wbindgen_malloc);
            len3 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr3) { wasm.__wbindgen_free(ptr3, len3 * 4, 4); }
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 8, 8); }
            throw e;
        }
        const ret = wasm.bferenderer_effect_basin(this.__wbg_ptr, val5, ptr0, len0, ptr1, len1, ptr2, len2, val6, val7, ptr3, len3);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v8 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v8;
    }
    /**
     * @param {number} kind
     * @param {number} variant
     * @param {boolean} shadows
     * @param {number} texture
     * @param {number} capacity
     * @param {number} owner
     * @param {Int32Array | number[]} samplers
     * @returns {number}
     */
    effect_batch(kind, variant, shadows, texture, capacity, owner, samplers) {
        const val2 = +kind;
        const val3 = +variant;
        const val4 = +shadows;
        const val5 = +texture;
        const val6 = +capacity;
        const val7 = +owner;
        const ptr0 = passArray32ToWasm0(samplers, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_effect_batch(this.__wbg_ptr, val2, val3, val4, val5, val6, val7, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} kind
     * @param {boolean} shadows
     * @param {number} slot
     * @param {number} vertex_at
     * @param {number} object
     * @returns {number}
     */
    effect_far_mesh(kind, shadows, slot, vertex_at, object) {
        const ret = wasm.bferenderer_effect_far_mesh(this.__wbg_ptr, kind, shadows, slot, vertex_at, object);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @returns {boolean}
     */
    effect_fixed(id) {
        const ret = wasm.bferenderer_effect_fixed(this.__wbg_ptr, id);
        return ret !== 0;
    }
    /**
     * @param {number} kind
     * @param {boolean} shadows
     * @param {number} texture
     * @param {number} vertices
     * @param {number} indices
     * @param {Float64Array | number[]} uv
     * @returns {number}
     */
    effect_mesh(kind, shadows, texture, vertices, indices, uv) {
        const val2 = +kind;
        const val3 = +shadows;
        const val4 = +texture;
        const val5 = +vertices;
        const val6 = +indices;
        const ptr0 = passArrayF64ToWasm0(uv, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_effect_mesh(this.__wbg_ptr, val2, val3, val4, val5, val6, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    effect_multiplier(id) {
        const ret = wasm.bferenderer_effect_multiplier(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @param {boolean} shadows
     */
    effect_settings(shadows) {
        const ret = wasm.bferenderer_effect_settings(this.__wbg_ptr, shadows);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} source
     * @param {number} kind
     * @param {boolean} far_view
     * @param {boolean} shadows
     * @param {number} slot
     * @param {number} object
     * @returns {Float64Array}
     */
    effect_water(source, kind, far_view, shadows, slot, object) {
        const ret = wasm.bferenderer_effect_water(this.__wbg_ptr, source, kind, far_view, shadows, slot, object);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * @param {number} item
     * @param {number} base
     */
    far_base(item, base) {
        wasm.bferenderer_far_base(this.__wbg_ptr, item, base);
    }
    /**
     * @param {number} item
     * @param {Float32Array | number[]} positions
     * @param {Uint32Array | number[]} index
     * @param {number} cell_size
     */
    far_cells(item, positions, index, cell_size) {
        const val3 = +item;
        const val4 = +cell_size;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArrayF32ToWasm0(positions, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(index, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        wasm.bferenderer_far_cells(this.__wbg_ptr, val3, ptr0, len0, ptr1, len1, val4);
    }
    /**
     * @returns {number}
     */
    far_input_len() {
        const ret = wasm.bferenderer_far_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    far_input_ptr() {
        const ret = wasm.bferenderer_far_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} item
     * @param {number} location
     * @param {number} columns
     * @param {number} rows
     * @param {number} max
     * @param {number} count
     * @param {number} at
     */
    far_instanced(item, location, columns, rows, max, count, at) {
        wasm.bferenderer_far_instanced(this.__wbg_ptr, item, location, columns, rows, max, count, at);
    }
    /**
     * @param {number} program
     * @param {number} table
     * @param {number} geometry
     * @param {number} flags
     * @param {number} depth_func
     * @param {Uint32Array | number[]} blend
     * @param {Float64Array | number[]} order
     * @param {Float64Array | number[]} sphere
     * @param {number} slot
     * @returns {number}
     */
    far_item(program, table, geometry, flags, depth_func, blend, order, sphere, slot) {
        const val4 = +program;
        const val5 = +table;
        const val6 = +geometry;
        const val7 = +flags;
        const val8 = +depth_func;
        const val9 = +slot;
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passArray32ToWasm0(blend, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF64ToWasm0(order, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArrayF64ToWasm0(sphere, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 8, 8); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 8, 8); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_far_item(this.__wbg_ptr, val4, val5, val6, val7, val8, ptr0, len0, ptr1, len1, ptr2, len2, val9);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    far_items() {
        const ret = wasm.bferenderer_far_items(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} item
     * @param {number} start
     * @param {number} count
     */
    far_range(item, start, count) {
        wasm.bferenderer_far_range(this.__wbg_ptr, item, start, count);
    }
    /**
     * @param {number} item
     * @param {number} at
     */
    far_range_input(item, at) {
        wasm.bferenderer_far_range_input(this.__wbg_ptr, item, at);
    }
    /**
     * @param {number} item
     * @param {number} buffer
     * @param {number} at
     * @param {number} floats
     * @param {number} flag
     */
    far_refresh(item, buffer, at, floats, flag) {
        wasm.bferenderer_far_refresh(this.__wbg_ptr, item, buffer, at, floats, flag);
    }
    /**
     * @param {number} item
     * @param {number} uniform
     * @param {number} kind
     * @param {Float32Array | number[]} values
     * @param {number} at
     */
    far_source(item, uniform, kind, values, at) {
        const val2 = +item;
        const val3 = +uniform;
        const val4 = +kind;
        const val5 = +at;
        const ptr0 = passArrayF32ToWasm0(values, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_far_source(this.__wbg_ptr, val2, val3, val4, ptr0, len0, val5);
    }
    /**
     * @param {number} program
     * @param {string} names
     * @param {Uint32Array | number[]} types
     * @param {Uint32Array | number[]} counts
     * @returns {number | undefined}
     */
    far_table(program, names, types, counts) {
        const val4 = +program;
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passStringToWasm0(names, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(types, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArray32ToWasm0(counts, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_far_table(this.__wbg_ptr, val4, ptr0, len0, ptr1, len1, ptr2, len2);
        return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
    }
    /**
     * @param {number} item
     * @param {number} level
     * @param {Uint32Array | number[]} index
     * @param {Float64Array | number[]} cells
     */
    far_terrain(item, level, index, cells) {
        const val3 = +item;
        const val4 = +level;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(index, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF64ToWasm0(cells, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 8, 8); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        wasm.bferenderer_far_terrain(this.__wbg_ptr, val3, val4, ptr0, len0, ptr1, len1);
    }
    /**
     * @returns {number}
     */
    far_vertices_len() {
        const ret = wasm.bferenderer_far_vertices_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    far_vertices_ptr() {
        const ret = wasm.bferenderer_far_vertices_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} texture
     * @param {boolean} grass
     * @param {boolean} shadows
     * @param {string} definition
     * @param {Float64Array | number[]} authored
     * @param {Float32Array | number[]} rows
     * @param {Float64Array | number[]} bounds
     * @param {Float32Array | number[]} map_matrix
     * @returns {number}
     */
    foliage_class(texture, grass, shadows, definition, authored, rows, bounds, map_matrix) {
        const val6 = +texture;
        const val7 = +grass;
        const val8 = +shadows;
        let ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, ptr4, len4;
        try {
            ptr0 = passStringToWasm0(definition, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF64ToWasm0(authored, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArrayF32ToWasm0(rows, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
            ptr3 = passArrayF64ToWasm0(bounds, wasm.__wbindgen_malloc);
            len3 = WASM_VECTOR_LEN;
            ptr4 = passArrayF32ToWasm0(map_matrix, wasm.__wbindgen_malloc);
            len4 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr4) { wasm.__wbindgen_free(ptr4, len4 * 4, 4); }
            if (ptr3) { wasm.__wbindgen_free(ptr3, len3 * 8, 8); }
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 8, 8); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_foliage_class(this.__wbg_ptr, val6, val7, val8, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, ptr4, len4);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    foliage_clear_frame() {
        wasm.bferenderer_foliage_clear_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    foliage_frame() {
        const ret = wasm.bferenderer_foliage_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    foliage_input_len() {
        const ret = wasm.bferenderer_foliage_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    foliage_input_ptr() {
        const ret = wasm.bferenderer_foliage_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} _class
     * @returns {number}
     */
    foliage_selected(_class) {
        const ret = wasm.bferenderer_foliage_selected(this.__wbg_ptr, _class);
        return ret >>> 0;
    }
    /**
     * @param {number} role
     */
    frame_admit_interface_frame(role) {
        wasm.bferenderer_frame_admit_interface_frame(this.__wbg_ptr, role);
    }
    /**
     * @param {GpuResources} resources
     * @param {number} role
     * @returns {boolean}
     */
    frame_aux_frame(resources, role) {
        _assertClass(resources, GpuResources);
        const ret = wasm.bferenderer_frame_aux_frame(this.__wbg_ptr, resources.__wbg_ptr, role);
        return ret !== 0;
    }
    frame_begin_frame() {
        wasm.bferenderer_frame_begin_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    frame_bindings_len() {
        const ret = wasm.bferenderer_frame_bindings_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_bindings_ptr() {
        const ret = wasm.bferenderer_frame_bindings_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    frame_cancel_frame() {
        wasm.bferenderer_frame_cancel_frame(this.__wbg_ptr);
    }
    /**
     * @returns {string | undefined}
     */
    frame_error_frame() {
        const ret = wasm.bferenderer_frame_error_frame(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]);
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * @returns {number}
     */
    frame_facts_len() {
        const ret = wasm.bferenderer_frame_facts_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_facts_ptr() {
        const ret = wasm.bferenderer_frame_facts_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_lines_len() {
        const ret = wasm.bferenderer_frame_lines_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_lines_ptr() {
        const ret = wasm.bferenderer_frame_lines_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} view
     * @param {number} count
     */
    frame_lines_publish_frame(view, count) {
        wasm.bferenderer_frame_lines_publish_frame(this.__wbg_ptr, view, count);
    }
    /**
     * @returns {number}
     */
    frame_meta_len() {
        const ret = wasm.bferenderer_frame_meta_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_meta_ptr() {
        const ret = wasm.bferenderer_frame_meta_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    frame_overflow_frame() {
        wasm.bferenderer_frame_overflow_frame(this.__wbg_ptr);
    }
    frame_plan_frame() {
        wasm.bferenderer_frame_plan_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} role
     * @returns {boolean}
     */
    frame_publish_frame(role) {
        const ret = wasm.bferenderer_frame_publish_frame(this.__wbg_ptr, role);
        return ret !== 0;
    }
    frame_reserve_frame() {
        wasm.bferenderer_frame_reserve_frame(this.__wbg_ptr);
    }
    frame_select_frame() {
        wasm.bferenderer_frame_select_frame(this.__wbg_ptr);
    }
    frame_shadow_reserve_frame() {
        wasm.bferenderer_frame_shadow_reserve_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    frame_state_len() {
        const ret = wasm.bferenderer_frame_state_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_state_ptr() {
        const ret = wasm.bferenderer_frame_state_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_stats_len() {
        const ret = wasm.bferenderer_frame_stats_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_stats_ptr() {
        const ret = wasm.bferenderer_frame_stats_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_trace_len() {
        const ret = wasm.bferenderer_frame_trace_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_trace_ptr() {
        const ret = wasm.bferenderer_frame_trace_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} role
     */
    frame_ui_slot_frame(role) {
        wasm.bferenderer_frame_ui_slot_frame(this.__wbg_ptr, role);
    }
    /**
     * @param {number} program
     * @param {number} texture
     * @param {Uint32Array | number[]} blend
     * @param {number} corner
     * @param {Uint32Array | number[]} attributes
     * @param {number} owner
     * @returns {number}
     */
    fx_add_batch(program, texture, blend, corner, attributes, owner) {
        const val3 = +program;
        const val4 = +texture;
        const val5 = +corner;
        const val6 = +owner;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(blend, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(attributes, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_fx_add_batch(this.__wbg_ptr, val3, val4, ptr0, len0, val5, ptr1, len1, val6);
        return ret >>> 0;
    }
    /**
     * @param {number} texture
     * @param {Uint32Array | number[]} blend
     * @param {number} depth_flags
     * @param {number} depth_func
     * @param {Uint32Array | number[]} sizes
     * @param {string} values
     * @returns {number}
     */
    fx_add_mesh(texture, blend, depth_flags, depth_func, sizes, values) {
        const val4 = +texture;
        const val5 = +depth_flags;
        const val6 = +depth_func;
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passArray32ToWasm0(blend, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(sizes, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passStringToWasm0(values, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 1, 1); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_fx_add_mesh(this.__wbg_ptr, val4, ptr0, len0, val5, val6, ptr1, len1, ptr2, len2);
        return ret >>> 0;
    }
    /**
     * @param {number} texture
     * @param {Uint32Array | number[]} blend
     * @param {number} depth_flags
     * @param {number} depth_func
     * @param {string} values
     * @param {Float64Array | number[]} sphere
     * @returns {number}
     */
    fx_add_model(texture, blend, depth_flags, depth_func, values, sphere) {
        const val4 = +texture;
        const val5 = +depth_flags;
        const val6 = +depth_func;
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passArray32ToWasm0(blend, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(values, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArrayF64ToWasm0(sphere, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 8, 8); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_fx_add_model(this.__wbg_ptr, val4, ptr0, len0, val5, val6, ptr1, len1, ptr2, len2);
        return ret >>> 0;
    }
    /**
     * @param {number} tracks
     * @param {number} track_rows
     * @returns {number}
     */
    fx_add_owner(tracks, track_rows) {
        const ret = wasm.bferenderer_fx_add_owner(this.__wbg_ptr, tracks, track_rows);
        return ret >>> 0;
    }
    /**
     * @param {number} batch
     * @param {Uint32Array | number[]} attributes
     * @param {number} vertices
     * @param {Float32Array | number[]} data
     * @param {Uint16Array | number[]} index
     */
    fx_batch_base(batch, attributes, vertices, data, index) {
        const val4 = +batch;
        const val5 = +vertices;
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passArray32ToWasm0(attributes, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(data, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArray16ToWasm0(index, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 2, 2); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        wasm.bferenderer_fx_batch_base(this.__wbg_ptr, val4, ptr0, len0, val5, ptr1, len1, ptr2, len2);
    }
    /**
     * @param {number} batch
     * @param {number} flags
     * @param {number} func
     */
    fx_batch_depth(batch, flags, func) {
        wasm.bferenderer_fx_batch_depth(this.__wbg_ptr, batch, flags, func);
    }
    /**
     * @param {number} batch
     */
    fx_batch_interleaved(batch) {
        wasm.bferenderer_fx_batch_interleaved(this.__wbg_ptr, batch);
    }
    /**
     * @param {number} batch
     * @param {string} name
     * @param {number} unit
     * @param {number} texture
     */
    fx_batch_sampler_3d(batch, name, unit, texture) {
        const val2 = +batch;
        const val3 = +unit;
        const val4 = +texture;
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_fx_batch_sampler_3d(this.__wbg_ptr, val2, ptr0, len0, val3, val4);
    }
    /**
     * @param {number} batch
     * @param {string} name
     * @param {number} unit
     * @param {number} texture
     */
    fx_batch_sampler(batch, name, unit, texture) {
        const val2 = +batch;
        const val3 = +unit;
        const val4 = +texture;
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_fx_batch_sampler(this.__wbg_ptr, val2, ptr0, len0, val3, val4);
    }
    /**
     * @param {number} batch
     * @param {number} side
     */
    fx_batch_side(batch, side) {
        wasm.bferenderer_fx_batch_side(this.__wbg_ptr, batch, side);
    }
    /**
     * @param {number} batch
     * @param {number} texture
     */
    fx_batch_texture(batch, texture) {
        wasm.bferenderer_fx_batch_texture(this.__wbg_ptr, batch, texture);
    }
    /**
     * @param {number} batch
     * @param {string} values
     */
    fx_batch_values(batch, values) {
        const val2 = +batch;
        const ptr0 = passStringToWasm0(values, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_fx_batch_values(this.__wbg_ptr, val2, ptr0, len0);
    }
    /**
     * @returns {number}
     */
    fx_clipped() {
        const ret = wasm.bferenderer_fx_clipped(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    fx_data_len() {
        const ret = wasm.bferenderer_fx_data_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    fx_data_ptr() {
        const ret = wasm.bferenderer_fx_data_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    fx_input_len() {
        const ret = wasm.bferenderer_fx_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    fx_input_ptr() {
        const ret = wasm.bferenderer_fx_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    fx_matrices_len() {
        const ret = wasm.bferenderer_fx_matrices_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    fx_matrices_ptr() {
        const ret = wasm.bferenderer_fx_matrices_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} mesh
     * @param {number} program
     * @param {number} side
     * @param {Uint32Array | number[]} locations
     * @returns {number}
     */
    fx_mesh_pass(mesh, program, side, locations) {
        const val2 = +mesh;
        const val3 = +program;
        const val4 = +side;
        const ptr0 = passArray32ToWasm0(locations, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_fx_mesh_pass(this.__wbg_ptr, val2, val3, val4, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * @param {number} mesh
     * @param {number} texture
     */
    fx_mesh_texture(mesh, texture) {
        wasm.bferenderer_fx_mesh_texture(this.__wbg_ptr, mesh, texture);
    }
    /**
     * @param {number} model
     * @param {number} program
     * @param {number} side
     * @param {number} geometry
     * @returns {number}
     */
    fx_model_pass(model, program, side, geometry) {
        const ret = wasm.bferenderer_fx_model_pass(this.__wbg_ptr, model, program, side, geometry);
        return ret >>> 0;
    }
    /**
     * @param {number} model
     * @param {string} name
     * @param {number} unit
     * @param {number} texture
     */
    fx_model_sampler(model, name, unit, texture) {
        const val2 = +model;
        const val3 = +unit;
        const val4 = +texture;
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_fx_model_sampler(this.__wbg_ptr, val2, ptr0, len0, val3, val4);
    }
    /**
     * @param {number} model
     * @param {number} texture
     */
    fx_model_texture(model, texture) {
        wasm.bferenderer_fx_model_texture(this.__wbg_ptr, model, texture);
    }
    /**
     * @param {number} floats
     */
    fx_reserve(floats) {
        wasm.bferenderer_fx_reserve(this.__wbg_ptr, floats);
    }
    /**
     * @param {number} g
     * @param {Uint8Array | number[]} data
     * @returns {number}
     */
    geometry_buffer(g, data) {
        const val2 = +g;
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_geometry_buffer(this.__wbg_ptr, val2, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * @param {number} g
     * @param {number} slot
     * @param {Uint8Array | number[]} data
     */
    geometry_buffer_data(g, slot, data) {
        const val2 = +g;
        const val3 = +slot;
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_geometry_buffer_data(this.__wbg_ptr, val2, val3, ptr0, len0);
    }
    /**
     * @param {number} g
     * @param {number} slot
     * @param {number} offset
     * @param {Uint8Array | number[]} data
     */
    geometry_buffer_sub_data(g, slot, offset, data) {
        const val2 = +g;
        const val3 = +slot;
        const val4 = +offset;
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_geometry_buffer_sub_data(this.__wbg_ptr, val2, val3, val4, ptr0, len0);
    }
    /**
     * @param {number} g
     * @param {number} start
     * @param {number} count
     */
    geometry_group(g, start, count) {
        wasm.bferenderer_geometry_group(this.__wbg_ptr, g, start, count);
    }
    /**
     * @param {number} g
     * @param {Uint8Array | number[]} data
     * @param {number} ty
     */
    geometry_index(g, data, ty) {
        const val2 = +g;
        const val3 = +ty;
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_geometry_index(this.__wbg_ptr, val2, ptr0, len0, val3);
    }
    /**
     * @param {number} g
     * @param {Uint8Array | number[]} data
     */
    geometry_index_data(g, data) {
        const val2 = +g;
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_geometry_index_data(this.__wbg_ptr, val2, ptr0, len0);
    }
    /**
     * @param {number} g
     * @param {number} offset
     * @param {Uint8Array | number[]} data
     */
    geometry_index_sub_data(g, offset, data) {
        const val2 = +g;
        const val3 = +offset;
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_geometry_index_sub_data(this.__wbg_ptr, val2, val3, ptr0, len0);
    }
    /**
     * @param {number} g
     * @param {number} buffer
     * @param {number} location
     * @param {number} size
     * @param {number} ty
     * @param {boolean} normalized
     * @param {number} stride
     * @param {number} offset
     */
    geometry_pointer(g, buffer, location, size, ty, normalized, stride, offset) {
        wasm.bferenderer_geometry_pointer(this.__wbg_ptr, g, buffer, location, size, ty, normalized, stride, offset);
    }
    /**
     * @param {number} g
     * @param {number} mode
     * @param {number} first
     * @param {boolean} indexed
     * @param {number} width
     */
    geometry_primitive(g, mode, first, indexed, width) {
        wasm.bferenderer_geometry_primitive(this.__wbg_ptr, g, mode, first, indexed, width);
    }
    /**
     * @param {number} map
     * @param {number} mask
     * @param {boolean} masked
     * @param {number} map_sample
     * @param {number} mask_sample
     * @returns {number | undefined}
     */
    hud_appearance(map, mask, masked, map_sample, mask_sample) {
        const ret = wasm.bferenderer_hud_appearance(this.__wbg_ptr, map, mask, masked, map_sample, mask_sample);
        return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
    }
    hud_descriptor_init() {
        const ret = wasm.bferenderer_hud_descriptor_init(this.__wbg_ptr);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {Uint32Array | number[]} index
     */
    hud_init(index) {
        const ptr0 = passArray32ToWasm0(index, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_hud_init(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @returns {number}
     */
    hud_input_len() {
        const ret = wasm.bferenderer_hud_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    hud_input_ptr() {
        const ret = wasm.bferenderer_hud_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    hud_mask_uv_len() {
        const ret = wasm.bferenderer_hud_mask_uv_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    hud_mask_uv_ptr() {
        const ret = wasm.bferenderer_hud_mask_uv_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} texture
     * @param {number} width
     * @param {number} height
     * @param {boolean} ready
     */
    hud_model_asset(id, texture, width, height, ready) {
        wasm.bferenderer_hud_model_asset(this.__wbg_ptr, id, texture, width, height, ready);
    }
    /**
     * @param {number} mode
     * @param {boolean} full
     * @returns {number}
     */
    hud_model_build(mode, full) {
        const ret = wasm.bferenderer_hud_model_build(this.__wbg_ptr, mode, full);
        return ret >>> 0;
    }
    /**
     * @param {number} kind
     * @param {number} a
     * @param {number} b
     * @param {number} c
     * @param {number} d
     */
    hud_model_event(kind, a, b, c, d) {
        wasm.bferenderer_hud_model_event(this.__wbg_ptr, kind, a, b, c, d);
    }
    /**
     * @param {number} font
     * @param {number} height
     * @param {Float64Array | number[]} rows
     */
    hud_model_font(font, height, rows) {
        const val2 = +font;
        const val3 = +height;
        const ptr0 = passArrayF64ToWasm0(rows, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_hud_model_font(this.__wbg_ptr, val2, val3, ptr0, len0);
    }
    /**
     * @returns {number}
     */
    hud_model_input_len() {
        const ret = wasm.bferenderer_hud_model_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    hud_model_input_ptr() {
        const ret = wasm.bferenderer_hud_model_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} kind
     * @param {number} a
     * @param {number} b
     * @param {number} c
     * @param {number} d
     */
    hud_model_observe(kind, a, b, c, d) {
        wasm.bferenderer_hud_model_observe(this.__wbg_ptr, kind, a, b, c, d);
    }
    /**
     * @param {string} text
     * @param {boolean} uppercase
     * @returns {number}
     */
    hud_model_text(text, uppercase) {
        const val2 = +uppercase;
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_hud_model_text(this.__wbg_ptr, ptr0, len0, val2);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    hud_model_update() {
        const ret = wasm.bferenderer_hud_model_update(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    hud_position_len() {
        const ret = wasm.bferenderer_hud_position_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    hud_position_ptr() {
        const ret = wasm.bferenderer_hud_position_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {GpuResources} resources
     * @param {number} mode
     * @param {boolean} full
     */
    hud_resources_frame(resources, mode, full) {
        _assertClass(resources, GpuResources);
        const ret = wasm.bferenderer_hud_resources_frame(this.__wbg_ptr, resources.__wbg_ptr, mode, full);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {number}
     */
    hud_tint_len() {
        const ret = wasm.bferenderer_hud_tint_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    hud_tint_ptr() {
        const ret = wasm.bferenderer_hud_tint_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    hud_uv_len() {
        const ret = wasm.bferenderer_hud_uv_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    hud_uv_ptr() {
        const ret = wasm.bferenderer_hud_uv_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.bferenderer_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.bferenderer_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {Uint32Array | number[]} blend
     * @param {number} depth_flags
     * @param {number} depth_func
     * @param {Uint32Array | number[]} passes
     * @returns {number}
     */
    inst_add_blend(blend, depth_flags, depth_func, passes) {
        const val3 = +depth_flags;
        const val4 = +depth_func;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(blend, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(passes, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_inst_add_blend(this.__wbg_ptr, ptr0, len0, val3, val4, ptr1, len1);
        return ret >>> 0;
    }
    /**
     * @param {number} geometry
     * @param {number} buffer
     * @param {Uint8Array | number[]} data
     * @returns {number}
     */
    interface_buffer_init(geometry, buffer, data) {
        const val2 = +geometry;
        const val3 = +buffer;
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_interface_buffer_init(this.__wbg_ptr, val2, val3, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * @param {number} stream
     * @returns {number}
     */
    interface_buffer_len(stream) {
        const ret = wasm.bferenderer_interface_buffer_len(this.__wbg_ptr, stream);
        return ret >>> 0;
    }
    /**
     * @param {number} stream
     * @returns {number}
     */
    interface_buffer_ptr(stream) {
        const ret = wasm.bferenderer_interface_buffer_ptr(this.__wbg_ptr, stream);
        return ret >>> 0;
    }
    /**
     * @param {number} draws
     */
    interface_frame(draws) {
        wasm.bferenderer_interface_frame(this.__wbg_ptr, draws);
    }
    /**
     * @returns {number}
     */
    interface_input_len() {
        const ret = wasm.bferenderer_interface_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    interface_input_ptr() {
        const ret = wasm.bferenderer_interface_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} stream
     * @param {number} offset
     * @param {number} length
     */
    interface_upload_frame(stream, offset, length) {
        wasm.bferenderer_interface_upload_frame(this.__wbg_ptr, stream, offset, length);
    }
    /**
     * @returns {number}
     */
    line_clipped() {
        const ret = wasm.bferenderer_line_clipped(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    line_data_len() {
        const ret = wasm.bferenderer_line_data_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    line_data_ptr() {
        const ret = wasm.bferenderer_line_data_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} view
     */
    line_frame(view) {
        wasm.bferenderer_line_frame(this.__wbg_ptr, view);
    }
    /**
     * @returns {number}
     */
    line_input_len() {
        const ret = wasm.bferenderer_line_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    line_input_ptr() {
        const ret = wasm.bferenderer_line_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} program
     * @param {number} kind
     * @param {number} capacity
     * @returns {number | undefined}
     */
    line_source(program, kind, capacity) {
        const ret = wasm.bferenderer_line_source(this.__wbg_ptr, program, kind, capacity);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] === Number.MAX_SAFE_INTEGER ? undefined : ret[0];
    }
    /**
     * @returns {number}
     */
    motion_bones_len() {
        const ret = wasm.bferenderer_motion_bones_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_bones_ptr() {
        const ret = wasm.bferenderer_motion_bones_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} role
     * @param {number} view
     */
    motion_camera_frame(role, view) {
        wasm.bferenderer_motion_camera_frame(this.__wbg_ptr, role, view);
    }
    /**
     * @param {number} program
     * @param {number} geometry
     * @param {number} max
     * @param {Uint32Array | number[]} layout
     * @returns {number}
     */
    motion_extra(program, geometry, max, layout) {
        const val2 = +program;
        const val3 = +geometry;
        const val4 = +max;
        const ptr0 = passArray32ToWasm0(layout, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_motion_extra(this.__wbg_ptr, val2, val3, val4, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_extra_values_len() {
        const ret = wasm.bferenderer_motion_extra_values_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_extra_values_ptr() {
        const ret = wasm.bferenderer_motion_extra_values_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_extras_len() {
        const ret = wasm.bferenderer_motion_extras_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_extras_ptr() {
        const ret = wasm.bferenderer_motion_extras_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    motion_history_commit_frame() {
        wasm.bferenderer_motion_history_commit_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    motion_history_current_len() {
        const ret = wasm.bferenderer_motion_history_current_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_history_current_ptr() {
        const ret = wasm.bferenderer_motion_history_current_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_history_flyer_frame() {
        const ret = wasm.bferenderer_motion_history_flyer_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {boolean} stepped
     * @returns {number}
     */
    motion_history_flyer_sample_frame(id, stepped) {
        const ret = wasm.bferenderer_motion_history_flyer_sample_frame(this.__wbg_ptr, id, stepped);
        return ret >>> 0;
    }
    /**
     * @param {number} skin
     * @param {Float64Array | number[]} bind
     * @returns {number}
     */
    motion_history_model_frame(skin, bind) {
        const val2 = +skin;
        const ptr0 = passArrayF64ToWasm0(bind, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_motion_history_model_frame(this.__wbg_ptr, val2, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_history_poses_len() {
        const ret = wasm.bferenderer_motion_history_poses_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_history_poses_ptr() {
        const ret = wasm.bferenderer_motion_history_poses_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    motion_history_prepare_frame(count) {
        wasm.bferenderer_motion_history_prepare_frame(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    motion_history_records_len() {
        const ret = wasm.bferenderer_motion_history_records_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_history_records_ptr() {
        const ret = wasm.bferenderer_motion_history_records_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     */
    motion_history_release_frame(id) {
        wasm.bferenderer_motion_history_release_frame(this.__wbg_ptr, id);
    }
    /**
     * @param {number} id
     */
    motion_history_reset_frame(id) {
        wasm.bferenderer_motion_history_reset_frame(this.__wbg_ptr, id);
    }
    /**
     * @param {number} id
     * @param {number} skin
     */
    motion_history_reskin_frame(id, skin) {
        wasm.bferenderer_motion_history_reskin_frame(this.__wbg_ptr, id, skin);
    }
    /**
     * @returns {number}
     */
    motion_history_selected_len() {
        const ret = wasm.bferenderer_motion_history_selected_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_history_selected_ptr() {
        const ret = wasm.bferenderer_motion_history_selected_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_history_sequence() {
        const ret = wasm.bferenderer_motion_history_sequence(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     * @returns {number}
     */
    motion_history_skin_frame(count) {
        const ret = wasm.bferenderer_motion_history_skin_frame(this.__wbg_ptr, count);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    motion_history_skin_start(id) {
        const ret = wasm.bferenderer_motion_history_skin_start(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    motion_history_write_frame(count) {
        wasm.bferenderer_motion_history_write_frame(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    motion_input_len() {
        const ret = wasm.bferenderer_motion_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_input_ptr() {
        const ret = wasm.bferenderer_motion_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_previous_len() {
        const ret = wasm.bferenderer_motion_previous_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_previous_ptr() {
        const ret = wasm.bferenderer_motion_previous_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_record_stride() {
        const ret = wasm.bferenderer_motion_record_stride(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    motion_records_at() {
        const ret = wasm.bferenderer_motion_records_at(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {WebGL2RenderingContext} context
     */
    constructor(context) {
        const ret = wasm.bferenderer_new(context);
        this.__wbg_ptr = ret;
        BfeRendererFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    post_chain_begin_frame() {
        wasm.bferenderer_post_chain_begin_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    post_chain_input_len() {
        const ret = wasm.bferenderer_post_chain_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    post_chain_input_ptr() {
        const ret = wasm.bferenderer_post_chain_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    post_chain_plan_frame() {
        const ret = wasm.bferenderer_post_chain_plan_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    post_chain_state_len() {
        const ret = wasm.bferenderer_post_chain_state_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    post_chain_state_ptr() {
        const ret = wasm.bferenderer_post_chain_state_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    post_input_len() {
        const ret = wasm.bferenderer_post_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    post_input_ptr() {
        const ret = wasm.bferenderer_post_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    post_states_len() {
        const ret = wasm.bferenderer_post_states_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    post_states_ptr() {
        const ret = wasm.bferenderer_post_states_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} texture
     * @returns {number}
     */
    post_target(texture) {
        const ret = wasm.bferenderer_post_target(this.__wbg_ptr, texture);
        return ret >>> 0;
    }
    /**
     * @param {number} family
     * @param {number} cascades
     * @param {boolean} lamps
     * @param {boolean} shadows
     */
    prepare_begin_frame(family, cascades, lamps, shadows) {
        wasm.bferenderer_prepare_begin_frame(this.__wbg_ptr, family, cascades, lamps, shadows);
    }
    /**
     * @param {number} family
     * @param {number} cascades
     * @param {boolean} lamps
     * @param {boolean} shadows
     * @returns {string}
     */
    prepare_keys_frame(family, cascades, lamps, shadows) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.bferenderer_prepare_keys_frame(this.__wbg_ptr, family, cascades, lamps, shadows);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {string}
     */
    prepare_next_frame() {
        let deferred2_0;
        let deferred2_1;
        try {
            const ret = wasm.bferenderer_prepare_next_frame(this.__wbg_ptr);
            var ptr1 = ret[0];
            var len1 = ret[1];
            if (ret[3]) {
                ptr1 = 0; len1 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * @param {WebGL2RenderingContext} context
     * @param {GpuResources} resources
     * @param {Uint32Array | number[]} textures
     */
    rebuild(context, resources, textures) {
        _assertClass(resources, GpuResources);
        const ptr0 = passArray32ToWasm0(textures, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_rebuild(this.__wbg_ptr, context, resources.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {number}
     */
    record_stride() {
        const ret = wasm.bferenderer_record_stride(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    records_at() {
        const ret = wasm.bferenderer_records_at(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    recovery_commands() {
        const ret = wasm.bferenderer_recovery_commands(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {string}
     */
    renderer_diagnostics_frame() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.bferenderer_renderer_diagnostics_frame(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {number} id
     * @param {GpuResources} resources
     * @param {number} source
     */
    replace_sample_texture(id, resources, source) {
        _assertClass(resources, GpuResources);
        const ret = wasm.bferenderer_replace_sample_texture(this.__wbg_ptr, id, resources.__wbg_ptr, source);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} program
     * @param {number} mode
     * @param {number} count
     * @param {number} index_type
     * @param {number} offset
     * @param {number} instances
     * @param {boolean} indexed
     */
    replay_program_draw(program, mode, count, index_type, offset, instances, indexed) {
        const ret = wasm.bferenderer_replay_program_draw(this.__wbg_ptr, program, mode, count, index_type, offset, instances, indexed);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {string}
     */
    resource_diagnostics() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.bferenderer_resource_diagnostics(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    scene_actor_depth_sources_frame() {
        wasm.bferenderer_scene_actor_depth_sources_frame(this.__wbg_ptr);
    }
    /**
     * @param {number} skin
     * @param {number} id
     * @param {number} generation
     * @param {number} serial
     */
    scene_actor_skin_frame(skin, id, generation, serial) {
        wasm.bferenderer_scene_actor_skin_frame(this.__wbg_ptr, skin, id, generation, serial);
    }
    /**
     * @returns {number}
     */
    scene_bounds_updates() {
        const ret = wasm.bferenderer_scene_bounds_updates(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_bounds_len() {
        const ret = wasm.bferenderer_scene_canopy_bounds_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_bounds_ptr() {
        const ret = wasm.bferenderer_scene_canopy_bounds_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_output_len() {
        const ret = wasm.bferenderer_scene_canopy_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_output_ptr() {
        const ret = wasm.bferenderer_scene_canopy_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     * @param {number} generation
     */
    scene_canopy_reserve(count, generation) {
        wasm.bferenderer_scene_canopy_reserve(this.__wbg_ptr, count, generation);
    }
    /**
     * @returns {number}
     */
    scene_canopy_select() {
        const ret = wasm.bferenderer_scene_canopy_select(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_states_len() {
        const ret = wasm.bferenderer_scene_canopy_states_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_states_ptr() {
        const ret = wasm.bferenderer_scene_canopy_states_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_view_len() {
        const ret = wasm.bferenderer_scene_canopy_view_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_view_ptr() {
        const ret = wasm.bferenderer_scene_canopy_view_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_changes_len() {
        const ret = wasm.bferenderer_scene_changes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_changes_ptr() {
        const ret = wasm.bferenderer_scene_changes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_clipped() {
        const ret = wasm.bferenderer_scene_clipped(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_drawable_visits() {
        const ret = wasm.bferenderer_scene_drawable_visits(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_facts_len() {
        const ret = wasm.bferenderer_scene_facts_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_facts_ptr() {
        const ret = wasm.bferenderer_scene_facts_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_groups_len() {
        const ret = wasm.bferenderer_scene_groups_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_groups_ptr() {
        const ret = wasm.bferenderer_scene_groups_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    scene_groups_reserve(count) {
        wasm.bferenderer_scene_groups_reserve(this.__wbg_ptr, count);
    }
    /**
     * @param {number} count
     * @returns {number}
     */
    scene_instances_frame(count) {
        const ret = wasm.bferenderer_scene_instances_frame(this.__wbg_ptr, count);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_lines_len() {
        const ret = wasm.bferenderer_scene_lines_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_lines_ptr() {
        const ret = wasm.bferenderer_scene_lines_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} generation
     * @returns {boolean}
     */
    scene_mirrored_frame(id, generation) {
        const ret = wasm.bferenderer_scene_mirrored_frame(this.__wbg_ptr, id, generation);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    scene_motion_events_len() {
        const ret = wasm.bferenderer_scene_motion_events_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_motion_events_ptr() {
        const ret = wasm.bferenderer_scene_motion_events_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} record
     * @param {number} id
     * @param {number} generation
     */
    scene_motion_locked_frame(record, id, generation) {
        wasm.bferenderer_scene_motion_locked_frame(this.__wbg_ptr, record, id, generation);
    }
    /**
     * @param {number} skin
     * @param {number} id
     * @param {number} generation
     * @returns {number}
     */
    scene_motion_model_frame(skin, id, generation) {
        const ret = wasm.bferenderer_scene_motion_model_frame(this.__wbg_ptr, skin, id, generation);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} slot
     * @param {number} generation
     * @returns {boolean}
     */
    scene_motion_source_frame(id, slot, generation) {
        const ret = wasm.bferenderer_scene_motion_source_frame(this.__wbg_ptr, id, slot, generation);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    scene_order_len() {
        const ret = wasm.bferenderer_scene_order_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_order_ptr() {
        const ret = wasm.bferenderer_scene_order_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_output_len() {
        const ret = wasm.bferenderer_scene_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_output_ptr() {
        const ret = wasm.bferenderer_scene_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} root
     * @param {number} generation
     * @param {number} owner
     * @param {number} maximum
     * @returns {number}
     */
    scene_place_frame(root, generation, owner, maximum) {
        const ret = wasm.bferenderer_scene_place_frame(this.__wbg_ptr, root, generation, owner, maximum);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_place_output_len() {
        const ret = wasm.bferenderer_scene_place_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_place_output_ptr() {
        const ret = wasm.bferenderer_scene_place_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_point_changes_len() {
        const ret = wasm.bferenderer_scene_point_changes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_point_changes_ptr() {
        const ret = wasm.bferenderer_scene_point_changes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} generation
     */
    scene_point_root_frame(id, generation) {
        wasm.bferenderer_scene_point_root_frame(this.__wbg_ptr, id, generation);
    }
    /**
     * @param {number} count
     */
    scene_points_apply_frame(count) {
        wasm.bferenderer_scene_points_apply_frame(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    scene_pose_frame() {
        const ret = wasm.bferenderer_scene_pose_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_pose_len() {
        const ret = wasm.bferenderer_scene_pose_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_pose_ptr() {
        const ret = wasm.bferenderer_scene_pose_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} owner
     * @param {number} owner_generation
     * @param {number} id
     * @param {number} generation
     * @param {number} node_start
     * @param {number} node_count
     * @param {number} skin_start
     * @param {number} skin_count
     * @returns {boolean}
     */
    scene_pose_register(owner, owner_generation, id, generation, node_start, node_count, skin_start, skin_count) {
        const ret = wasm.bferenderer_scene_pose_register(this.__wbg_ptr, owner, owner_generation, id, generation, node_start, node_count, skin_start, skin_count);
        return ret !== 0;
    }
    /**
     * @param {number} owner
     * @param {number} id
     * @param {number} generation
     * @param {number} skin
     * @returns {number}
     */
    scene_pose_skin_offset(owner, id, generation, skin) {
        const ret = wasm.bferenderer_scene_pose_skin_offset(this.__wbg_ptr, owner, id, generation, skin);
        return ret >>> 0;
    }
    /**
     * @param {number} owner
     * @param {number} id
     * @param {number} generation
     * @param {number} skin
     * @param {number} node
     * @param {number} bind
     * @param {Uint32Array | number[]} bones
     * @param {Float64Array | number[]} inverses
     * @returns {boolean}
     */
    scene_pose_skin_register(owner, id, generation, skin, node, bind, bones, inverses) {
        const val3 = +owner;
        const val4 = +id;
        const val5 = +generation;
        const val6 = +skin;
        const val7 = +node;
        const val8 = +bind;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(bones, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF64ToWasm0(inverses, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 8, 8); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_scene_pose_skin_register(this.__wbg_ptr, val3, val4, val5, val6, val7, val8, ptr0, len0, ptr1, len1);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    scene_pose_skins_len() {
        const ret = wasm.bferenderer_scene_pose_skins_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_pose_skins_ptr() {
        const ret = wasm.bferenderer_scene_pose_skins_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_queries() {
        const ret = wasm.bferenderer_scene_queries(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_rejected() {
        const ret = wasm.bferenderer_scene_rejected(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    scene_reserve(count) {
        wasm.bferenderer_scene_reserve(this.__wbg_ptr, count);
    }
    /**
     * @param {number} x
     * @param {number} y
     * @param {number} z
     * @param {number} w
     * @param {number} order
     */
    scene_rotation_frame(x, y, z, w, order) {
        wasm.bferenderer_scene_rotation_frame(this.__wbg_ptr, x, y, z, w, order);
    }
    /**
     * @returns {number}
     */
    scene_rotation_len() {
        const ret = wasm.bferenderer_scene_rotation_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_rotation_ptr() {
        const ret = wasm.bferenderer_scene_rotation_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} changes
     * @param {number} groups
     * @param {number} worlds
     * @returns {number}
     */
    scene_select(changes, groups, worlds) {
        const ret = wasm.bferenderer_scene_select(this.__wbg_ptr, changes, groups, worlds);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} generation
     * @param {Uint32Array | number[]} bones
     * @param {Uint32Array | number[]} generations
     * @param {Float64Array | number[]} inverses
     * @param {Uint32Array | number[]} controls
     * @returns {boolean}
     */
    scene_skeleton_register(id, generation, bones, generations, inverses, controls) {
        const val5 = +id;
        const val6 = +generation;
        let ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3;
        try {
            ptr0 = passArray32ToWasm0(bones, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(generations, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArrayF64ToWasm0(inverses, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
            ptr3 = passArray32ToWasm0(controls, wasm.__wbindgen_malloc);
            len3 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr3) { wasm.__wbindgen_free(ptr3, len3 * 4, 4); }
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 8, 8); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_scene_skeleton_register(this.__wbg_ptr, val5, val6, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3);
        return ret !== 0;
    }
    /**
     * @param {number} id
     * @param {number} generation
     * @param {number} count
     * @param {number} words
     */
    scene_skin_admit_frame(id, generation, count, words) {
        wasm.bferenderer_scene_skin_admit_frame(this.__wbg_ptr, id, generation, count, words);
    }
    /**
     * @returns {number}
     */
    scene_skin_admit_len() {
        const ret = wasm.bferenderer_scene_skin_admit_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_skin_admit_ptr() {
        const ret = wasm.bferenderer_scene_skin_admit_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {boolean} advance
     * @returns {number}
     */
    scene_stamp_frame(advance) {
        const ret = wasm.bferenderer_scene_stamp_frame(this.__wbg_ptr, advance);
        return ret >>> 0;
    }
    /**
     * @param {number} stamp
     */
    scene_stamp_set_frame(stamp) {
        wasm.bferenderer_scene_stamp_set_frame(this.__wbg_ptr, stamp);
    }
    /**
     * @param {number} changes
     * @param {number} groups
     * @param {number} worlds
     * @param {number} transforms
     */
    scene_transform_apply_frame(changes, groups, worlds, transforms) {
        wasm.bferenderer_scene_transform_apply_frame(this.__wbg_ptr, changes, groups, worlds, transforms);
    }
    /**
     * @returns {number}
     */
    scene_transform_changes_len() {
        const ret = wasm.bferenderer_scene_transform_changes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_changes_ptr() {
        const ret = wasm.bferenderer_scene_transform_changes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_frame() {
        const ret = wasm.bferenderer_scene_transform_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_phase_len() {
        const ret = wasm.bferenderer_scene_transform_phase_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_phase_ptr() {
        const ret = wasm.bferenderer_scene_transform_phase_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_updates() {
        const ret = wasm.bferenderer_scene_transform_updates(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_values_len() {
        const ret = wasm.bferenderer_scene_transform_values_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_values_ptr() {
        const ret = wasm.bferenderer_scene_transform_values_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_visits() {
        const ret = wasm.bferenderer_scene_transform_visits(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_frame() {
        const ret = wasm.bferenderer_scene_tree_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_info_len() {
        const ret = wasm.bferenderer_scene_tree_info_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_info_ptr() {
        const ret = wasm.bferenderer_scene_tree_info_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_input_len() {
        const ret = wasm.bferenderer_scene_tree_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_input_ptr() {
        const ret = wasm.bferenderer_scene_tree_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} words
     */
    scene_tree_reserve(words) {
        wasm.bferenderer_scene_tree_reserve(this.__wbg_ptr, words);
    }
    /**
     * @returns {number}
     */
    scene_view_len() {
        const ret = wasm.bferenderer_scene_view_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_view_ptr() {
        const ret = wasm.bferenderer_scene_view_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_visibility_builds() {
        const ret = wasm.bferenderer_scene_visibility_builds(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_visibility_visits() {
        const ret = wasm.bferenderer_scene_visibility_visits(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_visible_len() {
        const ret = wasm.bferenderer_scene_visible_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_visible_ptr() {
        const ret = wasm.bferenderer_scene_visible_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_world_changes_len() {
        const ret = wasm.bferenderer_scene_world_changes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_world_changes_ptr() {
        const ret = wasm.bferenderer_scene_world_changes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} texture
     */
    set_far_map(texture) {
        wasm.bferenderer_set_far_map(this.__wbg_ptr, texture);
    }
    /**
     * @param {Float32Array | number[]} data
     */
    set_lamps(data) {
        const ptr0 = passArrayF32ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_set_lamps(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @param {Uint32Array | number[]} ids
     */
    set_shadow_maps(ids) {
        const ptr0 = passArray32ToWasm0(ids, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_set_shadow_maps(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @returns {number}
     */
    shadow_input_len() {
        const ret = wasm.bferenderer_shadow_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    shadow_input_ptr() {
        const ret = wasm.bferenderer_shadow_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {boolean} by_program
     */
    sort(by_program) {
        wasm.bferenderer_sort(this.__wbg_ptr, by_program);
    }
    /**
     * @param {number} source
     * @param {number} texture
     * @returns {number}
     */
    static_depth_add(source, texture) {
        const ret = wasm.bferenderer_static_depth_add(this.__wbg_ptr, source, texture);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {number}
     */
    static_depth_clipped() {
        const ret = wasm.bferenderer_static_depth_clipped(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {string}
     */
    static_depth_finish() {
        let deferred2_0;
        let deferred2_1;
        try {
            const ret = wasm.bferenderer_static_depth_finish(this.__wbg_ptr);
            var ptr1 = ret[0];
            var len1 = ret[1];
            if (ret[3]) {
                ptr1 = 0; len1 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * @param {number} id
     * @param {number} source
     * @param {number} texture
     * @returns {number}
     */
    static_depth_replace(id, source, texture) {
        const ret = wasm.bferenderer_static_depth_replace(this.__wbg_ptr, id, source, texture);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {number}
     */
    static_shadow_input_len() {
        const ret = wasm.bferenderer_static_shadow_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    static_shadow_input_ptr() {
        const ret = wasm.bferenderer_static_shadow_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} authored
     * @param {Float32Array | number[]} vertices
     * @param {Uint32Array | number[]} index
     * @returns {number}
     */
    static_source(authored, vertices, index) {
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(vertices, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArray32ToWasm0(index, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_static_source(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @param {Uint8Array | number[]} weights
     */
    static_source_weights(id, weights) {
        const val2 = +id;
        const ptr0 = passArray8ToWasm0(weights, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_static_source_weights(this.__wbg_ptr, val2, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} source
     * @param {Uint8Array | number[]} weights
     * @returns {number}
     */
    static_terrain_layers(source, weights) {
        const val2 = +source;
        const ptr0 = passArray8ToWasm0(weights, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_static_terrain_layers(this.__wbg_ptr, val2, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * @param {number} source
     * @param {boolean} far_view
     * @param {number} low_res_scale
     * @param {Int32Array | number[]} samplers
     * @param {Uint32Array | number[]} features
     * @param {number} order
     * @param {number} slot
     * @param {number} object
     * @returns {number}
     */
    static_world(source, far_view, low_res_scale, samplers, features, order, slot, object) {
        const val3 = +source;
        const val4 = +far_view;
        const val5 = +low_res_scale;
        const val6 = +order;
        const val7 = +slot;
        const val8 = +object;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(samplers, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(features, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_static_world(this.__wbg_ptr, val3, val4, val5, ptr0, len0, ptr1, len1, val6, val7, val8);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {Uint32Array | number[]} near
     * @param {Uint32Array | number[]} far
     */
    static_world_settings(near, far) {
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(near, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(far, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_static_world_settings(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} geometry
     * @param {Uint32Array | number[]} blend
     * @param {number} depth_func
     * @param {number} flags
     * @param {boolean} point_lit
     * @param {Float64Array | number[]} bounds
     * @returns {number}
     */
    transparent_bucket(geometry, blend, depth_func, flags, point_lit, bounds) {
        const val3 = +geometry;
        const val4 = +depth_func;
        const val5 = +flags;
        const val6 = +point_lit;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(blend, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF64ToWasm0(bounds, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 8, 8); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_transparent_bucket(this.__wbg_ptr, val3, ptr0, len0, val4, val5, val6, ptr1, len1);
        return ret >>> 0;
    }
    /**
     * @param {number} b
     * @param {number} program
     * @param {number} side
     * @returns {number}
     */
    transparent_bucket_pass(b, program, side) {
        const ret = wasm.bferenderer_transparent_bucket_pass(this.__wbg_ptr, b, program, side);
        return ret >>> 0;
    }
    /**
     * @param {number} b
     * @param {number} unit
     * @param {number} texture
     */
    transparent_bucket_texture(b, unit, texture) {
        wasm.bferenderer_transparent_bucket_texture(this.__wbg_ptr, b, unit, texture);
    }
    /**
     * @param {number} b
     * @param {number} pass
     * @param {string} name
     * @param {Float32Array | number[]} values
     * @param {boolean} int
     * @returns {boolean}
     */
    transparent_bucket_uniform(b, pass, name, values, int) {
        const val3 = +b;
        const val4 = +pass;
        const val5 = +int;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(values, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_transparent_bucket_uniform(this.__wbg_ptr, val3, val4, ptr0, len0, ptr1, len1, val5);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    transparent_keys_len() {
        const ret = wasm.bferenderer_transparent_keys_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    transparent_keys_ptr() {
        const ret = wasm.bferenderer_transparent_keys_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} kind
     * @param {number} geometry
     * @param {number} start
     * @param {number} count
     * @param {Uint32Array | number[]} blend
     * @param {number} depth_func
     * @param {number} flags
     * @returns {number}
     */
    transparent_object(kind, geometry, start, count, blend, depth_func, flags) {
        const val2 = +kind;
        const val3 = +geometry;
        const val4 = +start;
        const val5 = +count;
        const val6 = +depth_func;
        const val7 = +flags;
        const ptr0 = passArray32ToWasm0(blend, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_transparent_object(this.__wbg_ptr, val2, val3, val4, val5, ptr0, len0, val6, val7);
        return ret >>> 0;
    }
    /**
     * @param {number} o
     * @param {number} program
     * @param {number} side
     */
    transparent_object_pass(o, program, side) {
        wasm.bferenderer_transparent_object_pass(this.__wbg_ptr, o, program, side);
    }
    /**
     * @returns {number}
     */
    transparent_object_stride() {
        const ret = wasm.bferenderer_transparent_object_stride(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} o
     * @param {number} texture
     */
    transparent_object_texture(o, texture) {
        wasm.bferenderer_transparent_object_texture(this.__wbg_ptr, o, texture);
    }
    /**
     * @returns {number}
     */
    transparent_objects_len() {
        const ret = wasm.bferenderer_transparent_objects_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    transparent_objects_ptr() {
        const ret = wasm.bferenderer_transparent_objects_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    transparent_segments_len() {
        const ret = wasm.bferenderer_transparent_segments_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    transparent_segments_ptr() {
        const ret = wasm.bferenderer_transparent_segments_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} source
     * @param {string} authored
     * @param {boolean} skin
     * @param {Int32Array | number[]} samplers
     * @param {Int32Array | number[]} public_samples
     * @param {number} order
     * @returns {Float64Array}
     */
    ui_appearance(source, authored, skin, samplers, public_samples, order) {
        const val4 = +source;
        const val5 = +skin;
        const val6 = +order;
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(samplers, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArray32ToWasm0(public_samples, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_ui_appearance(this.__wbg_ptr, val4, ptr0, len0, val5, ptr1, len1, ptr2, len2, val6);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v7 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v7;
    }
    /**
     * @returns {number}
     */
    ui_dirty_len() {
        const ret = wasm.bferenderer_ui_dirty_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    ui_dirty_ptr() {
        const ret = wasm.bferenderer_ui_dirty_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {GpuResources} resources
     */
    ui_resources_frame(resources) {
        _assertClass(resources, GpuResources);
        const ret = wasm.bferenderer_ui_resources_frame(this.__wbg_ptr, resources.__wbg_ptr);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    ui_stream_len(id) {
        const ret = wasm.bferenderer_ui_stream_len(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    ui_stream_ptr(id) {
        const ret = wasm.bferenderer_ui_stream_ptr(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    view_input_len() {
        const ret = wasm.bferenderer_view_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    view_input_ptr() {
        const ret = wasm.bferenderer_view_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    water_input_len() {
        const ret = wasm.bferenderer_water_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    water_input_ptr() {
        const ret = wasm.bferenderer_water_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} surface
     * @param {number} set
     * @param {number} k
     * @param {number} location
     */
    water_instanced(surface, set, k, location) {
        wasm.bferenderer_water_instanced(this.__wbg_ptr, surface, set, k, location);
    }
    /**
     * @returns {number}
     */
    water_instances_len() {
        const ret = wasm.bferenderer_water_instances_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    water_instances_ptr() {
        const ret = wasm.bferenderer_water_instances_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {Uint32Array | number[]} index
     * @param {Uint32Array | number[]} blend
     * @param {number} depth_func
     * @param {number} flags
     * @param {number} side
     * @returns {number}
     */
    water_rings(index, blend, depth_func, flags, side) {
        const val3 = +depth_func;
        const val4 = +flags;
        const val5 = +side;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(index, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(blend, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bferenderer_water_rings(this.__wbg_ptr, ptr0, len0, ptr1, len1, val3, val4, val5);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    water_rings_alpha_len() {
        const ret = wasm.bferenderer_water_rings_alpha_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    water_rings_alpha_ptr() {
        const ret = wasm.bferenderer_water_rings_alpha_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    water_rings_position_len() {
        const ret = wasm.bferenderer_water_rings_position_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    water_rings_position_ptr() {
        const ret = wasm.bferenderer_water_rings_position_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    water_rings_uv_len() {
        const ret = wasm.bferenderer_water_rings_uv_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    water_rings_uv_ptr() {
        const ret = wasm.bferenderer_water_rings_uv_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} max
     * @param {Uint32Array | number[]} sizes
     * @returns {number | undefined}
     */
    water_set(max, sizes) {
        const val2 = +max;
        const ptr0 = passArray32ToWasm0(sizes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_water_set(this.__wbg_ptr, val2, ptr0, len0);
        return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
    }
    /**
     * @param {string} authored
     * @param {boolean} ocean
     * @returns {Float64Array}
     */
    water_source(authored, ocean) {
        const val2 = +ocean;
        const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_water_source(this.__wbg_ptr, ptr0, len0, val2);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v3 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v3;
    }
    /**
     * @param {number} geometry
     * @param {Uint32Array | number[]} blend
     * @param {number} depth_func
     * @param {number} flags
     * @param {number} side
     * @returns {number}
     */
    water_surface(geometry, blend, depth_func, flags, side) {
        const val2 = +geometry;
        const val3 = +depth_func;
        const val4 = +flags;
        const val5 = +side;
        const ptr0 = passArray32ToWasm0(blend, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.bferenderer_water_surface(this.__wbg_ptr, val2, ptr0, len0, val3, val4, val5);
        return ret >>> 0;
    }
    /**
     * @param {number} source
     * @param {number} kind
     * @returns {Float64Array}
     */
    water_values(source, kind) {
        const ret = wasm.bferenderer_water_values(this.__wbg_ptr, source, kind);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * @param {number} w
     * @param {Float32Array | number[]} positions
     * @param {Uint32Array | number[]} index
     * @param {number} cell_size
     */
    world_cells(w, positions, index, cell_size) {
        const val3 = +w;
        const val4 = +cell_size;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArrayF32ToWasm0(positions, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(index, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        wasm.bferenderer_world_cells(this.__wbg_ptr, val3, ptr0, len0, ptr1, len1, val4);
    }
    /**
     * @returns {number}
     */
    world_input_len() {
        const ret = wasm.bferenderer_world_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    world_input_ptr() {
        const ret = wasm.bferenderer_world_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} w
     * @param {Float32Array | number[]} values
     */
    world_lamps(w, values) {
        const val2 = +w;
        const ptr0 = passArrayF32ToWasm0(values, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.bferenderer_world_lamps(this.__wbg_ptr, val2, ptr0, len0);
    }
    /**
     * @param {number} w
     * @param {Uint32Array | number[]} ranges
     * @param {Float64Array | number[]} boxes
     */
    world_runs(w, ranges, boxes) {
        const val3 = +w;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(ranges, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF64ToWasm0(boxes, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 8, 8); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        wasm.bferenderer_world_runs(this.__wbg_ptr, val3, ptr0, len0, ptr1, len1);
    }
    /**
     * @param {number} w
     * @param {boolean} shown
     */
    world_shown(w, shown) {
        wasm.bferenderer_world_shown(this.__wbg_ptr, w, shown);
    }
    /**
     * @param {number} w
     * @param {number} unit
     * @param {number} texture
     */
    world_texture(w, unit, texture) {
        wasm.bferenderer_world_texture(this.__wbg_ptr, w, unit, texture);
    }
    /**
     * @param {number} w
     * @param {string} name
     * @param {Float32Array | number[]} values
     * @param {boolean} int
     * @returns {boolean}
     */
    world_uniform(w, name, values, int) {
        const val3 = +w;
        const val4 = +int;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(values, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.bferenderer_world_uniform(this.__wbg_ptr, val3, ptr0, len0, ptr1, len1, val4);
        return ret !== 0;
    }
    /**
     * @param {number} id
     * @returns {boolean}
     */
    world_uses_lamps(id) {
        const ret = wasm.bferenderer_world_uses_lamps(this.__wbg_ptr, id);
        return ret !== 0;
    }
}
if (Symbol.dispose) BfeRenderer.prototype[Symbol.dispose] = BfeRenderer.prototype.free;

export class BfeScene {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BfeSceneFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bfescene_free(ptr, 0);
    }
    constructor() {
        const ret = wasm.bfescene_new();
        this.__wbg_ptr = ret;
        BfeSceneFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    scene_canopy_bounds_len() {
        const ret = wasm.bfescene_scene_canopy_bounds_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_bounds_ptr() {
        const ret = wasm.bfescene_scene_canopy_bounds_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_output_len() {
        const ret = wasm.bfescene_scene_canopy_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_output_ptr() {
        const ret = wasm.bfescene_scene_canopy_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     * @param {number} generation
     */
    scene_canopy_reserve(count, generation) {
        wasm.bfescene_scene_canopy_reserve(this.__wbg_ptr, count, generation);
    }
    /**
     * @returns {number}
     */
    scene_canopy_select() {
        const ret = wasm.bfescene_scene_canopy_select(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_states_len() {
        const ret = wasm.bfescene_scene_canopy_states_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_states_ptr() {
        const ret = wasm.bfescene_scene_canopy_states_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_view_len() {
        const ret = wasm.bfescene_scene_canopy_view_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_canopy_view_ptr() {
        const ret = wasm.bfescene_scene_canopy_view_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_changes_len() {
        const ret = wasm.bfescene_scene_changes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_changes_ptr() {
        const ret = wasm.bfescene_scene_changes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} generation
     * @param {Float32Array} out
     * @param {number} count
     * @param {Float64Array | number[]} visibility
     * @returns {boolean}
     */
    scene_copy_skin_frame(id, generation, out, count, visibility) {
        const val3 = +id;
        const val4 = +generation;
        const val5 = +count;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArrayF32ToWasm0(out, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF64ToWasm0(visibility, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 8, 8); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bfescene_scene_copy_skin_frame(this.__wbg_ptr, val3, val4, ptr0, len0, out, val5, ptr1, len1);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    scene_facts_len() {
        const ret = wasm.bfescene_scene_facts_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_facts_ptr() {
        const ret = wasm.bfescene_scene_facts_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_groups_len() {
        const ret = wasm.bfescene_scene_groups_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_groups_ptr() {
        const ret = wasm.bfescene_scene_groups_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    scene_groups_reserve(count) {
        wasm.bfescene_scene_groups_reserve(this.__wbg_ptr, count);
    }
    /**
     * @param {Float64Array} input
     * @param {Float32Array} bones
     * @param {number} count
     * @param {number} base
     * @returns {number}
     */
    scene_instances_frame(input, bones, count, base) {
        const val3 = +count;
        const val4 = +base;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArrayF64ToWasm0(input, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(bones, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 8, 8); }
            throw e;
        }
        const ret = wasm.bfescene_scene_instances_frame(this.__wbg_ptr, ptr0, len0, input, ptr1, len1, bones, val3, val4);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_lines_len() {
        const ret = wasm.bfescene_scene_lines_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_lines_ptr() {
        const ret = wasm.bfescene_scene_lines_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_motion_events_len() {
        const ret = wasm.bfescene_scene_motion_events_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_motion_events_ptr() {
        const ret = wasm.bfescene_scene_motion_events_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} record
     * @param {number} id
     * @param {number} generation
     * @param {Float64Array} input
     */
    scene_motion_locked_frame(record, id, generation, input) {
        const val2 = +record;
        const val3 = +id;
        const val4 = +generation;
        var ptr0 = passArrayF64ToWasm0(input, wasm.__wbindgen_malloc);
        var len0 = WASM_VECTOR_LEN;
        wasm.bfescene_scene_motion_locked_frame(this.__wbg_ptr, val2, val3, val4, ptr0, len0, input);
    }
    /**
     * @param {BfePoseHistory} history
     * @param {number} skin
     * @param {number} id
     * @param {number} generation
     * @returns {number}
     */
    scene_motion_model_frame(history, skin, id, generation) {
        _assertClass(history, BfePoseHistory);
        const ret = wasm.bfescene_scene_motion_model_frame(this.__wbg_ptr, history.__wbg_ptr, skin, id, generation);
        return ret >>> 0;
    }
    /**
     * @param {BfePoseHistory} history
     * @param {number} id
     * @param {number} slot
     * @param {number} generation
     * @returns {boolean}
     */
    scene_motion_source_frame(history, id, slot, generation) {
        _assertClass(history, BfePoseHistory);
        const ret = wasm.bfescene_scene_motion_source_frame(this.__wbg_ptr, history.__wbg_ptr, id, slot, generation);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    scene_order_len() {
        const ret = wasm.bfescene_scene_order_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_order_ptr() {
        const ret = wasm.bfescene_scene_order_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_output_len() {
        const ret = wasm.bfescene_scene_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_output_ptr() {
        const ret = wasm.bfescene_scene_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} root
     * @param {number} generation
     * @param {number} owner
     * @param {number} maximum
     * @returns {number}
     */
    scene_place_frame(root, generation, owner, maximum) {
        const ret = wasm.bfescene_scene_place_frame(this.__wbg_ptr, root, generation, owner, maximum);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_place_output_len() {
        const ret = wasm.bfescene_scene_place_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_place_output_ptr() {
        const ret = wasm.bfescene_scene_place_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_point_changes_len() {
        const ret = wasm.bfescene_scene_point_changes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_point_changes_ptr() {
        const ret = wasm.bfescene_scene_point_changes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} generation
     */
    scene_point_root_frame(id, generation) {
        wasm.bfescene_scene_point_root_frame(this.__wbg_ptr, id, generation);
    }
    /**
     * @param {number} count
     */
    scene_points_apply_frame(count) {
        wasm.bfescene_scene_points_apply_frame(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    scene_pose_frame() {
        const ret = wasm.bfescene_scene_pose_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_pose_len() {
        const ret = wasm.bfescene_scene_pose_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_pose_ptr() {
        const ret = wasm.bfescene_scene_pose_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} owner
     * @param {number} owner_generation
     * @param {number} id
     * @param {number} generation
     * @param {number} node_start
     * @param {number} node_count
     * @param {number} skin_start
     * @param {number} skin_count
     * @returns {boolean}
     */
    scene_pose_register(owner, owner_generation, id, generation, node_start, node_count, skin_start, skin_count) {
        const ret = wasm.bfescene_scene_pose_register(this.__wbg_ptr, owner, owner_generation, id, generation, node_start, node_count, skin_start, skin_count);
        return ret !== 0;
    }
    /**
     * @param {number} owner
     * @param {number} id
     * @param {number} generation
     * @param {number} skin
     * @returns {number}
     */
    scene_pose_skin_offset(owner, id, generation, skin) {
        const ret = wasm.bfescene_scene_pose_skin_offset(this.__wbg_ptr, owner, id, generation, skin);
        return ret >>> 0;
    }
    /**
     * @param {number} owner
     * @param {number} id
     * @param {number} generation
     * @param {number} skin
     * @param {number} node
     * @param {number} bind
     * @param {Uint32Array | number[]} bones
     * @param {Float64Array | number[]} inverses
     * @returns {boolean}
     */
    scene_pose_skin_register(owner, id, generation, skin, node, bind, bones, inverses) {
        const val3 = +owner;
        const val4 = +id;
        const val5 = +generation;
        const val6 = +skin;
        const val7 = +node;
        const val8 = +bind;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray32ToWasm0(bones, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF64ToWasm0(inverses, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 8, 8); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bfescene_scene_pose_skin_register(this.__wbg_ptr, val3, val4, val5, val6, val7, val8, ptr0, len0, ptr1, len1);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    scene_pose_skins_len() {
        const ret = wasm.bfescene_scene_pose_skins_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_pose_skins_ptr() {
        const ret = wasm.bfescene_scene_pose_skins_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_queries() {
        const ret = wasm.bfescene_scene_queries(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    scene_reserve(count) {
        wasm.bfescene_scene_reserve(this.__wbg_ptr, count);
    }
    /**
     * @param {number} x
     * @param {number} y
     * @param {number} z
     * @param {number} w
     * @param {number} order
     */
    scene_rotation_frame(x, y, z, w, order) {
        wasm.bfescene_scene_rotation_frame(this.__wbg_ptr, x, y, z, w, order);
    }
    /**
     * @returns {number}
     */
    scene_rotation_len() {
        const ret = wasm.bfescene_scene_rotation_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_rotation_ptr() {
        const ret = wasm.bfescene_scene_rotation_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} changes
     * @param {number} groups
     * @param {number} worlds
     * @returns {number}
     */
    scene_select(changes, groups, worlds) {
        const ret = wasm.bfescene_scene_select(this.__wbg_ptr, changes, groups, worlds);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} generation
     * @param {Uint32Array | number[]} bones
     * @param {Uint32Array | number[]} generations
     * @param {Float64Array | number[]} inverses
     * @param {Uint32Array | number[]} controls
     * @returns {boolean}
     */
    scene_skeleton_register(id, generation, bones, generations, inverses, controls) {
        const val5 = +id;
        const val6 = +generation;
        let ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3;
        try {
            ptr0 = passArray32ToWasm0(bones, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArray32ToWasm0(generations, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArrayF64ToWasm0(inverses, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
            ptr3 = passArray32ToWasm0(controls, wasm.__wbindgen_malloc);
            len3 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr3) { wasm.__wbindgen_free(ptr3, len3 * 4, 4); }
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 8, 8); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 4, 4); }
            throw e;
        }
        const ret = wasm.bfescene_scene_skeleton_register(this.__wbg_ptr, val5, val6, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3);
        return ret !== 0;
    }
    /**
     * @param {number} id
     * @param {number} generation
     * @param {number} count
     * @param {number} words
     */
    scene_skin_admit_frame(id, generation, count, words) {
        wasm.bfescene_scene_skin_admit_frame(this.__wbg_ptr, id, generation, count, words);
    }
    /**
     * @returns {number}
     */
    scene_skin_admit_len() {
        const ret = wasm.bfescene_scene_skin_admit_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_skin_admit_ptr() {
        const ret = wasm.bfescene_scene_skin_admit_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} changes
     * @param {number} groups
     * @param {number} worlds
     * @param {number} transforms
     */
    scene_transform_apply_frame(changes, groups, worlds, transforms) {
        wasm.bfescene_scene_transform_apply_frame(this.__wbg_ptr, changes, groups, worlds, transforms);
    }
    /**
     * @returns {number}
     */
    scene_transform_changes_len() {
        const ret = wasm.bfescene_scene_transform_changes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_changes_ptr() {
        const ret = wasm.bfescene_scene_transform_changes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_frame() {
        const ret = wasm.bfescene_scene_transform_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_phase_len() {
        const ret = wasm.bfescene_scene_transform_phase_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_phase_ptr() {
        const ret = wasm.bfescene_scene_transform_phase_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_updates() {
        const ret = wasm.bfescene_scene_transform_updates(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_values_len() {
        const ret = wasm.bfescene_scene_transform_values_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_values_ptr() {
        const ret = wasm.bfescene_scene_transform_values_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_transform_visits() {
        const ret = wasm.bfescene_scene_transform_visits(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_frame() {
        const ret = wasm.bfescene_scene_tree_frame(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_info_len() {
        const ret = wasm.bfescene_scene_tree_info_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_info_ptr() {
        const ret = wasm.bfescene_scene_tree_info_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_input_len() {
        const ret = wasm.bfescene_scene_tree_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_tree_input_ptr() {
        const ret = wasm.bfescene_scene_tree_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} words
     */
    scene_tree_reserve(words) {
        wasm.bfescene_scene_tree_reserve(this.__wbg_ptr, words);
    }
    /**
     * @param {number} id
     * @param {number} generation
     * @param {Float64Array} input
     * @returns {boolean}
     */
    scene_uniform_world_frame(id, generation, input) {
        const val2 = +id;
        const val3 = +generation;
        var ptr0 = passArrayF64ToWasm0(input, wasm.__wbindgen_malloc);
        var len0 = WASM_VECTOR_LEN;
        const ret = wasm.bfescene_scene_uniform_world_frame(this.__wbg_ptr, val2, val3, ptr0, len0, input);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    scene_view_len() {
        const ret = wasm.bfescene_scene_view_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_view_ptr() {
        const ret = wasm.bfescene_scene_view_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_visible_len() {
        const ret = wasm.bfescene_scene_visible_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_visible_ptr() {
        const ret = wasm.bfescene_scene_visible_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_world_changes_len() {
        const ret = wasm.bfescene_scene_world_changes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    scene_world_changes_ptr() {
        const ret = wasm.bfescene_scene_world_changes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) BfeScene.prototype[Symbol.dispose] = BfeScene.prototype.free;

export class BfeUniformTransforms {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BfeUniformTransformsFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_bfeuniformtransforms_free(ptr, 0);
    }
    /**
     * @param {boolean} uv
     */
    frame(uv) {
        wasm.bfeuniformtransforms_frame(this.__wbg_ptr, uv);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.bfeuniformtransforms_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.bfeuniformtransforms_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {boolean}
     */
    mirrored() {
        const ret = wasm.bfeuniformtransforms_mirrored(this.__wbg_ptr);
        return ret !== 0;
    }
    constructor() {
        const ret = wasm.bfeuniformtransforms_new();
        this.__wbg_ptr = ret;
        BfeUniformTransformsFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    output_len() {
        const ret = wasm.bfeuniformtransforms_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    output_ptr() {
        const ret = wasm.bfeuniformtransforms_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) BfeUniformTransforms.prototype[Symbol.dispose] = BfeUniformTransforms.prototype.free;

export class GpuResources {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        GpuResourcesFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_gpuresources_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    actor_gradient() {
        const ret = wasm.gpuresources_actor_gradient(this.__wbg_ptr);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} face
     * @param {number} level
     * @param {number} x
     * @param {number} y
     * @param {number} width
     * @param {number} height
     * @param {number} sx
     * @param {number} sy
     * @param {number} sw
     * @param {number} sh
     * @param {boolean} clipped
     */
    bind(id, face, level, x, y, width, height, sx, sy, sw, sh, clipped) {
        const ret = wasm.gpuresources_bind(this.__wbg_ptr, id, face, level, x, y, width, height, sx, sy, sw, sh, clipped);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} width
     * @param {object} pixels
     * @returns {number}
     */
    canopy_state(width, pixels) {
        const ret = wasm.gpuresources_canopy_state(this.__wbg_ptr, width, pixels);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} r
     * @param {number} g
     * @param {number} b
     * @param {number} alpha
     * @param {boolean} premultiply
     * @param {number} mask
     */
    clear(r, g, b, alpha, premultiply, mask) {
        wasm.gpuresources_clear(this.__wbg_ptr, r, g, b, alpha, premultiply, mask);
    }
    /**
     * @param {number} source
     * @param {number} destination
     * @param {number} mask
     */
    copy(source, destination, mask) {
        const ret = wasm.gpuresources_copy(this.__wbg_ptr, source, destination, mask);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     * @returns {WebGLTexture}
     */
    descriptor_texture(id) {
        const ret = wasm.gpuresources_descriptor_texture(this.__wbg_ptr, id);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return takeFromExternrefTable0(ret[0]);
    }
    /**
     * @param {number} id
     */
    dispose_effect_target(id) {
        const ret = wasm.gpuresources_dispose_effect_target(this.__wbg_ptr, id);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     */
    dispose_sample(id) {
        const ret = wasm.gpuresources_dispose_sample(this.__wbg_ptr, id);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     */
    dispose_target(id) {
        wasm.gpuresources_dispose_target(this.__wbg_ptr, id);
    }
    /**
     * @param {number} kind
     * @param {number} count
     * @param {object} pixels
     * @returns {number}
     */
    effect_data(kind, count, pixels) {
        const ret = wasm.gpuresources_effect_data(this.__wbg_ptr, kind, count, pixels);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} kind
     * @returns {number}
     */
    effect_noise(kind) {
        const ret = wasm.gpuresources_effect_noise(this.__wbg_ptr, kind);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} sample
     * @param {object} pixels
     */
    effect_pixels(sample, pixels) {
        const ret = wasm.gpuresources_effect_pixels(this.__wbg_ptr, sample, pixels);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} kind
     * @returns {number}
     */
    effect_target(kind) {
        const ret = wasm.gpuresources_effect_target(this.__wbg_ptr, kind);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @returns {Int32Array}
     */
    effect_target_facts(id) {
        const ret = wasm.gpuresources_effect_target_facts(this.__wbg_ptr, id);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @param {number} id
     * @param {number} width
     * @param {number} height
     * @returns {number}
     */
    effect_target_size(id, width, height) {
        const ret = wasm.gpuresources_effect_target_size(this.__wbg_ptr, id, width, height);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {number}
     */
    epoch() {
        const ret = wasm.gpuresources_epoch(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    fountain_atlas() {
        const ret = wasm.gpuresources_fountain_atlas(this.__wbg_ptr);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @param {boolean} multisample
     * @param {number} face
     * @returns {WebGLFramebuffer}
     */
    framebuffer(id, multisample, face) {
        const ret = wasm.gpuresources_framebuffer(this.__wbg_ptr, id, multisample, face);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return takeFromExternrefTable0(ret[0]);
    }
    /**
     * @returns {string}
     */
    memory() {
        let deferred2_0;
        let deferred2_1;
        try {
            const ret = wasm.gpuresources_memory(this.__wbg_ptr);
            var ptr1 = ret[0];
            var len1 = ret[1];
            if (ret[3]) {
                ptr1 = 0; len1 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * @param {number} sample
     */
    mipmap(sample) {
        const ret = wasm.gpuresources_mipmap(this.__wbg_ptr, sample);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {WebGL2RenderingContext} gl
     */
    constructor(gl) {
        const ret = wasm.gpuresources_new(gl);
        this.__wbg_ptr = ret;
        GpuResourcesFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @param {number} id
     */
    particle_top_level(id) {
        const ret = wasm.gpuresources_particle_top_level(this.__wbg_ptr, id);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} size
     * @returns {number}
     */
    probe_target(size) {
        const ret = wasm.gpuresources_probe_target(this.__wbg_ptr, size);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} x
     * @param {number} y
     * @param {number} width
     * @param {number} height
     * @param {object} data
     * @param {number} face
     */
    read(id, x, y, width, height, data, face) {
        const ret = wasm.gpuresources_read(this.__wbg_ptr, id, x, y, width, height, data, face);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    rebuild() {
        const ret = wasm.gpuresources_rebuild(this.__wbg_ptr);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     */
    release_skin_image(id) {
        const ret = wasm.gpuresources_release_skin_image(this.__wbg_ptr, id);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     */
    release_source(id) {
        wasm.gpuresources_release_source(this.__wbg_ptr, id);
    }
    /**
     * @param {number} source
     * @returns {number}
     */
    repeat_sample(source) {
        const ret = wasm.gpuresources_repeat_sample(this.__wbg_ptr, source);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @param {string} descriptor
     */
    resize_target(id, descriptor) {
        const val2 = +id;
        const ptr0 = passStringToWasm0(descriptor, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.gpuresources_resize_target(this.__wbg_ptr, val2, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     */
    resolve(id) {
        const ret = wasm.gpuresources_resolve(this.__wbg_ptr, id);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    retain_sample(id) {
        const ret = wasm.gpuresources_retain_sample(this.__wbg_ptr, id);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {number}
     */
    retired_sample() {
        const ret = wasm.gpuresources_retired_sample(this.__wbg_ptr);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} source
     * @param {string} settings
     * @returns {number}
     */
    sample(source, settings) {
        const val2 = +source;
        const ptr0 = passStringToWasm0(settings, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.gpuresources_sample(this.__wbg_ptr, val2, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @param {string} settings
     */
    sampling(id, settings) {
        const val2 = +id;
        const ptr0 = passStringToWasm0(settings, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.gpuresources_sampling(this.__wbg_ptr, val2, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} bones
     * @param {object} pixels
     * @returns {number}
     */
    skin_image(bones, pixels) {
        const ret = wasm.gpuresources_skin_image(this.__wbg_ptr, bones, pixels);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} flakes
     * @param {number} boxes
     * @param {number} heights
     * @param {boolean} cover
     */
    snow_pixels(flakes, boxes, heights, cover) {
        const ret = wasm.gpuresources_snow_pixels(this.__wbg_ptr, flakes, boxes, heights, cover);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {string} desc
     * @returns {number}
     */
    source(desc) {
        const ptr0 = passStringToWasm0(desc, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.gpuresources_source(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @param {Uint8Array | number[]} data
     * @param {string} metadata
     */
    source_blocks(id, data, metadata) {
        const val3 = +id;
        let ptr0, len0, ptr1, len1;
        try {
            ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passStringToWasm0(metadata, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len1 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.gpuresources_source_blocks(this.__wbg_ptr, val3, ptr0, len0, ptr1, len1);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     * @param {HTMLCanvasElement} canvas
     */
    source_canvas(id, canvas) {
        const ret = wasm.gpuresources_source_canvas(this.__wbg_ptr, id, canvas);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     * @param {string} desc
     */
    source_desc(id, desc) {
        const val2 = +id;
        const ptr0 = passStringToWasm0(desc, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.gpuresources_source_desc(this.__wbg_ptr, val2, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     * @returns {Uint8Array}
     */
    source_encoded(id) {
        const ret = wasm.gpuresources_source_encoded(this.__wbg_ptr, id);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @param {number} id
     * @param {object} pixels
     */
    source_pixels(id, pixels) {
        const ret = wasm.gpuresources_source_pixels(this.__wbg_ptr, id, pixels);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     * @param {Uint8Array} encoded
     * @param {object} pixels
     */
    source_png(id, encoded, pixels) {
        const ret = wasm.gpuresources_source_png(this.__wbg_ptr, id, encoded, pixels);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} value
     */
    static_anisotropy(value) {
        const ret = wasm.gpuresources_static_anisotropy(this.__wbg_ptr, value);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    suspend() {
        wasm.gpuresources_suspend(this.__wbg_ptr);
    }
    /**
     * @param {string} descriptor
     * @returns {number}
     */
    target(descriptor) {
        const ptr0 = passStringToWasm0(descriptor, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.gpuresources_target(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @returns {WebGLTexture}
     */
    texture(id) {
        const ret = wasm.gpuresources_texture(this.__wbg_ptr, id);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return takeFromExternrefTable0(ret[0]);
    }
    /**
     * @param {HTMLCanvasElement} canvas
     * @returns {number}
     */
    ui_canvas(canvas) {
        const ret = wasm.gpuresources_ui_canvas(this.__wbg_ptr, canvas);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} kind
     * @returns {number}
     */
    ui_image(kind) {
        const ret = wasm.gpuresources_ui_image(this.__wbg_ptr, kind);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} width
     * @param {number} height
     * @param {Uint8Array} encoded
     * @param {object} pixels
     */
    ui_png(id, width, height, encoded, pixels) {
        const ret = wasm.gpuresources_ui_png(this.__wbg_ptr, id, width, height, encoded, pixels);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     */
    ui_repeat(id) {
        const ret = wasm.gpuresources_ui_repeat(this.__wbg_ptr, id);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} size
     * @returns {number}
     */
    ui_target(size) {
        const ret = wasm.gpuresources_ui_target(this.__wbg_ptr, size);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} sample
     * @param {object} pixels
     */
    update_canopy_state(sample, pixels) {
        const ret = wasm.gpuresources_update_canopy_state(this.__wbg_ptr, sample, pixels);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {number} id
     */
    upload(id) {
        const ret = wasm.gpuresources_upload(this.__wbg_ptr, id);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    upload_fence() {
        const ret = wasm.gpuresources_upload_fence(this.__wbg_ptr);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {boolean}
     */
    upload_ready() {
        const ret = wasm.gpuresources_upload_ready(this.__wbg_ptr);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] !== 0;
    }
    /**
     * @param {boolean} normal
     * @returns {number}
     */
    water_fallback(normal) {
        const ret = wasm.gpuresources_water_fallback(this.__wbg_ptr, normal);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {string} authored
     * @returns {number}
     */
    water_mask(authored) {
        const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.gpuresources_water_mask(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} sample
     * @param {number} size
     * @param {object} pixels
     */
    weather_cover(sample, size, pixels) {
        const ret = wasm.gpuresources_weather_cover(this.__wbg_ptr, sample, size, pixels);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @param {boolean} neutral
     * @returns {number}
     */
    world_fallback(neutral) {
        const ret = wasm.gpuresources_world_fallback(this.__wbg_ptr, neutral);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
}
if (Symbol.dispose) GpuResources.prototype[Symbol.dispose] = GpuResources.prototype.free;

/**
 * @param {string} authored
 * @param {boolean} skin
 * @param {number} cascades
 * @param {boolean} lamps
 * @returns {string}
 */
export function actor_appearance_description(authored, skin, cascades, lamps) {
    let deferred6_0;
    let deferred6_1;
    try {
        const val2 = +skin;
        const val3 = +cascades;
        const val4 = +lamps;
        const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.actor_appearance_description(ptr0, len0, val2, val3, val4);
        var ptr5 = ret[0];
        var len5 = ret[1];
        if (ret[3]) {
            ptr5 = 0; len5 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred6_0 = ptr5;
        deferred6_1 = len5;
        return getStringFromWasm0(ptr5, len5);
    } finally {
        wasm.__wbindgen_free(deferred6_0, deferred6_1, 1);
    }
}

/**
 * @param {number} kind
 * @param {boolean} shadows
 * @param {boolean} view
 * @param {number} texture
 * @param {string} authored
 * @returns {string}
 */
export function actor_effect_description(kind, shadows, view, texture, authored) {
    let deferred7_0;
    let deferred7_1;
    try {
        const val2 = +kind;
        const val3 = +shadows;
        const val4 = +view;
        const val5 = +texture;
        const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.actor_effect_description(val2, val3, val4, val5, ptr0, len0);
        var ptr6 = ret[0];
        var len6 = ret[1];
        if (ret[3]) {
            ptr6 = 0; len6 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred7_0 = ptr6;
        deferred7_1 = len6;
        return getStringFromWasm0(ptr6, len6);
    } finally {
        wasm.__wbindgen_free(deferred7_0, deferred7_1, 1);
    }
}

/**
 * @param {string} authored
 * @param {Float32Array | number[]} vertices
 * @param {Uint32Array | number[]} indices
 * @returns {string}
 */
export function actor_source_description(authored, vertices, indices) {
    let deferred5_0;
    let deferred5_1;
    try {
        let ptr0, len0, ptr1, len1, ptr2, len2;
        try {
            ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            len0 = WASM_VECTOR_LEN;
            ptr1 = passArrayF32ToWasm0(vertices, wasm.__wbindgen_malloc);
            len1 = WASM_VECTOR_LEN;
            ptr2 = passArray32ToWasm0(indices, wasm.__wbindgen_malloc);
            len2 = WASM_VECTOR_LEN;
        } catch (e) {
            if (ptr2) { wasm.__wbindgen_free(ptr2, len2 * 4, 4); }
            if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 4, 4); }
            if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
            throw e;
        }
        const ret = wasm.actor_source_description(ptr0, len0, ptr1, len1, ptr2, len2);
        var ptr4 = ret[0];
        var len4 = ret[1];
        if (ret[3]) {
            ptr4 = 0; len4 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred5_0 = ptr4;
        deferred5_1 = len4;
        return getStringFromWasm0(ptr4, len4);
    } finally {
        wasm.__wbindgen_free(deferred5_0, deferred5_1, 1);
    }
}

/**
 * @param {number} alpha_test
 * @returns {string}
 */
export function fabric_depth_program(alpha_test) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.fabric_depth_program(alpha_test);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * @param {string} authored
 * @returns {string}
 */
export function fabric_description(authored) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.fabric_description(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * @param {number} family
 * @param {number} cascades
 * @param {boolean} lamps
 * @param {boolean} shadows
 * @returns {string}
 */
export function program_warmup_keys(family, cascades, lamps, shadows) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.program_warmup_keys(family, cascades, lamps, shadows);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * @param {WebGL2RenderingContext} gl
 * @returns {number}
 */
export function texture_capabilities(gl) {
    const ret = wasm.texture_capabilities(gl);
    return ret >>> 0;
}

/**
 * @param {string} authored
 * @returns {Float64Array}
 */
export function ui_appearance_defaults(authored) {
    const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.ui_appearance_defaults(ptr0, len0);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v2 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
    return v2;
}

/**
 * @param {string} authored
 * @param {boolean} skin
 * @param {boolean} map
 * @returns {string}
 */
export function ui_appearance_description(authored, skin, map) {
    let deferred5_0;
    let deferred5_1;
    try {
        const val2 = +skin;
        const val3 = +map;
        const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.ui_appearance_description(ptr0, len0, val2, val3);
        var ptr4 = ret[0];
        var len4 = ret[1];
        if (ret[3]) {
            ptr4 = 0; len4 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred5_0 = ptr4;
        deferred5_1 = len4;
        return getStringFromWasm0(ptr4, len4);
    } finally {
        wasm.__wbindgen_free(deferred5_0, deferred5_1, 1);
    }
}

/**
 * @param {string} authored
 * @returns {Float32Array}
 */
export function ui_pick_vertices(authored) {
    const ptr0 = passStringToWasm0(authored, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.ui_pick_vertices(ptr0, len0);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v2 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * @returns {string}
 */
export function water_screen_description() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.water_screen_description();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_boolean_get_4f1baf9a00be3c74: function(arg0) {
            const v = arg0;
            const ret = typeof(v) === 'boolean' ? v : undefined;
            return isLikeNone(ret) ? 0xFFFFFF : ret ? 1 : 0;
        },
        __wbg___wbindgen_copy_to_typed_array_680b191e71920dc1: function(arg0, arg1, arg2) {
            new Uint8Array(arg2.buffer, arg2.byteOffset, arg2.byteLength).set(getArrayU8FromWasm0(arg0, arg1));
        },
        __wbg___wbindgen_debug_string_f249a2a76caa9a55: function(arg0, arg1) {
            const ret = debugString(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_number_get_934b640d0889af78: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64((arg0 >>> 0) + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_7ce8eee7fd327545: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_4c88d8bf264fd984: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_activeTexture_45b425a49df733ce: function(arg0, arg1) {
            arg0.activeTexture(arg1 >>> 0);
        },
        __wbg_activeTexture_89cef8c297ad0698: function(arg0, arg1) {
            arg0.activeTexture(arg1 >>> 0);
        },
        __wbg_attachShader_3ed301cc92db59f6: function(arg0, arg1, arg2) {
            arg0.attachShader(arg1, arg2);
        },
        __wbg_attachShader_817dcec9e79fd18f: function(arg0, arg1, arg2) {
            arg0.attachShader(arg1, arg2);
        },
        __wbg_bindAttribLocation_40c41e6185822b52: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.bindAttribLocation(arg1, arg2 >>> 0, getStringFromWasm0(arg3, arg4));
        },
        __wbg_bindAttribLocation_b51bf2d65ded8f70: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.bindAttribLocation(arg1, arg2 >>> 0, getStringFromWasm0(arg3, arg4));
        },
        __wbg_bindBufferBase_6860496c67f7dd5a: function(arg0, arg1, arg2, arg3) {
            arg0.bindBufferBase(arg1 >>> 0, arg2 >>> 0, arg3);
        },
        __wbg_bindBuffer_b3bb001fdbee089a: function(arg0, arg1, arg2) {
            arg0.bindBuffer(arg1 >>> 0, arg2);
        },
        __wbg_bindBuffer_cbc9075f76fffa51: function(arg0, arg1, arg2) {
            arg0.bindBuffer(arg1 >>> 0, arg2);
        },
        __wbg_bindFramebuffer_79534f24b57b7b95: function(arg0, arg1, arg2) {
            arg0.bindFramebuffer(arg1 >>> 0, arg2);
        },
        __wbg_bindFramebuffer_b245cd93a150dd96: function(arg0, arg1, arg2) {
            arg0.bindFramebuffer(arg1 >>> 0, arg2);
        },
        __wbg_bindRenderbuffer_612f27d9b3b97b9f: function(arg0, arg1, arg2) {
            arg0.bindRenderbuffer(arg1 >>> 0, arg2);
        },
        __wbg_bindTexture_6f822943b2790466: function(arg0, arg1, arg2) {
            arg0.bindTexture(arg1 >>> 0, arg2);
        },
        __wbg_bindTexture_a23350e1b573ddfd: function(arg0, arg1, arg2) {
            arg0.bindTexture(arg1 >>> 0, arg2);
        },
        __wbg_bindVertexArrayOES_5161c1f5b1fea3f9: function(arg0, arg1) {
            arg0.bindVertexArrayOES(arg1);
        },
        __wbg_bindVertexArray_cc64c48bf67ab8d4: function(arg0, arg1) {
            arg0.bindVertexArray(arg1);
        },
        __wbg_blendEquationSeparate_0dceb23863585a87: function(arg0, arg1, arg2) {
            arg0.blendEquationSeparate(arg1 >>> 0, arg2 >>> 0);
        },
        __wbg_blendEquationSeparate_156fe040a9aebfd6: function(arg0, arg1, arg2) {
            arg0.blendEquationSeparate(arg1 >>> 0, arg2 >>> 0);
        },
        __wbg_blendFuncSeparate_6db2212299fd7f1e: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.blendFuncSeparate(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4 >>> 0);
        },
        __wbg_blendFuncSeparate_eb9098d760f4a25c: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.blendFuncSeparate(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4 >>> 0);
        },
        __wbg_blitFramebuffer_d17b7bddf5e86dbc: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10) {
            arg0.blitFramebuffer(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9 >>> 0, arg10 >>> 0);
        },
        __wbg_bufferData_2fb8740d60478bf0: function(arg0, arg1, arg2, arg3) {
            arg0.bufferData(arg1 >>> 0, arg2, arg3 >>> 0);
        },
        __wbg_bufferData_41b591a9f2d502ad: function(arg0, arg1, arg2, arg3) {
            arg0.bufferData(arg1 >>> 0, arg2, arg3 >>> 0);
        },
        __wbg_bufferData_9305dba3ba9cba4e: function(arg0, arg1, arg2, arg3) {
            arg0.bufferData(arg1 >>> 0, arg2, arg3 >>> 0);
        },
        __wbg_bufferData_9eb9c4c5053ac480: function(arg0, arg1, arg2, arg3) {
            arg0.bufferData(arg1 >>> 0, arg2, arg3 >>> 0);
        },
        __wbg_bufferSubData_8acfdcd4b9f7edeb: function(arg0, arg1, arg2, arg3) {
            arg0.bufferSubData(arg1 >>> 0, arg2, arg3);
        },
        __wbg_bufferSubData_9bec93d67e98757e: function(arg0, arg1, arg2, arg3) {
            arg0.bufferSubData(arg1 >>> 0, arg2, arg3);
        },
        __wbg_buffer_aca05a93c046db1a: function(arg0) {
            const ret = arg0.buffer;
            return ret;
        },
        __wbg_byteLength_c5f3c188d5252e5b: function(arg0) {
            const ret = arg0.byteLength;
            return ret;
        },
        __wbg_byteOffset_63cdc7b38f33a45e: function(arg0) {
            const ret = arg0.byteOffset;
            return ret;
        },
        __wbg_checkFramebufferStatus_03ce134350260eda: function(arg0, arg1) {
            const ret = arg0.checkFramebufferStatus(arg1 >>> 0);
            return ret;
        },
        __wbg_checkFramebufferStatus_a2495685980e489c: function(arg0, arg1) {
            const ret = arg0.checkFramebufferStatus(arg1 >>> 0);
            return ret;
        },
        __wbg_clearColor_0a9b08857907c212: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.clearColor(arg1, arg2, arg3, arg4);
        },
        __wbg_clearColor_195b5a731bca5836: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.clearColor(arg1, arg2, arg3, arg4);
        },
        __wbg_clearDepth_3417be30229e823a: function(arg0, arg1) {
            arg0.clearDepth(arg1);
        },
        __wbg_clearDepth_3dc1f52f96f29526: function(arg0, arg1) {
            arg0.clearDepth(arg1);
        },
        __wbg_clearStencil_29f687031586f5e6: function(arg0, arg1) {
            arg0.clearStencil(arg1);
        },
        __wbg_clearStencil_59a8a925116b24ec: function(arg0, arg1) {
            arg0.clearStencil(arg1);
        },
        __wbg_clear_3ffa1ed794f92a04: function(arg0, arg1) {
            arg0.clear(arg1 >>> 0);
        },
        __wbg_clear_ccb3d618da90fea3: function(arg0, arg1) {
            arg0.clear(arg1 >>> 0);
        },
        __wbg_clientWaitSync_fee803cb72073198: function(arg0, arg1, arg2, arg3) {
            const ret = arg0.clientWaitSync(arg1, arg2 >>> 0, arg3 >>> 0);
            return ret;
        },
        __wbg_colorMask_24c1b6c3ec53f8c8: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.colorMask(arg1 !== 0, arg2 !== 0, arg3 !== 0, arg4 !== 0);
        },
        __wbg_colorMask_cfc2d619716d01ed: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.colorMask(arg1 !== 0, arg2 !== 0, arg3 !== 0, arg4 !== 0);
        },
        __wbg_compileShader_11eb6754e9b15970: function(arg0, arg1) {
            arg0.compileShader(arg1);
        },
        __wbg_compileShader_54c3771819e6caba: function(arg0, arg1) {
            arg0.compileShader(arg1);
        },
        __wbg_compressedTexSubImage2D_8706ea3fb9e0180c: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8) {
            arg0.compressedTexSubImage2D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7 >>> 0, arg8);
        },
        __wbg_copyTexSubImage2D_0cc80bc35bca1d8e: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8) {
            arg0.copyTexSubImage2D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7, arg8);
        },
        __wbg_cos_a72b982af451b5a2: function(arg0) {
            const ret = Math.cos(arg0);
            return ret;
        },
        __wbg_createBuffer_2c27ad5f899d418d: function(arg0) {
            const ret = arg0.createBuffer();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createBuffer_ca6ca40e9fb13678: function(arg0) {
            const ret = arg0.createBuffer();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createFramebuffer_cdf686778c61014d: function(arg0) {
            const ret = arg0.createFramebuffer();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createFramebuffer_f52399034cb1a200: function(arg0) {
            const ret = arg0.createFramebuffer();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createProgram_de0881c688a8e43b: function(arg0) {
            const ret = arg0.createProgram();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createProgram_ea784724f6f0074f: function(arg0) {
            const ret = arg0.createProgram();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createRenderbuffer_e45c14c918a72668: function(arg0) {
            const ret = arg0.createRenderbuffer();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createShader_9754a1e4ef374e84: function(arg0, arg1) {
            const ret = arg0.createShader(arg1 >>> 0);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createShader_d2d90020273cef2b: function(arg0, arg1) {
            const ret = arg0.createShader(arg1 >>> 0);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createTexture_3287f26407473071: function(arg0) {
            const ret = arg0.createTexture();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createTexture_eec9a1994aabeca7: function(arg0) {
            const ret = arg0.createTexture();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createVertexArrayOES_3eaf087ef22da1cb: function(arg0) {
            const ret = arg0.createVertexArrayOES();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_createVertexArray_1293d2ffa8346be2: function(arg0) {
            const ret = arg0.createVertexArray();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_cullFace_208151a46885a1fa: function(arg0, arg1) {
            arg0.cullFace(arg1 >>> 0);
        },
        __wbg_cullFace_9eaca57652d7ed37: function(arg0, arg1) {
            arg0.cullFace(arg1 >>> 0);
        },
        __wbg_deleteBuffer_673584ba5a55669d: function(arg0, arg1) {
            arg0.deleteBuffer(arg1);
        },
        __wbg_deleteBuffer_ffa760fd3b0b5ef1: function(arg0, arg1) {
            arg0.deleteBuffer(arg1);
        },
        __wbg_deleteFramebuffer_0ec884f89db72a3e: function(arg0, arg1) {
            arg0.deleteFramebuffer(arg1);
        },
        __wbg_deleteFramebuffer_ffdb1bee4c92ae4c: function(arg0, arg1) {
            arg0.deleteFramebuffer(arg1);
        },
        __wbg_deleteProgram_8c38fff0f4a88dea: function(arg0, arg1) {
            arg0.deleteProgram(arg1);
        },
        __wbg_deleteProgram_c07d952a1b0c1cb6: function(arg0, arg1) {
            arg0.deleteProgram(arg1);
        },
        __wbg_deleteRenderbuffer_3a6e1676ab926e0f: function(arg0, arg1) {
            arg0.deleteRenderbuffer(arg1);
        },
        __wbg_deleteShader_320530dd636bc187: function(arg0, arg1) {
            arg0.deleteShader(arg1);
        },
        __wbg_deleteShader_ac74568b64a1c3c7: function(arg0, arg1) {
            arg0.deleteShader(arg1);
        },
        __wbg_deleteSync_eede535e5826f144: function(arg0, arg1) {
            arg0.deleteSync(arg1);
        },
        __wbg_deleteTexture_1653a9994a44215e: function(arg0, arg1) {
            arg0.deleteTexture(arg1);
        },
        __wbg_deleteVertexArrayOES_2214ecdf65be031d: function(arg0, arg1) {
            arg0.deleteVertexArrayOES(arg1);
        },
        __wbg_deleteVertexArray_2589b74b67ba20c8: function(arg0, arg1) {
            arg0.deleteVertexArray(arg1);
        },
        __wbg_depthFunc_04abd9f6ff980ba9: function(arg0, arg1) {
            arg0.depthFunc(arg1 >>> 0);
        },
        __wbg_depthFunc_584bfa0174e7628a: function(arg0, arg1) {
            arg0.depthFunc(arg1 >>> 0);
        },
        __wbg_depthMask_276856a569045897: function(arg0, arg1) {
            arg0.depthMask(arg1 !== 0);
        },
        __wbg_depthMask_2ba7e83178c3cec3: function(arg0, arg1) {
            arg0.depthMask(arg1 !== 0);
        },
        __wbg_detachShader_c85365fd7844e9b3: function(arg0, arg1, arg2) {
            arg0.detachShader(arg1, arg2);
        },
        __wbg_detachShader_f1bbf36cb414e038: function(arg0, arg1, arg2) {
            arg0.detachShader(arg1, arg2);
        },
        __wbg_disable_2ead6c11cefd0322: function(arg0, arg1) {
            arg0.disable(arg1 >>> 0);
        },
        __wbg_disable_8a9e81e6d64d046b: function(arg0, arg1) {
            arg0.disable(arg1 >>> 0);
        },
        __wbg_drawArraysInstancedANGLE_635e10224985b842: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.drawArraysInstancedANGLE(arg1 >>> 0, arg2, arg3, arg4);
        },
        __wbg_drawArraysInstanced_762b36f2147c7a85: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.drawArraysInstanced(arg1 >>> 0, arg2, arg3, arg4);
        },
        __wbg_drawArrays_a9c18db2e94f4a82: function(arg0, arg1, arg2, arg3) {
            arg0.drawArrays(arg1 >>> 0, arg2, arg3);
        },
        __wbg_drawArrays_fe5a51410bfabb5b: function(arg0, arg1, arg2, arg3) {
            arg0.drawArrays(arg1 >>> 0, arg2, arg3);
        },
        __wbg_drawBuffersWEBGL_49c672d0ddcdfe71: function(arg0, arg1) {
            arg0.drawBuffersWEBGL(arg1);
        },
        __wbg_drawBuffers_e296659ce384e721: function(arg0, arg1) {
            arg0.drawBuffers(arg1);
        },
        __wbg_drawElementsInstancedANGLE_470ce8b489eeafa5: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.drawElementsInstancedANGLE(arg1 >>> 0, arg2, arg3 >>> 0, arg4, arg5);
        },
        __wbg_drawElementsInstanced_3240c0c168d6af87: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.drawElementsInstanced(arg1 >>> 0, arg2, arg3 >>> 0, arg4, arg5);
        },
        __wbg_drawElements_c70e0887282fa70e: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.drawElements(arg1 >>> 0, arg2, arg3 >>> 0, arg4);
        },
        __wbg_drawElements_d76df94ac144b73b: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.drawElements(arg1 >>> 0, arg2, arg3 >>> 0, arg4);
        },
        __wbg_enableVertexAttribArray_d485706ade6bebf8: function(arg0, arg1) {
            arg0.enableVertexAttribArray(arg1 >>> 0);
        },
        __wbg_enableVertexAttribArray_eeecc6185af24227: function(arg0, arg1) {
            arg0.enableVertexAttribArray(arg1 >>> 0);
        },
        __wbg_enable_c3c00565c89d3175: function(arg0, arg1) {
            arg0.enable(arg1 >>> 0);
        },
        __wbg_enable_e955d456ef6b93f7: function(arg0, arg1) {
            arg0.enable(arg1 >>> 0);
        },
        __wbg_fenceSync_53f6f24f4bb43d24: function(arg0, arg1, arg2) {
            const ret = arg0.fenceSync(arg1 >>> 0, arg2 >>> 0);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_flush_a9981e0bd1a44897: function(arg0) {
            arg0.flush();
        },
        __wbg_framebufferRenderbuffer_ffaba93222aea77c: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.framebufferRenderbuffer(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4);
        },
        __wbg_framebufferTexture2D_2c69ac0ce67faf7d: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.framebufferTexture2D(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4, arg5);
        },
        __wbg_framebufferTexture2D_b6250a0c37eb48c2: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.framebufferTexture2D(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4, arg5);
        },
        __wbg_frontFace_124bb11a73cc0dd1: function(arg0, arg1) {
            arg0.frontFace(arg1 >>> 0);
        },
        __wbg_frontFace_8bbd77ca3b8f90a6: function(arg0, arg1) {
            arg0.frontFace(arg1 >>> 0);
        },
        __wbg_generateMipmap_41bc066b35a6cd3d: function(arg0, arg1) {
            arg0.generateMipmap(arg1 >>> 0);
        },
        __wbg_generateMipmap_e1a3b00736bb27bf: function(arg0, arg1) {
            arg0.generateMipmap(arg1 >>> 0);
        },
        __wbg_getActiveAttrib_54be27dea1186114: function(arg0, arg1, arg2) {
            const ret = arg0.getActiveAttrib(arg1, arg2 >>> 0);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_getActiveAttrib_83487440bc75ab73: function(arg0, arg1, arg2) {
            const ret = arg0.getActiveAttrib(arg1, arg2 >>> 0);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_getActiveUniformBlockName_5141b43c031c25f4: function(arg0, arg1, arg2, arg3) {
            const ret = arg1.getActiveUniformBlockName(arg2, arg3 >>> 0);
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg_getActiveUniformBlockParameter_ca8f7310f3720832: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = arg0.getActiveUniformBlockParameter(arg1, arg2 >>> 0, arg3 >>> 0);
            return ret;
        }, arguments); },
        __wbg_getActiveUniform_4d82b7834e1b4ef8: function(arg0, arg1, arg2) {
            const ret = arg0.getActiveUniform(arg1, arg2 >>> 0);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_getActiveUniform_7d405d07694ad340: function(arg0, arg1, arg2) {
            const ret = arg0.getActiveUniform(arg1, arg2 >>> 0);
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_getAttribLocation_239fdc3b9bf96547: function(arg0, arg1, arg2, arg3) {
            const ret = arg0.getAttribLocation(arg1, getStringFromWasm0(arg2, arg3));
            return ret;
        },
        __wbg_getAttribLocation_8e502cd753de95f7: function(arg0, arg1, arg2, arg3) {
            const ret = arg0.getAttribLocation(arg1, getStringFromWasm0(arg2, arg3));
            return ret;
        },
        __wbg_getBufferParameter_43b9e1e68973d982: function(arg0, arg1, arg2) {
            const ret = arg0.getBufferParameter(arg1 >>> 0, arg2 >>> 0);
            return ret;
        },
        __wbg_getBufferParameter_83ca21f8b4b4fe12: function(arg0, arg1, arg2) {
            const ret = arg0.getBufferParameter(arg1 >>> 0, arg2 >>> 0);
            return ret;
        },
        __wbg_getExtension_30c10f49f67eb05d: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getExtension(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getParameter_4c67158a3cef7a34: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.getParameter(arg1 >>> 0);
            return ret;
        }, arguments); },
        __wbg_getProgramInfoLog_01799fd7b07b7f3c: function(arg0, arg1, arg2) {
            const ret = arg1.getProgramInfoLog(arg2);
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg_getProgramInfoLog_92d61551e1ab39ea: function(arg0, arg1, arg2) {
            const ret = arg1.getProgramInfoLog(arg2);
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg_getProgramParameter_1e7562e7d66170c3: function(arg0, arg1, arg2) {
            const ret = arg0.getProgramParameter(arg1, arg2 >>> 0);
            return ret;
        },
        __wbg_getProgramParameter_3edd526f9e5f6c10: function(arg0, arg1, arg2) {
            const ret = arg0.getProgramParameter(arg1, arg2 >>> 0);
            return ret;
        },
        __wbg_getShaderInfoLog_3cfe5b3e097d0f2d: function(arg0, arg1, arg2) {
            const ret = arg1.getShaderInfoLog(arg2);
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg_getShaderInfoLog_8758e8aea7960121: function(arg0, arg1, arg2) {
            const ret = arg1.getShaderInfoLog(arg2);
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg_getShaderParameter_a346f74d5ba41c4b: function(arg0, arg1, arg2) {
            const ret = arg0.getShaderParameter(arg1, arg2 >>> 0);
            return ret;
        },
        __wbg_getShaderParameter_fc64069fbc010d19: function(arg0, arg1, arg2) {
            const ret = arg0.getShaderParameter(arg1, arg2 >>> 0);
            return ret;
        },
        __wbg_getSupportedExtensions_f48b337512e8f8ce: function(arg0) {
            const ret = arg0.getSupportedExtensions();
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_getUniformBlockIndex_0742ae35f926850a: function(arg0, arg1, arg2, arg3) {
            const ret = arg0.getUniformBlockIndex(arg1, getStringFromWasm0(arg2, arg3));
            return ret;
        },
        __wbg_getUniformLocation_6352e0669c8f5393: function(arg0, arg1, arg2, arg3) {
            const ret = arg0.getUniformLocation(arg1, getStringFromWasm0(arg2, arg3));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_getUniformLocation_e0d11c86376cdd26: function(arg0, arg1, arg2, arg3) {
            const ret = arg0.getUniformLocation(arg1, getStringFromWasm0(arg2, arg3));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_getUniform_d35b624a09520c70: function(arg0, arg1, arg2) {
            const ret = arg0.getUniform(arg1, arg2);
            return ret;
        },
        __wbg_getUniform_ddce5c34c6813a0b: function(arg0, arg1, arg2) {
            const ret = arg0.getUniform(arg1, arg2);
            return ret;
        },
        __wbg_get_2080c49015e35254: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_unchecked_e6325ad374eff861: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_height_ac6f1c3f49c13fe2: function(arg0) {
            const ret = arg0.height;
            return ret;
        },
        __wbg_hypot_603147c7418a59ed: function(arg0, arg1, arg2) {
            const ret = Math.hypot(arg0, arg1, arg2);
            return ret;
        },
        __wbg_hypot_a50a06dc733d28d3: function(arg0, arg1) {
            const ret = Math.hypot(arg0, arg1);
            return ret;
        },
        __wbg_instanceof_Float32Array_94718abab7dc1d7f: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Float32Array;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Int32Array_6d09616361bcf0c6: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Int32Array;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Uint32Array_ba721c3060d01f9a: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Uint32Array;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_WebGlFramebuffer_65301cc37e9738f1: function(arg0) {
            let result;
            try {
                result = arg0 instanceof WebGLFramebuffer;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_isContextLost_dc5d602e27060cc9: function(arg0) {
            const ret = arg0.isContextLost();
            return ret;
        },
        __wbg_length_03a2b32a9178fe11: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_186163212ffe79c3: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_6e9d2869242cabf3: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_b03a0fabb109b1e9: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_c702d06d28b96b72: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_lineWidth_5a5ff0d63e371f1b: function(arg0, arg1) {
            arg0.lineWidth(arg1);
        },
        __wbg_lineWidth_faffcbfeed8e9d7f: function(arg0, arg1) {
            arg0.lineWidth(arg1);
        },
        __wbg_linkProgram_99848b73f1ebab5f: function(arg0, arg1) {
            arg0.linkProgram(arg1);
        },
        __wbg_linkProgram_dee7cda98bacb12f: function(arg0, arg1) {
            arg0.linkProgram(arg1);
        },
        __wbg_multiDrawElementsWEBGL_792aac30dde30bf8: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9) {
            arg0.multiDrawElementsWEBGL(arg1 >>> 0, getArrayI32FromWasm0(arg2, arg3), arg4 >>> 0, arg5 >>> 0, getArrayI32FromWasm0(arg6, arg7), arg8 >>> 0, arg9);
        },
        __wbg_name_52bccd784c3d27a8: function(arg0, arg1) {
            const ret = arg1.name;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 1, len1, true);
            getDataViewMemory0().setInt32((arg0 >>> 0) + 4 * 0, ptr1, true);
        },
        __wbg_new_13de428334042d26: function(arg0) {
            const ret = new Uint8Array(arg0);
            return ret;
        },
        __wbg_new_37eb45aaf5eeb820: function() {
            const ret = new Array();
            return ret;
        },
        __wbg_new_with_byte_offset_and_length_1ea36b9f0c3bc55a: function(arg0, arg1, arg2) {
            const ret = new Float32Array(arg0, arg1 >>> 0, arg2 >>> 0);
            return ret;
        },
        __wbg_new_with_byte_offset_and_length_c71f41284ed576da: function(arg0, arg1, arg2) {
            const ret = new Uint16Array(arg0, arg1 >>> 0, arg2 >>> 0);
            return ret;
        },
        __wbg_of_027eb15371aac932: function(arg0) {
            const ret = Array.of(arg0);
            return ret;
        },
        __wbg_pixelStorei_43dc7945a0d4d49c: function(arg0, arg1, arg2) {
            arg0.pixelStorei(arg1 >>> 0, arg2);
        },
        __wbg_pixelStorei_6dd56b4da047c480: function(arg0, arg1, arg2) {
            arg0.pixelStorei(arg1 >>> 0, arg2);
        },
        __wbg_pow_81fe34dc2f799d54: function(arg0, arg1) {
            const ret = Math.pow(arg0, arg1);
            return ret;
        },
        __wbg_prototypesetcall_13162965d1fe7aa3: function(arg0, arg1, arg2) {
            Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), arg2);
        },
        __wbg_prototypesetcall_153eecd0d28a42a5: function(arg0, arg1, arg2) {
            Int32Array.prototype.set.call(getArrayI32FromWasm0(arg0, arg1), arg2);
        },
        __wbg_prototypesetcall_22b1bb88c4ec5c5a: function(arg0, arg1, arg2) {
            Float32Array.prototype.set.call(getArrayF32FromWasm0(arg0, arg1), arg2);
        },
        __wbg_prototypesetcall_4aea6470234c8414: function(arg0, arg1, arg2) {
            Uint32Array.prototype.set.call(getArrayU32FromWasm0(arg0, arg1), arg2);
        },
        __wbg_push_c9f4893fc209f768: function(arg0, arg1) {
            const ret = arg0.push(arg1);
            return ret;
        },
        __wbg_readBuffer_49d4b278b9838d40: function(arg0, arg1) {
            arg0.readBuffer(arg1 >>> 0);
        },
        __wbg_readPixels_22316d250d820aff: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7) {
            arg0.readPixels(arg1, arg2, arg3, arg4, arg5 >>> 0, arg6 >>> 0, arg7);
        }, arguments); },
        __wbg_renderbufferStorageMultisample_d5b01825cc08514b: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.renderbufferStorageMultisample(arg1 >>> 0, arg2, arg3 >>> 0, arg4, arg5);
        },
        __wbg_renderbufferStorage_ebe429f577dae949: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.renderbufferStorage(arg1 >>> 0, arg2 >>> 0, arg3, arg4);
        },
        __wbg_scissor_17956d9d23dfe025: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.scissor(arg1, arg2, arg3, arg4);
        },
        __wbg_shaderSource_519d0f56b613b2f9: function(arg0, arg1, arg2, arg3) {
            arg0.shaderSource(arg1, getStringFromWasm0(arg2, arg3));
        },
        __wbg_shaderSource_c4c06a3cbc2e7bf3: function(arg0, arg1, arg2, arg3) {
            arg0.shaderSource(arg1, getStringFromWasm0(arg2, arg3));
        },
        __wbg_sin_40ec7a9b9093c6a0: function(arg0) {
            const ret = Math.sin(arg0);
            return ret;
        },
        __wbg_size_772812b17f38f9bf: function(arg0) {
            const ret = arg0.size;
            return ret;
        },
        __wbg_stencilMask_1fe24adb96500c11: function(arg0, arg1) {
            arg0.stencilMask(arg1 >>> 0);
        },
        __wbg_stencilMask_6d5dbf91337de94e: function(arg0, arg1) {
            arg0.stencilMask(arg1 >>> 0);
        },
        __wbg_texImage2D_20b5002326193fe6: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9) {
            arg0.texImage2D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7 >>> 0, arg8 >>> 0, arg9);
        }, arguments); },
        __wbg_texImage2D_5048d2cd8540da16: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9) {
            arg0.texImage2D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7 >>> 0, arg8 >>> 0, arg9);
        }, arguments); },
        __wbg_texImage2D_84bdef619a1ea935: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9) {
            arg0.texImage2D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7 >>> 0, arg8 >>> 0, arg9);
        }, arguments); },
        __wbg_texImage3D_abf2fdb0f8c4c1b6: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10) {
            arg0.texImage3D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7, arg8 >>> 0, arg9 >>> 0, arg10);
        }, arguments); },
        __wbg_texParameterf_61aaba444e74bb7b: function(arg0, arg1, arg2, arg3) {
            arg0.texParameterf(arg1 >>> 0, arg2 >>> 0, arg3);
        },
        __wbg_texParameteri_0afc5d515132fed2: function(arg0, arg1, arg2, arg3) {
            arg0.texParameteri(arg1 >>> 0, arg2 >>> 0, arg3);
        },
        __wbg_texParameteri_e24e4fdc17153838: function(arg0, arg1, arg2, arg3) {
            arg0.texParameteri(arg1 >>> 0, arg2 >>> 0, arg3);
        },
        __wbg_texStorage2D_e0ebb5940fa1fc5e: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.texStorage2D(arg1 >>> 0, arg2, arg3 >>> 0, arg4, arg5);
        },
        __wbg_texStorage3D_a82e5be79cb278dc: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.texStorage3D(arg1 >>> 0, arg2, arg3 >>> 0, arg4, arg5, arg6);
        },
        __wbg_texSubImage2D_4343af4633b8b8f8: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9) {
            arg0.texSubImage2D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7 >>> 0, arg8 >>> 0, arg9);
        }, arguments); },
        __wbg_texSubImage2D_4aef07460a86c81e: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9) {
            arg0.texSubImage2D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7 >>> 0, arg8 >>> 0, arg9);
        }, arguments); },
        __wbg_texSubImage2D_711516a8a98bec6a: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7) {
            arg0.texSubImage2D(arg1 >>> 0, arg2, arg3, arg4, arg5 >>> 0, arg6 >>> 0, arg7);
        }, arguments); },
        __wbg_texSubImage2D_a26a8700e734449a: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9) {
            arg0.texSubImage2D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7 >>> 0, arg8 >>> 0, arg9);
        }, arguments); },
        __wbg_texSubImage3D_5e8b27dc040faf8d: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10, arg11) {
            arg0.texSubImage3D(arg1 >>> 0, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9 >>> 0, arg10 >>> 0, arg11);
        }, arguments); },
        __wbg_transfer_3fd25341962a2e4e: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.transfer(arg1 >>> 0);
            return ret;
        }, arguments); },
        __wbg_type_9c887dd8c3d0ba23: function(arg0) {
            const ret = arg0.type;
            return ret;
        },
        __wbg_uniform1f_0b36dc15cb2b4991: function(arg0, arg1, arg2) {
            arg0.uniform1f(arg1, arg2);
        },
        __wbg_uniform1f_fe417acc7c7a1dc5: function(arg0, arg1, arg2) {
            arg0.uniform1f(arg1, arg2);
        },
        __wbg_uniform1fv_19f7e96820e32a42: function(arg0, arg1, arg2, arg3) {
            arg0.uniform1fv(arg1, getArrayF32FromWasm0(arg2, arg3));
        },
        __wbg_uniform1fv_a71814c5c9b8550c: function(arg0, arg1, arg2, arg3) {
            arg0.uniform1fv(arg1, getArrayF32FromWasm0(arg2, arg3));
        },
        __wbg_uniform1i_385ebc488d4d763f: function(arg0, arg1, arg2) {
            arg0.uniform1i(arg1, arg2);
        },
        __wbg_uniform1i_778a02a48ecdceed: function(arg0, arg1, arg2) {
            arg0.uniform1i(arg1, arg2);
        },
        __wbg_uniform1iv_720e3c4c65756ffe: function(arg0, arg1, arg2, arg3) {
            arg0.uniform1iv(arg1, getArrayI32FromWasm0(arg2, arg3));
        },
        __wbg_uniform1iv_efb158454c8967c2: function(arg0, arg1, arg2, arg3) {
            arg0.uniform1iv(arg1, getArrayI32FromWasm0(arg2, arg3));
        },
        __wbg_uniform2f_6bd82d6e7f81060b: function(arg0, arg1, arg2, arg3) {
            arg0.uniform2f(arg1, arg2, arg3);
        },
        __wbg_uniform2f_7e4c627c4106dd3f: function(arg0, arg1, arg2, arg3) {
            arg0.uniform2f(arg1, arg2, arg3);
        },
        __wbg_uniform2fv_486658e6bcddfbaf: function(arg0, arg1, arg2, arg3) {
            arg0.uniform2fv(arg1, getArrayF32FromWasm0(arg2, arg3));
        },
        __wbg_uniform2fv_71fe605c72879b61: function(arg0, arg1, arg2, arg3) {
            arg0.uniform2fv(arg1, getArrayF32FromWasm0(arg2, arg3));
        },
        __wbg_uniform3f_6f94179d13a10f48: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.uniform3f(arg1, arg2, arg3, arg4);
        },
        __wbg_uniform3f_b9f8bdb406b70135: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.uniform3f(arg1, arg2, arg3, arg4);
        },
        __wbg_uniform3fv_431207a9407b7d29: function(arg0, arg1, arg2, arg3) {
            arg0.uniform3fv(arg1, getArrayF32FromWasm0(arg2, arg3));
        },
        __wbg_uniform3fv_58fa948c65cb5f6c: function(arg0, arg1, arg2, arg3) {
            arg0.uniform3fv(arg1, getArrayF32FromWasm0(arg2, arg3));
        },
        __wbg_uniform4f_14a3638afe93aa8f: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.uniform4f(arg1, arg2, arg3, arg4, arg5);
        },
        __wbg_uniform4f_5c31fb6dab645ad8: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.uniform4f(arg1, arg2, arg3, arg4, arg5);
        },
        __wbg_uniform4fv_a1415c11925ac651: function(arg0, arg1, arg2, arg3) {
            arg0.uniform4fv(arg1, getArrayF32FromWasm0(arg2, arg3));
        },
        __wbg_uniform4fv_d3afea28181f5342: function(arg0, arg1, arg2, arg3) {
            arg0.uniform4fv(arg1, getArrayF32FromWasm0(arg2, arg3));
        },
        __wbg_uniformBlockBinding_61c5a97721e0f36b: function(arg0, arg1, arg2, arg3) {
            arg0.uniformBlockBinding(arg1, arg2 >>> 0, arg3 >>> 0);
        },
        __wbg_uniformMatrix3fv_15b2b1f5505b8506: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.uniformMatrix3fv(arg1, arg2 !== 0, getArrayF32FromWasm0(arg3, arg4));
        },
        __wbg_uniformMatrix3fv_df1958f8b617c8d6: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.uniformMatrix3fv(arg1, arg2 !== 0, getArrayF32FromWasm0(arg3, arg4));
        },
        __wbg_uniformMatrix4fv_cd0ba72d1d780759: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.uniformMatrix4fv(arg1, arg2 !== 0, getArrayF32FromWasm0(arg3, arg4));
        },
        __wbg_uniformMatrix4fv_edd2b51dbb1caffd: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.uniformMatrix4fv(arg1, arg2 !== 0, getArrayF32FromWasm0(arg3, arg4));
        },
        __wbg_useProgram_d9c24a7d8632c4c1: function(arg0, arg1) {
            arg0.useProgram(arg1);
        },
        __wbg_useProgram_f6341ea2eeea1e5a: function(arg0, arg1) {
            arg0.useProgram(arg1);
        },
        __wbg_vertexAttribDivisorANGLE_5bec9a774ccd1cf8: function(arg0, arg1, arg2) {
            arg0.vertexAttribDivisorANGLE(arg1 >>> 0, arg2 >>> 0);
        },
        __wbg_vertexAttribDivisor_5b64fc697c0496e1: function(arg0, arg1, arg2) {
            arg0.vertexAttribDivisor(arg1 >>> 0, arg2 >>> 0);
        },
        __wbg_vertexAttribPointer_39b706eb26c50057: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.vertexAttribPointer(arg1 >>> 0, arg2, arg3 >>> 0, arg4 !== 0, arg5, arg6);
        },
        __wbg_vertexAttribPointer_9e0e4517b8d274bd: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.vertexAttribPointer(arg1 >>> 0, arg2, arg3 >>> 0, arg4 !== 0, arg5, arg6);
        },
        __wbg_viewport_aaac6dd8cff1831a: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.viewport(arg1, arg2, arg3, arg4);
        },
        __wbg_viewport_cdae9e8b2f33343d: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.viewport(arg1, arg2, arg3, arg4);
        },
        __wbg_width_babc5dc7654771b1: function(arg0) {
            const ret = arg0.width;
            return ret;
        },
        __wbindgen_generic_0000000000000001: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_generic_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(F32)) -> NamedExternref("Float32Array")`.
            const ret = getArrayF32FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_generic_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(I16)) -> NamedExternref("Int16Array")`.
            const ret = getArrayI16FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_generic_0000000000000004: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(I32)) -> NamedExternref("Int32Array")`.
            const ret = getArrayI32FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_generic_0000000000000005: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(I8)) -> NamedExternref("Int8Array")`.
            const ret = getArrayI8FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_generic_0000000000000006: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(U16)) -> NamedExternref("Uint16Array")`.
            const ret = getArrayU16FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_generic_0000000000000007: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(U32)) -> NamedExternref("Uint32Array")`.
            const ret = getArrayU32FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_generic_0000000000000008: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(U8)) -> NamedExternref("Uint8Array")`.
            const ret = getArrayU8FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_generic_0000000000000009: function(arg0, arg1) {
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
        "./bfe_render_bg.js": import0,
    };
}

const BfeDepthMarkerFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bfedepthmarker_free(ptr, 1));
const BfePoseHistoryFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bfeposehistory_free(ptr, 1));
const BfeRendererFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bferenderer_free(ptr, 1));
const BfeSceneFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bfescene_free(ptr, 1));
const BfeUniformTransformsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_bfeuniformtransforms_free(ptr, 1));
const GpuResourcesFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_gpuresources_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
}

function debugString(val) {
    switch (typeof val) {
        // primitive types
        case 'number':
        case 'boolean':
        case 'undefined':
        return `${val}`;
        case 'string':
        return JSON.stringify(val);
        case 'bigint':
        return `${val}n`;
        case 'symbol':
        return val.description !== undefined
            ? `Symbol(${val.description})`
            : 'Symbol';
        case 'function':
        return typeof val.name === 'string' && val.name !== ''
            ? `Function(${val.name})`
            : 'Function';
        case 'object': {
            if (val === null) {
                return 'null';
            }
            // objects
            if (Array.isArray(val)) {
                return '[' + Array.from(val, debugString).join(', ') + ']';
            }
            // errors
            if (val instanceof Error) {
                return `${val.name}: ${val.message}\n${val.stack}`;
            }

            const toStringOutput = toString.call(val);
            // Test for built-in
            const className = /\[object ([^\]]+)\]/.exec(toStringOutput)?.[1];

            // Failed to match the standard '[object ClassName]'
            if (className === undefined) {
                return toStringOutput;
            }
            if (className === 'Object') {
                // we're a user defined class or Object
                // JSON.stringify avoids problems with cycles, and is generally much
                // easier than looping through ownProperties of `val`.
                try {
                    return 'Object(' + JSON.stringify(val) + ')';
                } catch (_) {
                    return 'Object';
                }
            }
            // TODO we could test for more things here, like `Set`s and `Map`s.
            return className;
        }
        // Per MDN: 'This list of values is exhaustive.'
        // https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Operators/typeof#description
    }
}

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayF64FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat64ArrayMemory0().subarray(ptr / 8, ptr / 8 + len);
}

function getArrayI16FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getInt16ArrayMemory0().subarray(ptr / 2, ptr / 2 + len);
}

function getArrayI32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getInt32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayI8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getInt8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

function getArrayU16FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint16ArrayMemory0().subarray(ptr / 2, ptr / 2 + len);
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

let cachedFloat64ArrayMemory0 = null;
function getFloat64ArrayMemory0() {
    if (cachedFloat64ArrayMemory0 === null || cachedFloat64ArrayMemory0.byteLength === 0) {
        cachedFloat64ArrayMemory0 = new Float64Array(wasm.memory.buffer);
    }
    return cachedFloat64ArrayMemory0;
}

let cachedInt16ArrayMemory0 = null;
function getInt16ArrayMemory0() {
    if (cachedInt16ArrayMemory0 === null || cachedInt16ArrayMemory0.byteLength === 0) {
        cachedInt16ArrayMemory0 = new Int16Array(wasm.memory.buffer);
    }
    return cachedInt16ArrayMemory0;
}

let cachedInt32ArrayMemory0 = null;
function getInt32ArrayMemory0() {
    if (cachedInt32ArrayMemory0 === null || cachedInt32ArrayMemory0.byteLength === 0) {
        cachedInt32ArrayMemory0 = new Int32Array(wasm.memory.buffer);
    }
    return cachedInt32ArrayMemory0;
}

let cachedInt8ArrayMemory0 = null;
function getInt8ArrayMemory0() {
    if (cachedInt8ArrayMemory0 === null || cachedInt8ArrayMemory0.byteLength === 0) {
        cachedInt8ArrayMemory0 = new Int8Array(wasm.memory.buffer);
    }
    return cachedInt8ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

let cachedUint16ArrayMemory0 = null;
function getUint16ArrayMemory0() {
    if (cachedUint16ArrayMemory0 === null || cachedUint16ArrayMemory0.byteLength === 0) {
        cachedUint16ArrayMemory0 = new Uint16Array(wasm.memory.buffer);
    }
    return cachedUint16ArrayMemory0;
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

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passArray16ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 2, 2) >>> 0;
    getUint16ArrayMemory0().set(arg, ptr / 2);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArray32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getUint32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getFloat32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF64ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 8, 8) >>> 0;
    getFloat64ArrayMemory0().set(arg, ptr / 8);
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
    cachedDataViewMemory0 = null;
    cachedFloat32ArrayMemory0 = null;
    cachedFloat64ArrayMemory0 = null;
    cachedInt16ArrayMemory0 = null;
    cachedInt32ArrayMemory0 = null;
    cachedInt8ArrayMemory0 = null;
    cachedUint16ArrayMemory0 = null;
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
        module_or_path = new URL('bfe_render_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
