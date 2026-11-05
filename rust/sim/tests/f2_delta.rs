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
