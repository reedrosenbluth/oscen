use super::*;
use float_cmp::approx_eq;

fn assert_slice_approx(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len(), "slice lengths differ");
    for (i, (&a, &e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!(approx_eq!(f32, a, e, ulps = 2), "index {i}: {a} != {e}");
    }
}

#[test]
fn asset_from_samples_deinterleaves_to_channel_major() {
    let interleaved = vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0];
    let asset = AudioAsset::from_samples(interleaved, 2, 44100, 44100).unwrap();

    assert_eq!(asset.frames(), 3);
    assert_eq!(asset.channels(), 2);
    assert_eq!(asset.sample_rate(), 44100);
    assert_slice_approx(asset.channel(0), &[1.0, 2.0, 3.0]);
    assert_slice_approx(asset.channel(1), &[10.0, 20.0, 30.0]);
}

#[test]
fn asset_to_mono_averages_channels() {
    let interleaved = vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0];
    let asset = AudioAsset::from_samples(interleaved, 2, 44100, 44100).unwrap();

    assert_slice_approx(&asset.to_mono(), &[5.5, 11.0, 16.5]);
}

#[test]
fn asset_from_samples_empty_is_error() {
    let result = AudioAsset::from_samples(vec![], 1, 44100, 44100);
    assert!(matches!(result, Err(AssetError::Empty)));
}

#[test]
fn asset_from_samples_resamples_to_graph_rate() {
    // 1000 mono frames at 48 kHz, conformed to 24 kHz -> ~500 frames at 24 kHz.
    let src: Vec<f32> = (0..1000)
        .map(|i| (2.0 * std::f32::consts::PI * 200.0 * i as f32 / 48000.0).sin())
        .collect();
    let asset = AudioAsset::from_samples(src, 1, 48000, 24000).unwrap();

    assert_eq!(asset.channels(), 1);
    assert_eq!(asset.sample_rate(), 24000);
    assert_eq!(asset.frames(), 500);
    assert_eq!(asset.channel(0).len(), 500);
}

#[test]
fn asset_from_samples_resample_preserves_channels() {
    // Interleaved stereo: L constant 1.0, R constant -1.0. Resampling to a
    // different rate must keep the channels separate (unity DC gain each).
    let interleaved: Vec<f32> = (0..600).flat_map(|_| [1.0f32, -1.0f32]).collect();
    let asset = AudioAsset::from_samples(interleaved, 2, 48000, 44100).unwrap();

    assert_eq!(asset.channels(), 2);
    assert_eq!(asset.sample_rate(), 44100);
    // Sample away from the edges where the kernel is truncated.
    let mid = asset.frames() / 2;
    assert!(approx_eq!(f32, asset.channel(0)[mid], 1.0, epsilon = 1e-3));
    assert!(approx_eq!(f32, asset.channel(1)[mid], -1.0, epsilon = 1e-3));
}

#[test]
fn asset_from_samples_unconfigured_rate_is_error() {
    // graph_rate == 0 means the rate is not configured yet: cannot conform.
    let result = AudioAsset::from_samples(vec![0.0, 1.0], 1, 48000, 0);
    assert!(matches!(
        result,
        Err(AssetError::SampleRateMismatch {
            asset: 48000,
            graph: 0
        })
    ));
}

#[test]
fn asset_from_wav_normalizes_int_and_deinterleaves() {
    let path = std::env::temp_dir().join("oscen_asset_int_test.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        // frame 0: (i16::MAX, i16::MIN)
        writer.write_sample(i16::MAX).unwrap();
        writer.write_sample(i16::MIN).unwrap();
        // frame 1: (0, 0)
        writer.write_sample(0i16).unwrap();
        writer.write_sample(0i16).unwrap();
        writer.finalize().unwrap();
    }

    let asset = AudioAsset::from_wav(&path, 44100).unwrap();

    assert_eq!(asset.channels(), 2);
    assert_eq!(asset.frames(), 2);
    assert!(approx_eq!(
        f32,
        asset.channel(0)[0],
        1.0,
        epsilon = 1.0 / 32768.0
    ));
    assert!(approx_eq!(
        f32,
        asset.channel(1)[0],
        -1.0,
        epsilon = 1.0 / 32768.0
    ));
    assert!(approx_eq!(
        f32,
        asset.channel(0)[1],
        0.0,
        epsilon = 1.0 / 32768.0
    ));

    std::fs::remove_file(&path).unwrap();
}

#[test]
fn asset_from_wav_resamples_to_graph_rate() {
    let path = std::env::temp_dir().join("oscen_asset_rate_conform_test.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        // 480 frames of silence (0.01 s); enough to have a well-defined
        // resampled length without depending on content.
        for _ in 0..480 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    // 480 frames at 48 kHz -> ~441 frames at 44.1 kHz, stored at the graph rate.
    let asset = AudioAsset::from_wav(&path, 44100).unwrap();
    assert_eq!(asset.channels(), 1);
    assert_eq!(asset.sample_rate(), 44100);
    assert_eq!(asset.frames(), 441);

    std::fs::remove_file(&path).unwrap();
}
