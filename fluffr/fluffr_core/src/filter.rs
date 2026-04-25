#[derive(Clone, Debug, Default)]
pub struct BitMask {
    pub bits:  Vec<u32>,
    pub len:   usize,
    pub count: usize,
}

impl BitMask {
    /// Creates a new BitMask initialized to zero.
    pub fn new(len: usize) -> Self {
        let n = (len + 31) >> 5;
        Self { bits: vec![0; n], len, count: 0 }
    }
    pub fn full(len: usize) -> Self {
        let n = (len + 31) >> 5;
        let mut bits = vec![u32::MAX; n];
        // Mask off padding bits in the last word
        let remainder = len & 31;
        if remainder != 0 {
            if let Some(last) = bits.last_mut() {
                *last = (1 << remainder) - 1;
            }
        }
        Self { bits, len, count: len }
    }
    /// Sets the bit at the given index to 1.
    /// No-ops silently if the bit is already set so `count` stays accurate.
    #[inline(always)]
    pub fn set(&mut self, index: usize) {
        let word = &mut self.bits[index >> 5];
        let bit  = 1u32 << (index & 31);
        if *word & bit == 0 {
            *word |= bit;
            self.count += 1;
        }
    }
    /// Clears the bit at the given index to 0.
    /// No-ops silently if the bit is already clear so `count` stays accurate.
    #[inline(always)]
    pub fn unset(&mut self, index: usize) {
        let word = &mut self.bits[index >> 5];
        let bit  = 1u32 << (index & 31);
        if *word & bit != 0 {
            *word &= !bit;
            self.count -= 1;
        }
    }
    /// Returns true if the bit at the given index is 1.
    #[inline(always)]
    pub fn is_set(&self, index: &usize) -> bool {
        *index < self.len && (self.bits[index >> 5] & (1 << (index & 31))) != 0
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    #[inline(always)]
    pub const fn is_full(&self) -> bool {
        self.count == self.len
    }
    /// Clears all bits (resets to zero) without reallocating.
    pub fn clear(&mut self) {
        for b in self.bits.iter_mut() { *b = 0; }
        self.count = 0;
    }

    /// Performs a bitwise AND in-place (intersection).
    /// Recounts set bits via popcount after the operation.
    #[inline]
    pub fn and(&mut self, other: &Self) {
        for (a, b) in self.bits.iter_mut().zip(other.bits.iter()) {
            *a &= b;
        }
        self.count = self.bits.iter().map(|w| w.count_ones() as usize).sum();
    }

    /// Performs a bitwise OR in-place (union).
    /// Recounts set bits via popcount after the operation.
    #[inline]
    pub fn or(&mut self, other: &Self) {
        for (a, b) in self.bits.iter_mut().zip(other.bits.iter()) {
            *a |= b;
        }
        self.count = self.bits.iter().map(|w| w.count_ones() as usize).sum();
    }

    /// Returns a high-performance iterator over the indices of set bits.
    pub fn iter(&self) -> BitMaskIter<'_> {
        BitMaskIter::new(self)
    }
    pub fn concat(mut self, b: BitMask) -> Self {
        let new_len = self.len + b.len;
        let new_word_count = (new_len + 31) >> 5;
        self.bits.resize(new_word_count, 0);

        let shift = self.len & 31;
        let base  = self.len >> 5;

        if shift == 0 {
            for (i, &w) in b.bits.iter().enumerate() {
                self.bits[base + i] = w;
            }
        } else {
            for (i, &w) in b.bits.iter().enumerate() {
                self.bits[base + i]     |= w << shift;
                self.bits[base + i + 1] |= w >> (32 - shift);
            }
        }

        self.count += b.count;
        self.len    = new_len;
        self
    }
    pub fn extend(&mut self, b: BitMask) {
        let new_len = self.len + b.len;
        let new_word_count = (new_len + 31) >> 5;
        self.bits.resize(new_word_count, 0);

        let shift = self.len & 31;
        let base  = self.len >> 5;

        if shift == 0 {
            for (i, &w) in b.bits.iter().enumerate() {
                self.bits[base + i] = w;
            }
        } else {
            for (i, &w) in b.bits.iter().enumerate() {
                self.bits[base + i]     |= w << shift;
                self.bits[base + i + 1] |= w >> (32 - shift);
            }
        }

        self.count += b.count;
        self.len    = new_len;
    }
}

// ── Conversions ───────────────────────────────────────────────────────────────

impl From<&BitMask> for BitMask {
    fn from(other: &BitMask) -> BitMask { other.clone() }
}

// ── Operators ─────────────────────────────────────────────────────────────────

impl std::ops::Not for BitMask {
    type Output = BitMask;

    fn not(mut self) -> BitMask {
        for b in self.bits.iter_mut() { *b = !*b; }
        let remainder = self.len & 31;
        if remainder != 0 {
            if let Some(last) = self.bits.last_mut() {
                *last &= (1 << remainder) - 1;
            }
        }
        self.count = self.len - self.count;
        self
    }
}
impl std::ops::Not for &BitMask {
    type Output = BitMask;

    fn not(self) -> BitMask {
        let mut out = self.clone();
        for b in out.bits.iter_mut() { *b = !*b; }
        let remainder = out.len & 31;
        if remainder != 0 {
            if let Some(last) = out.bits.last_mut() {
                *last &= (1 << remainder) - 1;
            }
        }
        out.count = out.len - out.count;
        out
    }
}
impl<T: Into<BitMask>> std::ops::BitAnd<T> for BitMask {
    type Output = BitMask;

    fn bitand(mut self, other: T) -> BitMask {
        self.and(&other.into());
        self
    }
}
impl<T: Into<BitMask>> std::ops::BitOr<T> for BitMask {
    type Output = BitMask;

    fn bitor(mut self, other: T) -> BitMask {
        self.or(&other.into());
        self
    }
}
// ── Iteration ─────────────────────────────────────────────────────────────────

// Support for 'for index in &mask' syntax
impl<'a> IntoIterator for &'a BitMask {
    type Item     = usize;
    type IntoIter = BitMaskIter<'a>;
    fn into_iter(self) -> Self::IntoIter { self.iter() }
}

/// The zero-overhead state machine for BitMask iteration.
///
/// Tracks both a forward cursor (CTZ, lowest bit first) and a backward cursor
/// (CLZ, highest bit first).  `remaining` is the shared budget — it reaches
/// zero exactly when the two ends have consumed every set bit, cleanly handling
/// the case where they meet inside the same word.
pub struct BitMaskIter<'a> {
    mask:        &'a BitMask,
    // forward state
    fwd_bucket:  usize,
    fwd_word:    u32,
    // backward state
    bck_bucket:  usize,
    bck_word:    u32,
    // shared budget
    remaining:   usize,
}

impl<'a> BitMaskIter<'a> {
    fn new(mask: &'a BitMask) -> Self {
        let last = mask.bits.len().saturating_sub(1);
        Self {
            mask,
            fwd_bucket: 0,
            fwd_word:   mask.bits.first().copied().unwrap_or(0),
            bck_bucket: last,
            bck_word:   mask.bits.last().copied().unwrap_or(0),
            remaining:  mask.count,
        }
    }
}

impl<'a> Iterator for BitMaskIter<'a> {
    type Item = usize;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 { return None; }

        // Skip empty forward buckets
        while self.fwd_word == 0 {
            self.fwd_bucket += 1;
            self.fwd_word = self.mask.bits[self.fwd_bucket];
        }

        let bit_idx = self.fwd_word.trailing_zeros();
        self.fwd_word &= !(1 << bit_idx);
        // Keep bck_word in sync if both cursors are in the same bucket
        if self.fwd_bucket == self.bck_bucket {
            self.bck_word = self.fwd_word;
        }

        self.remaining -= 1;
        Some((self.fwd_bucket << 5) | bit_idx as usize)
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> DoubleEndedIterator for BitMaskIter<'a> {
    #[inline(always)]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 { return None; }

        // Skip empty backward buckets
        while self.bck_word == 0 {
            self.bck_bucket -= 1;
            self.bck_word = self.mask.bits[self.bck_bucket];
        }

        let bit_idx = 31 - self.bck_word.leading_zeros();
        self.bck_word &= !(1 << bit_idx);
        // Keep fwd_word in sync if both cursors are in the same bucket
        if self.bck_bucket == self.fwd_bucket {
            self.fwd_word = self.bck_word;
        }

        self.remaining -= 1;
        let absolute_idx = (self.bck_bucket << 5) | bit_idx as usize;
        // Guard against padding bits in the last word
        if absolute_idx < self.mask.len { Some(absolute_idx) } else { None }
    }
}

impl ExactSizeIterator for BitMaskIter<'_> {
    #[inline(always)]
    fn len(&self) -> usize { self.remaining }
}

impl BitMask {
    pub fn iter_zeros(&self) -> BitMaskZerosIter<'_> {
        BitMaskZerosIter::new(self)
    }
}

pub struct BitMaskZerosIter<'a> {
    mask: &'a BitMask,
    bucket: usize,
    current_word: u32,
    remaining: usize,
}

impl<'a> BitMaskZerosIter<'a> {
    fn new(mask: &'a BitMask) -> Self {
        let first_word = mask.bits.first().map(|&w| !w).unwrap_or(0);
        Self {
            mask,
            bucket: 0,
            current_word: first_word,
            // Total zeros is (length - set_bits)
            remaining: mask.len - mask.count,
        }
    }
}

impl<'a> Iterator for BitMaskZerosIter<'a> {
    type Item = usize;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 { return None; }

        // Skip words that were all 1s (now all 0s after NOT)
        while self.current_word == 0 {
            self.bucket += 1;
            if self.bucket >= self.mask.bits.len() { return None; }
            
            // Flip the bits of the next word
            self.current_word = !self.mask.bits[self.bucket];

            // Handle the "Tail" padding
            if self.bucket == self.mask.bits.len() - 1 {
                let remainder = self.mask.len & 31;
                if remainder != 0 {
                    // Mask off bits beyond the 'len' so we don't treat 
                    // padding zeros as actual row indices
                    self.current_word &= (1 << remainder) - 1;
                }
            }
        }

        // Standard TZCNT / BSF logic
        let bit_idx = self.current_word.trailing_zeros();
        
        // Clear the bit we just found
        self.current_word &= !(1 << bit_idx);
        self.remaining -= 1;
        
        Some((self.bucket << 5) | bit_idx as usize)
    }
}