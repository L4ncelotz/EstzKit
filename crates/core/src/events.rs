//! Event sink abstraction. Core never knows Tauri; hosts implement
//! [`EventSink`] to deliver [`StepEvent`](crate::StepEvent) values.

use crate::StepEvent;

/// Name of the event the Tauri host emits for each [`StepEvent`] transition.
pub const INSTALL_STATE_CHANGED_EVENT: &str = "install:state_changed";

/// Consumer of [`StepEvent`]s emitted by the executor and providers.
///
/// Implementors: the Tauri host forwards events to the frontend under the
/// `"install:state_changed"` event name; the CLI prints them to stdout.
pub trait EventSink {
    /// Deliver one step transition event.
    fn emit(&mut self, event: StepEvent);
}

/// No-op sink used in tests.
#[derive(Debug, Default)]
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&mut self, _event: StepEvent) {}
}

/// Sink that collects events in memory (useful for tests and the CLI).
#[derive(Debug, Default)]
pub struct CollectSink {
    /// Events received so far, in order.
    pub events: Vec<StepEvent>,
}

impl EventSink for CollectSink {
    fn emit(&mut self, event: StepEvent) {
        self.events.push(event);
    }
}
