use crate::rack::Real;
use approx::relative_eq;

/// Given f(0) = low, f(1/2) = mid, and f(1) = high, let f(x) = a + b*exp(cs).
/// Fit a, b, and c so to match the above. If mid < 1/2(high + low) then f is
/// convex, if equal f is linear, if greater then f is concave.
#[derive(Copy, Clone, Debug)]
pub struct ExpInterp {
    low: Real,
    mid: Real,
    high: Real,
    a: Real,
    b: Real,
    c: Real,
    linear: bool,
}

impl ExpInterp {
    pub fn new(low: Real, mid: Real, high: Real) -> Self {
        let mut exp_interp = ExpInterp {
            low,
            mid,
            high,
            a: 0.0,
            b: 0.0,
            c: 0.0,
            linear: true,
        };
        if relative_eq!(high - mid, mid - low) {
            exp_interp
        } else {
            exp_interp.update(low, mid, high);
            exp_interp
        }
    }

    pub fn update(&mut self, low: Real, mid: Real, high: Real) {
        if relative_eq!(high - mid, mid - low) {
            self.low = low;
            self.mid = mid;
            self.high = high;
            self.linear = true;
        } else {
            self.b = (mid - low) * (mid - low) / (high - 2.0 * mid + low);
            self.a = low - self.b;
            self.c = 2.0 * ((high - mid) / (mid - low)).ln();
            self.linear = false;
        }
    }

    /// Interpolate according to f(x).
    pub fn interp(&self, x: Real) -> Real {
        if self.linear {
            self.low + (self.high - self.low) * x
        } else {
            self.a + self.b * (self.c * x).exp()
        }
    }

    /// Inverse of interpolation function f.
    pub fn interp_inv(&self, y: Real) -> Real {
        if self.linear {
            (y - self.low) / (self.high - self.low)
        } else {
            ((y - self.a) / self.b).ln() / self.c
        }
    }
}

// pub fn signals<T>(module: &mut T, start: u32, end: u32, sample_rate: Real) -> Vec<(f32, f32)>
// where
//     T: Signal,
// {
//     let rack = Rack::new();
//     let mut result = vec![];
//     for i in start..=end {
//         result.push((
//             i as f32 / sample_rate as f32,
//             module.signal(&rack, sample_rate)[0] as f32,
//         ));
//     }
//     result
// }

/// Variable length circular buffer.
#[derive(Clone)]
pub struct RingBuffer<T> {
    buffer: Vec<T>,
    pub read_pos: Real,
    pub write_pos: usize,
}

impl<T> RingBuffer<T>
where
    T: Clone + Default,
{
    pub fn new(read_pos: Real, write_pos: usize) -> Self {
        assert!(
            read_pos.trunc() as usize <= write_pos,
            "Read position must be <= write postion"
        );
        RingBuffer {
            // +3 is to give room for cubic interpolation
            buffer: vec![Default::default(); write_pos + 3],
            read_pos,
            write_pos,
        }
    }

    pub fn push(&mut self, v: T) {
        let n = self.buffer.len();
        self.write_pos = (self.write_pos + 1) % n;
        self.read_pos = (self.read_pos + 1.0) % n as Real;
        self.buffer[self.write_pos] = v;
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn resize(&mut self, size: usize) {
        self.buffer.resize_with(size, Default::default);
    }

    pub fn set_read_pos(&mut self, rp: Real) {
        self.read_pos = rp % self.buffer.len() as Real;
    }

    pub fn set_write_pos(&mut self, wp: usize) {
        self.write_pos = wp % self.buffer.len();
    }
}

impl<T> RingBuffer<T>
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

impl RingBuffer<Real> {
    pub fn get_linear(&self) -> Real {
        let f = self.read_pos - self.read_pos.trunc();
        (1.0 - f) * self.get() + f * self.get_offset(1)
    }

    /// Hermite cubic polynomial interpolation.
    pub fn get_cubic(&self) -> Real {
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

    fn trunc4(x: Real) -> i32 {
        (10_000.0 * x + 0.5) as i32
    }
    #[test]
    fn linear_interp() {
        let ie = ExpInterp::new(0.0, 0.5, 1.0);
        assert!(relative_eq!(ie.interp(0.0), 0.0));
        assert!(relative_eq!(ie.interp(0.5), 0.5));
        assert!(relative_eq!(ie.interp(0.75), 0.75));
        assert!(relative_eq!(ie.interp(1.0), 1.0));
    }
    #[test]
    fn exp_interp() {
        let ie = ExpInterp::new(0.0, 0.4, 1.0);
        let result = trunc4(ie.interp(0.0));
        assert_eq!(result, 0, "interp returned {}, epxected 0", result);
        let result = trunc4(ie.interp(0.5));
        assert_eq!(result, 4_000, "interp returned {}, epxected 4,000", result);
        let result = trunc4(ie.interp(0.75));
        assert_eq!(result, 6_697, "interp returned {}, epxected 6,697", result);
        let result = trunc4(ie.interp(1.0));
        assert_eq!(
            result, 10_000,
            "interp returned {}, epxected 10,1000",
            result
        );
    }
    #[test]
    fn linear_interp_inv() {
        let ie = ExpInterp::new(0.0, 0.5, 1.0);
        assert!(relative_eq!(ie.interp_inv(0.0), 0.0));
        assert!(relative_eq!(ie.interp_inv(0.5), 0.5));
        assert!(relative_eq!(ie.interp_inv(0.75), 0.75));
        assert!(relative_eq!(ie.interp_inv(1.0), 1.0));
    }
    #[test]
    fn exp_interp_inv() {
        let ie = ExpInterp::new(0.0, 0.4, 1.0);
        let result = trunc4(ie.interp_inv(0.0));
        assert_eq!(result, 0, "interp returned {}, epxected 0", result);
        let result = trunc4(ie.interp_inv(0.4));
        assert_eq!(result, 5_000, "interp returned {}, epxected 4,000", result);
        let result = trunc4(ie.interp_inv(0.6697));
        assert_eq!(result, 7_500, "interp returned {}, epxected 7,500", result);
        let result = trunc4(ie.interp_inv(1.0));
        assert_eq!(
            result, 10_000,
            "interp returned {}, epxected 10,1000",
            result
        );
    }

    #[test]
    fn ring_buffer() {
        let mut rb = RingBuffer::<Real>::new(0.5, 5);
        let result = rb.get();
        assert_eq!(result, 0.0, "get returned {}, expected 0.0", result);
        for i in 0..=6 {
            rb.push(i as Real);
        }
        let result = rb.get();
        assert_eq!(result, 1.0, "get returned {}, expected 0.0", result);
        let result = rb.get_linear();
        assert_eq!(result, 1.5, "get_linear returned {}, expected 0.0", result);
        let result = rb.get_cubic();
        assert_eq!(result, 1.5, "get_cubic returned {}, expected 0.0", result);
    }

    #[test]
    fn ring_buffer_resize() {
        let mut rb = RingBuffer::<Real>::new(0.5, 5);
        rb.resize(10);
        for i in 0..=6 {
            rb.push(i as Real);
        }
        let result = rb.get_linear();
        assert_eq!(result, 1.5, "get_linear returned {}, expected 0.0", result);
        let result = rb.get_cubic();
        assert_eq!(result, 1.5, "get_cubic returned {}, expected 0.0", result);
    }
}
