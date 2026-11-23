use super::*;
use cynapse_client_core::ActionNarrationPhase;
use std::io::Cursor;

#[test]
fn seeking_uses_the_clip_offset_instead_of_playing_from_start() {
    let bytes = include_bytes!("../../../test-data/narration-seek.ogg");
    let mut decoder = Decoder::try_from(BufReader::new(Cursor::new(bytes.as_slice())))
        .expect("seek fixture decodes");
    seek_to_offset(&mut decoder, 3200).expect("fixture is seekable");

    let samples: Vec<f32> = decoder.take_duration(Duration::from_millis(500)).collect();
    let rms = (samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt();
    assert!(
        rms > 0.2,
        "seeking to 3200ms must reach the high-amplitude segment, got RMS {rms}"
    );
}

#[test]
fn caption_only_clip_reports_started_and_ended() {
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let service = NarrationService::start(events, 100).expect("narration thread");
    let clip = NarrationClipRef {
        step_run_id: 1,
        action_index: 0,
        phase: ActionNarrationPhase::During,
        clip_index: 0,
    };

    service.play_caption(clip.clone(), 30);
    assert!(matches!(
        futures::executor::block_on(receiver.recv()),
        Some(NarrationEvent::Started { clip: started }) if started == clip
    ));
    assert!(matches!(
        futures::executor::block_on(receiver.recv()),
        Some(NarrationEvent::Ended { clip: ended }) if ended == clip
    ));
}
