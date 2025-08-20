/// Represents the mode of operation for a buffer's size management
/// - PowerOfTwo: Buffer size is rounded up to the next power of 2
/// - Exact: Buffer size is kept exactly as specified
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BufferMode {
    #[default]
    PowerOfTwo,
    Exact,
}

/// A ring buffer implementation with linear and cubic interpolation for reading values.
/// Uses heap allocation to avoid stack overflow issues with large buffers.
#[derive(Clone, Debug)]
pub struct RingBuffer {
    /// The internal buffer storing samples (heap-allocated)
    buffer: Vec<f32>,
    /// Current write position in the buffer
    write_pos: usize,
    /// Actual capacity/size of the buffer being used.
    capacity: usize,
    /// Mask used for efficient wrapping of indices (only valid in PowerOfTwo mode)
    mask: usize,
    /// Buffer mode: PowerOfTwo for power-of-2 sized buffers, Exact for exact sizes
    mode: BufferMode,
}

impl RingBuffer {
    /// Creates a new RingBuffer with the specified size and default PowerOfTwo mode.
    pub fn new(size: usize) -> Self {
        Self::with_mode(size, BufferMode::default())
    }

    /// Creates a new RingBuffer with the specified size and mode.
    /// The buffer capacity will be the exact specified size or the next power of two.
    pub fn with_mode(size: usize, mode: BufferMode) -> Self {
        let capacity = match mode {
            BufferMode::PowerOfTwo => {
                // Ensure capacity is at least 1, then find next power of two
                size.max(1).next_power_of_two()
            }
            BufferMode::Exact => size.max(1), // Ensure logical capacity is at least 1
        };

        // Initialize the buffer with zeros up to the capacity (heap allocation)
        let buffer = vec![0.0; capacity];

        Self {
            buffer,
            write_pos: 0,
            capacity,
            mask: capacity.wrapping_sub(1), // mask is capacity - 1, correct for power-of-two
            mode,
        }
    }

    /// Pushes a new value into the buffer, advancing the write position correctly based on mode.
    pub fn push(&mut self, v: f32) {
        // Check if buffer has logical capacity
        if self.capacity == 0 {
            // This case should ideally not happen with capacity >= 1 guarantee, but check defensively.
            return;
        }

        // Ensure write_pos is valid before indexing. This should always hold if capacity > 0.
        debug_assert!(self.write_pos < self.capacity);

        // Write to the current position *before* incrementing, overwriting the oldest sample.
        // Access is safe because write_pos is always < capacity.
        self.buffer[self.write_pos] = v;

        // Advance write position with correct wrapping
        self.write_pos = match self.mode {
            BufferMode::PowerOfTwo => (self.write_pos + 1) & self.mask,
            BufferMode::Exact => (self.write_pos + 1) % self.capacity,
        };
    }

    /// Calculates the read position for a given offset, handling wrapping.
    /// Returns a float index.
    fn read_pos(&self, offset: f32) -> f32 {
        if self.capacity == 0 {
            return 0.0; // Should not happen with capacity >= 1
        }

        let n = self.capacity as f32;
        // Calculate raw read position relative to write_pos (offset samples ago)
        let rp = self.write_pos as f32 - offset - 1.0; // -1 because write_pos is the *next* spot to write

        // Wrap the read position correctly to [0, n) (handles negative results)
        (rp % n + n) % n
    }

    /// Gets a value from the buffer using linear interpolation.
    fn get_linear(&self, offset: f32) -> f32 {
        if self.capacity == 0 {
            return 0.0; // Should not happen
        }

        let rp = self.read_pos(offset);
        let i = rp as usize; // Integer part (index)
        let f = rp.fract(); // Fractional part

        // Ensure indices are within bounds (should be guaranteed by read_pos wrapping)
        debug_assert!(i < self.capacity);

        let idx0 = i;
        let idx1 = match self.mode {
            BufferMode::PowerOfTwo => (i + 1) & self.mask,
            BufferMode::Exact => (i + 1) % self.capacity,
        };

        // Accesses are safe due to wrapping logic and capacity checks
        let a = self.buffer[idx0];
        let b = self.buffer[idx1];

        // Linear interpolation: a + f * (b - a)
        a.mul_add(1.0 - f, b * f)
    }

    /// Gets a value from the buffer using cubic interpolation (Catmull-Rom).
    fn get_cubic(&self, offset: f32) -> f32 {
        if self.capacity < 4 {
            // Need at least 4 points for cubic interpolation
            return self.get_linear(offset); // Fallback to linear
        }

        let rp = self.read_pos(offset);
        let i = rp as usize; // Integer part (index)
        let f = rp.fract(); // Fractional part

        // Ensure base index is within bounds
        debug_assert!(i < self.capacity);

        // Calculate indices using correct wrapping for the mode
        let (im1, i0, i1, i2) = match self.mode {
            BufferMode::PowerOfTwo => (
                (i.wrapping_sub(1)) & self.mask,
                i,
                (i + 1) & self.mask,
                (i + 2) & self.mask,
            ),
            BufferMode::Exact => (
                (i + self.capacity - 1) % self.capacity, // Safe wrap-around for usize
                i,
                (i + 1) % self.capacity,
                (i + 2) % self.capacity,
            ),
        };

        // Accesses are safe due to wrapping logic
        let v0 = self.buffer[im1];
        let v1 = self.buffer[i0];
        let v2 = self.buffer[i1];
        let v3 = self.buffer[i2];

        // Catmull-Rom spline formula for interpolation
        let c0 = v1;
        let c1 = 0.5 * (v2 - v0);
        let c2 = v0 - 2.5 * v1 + 2.0 * v2 - 0.5 * v3;
        let c3 = 0.5 * (v3 - v0) + 1.5 * (v1 - v2);

        // Evaluate cubic polynomial: c0 + f * (c1 + f * (c2 + f * c3))
        c0 + f * (c1 + f * (c2 + f * c3))
    }

    /// Reads 'offset' samples into the past. Offset 0 is the most recent sample.
    pub fn get(&self, offset: f32) -> f32 {
        if self.capacity == 0 {
            return 0.0; // Should not happen
        }

        // Ensure offset is non-negative, but DON'T clamp the upper bound here.
        let non_negative_offset = offset.max(0.0);

        // If offset is very close to an integer, return the corresponding exact sample
        // to avoid potential floating point inaccuracies near sample boundaries.
        // Use the non-clamped offset for the fraction check.
        if (non_negative_offset.fract() < 1e-6) || ((1.0 - non_negative_offset.fract()) < 1e-6) {
            // Calculate the integer read index correctly using modulo arithmetic
            // for wrapping the offset itself relative to the buffer capacity.
            let offset_samples = non_negative_offset.round() as usize;
            // Ensure capacity > 0 before modulo
            let read_idx = if self.capacity > 0 {
                ((self.write_pos + self.capacity) - (offset_samples % self.capacity) - 1)
                    % self.capacity
            } else {
                0 // Or handle error, though capacity should be >= 1
            };
            // Safe access if capacity > 0
            return self.buffer[read_idx];
        }

        // Choose interpolation method based on available capacity.
        // Pass the original non-negative (but potentially > capacity) offset
        // to the interpolation functions. `read_pos` inside them handles wrapping.
        if self.capacity >= 4 {
            self.get_cubic(non_negative_offset)
        } else {
            self.get_linear(non_negative_offset)
        }
    }

    /// Sets the size (logical capacity) of the buffer, preserving existing data where possible.
    /// Note: This method allocates memory and should NOT be called during audio processing.
    pub fn set_size(&mut self, new_size: usize) {
        let old_capacity = self.capacity;
        let new_capacity = match self.mode {
            BufferMode::PowerOfTwo => new_size
                .max(1) // Ensure logical capacity is at least 1
                .next_power_of_two(),
            BufferMode::Exact => new_size.max(1), // Ensure logical capacity is at least 1
        };

        if new_capacity == old_capacity {
            return;
        }

        // how many samples to preserve
        let count_to_preserve = old_capacity.min(new_capacity);
        let mut preserved_data = Vec::with_capacity(count_to_preserve);

        if count_to_preserve > 0 {
            // Calculate the index of the oldest sample to preserve.
            // The samples range from `count_to_preserve` samples ago up to the most recent sample.
            let start_read_idx = match self.mode {
                BufferMode::PowerOfTwo => {
                    (self
                    .write_pos
                    .wrapping_add(old_capacity) // Go to the element *after* the newest
                    .wrapping_sub(count_to_preserve)) // Go back count_to_preserve steps
                    & self.mask
                } // ^ Apply old mask
                BufferMode::Exact => {
                    (self.write_pos + old_capacity - count_to_preserve) % old_capacity
                }
            };

            // Copy the most recent `count_to_preserve` samples from the old buffer
            // into preserved_data, ordered from oldest-preserved to newest-preserved.
            for i in 0..count_to_preserve {
                let read_idx = match self.mode {
                    BufferMode::PowerOfTwo => (start_read_idx.wrapping_add(i)) & self.mask, // Use old mask
                    BufferMode::Exact => (start_read_idx + i) % old_capacity,
                };
                debug_assert!(read_idx < old_capacity);
                preserved_data.push(self.buffer[read_idx]);
            }
        }

        // Resize the internal buffer, filling with zeros
        self.buffer.resize(new_capacity, 0.0);

        // Copy preserved data into the *end* of the new buffer
        if count_to_preserve > 0 {
            // Calculate the starting index in the new buffer to write preserved data
            let start_write_idx = new_capacity.saturating_sub(count_to_preserve);
            for (i, &value) in preserved_data.iter().enumerate().take(count_to_preserve) {
                self.buffer[start_write_idx + i] = value;
            }
        }

        // Update capacity, mask, and write position
        self.capacity = new_capacity;
        self.mask = new_capacity.wrapping_sub(1);
        // After resizing and copying, the write position resets to the start,
        // as the next write should overwrite the oldest sample (which is either a
        // preserved sample at index 0 if shrinking/same size, or a new zero at index 0 if growing).
        self.write_pos = 0;
    }

    /// Returns the current logical capacity of the buffer.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests;
