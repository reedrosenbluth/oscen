//! Per-edge fan-out shape: how many source values feed how many dest slots.
//! Used by both the same-rate emitter (to choose between scalar/parallel/
//! broadcast/fan-in `ConnectEndpoints` calls) and the cross-rate emitter
//! (to choose between shared and per-element resampler state).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanoutShape {
    /// Both sides scalar (or expression with no array root).
    Scalar,
    /// Both sides arrays of equal size N: parallel — one resampler per element.
    Parallel { n: usize },
    /// Scalar src → array dest of size N: broadcast — shared resampler, N dest writes.
    Broadcast { n: usize },
    /// Array src of size N → scalar dest: fan-in — sum sources first, then shared resampler.
    FanIn { n: usize },
}

/// Classify a connection edge given the array sizes of its source and dest
/// nodes (`None` for scalar nodes or graph endpoints).
///
/// For mismatched-but-nonzero array sizes, parity with the same-rate path's
/// existing behavior: silently truncate to `min(N, M)` (`Parallel { n: min }`).
/// Promoting this to a hard error is reserved for a future task.
pub fn classify_fanout(
    src_array_size: Option<usize>,
    dst_array_size: Option<usize>,
) -> FanoutShape {
    use FanoutShape::*;
    match (src_array_size, dst_array_size) {
        (None, None) => Scalar,
        (None, Some(n)) => Broadcast { n },
        (Some(n), None) => FanIn { n },
        (Some(n), Some(m)) if n == m => Parallel { n },
        (Some(n), Some(m)) => Parallel { n: n.min(m) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_to_scalar_is_scalar() {
        assert_eq!(classify_fanout(None, None), FanoutShape::Scalar);
    }

    #[test]
    fn scalar_to_array_is_broadcast() {
        assert_eq!(classify_fanout(None, Some(4)), FanoutShape::Broadcast { n: 4 });
    }

    #[test]
    fn array_to_scalar_is_fanin() {
        assert_eq!(classify_fanout(Some(8), None), FanoutShape::FanIn { n: 8 });
    }

    #[test]
    fn equal_arrays_are_parallel() {
        assert_eq!(classify_fanout(Some(4), Some(4)), FanoutShape::Parallel { n: 4 });
    }

    #[test]
    fn mismatched_arrays_truncate_to_min() {
        assert_eq!(classify_fanout(Some(4), Some(8)), FanoutShape::Parallel { n: 4 });
        assert_eq!(classify_fanout(Some(8), Some(4)), FanoutShape::Parallel { n: 4 });
    }
}
