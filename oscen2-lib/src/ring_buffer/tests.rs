use super::*;
use float_cmp::assert_approx_eq;

const TEST_MAX_SIZE: usize = 16;

#[test]
fn test_initialization_power_of_two() {
    let buf = RingBuffer::<TEST_MAX_SIZE>::with_mode(5, BufferMode::PowerOfTwo);
    assert_eq!(buf.capacity(), 8); // Next power of 2 >= 5
    assert_eq!(buf.mask, 7);
    assert_eq!(buf.mode, BufferMode::PowerOfTwo);
    assert_eq!(buf.buffer.len(), 8); // Logical length
    assert_eq!(buf.buffer.capacity(), TEST_MAX_SIZE); // Max capacity
    assert!(buf.buffer.iter().all(|&x| x == 0.0));

    let buf = RingBuffer::<TEST_MAX_SIZE>::with_mode(8, BufferMode::PowerOfTwo);
    assert_eq!(buf.capacity(), 8);
    assert_eq!(buf.mask, 7);
    assert_eq!(buf.buffer.len(), 8);

    let buf = RingBuffer::<TEST_MAX_SIZE>::with_mode(9, BufferMode::PowerOfTwo);
    assert_eq!(buf.capacity(), 16); // Next power of 2 is 16, <= TEST_MAX_SIZE
    assert_eq!(buf.mask, 15);
    assert_eq!(buf.buffer.len(), 16);

    // Test clamping to N (TEST_MAX_SIZE)
    let buf = RingBuffer::<TEST_MAX_SIZE>::with_mode(TEST_MAX_SIZE + 5, BufferMode::PowerOfTwo);
    // Requested 21, clamped to 16. Next power of 2 is 16.
    assert_eq!(buf.capacity(), TEST_MAX_SIZE); // Capacity clamped by N
    assert_eq!(buf.mask, TEST_MAX_SIZE - 1);
    assert_eq!(buf.buffer.len(), TEST_MAX_SIZE);

    let buf = RingBuffer::<TEST_MAX_SIZE>::with_mode(0, BufferMode::PowerOfTwo);
    assert_eq!(buf.capacity(), 1); // Minimum logical capacity is 1
    assert_eq!(buf.buffer.len(), 1);
}

#[test]
fn test_initialization_exact() {
    let buf = RingBuffer::<TEST_MAX_SIZE>::with_mode(5, BufferMode::Exact);
    assert_eq!(buf.capacity(), 5);
    assert_eq!(buf.mode, BufferMode::Exact);
    assert_eq!(buf.buffer.len(), 5);
    assert_eq!(buf.buffer.capacity(), TEST_MAX_SIZE);
    assert!(buf.buffer.iter().all(|&x| x == 0.0));

    let buf = RingBuffer::<TEST_MAX_SIZE>::with_mode(8, BufferMode::Exact);
    assert_eq!(buf.capacity(), 8);
    assert_eq!(buf.buffer.len(), 8);

    // Test clamping to N
    let buf = RingBuffer::<TEST_MAX_SIZE>::with_mode(TEST_MAX_SIZE + 5, BufferMode::Exact);
    assert_eq!(buf.capacity(), TEST_MAX_SIZE); // Clamped to N
    assert_eq!(buf.buffer.len(), TEST_MAX_SIZE);

    let buf = RingBuffer::<TEST_MAX_SIZE>::with_mode(0, BufferMode::Exact);
    assert_eq!(buf.capacity(), 1); // Minimum logical capacity is 1
    assert_eq!(buf.buffer.len(), 1);
}

#[test]
fn test_push_and_wrap_power_of_two() {
    let mut buf = RingBuffer::<4>::with_mode(4, BufferMode::PowerOfTwo); // N=4, size=4 -> capacity=4
    assert_eq!(buf.capacity(), 4);

    buf.push(1.0);
    buf.push(2.0);
    buf.push(3.0);
    buf.push(4.0);
    // Buffer: [1.0, 2.0, 3.0, 4.0], write_pos = 0 (wrapped)
    assert_eq!(buf.write_pos, 0);
    assert_eq!(buf.buffer[0], 1.0);
    assert_eq!(buf.buffer[3], 4.0);

    buf.push(5.0); // Overwrites 1.0
                   // Buffer: [5.0, 2.0, 3.0, 4.0], write_pos = 1
    assert_eq!(buf.write_pos, 1);
    assert_eq!(buf.buffer[0], 5.0);
    assert_eq!(buf.buffer[1], 2.0);

    buf.push(6.0); // Overwrites 2.0
                   // Buffer: [5.0, 6.0, 3.0, 4.0], write_pos = 2
    assert_eq!(buf.write_pos, 2);
    assert_eq!(buf.buffer[1], 6.0);
}

#[test]
fn test_push_and_wrap_exact() {
    let mut buf = RingBuffer::<3>::with_mode(3, BufferMode::Exact); // N=3, size=3 -> capacity=3
    assert_eq!(buf.capacity(), 3);

    buf.push(1.0);
    buf.push(2.0);
    buf.push(3.0);
    // Buffer: [1.0, 2.0, 3.0], write_pos = 0 (wrapped)
    assert_eq!(buf.write_pos, 0);
    assert_eq!(buf.buffer[0], 1.0);
    assert_eq!(buf.buffer[2], 3.0);

    buf.push(4.0); // Overwrites 1.0
                   // Buffer: [4.0, 2.0, 3.0], write_pos = 1
    assert_eq!(buf.write_pos, 1);
    assert_eq!(buf.buffer[0], 4.0);
    assert_eq!(buf.buffer[1], 2.0);

    buf.push(5.0); // Overwrites 2.0
                   // Buffer: [4.0, 5.0, 3.0], write_pos = 2
    assert_eq!(buf.write_pos, 2);
    assert_eq!(buf.buffer[1], 5.0);
}

#[test]
fn test_get_exact_offset() {
    let mut buf = RingBuffer::<5>::with_mode(5, BufferMode::Exact); // N=5, capacity=5
    buf.push(1.0); // idx 0
    buf.push(2.0); // idx 1
    buf.push(3.0); // idx 2
    buf.push(4.0); // idx 3, write_pos = 4

    // Most recent sample (offset 0)
    assert_approx_eq!(f32, buf.get(0.0), 4.0, epsilon = 1e-6);
    // 1 sample ago (offset 1)
    assert_approx_eq!(f32, buf.get(1.0), 3.0, epsilon = 1e-6);
    // 2 samples ago (offset 2)
    assert_approx_eq!(f32, buf.get(2.0), 2.0, epsilon = 1e-6);
    // 3 samples ago (offset 3)
    assert_approx_eq!(f32, buf.get(3.0), 1.0, epsilon = 1e-6);

    // Push more to wrap
    buf.push(5.0); // idx 4, write_pos = 0
    buf.push(6.0); // idx 0, write_pos = 1
                   // Buffer state: [6.0, 2.0, 3.0, 4.0, 5.0]

    assert_approx_eq!(f32, buf.get(0.0), 6.0, epsilon = 1e-6); // Most recent
    assert_approx_eq!(f32, buf.get(1.0), 5.0, epsilon = 1e-6);
    assert_approx_eq!(f32, buf.get(2.0), 4.0, epsilon = 1e-6);
    assert_approx_eq!(f32, buf.get(3.0), 3.0, epsilon = 1e-6);
    assert_approx_eq!(f32, buf.get(4.0), 2.0, epsilon = 1e-6); // Oldest value

    // Test wrapping/modulo offset for integer offsets >= capacity
    assert_approx_eq!(f32, buf.get(5.0), 6.0, epsilon = 1e-6); // Offset 5 wraps to 0 (newest value)
    assert_approx_eq!(f32, buf.get(-1.0), 6.0, epsilon = 1e-6); // Clamped to 0 offset
}

#[test]
fn test_get_linear_interpolation() {
    let mut buf = RingBuffer::<4>::with_mode(4, BufferMode::PowerOfTwo); // N=4, size=4 -> capacity=4
    buf.push(1.0); // idx 0
    buf.push(3.0); // idx 1
    buf.push(5.0); // idx 2
    buf.push(7.0); // idx 3, write_pos = 0
                   // Buffer: [1.0, 3.0, 5.0, 7.0]

    assert_approx_eq!(f32, buf.get(0.0), 7.0, epsilon = 1e-6); // Exact
    assert_approx_eq!(f32, buf.get(1.0), 5.0, epsilon = 1e-6); // Exact
    assert_approx_eq!(f32, buf.get(2.0), 3.0, epsilon = 1e-6); // Exact
    assert_approx_eq!(f32, buf.get(3.0), 1.0, epsilon = 1e-6); // Exact

    // Interpolated values (get uses cubic for cap=4)
    // Let's test get_linear directly
    assert_approx_eq!(f32, buf.get_linear(0.5), 6.0, epsilon = 1e-6); // Linear between 7.0 (idx 3) and 5.0 (idx 2)
    assert_approx_eq!(f32, buf.get_linear(1.5), 4.0, epsilon = 1e-6); // Linear between 5.0 (idx 2) and 3.0 (idx 1)
    assert_approx_eq!(f32, buf.get_linear(2.5), 2.0, epsilon = 1e-6); // Linear between 3.0 (idx 1) and 1.0 (idx 0)

    // Test wrapping interpolation
    buf.push(9.0); // idx 0, write_pos = 1
                   // Buffer: [9.0, 3.0, 5.0, 7.0]
                   // Offset 0.5 is between newest (idx 0, val 9.0) and second newest (idx 3, val 7.0)
    assert_approx_eq!(f32, buf.get_linear(0.5), 8.0, epsilon = 1e-6);
}

#[test]
fn test_get_cubic_interpolation() {
    let mut buf = RingBuffer::<5>::with_mode(5, BufferMode::Exact); // N=5, capacity=5
                                                                    // Fill with distinct values
    buf.push(1.0); // 0
    buf.push(2.0); // 1
    buf.push(4.0); // 2
    buf.push(8.0); // 3
    buf.push(16.0); // 4, write_pos = 0
                    // Buffer: [1.0, 2.0, 4.0, 8.0, 16.0]

    assert_approx_eq!(f32, buf.get(0.0), 16.0, epsilon = 1e-6); // Exact newest
    assert_approx_eq!(f32, buf.get(1.0), 8.0, epsilon = 1e-6); // Exact
    assert_approx_eq!(f32, buf.get(4.0), 1.0, epsilon = 1e-6); // Exact oldest

    // rp = read_pos(0.5) = (0 - 0.5 - 1 + 5) % 5 = 3.5
    // i = 3, f = 0.5
    // Indices (Exact mode, cap=5): im1=(3+5-1)%5=2, i0=3, i1=(3+1)%5=4, i2=(3+2)%5=0
    // v0=buf[2]=4.0, v1=buf[3]=8.0, v2=buf[4]=16.0, v3=buf[0]=1.0
    let cubic_val = buf.get_cubic(0.5);
    // Values based on trace in previous conversation: 13.1875
    assert_approx_eq!(f32, cubic_val, 13.1875, epsilon = 1e-6);

    // Test fallback to linear for small buffers
    let mut small_buf = RingBuffer::<3>::with_mode(3, BufferMode::Exact); // N=3, capacity=3
    small_buf.push(1.0);
    small_buf.push(5.0);
    small_buf.push(9.0); // write_pos=0, buffer=[1,5,9] -> get(0)=9, get(1)=5, get(2)=1
                         // get(0.5) should use linear because capacity=3 < 4
                         // rp = read_pos(0.5) = (0 - 0.5 - 1 + 3) % 3 = 1.5
                         // i = 1, f = 0.5
                         // idx0 = 1, idx1 = (1+1)%3 = 2
                         // a = buf[1]=5.0, b = buf[2]=9.0 -> WRONG! buffer=[1,5,9], write_pos=0
                         // State: buffer=[1.0(idx0), 5.0(idx1), 9.0(idx2)], write_pos=0
                         // get(0.0) -> rp = (0-0-1+3)%3 = 2. Index = 2. Value = 9.0. OK.
                         // get(1.0) -> rp = (0-1-1+3)%3 = 1. Index = 1. Value = 5.0. OK.
                         // get(2.0) -> rp = (0-2-1+3)%3 = 0. Index = 0. Value = 1.0. OK.
                         // get(0.5) -> rp = (0-0.5-1+3)%3 = 1.5. i=1, f=0.5.
                         // Indices for linear: idx0=1, idx1=(1+1)%3=2
                         // Values: a=buf[1]=5.0, b=buf[2]=9.0
                         // Lerp: 5.0*0.5 + 9.0*0.5 = 2.5 + 4.5 = 7.0
    assert_approx_eq!(f32, small_buf.get(0.5), 7.0, epsilon = 1e-6); // Linear between 9 and 5 (ERROR in comment, should be 5 and 9)
}

#[test]
fn test_set_size_growing() {
    let mut buf = RingBuffer::<10>::with_mode(4, BufferMode::Exact); // N=10, cap=4
    buf.push(1.0); // [1]
    buf.push(2.0); // [1, 2]
    buf.push(3.0); // [1, 2, 3]
    buf.push(4.0); // [1, 2, 3, 4], write_pos = 0
    buf.push(5.0); // [5, 2, 3, 4], write_pos = 1 (Most recent: 5, 4, 3, 2)

    buf.set_size(7); // Grow to 7
    assert_eq!(buf.capacity(), 7);
    assert_eq!(buf.write_pos, 0);
    // Expected: [0.0, 0.0, 0.0, 2.0, 3.0, 4.0, 5.0] (preserved 4 newest)
    assert_approx_eq!(f32, buf.buffer[0], 0.0);
    assert_approx_eq!(f32, buf.buffer[1], 0.0);
    assert_approx_eq!(f32, buf.buffer[2], 0.0);
    assert_approx_eq!(f32, buf.buffer[3], 2.0); // Oldest preserved
    assert_approx_eq!(f32, buf.buffer[4], 3.0);
    assert_approx_eq!(f32, buf.buffer[5], 4.0);
    assert_approx_eq!(f32, buf.buffer[6], 5.0); // Newest preserved
                                                // Check get() still works relative to new state
    assert_approx_eq!(f32, buf.get(0.0), 5.0); // Newest
    assert_approx_eq!(f32, buf.get(3.0), 2.0); // Oldest preserved
    assert_approx_eq!(f32, buf.get(4.0), 0.0); // Reading into the new zero space
}

#[test]
fn test_set_size_shrinking() {
    let mut buf = RingBuffer::<10>::with_mode(8, BufferMode::Exact); // N=10, cap=8
    for i in 1..=8 {
        buf.push(i as f32);
    }
    // Buffer contains [1..=8], write_pos = 0. Most recent: 8, 7, 6, 5, 4, 3, 2, 1

    buf.set_size(5); // Shrink to 5
    assert_eq!(buf.capacity(), 5);
    assert_eq!(buf.write_pos, 0);
    // Expected: [4.0, 5.0, 6.0, 7.0, 8.0] (preserved 5 newest: 8, 7, 6, 5, 4)
    assert_approx_eq!(f32, buf.buffer[0], 4.0); // Oldest preserved
    assert_approx_eq!(f32, buf.buffer[1], 5.0);
    assert_approx_eq!(f32, buf.buffer[2], 6.0);
    assert_approx_eq!(f32, buf.buffer[3], 7.0);
    assert_approx_eq!(f32, buf.buffer[4], 8.0); // Newest preserved
                                                // Check get()
    assert_approx_eq!(f32, buf.get(0.0), 8.0); // Newest
    assert_approx_eq!(f32, buf.get(4.0), 4.0); // Oldest preserved
}

#[test]
fn test_set_size_same_size() {
    let mut buf = RingBuffer::<10>::with_mode(5, BufferMode::Exact); // N=10, cap=5
    for i in 1..=5 {
        buf.push(i as f32);
    } // [1..=5], write_pos=0. Newest: 5,4,3,2,1
    let original_write_pos = buf.write_pos;

    buf.set_size(5); // Resize to same size
    assert_eq!(buf.capacity(), 5);
    // Should be a no-op according to the check at the start of set_size
    // If that check were removed, write_pos would reset to 0.
    // Let's keep the check that write_pos hasn't changed for same-size calls.
    assert_eq!(buf.write_pos, original_write_pos);
    // Check data is unchanged
    assert_approx_eq!(f32, buf.get(0.0), 5.0);
    assert_approx_eq!(f32, buf.get(4.0), 1.0);
}

#[test]
fn test_set_size_minimum() {
    let mut buf = RingBuffer::<10>::with_mode(3, BufferMode::Exact); // N=10, cap=3
    buf.push(10.0);
    buf.push(20.0);
    buf.push(30.0); // [10, 20, 30], write_pos=0. Newest: 30, 20, 10

    buf.set_size(0); // Resize to 0 -> becomes 1
    assert_eq!(buf.capacity(), 1);
    assert_eq!(buf.write_pos, 0);
    assert_eq!(buf.buffer.len(), 1);
    // Preserve min(3, 1) = 1 sample (the newest one)
    assert_approx_eq!(f32, buf.buffer[0], 30.0);
    assert_approx_eq!(f32, buf.get(0.0), 30.0);
    // Getting offset >= capacity should wrap
    assert_approx_eq!(f32, buf.get(1.0), 30.0);

    // --- Test Resizing from 1 ---
    let mut buf = RingBuffer::<10>::with_mode(1, BufferMode::Exact); // N=10, cap=1
    buf.push(99.0);

    buf.set_size(4); // Grow from 1 to 4
    assert_eq!(buf.capacity(), 4);
    assert_eq!(buf.write_pos, 0);
    assert_eq!(buf.buffer.len(), 4);
    // Preserve min(1, 4) = 1 sample
    // Expected: [0.0, 0.0, 0.0, 99.0]
    assert_approx_eq!(f32, buf.buffer[0], 0.0);
    assert_approx_eq!(f32, buf.buffer[1], 0.0);
    assert_approx_eq!(f32, buf.buffer[2], 0.0);
    assert_approx_eq!(f32, buf.buffer[3], 99.0);
    assert_approx_eq!(f32, buf.get(0.0), 99.0);
    assert_approx_eq!(f32, buf.get(1.0), 0.0);
}

#[test]
fn test_set_size_clamping() {
    let mut buf = RingBuffer::<10>::with_mode(8, BufferMode::Exact); // N=10, cap=8
    for i in 1..=8 {
        buf.push(i as f32);
    } // [1..=8], write_pos=0

    buf.set_size(15); // Request 15, N=10 -> new_cap=10
    assert_eq!(buf.capacity(), 10);
    assert_eq!(buf.write_pos, 0);
    // Preserve min(8, 10) = 8 samples
    // Expected: [0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
    assert_approx_eq!(f32, buf.buffer[0], 0.0);
    assert_approx_eq!(f32, buf.buffer[1], 0.0);
    assert_approx_eq!(f32, buf.buffer[2], 1.0);
    assert_approx_eq!(f32, buf.buffer[9], 8.0);

    // --- Test Clamping to N (Shrinking) ---
    let mut buf = RingBuffer::<10>::with_mode(12, BufferMode::Exact); // N=10, cap=10
    for i in 1..=10 {
        buf.push(i as f32);
    } // [1..=10], write_pos=0

    buf.set_size(5); // Shrink to 5
    assert_eq!(buf.capacity(), 5);
    assert_eq!(buf.write_pos, 0);
    // Preserve min(10, 5) = 5 samples (the newest: 10, 9, 8, 7, 6)
    // Expected: [6.0, 7.0, 8.0, 9.0, 10.0]
    assert_approx_eq!(f32, buf.buffer[0], 6.0);
    assert_approx_eq!(f32, buf.buffer[4], 10.0);
}

#[test]
fn test_set_size_power_of_two() {
    let mut buf = RingBuffer::<16>::with_mode(4, BufferMode::PowerOfTwo); // N=16, cap=4
    for i in 1..=5 {
        buf.push(i as f32);
    } // Pushed 5 values: newest are 5,4,3,2. write_pos=1.

    buf.set_size(6); // Request 6 -> pow2=8. Grow cap from 4 to 8.
    assert_eq!(buf.capacity(), 8);
    assert_eq!(buf.mask, 7);
    assert_eq!(buf.write_pos, 0);
    // Preserve min(4, 8) = 4 samples (newest: 5, 4, 3, 2)
    // Expected: [0.0, 0.0, 0.0, 0.0, 2.0, 3.0, 4.0, 5.0]
    assert_approx_eq!(f32, buf.buffer[0], 0.0);
    assert_approx_eq!(f32, buf.buffer[3], 0.0);
    assert_approx_eq!(f32, buf.buffer[4], 2.0);
    assert_approx_eq!(f32, buf.buffer[7], 5.0);
}

#[test]
fn test_minimum_capacity() {
    // Renamed from test_empty_buffer
    // Test with minimum N and size 0 -> capacity 1
    let mut buf = RingBuffer::<1>::with_mode(0, BufferMode::Exact);
    assert_eq!(buf.capacity(), 1);
    assert_eq!(buf.max_capacity(), 1);
    buf.push(5.0); // write_pos becomes 0
    assert_eq!(buf.buffer[0], 5.0);
    assert_approx_eq!(f32, buf.get(0.0), 5.0, epsilon = 1e-6); // Read the only sample
                                                               // Interpolation with capacity 1 falls back to linear, reads index 0 twice.
    assert_approx_eq!(f32, buf.get(0.5), 5.0, epsilon = 1e-6);
    assert_approx_eq!(f32, buf.get(10.0), 5.0, epsilon = 1e-6); // Offset wraps around

    // Test PowerOfTwo mode with size 0
    let buf_pow2 = RingBuffer::<8>::with_mode(0, BufferMode::PowerOfTwo); // N=8, size=0 -> capacity=1
    assert_eq!(buf_pow2.capacity(), 1);
    assert_eq!(buf_pow2.max_capacity(), 8);
}

#[test]
fn test_clamping_at_instantiation() {
    // Request size larger than N
    let buf = RingBuffer::<32>::with_mode(100, BufferMode::Exact);
    assert_eq!(buf.capacity(), 32); // Clamped to N=32
    assert_eq!(buf.max_capacity(), 32);

    // Request size larger than N, power of two mode
    // requested 40, N=32. Clamped size = 32. next_power_of_two(32)=32. min(32, 32)=32
    let buf_pow2 = RingBuffer::<32>::with_mode(40, BufferMode::PowerOfTwo);
    assert_eq!(buf_pow2.capacity(), 32);
    assert_eq!(buf_pow2.max_capacity(), 32);

    // Request size smaller than N, power of two rounds up but still below N
    // requested 10, N=32. Clamped size = 10. next_power_of_two(10)=16. min(16, 32)=16
    let buf_pow2_small = RingBuffer::<32>::with_mode(10, BufferMode::PowerOfTwo);
    assert_eq!(buf_pow2_small.capacity(), 16);
    assert_eq!(buf_pow2_small.max_capacity(), 32);
}
