//! Internal endpoint kernel split by layer.

#[path = "route_frontier/authority.rs"]
mod authority;
mod control;
mod core;
mod decode;
pub(crate) mod endpoint_init;
#[path = "route_frontier/evidence.rs"]
mod evidence;
#[path = "runtime/evidence_store.rs"]
mod evidence_store;
#[path = "runtime/frontier.rs"]
mod frontier;
#[path = "runtime/frontier_state.rs"]
mod frontier_state;
#[path = "runtime/inbox.rs"]
mod inbox;
#[path = "runtime/lane_port.rs"]
mod lane_port;
mod lane_slots {
    pub(super) struct LaneSlotArray<T> {
        ptr: *mut Option<T>,
        len: u16,
    }

    impl<T> LaneSlotArray<T> {
        pub(super) unsafe fn init_from_parts(dst: *mut Self, ptr: *mut Option<T>, len: usize) {
            if len > u16::MAX as usize {
                panic!("lane slot array overflow");
            }
            unsafe {
                core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
                core::ptr::addr_of_mut!((*dst).len).write(len as u16);
            }
            let mut idx = 0usize;
            while idx < len {
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
            Some(unsafe { self.ptr.add(lane_idx) })
        }

        #[inline]
        pub(super) fn get(&self, lane_idx: usize) -> Option<&Option<T>> {
            self.slot_ptr(lane_idx).map(|ptr| unsafe { &*ptr })
        }

        #[inline]
        pub(super) fn get_mut(&mut self, lane_idx: usize) -> Option<&mut Option<T>> {
            self.slot_ptr(lane_idx).map(|ptr| unsafe { &mut *ptr })
        }

        #[inline]
        pub(super) fn iter_mut(&mut self) -> core::slice::IterMut<'_, Option<T>> {
            unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len()).iter_mut() }
        }
    }

    impl<T> core::ops::Index<usize> for LaneSlotArray<T> {
        type Output = Option<T>;

        #[inline]
        fn index(&self, index: usize) -> &Self::Output {
            self.get(index)
                .unwrap_or_else(|| panic!("lane slot index {index} out of range"))
        }
    }

    impl<T> core::ops::IndexMut<usize> for LaneSlotArray<T> {
        #[inline]
        fn index_mut(&mut self, index: usize) -> &mut Self::Output {
            self.get_mut(index)
                .unwrap_or_else(|| panic!("lane slot index {index} out of range"))
        }
    }

    impl<T> Drop for LaneSlotArray<T> {
        fn drop(&mut self) {
            let mut idx = 0usize;
            while idx < self.len() {
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
            unsafe {
                LaneSlotArray::init_from_parts(
                    array.as_mut_ptr(),
                    storage.as_mut_ptr().cast::<Option<u16>>(),
                    256,
                );
            }
            let mut array = unsafe { array.assume_init() };

            assert_eq!(array.len(), 256);
            *array.get_mut(255).expect("lane 255 slot") = Some(7);
            assert_eq!(*array.get(255).expect("lane 255 slot"), Some(7));
        }
    }
}
#[path = "runtime/layout.rs"]
pub(crate) mod layout;
#[path = "runtime/observe.rs"]
mod observe;
#[path = "route_frontier/offer.rs"]
mod offer;
mod recv;
#[path = "runtime/route_state.rs"]
mod route_state;

pub(crate) use self::core::cursor_endpoint_storage_layout;
pub(super) use self::core::*;
pub(crate) use self::core::{
    CursorEndpoint, MaterializedRouteBranch, SendControlOutcome, SendDesc, SendPreview,
};
pub(crate) use self::decode::DecodeDesc;
pub(crate) use self::frontier::FrontierScratchLayout;
pub(crate) use self::lane_port::RawSendPayload;
pub(crate) use self::layout::EndpointArenaLayout;
pub(crate) use self::recv::RecvDesc;
