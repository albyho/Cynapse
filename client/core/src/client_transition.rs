use crate::client_state::{
    ClientTransitionKind, ClientTransitionPhase, ClientTransitionStage, ClientTransitionView,
};
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientTransitionTicket {
    pub id: String,
    pub invalidation_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientTransitionRequirement {
    /// Invalidated immediately when a required Adapter disconnects.
    AdapterDependent,
    /// The transition is bringing the Adapter up; a transient disconnect must
    /// not cancel it. Its own handshake timeout owns the failure path.
    AdapterStarting,
    /// Does not depend on an Adapter connection.
    AdapterIndependent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionStartError {
    InProgress,
    StopInProgress,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionRequest {
    Started(ClientTransitionTicket),
    Queued(ClientTransitionTicket),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionWaitOutcome {
    Started,
    Superseded,
    Cancelled,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TransitionCommitError<E> {
    Superseded,
    CommitFailed(E),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopTransitionRequest {
    Started(ClientTransitionTicket),
    Joined(ClientTransitionTicket),
}

#[derive(Debug, Default)]
struct ClientTransitionCoordinator {
    next_sequence: u64,
    generation: u64,
    active_transition: Option<ActiveTransition>,
    pending_transition: Option<ActiveTransition>,
    stop_barrier: Option<ClientTransitionTicket>,
    /// The Stop has no `ActiveTransition` of its own, so its view sits here.
    stop_view: Option<ClientTransitionView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveTransition {
    ticket: ClientTransitionTicket,
    requirement: ClientTransitionRequirement,
    finalizing: bool,
    /// What the Client shows for this transition. It lives on the ticket's own
    /// record so promotion and removal follow the state machine instead of a
    /// second copy of it.
    view: Option<ClientTransitionView>,
}

/// The transition views the Client should be showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionViews {
    pub current: Option<ClientTransitionView>,
    pub pending: Option<ClientTransitionView>,
}

impl ClientTransitionCoordinator {
    pub fn try_transition(
        &mut self,
        requirement: ClientTransitionRequirement,
    ) -> Result<ClientTransitionTicket, TransitionStartError> {
        if self.stop_barrier.is_some() {
            return Err(TransitionStartError::StopInProgress);
        }
        if self.active_transition.is_some() || self.pending_transition.is_some() {
            return Err(TransitionStartError::InProgress);
        }
        let ticket = self.next_ticket();
        self.active_transition = Some(ActiveTransition {
            ticket: ticket.clone(),
            requirement,
            finalizing: false,
            view: None,
        });
        Ok(ticket)
    }

    pub fn request_latest_transition(
        &mut self,
        requirement: ClientTransitionRequirement,
    ) -> Result<TransitionRequest, TransitionStartError> {
        if self.stop_barrier.is_some() {
            return Err(TransitionStartError::StopInProgress);
        }
        if self.active_transition.is_none() && self.pending_transition.is_none() {
            let ticket = self.next_ticket();
            self.active_transition = Some(ActiveTransition {
                ticket: ticket.clone(),
                requirement,
                finalizing: false,
                view: None,
            });
            return Ok(TransitionRequest::Started(ticket));
        }
        let active_is_finalizing = self
            .active_transition
            .as_ref()
            .is_some_and(|active| active.finalizing);
        if !active_is_finalizing {
            self.generation = self.generation.saturating_add(1);
        }
        let ticket = self.next_ticket();
        self.pending_transition = Some(ActiveTransition {
            ticket: ticket.clone(),
            requirement,
            finalizing: false,
            view: None,
        });
        Ok(TransitionRequest::Queued(ticket))
    }

    pub fn request_stop(&mut self) -> StopTransitionRequest {
        if let Some(ticket) = self.stop_barrier.as_ref() {
            return StopTransitionRequest::Joined(ticket.clone());
        }
        if !self
            .active_transition
            .as_ref()
            .is_some_and(|active| active.finalizing)
        {
            self.generation = self.generation.saturating_add(1);
        }
        self.pending_transition = None;
        // The Stop takes over what the Client shows. The active ticket stays so
        // its waiter can finish it, but its view would be stale from here on.
        if let Some(active) = self.active_transition.as_mut() {
            active.view = None;
        }
        self.stop_view = None;
        let ticket = self.next_ticket();
        self.stop_barrier = Some(ticket.clone());
        StopTransitionRequest::Started(ticket)
    }

    pub fn transition_is_current(&self, ticket: &ClientTransitionTicket) -> bool {
        self.active_transition.as_ref().is_some_and(|active| {
            active.ticket == *ticket
                && ticket.invalidation_generation == self.generation
                && (self.stop_barrier.is_none() || active.finalizing)
        })
    }

    pub fn transition_is_pending(&self, ticket: &ClientTransitionTicket) -> bool {
        self.pending_transition
            .as_ref()
            .is_some_and(|pending| pending.ticket == *ticket)
    }

    /// Hangs the Client-facing view on the ticket it belongs to. A ticket the
    /// machine no longer holds — finished or invalidated while the caller was
    /// building the view — keeps none.
    pub fn attach_view(&mut self, ticket: &ClientTransitionTicket, mut view: ClientTransitionView) {
        view.id = ticket.id.clone();
        if let Some(active) = self
            .active_transition
            .as_mut()
            .filter(|active| active.ticket == *ticket)
        {
            active.view = Some(view);
        } else if let Some(pending) = self
            .pending_transition
            .as_mut()
            .filter(|pending| pending.ticket == *ticket)
        {
            pending.view = Some(view);
        } else if self.stop_barrier.as_ref() == Some(ticket) {
            self.stop_view = Some(view);
        }
    }

    /// What the Client shows: the Stop takes the current slot while it runs,
    /// and nothing is queued behind it.
    pub fn views(&self) -> TransitionViews {
        TransitionViews {
            current: self.stop_view.clone().or_else(|| {
                self.active_transition
                    .as_ref()
                    .and_then(|active| active.view.clone())
            }),
            pending: self
                .pending_transition
                .as_ref()
                .and_then(|pending| pending.view.clone()),
        }
    }

    /// Advances the Stage of the running view and reports whether it moved, so
    /// the caller republishes only on a real change.
    pub fn set_stage(&mut self, stage: ClientTransitionStage, message: String) -> bool {
        let Some(view) = self
            .active_transition
            .as_mut()
            .and_then(|active| active.view.as_mut())
        else {
            return false;
        };
        if view.kind != ClientTransitionKind::Starting
            || (view.stage == Some(stage) && view.message.as_deref() == Some(message.as_str()))
        {
            return false;
        }
        view.stage = Some(stage);
        view.message = Some(message);
        true
    }

    pub fn begin_finalizing(&mut self, ticket: &ClientTransitionTicket) -> bool {
        if !self.transition_is_current(ticket) {
            return false;
        }
        let Some(active) = self.active_transition.as_mut() else {
            return false;
        };
        if active.ticket != *ticket {
            return false;
        }
        active.finalizing = true;
        if let Some(view) = active.view.as_mut() {
            view.phase = ClientTransitionPhase::Finalizing;
            view.cancellable = false;
        }
        true
    }

    pub fn commit_transition<T, E>(
        &mut self,
        ticket: &ClientTransitionTicket,
        commit: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, TransitionCommitError<E>> {
        if !self.transition_is_current(ticket) {
            return Err(TransitionCommitError::Superseded);
        }
        commit().map_err(TransitionCommitError::CommitFailed)
    }

    pub fn invalidate_adapter_transitions(&mut self) -> bool {
        let mut invalidated = false;
        if matches!(
            &self.active_transition,
            Some(ActiveTransition {
                requirement: ClientTransitionRequirement::AdapterDependent,
                finalizing: false,
                ..
            })
        ) {
            self.active_transition = None;
            invalidated = true;
        }
        if matches!(
            &self.pending_transition,
            Some(ActiveTransition {
                requirement: ClientTransitionRequirement::AdapterDependent,
                ..
            })
        ) {
            self.pending_transition = None;
            invalidated = true;
        }
        if self.stop_barrier.is_none() && self.active_transition.is_none() {
            self.active_transition = self.pending_transition.take();
        }
        invalidated
    }

    pub fn stop_in_progress(&self) -> bool {
        self.stop_barrier.is_some()
    }

    /// Removes the ticket and returns the view it was showing. When the active
    /// one finishes, the queued one is promoted with its view attached.
    pub fn finish_transition(
        &mut self,
        ticket: &ClientTransitionTicket,
    ) -> Option<ClientTransitionView> {
        if self
            .active_transition
            .as_ref()
            .is_some_and(|active| active.ticket == *ticket)
        {
            let finished = self
                .active_transition
                .take()
                .and_then(|mut active| active.view.take());
            if self.stop_barrier.is_none() {
                self.active_transition = self.pending_transition.take();
            }
            finished
        } else if self.transition_is_pending(ticket) {
            self.pending_transition
                .take()
                .and_then(|mut pending| pending.view.take())
        } else {
            None
        }
    }

    pub fn finish_stop(&mut self, ticket: &ClientTransitionTicket) {
        if self.stop_barrier.as_ref() == Some(ticket) {
            self.stop_barrier = None;
            self.stop_view = None;
        }
    }

    fn next_ticket(&mut self) -> ClientTransitionTicket {
        self.next_sequence = self.next_sequence.saturating_add(1);
        ClientTransitionTicket {
            id: format!("transition-{}", self.next_sequence),
            invalidation_generation: self.generation,
        }
    }
}

pub struct ClientTransitionRuntime {
    coordinator: Mutex<ClientTransitionCoordinator>,
    notification: tokio::sync::Notify,
}

impl Default for ClientTransitionRuntime {
    fn default() -> Self {
        Self {
            coordinator: Mutex::new(ClientTransitionCoordinator::default()),
            notification: tokio::sync::Notify::new(),
        }
    }
}

impl ClientTransitionRuntime {
    pub fn try_transition(
        &self,
        requirement: ClientTransitionRequirement,
    ) -> Result<ClientTransitionTicket, TransitionStartError> {
        self.coordinator().try_transition(requirement)
    }

    pub fn request_stop(&self) -> StopTransitionRequest {
        let request = self.coordinator().request_stop();
        self.notification.notify_waiters();
        request
    }

    pub fn request_latest_transition(
        &self,
        requirement: ClientTransitionRequirement,
    ) -> Result<TransitionRequest, TransitionStartError> {
        let request = self.coordinator().request_latest_transition(requirement);
        if request.is_ok() {
            self.notification.notify_waiters();
        }
        request
    }

    pub async fn wait_for_transition(
        &self,
        ticket: &ClientTransitionTicket,
    ) -> TransitionWaitOutcome {
        loop {
            let notified = self.notification.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let coordinator = self.coordinator();
                if coordinator.stop_in_progress() {
                    return TransitionWaitOutcome::Cancelled;
                } else if coordinator.transition_is_current(ticket) {
                    return TransitionWaitOutcome::Started;
                } else if coordinator.transition_is_pending(ticket) {
                    // Wait for promotion, replacement, or Stop below.
                } else {
                    return TransitionWaitOutcome::Superseded;
                }
            }
            notified.as_mut().await;
        }
    }

    pub fn transition_is_current(&self, ticket: &ClientTransitionTicket) -> bool {
        self.coordinator().transition_is_current(ticket)
    }

    /// Resolves once the transition is no longer current or pending, or a Stop
    /// is in progress. Used to wake waits without polling.
    pub async fn wait_for_transition_end(&self, ticket: &ClientTransitionTicket) {
        loop {
            let notified = self.notification.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let coordinator = self.coordinator();
                if coordinator.stop_in_progress()
                    || (!coordinator.transition_is_current(ticket)
                        && !coordinator.transition_is_pending(ticket))
                {
                    return;
                }
            }
            notified.as_mut().await;
        }
    }

    pub fn commit_transition<T, E>(
        &self,
        ticket: &ClientTransitionTicket,
        commit: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, TransitionCommitError<E>> {
        self.coordinator().commit_transition(ticket, commit)
    }

    pub fn invalidate_adapter_transitions(&self) -> bool {
        let invalidated = self.coordinator().invalidate_adapter_transitions();
        if invalidated {
            self.notification.notify_waiters();
        }
        invalidated
    }

    pub fn stop_in_progress(&self) -> bool {
        self.coordinator().stop_in_progress()
    }

    pub fn finish_transition(
        &self,
        ticket: &ClientTransitionTicket,
    ) -> Option<ClientTransitionView> {
        let finished = self.coordinator().finish_transition(ticket);
        self.notification.notify_waiters();
        finished
    }

    pub fn finish_stop(&self, ticket: &ClientTransitionTicket) {
        self.coordinator().finish_stop(ticket);
        self.notification.notify_waiters();
    }

    pub fn transition_is_pending(&self, ticket: &ClientTransitionTicket) -> bool {
        self.coordinator().transition_is_pending(ticket)
    }

    pub fn attach_view(&self, ticket: &ClientTransitionTicket, view: ClientTransitionView) {
        self.coordinator().attach_view(ticket, view);
    }

    pub fn views(&self) -> TransitionViews {
        self.coordinator().views()
    }

    pub fn set_stage(&self, stage: ClientTransitionStage, message: String) -> bool {
        self.coordinator().set_stage(stage, message)
    }

    pub fn begin_finalizing(&self, ticket: &ClientTransitionTicket) -> bool {
        self.coordinator().begin_finalizing(ticket)
    }

    fn coordinator(&self) -> MutexGuard<'_, ClientTransitionCoordinator> {
        self.coordinator
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests;
