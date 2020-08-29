use crate::rack::*;
use approx::relative_eq;

/// Given f(0) = low, f(1/2) = mid, and f(1) = high, let f(x) = a + b*exp(cs).
/// Fit a, b, and c so to match the above. If mid < 1/2(high + low) then f is
/// convex, if equal f is linear, if greater then f is concave.
pub fn interp(low: f32, mid: f32, high: f32, x: f32) -> f32 {
    if relative_eq!(high - mid, mid - low) {
        low + (high - low) * x
    } else {
        let b = (mid - low) * (mid - low) / (high - 2.0 * mid + low);
        let a = low - b;
        let c = 2.0 * ((high - mid) / (mid - low)).ln();
        a + b * (c * x).exp()
    }
}

pub fn interp_inv(low: f32, mid: f32, high: f32, y: f32) -> f32 {
    if relative_eq!(high - mid, mid - low) {
        (y - low) / (high - low)
    } else {
        let b = (mid - low) * (mid - low) / (high - 2.0 * mid + low);
        let a = low - b;
        let c = 2.0 * ((high - mid) / (mid - low)).ln();
        ((y - a) / b).ln() / c
    }
}

pub fn signals<T>(rack: &mut Rack, start: u32, end: u32, sample_rate: f32) -> Vec<(f32, f32)> {
    let controls = Controls::new();
    let mut state = State::new();
    let mut outputs = Outputs::new();
    let mut result = vec![];
    for i in start..=end {
        result.push((
            i as f32 / sample_rate as f32,
            rack.mono(&controls, &mut state, &mut outputs, sample_rate),
        ));
    }
    result
}

/// Variable length circular buffer.
pub struct RingBuffer<'a, T> {
    buffer: &'a mut [T],
    pub read_pos: f32,
    pub write_pos: usize,
}

impl<'a, T> RingBuffer<'a, T>
where
    T: Clone + Default,
{
    pub fn new(buffer: &'a mut [T], read_pos: f32, write_pos: usize) -> Self {
        assert!(
            read_pos.trunc() as usize <= write_pos,
            "Read position must be <= write postion"
        );
        assert!(
            write_pos < buffer.len(),
            "Write postion is >= to buffer length"
        );
        RingBuffer {
            // +3 is to give room for cubic interpolation
            buffer,
            read_pos,
            write_pos,
        }
    }

    pub fn push(&mut self, v: T) {
        let n = self.buffer.len();
        self.write_pos = (self.write_pos + 1) % n;
        self.read_pos = (self.read_pos + 1.0) % n as f32;
        self.buffer[self.write_pos] = v;
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn set_read_pos(&mut self, rp: f32) {
        self.read_pos = rp % self.buffer.len() as f32;
    }

    pub fn set_write_pos(&mut self, wp: usize) {
        self.write_pos = wp % self.buffer.len();
    }
}

impl<'a, T> RingBuffer<'a, T>
where
    T: Copy + Default,
{
    pub fn get(&self) -> T {
        self.buffer[self.read_pos.trunc() as usize]
    }

    pub fn get_offset(&self, offset: i32) -> T {
        let n = self.buffer.len() as i32;
        let mut offset = offset;
        while offset < 0 {
            offset += n;
        }
        let i = (self.read_pos.trunc() as usize + offset as usize) % n as usize;
        self.buffer[i]
    }
}

impl<'a> RingBuffer<'a, f32> {
    pub fn get_linear(&self) -> f32 {
        let f = self.read_pos - self.read_pos.trunc();
        (1.0 - f) * self.get() + f * self.get_offset(1)
    }

    /// Hermite cubic polynomial interpolation.
    pub fn get_cubic(&self) -> f32 {
        let v0 = self.get_offset(-1);
        let v1 = self.get();
        let v2 = self.get_offset(1);
        let v3 = self.get_offset(2);
        let f = self.read_pos - self.read_pos.trunc();
        let a1 = 0.5 * (v2 - v0);
        let a2 = v0 - 2.5 * v1 + 2.0 * v2 - 0.5 * v3;
        let a3 = 0.5 * (v3 - v0) + 1.5 * (v1 - v2);
        a3 * f * f * f + a2 * f * f + a1 * f + v1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::relative_eq;

    fn trunc4(x: f32) -> i32 {
        (10_000.0 * x + 0.5) as i32
    }
    #[test]
    fn linear_interp() {
        fn ie(x: f32) -> f32 {
            interp(0.0, 0.5, 1.0, x)
        }
        assert!(relative_eq!(ie(0.0), 0.0));
        assert!(relative_eq!(ie(0.5), 0.5));
        assert!(relative_eq!(ie(0.75), 0.75));
        assert!(relative_eq!(ie(1.0), 1.0));
    }
    #[test]
    fn exp_interp() {
        fn ie(x: f32) -> f32 {
            interp(0.0, 0.4, 1.0, x)
        }
        let result = trunc4(ie(0.0));
        assert_eq!(result, 0, "interp returned {}, epxected 0", result);
        let result = trunc4(ie(0.5));
        assert_eq!(result, 4_000, "interp returned {}, epxected 4,000", result);
        let result = trunc4(ie(0.75));
        assert_eq!(result, 6_697, "interp returned {}, epxected 6,697", result);
        let result = trunc4(ie(1.0));
        assert_eq!(
            result, 10_000,
            "interp returned {}, epxected 10,1000",
            result
        );
    }
    #[test]
    fn linear_interp_inv() {
        fn ie_inv(x: f32) -> f32 {
            interp_inv(0.0, 0.5, 1.0, x)
        }
        assert!(relative_eq!(ie_inv(0.0), 0.0));
        assert!(relative_eq!(ie_inv(0.5), 0.5));
        assert!(relative_eq!(ie_inv(0.75), 0.75));
        assert!(relative_eq!(ie_inv(1.0), 1.0));
    }
    #[test]
    fn exp_interp_inv() {
        fn ie_inv(x: f32) -> f32 {
            interp_inv(0.0, 0.4, 1.0, x)
        }
        let result = trunc4(ie_inv(0.0));
        assert_eq!(result, 0, "interp returned {}, epxected 0", result);
        let result = trunc4(ie_inv(0.4));
        assert_eq!(result, 5_000, "interp returned {}, epxected 4,000", result);
        let result = trunc4(ie_inv(0.6697));
        assert_eq!(result, 7_500, "interp returned {}, epxected 7,500", result);
        let result = trunc4(ie_inv(1.0));
        assert_eq!(
            result, 10_000,
            "interp returned {}, epxected 10,1000",
            result
        );
    }

    #[test]
    fn ring_buffer() {
        let buffer = &mut vec![0.0; 10];
        let mut rb = RingBuffer::<f32>::new(buffer, 0.5, 5);
        let result = rb.get();
        assert_eq!(result, 0.0, "get returned {}, expected 0.0", result);
        for i in 0..=6 {
            rb.push(i as f32);
        }
        let result = rb.get();
        assert_eq!(result, 1.0, "get returned {}, expected 0.0", result);
        let result = rb.get_linear();
        assert_eq!(result, 1.5, "get_linear returned {}, expected 0.0", result);
        let result = rb.get_cubic();
        assert_eq!(result, 1.5, "get_cubic returned {}, expected 0.0", result);
    }
}
