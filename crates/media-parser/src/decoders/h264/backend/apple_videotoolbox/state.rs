//! Framework-independent callback ownership, event delivery and lifecycle.

use super::super::FrameSink;
use super::image::OwnedNv12;
use crate::decoders::h264::{DecodeError, FrameToken};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

#[derive(Debug)]
pub(super) struct CallbackTicket {
   token: FrameToken,
   completion_count: AtomicUsize,
}

impl CallbackTicket {
   pub(super) fn new(token: FrameToken) -> Self {
      Self {
         token,
         completion_count: AtomicUsize::new(0),
      }
   }

   pub(super) fn token(&self) -> FrameToken {
      self.token
   }

   pub(super) fn increment_completion(&self) -> usize {
      self
         .completion_count
         .fetch_add(1, Ordering::AcqRel)
         .saturating_add(1)
   }

   pub(super) fn completion_count(&self) -> usize {
      self.completion_count.load(Ordering::Acquire)
   }
}

pub(super) fn validate_completion(
   ticket: &CallbackTicket,
   native_status: i32,
) -> Result<(), DecodeError> {
   let count = ticket.completion_count();
   match (native_status, count) {
      (0, 1) => Ok(()),
      (status, 0) if status != 0 => Ok(()),
      (0, 0) => Err(DecodeError::BackendContract(format!(
         "Apple VideoToolbox missing callback for token {:?}",
         ticket.token()
      ))),
      (0, count) => Err(DecodeError::BackendContract(format!(
         "Apple VideoToolbox received {count} callbacks for token {:?}",
         ticket.token()
      ))),
      (_, count) => Err(DecodeError::BackendContract(format!(
         "Apple VideoToolbox native failure produced {count} callbacks for token {:?}",
         ticket.token()
      ))),
   }
}

#[derive(Debug)]
enum CallbackEvent {
   Frame(FrameToken, OwnedNv12),
   Fatal(DecodeError),
}

#[derive(Debug, Default)]
struct PendingEvents {
   events: VecDeque<CallbackEvent>,
   accepting: bool,
}

impl PendingEvents {
   fn new() -> Self {
      Self {
         events: VecDeque::new(),
         accepting: true,
      }
   }
}

#[derive(Debug)]
pub(super) struct CallbackState {
   pending: Mutex<PendingEvents>,
   #[cfg(apple_videotoolbox_backend)]
   expected_pixel_format: u32,
}

impl CallbackState {
   pub(super) fn new(expected_pixel_format: u32) -> Self {
      #[cfg(not(apple_videotoolbox_backend))]
      let _ = expected_pixel_format;
      Self {
         pending: Mutex::new(PendingEvents::new()),
         #[cfg(apple_videotoolbox_backend)]
         expected_pixel_format,
      }
   }

   #[cfg(apple_videotoolbox_backend)]
   pub(super) fn expected_pixel_format(&self) -> u32 {
      self.expected_pixel_format
   }

   fn lock_pending(&self) -> Result<MutexGuard<'_, PendingEvents>, DecodeError> {
      self.pending.lock().map_err(|_| {
         DecodeError::BackendContract(
            "Apple VideoToolbox callback state mutex was poisoned".to_string(),
         )
      })
   }

   pub(super) fn push_frame(&self, token: FrameToken, frame: OwnedNv12) -> Result<(), DecodeError> {
      let mut pending = self.lock_pending()?;
      if pending.accepting {
         pending.events.push_back(CallbackEvent::Frame(token, frame));
      }
      Ok(())
   }

   pub(super) fn push_fatal(&self, error: DecodeError) -> Result<(), DecodeError> {
      let mut pending = self.lock_pending()?;
      if pending.accepting {
         pending.accepting = false;
         pending.events.clear();
         pending.events.push_back(CallbackEvent::Fatal(error));
      }
      Ok(())
   }

   pub(super) fn record_null_ticket(&self) -> Result<(), DecodeError> {
      self.push_fatal(DecodeError::BackendContract(
         "Apple VideoToolbox callback received a null callback ticket".to_string(),
      ))
   }

   pub(super) fn contain_callback(&self, callback: impl FnOnce() -> Result<(), DecodeError>) {
      let fatal = match catch_unwind(AssertUnwindSafe(callback)) {
         Ok(Ok(())) => return,
         Ok(Err(error)) => error,
         Err(_) => {
            DecodeError::BackendContract("Apple VideoToolbox output callback panicked".to_string())
         }
      };
      // If the mutex is already poisoned, `deliver` will surface that contract
      // error. A callback must never unwrap or unwind while trying to report it.
      let _ = self.push_fatal(fatal);
   }

   fn stop_and_clear(&self) {
      if let Ok(mut pending) = self.lock_pending() {
         pending.accepting = false;
         pending.events.clear();
      }
   }

   #[cfg(apple_videotoolbox_backend)]
   pub(super) fn discard(&self) {
      self.stop_and_clear();
   }

   pub(super) fn deliver(&self, sink: &mut FrameSink<'_>) -> Result<(), DecodeError> {
      let mut events = {
         let mut pending = self.lock_pending()?;
         std::mem::take(&mut pending.events)
      };
      while let Some(event) = events.pop_front() {
         match event {
            CallbackEvent::Frame(token, frame) => {
               if let Err(error) = sink(token, &frame.as_planar()) {
                  self.stop_and_clear();
                  return Err(error);
               }
            }
            CallbackEvent::Fatal(error) => {
               self.stop_and_clear();
               return Err(error);
            }
         }
      }
      Ok(())
   }

   pub(super) fn ensure_empty(&self) -> Result<(), DecodeError> {
      let mut pending = self.lock_pending()?;
      let Some(event) = pending.events.pop_front() else {
         return Ok(());
      };
      pending.accepting = false;
      pending.events.clear();
      match event {
         CallbackEvent::Fatal(error) => Err(error),
         CallbackEvent::Frame(_, _) => Err(DecodeError::BackendContract(
            "Apple VideoToolbox left an undelivered callback event after drain".to_string(),
         )),
      }
   }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DecoderLifecycle {
   Active,
   Drained,
   Fatal,
}

impl DecoderLifecycle {
   pub(super) fn ensure_decode(self) -> Result<(), DecodeError> {
      match self {
         Self::Active => Ok(()),
         Self::Drained => Err(DecodeError::BackendContract(
            "Apple VideoToolbox received decode after drain".to_string(),
         )),
         Self::Fatal => Err(DecodeError::BackendContract(
            "Apple VideoToolbox decoder is in a fatal state".to_string(),
         )),
      }
   }

   pub(super) fn begin_drain(&mut self) -> Result<(), DecodeError> {
      self.ensure_decode()?;
      *self = Self::Drained;
      Ok(())
   }

   pub(super) fn mark_fatal(&mut self) {
      *self = Self::Fatal;
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::decoders::h264::frame::PlanarYuv;
   use crate::decoders::h264::{DecodeError, FrameToken};

   fn owned(value: u8) -> OwnedNv12 {
      OwnedNv12::from_compact_for_test(2, 2, vec![value; 4], vec![value; 2])
   }

   #[test]
   fn ticket_preserves_every_token_without_using_timing() {
      for value in [0, 7, u64::MAX] {
         let ticket = CallbackTicket::new(FrameToken::new(value));
         assert_eq!(ticket.token(), FrameToken::new(value));
         assert_eq!(ticket.completion_count(), 0);
         assert_eq!(ticket.increment_completion(), 1);
         assert_eq!(ticket.increment_completion(), 2);
      }
   }

   #[test]
   fn validates_missing_duplicate_and_impossible_native_error_completions() {
      let completed = CallbackTicket::new(FrameToken::new(0));
      completed.increment_completion();
      assert_eq!(validate_completion(&completed, 0), Ok(()));

      let missing = CallbackTicket::new(FrameToken::new(1));
      assert!(matches!(
         validate_completion(&missing, 0),
         Err(DecodeError::BackendContract(message)) if message.contains("missing")
      ));
      assert_eq!(validate_completion(&missing, -12902), Ok(()));

      let duplicate = CallbackTicket::new(FrameToken::new(2));
      duplicate.increment_completion();
      duplicate.increment_completion();
      assert!(matches!(
         validate_completion(&duplicate, 0),
         Err(DecodeError::BackendContract(message)) if message.contains("2 callbacks")
      ));
      assert!(matches!(
         validate_completion(&duplicate, -12902),
         Err(DecodeError::BackendContract(message)) if message.contains("native failure")
      ));
   }

   #[test]
   fn delivers_fifo_events_without_holding_the_mutex() {
      let state = CallbackState::new(0);
      state
         .push_frame(FrameToken::new(2), owned(2))
         .expect("first frame");
      state
         .push_frame(FrameToken::new(1), owned(1))
         .expect("second frame");
      let mut delivered = Vec::new();

      state
         .deliver(&mut |token, _frame: &PlanarYuv<'_>| {
            state.push_frame(FrameToken::new(3), owned(3))?;
            delivered.push(token);
            Ok(())
         })
         .expect("initial FIFO delivery");
      state
         .deliver(&mut |token, _frame: &PlanarYuv<'_>| {
            delivered.push(token);
            Ok(())
         })
         .expect("events queued by sink are delivered later");

      assert_eq!(
         delivered,
         [
            FrameToken::new(2),
            FrameToken::new(1),
            FrameToken::new(3),
            FrameToken::new(3),
         ]
      );
   }

   #[test]
   fn rejects_and_discards_events_left_after_delivery() {
      let state = CallbackState::new(0);
      state
         .push_frame(FrameToken::new(1), owned(1))
         .expect("frame");
      let mut calls = 0;

      state
         .deliver(&mut |_token, _| {
            calls += 1;
            state.push_frame(FrameToken::new(2), owned(2))
         })
         .expect("initial event is delivered");

      assert!(matches!(
         state.ensure_empty(),
         Err(DecodeError::BackendContract(message)) if message.contains("undelivered")
      ));
      state
         .deliver(&mut |_token, _| {
            calls += 1;
            Ok(())
         })
         .expect("undelivered events were discarded");
      assert_eq!(calls, 1);

      let fatal_state = CallbackState::new(0);
      fatal_state
         .push_fatal(DecodeError::Backend("first fatal".to_string()))
         .expect("fatal");
      fatal_state
         .push_fatal(DecodeError::Backend("second fatal".to_string()))
         .expect("later fatal is ignored");
      assert!(matches!(
         fatal_state.ensure_empty(),
         Err(DecodeError::Backend(message)) if message == "first fatal"
      ));
   }

   #[test]
   fn first_fatal_discards_all_frames_and_stops_the_sink_after_errors() {
      let state = CallbackState::new(0);
      state
         .push_frame(FrameToken::new(1), owned(1))
         .expect("frame");
      state
         .push_fatal(DecodeError::Backend("first fatal".to_string()))
         .expect("fatal");
      state
         .push_fatal(DecodeError::Backend("second fatal".to_string()))
         .expect("ignored later fatal");
      state
         .push_frame(FrameToken::new(2), owned(2))
         .expect("ignored frame");
      let mut delivered = Vec::new();

      let result = state.deliver(&mut |token, _| {
         delivered.push(token);
         Ok(())
      });

      assert!(delivered.is_empty());
      assert!(matches!(
         result,
         Err(DecodeError::Backend(message)) if message == "first fatal"
      ));

      let sink_error_state = CallbackState::new(0);
      sink_error_state
         .push_frame(FrameToken::new(3), owned(3))
         .expect("first frame");
      sink_error_state
         .push_frame(FrameToken::new(4), owned(4))
         .expect("second frame");
      let mut calls = 0;
      assert!(
         sink_error_state
            .deliver(&mut |_token, _| {
               calls += 1;
               Err(DecodeError::Convert("sink stopped".to_string()))
            })
            .is_err()
      );
      assert_eq!(calls, 1);
      sink_error_state
         .deliver(&mut |_token, _| {
            calls += 1;
            Ok(())
         })
         .expect("remaining frames were discarded");
      assert_eq!(calls, 1);
   }

   #[test]
   fn null_ticket_panic_and_poison_become_contract_errors() {
      let null_state = CallbackState::new(0);
      null_state.record_null_ticket().expect("record null ticket");
      assert!(matches!(
         null_state.deliver(&mut |_token, _| Ok(())),
         Err(DecodeError::BackendContract(message)) if message.contains("null callback ticket")
      ));

      let panic_state = CallbackState::new(0);
      panic_state.contain_callback(|| panic!("callback panic"));
      assert!(matches!(
         panic_state.deliver(&mut |_token, _| Ok(())),
         Err(DecodeError::BackendContract(message)) if message.contains("panicked")
      ));

      let poisoned = CallbackState::new(0);
      let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
         let _guard = poisoned.pending.lock().expect("lock before poisoning");
         panic!("poison it");
      }));
      assert!(matches!(
         poisoned.deliver(&mut |_token, _| Ok(())),
         Err(DecodeError::BackendContract(message)) if message.contains("poisoned")
      ));
   }

   #[test]
   fn lifecycle_rejects_decode_after_drain_and_repeated_drain() {
      let mut lifecycle = DecoderLifecycle::Active;
      lifecycle.ensure_decode().expect("active decode");
      lifecycle.begin_drain().expect("first drain");
      assert!(matches!(
         lifecycle.ensure_decode(),
         Err(DecodeError::BackendContract(_))
      ));
      assert!(matches!(
         lifecycle.begin_drain(),
         Err(DecodeError::BackendContract(_))
      ));
      lifecycle.mark_fatal();
      assert!(matches!(
         lifecycle.ensure_decode(),
         Err(DecodeError::BackendContract(_))
      ));
   }
}
