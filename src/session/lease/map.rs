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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_map<K: Copy + Eq, V, const N: usize>() -> ArrayMap<K, V, N> {
        let mut map = MaybeUninit::<ArrayMap<K, V, N>>::uninit();
        /* SAFETY: test setup owns the uninitialized map storage and exposes it only after init_empty writes its initialized prefix metadata. */
        unsafe {
            ArrayMap::init_empty(map.as_mut_ptr());
            map.assume_init()
        }
    }

    fn push_entry<K: Copy + Eq, V: Copy, const N: usize>(
        map: &mut ArrayMap<K, V, N>,
        key: K,
        value: V,
    ) -> Result<(), V> {
        // SAFETY: the closure writes a complete `(K, V)` entry before returning `Ok(())`;
        // on the full-map error path `try_push_with` does not expose or read the slot.
        unsafe {
            map.try_push_with(value, |slot| {
                slot.write((key, value));
                Ok(())
            })
        }
    }

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
        let mut map: ArrayMap<u8, u32, 4> = test_map();

        assert!(push_entry(&mut map, 1, 100).is_ok());
        assert!(push_entry(&mut map, 2, 200).is_ok());

        assert_eq!(map.get(&1), Some(&100));
        assert_eq!(map.get(&2), Some(&200));
        assert_eq!(map.get(&3), None);

        assert_eq!(map.len, 2);
    }

    #[test]
    fn test_insert_full() {
        let mut map: ArrayMap<u8, u32, 2> = test_map();

        assert!(push_entry(&mut map, 1, 100).is_ok());
        assert!(push_entry(&mut map, 2, 200).is_ok());
        assert_eq!(push_entry(&mut map, 3, 300), Err(300));

        assert_eq!(map.len, 2);
    }

    #[test]
    fn test_get_mut_updates_existing() {
        let mut map: ArrayMap<u8, u32, 4> = test_map();

        assert!(push_entry(&mut map, 1, 100).is_ok());
        *map.get_mut(&1).expect("entry") = 999;

        assert_eq!(map.get(&1), Some(&999));
        assert_eq!(map.len, 1);
    }

    #[test]
    fn test_remove() {
        let mut map: ArrayMap<u8, u32, 4> = test_map();

        push_entry(&mut map, 1, 100).expect("test setup entry fits map");
        push_entry(&mut map, 2, 200).expect("test setup entry fits map");
        push_entry(&mut map, 3, 300).expect("test setup entry fits map");

        assert_eq!(remove_entry(&mut map, &2), Some(200));
        assert_eq!(map.len, 2);
        assert_eq!(map.get(&2), None);

        assert_eq!(map.get(&1), Some(&100));
        assert_eq!(map.get(&3), Some(&300));
    }

    #[test]
    fn test_clear() {
        let mut map: ArrayMap<u8, u32, 4> = test_map();

        push_entry(&mut map, 1, 100).expect("test setup entry fits map");
        push_entry(&mut map, 2, 200).expect("test setup entry fits map");

        assert_eq!(map.len, 2);

        clear_entries(&mut map);

        assert_eq!(map.len, 0);
    }

    #[test]
    fn test_get_mut() {
        let mut map: ArrayMap<u8, u32, 4> = test_map();

        push_entry(&mut map, 1, 100).expect("test setup entry fits map");

        if let Some(v) = map.get_mut(&1) {
            *v = 999;
        }

        assert_eq!(map.get(&1), Some(&999));
    }
}
