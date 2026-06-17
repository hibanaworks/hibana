use super::PhantomData;
pub(crate) use core::primitive::usize as LaneWord;
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DenseLaneOrdinal(u16);

impl DenseLaneOrdinal {
    pub(crate) const ABSENT: Self = Self(u16::MAX);

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
pub(crate) const DENSE_LANE_ABSENT: DenseLaneOrdinal = DenseLaneOrdinal::ABSENT;
pub(crate) const MIN_ENDPOINT_LANE_SLOTS: usize = 2;
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
            crate::invariant();
        }
        if word_len > LANE_SET_VIEW_WORDS {
            crate::invariant();
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
            crate::invariant();
        }
        if word_len > LANE_SET_VIEW_WORDS {
            crate::invariant();
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
            /* SAFETY: `word_idx < word_len` bounds the lane-word slice stored
            in this view; word-mode reads one initialized `LaneWord`. */
            unsafe { *self.ptr.cast::<LaneWord>().add(word_idx) }
        } else {
            let bits = LaneWord::BITS as usize;
            let word_start = word_idx * bits;
            let word_end = word_start + bits;
            let mut word = 0usize;
            let mut byte_idx = word_start / (u8::BITS as usize);
            while byte_idx < self.byte_len() && byte_idx * (u8::BITS as usize) < word_end {
                let lane_start = byte_idx * (u8::BITS as usize);
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
            /* SAFETY: `idx < byte_len` bounds the byte-mode lane-set view
            stored in this descriptor image. */
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
            while byte_idx < self.byte_len() && byte_idx * bits < lane_limit {
                let mut byte = self.byte_at(byte_idx);
                byte &= u8::MAX << bit_offset;
                if byte != 0 {
                    let lane = byte_idx * bits + byte.trailing_zeros() as usize;
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
        while word_idx < self.word_len() && word_idx * bits < lane_limit {
            let mut word = self.word_at(word_idx);
            word &= LaneWord::MAX << bit_offset;
            if word != 0 {
                let lane = word_idx * bits + word.trailing_zeros() as usize;
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
            crate::invariant();
        }
        Self {
            ptr,
            word_len: word_len as u16,
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn init_from_parts(dst: *mut Self, ptr: *mut LaneWord, word_len: usize) {
        if word_len > u16::MAX as usize {
            crate::invariant();
        }
        /* SAFETY: route/frontier initialization passes an unpublished
        `LaneSet`; the lane-word pointer and checked u16 length are installed
        before the set is exposed. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).word_len).write(word_len as u16);
        }
        let mut idx = 0usize;
        while idx < word_len {
            /* SAFETY: `idx < word_len` bounds the lane-word backing slice, and
            initialization clears every word before any lane-set operation can
            read it. */
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
            /* SAFETY: `idx < word_len` bounds this lane-set's word slice, and
            `&mut self` owns the clear operation. */
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
        /* SAFETY: `word_idx < word_len` bounds the lane-set word containing
        `lane`, and `&mut self` owns the read-modify-write for that word. */
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
        /* SAFETY: `word_idx < word_len` bounds the lane-set word containing
        `lane`, and `&mut self` owns the removal read-modify-write. */
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
            /* SAFETY: `idx < len` is inside both initialized destination and source
            lane-word views; `&mut self` owns the destination copy. */
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
    if active_lane_count > usize::MAX - MIN_ENDPOINT_LANE_SLOTS {
        crate::invariant();
    }
    let required = active_lane_count + MIN_ENDPOINT_LANE_SLOTS;
    let requested = if required > endpoint_lane_slot_count {
        required
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
#[derive(Clone, Copy, Debug)]
pub(crate) struct LaneSteps {
    /// First offset into the RoleProgram's local step stream for contiguous rows.
    pub(crate) start: u16,
    /// Number of steps in this lane.
    pub(crate) len: u16,
    pub(crate) layout: LaneStepLayout,
}

impl LaneSteps {
    /// Whether this lane has any steps.
    #[inline(always)]
    pub(crate) const fn is_active(&self) -> bool {
        self.len > 0
    }

    #[inline(always)]
    pub(crate) const fn is_contiguous(&self) -> bool {
        self.layout.is_contiguous()
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaneStepLayout {
    Contiguous = 0,
    Sparse = 1,
}

impl LaneStepLayout {
    #[inline(always)]
    pub(crate) const fn is_contiguous(self) -> bool {
        matches!(self, Self::Contiguous)
    }
}
