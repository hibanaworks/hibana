//! Internal endpoint kernel split by responsibility.

mod authority;
mod control;
mod core;
mod decode;
pub(crate) mod endpoint_init;
mod evidence;
mod evidence_store;
mod frontier;
mod frontier_state;
mod inbox;
mod lane_port;
mod lane_slots {
    #[derive(Clone, Copy)]
    pub(super) struct LaneSlotArray<T> {
        ptr: *mut Option<T>,
        len: u8,
    }

    impl<T> LaneSlotArray<T> {
        pub(super) unsafe fn init_from_parts(dst: *mut Self, ptr: *mut Option<T>, len: usize) {
            if len > u8::MAX as usize {
                panic!("lane slot array overflow");
            }
            unsafe {
                core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
                core::ptr::addr_of_mut!((*dst).len).write(len as u8);
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
}
pub(crate) mod layout;
mod observe;
mod offer;
mod recv;
mod route_state;
mod send;

pub(crate) use self::core::cursor_endpoint_storage_layout;
#[allow(unused_imports)]
pub(super) use self::core::*;
pub(crate) use self::core::{CanonicalTokenProvider, CursorEndpoint, RouteBranch, SendPreview};
pub(crate) use self::frontier::FrontierScratchLayout;
pub(crate) use self::frontier::MAX_ROUTE_ARM_STACK;
pub(crate) use self::layout::EndpointArenaLayout;
