export class PageAnalyticEffects {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageAnalyticEffectsFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pageanalyticeffects_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    add_batch() {
        const ret = wasm.pageanalyticeffects_add_batch(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    arrays_len() {
        const ret = wasm.pageanalyticeffects_arrays_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    arrays_ptr() {
        const ret = wasm.pageanalyticeffects_arrays_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    batches_len() {
        const ret = wasm.pageanalyticeffects_batches_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    batches_ptr() {
        const ret = wasm.pageanalyticeffects_batches_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    data_len() {
        const ret = wasm.pageanalyticeffects_data_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    data_ptr() {
        const ret = wasm.pageanalyticeffects_data_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    definitions_len() {
        const ret = wasm.pageanalyticeffects_definitions_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    definitions_ptr() {
        const ret = wasm.pageanalyticeffects_definitions_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    emitters() {
        wasm.pageanalyticeffects_emitters(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    emitters_len() {
        const ret = wasm.pageanalyticeffects_emitters_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    emitters_ptr() {
        const ret = wasm.pageanalyticeffects_emitters_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    epochs_len() {
        const ret = wasm.pageanalyticeffects_epochs_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    epochs_ptr() {
        const ret = wasm.pageanalyticeffects_epochs_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    frame() {
        wasm.pageanalyticeffects_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pageanalyticeffects_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pageanalyticeffects_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} content
     */
    constructor(content) {
        const ptr0 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageanalyticeffects_new(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        PageAnalyticEffectsFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    spawns_len() {
        const ret = wasm.pageanalyticeffects_spawns_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    spawns_ptr() {
        const ret = wasm.pageanalyticeffects_spawns_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    track_rows() {
        const ret = wasm.pageanalyticeffects_track_rows(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    tracks_len() {
        const ret = wasm.pageanalyticeffects_tracks_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    tracks_ptr() {
        const ret = wasm.pageanalyticeffects_tracks_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageAnalyticEffects.prototype[Symbol.dispose] = PageAnalyticEffects.prototype.free;

export class PageArcCamera {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageArcCameraFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagearccamera_free(ptr, 0);
    }
    constructor() {
        const ret = wasm.pagearccamera_new();
        this.__wbg_ptr = ret;
        PageArcCameraFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    output_ptr() {
        const ret = wasm.pagearccamera_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} fov
     * @param {number} zoom
     * @param {number} near
     * @param {number} height
     */
    step(fov, zoom, near, height) {
        wasm.pagearccamera_step(this.__wbg_ptr, fov, zoom, near, height);
    }
}
if (Symbol.dispose) PageArcCamera.prototype[Symbol.dispose] = PageArcCamera.prototype.free;

/**
 * Replicated combat lifecycle shares the presentation memory; the host only
 * answers resource/prediction requests and applies ordered output records.
 */
export class PageCombat {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageCombatFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagecombat_free(ptr, 0);
    }
    /**
     * @param {string} facts
     * @returns {string}
     */
    static capture_bolt(facts) {
        let deferred3_0;
        let deferred3_1;
        try {
            const ptr0 = passStringToWasm0(facts, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.pagecombat_capture_bolt(ptr0, len0);
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
     * @param {string} kind
     * @returns {boolean}
     */
    static capture_feedback(kind) {
        const ptr0 = passStringToWasm0(kind, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagecombat_capture_feedback(ptr0, len0);
        return ret !== 0;
    }
    /**
     * @param {number} id
     * @returns {Float64Array}
     */
    static capture_identity(id) {
        const ret = wasm.pagecombat_capture_identity(id);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v1 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v1;
    }
    /**
     * @param {string} state
     * @param {number} tick
     * @returns {string}
     */
    static capture_terminal(state, tick) {
        let deferred4_0;
        let deferred4_1;
        try {
            const val2 = +tick;
            const ptr0 = passStringToWasm0(state, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.pagecombat_capture_terminal(ptr0, len0, val2);
            var ptr3 = ret[0];
            var len3 = ret[1];
            if (ret[3]) {
                ptr3 = 0; len3 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred4_0 = ptr3;
            deferred4_1 = len3;
            return getStringFromWasm0(ptr3, len3);
        } finally {
            wasm.__wbindgen_free(deferred4_0, deferred4_1, 1);
        }
    }
    /**
     * @param {number} schema
     * @param {number} target
     * @param {number} attacker
     * @param {number} own
     * @returns {boolean}
     */
    static concerns(schema, target, attacker, own) {
        const ret = wasm.pagecombat_concerns(schema, target, attacker, own);
        return ret !== 0;
    }
    /**
     * @param {string} kind
     * @param {number} schema
     * @param {number} tick
     * @param {Uint8Array | number[]} bytes
     * @param {boolean} valid
     * @returns {string}
     */
    static decode_record(kind, schema, tick, bytes, valid) {
        let deferred7_0;
        let deferred7_1;
        try {
            const val3 = +schema;
            const val4 = +tick;
            const val5 = +valid;
            let ptr0, len0, ptr1, len1;
            try {
                ptr0 = passStringToWasm0(kind, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
                len0 = WASM_VECTOR_LEN;
                ptr1 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
                len1 = WASM_VECTOR_LEN;
            } catch (e) {
                if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
                if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
                throw e;
            }
            const ret = wasm.pagecombat_decode_record(ptr0, len0, val3, val4, ptr1, len1, val5);
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
     * @param {string} facts
     * @returns {string}
     */
    static event_capture(facts) {
        let deferred3_0;
        let deferred3_1;
        try {
            const ptr0 = passStringToWasm0(facts, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.pagecombat_event_capture(ptr0, len0);
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
     * @param {string} event
     * @param {string} facts
     * @returns {string}
     */
    static event_plan(event, facts) {
        let deferred4_0;
        let deferred4_1;
        try {
            let ptr0, len0, ptr1, len1;
            try {
                ptr0 = passStringToWasm0(event, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
                len0 = WASM_VECTOR_LEN;
                ptr1 = passStringToWasm0(facts, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
                len1 = WASM_VECTOR_LEN;
            } catch (e) {
                if (ptr1) { wasm.__wbindgen_free(ptr1, len1 * 1, 1); }
                if (ptr0) { wasm.__wbindgen_free(ptr0, len0 * 1, 1); }
                throw e;
            }
            const ret = wasm.pagecombat_event_plan(ptr0, len0, ptr1, len1);
            var ptr3 = ret[0];
            var len3 = ret[1];
            if (ret[3]) {
                ptr3 = 0; len3 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred4_0 = ptr3;
            deferred4_1 = len3;
            return getStringFromWasm0(ptr3, len3);
        } finally {
            wasm.__wbindgen_free(deferred4_0, deferred4_1, 1);
        }
    }
    /**
     * @param {string} kind
     * @param {number} owner
     * @param {number} generation
     * @param {boolean} present
     * @param {number} actual
     * @param {number} mount
     * @param {number} mount_generation
     * @param {boolean} mount_present
     * @param {number} mount_actual
     * @returns {boolean}
     */
    static event_ready(kind, owner, generation, present, actual, mount, mount_generation, mount_present, mount_actual) {
        const val2 = +owner;
        const val3 = +generation;
        const val4 = +present;
        const val5 = +actual;
        const val6 = +mount;
        const val7 = +mount_generation;
        const val8 = +mount_present;
        const val9 = +mount_actual;
        const ptr0 = passStringToWasm0(kind, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagecombat_event_ready(ptr0, len0, val2, val3, val4, val5, val6, val7, val8, val9);
        return ret !== 0;
    }
    feedback() {
        wasm.pagecombat_feedback(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    feedback_input_len() {
        const ret = wasm.pagecombat_feedback_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    feedback_input_ptr() {
        const ret = wasm.pagecombat_feedback_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} locals
     * @param {number} team
     * @returns {boolean}
     */
    static feedback_locals(locals, team) {
        const ret = wasm.pagecombat_feedback_locals(locals, team);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    feedback_output_len() {
        const ret = wasm.pagecombat_feedback_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    feedback_output_ptr() {
        const ret = wasm.pagecombat_feedback_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} relation
     * @returns {number}
     */
    static feedback_relation(relation) {
        const ptr0 = passStringToWasm0(relation, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagecombat_feedback_relation(ptr0, len0);
        return ret;
    }
    /**
     * @param {number} effect
     * @param {string} definition
     * @returns {string}
     */
    static impact_name(effect, definition) {
        let deferred4_0;
        let deferred4_1;
        try {
            const val2 = +effect;
            const ptr0 = passStringToWasm0(definition, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.pagecombat_impact_name(val2, ptr0, len0);
            var ptr3 = ret[0];
            var len3 = ret[1];
            if (ret[3]) {
                ptr3 = 0; len3 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred4_0 = ptr3;
            deferred4_1 = len3;
            return getStringFromWasm0(ptr3, len3);
        } finally {
            wasm.__wbindgen_free(deferred4_0, deferred4_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagecombat_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagecombat_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {boolean} predicted
     * @returns {boolean}
     */
    muzzle_request(predicted) {
        const ret = wasm.pagecombat_muzzle_request(this.__wbg_ptr, predicted);
        return ret !== 0;
    }
    /**
     * @param {number} capacity
     */
    constructor(capacity) {
        const ret = wasm.pagecombat_new(capacity);
        this.__wbg_ptr = ret;
        PageCombatFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    output_len() {
        const ret = wasm.pagecombat_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    output_ptr() {
        const ret = wasm.pagecombat_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} tick
     */
    present(tick) {
        wasm.pagecombat_present(this.__wbg_ptr, tick);
    }
    /**
     * @param {number} epoch
     * @param {number} tick
     * @returns {boolean}
     */
    receive(epoch, tick) {
        const ret = wasm.pagecombat_receive(this.__wbg_ptr, epoch, tick);
        return ret !== 0;
    }
    /**
     * @param {number} epoch
     * @param {number} tick
     * @param {string} definition
     * @param {boolean} has_definition
     * @returns {boolean}
     */
    receive_facts(epoch, tick, definition, has_definition) {
        const val2 = +epoch;
        const val3 = +tick;
        const val4 = +has_definition;
        const ptr0 = passStringToWasm0(definition, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagecombat_receive_facts(this.__wbg_ptr, val2, val3, ptr0, len0, val4);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] !== 0;
    }
    /**
     * @param {number} epoch
     */
    reset(epoch) {
        wasm.pagecombat_reset(this.__wbg_ptr, epoch);
    }
    /**
     * @param {boolean} predicted
     * @returns {boolean}
     */
    resolve(predicted) {
        const ret = wasm.pagecombat_resolve(this.__wbg_ptr, predicted);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    result_len() {
        const ret = wasm.pagecombat_result_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    result_ptr() {
        const ret = wasm.pagecombat_result_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} tick
     */
    settle(tick) {
        wasm.pagecombat_settle(this.__wbg_ptr, tick);
    }
    /**
     * @param {number} amount
     * @returns {number}
     */
    static visual_damage(amount) {
        const ret = wasm.pagecombat_visual_damage(amount);
        return ret;
    }
}
if (Symbol.dispose) PageCombat.prototype[Symbol.dispose] = PageCombat.prototype.free;

export class PageDeathTrace {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageDeathTraceFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagedeathtrace_free(ptr, 0);
    }
    frame() {
        wasm.pagedeathtrace_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagedeathtrace_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagedeathtrace_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagedeathtrace_new();
        this.__wbg_ptr = ret;
        PageDeathTraceFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    positions_len() {
        const ret = wasm.pagedeathtrace_positions_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    positions_ptr() {
        const ret = wasm.pagedeathtrace_positions_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageDeathTrace.prototype[Symbol.dispose] = PageDeathTrace.prototype.free;

export class PageFeet {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageFeetFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagefeet_free(ptr, 0);
    }
    /**
     * @param {string} content
     * @returns {number}
     */
    add(content) {
        const ptr0 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagefeet_add(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    frame() {
        wasm.pagefeet_frame(this.__wbg_ptr);
    }
    /**
     * @param {string} content
     * @returns {string}
     */
    static heights(content) {
        let deferred3_0;
        let deferred3_1;
        try {
            const ptr0 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.pagefeet_heights(ptr0, len0);
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
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagefeet_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagefeet_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagefeet_new();
        this.__wbg_ptr = ret;
        PageFeetFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    output_len() {
        const ret = wasm.pagefeet_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    output_ptr() {
        const ret = wasm.pagefeet_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageFeet.prototype[Symbol.dispose] = PageFeet.prototype.free;

export class PageGunnerView {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageGunnerViewFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagegunnerview_free(ptr, 0);
    }
    frame() {
        wasm.pagegunnerview_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagegunnerview_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagegunnerview_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagegunnerview_new();
        this.__wbg_ptr = ret;
        PageGunnerViewFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    resolve() {
        wasm.pagegunnerview_resolve(this.__wbg_ptr);
    }
    wall() {
        wasm.pagegunnerview_wall(this.__wbg_ptr);
    }
}
if (Symbol.dispose) PageGunnerView.prototype[Symbol.dispose] = PageGunnerView.prototype.free;

export class PageInterfaceBatch {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageInterfaceBatchFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pageinterfacebatch_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    commands_ptr() {
        const ret = wasm.pageinterfacebatch_commands_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    dirty_ptr() {
        const ret = wasm.pageinterfacebatch_dirty_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_ptr() {
        const ret = wasm.pageinterfacebatch_frame_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    mask_ptr() {
        const ret = wasm.pageinterfacebatch_mask_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pageinterfacebatch_new();
        this.__wbg_ptr = ret;
        PageInterfaceBatchFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    position_ptr() {
        const ret = wasm.pageinterfacebatch_position_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    step() {
        wasm.pageinterfacebatch_step(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    tint_ptr() {
        const ret = wasm.pageinterfacebatch_tint_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    uv_ptr() {
        const ret = wasm.pageinterfacebatch_uv_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageInterfaceBatch.prototype[Symbol.dispose] = PageInterfaceBatch.prototype.free;

/**
 * Thin browser binding for the item phase: fixed blocks and four calls. The
 * page answers the rounds' ray and water requests between `present_items`
 * and `resume_items`, as many rounds as it allows a frame.
 */
export class PageItems {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageItemsFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pageitems_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    answers_len() {
        const ret = wasm.pageitems_answers_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    answers_ptr() {
        const ret = wasm.pageitems_answers_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    apply_item_records(count) {
        wasm.pageitems_apply_item_records(this.__wbg_ptr, count);
    }
    /**
     * @param {number} index
     */
    define_item(index) {
        wasm.pageitems_define_item(this.__wbg_ptr, index);
    }
    /**
     * @returns {number}
     */
    definitions_len() {
        const ret = wasm.pageitems_definitions_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    definitions_ptr() {
        const ret = wasm.pageitems_definitions_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_len() {
        const ret = wasm.pageitems_frame_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_ptr() {
        const ret = wasm.pageitems_frame_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    header_len() {
        const ret = wasm.pageitems_header_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    header_ptr() {
        const ret = wasm.pageitems_header_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pageitems_new();
        this.__wbg_ptr = ret;
        PageItemsFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    present_items() {
        wasm.pageitems_present_items(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    records_len() {
        const ret = wasm.pageitems_records_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    records_ptr() {
        const ret = wasm.pageitems_records_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    releases_len() {
        const ret = wasm.pageitems_releases_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    releases_ptr() {
        const ret = wasm.pageitems_releases_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    requests_len() {
        const ret = wasm.pageitems_requests_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    requests_ptr() {
        const ret = wasm.pageitems_requests_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} epoch
     */
    reset_items(epoch) {
        wasm.pageitems_reset_items(this.__wbg_ptr, epoch);
    }
    /**
     * @param {boolean} abandon
     */
    resume_items(abandon) {
        wasm.pageitems_resume_items(this.__wbg_ptr, abandon);
    }
    /**
     * @returns {number}
     */
    ripples_len() {
        const ret = wasm.pageitems_ripples_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    ripples_ptr() {
        const ret = wasm.pageitems_ripples_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    views_len() {
        const ret = wasm.pageitems_views_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    views_ptr() {
        const ret = wasm.pageitems_views_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageItems.prototype[Symbol.dispose] = PageItems.prototype.free;

export class PageJetVolume {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageJetVolumeFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagejetvolume_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    arrays_len() {
        const ret = wasm.pagejetvolume_arrays_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    arrays_ptr() {
        const ret = wasm.pagejetvolume_arrays_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    frame() {
        wasm.pagejetvolume_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagejetvolume_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagejetvolume_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagejetvolume_new();
        this.__wbg_ptr = ret;
        PageJetVolumeFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    nozzles_len() {
        const ret = wasm.pagejetvolume_nozzles_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    nozzles_ptr() {
        const ret = wasm.pagejetvolume_nozzles_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageJetVolume.prototype[Symbol.dispose] = PageJetVolume.prototype.free;

export class PageJetpack {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageJetpackFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagejetpack_free(ptr, 0);
    }
    /**
     * @param {Function} random
     */
    frame(random) {
        wasm.pagejetpack_frame(this.__wbg_ptr, random);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagejetpack_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagejetpack_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    local_len() {
        const ret = wasm.pagejetpack_local_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    local_ptr() {
        const ret = wasm.pagejetpack_local_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} content
     */
    constructor(content) {
        const ptr0 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagejetpack_new(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        PageJetpackFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    nozzles_len() {
        const ret = wasm.pagejetpack_nozzles_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    nozzles_ptr() {
        const ret = wasm.pagejetpack_nozzles_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    volume_arrays_len() {
        const ret = wasm.pagejetpack_volume_arrays_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    volume_arrays_ptr() {
        const ret = wasm.pagejetpack_volume_arrays_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    volume_input_len() {
        const ret = wasm.pagejetpack_volume_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    volume_input_ptr() {
        const ret = wasm.pagejetpack_volume_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    wake_input_len() {
        const ret = wasm.pagejetpack_wake_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    wake_input_ptr() {
        const ret = wasm.pagejetpack_wake_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    wake_particles_len() {
        const ret = wasm.pagejetpack_wake_particles_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    wake_particles_ptr() {
        const ret = wasm.pagejetpack_wake_particles_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageJetpack.prototype[Symbol.dispose] = PageJetpack.prototype.free;

export class PageLightning {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageLightningFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagelightning_free(ptr, 0);
    }
    /**
     * @param {number} id
     * @param {string} data
     * @returns {number}
     */
    actor(id, data) {
        const val2 = +id;
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagelightning_actor(this.__wbg_ptr, val2, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} def
     * @param {boolean} classic
     * @param {boolean} enhanced
     * @param {any} host
     * @returns {number}
     */
    add(def, classic, enhanced, host) {
        const ret = wasm.pagelightning_add(this.__wbg_ptr, def, classic, enhanced, host);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    aim_ptr() {
        const ret = wasm.pagelightning_aim_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    capsules_ptr() {
        const ret = wasm.pagelightning_capsules_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    clear() {
        wasm.pagelightning_clear(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    colors_ptr() {
        const ret = wasm.pagelightning_colors_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    counts_ptr() {
        const ret = wasm.pagelightning_counts_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} data
     * @returns {number}
     */
    definition(data) {
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagelightning_definition(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {string} data
     * @param {any} host
     * @returns {string}
     */
    fixture(data, host) {
        let deferred3_0;
        let deferred3_1;
        try {
            const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.pagelightning_fixture(this.__wbg_ptr, ptr0, len0, host);
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
     * @returns {number}
     */
    follows_ptr() {
        const ret = wasm.pagelightning_follows_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    frame_ptr() {
        const ret = wasm.pagelightning_frame_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    held_ptr() {
        const ret = wasm.pagelightning_held_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    indices_ptr() {
        const ret = wasm.pagelightning_indices_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {string} data
     */
    inject_shock(id, data) {
        const val2 = +id;
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagelightning_inject_shock(this.__wbg_ptr, val2, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {string}
     */
    inspect() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.pagelightning_inspect(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    joints_ptr() {
        const ret = wasm.pagelightning_joints_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    lights_ptr() {
        const ret = wasm.pagelightning_lights_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagelightning_new();
        this.__wbg_ptr = ret;
        PageLightningFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    nodes_len() {
        const ret = wasm.pagelightning_nodes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    nodes_ptr() {
        const ret = wasm.pagelightning_nodes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    output_ptr() {
        const ret = wasm.pagelightning_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    positions_ptr() {
        const ret = wasm.pagelightning_positions_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    prepare() {
        wasm.pagelightning_prepare(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    readers_ptr() {
        const ret = wasm.pagelightning_readers_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    shocks_output_ptr() {
        const ret = wasm.pagelightning_shocks_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    spawn_ptr() {
        const ret = wasm.pagelightning_spawn_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {any} host
     */
    step(host) {
        wasm.pagelightning_step(this.__wbg_ptr, host);
    }
    /**
     * @returns {number}
     */
    unit_order_ptr() {
        const ret = wasm.pagelightning_unit_order_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    units_ptr() {
        const ret = wasm.pagelightning_units_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    used_ptr() {
        const ret = wasm.pagelightning_used_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    uvs_ptr() {
        const ret = wasm.pagelightning_uvs_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    view_capsules_ptr() {
        const ret = wasm.pagelightning_view_capsules_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageLightning.prototype[Symbol.dispose] = PageLightning.prototype.free;

export class PageLights {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageLightsFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagelights_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    commands_ptr() {
        const ret = wasm.pagelights_commands_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    frame() {
        wasm.pagelights_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    handles_ptr() {
        const ret = wasm.pagelights_handles_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    header_ptr() {
        const ret = wasm.pagelights_header_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagelights_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    constructor(count) {
        const ret = wasm.pagelights_new(count);
        this.__wbg_ptr = ret;
        PageLightsFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    output_ptr() {
        const ret = wasm.pagelights_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageLights.prototype[Symbol.dispose] = PageLights.prototype.free;

/**
 * Packet bytes and outbound input share the display module's memory. The
 * expensive receive/ring reservation is lazy: an encode-only host needs none.
 */
export class PageNetwork {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageNetworkFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagenetwork_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    bytes_ptr() {
        const ret = wasm.pagenetwork_bytes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} tick
     * @returns {string}
     */
    capture(tick) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.pagenetwork_capture(this.__wbg_ptr, tick);
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
    capture_publication() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.pagenetwork_capture_publication(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {number} id
     * @returns {string}
     */
    capture_record(id) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.pagenetwork_capture_record(this.__wbg_ptr, id);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    close() {
        wasm.pagenetwork_close(this.__wbg_ptr);
    }
    /**
     * @param {number} op
     * @param {number} length
     * @returns {number}
     */
    encode(op, length) {
        const ret = wasm.pagenetwork_encode(this.__wbg_ptr, op, length);
        return ret;
    }
    /**
     * @param {number} id
     * @param {number} length
     * @param {number} tick
     * @param {number} edges
     * @param {number} interval
     * @returns {number}
     */
    encode_outbound(id, length, tick, edges, interval) {
        const ret = wasm.pagenetwork_encode_outbound(this.__wbg_ptr, id, length, tick, edges, interval);
        return ret;
    }
    /**
     * @returns {number}
     */
    encoded_ptr() {
        const ret = wasm.pagenetwork_encoded_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    header_ptr() {
        const ret = wasm.pagenetwork_header_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} op
     * @param {number} length
     * @returns {number}
     */
    inspect(op, length) {
        const ret = wasm.pagenetwork_inspect(this.__wbg_ptr, op, length);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Cold fixture inspection uses the same numeric readers. Its result
     * stream is absent from live receive and display publication.
     * @returns {number}
     */
    inspection_ptr() {
        const ret = wasm.pagenetwork_inspection_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    lease_changes_ptr() {
        const ret = wasm.pagenetwork_lease_changes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} kind
     * @param {number} id
     * @returns {number}
     */
    lookup(kind, id) {
        const ret = wasm.pagenetwork_lookup(this.__wbg_ptr, kind, id);
        return ret;
    }
    /**
     * @returns {number}
     */
    mount_frame_ptr() {
        const ret = wasm.pagenetwork_mount_frame_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    mount_input_ptr() {
        const ret = wasm.pagenetwork_mount_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    mount_output_ptr() {
        const ret = wasm.pagenetwork_mount_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagenetwork_new();
        this.__wbg_ptr = ret;
        PageNetworkFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    numbers_ptr() {
        const ret = wasm.pagenetwork_numbers_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    outbound_actions_ptr() {
        const ret = wasm.pagenetwork_outbound_actions_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    outbound_aim(count) {
        wasm.pagenetwork_outbound_aim(this.__wbg_ptr, count);
    }
    /**
     * @param {number} id
     * @param {number} length
     * @param {number} count
     * @returns {boolean}
     */
    outbound_batch(id, length, count) {
        const ret = wasm.pagenetwork_outbound_batch(this.__wbg_ptr, id, length, count);
        return ret !== 0;
    }
    /**
     * @param {number} id
     * @returns {string}
     */
    outbound_capture(id) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.pagenetwork_outbound_capture(this.__wbg_ptr, id);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {number} id
     */
    outbound_drop(id) {
        wasm.pagenetwork_outbound_drop(this.__wbg_ptr, id);
    }
    /**
     * @returns {number}
     */
    outbound_fires_ptr() {
        const ret = wasm.pagenetwork_outbound_fires_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    outbound_new() {
        const ret = wasm.pagenetwork_outbound_new(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    outbound_results_ptr() {
        const ret = wasm.pagenetwork_outbound_results_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @param {number} op
     * @param {number} length
     * @returns {boolean}
     */
    outbound_run(id, op, length) {
        const ret = wasm.pagenetwork_outbound_run(this.__wbg_ptr, id, op, length);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    payloads_ptr() {
        const ret = wasm.pagenetwork_payloads_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} tick
     * @param {number} player
     * @param {number} vehicle
     */
    present(tick, player, vehicle) {
        wasm.pagenetwork_present(this.__wbg_ptr, tick, player, vehicle);
    }
    present_view() {
        wasm.pagenetwork_present_view(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    preview_commands_ptr() {
        const ret = wasm.pagenetwork_preview_commands_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    preview_frame_ptr() {
        const ret = wasm.pagenetwork_preview_frame_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    preview_looks_ptr() {
        const ret = wasm.pagenetwork_preview_looks_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    preview_output_ptr() {
        const ret = wasm.pagenetwork_preview_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    preview_setup() {
        wasm.pagenetwork_preview_setup(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    publications_ptr() {
        const ret = wasm.pagenetwork_publications_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} length
     * @returns {boolean}
     */
    receive(length) {
        const ret = wasm.pagenetwork_receive(this.__wbg_ptr, length);
        return ret !== 0;
    }
    receive_setup() {
        wasm.pagenetwork_receive_setup(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    records_ptr() {
        const ret = wasm.pagenetwork_records_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     */
    remove(id) {
        wasm.pagenetwork_remove(this.__wbg_ptr, id);
    }
    /**
     * @param {number} epoch
     */
    reset(epoch) {
        wasm.pagenetwork_reset(this.__wbg_ptr, epoch);
    }
    /**
     * @returns {number}
     */
    seats_ptr() {
        const ret = wasm.pagenetwork_seats_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} x
     * @param {number} dx
     * @param {number} z
     * @param {number} dz
     * @param {number} y
     * @param {number} dy
     */
    set_bounds(x, dx, z, dz, y, dy) {
        wasm.pagenetwork_set_bounds(this.__wbg_ptr, x, dx, z, dz, y, dy);
    }
    /**
     * @returns {number}
     */
    smooth_commands_ptr() {
        const ret = wasm.pagenetwork_smooth_commands_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    smooth_input_ptr() {
        const ret = wasm.pagenetwork_smooth_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    smooth_output_ptr() {
        const ret = wasm.pagenetwork_smooth_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {string}
     */
    take_network_error() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.pagenetwork_take_network_error(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    timeline_input_ptr() {
        const ret = wasm.pagenetwork_timeline_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} maximum
     * @param {number} hz
     * @param {number} extrapolate
     */
    timeline_options(maximum, hz, extrapolate) {
        wasm.pagenetwork_timeline_options(this.__wbg_ptr, maximum, hz, extrapolate);
    }
    /**
     * @returns {number}
     */
    timeline_output_ptr() {
        const ret = wasm.pagenetwork_timeline_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    timeline_push() {
        wasm.pagenetwork_timeline_push(this.__wbg_ptr);
    }
    timeline_setup() {
        wasm.pagenetwork_timeline_setup(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    words_ptr() {
        const ret = wasm.pagenetwork_words_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageNetwork.prototype[Symbol.dispose] = PageNetwork.prototype.free;

/**
 * Thin browser binding for the ordnance phase: fixed buffers the page
 * fills and reads, one `present` per frame.
 */
export class PageOrdnance {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageOrdnanceFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pageordnance_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    batch_handle() {
        const ret = wasm.pageordnance_batch_handle(this.__wbg_ptr);
        return ret;
    }
    /**
     * @param {number} attribute
     * @returns {number}
     */
    blaster_len(attribute) {
        const ret = wasm.pageordnance_blaster_len(this.__wbg_ptr, attribute);
        return ret >>> 0;
    }
    /**
     * The four blaster attributes (head, tail, colour, size).
     * @param {number} attribute
     * @returns {number}
     */
    blaster_ptr(attribute) {
        const ret = wasm.pageordnance_blaster_ptr(this.__wbg_ptr, attribute);
        return ret >>> 0;
    }
    /**
     * @param {boolean} perspective
     * @param {number} fov
     * @param {number} zoom
     * @param {number} height
     * @param {number} previous
     * @returns {number}
     */
    static bolt_view(perspective, fov, zoom, height, previous) {
        const ret = wasm.pageordnance_bolt_view(perspective, fov, zoom, height, previous);
        return ret;
    }
    /**
     * @returns {number}
     */
    calls_len() {
        const ret = wasm.pageordnance_calls_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    calls_ptr() {
        const ret = wasm.pageordnance_calls_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     */
    capture_style(index) {
        wasm.pageordnance_capture_style(this.__wbg_ptr, index);
    }
    /**
     * @returns {number}
     */
    counts_ptr() {
        const ret = wasm.pageordnance_counts_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    flush_net(count) {
        wasm.pageordnance_flush_net(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    header_ptr() {
        const ret = wasm.pageordnance_header_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {boolean} bolts
     * @param {boolean} sniper
     */
    host_begin(bolts, sniper) {
        wasm.pageordnance_host_begin(this.__wbg_ptr, bolts, sniper);
    }
    /**
     * @param {string} data
     * @returns {number}
     */
    host_class(data) {
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageordnance_host_class(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0];
    }
    /**
     * @returns {number}
     */
    host_count() {
        const ret = wasm.pageordnance_host_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    host_len() {
        const ret = wasm.pageordnance_host_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    host_ptr() {
        const ret = wasm.pageordnance_host_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    host_record() {
        const ret = wasm.pageordnance_host_record(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {boolean}
     */
    host_report() {
        const ret = wasm.pageordnance_host_report(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pageordnance_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pageordnance_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    net_allocate() {
        const ret = wasm.pageordnance_net_allocate(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    net_count() {
        const ret = wasm.pageordnance_net_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {boolean}
     */
    net_full() {
        const ret = wasm.pageordnance_net_full(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    net_len() {
        const ret = wasm.pageordnance_net_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    net_ptr() {
        const ret = wasm.pageordnance_net_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} slot
     */
    net_release(slot) {
        wasm.pageordnance_net_release(this.__wbg_ptr, slot);
    }
    /**
     * @param {string} kind
     * @returns {number}
     */
    static net_role(kind) {
        const ptr0 = passStringToWasm0(kind, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageordnance_net_role(ptr0, len0);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    net_row() {
        const ret = wasm.pageordnance_net_row(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} at
     * @param {number} slot
     * @param {number} style
     * @param {number} flags
     * @param {string} data
     */
    net_sample(at, slot, style, flags, data) {
        const val2 = +at;
        const val3 = +slot;
        const val4 = +style;
        const val5 = +flags;
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageordnance_net_sample(this.__wbg_ptr, val2, val3, val4, val5, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    constructor() {
        const ret = wasm.pageordnance_new();
        this.__wbg_ptr = ret;
        PageOrdnanceFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    present_ordnance() {
        wasm.pageordnance_present_ordnance(this.__wbg_ptr);
    }
    /**
     * @param {number} attribute
     * @returns {number}
     */
    quad_len(attribute) {
        const ret = wasm.pageordnance_quad_len(this.__wbg_ptr, attribute);
        return ret >>> 0;
    }
    /**
     * The six particle attributes (position, colour, shape, axis, axis2,
     * frame) of batch handle regions, by attribute index.
     * @param {number} attribute
     * @returns {number}
     */
    quad_ptr(attribute) {
        const ret = wasm.pageordnance_quad_ptr(this.__wbg_ptr, attribute);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     */
    set_style(index) {
        wasm.pageordnance_set_style(this.__wbg_ptr, index);
    }
    /**
     * @param {Float64Array | number[]} rgba
     * @returns {Float64Array}
     */
    static sheath(rgba) {
        const ptr0 = passArrayF64ToWasm0(rgba, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageordnance_sheath(ptr0, len0);
        var v2 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v2;
    }
    /**
     * @param {number} slot
     * @returns {number}
     */
    source_token(slot) {
        const ret = wasm.pageordnance_source_token(this.__wbg_ptr, slot);
        return ret;
    }
    /**
     * @param {number} index
     * @param {number} core
     * @param {number} glow
     * @param {number} beam
     */
    style_batches(index, core, glow, beam) {
        wasm.pageordnance_style_batches(this.__wbg_ptr, index, core, glow, beam);
    }
    /**
     * @param {number} index
     * @param {Uint8Array | number[]} pixels
     */
    style_pixels(index, pixels) {
        const val2 = +index;
        const ptr0 = passArray8ToWasm0(pixels, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.pageordnance_style_pixels(this.__wbg_ptr, val2, ptr0, len0);
    }
    /**
     * @returns {number}
     */
    style_ptr() {
        const ret = wasm.pageordnance_style_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @param {number} role
     * @param {boolean} enhanced
     * @returns {string}
     */
    style_request(index, role, enhanced) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.pageordnance_style_request(this.__wbg_ptr, index, role, enhanced);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {Uint8Array | number[]} pixels
     * @returns {Float64Array}
     */
    static texture_hue(pixels) {
        const ptr0 = passArray8ToWasm0(pixels, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageordnance_texture_hue(ptr0, len0);
        var v2 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v2;
    }
    /**
     * @param {Float64Array | number[]} rgb
     * @param {number} power
     * @returns {Float64Array}
     */
    static vivid(rgb, power) {
        const val2 = +power;
        const ptr0 = passArrayF64ToWasm0(rgb, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageordnance_vivid(ptr0, len0, val2);
        var v3 = getArrayF64FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 8, 8);
        return v3;
    }
}
if (Symbol.dispose) PageOrdnance.prototype[Symbol.dispose] = PageOrdnance.prototype.free;

export class PagePostEffects {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PagePostEffectsFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pageposteffects_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    art_len() {
        const ret = wasm.pageposteffects_art_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    art_ptr() {
        const ret = wasm.pageposteffects_art_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    beam_indices_len() {
        const ret = wasm.pageposteffects_beam_indices_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    beam_indices_ptr() {
        const ret = wasm.pageposteffects_beam_indices_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    beam_normals_len() {
        const ret = wasm.pageposteffects_beam_normals_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    beam_normals_ptr() {
        const ret = wasm.pageposteffects_beam_normals_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    beam_positions_len() {
        const ret = wasm.pageposteffects_beam_positions_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    beam_positions_ptr() {
        const ret = wasm.pageposteffects_beam_positions_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    beam_uv_len() {
        const ret = wasm.pageposteffects_beam_uv_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    beam_uv_ptr() {
        const ret = wasm.pageposteffects_beam_uv_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    counts_len() {
        const ret = wasm.pageposteffects_counts_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    counts_ptr() {
        const ret = wasm.pageposteffects_counts_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     * @param {Function} random
     */
    frame(count, random) {
        wasm.pageposteffects_frame(this.__wbg_ptr, count, random);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pageposteffects_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pageposteffects_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} content
     */
    constructor(content) {
        const ptr0 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageposteffects_new(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        PagePostEffectsFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @param {number} count
     */
    remaining(count) {
        wasm.pageposteffects_remaining(this.__wbg_ptr, count);
    }
}
if (Symbol.dispose) PagePostEffects.prototype[Symbol.dispose] = PagePostEffects.prototype.free;

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
     * @param {string} data
     * @returns {number}
     */
    add_turret(data) {
        const ptr0 = passStringToWasm0(data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageprops_add_turret(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {PageUnits} units
     * @param {number} actor
     * @returns {number}
     */
    add_unit_rider(units, actor) {
        _assertClass(units, PageUnits);
        const ret = wasm.pageprops_add_unit_rider(this.__wbg_ptr, units.__wbg_ptr, actor);
        return ret;
    }
    /**
     * @returns {number}
     */
    bodies_at() {
        const ret = wasm.pageprops_bodies_at(this.__wbg_ptr);
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
    /**
     * @returns {number}
     */
    nodes_len() {
        const ret = wasm.pageprops_nodes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    nodes_ptr() {
        const ret = wasm.pageprops_nodes_ptr(this.__wbg_ptr);
        return ret >>> 0;
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
    /**
     * @returns {number}
     */
    turret_input_ptr() {
        const ret = wasm.pageprops_turret_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageProps.prototype[Symbol.dispose] = PageProps.prototype.free;

export class PageShellMusic {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageShellMusicFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pageshellmusic_free(ptr, 0);
    }
    frame() {
        wasm.pageshellmusic_frame(this.__wbg_ptr);
    }
    /**
     * @param {string} groups
     */
    constructor(groups) {
        const ptr0 = passStringToWasm0(groups, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pageshellmusic_new(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        PageShellMusicFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    row_ptr() {
        const ret = wasm.pageshellmusic_row_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageShellMusic.prototype[Symbol.dispose] = PageShellMusic.prototype.free;

export class PageShield {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageShieldFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pageshield_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    blasts_len() {
        const ret = wasm.pageshield_blasts_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    blasts_ptr() {
        const ret = wasm.pageshield_blasts_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    frame() {
        wasm.pageshield_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    impacts_len() {
        const ret = wasm.pageshield_impacts_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    impacts_ptr() {
        const ret = wasm.pageshield_impacts_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pageshield_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pageshield_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pageshield_new();
        this.__wbg_ptr = ret;
        PageShieldFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    powers_len() {
        const ret = wasm.pageshield_powers_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    powers_ptr() {
        const ret = wasm.pageshield_powers_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageShield.prototype[Symbol.dispose] = PageShield.prototype.free;

export class PageSniper {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageSniperFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagesniper_free(ptr, 0);
    }
    collect() {
        wasm.pagesniper_collect(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    counts_len() {
        const ret = wasm.pagesniper_counts_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    counts_ptr() {
        const ret = wasm.pagesniper_counts_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    events_len() {
        const ret = wasm.pagesniper_events_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    events_ptr() {
        const ret = wasm.pagesniper_events_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    flares_len() {
        const ret = wasm.pagesniper_flares_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    flares_ptr() {
        const ret = wasm.pagesniper_flares_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    frame() {
        wasm.pagesniper_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagesniper_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagesniper_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} count
     */
    match_impacts(count) {
        wasm.pagesniper_match_impacts(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    matches_len() {
        const ret = wasm.pagesniper_matches_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    matches_ptr() {
        const ret = wasm.pagesniper_matches_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagesniper_new();
        this.__wbg_ptr = ret;
        PageSniperFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    streaks_len() {
        const ret = wasm.pagesniper_streaks_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    streaks_ptr() {
        const ret = wasm.pagesniper_streaks_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageSniper.prototype[Symbol.dispose] = PageSniper.prototype.free;

export class PageSoldierView {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageSoldierViewFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagesoldierview_free(ptr, 0);
    }
    frame() {
        wasm.pagesoldierview_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagesoldierview_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagesoldierview_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagesoldierview_new();
        this.__wbg_ptr = ret;
        PageSoldierViewFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
}
if (Symbol.dispose) PageSoldierView.prototype[Symbol.dispose] = PageSoldierView.prototype.free;

export class PageTrail {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageTrailFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagetrail_free(ptr, 0);
    }
    /**
     * @param {Function} random
     */
    frame(random) {
        wasm.pagetrail_frame(this.__wbg_ptr, random);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagetrail_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagetrail_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} content
     */
    constructor(content) {
        const ptr0 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagetrail_new(ptr0, len0);
        this.__wbg_ptr = ret;
        PageTrailFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    probe() {
        wasm.pagetrail_probe(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    rays_len() {
        const ret = wasm.pagetrail_rays_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    rays_ptr() {
        const ret = wasm.pagetrail_rays_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    records_len() {
        const ret = wasm.pagetrail_records_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    records_ptr() {
        const ret = wasm.pagetrail_records_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    roof() {
        wasm.pagetrail_roof(this.__wbg_ptr);
    }
}
if (Symbol.dispose) PageTrail.prototype[Symbol.dispose] = PageTrail.prototype.free;

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
     * @param {PageNetwork} network
     * @param {number} actor
     * @param {number} id
     * @param {number} generation
     * @param {number} kind
     */
    bind_network(network, actor, id, generation, kind) {
        _assertClass(network, PageNetwork);
        wasm.pageunits_bind_network(this.__wbg_ptr, network.__wbg_ptr, actor, id, generation, kind);
    }
    /**
     * Body display keeps native references to the already-retained sources.
     * @param {PageVehicles} vehicles
     * @param {PageProps} props
     */
    bind_sources(vehicles, props) {
        _assertClass(vehicles, PageVehicles);
        _assertClass(props, PageProps);
        wasm.pageunits_bind_sources(this.__wbg_ptr, vehicles.__wbg_ptr, props.__wbg_ptr);
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
     * @param {number} count
     */
    collide_units(count) {
        wasm.pageunits_collide_units(this.__wbg_ptr, count);
    }
    /**
     * @returns {number}
     */
    collision_actors_len() {
        const ret = wasm.pageunits_collision_actors_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    collision_actors_ptr() {
        const ret = wasm.pageunits_collision_actors_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    collision_input_len() {
        const ret = wasm.pageunits_collision_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    collision_input_ptr() {
        const ret = wasm.pageunits_collision_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    collision_output_len() {
        const ret = wasm.pageunits_collision_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    collision_output_ptr() {
        const ret = wasm.pageunits_collision_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    collision_shapes_len() {
        const ret = wasm.pageunits_collision_shapes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    collision_shapes_ptr() {
        const ret = wasm.pageunits_collision_shapes_ptr(this.__wbg_ptr);
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
    /**
     * @returns {number}
     */
    network_states_len() {
        const ret = wasm.pageunits_network_states_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    network_states_ptr() {
        const ret = wasm.pageunits_network_states_ptr(this.__wbg_ptr);
        return ret >>> 0;
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
    /**
     * @param {number} count
     */
    prepare_aim(count) {
        wasm.pageunits_prepare_aim(this.__wbg_ptr, count);
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
     * @param {number} actor
     */
    unbind_network(actor) {
        wasm.pageunits_unbind_network(this.__wbg_ptr, actor);
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

export class PageVehicleEffects {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageVehicleEffectsFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagevehicleeffects_free(ptr, 0);
    }
    /**
     * @param {string} content
     * @returns {number}
     */
    add(content) {
        const ptr0 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.pagevehicleeffects_add(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @param {number} id
     */
    carrier(id) {
        wasm.pagevehicleeffects_carrier(this.__wbg_ptr, id);
    }
    complete() {
        wasm.pagevehicleeffects_complete(this.__wbg_ptr);
    }
    frame() {
        wasm.pagevehicleeffects_frame(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    header_len() {
        const ret = wasm.pagevehicleeffects_header_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    header_ptr() {
        const ret = wasm.pagevehicleeffects_header_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagevehicleeffects_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagevehicleeffects_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    inspect() {
        wasm.pagevehicleeffects_inspect(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    inspection_len() {
        const ret = wasm.pagevehicleeffects_inspection_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    inspection_ptr() {
        const ret = wasm.pagevehicleeffects_inspection_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagevehicleeffects_new();
        this.__wbg_ptr = ret;
        PageVehicleEffectsFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    node_start(id) {
        const ret = wasm.pagevehicleeffects_node_start(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    nodes_len() {
        const ret = wasm.pagevehicleeffects_nodes_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    nodes_ptr() {
        const ret = wasm.pagevehicleeffects_nodes_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    order_len() {
        const ret = wasm.pagevehicleeffects_order_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    order_ptr() {
        const ret = wasm.pagevehicleeffects_order_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    output_len() {
        const ret = wasm.pagevehicleeffects_output_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    output_ptr() {
        const ret = wasm.pagevehicleeffects_output_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} id
     * @returns {number}
     */
    output_start(id) {
        const ret = wasm.pagevehicleeffects_output_start(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    resume() {
        wasm.pagevehicleeffects_resume(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    status_len() {
        const ret = wasm.pagevehicleeffects_status_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    status_ptr() {
        const ret = wasm.pagevehicleeffects_status_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageVehicleEffects.prototype[Symbol.dispose] = PageVehicleEffects.prototype.free;

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
     * @param {PageNetwork} network
     * @param {number} rig
     * @param {number} id
     * @param {number} generation
     * @param {number} kind
     */
    bind_network(network, rig, id, generation, kind) {
        _assertClass(network, PageNetwork);
        wasm.pagevehicles_bind_network(this.__wbg_ptr, network.__wbg_ptr, rig, id, generation, kind);
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
    /**
     * @returns {number}
     */
    network_states_len() {
        const ret = wasm.pagevehicles_network_states_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    network_states_ptr() {
        const ret = wasm.pagevehicles_network_states_ptr(this.__wbg_ptr);
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
     * @returns {number}
     */
    rays_len() {
        const ret = wasm.pagevehicles_rays_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The frame's CastRay block (VehicleHeader.RAYS records in use).
     * @returns {number}
     */
    rays_ptr() {
        const ret = wasm.pagevehicles_rays_ptr(this.__wbg_ptr);
        return ret >>> 0;
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
    /**
     * @param {number} rig
     */
    unbind_network(rig) {
        wasm.pagevehicles_unbind_network(this.__wbg_ptr, rig);
    }
}
if (Symbol.dispose) PageVehicles.prototype[Symbol.dispose] = PageVehicles.prototype.free;

export class PageWake {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageWakeFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagewake_free(ptr, 0);
    }
    /**
     * @param {Function} random
     */
    frame(random) {
        wasm.pagewake_frame(this.__wbg_ptr, random);
    }
    /**
     * @returns {number}
     */
    input_len() {
        const ret = wasm.pagewake_input_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    input_ptr() {
        const ret = wasm.pagewake_input_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    constructor() {
        const ret = wasm.pagewake_new();
        this.__wbg_ptr = ret;
        PageWakeFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    nozzles_len() {
        const ret = wasm.pagewake_nozzles_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    nozzles_ptr() {
        const ret = wasm.pagewake_nozzles_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    particles_len() {
        const ret = wasm.pagewake_particles_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    particles_ptr() {
        const ret = wasm.pagewake_particles_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageWake.prototype[Symbol.dispose] = PageWake.prototype.free;

export class PageWarning {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        PageWarningFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_pagewarning_free(ptr, 0);
    }
    frame() {
        wasm.pagewarning_frame(this.__wbg_ptr);
    }
    constructor() {
        const ret = wasm.pagewarning_new();
        this.__wbg_ptr = ret;
        PageWarningFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    row_ptr() {
        const ret = wasm.pagewarning_row_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) PageWarning.prototype[Symbol.dispose] = PageWarning.prototype.free;
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_throw_4c88d8bf264fd984: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_call_6fe39e7352b5243d: function(arg0, arg1) {
            const ret = arg0.call(arg1);
            return ret;
        },
        __wbg_cast_4e16be1b530fcca5: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7) {
            const ret = arg0.cast(arg1, arg2, arg3, arg4, arg5, arg6, arg7);
            return ret;
        },
        __wbg_normal_3353539eda715d4c: function(arg0, arg1) {
            const ret = arg0.normal(arg1 >>> 0);
            return ret;
        },
        __wbg_random_298d5763e452935f: function() {
            const ret = Math.random();
            return ret;
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

const PageAnalyticEffectsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageanalyticeffects_free(ptr, 1));
const PageArcCameraFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagearccamera_free(ptr, 1));
const PageCombatFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagecombat_free(ptr, 1));
const PageDeathTraceFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagedeathtrace_free(ptr, 1));
const PageFeetFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagefeet_free(ptr, 1));
const PageGunnerViewFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagegunnerview_free(ptr, 1));
const PageInterfaceBatchFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageinterfacebatch_free(ptr, 1));
const PageItemsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageitems_free(ptr, 1));
const PageJetVolumeFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagejetvolume_free(ptr, 1));
const PageJetpackFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagejetpack_free(ptr, 1));
const PageLightningFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagelightning_free(ptr, 1));
const PageLightsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagelights_free(ptr, 1));
const PageNetworkFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagenetwork_free(ptr, 1));
const PageOrdnanceFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageordnance_free(ptr, 1));
const PagePostEffectsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageposteffects_free(ptr, 1));
const PagePresentationFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagepresentation_free(ptr, 1));
const PagePropsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageprops_free(ptr, 1));
const PageShellMusicFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageshellmusic_free(ptr, 1));
const PageShieldFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageshield_free(ptr, 1));
const PageSniperFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagesniper_free(ptr, 1));
const PageSoldierViewFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagesoldierview_free(ptr, 1));
const PageTrailFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagetrail_free(ptr, 1));
const PageUnitsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pageunits_free(ptr, 1));
const PageVehicleEffectsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagevehicleeffects_free(ptr, 1));
const PageVehiclesFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagevehicles_free(ptr, 1));
const PageWakeFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagewake_free(ptr, 1));
const PageWarningFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_pagewarning_free(ptr, 1));

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
}

function getArrayF64FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat64ArrayMemory0().subarray(ptr / 8, ptr / 8 + len);
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
    cachedFloat32ArrayMemory0 = null;
    cachedFloat64ArrayMemory0 = null;
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
