use audio_chromaprint::*;

fn pcm() -> Vec<i16> {
    include_bytes!("fixtures/stereo_44100.raw")
        .chunks_exact(2)
        .map(|x| i16::from_le_bytes([x[0], x[1]]))
        .collect()
}

#[test]
fn source_golden_fingerprint_and_streaming_chunks() {
    let data = pcm();
    let mut context = Context::new(Algorithm::Test2);
    assert!(context.feed(&data).is_err());
    assert!(context.finish().is_err());
    assert!(context.start(1000, 1).is_err());
    assert!(context.start(11025, 0).is_err());
    context.start(44100, 1).unwrap();
    for samples in data.chunks(237) {
        context.feed(samples).unwrap();
    }
    context.finish().unwrap();
    assert_eq!(
        context.fingerprint().unwrap(),
        "AQAAC0kkZUqYREkUnFAXHk8uuMZl6EfO4zu-4ABKFGESWIIMEQE"
    );
    assert_eq!(context.fingerprint_hash(), 3732003127);
    assert_eq!(context.raw_fingerprint().len(), 11);
    assert_eq!(context.config().sample_rate(), 11025);
    assert_eq!(context.config().item_duration(), 1365);
    assert_eq!(context.config().delay(), 28666);
    assert!((context.config().item_duration_seconds() - 1365.0 / 11025.0).abs() < 1e-10);
    assert!((context.config().delay_seconds() - 28666.0 / 11025.0).abs() < 1e-10);
    let expected = context.raw_fingerprint().to_vec();
    context.start(44100, 1).unwrap();
    context.feed(&data).unwrap();
    context.finish().unwrap();
    assert_eq!(context.raw_fingerprint(), expected);
    context.clear();
    assert!(context.raw_fingerprint().is_empty());
    context.feed(&data).unwrap();
    context.finish().unwrap();
    assert!(!context.raw_fingerprint().is_empty());
}

#[test]
fn encoding_round_trip_and_corruption() {
    assert_eq!(encode(&[1, 0], 55).unwrap(), [55, 0, 0, 2, 65, 0]);
    assert_eq!(encode_base64(&[1, 0], 55).unwrap(), "NwAAAkEA");
    for values in [
        vec![],
        vec![0],
        vec![u32::MAX, 0, 1 << 31],
        (0..1024u32).map(|x| x.wrapping_mul(2654435761)).collect(),
    ] {
        let encoded = encode_base64(&values, 1).unwrap();
        assert_eq!(decode_base64(&encoded).unwrap(), (values.clone(), 1));
        assert_eq!(decode_header_base64(&encoded).unwrap(), (values.len(), 1));
        let binary = encode(&values, 1).unwrap();
        assert_eq!(decode(&binary).unwrap(), (values, 1));
    }
    let golden =
        "AQAAEwkjrUmSJQpUHflR9mjSJMdZpcO_Imdw9dCO9Clu4_wQPvhCB01w6xAtXNcAp5RASgDBhDSCGGIAcwA";
    let (values, algorithm) = decode_base64(golden).unwrap();
    assert_eq!(values.len(), 19);
    assert_eq!(algorithm, 1);
    assert_eq!(
        values[..2],
        [(-587455133i32) as u32, (-591649759i32) as u32]
    );
    assert_eq!(encode_base64(&values, algorithm).unwrap(), golden);
    for input in ["null", "coustid Fingerprint", "AQ", ""] {
        assert!(decode_base64(input).is_err());
    }
    assert!(decode(&[1, 0, 0, 2]).is_err());
    assert!(decode(&[1, 0, 0, 2, 255]).is_err());
    assert!(decode_header_base64("!").is_err());
    assert_eq!(simhash(&[]), 0);
    assert_eq!(simhash(&[1, 2]), 0);
    assert_eq!(simhash(&[1, 1, 2]), 1);
    assert_eq!(hamming_distance(0, u32::MAX), 32);
}

#[test]
fn all_algorithms_audio_shapes_and_silence() {
    let data = pcm();
    for algorithm in Algorithm::ALL {
        let mut context = Context::new(algorithm);
        assert_eq!(context.algorithm(), algorithm);
        context.set_silence_threshold(50);
        context.start(11025, 1).unwrap();
        context.feed(&vec![0; 4096 * 40]).unwrap();
        context.finish().unwrap();
        if algorithm == Algorithm::Test4 {
            assert!(context.raw_fingerprint().is_empty());
        } else {
            assert!(!context.raw_fingerprint().is_empty());
        }
        context.start(11025, 1).unwrap();
        context.feed(&data).unwrap();
        context.finish().unwrap();
        assert!(!context.raw_fingerprint().is_empty());
        assert_eq!(
            decode_base64(&context.fingerprint().unwrap()).unwrap().1,
            algorithm as u8
        );
    }
    let mut context = Context::new(Algorithm::Test2);
    context.start(44100, 1).unwrap();
    context.feed(&vec![0; 130 * 1024]).unwrap();
    context.finish().unwrap();
    assert_eq!(context.raw_fingerprint(), [627964279; 3]);
    assert_eq!(context.fingerprint().unwrap(), "AQAAA0mUaEkSRZEGAA");
    let mono: Vec<i16> = data.iter().map(|x| x.saturating_add(15000)).collect();
    let mut expected = Context::new(Algorithm::Test2);
    expected.start(11025, 1).unwrap();
    expected.feed(&mono).unwrap();
    expected.finish().unwrap();
    for channels in [2, 3] {
        let input: Vec<_> = mono
            .iter()
            .flat_map(|x| std::iter::repeat_n(*x, channels))
            .collect();
        context.start(11025, channels).unwrap();
        for chunk in input.chunks(13) {
            context.feed(chunk).unwrap();
        }
        context.finish().unwrap();
        assert_eq!(
            context.raw_fingerprint(),
            expected.raw_fingerprint(),
            "downmix must divide before narrowing"
        );
    }
}

#[test]
fn matching_offsets_thresholds_and_separated_regions() {
    let fp: Vec<_> = (0..400u32).map(|x| x.wrapping_mul(2654435761)).collect();
    let mut matcher = Matcher::new(Algorithm::Test2);
    assert_eq!(matcher.match_fingerprints(&[], &fp).unwrap(), []);
    let segments = matcher.match_fingerprints(&fp, &fp[23..]).unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(
        (segments[0].pos1, segments[0].pos2, segments[0].duration),
        (23, 0, 377)
    );
    assert_eq!(segments[0].public_score(), 0);
    assert_eq!(segments[0].similarity_percent(), 100.0);
    assert!(matcher.hash_duration(4) > matcher.hash_time(4));
    matcher.set_match_threshold(0.0).unwrap();
    assert!(matcher.match_fingerprints(&fp, &fp).unwrap().is_empty());
    assert!(matcher.set_match_threshold(f64::NAN).is_err());
    matcher.set_match_threshold(10.0).unwrap();
    let mut other = fp.clone();
    for v in &mut other[100..300] {
        *v ^= u32::MAX;
    }
    let segments = matcher.match_fingerprints(&fp, &other).unwrap();
    assert_eq!(
        segments.len(),
        2,
        "accepted segments must never bridge a rejected gap"
    );
    assert!(segments[0].pos1 + segments[0].duration < segments[1].pos1);
    assert!(matcher.match_fingerprints(&vec![0; 524286], &fp).is_err());
    assert!(matcher.match_fingerprints(&[1], &[2]).unwrap().is_empty());
}

#[test]
fn native_chromaprint_161_golden_algorithms() {
    let data = pcm();
    for (algorithm, fingerprint) in [
        (
            Algorithm::Test1,
            "AAAAC5GkJAnDRIpQ-YOOmyWqJ4fR69AlCbeg2Yed5wDBAGFAAIsIAQ",
        ),
        (
            Algorithm::Test2,
            "AQAAC0kkZUqYREkUnFAXHk8uuMZl6EfO4zu-4ABKFGESWIIMEQE",
        ),
        // Algorithm 3 oracle comes from gochromaprint; fpcalc 1.6.1 returned
        // an empty fingerprint for this preset on the same audio.
        (
            Algorithm::Test3,
            "AgAAC8mUpImUREkUXJGgflzwHC5OaH_R68i14_gOAZRhDEgiDFME",
        ),
        (
            Algorithm::Test4,
            "AwAACkkkZUqYREkUaA1FVM4D-zhF_NAfIv9x7XgOgxBTQhAnCFEG",
        ),
        (
            Algorithm::Test5,
            "BAAAF1qSRGGWJEmi4DzsDD8uIdI8tA_EUDTy7bhEHDe-Gz-uHjnEUciP7TRqG_0KlzvCa58AZBBgTiBiqQNWIAATFAQAAg",
        ),
    ] {
        let mut context = Context::new(algorithm);
        context.start(44100, 1).unwrap();
        context.feed(&data).unwrap();
        context.finish().unwrap();
        assert_eq!(context.fingerprint().unwrap(), fingerprint, "{algorithm:?}");
    }
}
