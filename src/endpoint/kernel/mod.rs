//! Endpoint kernel split by layer.

mod authority;
mod branch_recv;
mod core;
pub(crate) mod endpoint_init;
mod evidence;
mod evidence_store;
mod frontier;
mod frontier_state;
mod lane_port;
mod session;
mod lane_slots {
    pub(super) struct LaneSlotArray<T> {
        ptr: *mut Option<T>,
        len: u16,
    }

    impl<T> LaneSlotArray<T> {
        pub(super) unsafe fn init_from_parts(dst: *mut Self, ptr: *mut Option<T>, len: usize) {
            if len > u16::MAX as usize {
                crate::invariant();
            }
            /* SAFETY: endpoint initialization passes an unpublished
            `LaneSlotArray` field. The backing pointer and checked u16 length
            are written before any lane slot accessor can observe the array. */
            unsafe {
                core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
                core::ptr::addr_of_mut!((*dst).len).write(len as u16);
            }
            let mut idx = 0usize;
            while idx < len {
                /* SAFETY: `idx < len` selects one slot in the endpoint-owned
                lane slot backing slice, and every slot is initialized to
                `None` before the endpoint is published. */
                unsafe {
                    ptr.add(idx).write(None);
                }
                idx += 1;
            }
        }

        #[inline]
        pub(super) fn len(&self) -> usize {
            self.len as usize
        }

        #[inline]
        fn slot_ptr(&self, lane_idx: usize) -> Option<*mut Option<T>> {
            if lane_idx >= self.len() {
                return None;
            }
            /* SAFETY: `lane_idx < self.len` bounds this endpoint lane-slot
            array; the pointer was installed from the endpoint arena during
            initialization. */
            Some(unsafe { self.ptr.add(lane_idx) })
        }

        #[inline]
        pub(super) fn get(&self, lane_idx: usize) -> Option<&Option<T>> {
            self.slot_ptr(lane_idx).map(|ptr| {
                /* SAFETY: `slot_ptr` returned a lane slot owned by this array;
                shared access is tied to `&self` and cannot mutate the option. */
                unsafe { &*ptr }
            })
        }

        #[inline]
        pub(super) fn get_mut(&mut self, lane_idx: usize) -> Option<&mut Option<T>> {
            self.slot_ptr(lane_idx).map(|ptr| {
                /* SAFETY: `&mut self` is the lane-slot mutation token, so the
                returned mutable option is the only live borrow of this slot. */
                unsafe { &mut *ptr }
            })
        }

        #[inline]
        pub(super) fn iter(&self) -> core::slice::Iter<'_, Option<T>> {
            let len = self.len();
            let ptr = if len == 0 {
                core::ptr::NonNull::<Option<T>>::dangling().as_ptr()
            } else {
                self.ptr
            };
            /* SAFETY: `ptr,len` describe the initialized endpoint lane-slot
            slice installed by `init_from_parts`; shared iteration cannot
            mutate or move a lane owner. */
            unsafe { core::slice::from_raw_parts(ptr, len).iter() }
        }

        #[inline]
        pub(super) fn iter_mut(&mut self) -> core::slice::IterMut<'_, Option<T>> {
            let len = self.len();
            let ptr = if len == 0 {
                core::ptr::NonNull::<Option<T>>::dangling().as_ptr()
            } else {
                self.ptr
            };
            /* SAFETY: `ptr,len` describe the initialized endpoint lane-slot
            slice installed by `init_from_parts`; zero-length arrays use a
            dangling pointer accepted by `from_raw_parts_mut`. */
            unsafe { core::slice::from_raw_parts_mut(ptr, len).iter_mut() }
        }
    }

    impl<T> core::ops::Index<usize> for LaneSlotArray<T> {
        type Output = Option<T>;

        #[inline]
        fn index(&self, index: usize) -> &Self::Output {
            crate::invariant_some(self.get(index))
        }
    }

    impl<T> core::ops::IndexMut<usize> for LaneSlotArray<T> {
        #[inline]
        fn index_mut(&mut self, index: usize) -> &mut Self::Output {
            crate::invariant_some(self.get_mut(index))
        }
    }

    impl<T> Drop for LaneSlotArray<T> {
        fn drop(&mut self) {
            let mut idx = 0usize;
            while idx < self.len() {
                /* SAFETY: `idx < self.len` selects an initialized lane slot in
                this array, and Drop owns the array so no slot borrow remains. */
                unsafe {
                    core::ptr::drop_in_place(self.ptr.add(idx));
                }
                idx += 1;
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::LaneSlotArray;
        use core::mem::MaybeUninit;

        #[test]
        fn lane_slot_array_accepts_full_u8_lane_domain() {
            let mut storage = std::vec::Vec::with_capacity(256);
            storage.resize_with(256, MaybeUninit::<Option<u16>>::uninit);
            let mut array = MaybeUninit::<LaneSlotArray<u16>>::uninit();
            /* SAFETY: `array` is this test's uninitialized `LaneSlotArray`
            storage and `storage` owns 256 uninitialized option slots until the
            initialized array takes responsibility for dropping them. */
            unsafe {
                LaneSlotArray::init_from_parts(
                    array.as_mut_ptr(),
                    storage.as_mut_ptr().cast::<Option<u16>>(),
                    256,
                );
            }
            let mut array =
                /* SAFETY: `LaneSlotArray::init_from_parts` returned after writing
                both fields and every `Option<u16>` slot in `storage`. */
                unsafe { array.assume_init() };

            assert_eq!(array.len(), 256);
            *array.get_mut(255).expect("lane 255 slot") = Some(7);
            assert_eq!(*array.get(255).expect("lane 255 slot"), Some(7));
        }
    }
}
mod decision_state;
pub(crate) mod layout;
mod observe;
mod offer;
mod public_ops;
mod public_poll;
mod recv;
mod recv_commit_plan;

pub(crate) use self::core::cursor_endpoint_storage_layout;
pub(super) use self::core::*;
pub(crate) use self::core::{
    CursorEndpoint, PublicSlotOwnership, SendInit, SendPreview, SendRuntimeDesc,
};
pub(crate) use self::frontier::FrontierScratchLayout;
pub(crate) use self::lane_port::RawSendPayload;
pub(crate) use self::layout::EndpointArenaLayout;
