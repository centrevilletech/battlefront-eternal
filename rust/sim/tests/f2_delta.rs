//! Generated publication masks retain numeric bits and variable-list layout.
use bfe_sim::store::block::HEADER;
use bfe_sim::vehicles::f2_delta::{Cache, MASK_BITS};
fn apply(cache: &Cache, row: &mut Vec<f64>) {
    let d = &cache.delta.value;
    row.resize(d.length as usize, 0.0); let mut value = d.values.iter();
    for (chunk,&mask) in d.mask.iter().enumerate() { for bit in 0..MASK_BITS {
        if mask & (1 << bit) != 0 { row[chunk*MASK_BITS+bit] = *value.next().unwrap(); }
    } }
    assert!(value.next().is_none());
}
#[test]
fn delta_masks_reconstruct_rows_without_recopied_fields() {
    let mut cache = Cache::new(256); let mut mirror = Vec::new();
    let write = |row: &mut [f64], heap: &mut Vec<f64>| { row[0] = -0.0; row[1] = f64::from_bits(0x7ff8000000000001); heap.extend([1.0,2.0,3.0]); };
    assert!(cache.encode(2,1,2,write)); apply(&cache,&mut mirror);
    assert_eq!(mirror.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),cache.last.words.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
    assert!(!cache.encode(2,1,2,write));
    assert!(cache.encode(2,1,2,|row,heap| { write(row,heap); row[0] = 0.0; }));
    assert_eq!(cache.delta.value.values.len(),1); apply(&cache,&mut mirror);
    assert_eq!(mirror[HEADER].to_bits(),0.0f64.to_bits());
    // A reused slab slot sends every initial word even if most values match.
    assert!(cache.encode(2,2,2,write)); assert_eq!(cache.delta.value.values.len(),cache.last.words.len());
}

// Movement observers retain the consumed combat heap while updating fixed
// generated fields. Reconstruct the sparse result, including unchanged NaNs.
#[test]
fn retained_field_updates_keep_heap_bits_and_reject_rebound_rows() {
    let mut cache=Cache::new(256);let mut mirror=Vec::new();
    assert!(cache.encode(2,1,2,|row,heap| {
        row[0]=-0.0;row[1]=17.0;heap.extend([f64::from_bits(0x7ff8000000000001),3.0]);
    }));
    apply(&cache,&mut mirror);
    assert_eq!(cache.prepare_retained(2,1,2,&[HEADER..HEADER+1],|row,_|row[0]=0.0),Some(true));
    apply(&cache,&mut mirror);cache.commit();
    assert_eq!(mirror.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),cache.last.words.iter().map(|v|v.to_bits()).collect::<Vec<_>>());
    assert_eq!(cache.prepare_retained(2,1,2,&[HEADER..HEADER+1],|row,_|row[0]=0.0),Some(false));
    assert_eq!(cache.prepare_retained(2,2,2,&[HEADER..HEADER+1],|_,_|panic!("rebound row")),None);
}

// A live publication crosses mask boundaries and ends in a partial SIMD pair.
// The fixed expected masks also cover a signed-zero word without float math.
#[test]
fn changed_words_keep_order_across_mask_boundaries() {
    let mut cache = Cache::new(128);
    let mut mirror = Vec::new();
    assert!(cache.encode(4, 2, 70, |row, _| {
        for (i, word) in row.iter_mut().enumerate() { *word = (i + 1) as f64; }
        row[0] = -0.0;
    }));
    apply(&cache, &mut mirror);
    assert!(cache.encode(4, 2, 70, |row, _| {
        for (i, word) in row.iter_mut().enumerate() { *word = (i + 1) as f64; }
        row[0] = 0.0;
        row[26] = 101.0; row[27] = 102.0;
        row[58] = 103.0; row[59] = 104.0; row[69] = 105.0;
    }));
    assert_eq!(cache.delta.value.mask, [0x80000020, 0x80000001, 0x401]);
    assert_eq!(cache.delta.value.values, [0.0, 101.0, 102.0, 103.0, 104.0, 105.0]);
    apply(&cache, &mut mirror);
    assert_eq!(mirror.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        cache.last.words.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
}

// A borrowed publication has the same mask and acknowledgement contract as
// packed values. Only the source of the changed words differs at the boundary.
#[test]
fn borrowed_publications_retain_unread_words_and_one_writer_heap() {
    let mut cache = Cache::borrowed(128);
    let heap = cache.next.heap.as_ptr();
    assert_eq!(cache.delta.value.values.capacity(), 0);
    assert!(cache.prepare(3, 1, 2, |row, heap| {
        row[0] = 7.0; row[1] = -0.0; heap.extend([17.0, 19.0]);
    }));
    assert_eq!(cache.delta.value.source, cache.next.ptr() as u32);
    assert_eq!(cache.borrowed_words(), 9);
    assert!(cache.delta.value.values.is_empty());
    cache.commit();
    assert_eq!(cache.next.heap.as_ptr(), heap);
    assert!(cache.prepare(3, 1, 2, |row, heap| {
        row[0] = 8.0; row[1] = -0.0; heap.extend([17.0, 19.0]);
    }));
    assert_eq!(cache.delta.value.mask, [1 << HEADER]);
    assert_eq!(cache.borrowed_words(), 1);
    // Another unpublished barrier compares against the same consumed row.
    assert!(!cache.prepare(3, 1, 2, |row, heap| {
        row[0] = 7.0; row[1] = -0.0; heap.extend([17.0, 19.0]);
    }));
}
