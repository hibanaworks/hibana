use super::PhantomData;
pub(crate) use core::primitive::usize as LaneWord;
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DenseLaneOrdinal(u16);

impl DenseLaneOrdinal {
    pub(crate) const NONE: Self = Self(u16::MAX);

    pub(crate) const fn new(index: usize) -> Option<Self> {
        if index < u16::MAX as usize {
            Some(Self(index as u16))
        } else {
            None
        }
    }

    pub(crate) const fn get(self) -> usize {
        self.0 as usize
    }
}

pub(crate) const LANE_DOMAIN_SIZE: usize = u8::MAX as usize + 1;
pub(crate) const DENSE_LANE_NONE: DenseLaneOrdinal = DenseLaneOrdinal::NONE;
pub(crate) const RESERVED_BINDING_LANES: usize = 2;
pub(crate) const LANE_SET_VIEW_WORDS: usize = lane_word_count(LANE_DOMAIN_SIZE);
pub(crate) const LANE_DOMAIN_BYTES: usize = lane_byte_count(LANE_DOMAIN_SIZE);

#[inline(always)]
pub(crate) const fn lane_word_count(lane_count: usize) -> usize {
    if lane_count == 0 {
        0
    } else {
        lane_count.div_ceil(LaneWord::BITS as usize)
    }
}

#[inline(always)]
pub(crate) const fn lane_word_index(lane: usize) -> (usize, LaneWord) {
    let bits = LaneWord::BITS as usize;
    (lane / bits, 1usize << (lane % bits))
}

#[inline(always)]
pub(crate) const fn lane_byte_count(lane_count: usize) -> usize {
    if lane_count == 0 {
        0
    } else {
        lane_count.div_ceil(u8::BITS as usize)
    }
}

#[inline(always)]
pub(crate) const fn lane_byte_index(lane: usize) -> (usize, u8) {
    let bits = u8::BITS as usize;
    (lane / bits, 1u8 << (lane % bits))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LaneSetView<'a> {
    pub(crate) ptr: *const u8,
    word_len: u16,
    byte_len: u16,
    _marker: PhantomData<&'a [LaneWord]>,
}

impl<'a> LaneSetView<'a> {
    const WORD_MODE: u16 = u16::MAX;
    pub(crate) const EMPTY: Self = Self {
        ptr: core::ptr::null(),
        word_len: 0,
        byte_len: 0,
        _marker: PhantomData,
    };

    #[inline]
    pub(crate) const fn from_parts(ptr: *const LaneWord, word_len: usize) -> Self {
        if word_len > u16::MAX as usize {
            panic!("lane word count overflow");
        }
        if word_len > LANE_SET_VIEW_WORDS {
            panic!("lane word count exceeds lane-domain storage");
        }
        Self {
            ptr: ptr.cast::<u8>(),
            word_len: word_len as u16,
            byte_len: Self::WORD_MODE,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) const fn from_bytes(ptr: *const u8, byte_len: usize, word_len: usize) -> Self {
        if byte_len > u16::MAX as usize || word_len > u16::MAX as usize {
            panic!("lane set byte count overflow");
        }
        if word_len > LANE_SET_VIEW_WORDS {
            panic!("lane word count exceeds lane-domain storage");
        }
        Self {
            ptr,
            word_len: word_len as u16,
            byte_len: byte_len as u16,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    const fn is_word_mode(self) -> bool {
        self.byte_len == Self::WORD_MODE
    }

    #[inline(always)]
    const fn byte_len(self) -> usize {
        if self.is_word_mode() {
            0
        } else {
            self.byte_len as usize
        }
    }

    #[inline(always)]
    pub(crate) const fn word_len(self) -> usize {
        self.word_len as usize
    }

    #[inline(always)]
    pub(crate) fn contains(self, lane: usize) -> bool {
        if !self.is_word_mode() {
            let (byte_idx, bit) = lane_byte_index(lane);
            return byte_idx < self.byte_len() && (self.byte_at(byte_idx) & bit) != 0;
        }
        let (word_idx, bit) = lane_word_index(lane);
        if word_idx >= self.word_len() {
            return false;
        }
        (self.word_at(word_idx) & bit) != 0
    }

    #[inline(always)]
    pub(crate) fn is_empty(self) -> bool {
        if !self.is_word_mode() {
            let mut idx = 0usize;
            while idx < self.byte_len() {
                if self.byte_at(idx) != 0 {
                    return false;
                }
                idx += 1;
            }
            return true;
        }
        let mut idx = 0usize;
        while idx < self.word_len() {
            if self.word_at(idx) != 0 {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    pub(crate) fn equals(self, other: Self) -> bool {
        if self.word_len() != other.word_len() {
            return false;
        }
        let mut idx = 0usize;
        while idx < self.word_len() {
            let lhs = self.word_at(idx);
            let rhs = other.word_at(idx);
            if lhs != rhs {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    pub(crate) fn word_at(self, word_idx: usize) -> LaneWord {
        if word_idx >= self.word_len() || self.ptr.is_null() {
            return 0;
        }
        if self.is_word_mode() {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { *self.ptr.cast::<LaneWord>().add(word_idx) }
        } else {
            let bits = LaneWord::BITS as usize;
            let word_start = word_idx.saturating_mul(bits);
            let word_end = word_start.saturating_add(bits);
            let mut word = 0usize;
            let mut byte_idx = word_start / (u8::BITS as usize);
            while byte_idx < self.byte_len()
                && byte_idx.saturating_mul(u8::BITS as usize) < word_end
            {
                let lane_start = byte_idx.saturating_mul(u8::BITS as usize);
                if lane_start >= word_start {
                    word |= (self.byte_at(byte_idx) as LaneWord) << (lane_start - word_start);
                }
                byte_idx += 1;
            }
            word
        }
    }

    #[inline(always)]
    fn byte_at(self, idx: usize) -> u8 {
        if self.ptr.is_null() || idx >= self.byte_len() {
            0
        } else {
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { *self.ptr.add(idx) }
        }
    }

    #[inline(always)]
    pub(crate) fn first_set(self, lane_limit: usize) -> Option<usize> {
        self.next_set_from(0, lane_limit)
    }

    #[inline(always)]
    pub(crate) fn next_set_from(self, start: usize, lane_limit: usize) -> Option<usize> {
        if start >= lane_limit {
            return None;
        }
        if !self.is_word_mode() {
            let bits = u8::BITS as usize;
            let mut byte_idx = start / bits;
            let mut bit_offset = start % bits;
            while byte_idx < self.byte_len() && byte_idx.saturating_mul(bits) < lane_limit {
                let mut byte = self.byte_at(byte_idx);
                byte &= u8::MAX << bit_offset;
                while byte != 0 {
                    let lane = byte_idx
                        .saturating_mul(bits)
                        .saturating_add(byte.trailing_zeros() as usize);
                    if lane < lane_limit {
                        return Some(lane);
                    }
                    return None;
                }
                byte_idx += 1;
                bit_offset = 0;
            }
            return None;
        }
        let bits = LaneWord::BITS as usize;
        let mut word_idx = start / bits;
        let mut bit_offset = start % bits;
        while word_idx < self.word_len() && word_idx.saturating_mul(bits) < lane_limit {
            let mut word = self.word_at(word_idx);
            word &= LaneWord::MAX << bit_offset;
            while word != 0 {
                let lane = word_idx
                    .saturating_mul(bits)
                    .saturating_add(word.trailing_zeros() as usize);
                if lane < lane_limit {
                    return Some(lane);
                }
                return None;
            }
            word_idx += 1;
            bit_offset = 0;
        }
        None
    }
}

impl<'a, 'b> PartialEq<LaneSetView<'b>> for LaneSetView<'a> {
    #[inline(always)]
    fn eq(&self, other: &LaneSetView<'b>) -> bool {
        (*self).equals(*other)
    }
}

impl Eq for LaneSetView<'_> {}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LaneSet {
    pub(crate) ptr: *mut LaneWord,
    word_len: u16,
}

impl LaneSet {
    pub(crate) const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
        word_len: 0,
    };

    #[inline(always)]
    pub(crate) const fn from_parts(ptr: *mut LaneWord, word_len: usize) -> Self {
        if word_len > u16::MAX as usize {
            panic!("lane word count overflow");
        }
        Self {
            ptr,
            word_len: word_len as u16,
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn init_from_parts(dst: *mut Self, ptr: *mut LaneWord, word_len: usize) {
        if word_len > u16::MAX as usize {
            panic!("lane word count overflow");
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).word_len).write(word_len as u16);
        }
        let mut idx = 0usize;
        while idx < word_len {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                ptr.add(idx).write(0);
            }
            idx += 1;
        }
    }

    #[inline(always)]
    pub(crate) const fn word_len(self) -> usize {
        self.word_len as usize
    }

    #[inline(always)]
    pub(crate) fn view(&self) -> LaneSetView<'_> {
        LaneSetView::from_parts(self.ptr.cast_const(), self.word_len())
    }

    #[inline(always)]
    pub(crate) fn clear(&mut self) {
        let mut idx = 0usize;
        while idx < self.word_len() {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                self.ptr.add(idx).write(0);
            }
            idx += 1;
        }
    }

    #[inline(always)]
    pub(crate) fn insert(&mut self, lane: usize) {
        let (word_idx, bit) = lane_word_index(lane);
        if word_idx >= self.word_len() {
            return;
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            let word = self.ptr.add(word_idx);
            word.write(word.read() | bit);
        }
    }

    #[inline(always)]
    pub(crate) fn remove(&mut self, lane: usize) {
        let (word_idx, bit) = lane_word_index(lane);
        if word_idx >= self.word_len() {
            return;
        }
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            let word = self.ptr.add(word_idx);
            word.write(word.read() & !bit);
        }
    }

    #[inline(always)]
    pub(crate) fn copy_from(&mut self, src: LaneSetView<'_>) {
        self.clear();
        let len = if self.word_len() < src.word_len() {
            self.word_len()
        } else {
            src.word_len()
        };
        let mut idx = 0usize;
        while idx < len {
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                self.ptr.add(idx).write(src.word_at(idx));
            }
            idx += 1;
        }
    }
}

#[inline(always)]
pub(crate) const fn logical_lane_count_for_role(
    active_lane_count: usize,
    endpoint_lane_slot_count: usize,
) -> usize {
    let reserved = active_lane_count.saturating_add(RESERVED_BINDING_LANES);
    let requested = if reserved > endpoint_lane_slot_count {
        reserved
    } else {
        endpoint_lane_slot_count
    };
    if requested > LANE_DOMAIN_SIZE {
        LANE_DOMAIN_SIZE
    } else {
        requested
    }
}

/// Steps for a single lane within the resident role descriptor.
///
/// The resident descriptor computes local nodes directly from the compiled
/// choreography image, so this is only a compact lane range cursor.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LaneSteps {
    /// First offset into the RoleProgram's local step stream for contiguous rows.
    pub start: u16,
    /// Number of steps in this lane.
    pub len: u16,
    /// True when this lane's steps are not contiguous within the resident row.
    pub sparse: bool,
}

impl LaneSteps {
    /// Whether this lane has any steps.
    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.len > 0
    }

    #[inline(always)]
    pub const fn is_contiguous(&self) -> bool {
        !self.sparse
    }
}
