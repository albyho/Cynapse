//! Narration audio playback on a dedicated thread.
//!
//! The audio device and `Player` stay on this thread. The GPUI side sends
//! typed commands and receives start/end/failure events; it never touches an
//! audio API on the UI thread.

use cynapse_client_core::NarrationClipRef;
use cynapse_client_i18n::t_with;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone)]
pub enum NarrationEvent {
    Started {
        clip: NarrationClipRef,
    },
    Ended {
        clip: NarrationClipRef,
    },
    Failed {
        clip: NarrationClipRef,
        message: String,
    },
}

enum AudioCommand {
    Play {
        clip: NarrationClipRef,
        path: PathBuf,
        offset_ms: u64,
        duration_ms: u64,
    },
    Caption {
        clip: NarrationClipRef,
        duration_ms: u64,
    },
    Stop,
}

pub struct NarrationService {
    commands: Sender<AudioCommand>,
    volume: Arc<AtomicU8>,
}

impl NarrationService {
    pub fn start(events: UnboundedSender<NarrationEvent>, volume: u8) -> Option<Self> {
        let (commands, receiver) = std::sync::mpsc::channel();
        let volume = Arc::new(AtomicU8::new(volume.min(100)));
        let thread_volume = Arc::clone(&volume);
        std::thread::Builder::new()
            .name("CynapseNarration".to_string())
            .spawn(move || run(receiver, events, thread_volume))
            .ok()?;
        Some(Self { commands, volume })
    }

    /// Volume 0 keeps the clip playing silently; playback never pauses.
    pub fn set_volume(&self, volume: u8) {
        self.volume.store(volume.min(100), Ordering::Relaxed);
    }

    pub fn play(&self, clip: NarrationClipRef, path: PathBuf, offset_ms: u64, duration_ms: u64) {
        let _ = self.commands.send(AudioCommand::Play {
            clip,
            path,
            offset_ms,
            duration_ms,
        });
    }

    pub fn play_caption(&self, clip: NarrationClipRef, duration_ms: u64) {
        let _ = self
            .commands
            .send(AudioCommand::Caption { clip, duration_ms });
    }

    pub fn stop(&self) {
        let _ = self.commands.send(AudioCommand::Stop);
    }
}

struct Active {
    clip: NarrationClipRef,
    started: Instant,
    duration: Duration,
    audio: bool,
}

fn run(
    commands: Receiver<AudioCommand>,
    events: UnboundedSender<NarrationEvent>,
    volume: Arc<AtomicU8>,
) {
    let mut device: Option<MixerDeviceSink> = None;
    let mut player: Option<Player> = None;
    let mut active: Option<Active> = None;

    loop {
        if active.is_some() {
            loop {
                match commands.try_recv() {
                    Ok(command) => {
                        if let Some(next) =
                            apply(command, &mut device, &mut player, &volume, &events)
                        {
                            active = Some(next);
                        } else {
                            active = None;
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }

            if let Some(player) = player.as_ref() {
                player.set_volume(volume_factor(&volume));
            }
            if let Some(current) = active.as_ref() {
                let finished = if current.audio {
                    player.as_ref().is_some_and(|player| player.empty())
                } else {
                    current.started.elapsed() >= current.duration
                };
                if finished {
                    let _ = events.send(NarrationEvent::Ended {
                        clip: current.clip.clone(),
                    });
                    active = None;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        } else {
            match commands.recv() {
                Ok(command) => active = apply(command, &mut device, &mut player, &volume, &events),
                Err(_) => return,
            }
        }
    }
}

fn volume_factor(volume: &AtomicU8) -> f32 {
    f32::from(volume.load(Ordering::Relaxed)) / 100.0
}

fn apply(
    command: AudioCommand,
    device: &mut Option<MixerDeviceSink>,
    player: &mut Option<Player>,
    volume: &AtomicU8,
    events: &UnboundedSender<NarrationEvent>,
) -> Option<Active> {
    match command {
        AudioCommand::Stop => {
            stop_player(player.as_ref());
            None
        }
        AudioCommand::Caption { clip, duration_ms } => {
            let _ = events.send(NarrationEvent::Started { clip: clip.clone() });
            Some(Active {
                clip,
                started: Instant::now(),
                duration: Duration::from_millis(duration_ms.max(1)),
                audio: false,
            })
        }
        AudioCommand::Play {
            clip,
            path,
            offset_ms,
            duration_ms,
        } => {
            if let Err(error) = ensure_player(device, player) {
                let _ = events.send(NarrationEvent::Failed {
                    clip,
                    message: error,
                });
                return None;
            }
            let player = player.as_ref()?;
            player.set_volume(volume_factor(volume));
            stop_player(Some(player));
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) => {
                    let _ = events.send(NarrationEvent::Failed {
                        clip,
                        message: t_with(
                            "error.narration_audio_open_failed",
                            &[("error", &error.to_string())],
                        ),
                    });
                    return None;
                }
            };
            let mut decoder = match Decoder::try_from(BufReader::new(file)) {
                Ok(decoder) => decoder,
                Err(error) => {
                    let _ = events.send(NarrationEvent::Failed {
                        clip,
                        message: t_with(
                            "error.narration_audio_decode_failed",
                            &[("error", &error.to_string())],
                        ),
                    });
                    return None;
                }
            };
            if let Err(error) = seek_to_offset(&mut decoder, offset_ms) {
                let _ = events.send(NarrationEvent::Failed {
                    clip,
                    message: error,
                });
                return None;
            }
            let duration = Duration::from_millis(duration_ms.max(1));
            player.append(decoder.take_duration(duration));
            player.play();
            let _ = events.send(NarrationEvent::Started { clip: clip.clone() });
            Some(Active {
                clip,
                started: Instant::now(),
                duration,
                audio: true,
            })
        }
    }
}

/// `skip_duration` is a no-op on a fresh rodio/Symphonia decoder because its
/// first span is empty; seek instead and fail rather than play the wrong offset.
fn seek_to_offset<R>(decoder: &mut Decoder<R>, offset_ms: u64) -> Result<(), String>
where
    R: Read + Seek,
{
    if offset_ms == 0 {
        return Ok(());
    }
    decoder
        .try_seek(Duration::from_millis(offset_ms))
        .map_err(|error| {
            t_with(
                "error.narration_audio_seek_failed",
                &[("error", &error.to_string())],
            )
        })
}

fn ensure_player(
    device: &mut Option<MixerDeviceSink>,
    player: &mut Option<Player>,
) -> Result<(), String> {
    if player.is_some() {
        return Ok(());
    }
    let sink = DeviceSinkBuilder::open_default_sink().map_err(|error| {
        t_with(
            "error.audio_device_open_failed",
            &[("error", &error.to_string())],
        )
    })?;
    *player = Some(Player::connect_new(sink.mixer()));
    *device = Some(sink);
    Ok(())
}

fn stop_player(player: Option<&Player>) {
    if let Some(player) = player {
        player.stop();
        player.clear();
    }
}

#[cfg(test)]
mod tests;
