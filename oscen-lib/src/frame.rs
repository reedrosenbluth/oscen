//! Multi-channel audio frames: one sample-instant across N channels.

/// One sample-instant across N channels. The audio-standard "frame".
///
/// `f32` is the canonical mono type; `Frame<N>` is the multi-channel value a
/// stream edge can carry. Element type is fixed to `f32` by design — see the
/// design spec, decision #4.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Frame<const N: usize>(pub [f32; N]);

impl<const N: usize> Default for Frame<N> {
    #[inline]
    fn default() -> Self {
        Frame([0.0; N])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_zeros() {
        assert_eq!(Frame::<2>::default(), Frame([0.0, 0.0]));
        assert_eq!(Frame::<4>::default(), Frame([0.0; 4]));
    }

    #[test]
    fn construct_and_index_channels() {
        let f = Frame([0.25_f32, -0.5]);
        assert_eq!(f.0[0], 0.25);
        assert_eq!(f.0[1], -0.5);
    }

    #[test]
    fn equality_is_elementwise() {
        assert_eq!(Frame([1.0, 2.0]), Frame([1.0, 2.0]));
        assert_ne!(Frame([1.0, 2.0]), Frame([1.0, 2.5]));
    }
}
