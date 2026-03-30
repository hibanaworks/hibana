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
    #[cfg(any(test, feature = "std"))]
    pub(crate) unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).len).write(0);
        }
    }

    /// Returns the number of entries in the map.
    #[cfg(test)]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the map is empty.
    #[cfg(test)]
    pub(crate) const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns true if the map is full.
    pub(crate) const fn is_full(&self) -> bool {
        self.len == N
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
                // Replace existing value
                // SAFETY: we're replacing an initialized value
                unsafe {
                    self.entries[i].assume_init_drop();
                    self.entries[i].write((key, value));
                }
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

    /// Append a freshly initialised entry in place.
    ///
    /// This is the lower-layer constructor used when the caller needs to
    /// materialise the `(K, V)` pair directly into backing storage instead of
    /// first building a large temporary on the caller stack.
    pub(crate) fn push_with(
        &mut self,
        init: impl FnOnce(&mut MaybeUninit<(K, V)>),
    ) -> Result<(), ()> {
        if self.is_full() {
            return Err(());
        }

        init(&mut self.entries[self.len]);
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

    /// Return the first initialized slot index matching `pred`.
    pub(crate) fn position(&self, mut pred: impl FnMut(&K, &V) -> bool) -> Option<usize> {
        for i in 0..self.len {
            // SAFETY: entries[0..len] are initialized
            let (k, v) = unsafe { self.entries[i].assume_init_ref() };
            if pred(k, v) {
                return Some(i);
            }
        }
        None
    }

    /// Borrow the entry stored at `idx`.
    pub(crate) fn get_at(&self, idx: usize) -> Option<(&K, &V)> {
        if idx >= self.len {
            return None;
        }
        // SAFETY: entries[idx] is initialized (idx < len)
        let (k, v) = unsafe { self.entries[idx].assume_init_ref() };
        Some((k, v))
    }

    /// Mutably borrow the entry stored at `idx`.
    pub(crate) fn get_mut_at(&mut self, idx: usize) -> Option<(&K, &mut V)> {
        if idx >= self.len {
            return None;
        }
        // SAFETY: entries[idx] is initialized (idx < len)
        let (k, v) = unsafe { self.entries[idx].assume_init_mut() };
        Some((&*k, v))
    }

    /// Replace the initialized slot at `idx` in place.
    pub(crate) fn replace_at_with(
        &mut self,
        idx: usize,
        init: impl FnOnce(&mut MaybeUninit<(K, V)>),
    ) -> Result<(), ()> {
        if idx >= self.len {
            return Err(());
        }

        // SAFETY: entries[idx] is initialized (idx < len)
        unsafe {
            self.entries[idx].assume_init_drop();
        }
        init(&mut self.entries[idx]);
        Ok(())
    }

    /// Remove a key-value pair.
    ///
    /// Returns the value if the key was present, or `None` otherwise.
    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        for i in 0..self.len {
            // SAFETY: entries[0..len] are initialized
            let (k, _) = unsafe { self.entries[i].assume_init_ref() };
            if k == key {
                // SAFETY: we're removing an initialized value
                let (_k, v) = unsafe { self.entries[i].assume_init_read() };

                // Shift remaining entries down
                for j in i..self.len - 1 {
                    // SAFETY: entries[j+1] is initialized (j+1 < len)
                    unsafe {
                        let entry = self.entries[j + 1].assume_init_read();
                        self.entries[j].write(entry);
                    }
                }

                self.len -= 1;
                return Some(v);
            }
        }
        None
    }

    /// Clear all entries.
    pub(crate) fn clear(&mut self) {
        for i in 0..self.len {
            // SAFETY: entries[0..len] are initialized
            unsafe {
                self.entries[i].assume_init_drop();
            }
        }
        self.len = 0;
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

    #[test]
    fn test_insert_and_get() {
        let mut map: ArrayMap<u8, u32, 4> = ArrayMap::new();

        assert!(map.insert(1, 100).is_ok());
        assert!(map.insert(2, 200).is_ok());

        assert_eq!(map.get(&1), Some(&100));
        assert_eq!(map.get(&2), Some(&200));
        assert_eq!(map.get(&3), None);

        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_insert_full() {
        let mut map: ArrayMap<u8, u32, 2> = ArrayMap::new();

        assert!(map.insert(1, 100).is_ok());
        assert!(map.insert(2, 200).is_ok());
        assert_eq!(map.insert(3, 300), Err(300)); // Full

        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_replace_existing() {
        let mut map: ArrayMap<u8, u32, 4> = ArrayMap::new();

        assert!(map.insert(1, 100).is_ok());
        assert!(map.insert(1, 999).is_ok()); // Replace

        assert_eq!(map.get(&1), Some(&999));
        assert_eq!(map.len(), 1); // Should still be 1
    }

    #[test]
    fn test_remove() {
        let mut map: ArrayMap<u8, u32, 4> = ArrayMap::new();

        map.insert(1, 100).unwrap();
        map.insert(2, 200).unwrap();
        map.insert(3, 300).unwrap();

        assert_eq!(map.remove(&2), Some(200));
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&2), None);

        assert_eq!(map.get(&1), Some(&100));
        assert_eq!(map.get(&3), Some(&300));
    }

    #[test]
    fn test_clear() {
        let mut map: ArrayMap<u8, u32, 4> = ArrayMap::new();

        map.insert(1, 100).unwrap();
        map.insert(2, 200).unwrap();

        assert_eq!(map.len(), 2);

        map.clear();

        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
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
