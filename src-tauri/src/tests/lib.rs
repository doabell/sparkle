use super::*;
use std::cell::RefCell;

#[test]
fn logging_filter_keeps_lifecycle_and_scopes_verbose_output() {
    assert!(should_emit_log(log::Level::Info, "dependency", false));
    assert!(should_emit_log(log::Level::Error, "sparkle::audio", false));
    assert!(!should_emit_log(
        log::Level::Debug,
        "sparkle::playback",
        false
    ));
    assert!(should_emit_log(
        log::Level::Trace,
        "sparkle::analytics::writer",
        true
    ));
    assert!(!should_emit_log(log::Level::Debug, "dependency", true));
}

struct FakePlayback {
    is_playing: bool,
    position_ms: i64,
    duration_ms: i64,
    actions: RefCell<Vec<String>>,
}

impl FakePlayback {
    fn new(is_playing: bool) -> Self {
        Self {
            is_playing,
            position_ms: 30_000,
            duration_ms: 60_000,
            actions: RefCell::new(Vec::new()),
        }
    }
}

impl NativeMediaPlayback for FakePlayback {
    fn is_playing(&self) -> Result<bool, String> {
        Ok(self.is_playing)
    }

    fn playback_position(&self) -> Result<(i64, i64), String> {
        Ok((self.position_ms, self.duration_ms))
    }

    fn play(&self) -> Result<(), String> {
        self.actions.borrow_mut().push("play".to_string());
        Ok(())
    }

    fn pause(&self) -> Result<(), String> {
        self.actions.borrow_mut().push("pause".to_string());
        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        self.actions.borrow_mut().push("stop".to_string());
        Ok(())
    }

    fn next(&self) -> Result<(), String> {
        self.actions.borrow_mut().push("next".to_string());
        Ok(())
    }

    fn previous(&self) -> Result<(), String> {
        self.actions.borrow_mut().push("previous".to_string());
        Ok(())
    }

    fn seek(&self, position_ms: i64) -> Result<(), String> {
        self.actions
            .borrow_mut()
            .push(format!("seek:{position_ms}"));
        Ok(())
    }
}

#[test]
fn native_play_dispatches_directly_to_the_audio_backend() {
    let playback = FakePlayback::new(false);

    dispatch_native_media_action(&playback, NativeMediaAction::Play).unwrap();

    assert_eq!(playback.actions.into_inner(), vec!["play"]);
}

#[test]
fn native_pause_dispatches_directly_to_the_audio_backend() {
    let playback = FakePlayback::new(true);

    dispatch_native_media_action(&playback, NativeMediaAction::Pause).unwrap();

    assert_eq!(playback.actions.into_inner(), vec!["pause"]);
}

#[test]
fn native_toggle_uses_the_audio_backends_current_state() {
    let paused = FakePlayback::new(false);
    let playing = FakePlayback::new(true);

    dispatch_native_media_action(&paused, NativeMediaAction::Toggle).unwrap();
    dispatch_native_media_action(&playing, NativeMediaAction::Toggle).unwrap();

    assert_eq!(paused.actions.into_inner(), vec!["play"]);
    assert_eq!(playing.actions.into_inner(), vec!["pause"]);
}

#[test]
fn native_seek_is_bounded_to_the_track_duration() {
    let playback = FakePlayback::new(false);

    dispatch_native_media_action(&playback, NativeMediaAction::SeekTo(120.0)).unwrap();

    assert_eq!(playback.actions.into_inner(), vec!["seek:60000"]);
}
