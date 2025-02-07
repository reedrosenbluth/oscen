use parking_lot::Mutex; // Use parking_lot::Mutex instead of std::sync::Mutex so we don't need to unwarp.
use std::sync::Arc;

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

pub fn signals(rack: &mut Rack, start: u32, end: u32, sample_rate: f32) -> Vec<(f32, f32)> {
    let mut result = vec![];
    for i in start..=end {
        result.push((i as f32 / sample_rate, rack.mono(sample_rate)));
    }
    result
}

pub trait AsBool: Copy {
    fn as_bool(self) -> bool;
}

impl AsBool for f32 {
    fn as_bool(self) -> bool {
        self > 0.0
    }
}

impl AsBool for f64 {
    fn as_bool(self) -> bool {
        self > 0.0
    }
}

pub trait AsUsize: Copy {
    fn as_usize(self) -> usize;
}

impl AsUsize for f32 {
    fn as_usize(self) -> usize {
        self.clamp(0.0, 255.0) as usize
    }
}

impl AsUsize for f64 {
    fn as_usize(self) -> usize {
        self.clamp(0.0, 255.0) as usize
    }
}

pub type ArcMutex<T> = Arc<Mutex<T>>;

pub fn arc_mutex<T>(t: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(t))
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
        assert_eq!(result, 0, "interp returned {}, expected 0", result);
        let result = trunc4(ie_inv(0.4));
        assert_eq!(result, 5_000, "interp returned {}, expected 4,000", result);
        let result = trunc4(ie_inv(0.6697));
        assert_eq!(result, 7_500, "interp returned {}, expected 7,500", result);
        let result = trunc4(ie_inv(1.0));
        assert_eq!(
            result, 10_000,
            "interp returned {}, expected 10,1000",
            result
        );
    }

    #[test]
    fn as_bool() {
        assert_eq!(0.0.as_bool(), false);
        assert_eq!(0.1.as_bool(), true);
        assert_eq!((-0.1).as_bool(), false);
        assert_eq!((1.1).as_bool(), true);
    }

    #[test]
    fn as_usize() {
        assert_eq!(0.0.as_usize(), 0);
        assert_eq!(0.5.as_usize(), 0);
        assert_eq!((-5.1).as_usize(), 0);
        assert_eq!((134.231).as_usize(), 134);
        assert_eq!((1304.231).as_usize(), 255);
    }
}
