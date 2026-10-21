//! Cancellation for in-flight `stream_thumbnails` extractions.
//!
//! Each stream is keyed by the label of the webview that invoked it, as
//! injected by Tauri, and its channel id, so a webview can only cancel its own
//! streams. Cancelling sets the stream's stop flag, which the decode worker
//! checks between native calls, and then aborts the async task.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{
   Arc, Mutex, MutexGuard, PoisonError,
   atomic::{AtomicBool, Ordering},
};
use tauri::async_runtime::JoinHandle;
use tauri::ipc::InvokeResponseBody;

type StreamKey = (String, u32);

struct StreamEntry {
   window_label: String,
   stop: Arc<AtomicBool>,
   task: JoinHandle<()>,
}

impl StreamEntry {
   fn cancel(self) {
      // The flag first: the abort cannot reach a decode already on the
      // blocking pool, but the flag can.
      self.stop.store(true, Ordering::Relaxed);
      self.task.abort();
   }
}

type Entries = Arc<Mutex<HashMap<StreamKey, StreamEntry>>>;

/// Map operations cannot leave the table half-updated, so a poisoned lock is
/// still safe to use; giving up would leave streams uncancellable.
fn lock(entries: &Entries) -> MutexGuard<'_, HashMap<StreamKey, StreamEntry>> {
   entries.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Removes its stream's entry when the task ends, or when the task is dropped
/// without ever being polled, unless a later stream has taken the key.
struct EntryGuard {
   entries: Entries,
   key: StreamKey,
   stop: Arc<AtomicBool>,
}

impl Drop for EntryGuard {
   fn drop(&mut self) {
      let mut entries = lock(&self.entries);
      if entries
         .get(&self.key)
         .is_some_and(|entry| Arc::ptr_eq(&entry.stop, &self.stop))
      {
         entries.remove(&self.key);
      }
   }
}

#[derive(Default)]
pub(crate) struct ThumbnailStreams {
   entries: Entries,
}

impl ThumbnailStreams {
   /// Spawns the future `run` builds from a fresh stop flag and keeps it
   /// cancellable until it ends. A stream already holding the key is
   /// cancelled, since its channel id now belongs to another stream.
   pub(crate) fn spawn<Fut>(
      &self,
      webview_label: &str,
      window_label: &str,
      channel_id: u32,
      run: impl FnOnce(Arc<AtomicBool>) -> Fut,
   ) where
      Fut: Future<Output = ()> + Send + 'static,
   {
      let key = (webview_label.to_owned(), channel_id);
      let stop = Arc::new(AtomicBool::new(false));
      let guard = EntryGuard {
         entries: Arc::clone(&self.entries),
         key: key.clone(),
         stop: Arc::clone(&stop),
      };
      let future = run(Arc::clone(&stop));
      let displaced = {
         let mut entries = lock(&self.entries);
         // Spawning under the lock keeps a task that finishes at once from
         // running its guard before its entry exists.
         let task = tauri::async_runtime::spawn(async move {
            let _guard = guard;
            future.await;
         });
         entries.insert(
            key,
            StreamEntry {
               window_label: window_label.to_owned(),
               stop,
               task,
            },
         )
      };
      if let Some(entry) = displaced {
         entry.cancel();
      }
   }

   /// Cancels one stream of `webview_label`. An unknown or finished id is
   /// not an error, so repeated calls are harmless.
   pub(crate) fn cancel(&self, webview_label: &str, channel_id: u32) {
      let entry = lock(&self.entries).remove(&(webview_label.to_owned(), channel_id));
      if let Some(entry) = entry {
         entry.cancel();
      }
   }

   /// Cancels every stream of a webview that starts loading a page: the
   /// document that would receive their messages is gone.
   pub(crate) fn cancel_webview(&self, webview_label: &str) {
      self.cancel_where(|(label, _), _| label == webview_label);
   }

   /// Cancels every stream started by a webview of a destroyed window.
   pub(crate) fn cancel_window(&self, window_label: &str) {
      self.cancel_where(|_, entry| entry.window_label == window_label);
   }

   fn cancel_where(&self, mut matches: impl FnMut(&StreamKey, &StreamEntry) -> bool) {
      let cancelled = lock(&self.entries)
         .extract_if(|key, entry| matches(key, entry))
         .map(|(_, entry)| entry)
         .collect::<Vec<_>>();
      for entry in cancelled {
         entry.cancel();
      }
   }
}

/// The last message of a stream: `{ "done": <thumbnails sent> }` or
/// `{ "error": <the error as a rejected invoke would carry it> }`.
pub(crate) fn terminal_message(result: crate::Result<u32>) -> InvokeResponseBody {
   let message = match result {
      Ok(sent) => serde_json::json!({ "done": sent }),
      Err(error) => serde_json::json!({ "error": error }),
   };
   InvokeResponseBody::Json(message.to_string())
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::time::{Duration, Instant};

   fn keys(streams: &ThumbnailStreams) -> Vec<StreamKey> {
      let mut keys = lock(&streams.entries).keys().cloned().collect::<Vec<_>>();
      keys.sort();
      keys
   }

   fn wait_until(mut condition: impl FnMut() -> bool) {
      let deadline = Instant::now() + Duration::from_secs(5);
      while !condition() {
         assert!(Instant::now() < deadline, "condition not reached in time");
         std::thread::sleep(Duration::from_millis(1));
      }
   }

   /// Spawns a stream that never finishes on its own and returns its flag.
   fn spawn_pending(
      streams: &ThumbnailStreams,
      webview: &str,
      window: &str,
      id: u32,
   ) -> Arc<AtomicBool> {
      let mut flag = None;
      streams.spawn(webview, window, id, |stop| {
         flag = Some(stop);
         std::future::pending()
      });
      flag.expect("spawn builds the future")
   }

   #[test]
   fn streams_that_finish_at_once_leave_no_entry() {
      let streams = ThumbnailStreams::default();
      for id in 0..100 {
         streams.spawn("main", "main", id, |_| async {});
      }
      wait_until(|| keys(&streams).is_empty());
   }

   #[test]
   fn dropping_the_task_removes_its_entry() {
      let streams = ThumbnailStreams::default();
      spawn_pending(&streams, "main", "main", 1);

      lock(&streams.entries)[&("main".to_owned(), 1)].task.abort();

      wait_until(|| keys(&streams).is_empty());
   }

   #[test]
   fn an_earlier_guard_keeps_a_later_entry_with_the_same_key() {
      let streams = ThumbnailStreams::default();
      let later = spawn_pending(&streams, "main", "main", 1);
      let earlier = EntryGuard {
         entries: Arc::clone(&streams.entries),
         key: ("main".to_owned(), 1),
         stop: Arc::new(AtomicBool::new(false)),
      };

      drop(earlier);

      assert_eq!(keys(&streams), vec![("main".to_owned(), 1)]);
      streams.cancel("main", 1);
      assert!(later.load(Ordering::Relaxed));
   }

   #[test]
   fn a_reused_key_cancels_the_stream_that_held_it() {
      let streams = ThumbnailStreams::default();
      let first = spawn_pending(&streams, "main", "main", 1);
      let second = spawn_pending(&streams, "main", "main", 1);

      assert!(first.load(Ordering::Relaxed));
      assert!(!second.load(Ordering::Relaxed));
      assert_eq!(keys(&streams), vec![("main".to_owned(), 1)]);
      streams.cancel("main", 1);
   }

   #[test]
   fn cancelling_twice_or_an_unknown_id_is_harmless() {
      let streams = ThumbnailStreams::default();
      let stop = spawn_pending(&streams, "main", "main", 1);

      streams.cancel("main", 1);
      streams.cancel("main", 1);
      streams.cancel("main", 2);

      assert!(stop.load(Ordering::Relaxed));
      assert!(keys(&streams).is_empty());
   }

   #[test]
   fn cancellation_is_scoped_to_the_webview_id_and_window() {
      let streams = ThumbnailStreams::default();
      let main_1 = spawn_pending(&streams, "main", "main", 1);
      let main_2 = spawn_pending(&streams, "main", "main", 2);
      let panel_1 = spawn_pending(&streams, "panel", "settings", 1);

      // Another webview cannot reach `main`'s stream with the same id.
      streams.cancel("other", 1);
      assert_eq!(keys(&streams).len(), 3);

      streams.cancel("main", 1);
      assert!(main_1.load(Ordering::Relaxed));
      assert!(!main_2.load(Ordering::Relaxed));
      assert!(!panel_1.load(Ordering::Relaxed));

      // A page load in `main` (simulated) leaves `panel` running.
      streams.cancel_webview("main");
      assert!(main_2.load(Ordering::Relaxed));
      assert!(!panel_1.load(Ordering::Relaxed));

      // Destroying `panel`'s window (simulated) matches by window label.
      streams.cancel_window("panel");
      assert!(!panel_1.load(Ordering::Relaxed));
      streams.cancel_window("settings");
      assert!(panel_1.load(Ordering::Relaxed));
      assert!(keys(&streams).is_empty());
   }

   #[test]
   fn terminal_messages_carry_the_count_or_the_serialized_error() {
      let InvokeResponseBody::Json(done) = terminal_message(Ok(2)) else {
         panic!("the terminal is JSON")
      };
      assert_eq!(
         serde_json::from_str::<serde_json::Value>(&done).unwrap(),
         serde_json::json!({ "done": 2 })
      );

      let error = crate::Error::from(media_parser::MediaParserError::Cancelled);
      let serialized = serde_json::to_value(&error).unwrap();
      let InvokeResponseBody::Json(failed) = terminal_message(Err(error)) else {
         panic!("the terminal is JSON")
      };
      assert_eq!(
         serde_json::from_str::<serde_json::Value>(&failed).unwrap(),
         serde_json::json!({ "error": serialized })
      );
      assert_eq!(serialized, "Media parser error: Operation cancelled");
   }
}
