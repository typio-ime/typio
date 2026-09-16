//! Owned per-client input context.

use crate::core::engine::{Composition, ContextOutput, EngineError, EngineMode, InputContext};
use crate::core::registry::EngineRegistry;
use crate::instance::{RuntimeState, TypioInstance};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Ordered host-facing output emitted by a context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextEvent {
    /// Commit text.
    Commit(String),
    /// Replace the composition atomically.
    Composition(Composition),
    /// Delete UTF-8 bytes around the client cursor.
    DeleteSurrounding {
        /// Bytes before the cursor.
        before: u32,
        /// Bytes after the cursor.
        after: u32,
    },
}

/// Input state associated with one focused application context.
pub struct TypioInputContext {
    registry: Rc<RefCell<EngineRegistry>>,
    runtime: Rc<RefCell<RuntimeState>>,
    focused: bool,
    surrounding_text: String,
    surrounding_cursor: i32,
    surrounding_anchor: i32,
    composition: Composition,
    engine_context: InputContext,
    pending_events: Vec<ContextEvent>,
}

impl TypioInputContext {
    /// Create an owned context attached to an initialized runtime.
    pub fn new_rust(instance: &TypioInstance) -> Box<Self> {
        Box::new(Self {
            registry: instance.registry.clone(),
            runtime: instance.runtime.clone(),
            focused: false,
            surrounding_text: String::new(),
            surrounding_cursor: 0,
            surrounding_anchor: 0,
            composition: Composition::default(),
            engine_context: InputContext::new(
                NEXT_CONTEXT_ID.fetch_add(1, Ordering::Relaxed).max(1),
            ),
            pending_events: Vec::new(),
        })
    }

    /// Notify the active keyboard that focus was gained.
    pub fn focus_in(&mut self) {
        if self.focused {
            return;
        }
        self.focused = true;
        self.registry
            .borrow_mut()
            .focus_in_active_keyboard(&mut self.engine_context);
        self.reconcile_mode(false);
        self.apply_engine_outputs();
    }

    /// Notify the active keyboard that focus was lost.
    pub fn focus_out(&mut self) {
        if !self.focused {
            return;
        }
        self.registry
            .borrow_mut()
            .focus_out_active_keyboard(&mut self.engine_context);
        self.apply_engine_outputs();
        self.focused = false;
    }

    /// Whether this context currently owns focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Clear composition and reset the active keyboard context.
    pub fn reset(&mut self) {
        self.composition = Composition::default();
        self.pending_events
            .push(ContextEvent::Composition(self.composition.clone()));
        self.registry
            .borrow_mut()
            .reset_active_keyboard(&mut self.engine_context);
        self.reconcile_mode(false);
        self.apply_engine_outputs();
    }

    /// Update surrounding UTF-8 text.
    pub fn set_surrounding(&mut self, text: &str, cursor: i32, anchor: i32) {
        self.surrounding_text.clear();
        self.surrounding_text.push_str(text);
        self.surrounding_cursor = cursor;
        self.surrounding_anchor = anchor;
    }

    /// Update the locally highlighted candidate.
    pub fn set_candidate_selection(&mut self, selected: usize) -> crate::core::engine::Result<()> {
        if selected >= self.composition.candidates.len() {
            return Err(EngineError::InvalidArgument);
        }
        self.composition.selected = selected as i32;
        Ok(())
    }

    /// Ask the active engine to commit a candidate.
    pub fn commit_candidate(&mut self, index: i32) -> crate::core::engine::Result<()> {
        let result = self
            .registry
            .borrow_mut()
            .commit_candidate_active_keyboard(&mut self.engine_context, index);
        self.apply_engine_outputs();
        result
    }

    /// Restore an active keyboard mode for this context.
    pub fn set_active_mode(&mut self, mode_id: &str) -> crate::core::engine::Result<()> {
        let result = self
            .registry
            .borrow_mut()
            .set_active_mode_keyboard(&mut self.engine_context, mode_id);
        self.reconcile_mode(false);
        self.apply_engine_outputs();
        result
    }

    /// Forward a typed key event and report whether the engine consumed it.
    pub fn process_key(&mut self, event: &crate::core::engine::KeyEvent) -> bool {
        let result = self
            .registry
            .borrow_mut()
            .process_key_active_keyboard(&mut self.engine_context, event);
        self.reconcile_mode(true);
        self.apply_engine_outputs();
        result != crate::core::engine::KeyProcessResult::NotHandled
    }

    /// Drain ordered output for the embedding host.
    pub fn drain_events(&mut self) -> impl Iterator<Item = ContextEvent> + '_ {
        self.pending_events.drain(..)
    }

    fn reconcile_mode(&mut self, announce: bool) {
        let mode: Option<EngineMode> = self
            .registry
            .borrow_mut()
            .take_active_keyboard_changed_mode();
        if let Some(mode) = mode {
            self.runtime.borrow_mut().observe_mode(mode, announce);
        }
    }

    fn apply_engine_outputs(&mut self) {
        for output in self.engine_context.drain_outputs() {
            match output {
                ContextOutput::Commit(text) => {
                    self.registry.borrow_mut().notify_keyboard_commit();
                    self.pending_events.push(ContextEvent::Commit(text));
                }
                ContextOutput::Clear => {
                    self.composition = Composition::default();
                    self.pending_events
                        .push(ContextEvent::Composition(self.composition.clone()));
                }
                ContextOutput::Composition(composition) => {
                    self.composition = composition.clone();
                    self.pending_events
                        .push(ContextEvent::Composition(composition));
                }
            }
        }
    }
}
