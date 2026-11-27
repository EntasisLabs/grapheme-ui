//! Store session on a worker thread.
//!
//! The UI thread sends action ids and receives snapshots. A task `apply`
//! notifies [`grapheme_runtime::StoreSession::set_commit_observer`] before the
//! next step, and the worker waits until the UI has accepted that snapshot.
//! The UI presents the frame before acknowledging, so a blocking capability
//! does not freeze Update and `pending` is on screen before `ready`.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use grapheme_runtime::{AppSnapshot, StoreCommit};
use grapheme_sdk::{GraphemeSdkError, GraphemeStoreSession};
use serde_json::Value as JsonValue;

use crate::error::GraphemeUiError;

struct Job {
    id: String,
    payload: Option<JsonValue>,
}

enum Cmd {
    Invoke(Job),
    AppSnapshot,
    Checkout,
    Checkin(Box<GraphemeStoreSession>),
    Shutdown,
}

struct Finished {
    result: Result<StoreCommit, GraphemeSdkError>,
    snapshot: Result<AppSnapshot, GraphemeSdkError>,
}

enum Event {
    /// A task commit is visible. The worker waits for an ack before continuing.
    Step(AppSnapshot),
    Finished(Box<Finished>),
    Snapshot(Result<AppSnapshot, GraphemeSdkError>),
    CheckedOut(Box<GraphemeStoreSession>),
}

/// What the UI thread should do with one worker message.
///
/// Snapshots are already owned. Boxing them would not change the channel,
/// and the extra `status` field on a commit pushed the variant over clippy's
/// size threshold.
#[allow(clippy::large_enum_variant)]
pub(crate) enum WorkerEvent {
    /// Project this snapshot, then let the frame end before the task continues.
    Step(AppSnapshot),
    /// An invoke finished. `snapshot` is the view after it, when evaluation worked.
    Done {
        result: Result<StoreCommit, GraphemeSdkError>,
        snapshot: Option<AppSnapshot>,
    },
}

pub(crate) struct SessionBridge {
    cmds: Sender<Cmd>,
    events: Receiver<Event>,
    acks: Sender<()>,
    join: Option<JoinHandle<()>>,
    /// Set while [`SessionBridge::checkout`] has the session on this thread.
    local: Option<GraphemeStoreSession>,
    queue: VecDeque<Job>,
    inflight: bool,
    /// The UI projected a step and has not yet let the worker continue.
    ack_owed: bool,
    /// The worker is past an ack and may be inside a capability. Do not block
    /// Update waiting for it.
    awaiting_capability: bool,
}

impl SessionBridge {
    pub(crate) fn spawn(session: GraphemeStoreSession) -> Result<Self, GraphemeUiError> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let (ack_tx, ack_rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("grapheme-session".into())
            .spawn(move || worker(session, cmd_rx, event_tx, ack_rx))
            .map_err(|err| {
                GraphemeUiError::Usage(format!("failed to start the session thread: {err}"))
            })?;
        Ok(Self {
            cmds: cmd_tx,
            events: event_rx,
            acks: ack_tx,
            join: Some(join),
            local: None,
            queue: VecDeque::new(),
            inflight: false,
            ack_owed: false,
            awaiting_capability: false,
        })
    }

    pub(crate) fn inflight(&self) -> bool {
        self.inflight
    }

    pub(crate) fn has_work(&self) -> bool {
        self.inflight || !self.queue.is_empty()
    }

    /// Move a checked-out session back to the worker.
    pub(crate) fn ensure_checked_in(&mut self) -> Result<(), GraphemeUiError> {
        if let Some(session) = self.local.take() {
            self.send(Cmd::Checkin(Box::new(session)))?;
        }
        Ok(())
    }

    /// Block until the worker is idle and return the session on this thread.
    ///
    /// Steps are acknowledged immediately so a running task can finish. The
    /// caller is not the frame loop; dropping the borrow does not send the
    /// session back. The next drain or invoke checks it in.
    pub(crate) fn checkout(&mut self) -> Result<&mut GraphemeStoreSession, GraphemeUiError> {
        if self.local.is_none() {
            // A task step may be parked waiting for this thread to ack.
            self.ack_now();
            self.send(Cmd::Checkout)?;
            loop {
                match self.recv_raw()? {
                    Event::Step(_) => {
                        let _ = self.acks.send(());
                        self.ack_owed = false;
                    }
                    Event::Finished(finished) => {
                        let result = finished.result;
                        self.inflight = false;
                        self.awaiting_capability = false;
                        self.ack_owed = false;
                        if result.is_err() {
                            self.queue.clear();
                        }
                    }
                    Event::CheckedOut(session) => {
                        self.local = Some(*session);
                        break;
                    }
                    Event::Snapshot(_) => {}
                }
            }
        }
        Ok(self.local.as_mut().expect("just checked out"))
    }

    pub(crate) fn enqueue(&mut self, id: String, payload: Option<JsonValue>) {
        self.queue.push_back(Job { id, payload });
    }

    pub(crate) fn dispatch(&mut self) -> Result<(), GraphemeUiError> {
        if self.inflight {
            return Ok(());
        }
        let Some(job) = self.queue.pop_front() else {
            return Ok(());
        };
        self.inflight = true;
        self.awaiting_capability = false;
        self.send(Cmd::Invoke(job))
    }

    /// If the previous frame parked on a task step, let the worker continue
    /// into the next step or capability. Update must not block after this.
    pub(crate) fn prepare_frame(&mut self) {
        if self.ack_owed {
            let _ = self.acks.send(());
            self.ack_owed = false;
            self.awaiting_capability = true;
        }
    }

    /// The frame will present `step` before the worker is allowed to continue.
    pub(crate) fn defer_ack(&mut self) {
        self.ack_owed = true;
        self.awaiting_capability = false;
    }

    /// Used by the synchronous `invoke_action` path, which waits for the task.
    ///
    /// Sends an ack only when a previous frame left the worker parked on a step.
    pub(crate) fn ack_now(&mut self) {
        if self.ack_owed {
            self.release_worker();
        }
    }

    /// Let the worker leave the commit observer. Call this once per `Step`
    /// received on the synchronous path; the worker is waiting on this ack.
    pub(crate) fn release_worker(&mut self) {
        let _ = self.acks.send(());
        self.ack_owed = false;
        self.awaiting_capability = false;
    }

    /// Next worker message. `Ok(None)` means a capability is still running and
    /// this Update should return so the frame can be presented.
    pub(crate) fn poll_frame_event(&mut self) -> Result<Option<WorkerEvent>, GraphemeUiError> {
        if !self.has_work() {
            return Ok(None);
        }
        if self.awaiting_capability {
            match self.events.try_recv() {
                Ok(event) => self.interpret(event).map(Some),
                Err(TryRecvError::Empty) => Ok(None),
                Err(TryRecvError::Disconnected) => Err(stopped()),
            }
        } else {
            let event = self.events.recv().map_err(|_| stopped())?;
            self.interpret(event).map(Some)
        }
    }

    /// Block until the worker sends something. For `invoke_action`.
    pub(crate) fn recv_worker_event(&mut self) -> Result<WorkerEvent, GraphemeUiError> {
        let event = self.events.recv().map_err(|_| stopped())?;
        self.interpret(event)
    }

    pub(crate) fn current_snapshot(&mut self) -> Result<AppSnapshot, GraphemeUiError> {
        if let Some(session) = self.local.as_mut() {
            return session.app_snapshot().map_err(Into::into);
        }
        self.send(Cmd::AppSnapshot)?;
        match self.recv_raw()? {
            Event::Snapshot(result) => result.map_err(Into::into),
            Event::CheckedOut(session) => {
                self.local = Some(*session);
                Err(GraphemeUiError::Usage(
                    "session thread returned the session instead of a snapshot".into(),
                ))
            }
            Event::Step(snap) => Ok(snap),
            Event::Finished(finished) => finished.snapshot.map_err(Into::into),
        }
    }

    fn interpret(&mut self, event: Event) -> Result<WorkerEvent, GraphemeUiError> {
        match event {
            Event::Step(snap) => Ok(WorkerEvent::Step(snap)),
            Event::Finished(finished) => {
                self.inflight = false;
                self.awaiting_capability = false;
                self.ack_owed = false;
                if finished.result.is_err() {
                    self.queue.clear();
                }
                Ok(WorkerEvent::Done {
                    result: finished.result,
                    snapshot: finished.snapshot.ok(),
                })
            }
            Event::Snapshot(result) => match result {
                Ok(snap) => Ok(WorkerEvent::Step(snap)),
                Err(err) => Err(err.into()),
            },
            Event::CheckedOut(session) => {
                self.local = Some(*session);
                Err(GraphemeUiError::Usage(
                    "session thread checked out during a drain".into(),
                ))
            }
        }
    }

    fn send(&self, cmd: Cmd) -> Result<(), GraphemeUiError> {
        self.cmds.send(cmd).map_err(|_| stopped())
    }

    fn recv_raw(&mut self) -> Result<Event, GraphemeUiError> {
        self.events.recv().map_err(|_| stopped())
    }
}

impl Drop for SessionBridge {
    fn drop(&mut self) {
        if self.ack_owed {
            let _ = self.acks.send(());
        }
        let _ = self.cmds.send(Cmd::Shutdown);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

fn stopped() -> GraphemeUiError {
    GraphemeUiError::Usage("grapheme session thread stopped".into())
}

fn worker(
    mut session: GraphemeStoreSession,
    cmds: Receiver<Cmd>,
    events: Sender<Event>,
    acks: Receiver<()>,
) {
    let acks = Arc::new(Mutex::new(acks));
    install_observer(&mut session, events.clone(), Arc::clone(&acks));
    let mut slot = Some(session);
    while let Ok(cmd) = cmds.recv() {
        match cmd {
            Cmd::Shutdown => break,
            Cmd::Checkin(mut session) => {
                install_observer(&mut session, events.clone(), Arc::clone(&acks));
                slot = Some(*session);
            }
            Cmd::Checkout => {
                if let Some(mut session) = slot.take() {
                    session.set_commit_observer(None);
                    if events.send(Event::CheckedOut(Box::new(session))).is_err() {
                        break;
                    }
                }
            }
            Cmd::AppSnapshot => {
                let snapshot = match slot.as_mut() {
                    Some(session) => session.app_snapshot(),
                    None => Err(GraphemeSdkError::Contract("session is checked out".into())),
                };
                if events.send(Event::Snapshot(snapshot)).is_err() {
                    break;
                }
            }
            Cmd::Invoke(job) => {
                let Some(session) = slot.as_mut() else {
                    let err = GraphemeSdkError::Contract("session is checked out".into());
                    let _ = events.send(Event::Finished(Box::new(Finished {
                        result: Err(err),
                        snapshot: Err(GraphemeSdkError::Contract("session is checked out".into())),
                    })));
                    continue;
                };
                let result = session.invoke_action(&job.id, job.payload);
                let snapshot = session.app_snapshot();
                if events
                    .send(Event::Finished(Box::new(Finished { result, snapshot })))
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

fn install_observer(
    session: &mut GraphemeStoreSession,
    events: Sender<Event>,
    acks: Arc<Mutex<Receiver<()>>>,
) {
    session.set_commit_observer(Some(Arc::new(move |runtime, _revision| {
        if let Ok(snap) = runtime.app_snapshot() {
            if events.send(Event::Step(snap)).is_err() {
                return;
            }
            let guard = acks.lock().unwrap_or_else(|err| err.into_inner());
            let _ = guard.recv();
        }
    })));
}
