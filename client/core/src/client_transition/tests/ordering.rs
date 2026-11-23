use super::*;

#[test]
fn the_latest_pending_transition_runs_after_the_active_transition_finishes() {
    let runtime = ClientTransitionRuntime::default();
    let TransitionRequest::Started(active) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterDependent)
        .expect("active transition")
    else {
        panic!("first transition must start");
    };
    let TransitionRequest::Queued(pending) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("pending transition")
    else {
        panic!("second transition must queue");
    };

    assert!(!runtime.transition_is_current(&active));
    runtime.finish_transition(&active);

    assert!(!runtime.transition_is_pending(&pending));
    assert!(runtime.transition_is_current(&pending));
}

#[test]
fn stop_discards_all_pending_transitions() {
    let runtime = ClientTransitionRuntime::default();
    let _ = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("active transition");
    let TransitionRequest::Queued(pending) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("pending transition")
    else {
        panic!("second transition must queue");
    };

    let StopTransitionRequest::Started(stop) = runtime.request_stop() else {
        panic!("Stop must start a barrier");
    };
    assert!(!runtime.transition_is_pending(&pending));
    runtime.finish_stop(&stop);
}

#[test]
fn a_strict_transition_is_rejected_while_a_latest_intent_is_pending() {
    let runtime = ClientTransitionRuntime::default();
    let _ = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("active transition");
    let _ = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("pending transition");

    assert_eq!(
        runtime.try_transition(ClientTransitionRequirement::AdapterIndependent),
        Err(TransitionStartError::InProgress)
    );
}

#[test]
fn a_destructive_transition_cannot_commit_after_a_newer_intent_arrives() {
    let runtime = ClientTransitionRuntime::default();
    let TransitionRequest::Started(reset) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("reset starts")
    else {
        panic!("reset must start");
    };
    let _ = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("open queues");
    let mut committed = false;

    let outcome = runtime.commit_transition(&reset, || {
        committed = true;
        Ok::<_, ()>(())
    });

    assert_eq!(outcome, Err(TransitionCommitError::Superseded));
    assert!(!committed);
}

#[tokio::test]
async fn waiting_for_a_superseded_pending_transition_returns_superseded() {
    let runtime = ClientTransitionRuntime::default();
    let _ = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("active transition");
    let TransitionRequest::Queued(pending) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("pending transition")
    else {
        panic!("second transition must queue");
    };

    let (_newer, outcome) = tokio::join!(
        async {
            runtime
                .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
                .expect("newer transition")
        },
        runtime.wait_for_transition(&pending)
    );

    assert_eq!(outcome, TransitionWaitOutcome::Superseded);
}

#[tokio::test]
async fn waiting_for_a_promoted_transition_returns_started() {
    let runtime = ClientTransitionRuntime::default();
    let TransitionRequest::Started(active) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("active transition")
    else {
        panic!("first transition must start");
    };
    let TransitionRequest::Queued(pending) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("pending transition")
    else {
        panic!("second transition must queue");
    };

    let ((), outcome) = tokio::join!(
        async {
            runtime.finish_transition(&active);
        },
        runtime.wait_for_transition(&pending)
    );

    assert_eq!(outcome, TransitionWaitOutcome::Started);
}

#[test]
fn open_intent_supersedes_an_active_reset_at_a_safe_point() {
    let runtime = ClientTransitionRuntime::default();
    let TransitionRequest::Started(reset) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("reset starts")
    else {
        panic!("reset must start");
    };
    let TransitionRequest::Queued(open) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("open queues")
    else {
        panic!("open must queue");
    };

    assert!(!runtime.transition_is_current(&reset));
    assert!(runtime.transition_is_pending(&open));
}

#[test]
fn reset_intent_supersedes_an_active_start_at_a_safe_point() {
    let runtime = ClientTransitionRuntime::default();
    let start = runtime
        .try_transition(ClientTransitionRequirement::AdapterDependent)
        .expect("start begins");
    let TransitionRequest::Queued(reset) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("reset queues")
    else {
        panic!("reset must queue");
    };

    assert!(!runtime.transition_is_current(&start));
    runtime.finish_transition(&start);
    assert!(runtime.transition_is_current(&reset));
}

#[test]
fn a_finalizing_destructive_transition_finishes_before_the_pending_intent() {
    let runtime = ClientTransitionRuntime::default();
    let TransitionRequest::Started(reset) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("reset starts")
    else {
        panic!("reset must start");
    };
    assert!(runtime.begin_finalizing(&reset));
    let TransitionRequest::Queued(open) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterIndependent)
        .expect("open queues")
    else {
        panic!("open must queue");
    };

    assert!(runtime.transition_is_current(&reset));
    let mut committed = false;
    let outcome = runtime.commit_transition(&reset, || {
        committed = true;
        Ok::<_, ()>(())
    });
    assert_eq!(outcome, Ok(()));
    assert!(committed);

    runtime.finish_transition(&reset);
    assert!(runtime.transition_is_current(&open));
}

#[test]
fn stop_waits_for_a_finalizing_destructive_transition() {
    let runtime = ClientTransitionRuntime::default();
    let TransitionRequest::Started(reset) = runtime
        .request_latest_transition(ClientTransitionRequirement::AdapterDependent)
        .expect("reset starts")
    else {
        panic!("reset must start");
    };
    assert!(runtime.begin_finalizing(&reset));

    let _ = runtime.request_stop();

    assert!(runtime.transition_is_current(&reset));
}
