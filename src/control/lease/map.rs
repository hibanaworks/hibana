//! Fixed-size array-based map for `no_alloc` environments.
//!
//! This module provides a simple key-value map backed by a fixed-size array,
//! suitable for use in `no_std`/`no_alloc` contexts.

use core::mem::MaybeUninit;

/// Fixed-size array-based map.
///
/// This map uses a fixed-size array and linear search. It is suitable for small
/// maps (< 16 entries) in `no_alloc` environments.
///
/// # Type Parameters
///
/// - `K`: Key type (must implement `Copy + Eq`)
/// - `V`: Value type
/// - `N`: Maximum number of entries
pub(crate) struct ArrayMap<K, V, const N: usize> {
    entries: [MaybeUninit<(K, V)>; N],
    len: usize,
}

impl<K: Copy + Eq, V, const N: usize> ArrayMap<K, V, N> {
    /// Create a new empty map.
    pub(crate) const fn new() -> Self {
        Self {
            // SAFETY: MaybeUninit::uninit() creates an uninitialized array of MaybeUninit entries.
            entries: unsafe { MaybeUninit::uninit().assume_init() },
            len: 0,
        }
    }

    /// Initialize an empty map in place without first materializing the full
    /// backing array on the caller's stack.
    ///
    /// # Safety
    /// `dst` must point to valid, writable memory for `Self`.
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).len).write(0);
        }
    }

    /// Returns true if the map is full.
    pub(crate) const fn is_full(&self) -> bool {
        self.len >= N
    }

    /// Insert a key-value pair.
    ///
    /// Returns `Ok(())` if the insertion succeeded, or `Err(value)` if the map is full.
    pub(crate) fn insert(&mut self, key: K, value: V) -> Result<(), V> {
        // Check if key already exists
        for i in 0..self.len {
            // SAFETY: entries[0..len] are initialized
            let (k, _) = unsafe { self.entries[i].assume_init_ref() };
            if *k == key {
                // SAFETY: entries[0..len] are initialized. Replacing before
                // dropping the old entry preserves the initialized-prefix
                // invariant even if the old value's destructor panics.
                let old = unsafe { self.entries[i].as_mut_ptr().replace((key, value)) };
                drop(old);
                return Ok(());
            }
        }

        // Insert new entry
        if self.is_full() {
            return Err(value);
        }

        // len < N, so entries[len] is valid
        self.entries[self.len].write((key, value));
        self.len += 1;
        Ok(())
    }

    /// Append a freshly initialised entry in place, committing the slot only on success.
    ///
    /// # Safety
    ///
    /// `init` must fully initialize the provided slot before returning `Ok(())`.
    /// If it returns `Err`, it must not leave droppable initialized state in
    /// the slot.
    pub(crate) unsafe fn try_push_with<E>(
        &mut self,
        full_error: E,
        init: impl FnOnce(&mut MaybeUninit<(K, V)>) -> Result<(), E>,
    ) -> Result<(), E> {
        if self.is_full() {
            return Err(full_error);
        }
        init(&mut self.entries[self.len])?;
        self.len += 1;
        Ok(())
    }

    /// Get a reference to the value associated with the key.
    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        for i in 0..self.len {
            // SAFETY: entries[0..len] are initialized
            let (k, v) = unsafe { self.entries[i].assume_init_ref() };
            if k == key {
                return Some(v);
            }
        }
        None
    }

    /// Get a mutable reference to the value associated with the key.
    pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        // Find the index first
        let mut found_idx = None;
        for i in 0..self.len {
            // SAFETY: entries[0..len] are initialized
            let (k, _) = unsafe { self.entries[i].assume_init_ref() };
            if k == key {
                found_idx = Some(i);
                break;
            }
        }

        // Then get mutable reference
        if let Some(idx) = found_idx {
            // SAFETY: entries[idx] is initialized (idx < len)
            let (_k, v) = unsafe { self.entries[idx].assume_init_mut() };
            Some(v)
        } else {
            None
        }
    }

    /// Return the initialized slot index for `key`.
    pub(crate) fn index_of(&self, key: &K) -> Option<usize> {
        for i in 0..self.len {
            // SAFETY: entries[0..len] are initialized.
            let (k, _) = unsafe { self.entries[i].assume_init_ref() };
            if k == key {
                return Some(i);
            }
        }
        None
    }

    /// Get a mutable value reference by an owner-minted initialized slot index.
    pub(crate) fn get_index_mut(&mut self, idx: usize) -> Option<(&K, &mut V)> {
        if idx >= self.len {
            return None;
        }
        // SAFETY: `idx < len`, so the entry is initialized.
        let (key, value) = unsafe { self.entries[idx].assume_init_mut() };
        Some((key, value))
    }

    /// Get mutable references to two distinct initialized slot indices.
    pub(crate) fn get_pair_index_mut(
        &mut self,
        left_idx: usize,
        right_idx: usize,
    ) -> Option<((&K, &mut V), (&K, &mut V))> {
        if left_idx == right_idx || left_idx >= self.len || right_idx >= self.len {
            return None;
        }
        // SAFETY: both indices are initialized and distinct map slots.
        unsafe {
            let left_entry = self.entries[left_idx].as_mut_ptr();
            let right_entry = self.entries[right_idx].as_mut_ptr();
            Some((
                (&(*left_entry).0, &mut (*left_entry).1),
                (&(*right_entry).0, &mut (*right_entry).1),
            ))
        }
    }

    /// Retain only entries accepted by `keep`, compacting the initialized prefix.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool)
    where
        V: Copy,
    {
        let old_len = self.len;
        self.len = 0;
        for read in 0..old_len {
            let retain = {
                let (key, value) = /* SAFETY: the table owner tracks the initialized prefix and checks this slot before reading initialized storage. */ unsafe { self.entries[read].assume_init_mut() };
                keep(key, value)
            };
            if retain {
                if self.len != read {
                    let entry = /* SAFETY: the table owner tracks the initialized prefix and checks this slot before reading initialized storage. */ unsafe { *self.entries[read].assume_init_ref() };
                    self.entries[self.len].write(entry);
                }
                self.len += 1;
            }
        }
    }

    /// Returns true if the map contains the given key.
    pub(crate) fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }
}

impl<K, V, const N: usize> Drop for ArrayMap<K, V, N> {
    fn drop(&mut self) {
        for i in 0..self.len {
            // SAFETY: entries[0..len] are initialized
            unsafe {
                self.entries[i].assume_init_drop();
            }
        }
    }
}

impl<K: Copy + Eq, V, const N: usize> Default for ArrayMap<K, V, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remove_entry<K: Copy + Eq, V, const N: usize>(
        map: &mut ArrayMap<K, V, N>,
        key: &K,
    ) -> Option<V> {
        for i in 0..map.len {
            // SAFETY: entries[0..len] are initialized.
            let (k, _) = unsafe { map.entries[i].assume_init_ref() };
            if k == key {
                // SAFETY: the matching slot is initialized.
                let (_k, v) = unsafe { map.entries[i].assume_init_read() };
                for j in i..map.len - 1 {
                    // SAFETY: entries[j + 1] is initialized because j + 1 < len.
                    let entry = unsafe { map.entries[j + 1].assume_init_read() };
                    map.entries[j].write(entry);
                }
                map.len -= 1;
                return Some(v);
            }
        }
        None
    }

    fn clear_entries<K, V, const N: usize>(map: &mut ArrayMap<K, V, N>) {
        for i in 0..map.len {
            // SAFETY: entries[0..len] are initialized.
            unsafe {
                map.entries[i].assume_init_drop();
            }
        }
        map.len = 0;
    }

    #[test]
    fn test_insert_and_get() {
        let mut map: ArrayMap<u8, u32, 4> = ArrayMap::new();

        assert!(map.insert(1, 100).is_ok());
        assert!(map.insert(2, 200).is_ok());

        assert_eq!(map.get(&1), Some(&100));
        assert_eq!(map.get(&2), Some(&200));
        assert_eq!(map.get(&3), None);

        assert_eq!(map.len, 2);
    }

    #[test]
    fn test_insert_full() {
        let mut map: ArrayMap<u8, u32, 2> = ArrayMap::new();

        assert!(map.insert(1, 100).is_ok());
        assert!(map.insert(2, 200).is_ok());
        assert_eq!(map.insert(3, 300), Err(300)); // Full

        assert_eq!(map.len, 2);
    }

    #[test]
    fn test_replace_existing() {
        let mut map: ArrayMap<u8, u32, 4> = ArrayMap::new();

        assert!(map.insert(1, 100).is_ok());
        assert!(map.insert(1, 999).is_ok()); // Replace

        assert_eq!(map.get(&1), Some(&999));
        assert_eq!(map.len, 1); // Should still be 1
    }

    #[test]
    fn test_remove() {
        let mut map: ArrayMap<u8, u32, 4> = ArrayMap::new();

        map.insert(1, 100).unwrap();
        map.insert(2, 200).unwrap();
        map.insert(3, 300).unwrap();

        assert_eq!(remove_entry(&mut map, &2), Some(200));
        assert_eq!(map.len, 2);
        assert_eq!(map.get(&2), None);

        assert_eq!(map.get(&1), Some(&100));
        assert_eq!(map.get(&3), Some(&300));
    }

    #[test]
    fn test_clear() {
        let mut map: ArrayMap<u8, u32, 4> = ArrayMap::new();

        map.insert(1, 100).unwrap();
        map.insert(2, 200).unwrap();

        assert_eq!(map.len, 2);

        clear_entries(&mut map);

        assert_eq!(map.len, 0);
    }

    #[test]
    fn test_get_mut() {
        let mut map: ArrayMap<u8, u32, 4> = ArrayMap::new();

        map.insert(1, 100).unwrap();

        if let Some(v) = map.get_mut(&1) {
            *v = 999;
        }

        assert_eq!(map.get(&1), Some(&999));
    }
}
