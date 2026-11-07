//! `Array.prototype.sort` and the TypedArray sorts, as V8 13.6 runs
//! them.
//!
//! `js_sort` is a transliteration of V8's `ArrayTimSort`
//! (`third_party/v8/builtins/array-sort.tq` in V8 13.6.233, itself a
//! port of CPython's listsort; the Python Software Foundation's
//! copyright in that file is noted in THIRD-PARTY-NOTICES.md). It is
//! ported function by function — binary insertion sort for short
//! runs, natural runs, the pending-runs stack with its two
//! invariants, galloping merges with the same `minGallop` dynamics —
//! so that not only stable order but also the result of an
//! inconsistent comparator (one whose answers contradict a total
//! order, as `a - b` does around Infinity and NaN) is V8's result.
//! Rust's `sort_by` is stable too, but it is a different algorithm:
//! with an inconsistent comparator its output is its own, not V8's,
//! and the sim has sorts whose ties and edge cases are load-bearing.
//!
//! A comparator is a JavaScript comparefn: it returns a Number, of
//! which only the sign matters, and NaN counts as +0
//! (SortCompareUserFn). It must not mutate the array being sorted:
//! V8 sorts a compacted snapshot, so such mutation is unobservable
//! there except through the receiver afterwards, and no sim
//! comparator does it.
//!
//! Densely typed Rust slices have no holes or `undefined`; where a
//! ported array can hold them, [`js_sort_slots`] applies the
//! compaction V8 does first (values sorted, then the undefineds,
//! then the holes) — see `CompactReceiverElementsIntoWorkArray` and
//! `CopyWorkArrayToReceiver` in the Torque.
//!
//! TypedArrays sort differently in V8, and so here:
//! [`typed_sort`] with a comparator is V8's `TypedArrayMergeSort`
//! (a plain recursive mergesort between two buffers), and the
//! no-comparator numeric sort is [`typed_sort_f64`]. V8's
//! no-comparator path is a C++ `std::sort` (`TypedArraySortFast`)
//! under its `CompareNum`, which ranks -0 below +0 and numbers
//! below NaN: `typed_sort_f64` runs that same comparison through
//! the TimSort above. std::sort is not stable, so the one corner
//! left is the order of NaNs among themselves (equal under
//! CompareNum, distinguishable only by payload); every other
//! element's place is determined by its value.

use std::cmp::Ordering;

const K_MIN_GALLOP_WINS: i64 = 7;
const K_MAX_MERGE_PENDING: usize = 85;

/// One slot of a JavaScript array that may be sparse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsSlot<T> {
    /// A hole (no property at the index).
    Hole,
    /// The value `undefined`.
    Undefined,
    /// A value.
    Value(T),
}

/// The sign of a comparefn result, with SortCompare's rule that NaN
/// is +0.
#[inline(always)]
fn normalize_order(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v }
}

struct TimSort<'a, T, F: FnMut(&T, &T) -> f64> {
    a: &'a mut Vec<T>,
    cmp: &'a mut F,
    tmp: Vec<T>,
    min_gallop: i64,
    runs: Vec<(i64, i64)>, // (base, length), the pending-runs stack
}

impl<T: Clone, F: FnMut(&T, &T) -> f64> TimSort<'_, T, F> {
    #[inline(always)]
    fn compare(&mut self, x: &T, y: &T) -> f64 {
        normalize_order((self.cmp)(x, y))
    }

    fn at(&self, i: i64) -> T {
        self.a[i as usize].clone()
    }

    fn tmp_at(&self, i: i64) -> T {
        self.tmp[i as usize].clone()
    }

    fn copy_range(&mut self, src: i64, dst: i64, len: i64) {
        // Copy builtin: backwards when the ranges might overlap that
        // way (same array, src < dst), forwards otherwise.
        if src < dst {
            let mut s = src + len - 1;
            let mut d = dst + len - 1;
            while s >= src {
                let v = self.at(s);
                self.a[d as usize] = v;
                s -= 1;
                d -= 1;
            }
        } else {
            for k in 0..len {
                let v = self.at(src + k);
                self.a[(dst + k) as usize] = v;
            }
        }
    }

    fn copy_to_tmp(&mut self, src: i64, len: i64) {
        self.tmp.clear();
        self.tmp.reserve(len as usize);
        for k in 0..len {
            let v = self.at(src + k);
            self.tmp.push(v);
        }
    }

    fn copy_from_tmp(&mut self, tmp_src: i64, dst: i64, len: i64) {
        for k in 0..len {
            let v = self.tmp_at(tmp_src + k);
            self.a[(dst + k) as usize] = v;
        }
    }

    fn reverse_range(&mut self, from: i64, to: i64) {
        let mut low = from;
        let mut high = to - 1;
        while low < high {
            self.a.swap(low as usize, high as usize);
            low += 1;
            high -= 1;
        }
    }

    // BinaryInsertionSort, array-sort.tq.
    fn binary_insertion_sort(&mut self, low: i64, start_arg: i64, high: i64) {
        let mut start = if low == start_arg { start_arg + 1 } else { start_arg };
        while start < high {
            let mut left = low;
            let mut right = start;
            let pivot = self.at(right);
            while left < right {
                let mid = left + ((right - left) >> 1);
                let mid_value = self.at(mid);
                let order = self.compare(&pivot, &mid_value);
                if order < 0.0 {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
            let mut p = start;
            while p > left {
                let v = self.at(p - 1);
                self.a[p as usize] = v;
                p -= 1;
            }
            self.a[left as usize] = pivot;
            start += 1;
        }
    }

    // CountAndMakeRun, array-sort.tq.
    fn count_and_make_run(&mut self, low_arg: i64, high: i64) -> i64 {
        let low = low_arg + 1;
        if low == high {
            return 1;
        }
        let mut run_length = 2i64;
        let element_low = self.at(low);
        let element_low_pred = self.at(low - 1);
        let mut order = self.compare(&element_low, &element_low_pred);
        let is_descending = order < 0.0;
        let mut previous = element_low;
        let mut idx = low + 1;
        while idx < high {
            let current = self.at(idx);
            order = self.compare(&current, &previous);
            if is_descending {
                if order >= 0.0 {
                    break;
                }
            } else if order < 0.0 {
                break;
            }
            previous = current;
            run_length += 1;
            idx += 1;
        }
        if is_descending {
            self.reverse_range(low_arg, low_arg + run_length);
        }
        run_length
    }

    // GallopLeft over `a` (or, with `in_tmp`, over the temp array),
    // array-sort.tq. Takes the value to read at an offset from the
    // chosen array.
    fn gallop_left(&mut self, in_tmp: bool, key: &T, base: i64, length: i64, hint: i64) -> i64 {
        let get = |s: &Self, i: i64| -> T {
            if in_tmp { s.tmp_at(i) } else { s.at(i) }
        };
        let mut last_ofs = 0i64;
        let mut offset = 1i64;
        let base_hint = get(self, base + hint);
        let mut order = self.compare(&base_hint, key);
        if order < 0.0 {
            let max_ofs = length - hint;
            while offset < max_ofs {
                let element = get(self, base + hint + offset);
                order = self.compare(&element, key);
                if order >= 0.0 {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
                if offset <= 0 {
                    offset = max_ofs;
                }
            }
            if offset > max_ofs {
                offset = max_ofs;
            }
            last_ofs += hint;
            offset += hint;
        } else {
            let max_ofs = hint + 1;
            while offset < max_ofs {
                let element = get(self, base + hint - offset);
                order = self.compare(&element, key);
                if order < 0.0 {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
                if offset <= 0 {
                    offset = max_ofs;
                }
            }
            if offset > max_ofs {
                offset = max_ofs;
            }
            let tmp = last_ofs;
            last_ofs = hint - offset;
            offset = hint - tmp;
        }
        last_ofs += 1;
        while last_ofs < offset {
            let m = last_ofs + ((offset - last_ofs) >> 1);
            let element = get(self, base + m);
            order = self.compare(&element, key);
            if order < 0.0 {
                last_ofs = m + 1;
            } else {
                offset = m;
            }
        }
        offset
    }

    // GallopRight, array-sort.tq.
    fn gallop_right(&mut self, in_tmp: bool, key: &T, base: i64, length: i64, hint: i64) -> i64 {
        let get = |s: &Self, i: i64| -> T {
            if in_tmp { s.tmp_at(i) } else { s.at(i) }
        };
        let mut last_ofs = 0i64;
        let mut offset = 1i64;
        let base_hint = get(self, base + hint);
        let mut order = self.compare(key, &base_hint);
        if order < 0.0 {
            let max_ofs = hint + 1;
            while offset < max_ofs {
                let element = get(self, base + hint - offset);
                order = self.compare(key, &element);
                if order >= 0.0 {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
                if offset <= 0 {
                    offset = max_ofs;
                }
            }
            if offset > max_ofs {
                offset = max_ofs;
            }
            let tmp = last_ofs;
            last_ofs = hint - offset;
            offset = hint - tmp;
        } else {
            let max_ofs = length - hint;
            while offset < max_ofs {
                let element = get(self, base + hint + offset);
                order = self.compare(key, &element);
                if order < 0.0 {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
                if offset <= 0 {
                    offset = max_ofs;
                }
            }
            if offset > max_ofs {
                offset = max_ofs;
            }
            last_ofs += hint;
            offset += hint;
        }
        last_ofs += 1;
        while last_ofs < offset {
            let m = last_ofs + ((offset - last_ofs) >> 1);
            let element = get(self, base + m);
            order = self.compare(key, &element);
            if order < 0.0 {
                offset = m;
            } else {
                last_ofs = m + 1;
            }
        }
        offset
    }

    // MergeLow, array-sort.tq. The Torque's try/labels become the
    // MergeEnd the body exits with.
    fn merge_low(&mut self, base_a: i64, length_a_arg: i64, base_b: i64, length_b_arg: i64) {
        let mut length_a = length_a_arg;
        let mut length_b = length_b_arg;
        self.copy_to_tmp(base_a, length_a);
        let mut dest = base_a;
        let mut cursor_tmp = 0i64;
        let mut cursor_b = base_b;
        let first_b = self.at(cursor_b);
        self.a[dest as usize] = first_b;
        dest += 1;
        cursor_b += 1;

        enum End {
            Succeed,
            CopyB,
        }
        let end = 'outer: {
            length_b -= 1;
            if length_b == 0 {
                break 'outer End::Succeed;
            }
            if length_a == 1 {
                break 'outer End::CopyB;
            }
            let mut min_gallop = self.min_gallop;
            'gallop: loop {
                let mut wins_a = 0i64;
                let mut wins_b = 0i64;
                loop {
                    let b_value = self.at(cursor_b);
                    let t_value = self.tmp_at(cursor_tmp);
                    let order = self.compare(&b_value, &t_value);
                    if order < 0.0 {
                        self.a[dest as usize] = b_value;
                        dest += 1;
                        cursor_b += 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 0 {
                            break 'outer End::Succeed;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    } else {
                        self.a[dest as usize] = t_value;
                        dest += 1;
                        cursor_tmp += 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 1 {
                            break 'outer End::CopyB;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= K_MIN_GALLOP_WINS || wins_b >= K_MIN_GALLOP_WINS || first {
                    first = false;
                    min_gallop = i64::max(1, min_gallop - 1);
                    self.min_gallop = min_gallop;
                    let key = self.at(cursor_b);
                    wins_a = self.gallop_right(true, &key, cursor_tmp, length_a, 0);
                    if wins_a > 0 {
                        self.copy_from_tmp(cursor_tmp, dest, wins_a);
                        dest += wins_a;
                        cursor_tmp += wins_a;
                        length_a -= wins_a;
                        if length_a == 1 {
                            break 'outer End::CopyB;
                        }
                        if length_a == 0 {
                            break 'outer End::Succeed;
                        }
                    }
                    let b_value = self.at(cursor_b);
                    self.a[dest as usize] = b_value;
                    dest += 1;
                    cursor_b += 1;
                    length_b -= 1;
                    if length_b == 0 {
                        break 'outer End::Succeed;
                    }
                    let key = self.tmp_at(cursor_tmp);
                    wins_b = self.gallop_left(false, &key, cursor_b, length_b, 0);
                    if wins_b > 0 {
                        self.copy_range(cursor_b, dest, wins_b);
                        dest += wins_b;
                        cursor_b += wins_b;
                        length_b -= wins_b;
                        if length_b == 0 {
                            break 'outer End::Succeed;
                        }
                    }
                    let t_value = self.tmp_at(cursor_tmp);
                    self.a[dest as usize] = t_value;
                    dest += 1;
                    cursor_tmp += 1;
                    length_a -= 1;
                    if length_a == 1 {
                        break 'outer End::CopyB;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
                continue 'gallop;
            }
        };
        match end {
            End::Succeed => {
                if length_a > 0 {
                    self.copy_from_tmp(cursor_tmp, dest, length_a);
                }
            }
            End::CopyB => {
                self.copy_range(cursor_b, dest, length_b);
                let last = self.tmp_at(cursor_tmp);
                self.a[(dest + length_b) as usize] = last;
            }
        }
    }

    // MergeHigh, array-sort.tq.
    fn merge_high(&mut self, base_a: i64, length_a_arg: i64, base_b: i64, length_b_arg: i64) {
        let mut length_a = length_a_arg;
        let mut length_b = length_b_arg;
        self.copy_to_tmp(base_b, length_b);
        let mut dest = base_b + length_b - 1;
        let mut cursor_tmp = length_b - 1;
        let mut cursor_a = base_a + length_a - 1;
        let first_a = self.at(cursor_a);
        self.a[dest as usize] = first_a;
        dest -= 1;
        cursor_a -= 1;

        enum End {
            Succeed,
            CopyA,
        }
        let end = 'outer: {
            length_a -= 1;
            if length_a == 0 {
                break 'outer End::Succeed;
            }
            if length_b == 1 {
                break 'outer End::CopyA;
            }
            let mut min_gallop = self.min_gallop;
            'gallop: loop {
                let mut wins_a = 0i64;
                let mut wins_b = 0i64;
                loop {
                    let t_value = self.tmp_at(cursor_tmp);
                    let a_value = self.at(cursor_a);
                    let order = self.compare(&t_value, &a_value);
                    if order < 0.0 {
                        self.a[dest as usize] = a_value;
                        dest -= 1;
                        cursor_a -= 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 0 {
                            break 'outer End::Succeed;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    } else {
                        self.a[dest as usize] = t_value;
                        dest -= 1;
                        cursor_tmp -= 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 1 {
                            break 'outer End::CopyA;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= K_MIN_GALLOP_WINS || wins_b >= K_MIN_GALLOP_WINS || first {
                    first = false;
                    min_gallop = i64::max(1, min_gallop - 1);
                    self.min_gallop = min_gallop;
                    let key = self.tmp_at(cursor_tmp);
                    let k = self.gallop_right(false, &key, base_a, length_a, length_a - 1);
                    wins_a = length_a - k;
                    if wins_a > 0 {
                        dest -= wins_a;
                        cursor_a -= wins_a;
                        self.copy_range(cursor_a + 1, dest + 1, wins_a);
                        length_a -= wins_a;
                        if length_a == 0 {
                            break 'outer End::Succeed;
                        }
                    }
                    let t_value = self.tmp_at(cursor_tmp);
                    self.a[dest as usize] = t_value;
                    dest -= 1;
                    cursor_tmp -= 1;
                    length_b -= 1;
                    if length_b == 1 {
                        break 'outer End::CopyA;
                    }
                    let key = self.at(cursor_a);
                    let k = self.gallop_left(true, &key, 0, length_b, length_b - 1);
                    wins_b = length_b - k;
                    if wins_b > 0 {
                        dest -= wins_b;
                        cursor_tmp -= wins_b;
                        self.copy_from_tmp(cursor_tmp + 1, dest + 1, wins_b);
                        length_b -= wins_b;
                        if length_b == 1 {
                            break 'outer End::CopyA;
                        }
                        if length_b == 0 {
                            break 'outer End::Succeed;
                        }
                    }
                    let a_value = self.at(cursor_a);
                    self.a[dest as usize] = a_value;
                    dest -= 1;
                    cursor_a -= 1;
                    length_a -= 1;
                    if length_a == 0 {
                        break 'outer End::Succeed;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
                continue 'gallop;
            }
        };
        match end {
            End::Succeed => {
                if length_b > 0 {
                    self.copy_from_tmp(0, dest - (length_b - 1), length_b);
                }
            }
            End::CopyA => {
                dest -= length_a;
                cursor_a -= length_a;
                self.copy_range(cursor_a + 1, dest + 1, length_a);
                let first = self.tmp_at(cursor_tmp);
                self.a[dest as usize] = first;
            }
        }
    }

    // MergeAt, array-sort.tq.
    fn merge_at(&mut self, i: i64) {
        let i = i as usize;
        let (mut base_a, mut length_a) = self.runs[i];
        let (base_b, mut length_b) = self.runs[i + 1];
        self.runs[i].1 = length_a + length_b;
        if self.runs.len() >= 3 && i == self.runs.len() - 3 {
            let run = self.runs[i + 2];
            self.runs[i + 1] = run;
        }
        self.runs.pop();

        let key_right = self.at(base_b);
        let k = self.gallop_right(false, &key_right, base_a, length_a, 0);
        base_a += k;
        length_a -= k;
        if length_a == 0 {
            return;
        }
        let key_left = self.at(base_a + length_a - 1);
        length_b = self.gallop_left(false, &key_left, base_b, length_b, length_b - 1);
        if length_b == 0 {
            return;
        }
        if length_a <= length_b {
            self.merge_low(base_a, length_a, base_b, length_b);
        } else {
            self.merge_high(base_a, length_a, base_b, length_b);
        }
    }

    // RunInvariantEstablished(pendingRuns, n), n a stack index.
    fn run_invariant_established(&self, n: i64) -> bool {
        if n < 2 {
            return true;
        }
        let n = n as usize;
        self.runs[n - 2].1 > self.runs[n - 1].1 + self.runs[n].1
    }

    // MergeCollapse, array-sort.tq.
    fn merge_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() as i64 - 2;
            if !self.run_invariant_established(n + 1) || !self.run_invariant_established(n) {
                if self.runs[(n - 1) as usize].1 < self.runs[(n + 1) as usize].1 {
                    n -= 1;
                }
                self.merge_at(n);
            } else if self.runs[n as usize].1 <= self.runs[(n + 1) as usize].1 {
                self.merge_at(n);
            } else {
                break;
            }
        }
    }

    // MergeForceCollapse, array-sort.tq.
    fn merge_force_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() as i64 - 2;
            if n > 0 && self.runs[(n - 1) as usize].1 < self.runs[(n + 1) as usize].1 {
                n -= 1;
            }
            self.merge_at(n);
        }
    }

    // ArrayTimSortImpl, array-sort.tq.
    fn sort(&mut self) {
        let length = self.a.len() as i64;
        if length < 2 {
            return;
        }
        // V8 asserts the pending-runs stack holds; honest inputs
        // need far fewer than 85 runs. (A comparator cannot change
        // the run lengths once pushed, so the bound is CPython's.)
        debug_assert!(K_MAX_MERGE_PENDING >= 85);
        let mut remaining = length;
        let mut low = 0i64;
        let min_run = compute_min_run_length(remaining);
        while remaining != 0 {
            let mut run = self.count_and_make_run(low, low + remaining);
            if run < min_run {
                let forced = i64::min(min_run, remaining);
                self.binary_insertion_sort(low, low + run, low + forced);
                run = forced;
            }
            self.runs.push((low, run));
            self.merge_collapse();
            low += run;
            remaining -= run;
        }
        self.merge_force_collapse();
    }
}

// ComputeMinRunLength, array-sort.tq.
fn compute_min_run_length(mut n: i64) -> i64 {
    let mut r = 0i64;
    while n >= 64 {
        r |= n & 1;
        n >>= 1;
    }
    n + r
}

/// `array.sort(comparefn)`: V8's TimSort over the values, in place.
/// The comparator returns a JavaScript Number; NaN counts as +0.
pub fn js_sort<T: Clone>(a: &mut Vec<T>, cmp: &mut impl FnMut(&T, &T) -> f64) {
    js_sort_buffered(a, cmp, &mut SortScratch::default());
}

/// Retained TimSort storage. Reserve the largest candidate set at load; each
/// sort resets minGallop and the pending-run stack exactly as Array.sort does.
#[derive(Clone, Debug)]
pub struct SortScratch<T> {
    tmp: Vec<T>,
    runs: Vec<(i64, i64)>,
}
impl<T> Default for SortScratch<T> {
    fn default() -> Self { Self { tmp: Vec::new(), runs: Vec::new() } }
}
impl<T> SortScratch<T> {
    /// Release retained values without releasing the reserved merge storage.
    pub fn clear(&mut self) { self.tmp.clear(); self.runs.clear(); }
    /// Fallible preparation for an exceptional publication backlog. Stable
    /// sorting still reuses the same merge banks on all subsequent packets.
    pub fn try_reserve(&mut self, count: usize) -> Result<(), std::collections::TryReserveError> {
        self.tmp.try_reserve(count.saturating_sub(self.tmp.len()))?;
        self.runs.try_reserve(K_MAX_MERGE_PENDING.saturating_sub(self.runs.len()))?;
        Ok(())
    }
    pub fn reserve(&mut self, count: usize) {
        self.tmp.reserve(count.saturating_sub(self.tmp.len()));
        self.runs.reserve(K_MAX_MERGE_PENDING.saturating_sub(self.runs.len()));
    }
}

/// ArrayTimSort with battle-owned scratch. Only the storage is reused; merge
/// order, inconsistent-comparator handling and galloping remain V8's.
pub fn js_sort_buffered<T: Clone>(a: &mut Vec<T>, cmp: &mut impl FnMut(&T, &T) -> f64, scratch: &mut SortScratch<T>) {
    scratch.tmp.clear();
    scratch.runs.clear();
    let mut sorter = TimSort {
        a, cmp, tmp: std::mem::take(&mut scratch.tmp), min_gallop: K_MIN_GALLOP_WINS,
        runs: std::mem::take(&mut scratch.runs),
    };
    sorter.sort();
    scratch.tmp = sorter.tmp;
    scratch.runs = sorter.runs;
}

/// `array.sort(comparefn)` over a possibly sparse array: the values
/// are compacted and sorted, then the undefineds, then the holes,
/// in place, as V8's receiver ends up.
pub fn js_sort_slots<T: Clone>(
    a: &mut Vec<JsSlot<T>>,
    cmp: &mut impl FnMut(&T, &T) -> f64,
) {
    let mut values: Vec<T> = Vec::new();
    let mut undefineds = 0usize;
    let mut holes = 0usize;
    for slot in a.iter() {
        match slot {
            JsSlot::Value(v) => values.push(v.clone()),
            JsSlot::Undefined => undefineds += 1,
            JsSlot::Hole => holes += 1,
        }
    }
    js_sort(&mut values, cmp);
    let mut out: Vec<JsSlot<T>> = values.into_iter().map(JsSlot::Value).collect();
    out.extend(core::iter::repeat_n(JsSlot::Undefined, undefineds));
    out.extend(core::iter::repeat_n(JsSlot::Hole, holes));
    *a = out;
}

/// The default comparator for numbers: `ToString` of each, compared
/// as strings by code units. V8 has a Smi fast path
/// (SmiLexicographicCompare) that compares small integers by their
/// decimal text without building it; the result is the same order
/// `Number::toString` gives, which is what this uses for every
/// number.
pub fn js_sort_default_f64(a: &mut Vec<f64>) {
    // ToString is computed per comparison in V8's generic path too
    // (SortCompareDefault builds both strings). The values do not
    // change under a pure comparator, so cache the texts alongside
    // and sort the pairs: same result, as the texts are a function
    // of the values alone.
    let mut pairs: Vec<(String, f64)> = a
        .iter()
        .map(|&x| (crate::numfmt::number_to_string(x), x))
        .collect();
    let mut cmp = |p: &(String, f64), q: &(String, f64)| -> f64 {
        match crate::text::compare_str_utf16(&p.0, &q.0) {
            Ordering::Less => -1.0,
            Ordering::Equal => 0.0,
            Ordering::Greater => 1.0,
        }
    };
    js_sort(&mut pairs, &mut cmp);
    for (i, (_, x)) in pairs.into_iter().enumerate() {
        a[i] = x;
    }
}

/// The default comparator over strings held as Rust strings:
/// code-unit order (which is `ToString` of a string, itself).
pub fn js_sort_default_strings(a: &mut Vec<String>) {
    let mut cmp = |p: &String, q: &String| -> f64 {
        match crate::text::compare_str_utf16(p, q) {
            Ordering::Less => -1.0,
            Ordering::Equal => 0.0,
            Ordering::Greater => 1.0,
        }
    };
    js_sort(a, &mut cmp);
}

/// The default comparator over strings held as code units.
pub fn js_sort_default_js_strings(a: &mut Vec<crate::text::JsString>) {
    let mut cmp = |p: &crate::text::JsString, q: &crate::text::JsString| -> f64 {
        match crate::text::compare_utf16(p, q) {
            Ordering::Less => -1.0,
            Ordering::Equal => 0.0,
            Ordering::Greater => 1.0,
        }
    };
    js_sort(a, &mut cmp);
}

/// The comparison V8's no-comparator TypedArray sort runs
/// (`CompareNum` in src/runtime/runtime-typedarray.cc, fed to a C++
/// std::sort): numeric order, with -0 below +0 and every number
/// below NaN. (The spec's own SortCompare calls -0 and +0 equal;
/// V8's fast path ranks them, measurably: -0 always sorts first.)
/// NaNs compare equal to each other, so their relative order in
/// V8 is std::sort's unstable choice; this stable sort keeps input
/// order there, the one corner that cannot be reproduced without
/// porting libstdc++'s introsort. The sim's Float64Array sorts
/// order times, where NaN payloads carry no meaning, and the
/// differential harness records the corner.
pub fn typed_numeric_compare(a: f64, b: f64) -> f64 {
    if a < b {
        -1.0
    } else if a > b {
        1.0
    } else if a == 0.0 && a == b {
        // -0.0 is less than +0.0 (signbit(a) && !signbit(b)).
        if a.is_sign_negative() && !b.is_sign_negative() {
            -1.0
        } else if b.is_sign_negative() && !a.is_sign_negative() {
            1.0
        } else {
            0.0
        }
    } else if !a.is_nan() && b.is_nan() {
        -1.0
    } else if a.is_nan() && !b.is_nan() {
        1.0
    } else {
        0.0
    }
}

/// `float64Array.sort()` with no comparator: numeric order. See the
/// module notes on V8's `std::sort` and the ±0 corner.
pub fn typed_sort_f64(a: &mut Vec<f64>) {
    let mut cmp = |p: &f64, q: &f64| typed_numeric_compare(*p, *q);
    js_sort(a, &mut cmp);
}

/// `float32Array.sort()` with no comparator. The elements are read
/// as doubles, compared numerically, and stored back (each already
/// a float32 value, so the store is exact).
pub fn typed_sort_f32(a: &mut Vec<f32>) {
    let mut doubles: Vec<f64> = a.iter().map(|&x| x as f64).collect();
    typed_sort_f64(&mut doubles);
    for (i, x) in doubles.into_iter().enumerate() {
        a[i] = x as f32;
    }
}

// TypedArrayMerge / TypedArrayMergeSort, typed-array-sort.tq: a
// recursive mergesort that alternates two buffers, both starting as
// copies of the input.
fn typed_merge<T: Clone>(
    source: &[T],
    target: &mut [T],
    from: usize,
    middle: usize,
    to: usize,
    cmp: &mut impl FnMut(&T, &T) -> f64,
) {
    let mut left = from;
    let mut right = middle;
    for target_index in from..to {
        if left < middle && right >= to {
            target[target_index] = source[left].clone();
            left += 1;
        } else if left < middle {
            let order = normalize_order(cmp(&source[left], &source[right]));
            if order <= 0.0 {
                target[target_index] = source[left].clone();
                left += 1;
            } else {
                target[target_index] = source[right].clone();
                right += 1;
            }
        } else {
            target[target_index] = source[right].clone();
            right += 1;
        }
    }
}

fn typed_merge_sort<T: Clone>(
    source: &mut Vec<T>,
    target: &mut Vec<T>,
    from: usize,
    to: usize,
    cmp: &mut impl FnMut(&T, &T) -> f64,
) {
    debug_assert!(to - from > 1);
    let middle = from + ((to - from) >> 1);
    // On the next recursion step source becomes target and vice
    // versa, as in the Torque.
    if middle - from > 1 {
        typed_merge_sort(target, source, from, middle, cmp);
    }
    if to - middle > 1 {
        typed_merge_sort(target, source, middle, to, cmp);
    }
    typed_merge(source.as_slice(), target.as_mut_slice(), from, middle, to, cmp);
}

/// `typedArray.sort(comparefn)`: V8's mergesort (not the TimSort).
/// The numbers V8 passes the comparator are the elements widened to
/// doubles, which is the caller's `T` here (a Float64Array's are
/// f64 already).
pub fn typed_sort<T: Clone>(a: &mut Vec<T>, cmp: &mut impl FnMut(&T, &T) -> f64) {
    let len = a.len();
    if len < 2 {
        return;
    }
    // work1 and work2 both start as copies; the sort runs
    // TypedArrayMergeSort(work2, 0, len, work1) and work1 holds the
    // result.
    let mut work1 = a.clone();
    let mut work2 = a.clone();
    typed_merge_sort(&mut work2, &mut work1, 0, len, cmp);
    *a = work1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric(p: &i64, q: &i64) -> f64 {
        (*p - *q) as f64
    }

    #[test]
    fn sorts_and_is_stable() {
        let mut v: Vec<(i64, usize)> =
            (0..500).map(|i| (((i * 2654435761usize) % 97) as i64, i)).collect();
        let mut cmp = |p: &(i64, usize), q: &(i64, usize)| (p.0 - q.0) as f64;
        js_sort(&mut v, &mut cmp);
        for w in v.windows(2) {
            assert!(w[0].0 < w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1));
        }
    }

    #[test]
    fn edge_lengths() {
        for n in [0usize, 1, 2, 3, 63, 64, 65, 100, 1000] {
            let mut v: Vec<i64> = (0..n as i64).rev().collect();
            let mut c = |p: &i64, q: &i64| numeric(p, q);
            js_sort(&mut v, &mut c);
            assert_eq!(v, (0..n as i64).collect::<Vec<_>>(), "n={n}");
        }
    }

    #[test]
    fn default_sort_is_textual() {
        let mut v = vec![10.0, 9.0, 80.0, 1.0, 5e21, -3.0, 0.5];
        js_sort_default_f64(&mut v);
        // "0.5" < "1" < "10" < "5e+21" < "80" < "9", and "-3" first
        // ('-' is 0x2d, below every digit).
        assert_eq!(v, vec![-3.0, 0.5, 1.0, 10.0, 5e21, 80.0, 9.0]);
    }

    #[test]
    fn slots_compact_like_v8() {
        let mut v = vec![
            JsSlot::Value(3i64),
            JsSlot::Hole,
            JsSlot::Undefined,
            JsSlot::Value(1),
            JsSlot::Hole,
        ];
        let mut c = |p: &i64, q: &i64| (*p - *q) as f64;
        js_sort_slots(&mut v, &mut c);
        assert_eq!(
            v,
            vec![
                JsSlot::Value(1),
                JsSlot::Value(3),
                JsSlot::Undefined,
                JsSlot::Hole,
                JsSlot::Hole
            ]
        );
    }

    #[test]
    fn typed_numeric_sort_puts_nan_last() {
        let mut v = vec![3.0, f64::NAN, -1.0, 0.0, f64::NAN, 2.0];
        typed_sort_f64(&mut v);
        assert_eq!(&v[..4], &[-1.0, 0.0, 2.0, 3.0]);
        assert!(v[4].is_nan() && v[5].is_nan());
    }

    #[test]
    fn typed_sort_matches_stable_mergesort() {
        let mut v: Vec<(i64, usize)> =
            (0..300).map(|i| (((i * 40503usize) % 50) as i64, i)).collect();
        let mut cmp = |p: &(i64, usize), q: &(i64, usize)| (p.0 - q.0) as f64;
        typed_sort(&mut v, &mut cmp);
        for w in v.windows(2) {
            assert!(w[0].0 < w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1));
        }
    }

    #[test]
    fn nan_comparator_counts_as_zero() {
        // A comparator that always returns NaN leaves the order
        // alone under a stable sort whose comparisons all read 0.
        let mut v: Vec<i64> = (0..100).collect();
        let original = v.clone();
        let mut cmp = |_: &i64, _: &i64| f64::NAN;
        js_sort(&mut v, &mut cmp);
        assert_eq!(v, original);
    }
}
