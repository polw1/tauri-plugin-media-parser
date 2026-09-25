//! Opt-in JVM registry using the same Rust assertion bodies as native libtest.
use crate::encoders::jpeg::android::test_cases;
use jni::{
   JNIEnv,
   objects::{JClass, JString},
};

// Each suite keeps its own `mod common;` exactly as its libtest binary compiles it.
#[allow(clippy::duplicate_mod)]
#[path = "../tests/android_mediacodec.rs"]
mod android_mediacodec;
#[allow(clippy::duplicate_mod)]
#[path = "../tests/mp4_thumbnails.rs"]
mod mp4_thumbnails;

type Case<'a> = (&'static str, Box<dyn Fn() + 'a>);

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plugin_mediaparser_Harness_init(
   env: JNIEnv,
   _: JClass,
   class: JClass,
) -> i32 {
   let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let vm = env.get_java_vm().map_err(|error| error.to_string())?;
      let class = env
         .new_global_ref(class)
         .map_err(|error| error.to_string())?;
      crate::initialize_android_jpeg(vm, class)
   }));
   match result {
      Ok(Ok(())) => 0,
      other => {
         if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
         }
         eprintln!("bootstrap failed: {other:?}");
         1
      }
   }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_plugin_mediaparser_Harness_run(
   mut env: JNIEnv,
   _: JClass,
   mode: JString,
) -> i32 {
   let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let mode: String = env.get_string(&mode).expect("mode string").into();
      if mode == "negative" {
         eprintln!("test intentional_failure ... FAILED");
         panic!("intentional Rust case failure");
      }
      if mode == "missing-runtime" || mode == "failed-bootstrap" {
         println!("EXPECTED {mode}");
         test_cases::missing_runtime();
         println!("test {mode} ... ok");
         println!("JVM_TESTS=1");
         return;
      }
      assert_eq!(mode, "normal");
      run_cases();
   }));
   if result.is_err() {
      if env.exception_check().unwrap_or(false) {
         let _ = env.exception_clear();
      }
      eprintln!("Rust JVM suite FAILED");
      1
   } else {
      0
   }
}

fn run_cases() {
   let runtime = tokio::runtime::Runtime::new().unwrap();
   let cases: Vec<Case<'_>> = vec![
      (
         "retryable_failure_before_first_frame_replays_with_next_decoder",
         Box::new(|| {
            crate::decoders::h264::pipeline::test_support::retryable_failure_before_first_frame_replays_with_next_decoder_case();
         }),
      ),
      (
         "retryable_failure_after_first_frame_does_not_retry",
         Box::new(|| {
            crate::decoders::h264::pipeline::test_support::retryable_failure_after_first_frame_does_not_retry_case();
         }),
      ),
      (
         "retryable_open_failure_opens_the_next_decoder_once",
         Box::new(|| {
            crate::decoders::h264::pipeline::test_support::retryable_open_failure_opens_the_next_decoder_once_case();
         }),
      ),
      (
         "shared_pipeline_decodes_a_single_batch",
         Box::new(|| {
            crate::decoders::h264::pipeline::test_support::shared_pipeline_decodes_a_single_batch_case();
         }),
      ),
      (
         "compatible_batches_reuse_one_decoder_with_sufficient_input_capacity",
         Box::new(|| {
            crate::decoders::h264::pipeline::test_support::compatible_batches_reuse_one_decoder_with_sufficient_input_capacity_case();
         }),
      ),
      (
         "incompatible_batches_open_separate_decoders",
         Box::new(|| {
            crate::decoders::h264::pipeline::test_support::incompatible_batches_open_separate_decoders_case();
         }),
      ),
      (
         "orchestration_returns_reordered_callbacks_in_token_order",
         Box::new(|| {
            crate::decoders::h264::pipeline::test_support::orchestration_returns_reordered_callbacks_in_token_order_case();
         }),
      ),
      (
         "real_decode_job_produces_an_image_from_a_shared_sample_region",
         Box::new(|| {
            runtime.block_on(crate::format::mp4::thumbnails::native_test_cases::real_decode_job_produces_an_image_from_a_shared_sample_region_case());
         }),
      ),
      (
         "mediacodec_reports_a_dropped_corrupt_sample",
         Box::new(|| {
            runtime
               .block_on(android_mediacodec::mediacodec_reports_a_dropped_corrupt_sample_case());
         }),
      ),
      (
         "mediacodec_preserves_deep_b_frame_presentation_order",
         Box::new(|| {
            runtime.block_on(
               android_mediacodec::mediacodec_preserves_deep_b_frame_presentation_order_case(),
            );
         }),
      ),
      (
         "mediacodec_decodes_bt709_through_the_area_scaler",
         Box::new(|| {
            runtime.block_on(
               android_mediacodec::mediacodec_decodes_bt709_through_the_area_scaler_case(),
            );
         }),
      ),
      (
         "mediacodec_honors_media_image_crop_geometry",
         Box::new(|| {
            runtime
               .block_on(android_mediacodec::mediacodec_honors_media_image_crop_geometry_case());
         }),
      ),
      (
         "mediacodec_accepts_empty_avc3_configuration_with_in_band_headers",
         Box::new(|| {
            runtime.block_on(android_mediacodec::mediacodec_accepts_empty_avc3_configuration_with_in_band_headers_case());
         }),
      ),
      (
         "mediacodec_decodes_bt709_full_range_without_limited_range_expansion",
         Box::new(|| {
            runtime.block_on(android_mediacodec::mediacodec_decodes_bt709_full_range_without_limited_range_expansion_case());
         }),
      ),
      (
         "test_mp4_h264_thumbnail_extraction",
         Box::new(|| {
            runtime.block_on(mp4_thumbnails::test_mp4_h264_thumbnail_extraction_case());
         }),
      ),
      (
         "test_mp4_hd_thumbnail_uses_the_area_scaler_and_the_declared_matrix",
         Box::new(|| {
            runtime.block_on(mp4_thumbnails::test_mp4_hd_thumbnail_uses_the_area_scaler_and_the_declared_matrix_case());
         }),
      ),
      (
         "test_mp4_thumbnail_jpeg_capacity_tracks_compressed_bytes",
         Box::new(|| {
            runtime.block_on(
               mp4_thumbnails::test_mp4_thumbnail_jpeg_capacity_tracks_compressed_bytes_case(),
            );
         }),
      ),
      (
         "test_mp4_thumbnail_is_resized_before_jpeg_encoding",
         Box::new(|| {
            runtime
               .block_on(mp4_thumbnails::test_mp4_thumbnail_is_resized_before_jpeg_encoding_case());
         }),
      ),
      (
         "test_mp4_thumbnail_quality_reaches_the_jpeg_encoder",
         Box::new(|| {
            runtime.block_on(
               mp4_thumbnails::test_mp4_thumbnail_quality_reaches_the_jpeg_encoder_case(),
            );
         }),
      ),
      (
         "test_mp4_thumbnail_budget_counts_each_requested_output",
         Box::new(|| {
            runtime.block_on(
               mp4_thumbnails::test_mp4_thumbnail_budget_counts_each_requested_output_case(),
            );
         }),
      ),
      (
         "test_mp4_h264_thumbnails_follow_presentation_order",
         Box::new(|| {
            runtime
               .block_on(mp4_thumbnails::test_mp4_h264_thumbnails_follow_presentation_order_case());
         }),
      ),
      (
         "test_mp4_h264_thumbnails_follow_presentation_order_with_deep_b_frames",
         Box::new(|| {
            runtime.block_on(mp4_thumbnails::test_mp4_h264_thumbnails_follow_presentation_order_with_deep_b_frames_case());
         }),
      ),
      (
         "test_mp4_thumbnail_index_can_be_reused_with_another_reader",
         Box::new(|| {
            runtime.block_on(
               mp4_thumbnails::test_mp4_thumbnail_index_can_be_reused_with_another_reader_case(),
            );
         }),
      ),
      (
         "test_mp4_thumbnail_index_reports_the_automatically_selected_track",
         Box::new(|| {
            runtime.block_on(mp4_thumbnails::test_mp4_thumbnail_index_reports_the_automatically_selected_track_case());
         }),
      ),
      (
         "test_mp4_fast_thumbnail_reports_the_keyframe_pts",
         Box::new(|| {
            runtime
               .block_on(mp4_thumbnails::test_mp4_fast_thumbnail_reports_the_keyframe_pts_case());
         }),
      ),
      (
         "test_mp4_fast_thumbnails_read_each_keyframe_once",
         Box::new(|| {
            runtime
               .block_on(mp4_thumbnails::test_mp4_fast_thumbnails_read_each_keyframe_once_case());
         }),
      ),
      (
         "test_mp4_exact_thumbnails_read_a_shared_gop_once",
         Box::new(|| {
            runtime
               .block_on(mp4_thumbnails::test_mp4_exact_thumbnails_read_a_shared_gop_once_case());
         }),
      ),
      (
         "test_mp4_exact_thumbnails_truncate_the_gop_at_the_last_target",
         Box::new(|| {
            runtime.block_on(
               mp4_thumbnails::test_mp4_exact_thumbnails_truncate_the_gop_at_the_last_target_case(),
            );
         }),
      ),
      (
         "test_mp4_thumbnail_batch_rejects_too_many_outputs",
         Box::new(|| {
            runtime
               .block_on(mp4_thumbnails::test_mp4_thumbnail_batch_rejects_too_many_outputs_case());
         }),
      ),
      (
         "test_mp4_frames_rejects_any_timestamp_outside_track_duration",
         Box::new(|| {
            runtime.block_on(
               mp4_thumbnails::test_mp4_frames_rejects_any_timestamp_outside_track_duration_case(),
            );
         }),
      ),
      (
         "test_mp4_thumbnail_rejects_non_h264_video",
         Box::new(|| {
            runtime.block_on(mp4_thumbnails::test_mp4_thumbnail_rejects_non_h264_video_case());
         }),
      ),
      (
         "jpeg_bands_and_quality",
         Box::new(test_cases::bands_and_quality),
      ),
      ("jpeg_output_limit", Box::new(test_cases::output_limit)),
      (
         "jpeg_allocation_failures",
         Box::new(test_cases::allocation_failures),
      ),
      (
         "jpeg_concurrent_encoding",
         Box::new(test_cases::concurrent_encoding),
      ),
   ];
   assert_eq!(cases.len(), 35);
   let mut expected = std::collections::HashSet::new();
   for (name, _) in &cases {
      assert!(expected.insert(*name), "duplicate case {name}");
      println!("EXPECTED {name}");
   }
   let mut executed = std::collections::HashSet::new();
   for (name, run) in cases {
      if std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).is_err() {
         eprintln!("test {name} ... FAILED");
         panic!("case {name} failed");
      }
      assert!(executed.insert(name));
      println!("test {name} ... ok");
   }
   assert_eq!(expected, executed);
   println!("JVM_TESTS={}", executed.len());
}
