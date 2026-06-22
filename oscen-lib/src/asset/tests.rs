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
fn asset_from_samples_rate_mismatch_is_error() {
    let result = AudioAsset::from_samples(vec![0.0, 1.0], 1, 48000, 44100);
    assert!(matches!(
        result,
        Err(AssetError::SampleRateMismatch {
            asset: 48000,
            graph: 44100
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
fn asset_from_wav_rate_mismatch_is_error() {
    let path = std::env::temp_dir().join("oscen_asset_rate_mismatch_test.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        writer.write_sample(0i16).unwrap();
        writer.write_sample(100i16).unwrap();
        writer.finalize().unwrap();
    }

    let result = AudioAsset::from_wav(&path, 44100);
    assert!(matches!(
        result,
        Err(AssetError::SampleRateMismatch {
            asset: 48000,
            graph: 44100
        })
    ));

    std::fs::remove_file(&path).unwrap();
}
