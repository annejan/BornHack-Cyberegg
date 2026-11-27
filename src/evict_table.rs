//! "Update the matching row, or insert a new one and evict the oldest if
//! full" for the small bounded tables the mesh code keeps (adverts heard,
//! read cursors, discovery cache). Pure, `no_std`.

/// Find the row matching `is_match` and call `update` on it, or, if there is
/// none, insert `make()`. When the table is full the oldest row is dropped
/// first: the row with the smallest `age` key, the first one in table order
/// on ties. Removal is `swap_remove`, so the table order changes (the last
/// row moves into the freed slot) and later ties resolve against that new
/// order.
///
/// Returns `true` when a row was inserted, `false` when an existing one was
/// updated.
pub fn upsert_evict_oldest<T, K: Ord, const N: usize>(
    list: &mut heapless::Vec<T, N>,
    is_match: impl Fn(&T) -> bool,
    age: impl Fn(&T) -> K,
    update: impl FnOnce(&mut T),
    make: impl FnOnce() -> T,
) -> bool {
    if let Some(row) = list.iter_mut().find(|row| is_match(row)) {
        update(row);
        return false;
    }
    if list.is_full() {
        let oldest_idx = list
            .iter()
            .enumerate()
            .min_by_key(|(_, row)| age(row))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let _ = list.swap_remove(oldest_idx);
    }
    let _ = list.push(make());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    type Row = (u8, u64); // (key, age)

    fn upsert<const N: usize>(list: &mut heapless::Vec<Row, N>, key: u8, age: u64) -> bool {
        upsert_evict_oldest(
            list,
            |r| r.0 == key,
            |r| r.1,
            |r| r.1 = age,
            || (key, age),
        )
    }

    #[test]
    fn inserts_until_full_then_updates_in_place() {
        let mut t: heapless::Vec<Row, 3> = heapless::Vec::new();
        assert!(upsert(&mut t, 1, 10));
        assert!(upsert(&mut t, 2, 20));
        assert!(!upsert(&mut t, 1, 30)); // update, not insert
        assert_eq!(t.as_slice(), &[(1, 30), (2, 20)]);
    }

    #[test]
    fn full_table_evicts_smallest_age() {
        let mut t: heapless::Vec<Row, 3> = heapless::Vec::new();
        upsert(&mut t, 1, 50);
        upsert(&mut t, 2, 10);
        upsert(&mut t, 3, 30);
        assert!(upsert(&mut t, 4, 40));
        // key 2 (age 10) was the oldest; swap_remove moved the last row (3)
        // into its slot, then the new row was appended.
        assert_eq!(t.as_slice(), &[(1, 50), (3, 30), (4, 40)]);
    }

    #[test]
    fn full_table_update_does_not_evict() {
        let mut t: heapless::Vec<Row, 2> = heapless::Vec::new();
        upsert(&mut t, 1, 10);
        upsert(&mut t, 2, 20);
        assert!(!upsert(&mut t, 2, 99));
        assert_eq!(t.as_slice(), &[(1, 10), (2, 99)]);
    }

    #[test]
    fn tie_evicts_first_in_table_order() {
        let mut t: heapless::Vec<Row, 3> = heapless::Vec::new();
        upsert(&mut t, 1, 5);
        upsert(&mut t, 2, 5);
        upsert(&mut t, 3, 5);
        assert!(upsert(&mut t, 4, 9));
        // Index 0 (key 1) wins the tie; key 3 swaps into slot 0.
        assert_eq!(t.as_slice(), &[(3, 5), (2, 5), (4, 9)]);
        // New order decides the next tie: slot 0 (key 3) goes first.
        assert!(upsert(&mut t, 5, 9));
        assert_eq!(t.as_slice(), &[(4, 9), (2, 5), (5, 9)]);
    }

    #[test]
    fn single_slot_table_replaces() {
        let mut t: heapless::Vec<Row, 1> = heapless::Vec::new();
        upsert(&mut t, 1, 1);
        assert!(upsert(&mut t, 2, 2));
        assert_eq!(t.as_slice(), &[(2, 2)]);
    }
}
