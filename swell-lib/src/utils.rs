use super::signal::Real;
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
        assert_eq!(
            result, 
            0,
            "interp returned {}, epxected 0",
            result
        );
        let result = trunc4(ie.interp(0.5));
        assert_eq!(
            result, 
            4_000,
            "interp returned {}, epxected 4,000",
            result
        );
        let result = trunc4(ie.interp(0.75));
        assert_eq!(
            result, 
            6_697,
            "interp returned {}, epxected 6,697",
            result
        );
        let result = trunc4(ie.interp(1.0));
        assert_eq!(
            result, 
            10_000,
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
        assert_eq!(
            result, 
            0,
            "interp returned {}, epxected 0",
            result
        );
        let result = trunc4(ie.interp_inv(0.4));
        assert_eq!(
            result, 
            5_000,
            "interp returned {}, epxected 4,000",
            result
        );
        let result = trunc4(ie.interp_inv(0.6697));
        assert_eq!(
            result, 
            7_500,
            "interp returned {}, epxected 7,500",
            result
        );
        let result = trunc4(ie.interp_inv(1.0));
        assert_eq!(
            result, 
            10_000,
            "interp returned {}, epxected 10,1000",
            result
        );
    }
}
