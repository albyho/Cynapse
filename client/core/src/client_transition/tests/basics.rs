use super::*;
use std::sync::Arc;

#[test]
fn duplicate_learning_transition_is_rejected() {
    let coordinator = ClientTransitionRuntime::default();
    let first = coordinator
        .try_transition(ClientTransitionRequirement::AdapterIndependent)
        .unwrap();

    assert_eq!(
        coordinator.try_transition(ClientTransitionRequirement::AdapterIndependent),
        Err(TransitionStartError::InProgress)
    );
    assert!(coordinator.transition_is_current(&first));
}

#[test]
fn stop_invalidates_the_active_transition() {
    let coordinator = ClientTransitionRuntime::default();
    let transition = coordinator
        .try_transition(ClientTransitionRequirement::AdapterIndependent)
        .unwrap();
    let stop = coordinator.request_stop();

    let StopTransitionRequest::Started(stop) = stop else {
        panic!("first Stop must start a barrier");
    };
    assert!(!coordinator.transition_is_current(&transition));
    coordinator.finish_transition(&transition);
    coordinator.finish_stop(&stop);
    assert!(
        coordinator
            .try_transition(ClientTransitionRequirement::AdapterIndependent)
            .is_ok()
    );
}

#[test]
fn a_learning_transition_waits_while_stop_is_a_barrier() {
    let coordinator = ClientTransitionRuntime::default();
    let StopTransitionRequest::Started(stop) = coordinator.request_stop() else {
        panic!("Stop must start a barrier");
    };

    assert_eq!(
        coordinator.try_transition(ClientTransitionRequirement::AdapterIndependent),
        Err(TransitionStartError::StopInProgress)
    );
    assert!(coordinator.stop_in_progress());
    coordinator.finish_stop(&stop);
    assert!(
        coordinator
            .try_transition(ClientTransitionRequirement::AdapterIndependent)
            .is_ok()
    );
}

#[test]
fn duplicate_stop_joins_the_existing_barrier() {
    let coordinator = ClientTransitionRuntime::default();
    let StopTransitionRequest::Started(first) = coordinator.request_stop() else {
        panic!("first Stop must start a barrier");
    };
    let joined = coordinator.request_stop();

    assert_eq!(joined, StopTransitionRequest::Joined(first));
}

#[test]
fn a_superseded_transition_cannot_commit_after_the_final_checks() {
    let coordinator = ClientTransitionRuntime::default();
    let transition = coordinator
        .try_transition(ClientTransitionRequirement::AdapterIndependent)
        .unwrap();
    let _ = coordinator.request_stop();
    let mut committed = false;

    let outcome = coordinator.commit_transition(&transition, || {
        committed = true;
        Ok::<_, ()>("started")
    });

    assert_eq!(outcome, Err(TransitionCommitError::Superseded));
    assert!(!committed);
}

#[test]
fn adapter_disconnect_invalidates_an_adapter_dependent_transition() {
    let coordinator = ClientTransitionRuntime::default();
    let transition = coordinator
        .try_transition(ClientTransitionRequirement::AdapterDependent)
        .unwrap();

    assert!(coordinator.invalidate_adapter_transitions());
    assert!(!coordinator.transition_is_current(&transition));
}

#[test]
fn adapter_disconnect_preserves_an_adapter_starting_transition() {
    let coordinator = ClientTransitionRuntime::default();
    let transition = coordinator
        .try_transition(ClientTransitionRequirement::AdapterStarting)
        .unwrap();

    assert!(!coordinator.invalidate_adapter_transitions());
    assert!(coordinator.transition_is_current(&transition));
}

#[tokio::test]
async fn waiting_for_transition_end_wakes_on_finish() {
    let runtime = Arc::new(ClientTransitionRuntime::default());
    let ticket = runtime
        .try_transition(ClientTransitionRequirement::AdapterStarting)
        .unwrap();
    let waiter = {
        let runtime = Arc::clone(&runtime);
        let ticket = ticket.clone();
        tokio::spawn(async move {
            runtime.wait_for_transition_end(&ticket).await;
        })
    };
    tokio::task::yield_now().await;

    runtime.finish_transition(&ticket);
    waiter.await.unwrap();
}

#[test]
fn adapter_disconnect_preserves_an_adapter_independent_transition() {
    let coordinator = ClientTransitionRuntime::default();
    let transition = coordinator
        .try_transition(ClientTransitionRequirement::AdapterIndependent)
        .unwrap();

    assert!(!coordinator.invalidate_adapter_transitions());
    assert!(coordinator.transition_is_current(&transition));
}

#[test]
fn adapter_disconnect_does_not_abandon_a_finalizing_filesystem_transition() {
    let runtime = ClientTransitionRuntime::default();
    let transition = runtime
        .try_transition(ClientTransitionRequirement::AdapterDependent)
        .unwrap();
    assert!(runtime.begin_finalizing(&transition));

    assert!(!runtime.invalidate_adapter_transitions());
    assert!(runtime.transition_is_current(&transition));
}

#[test]
fn a_newer_reset_replaces_an_older_pending_open() {
    let runtime = ClientTransitionRuntime::default();
    let reset = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("reset starts");
    assert!(matches!(reset, TransitionRequest::Started(_)));

    let first_open = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("open queues");
    let TransitionRequest::Queued(first_open) = first_open else {
        panic!("open must queue behind reset");
    };
    let latest_reset = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("latest reset queues");
    let TransitionRequest::Queued(latest_reset) = latest_reset else {
        panic!("latest reset must queue");
    };

    assert!(!runtime.transition_is_pending(&first_open));
    assert!(runtime.transition_is_pending(&latest_reset));
    assert!(!runtime.transition_is_current(&first_open));
    assert!(!runtime.transition_is_current(&latest_reset));
}

/// A view whose `id` deliberately disagrees with the ticket: `attach_view` is
/// what makes them the same, so the tests below cannot pass by accident.
fn view(kind: ClientTransitionKind) -> ClientTransitionView {
    ClientTransitionView {
        id: "not-a-ticket".to_string(),
        kind,
        phase: ClientTransitionPhase::Running,
        cancellable: true,
        stage: None,
        message: None,
    }
}

#[test]
fn the_view_follows_its_ticket_through_promotion() {
    let runtime = ClientTransitionRuntime::default();
    let TransitionRequest::Started(first) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("first starts")
    else {
        panic!("first must start");
    };
    runtime.attach_view(&first, view(ClientTransitionKind::Preparing));

    let TransitionRequest::Queued(second) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("second queues")
    else {
        panic!("second must queue");
    };
    runtime.attach_view(&second, view(ClientTransitionKind::Opening));

    let views = runtime.views();
    assert_eq!(
        views.current.as_ref().map(|view| view.id.as_str()),
        Some(first.id.as_str())
    );
    assert_eq!(
        views.current.as_ref().map(|view| view.kind),
        Some(ClientTransitionKind::Preparing)
    );
    assert_eq!(
        views.pending.as_ref().map(|view| view.id.as_str()),
        Some(second.id.as_str())
    );

    runtime.finish_transition(&first);
    let views = runtime.views();
    assert_eq!(
        views.current.as_ref().map(|view| view.id.as_str()),
        Some(second.id.as_str()),
        "the queued view is promoted with its ticket"
    );
    assert!(views.pending.is_none());
}

#[test]
fn a_stop_takes_over_the_current_view() {
    let runtime = ClientTransitionRuntime::default();
    let ticket = runtime
        .try_transition(ClientTransitionRequirement::AdapterDependent)
        .unwrap();
    runtime.attach_view(&ticket, view(ClientTransitionKind::Starting));

    let StopTransitionRequest::Started(stop) = runtime.request_stop() else {
        panic!("first Stop must start a barrier");
    };
    runtime.attach_view(&stop, view(ClientTransitionKind::Stopping));

    let views = runtime.views();
    assert_eq!(
        views.current.map(|view| view.kind),
        Some(ClientTransitionKind::Stopping)
    );
    assert!(views.pending.is_none());

    runtime.finish_stop(&stop);
    assert!(
        runtime.views().current.is_none(),
        "a finished Stop must not fall back to the transition it replaced"
    );
}

#[test]
fn finishing_a_queued_transition_removes_it() {
    let runtime = ClientTransitionRuntime::default();
    let TransitionRequest::Started(active) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("first starts")
    else {
        panic!("first must start");
    };
    let TransitionRequest::Queued(queued) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("second queues")
    else {
        panic!("second must queue");
    };
    assert!(runtime.transition_is_pending(&queued));

    assert!(runtime.finish_transition(&queued).is_none());
    assert!(
        !runtime.transition_is_pending(&queued),
        "a finished queued ticket must not linger"
    );

    runtime.finish_transition(&active);
    assert!(
        runtime
            .try_transition(ClientTransitionRequirement::AdapterIndependent)
            .is_ok(),
        "the removed ticket must not keep the machine busy"
    );
}
