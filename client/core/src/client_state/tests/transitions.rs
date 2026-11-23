use super::*;
use std::sync::Arc;

#[test]
fn only_errors_with_a_resumable_position_are_retryable() {
    let resumable_error = PlaybackState::Error {
        code: "command_failed".to_string(),
        message: "failed".to_string(),
        position: Some(playback_position()),
        interruption: None,
    };
    let terminal_error = PlaybackState::Error {
        code: "persistence_failed".to_string(),
        message: "invalid".to_string(),
        position: None,
        interruption: None,
    };
    let adapter_pause = PlaybackState::Paused {
        position: playback_position(),
        interruption: crate::PlaybackInterruption {
            reason: crate::PlaybackInterruptionReason::AdapterDisconnected,
            exit_code: None,
            output: None,
            output_truncated: None,
        },
    };

    assert!(playback_is_retryable(&resumable_error));
    assert!(!playback_is_retryable(&terminal_error));
    assert!(!playback_is_retryable(&adapter_pause));
}

#[test]
fn adapter_readiness_requires_a_connection_only_when_adapters_are_declared() {
    let connected = ConnectionState::Connected {
        adapter: "vscode".to_string(),
    };
    let disconnected = ConnectionState::WaitingForAdapter;

    assert!(adapter_readiness(&[], &disconnected).is_ok());
    assert!(adapter_readiness(&[crate::AdapterId::Vscode], &connected).is_ok());
    assert_eq!(
        adapter_readiness(&[crate::AdapterId::Vscode], &disconnected).unwrap_err(),
        crate::Warning::AdapterDisconnected
    );
    assert_eq!(
        adapter_readiness(
            &[crate::AdapterId::Vscode],
            &ConnectionState::Connected {
                adapter: "other".to_string(),
            },
        )
        .unwrap_err(),
        crate::Warning::AdapterDisconnected
    );
}

#[test]
fn adapter_readiness_requires_the_units_own_connection_name() {
    let jupyterlab = ConnectionState::Connected {
        adapter: "jupyterlab".to_string(),
    };
    let vscode = ConnectionState::Connected {
        adapter: "vscode".to_string(),
    };

    assert!(adapter_readiness(&[crate::AdapterId::Jupyterlab], &jupyterlab).is_ok());
    assert_eq!(
        adapter_readiness(&[crate::AdapterId::Jupyterlab], &vscode).unwrap_err(),
        crate::Warning::AdapterDisconnected
    );
    assert_eq!(
        adapter_readiness(&[crate::AdapterId::Vscode], &jupyterlab).unwrap_err(),
        crate::Warning::AdapterDisconnected
    );
}

#[test]
fn deterministic_transition_stress_never_revives_a_cleared_unit() {
    struct TrackedTicket {
        ticket: ClientTransitionTicket,
        created_epoch: u64,
    }

    for seed in 1..=64_u64 {
        let runtime = ClientTransitionRuntime::default();
        let mut reducer = ClientStateReducer::default();
        let mut course = course_state();
        course.selected_unit = course
            .active_course
            .as_ref()
            .and_then(|value| value.tree.first())
            .and_then(|node| node.unit.clone());
        reducer.apply(&[
            ClientEvent::Course(Arc::new(course.clone())),
            ClientEvent::Playback(Box::new(PlaybackState::Ready {
                position: Some(playback_position()),
            })),
        ]);
        let mut tickets: Vec<TrackedTicket> = Vec::new();
        let mut recovery_point = Some("chapter/unit".to_string());
        let mut clear_epoch = 0_u64;
        let mut random = seed;

        for step in 0..96_u64 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            match (random.wrapping_add(step)) % 6 {
                0 | 1 => {
                    let requirement = if random & 1 == 0 {
                        ClientTransitionRequirement::AdapterDependent
                    } else {
                        ClientTransitionRequirement::AdapterIndependent
                    };
                    if let Ok(request) = runtime.request_latest_transition(requirement) {
                        let ticket = match request {
                            TransitionRequest::Started(ticket)
                            | TransitionRequest::Queued(ticket) => ticket,
                        };
                        tickets.push(TrackedTicket {
                            ticket,
                            created_epoch: clear_epoch,
                        });
                    }
                }
                2 => {
                    if let Some(tracked) = tickets.pop() {
                        let finalizing = runtime.begin_finalizing(&tracked.ticket);
                        let mut committed = false;
                        let outcome = runtime.commit_transition(&tracked.ticket, || {
                            committed = true;
                            Ok::<_, ()>(())
                        });
                        if finalizing && outcome.is_ok() {
                            assert_eq!(
                                tracked.created_epoch, clear_epoch,
                                "only a ticket created after the latest clear may select the Unit again"
                            );
                            assert!(committed);
                            course.selected_unit = course
                                .active_course
                                .as_ref()
                                .and_then(|value| value.tree.first())
                                .and_then(|node| node.unit.clone());
                            reducer.apply(&[ClientEvent::Course(Arc::new(course.clone()))]);
                            recovery_point = Some("chapter/unit".to_string());
                        } else {
                            assert!(!committed);
                        }
                    }
                }
                3 => {
                    runtime.invalidate_adapter_transitions();
                }
                4 => {
                    reducer.apply(&[ClientEvent::Playback(Box::new(
                        PlaybackState::StepComplete {
                            position: playback_position(),
                            continuation: None,
                            requires_human: false,
                        },
                    ))]);
                }
                _ => {
                    let stop_request = runtime.request_stop();
                    if let StopTransitionRequest::Started(stop) = stop_request {
                        for tracked in &tickets {
                            runtime.finish_transition(&tracked.ticket);
                        }
                        runtime.finish_stop(&stop);
                    }
                    for tracked in &tickets {
                        let mut committed = false;
                        let outcome = runtime.commit_transition(&tracked.ticket, || {
                            committed = true;
                            Ok::<_, ()>(())
                        });
                        assert!(outcome.is_err());
                        assert!(!committed);
                    }
                    course.selected_unit = None;
                    reducer.apply(&[
                        ClientEvent::Course(Arc::new(course.clone())),
                        ClientEvent::Playback(Box::new(PlaybackState::Ready { position: None })),
                        ClientEvent::Transition(None),
                        ClientEvent::PendingTransition(None),
                    ]);
                    recovery_point = None;
                    clear_epoch = clear_epoch.saturating_add(1);
                }
            }
            let current = tickets
                .iter()
                .find(|tracked| runtime.transition_is_current(&tracked.ticket));
            let pending = tickets
                .iter()
                .find(|tracked| runtime.transition_is_pending(&tracked.ticket));
            reducer.apply(&[
                ClientEvent::Transition(current.map(|tracked| ClientTransitionView {
                    id: tracked.ticket.id.clone(),
                    kind: ClientTransitionKind::Resetting,
                    phase: ClientTransitionPhase::Running,
                    cancellable: true,
                    stage: None,
                    message: Some("stress transition".to_string()),
                })),
                ClientEvent::PendingTransition(pending.map(|tracked| ClientTransitionView {
                    id: tracked.ticket.id.clone(),
                    kind: ClientTransitionKind::Opening,
                    phase: ClientTransitionPhase::Running,
                    cancellable: true,
                    stage: None,
                    message: Some("stress pending transition".to_string()),
                })),
            ]);
            assert_eq!(
                reducer.transition().map(|view| view.id.as_str()),
                current.map(|tracked| tracked.ticket.id.as_str())
            );
            assert_eq!(
                reducer.pending_transition().map(|view| view.id.as_str()),
                pending.map(|tracked| tracked.ticket.id.as_str())
            );
            assert_eq!(
                reducer.course().selected_unit.is_some(),
                recovery_point.is_some()
            );
            assert!(
                tickets
                    .iter()
                    .filter(|tracked| runtime.transition_is_current(&tracked.ticket))
                    .count()
                    <= 1
            );
            assert!(
                tickets
                    .iter()
                    .filter(|tracked| runtime.transition_is_pending(&tracked.ticket))
                    .count()
                    <= 1
            );
        }
    }
}
