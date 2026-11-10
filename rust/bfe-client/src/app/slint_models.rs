//! Keeps the list models behind Slint screens stable from frame to frame.
//!
//! The screens are projected from the Rust state on every frame. A Slint `for` repeater destroys
//! and rebuilds all of its delegates when its model is replaced or reset, and everything a
//! delegate was holding goes with it: the hover highlight, a press that has not been released
//! yet, the first click of a double click, a text cursor. A model rebuilt every frame therefore
//! makes rows that cannot be clicked at human speed. Screens keep one `VecModel` per list for
//! their whole life and bring it up to date with `sync_rows`, which only touches what changed.

use slint::{Model, VecModel};

/// Makes `model` hold exactly `next` without ever resetting it.
///
/// Rows that already hold the right value are left alone, so their delegates keep their state.
/// A changed row is rewritten in place, which updates its delegate instead of rebuilding it.
/// A longer list is appended to and a shorter one trimmed at the tail, so the rows that remain
/// are still the same delegates.
pub(super) fn sync_rows<T: Clone + PartialEq + 'static>(model: &VecModel<T>, next: &[T]) {
    let current = model.row_count();
    let shared = current.min(next.len());
    for (row, value) in next.iter().enumerate().take(shared) {
        if model.row_data(row).as_ref() != Some(value) {
            model.set_row_data(row, value.clone());
        }
    }
    for value in &next[shared..] {
        model.push(value.clone());
    }
    // Trim from the end: removing the last row never renumbers a row that stays.
    for row in (next.len()..current).rev() {
        model.remove(row);
    }
}

#[cfg(test)]
mod tests {
    use super::sync_rows;
    use i_slint_core::model::{ModelChangeListener, ModelChangeListenerContainer};
    use slint::{Model, VecModel};
    use std::cell::Cell;
    use std::pin::Pin;

    /// Counts the notifications a repeater would receive from the model.
    #[derive(Default)]
    struct Seen {
        changed: Cell<usize>,
        added: Cell<usize>,
        removed: Cell<usize>,
        reset: Cell<usize>,
    }

    impl ModelChangeListener for Seen {
        fn row_changed(self: Pin<&Self>, _row: usize) {
            self.changed.set(self.changed.get() + 1);
        }
        fn row_added(self: Pin<&Self>, _index: usize, count: usize) {
            self.added.set(self.added.get() + count);
        }
        fn row_removed(self: Pin<&Self>, _index: usize, count: usize) {
            self.removed.set(self.removed.get() + count);
        }
        fn reset(self: Pin<&Self>) {
            self.reset.set(self.reset.get() + 1);
        }
    }

    fn rows(model: &VecModel<i32>) -> Vec<i32> {
        model.iter().collect()
    }

    fn watched(start: &[i32]) -> (VecModel<i32>, Pin<Box<ModelChangeListenerContainer<Seen>>>) {
        let model = VecModel::from(start.to_vec());
        let seen = Box::pin(ModelChangeListenerContainer::<Seen>::default());
        model.model_tracker().attach_peer(seen.as_ref().model_peer());
        (model, seen)
    }

    #[test]
    fn an_unchanged_list_notifies_nothing() {
        let (model, seen) = watched(&[1, 2, 3]);
        sync_rows(&model, &[1, 2, 3]);
        assert_eq!(rows(&model), [1, 2, 3]);
        assert_eq!(
            (seen.changed.get(), seen.added.get(), seen.removed.get(), seen.reset.get()),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn only_the_rows_that_differ_are_rewritten() {
        let (model, seen) = watched(&[1, 2, 3]);
        sync_rows(&model, &[1, 9, 3]);
        assert_eq!(rows(&model), [1, 9, 3]);
        assert_eq!(
            (seen.changed.get(), seen.added.get(), seen.removed.get(), seen.reset.get()),
            (1, 0, 0, 0)
        );
    }

    #[test]
    fn a_longer_list_appends_and_a_shorter_one_trims_the_tail() {
        let (model, seen) = watched(&[1, 2]);
        sync_rows(&model, &[1, 2, 3, 4]);
        assert_eq!(rows(&model), [1, 2, 3, 4]);
        assert_eq!((seen.changed.get(), seen.added.get(), seen.reset.get()), (0, 2, 0));

        sync_rows(&model, &[7]);
        assert_eq!(rows(&model), [7]);
        assert_eq!((seen.changed.get(), seen.removed.get(), seen.reset.get()), (1, 3, 0));

        sync_rows(&model, &[]);
        assert_eq!(rows(&model), Vec::<i32>::new());
        assert_eq!((seen.removed.get(), seen.reset.get()), (4, 0));
    }
}
