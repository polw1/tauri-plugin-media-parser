use super::color::visit_avc_nals;
use super::{AvcConfig, DecodeError};
#[cfg(any(test, apple_videotoolbox_backend))]
use super::{
   MAX_AVC_PARAMETER_SET_BYTES, MAX_AVC_PICTURE_PARAMETER_SETS, MAX_AVC_SEQUENCE_PARAMETER_SETS,
};
#[cfg(any(test, apple_videotoolbox_backend))]
use std::collections::HashSet;

const MAX_ANNEX_B_SAMPLE_BYTES: usize = 64 * 1024 * 1024;
const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// Annex B bytes one NAL contributes once rewritten.
///
/// `annex_b_sample_len` and `append_annex_b_nal` must agree on this to the byte:
/// the former sizes `max_input_size`, the latter produces the buffer that has to
/// fit in it. If they drift, Android accepts the configuration and then fails at
/// decode time with "N bytes exceed capacity M".
fn annex_b_nal_len(nal: &[u8]) -> Result<usize, DecodeError> {
   if nal.is_empty() {
      return Err(DecodeError::Bitstream("empty H.264 NAL unit".to_string()));
   }
   START_CODE
      .len()
      .checked_add(nal.len())
      .ok_or_else(|| DecodeError::Bitstream("H.264 NAL size overflow".to_string()))
}

/// Running Annex B total after one more NAL, held under the decode size limit.
fn extend_annex_b_total(total: usize, additional: usize) -> Result<usize, DecodeError> {
   let total = total
      .checked_add(additional)
      .ok_or_else(|| DecodeError::Bitstream("H.264 sample size overflow".to_string()))?;
   if total > MAX_ANNEX_B_SAMPLE_BYTES {
      return Err(DecodeError::Bitstream(
         "H.264 sample exceeds the decode size limit".to_string(),
      ));
   }
   Ok(total)
}

#[cfg(any(test, not(all(target_os = "android", feature = "android-mediacodec"))))]
pub(crate) fn config_with_max_input_size(
   config: &AvcConfig,
   samples: &[Vec<u8>],
) -> Result<AvcConfig, DecodeError> {
   config_with_max_input_size_and_sps(config, samples, |_| Ok(()))
}

pub(crate) fn config_with_max_input_size_and_sps(
   config: &AvcConfig,
   samples: &[Vec<u8>],
   mut validate_sps: impl FnMut(&[u8]) -> Result<(), DecodeError>,
) -> Result<AvcConfig, DecodeError> {
   for sps in &config.sps {
      validate_sps(sps)?;
   }
   let max_input_size = samples
      .iter()
      .map(|sample| annex_b_sample_len_with_sps(sample, config.length_size, &mut validate_sps))
      .try_fold(None, |largest, size| {
         let size = size?;
         Ok::<_, DecodeError>(Some(
            largest.map_or(size, |largest: usize| largest.max(size)),
         ))
      })?
      .ok_or_else(|| DecodeError::Bitstream("no H.264 samples to decode".to_string()))?;
   let mut prepared = config.clone();
   prepared.max_input_size = Some(max_input_size);
   Ok(prepared)
}

#[cfg(test)]
pub(crate) fn annex_b_sample_len(sample: &[u8], length_size: usize) -> Result<usize, DecodeError> {
   annex_b_sample_len_with_sps(sample, length_size, &mut |_| Ok(()))
}

fn annex_b_sample_len_with_sps(
   sample: &[u8],
   length_size: usize,
   validate_sps: &mut impl FnMut(&[u8]) -> Result<(), DecodeError>,
) -> Result<usize, DecodeError> {
   let mut total = 0usize;
   let mut callback_error = None;
   let result = visit_avc_nals(sample, length_size, |nal| {
      total = annex_b_nal_len(nal)
         .and_then(|additional| extend_annex_b_total(total, additional))
         .map_err(|error| error.to_string())?;
      if nal[0] & 0x1f == 7
         && let Err(error) = validate_sps(nal)
      {
         callback_error = Some(error);
         return Err("H.264 SPS validation failed".to_string());
      }
      Ok(())
   });
   if let Err(message) = result {
      return Err(callback_error.unwrap_or(DecodeError::Bitstream(message)));
   }
   Ok(total)
}

#[cfg(any(test, apple_videotoolbox_backend))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AvcParameterSets {
   pub(crate) sps: Vec<Vec<u8>>,
   pub(crate) pps: Vec<Vec<u8>>,
}

/// Merges parameter sets from the sample entry and one AVCC access unit.
///
/// Traversal deliberately stays in `visit_avc_nals`, so deferred `avc3`
/// initialization cannot disagree with color parsing about AVCC boundaries.
#[cfg(any(test, apple_videotoolbox_backend))]
pub(crate) fn collect_avc_parameter_sets<'a>(
   config: &'a AvcConfig,
   sample: &'a [u8],
) -> Result<AvcParameterSets, DecodeError> {
   fn push_unique<'a>(
      destination: &mut Vec<Vec<u8>>,
      seen: &mut HashSet<&'a [u8]>,
      nal: &'a [u8],
      name: &str,
      count_limit: usize,
      total_bytes: &mut usize,
   ) -> Result<(), DecodeError> {
      if nal.is_empty() {
         return Err(DecodeError::Bitstream(
            "empty H.264 parameter set".to_string(),
         ));
      }
      if seen.contains(nal) {
         return Ok(());
      }
      if destination.len() >= count_limit {
         return Err(DecodeError::ResourceLimit(format!(
            "too many unique H.264 {name} parameter sets"
         )));
      }
      let new_total = total_bytes.checked_add(nal.len()).ok_or_else(|| {
         DecodeError::ResourceLimit("H.264 parameter-set byte count overflow".to_string())
      })?;
      if new_total > MAX_AVC_PARAMETER_SET_BYTES {
         return Err(DecodeError::ResourceLimit(
            "H.264 parameter-set bytes exceed the resource limit".to_string(),
         ));
      }
      seen.try_reserve(1).map_err(|_| {
         DecodeError::ResourceLimit("H.264 parameter-set allocation failed".to_string())
      })?;
      destination.try_reserve(1).map_err(|_| {
         DecodeError::ResourceLimit("H.264 parameter-set allocation failed".to_string())
      })?;
      let mut owned = Vec::new();
      owned.try_reserve_exact(nal.len()).map_err(|_| {
         DecodeError::ResourceLimit("H.264 parameter-set allocation failed".to_string())
      })?;
      owned.extend_from_slice(nal);
      let inserted = seen.insert(nal);
      debug_assert!(inserted);
      destination.push(owned);
      *total_bytes = new_total;
      Ok(())
   }

   let mut parameter_sets = AvcParameterSets {
      sps: Vec::new(),
      pps: Vec::new(),
   };
   let mut seen_sps = HashSet::new();
   let mut seen_pps = HashSet::new();
   let mut total_bytes = 0usize;
   for sps in &config.sps {
      push_unique(
         &mut parameter_sets.sps,
         &mut seen_sps,
         sps,
         "SPS",
         MAX_AVC_SEQUENCE_PARAMETER_SETS,
         &mut total_bytes,
      )?;
   }
   for pps in &config.pps {
      push_unique(
         &mut parameter_sets.pps,
         &mut seen_pps,
         pps,
         "PPS",
         MAX_AVC_PICTURE_PARAMETER_SETS,
         &mut total_bytes,
      )?;
   }

   let mut collector_error = None;
   visit_avc_nals(sample, config.length_size, |nal| {
      let result = match nal.first().map(|header| header & 0x1f) {
         Some(7) => push_unique(
            &mut parameter_sets.sps,
            &mut seen_sps,
            nal,
            "SPS",
            MAX_AVC_SEQUENCE_PARAMETER_SETS,
            &mut total_bytes,
         ),
         Some(8) => push_unique(
            &mut parameter_sets.pps,
            &mut seen_pps,
            nal,
            "PPS",
            MAX_AVC_PICTURE_PARAMETER_SETS,
            &mut total_bytes,
         ),
         _ => Ok(()),
      };
      if let Err(error) = result {
         collector_error = Some(error);
         return Err("H.264 parameter-set collection failed".to_string());
      }
      Ok(())
   })
   .map_err(|message| collector_error.unwrap_or(DecodeError::Bitstream(message)))?;

   if parameter_sets.sps.is_empty() || parameter_sets.pps.is_empty() {
      return Err(DecodeError::Bitstream(
         "initial avc3 sample does not contain both SPS and PPS parameter sets".to_string(),
      ));
   }
   Ok(parameter_sets)
}

#[cfg(any(test, all(target_os = "windows", feature = "windows-media-foundation")))]
pub(crate) fn parameter_sets_annex_b(config: &AvcConfig) -> Result<Vec<u8>, DecodeError> {
   collect_annex_b(config.sps.iter().chain(&config.pps).map(Vec::as_slice))
}

// Mirrors the gate on `mod android` in backend/mod.rs. MediaCodec is the only
// consumer, so its gate mirrors `mod android` in backend/mod.rs.
#[cfg(any(test, all(target_os = "android", feature = "android-mediacodec")))]
pub(crate) fn nals_annex_b(nals: &[Vec<u8>]) -> Result<Vec<u8>, DecodeError> {
   collect_annex_b(nals.iter().map(Vec::as_slice))
}

#[cfg(any(
   test,
   all(target_os = "android", feature = "android-mediacodec"),
   all(target_os = "windows", feature = "windows-media-foundation")
))]
fn collect_annex_b<'a>(nals: impl IntoIterator<Item = &'a [u8]>) -> Result<Vec<u8>, DecodeError> {
   let mut data = Vec::new();
   for parameter_set in nals {
      append_annex_b_nal(&mut data, parameter_set)?;
   }
   Ok(data)
}

/// Rewrites a length-prefixed AVC sample into `output` as Annex B. `output` is
/// cleared first, so callers can reuse one buffer across a whole GOP.
#[cfg(any(test, all(target_os = "windows", feature = "windows-media-foundation")))]
pub(crate) fn sample_to_annex_b(
   sample: &[u8],
   length_size: usize,
   output: &mut Vec<u8>,
) -> Result<(), DecodeError> {
   output.clear();
   output
      .try_reserve(sample.len().saturating_add(4))
      .map_err(|_| DecodeError::ResourceLimit("H.264 sample allocation failed".to_string()))?;
   sample_to_annex_b_into_reserved(sample, length_size, output)
}

#[cfg(any(
   test,
   all(target_os = "android", feature = "android-mediacodec"),
   all(target_os = "windows", feature = "windows-media-foundation")
))]
pub(crate) fn sample_to_annex_b_into_reserved(
   sample: &[u8],
   length_size: usize,
   output: &mut Vec<u8>,
) -> Result<(), DecodeError> {
   output.clear();
   visit_avc_nals(sample, length_size, |nal| {
      append_annex_b_nal(output, nal).map_err(|error| error.to_string())
   })
   .map_err(DecodeError::Bitstream)
}

/// Prefixes an already validated Annex B access unit without exceeding the
/// same checked limit used while rewriting it.
#[cfg(any(test, all(target_os = "windows", feature = "windows-media-foundation")))]
pub(crate) fn prepend_annex_b(output: &mut Vec<u8>, prefix: &[u8]) -> Result<(), DecodeError> {
   if prefix.is_empty() {
      return Ok(());
   }
   let original_len = output.len();
   let combined_len = extend_annex_b_total(original_len, prefix.len())?;
   output
      .try_reserve(prefix.len())
      .map_err(|_| DecodeError::ResourceLimit("H.264 sample allocation failed".to_string()))?;
   output.resize(combined_len, 0);
   output.copy_within(..original_len, prefix.len());
   output[..prefix.len()].copy_from_slice(prefix);
   Ok(())
}

#[cfg(any(
   test,
   all(target_os = "android", feature = "android-mediacodec"),
   all(target_os = "windows", feature = "windows-media-foundation")
))]
fn append_annex_b_nal(output: &mut Vec<u8>, nal: &[u8]) -> Result<(), DecodeError> {
   let additional = annex_b_nal_len(nal)?;
   extend_annex_b_total(output.len(), additional)?;
   output
      .try_reserve(additional)
      .map_err(|_| DecodeError::ResourceLimit("H.264 sample allocation failed".to_string()))?;
   output.extend_from_slice(&START_CODE);
   output.extend_from_slice(nal);
   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::decoders::h264::{AvcColorMetadata, AvcConfig, DecodeError};

   #[test]
   fn rewrites_length_prefixed_samples_and_reuses_the_output() {
      let first = [0, 0, 0, 2, 0x65, 0x88, 0, 0, 0, 1, 0x41];
      let second = [0, 0, 0, 1, 0x06];
      let mut output = Vec::new();

      sample_to_annex_b(&first, 4, &mut output).expect("valid AVC sample");
      let capacity = output.capacity();
      assert_eq!(output, [0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1, 0x41]);

      sample_to_annex_b(&second, 4, &mut output).expect("second AVC sample");
      assert_eq!(output, [0, 0, 0, 1, 0x06]);
      assert_eq!(output.capacity(), capacity);
   }

   #[test]
   fn prefixes_the_first_access_unit_with_checked_parameter_sets() {
      let mut output = vec![0, 0, 0, 1, 0x65, 0x88];
      let parameter_sets = [0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x68, 0xce];

      prepend_annex_b(&mut output, &parameter_sets).expect("combined input fits");

      assert_eq!(
         output,
         [
            0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x68, 0xce, 0, 0, 0, 1, 0x65, 0x88,
         ]
      );

      prepend_annex_b(&mut output, &[]).expect("empty avc3 prefix is a no-op");
      assert_eq!(output.len(), 18);
   }

   /// The pre-flight size and the real rewrite have to agree to the byte:
   /// `max_input_size` is derived from the former, and Android sizes its input
   /// buffer from that, so any drift shows up only at decode time on-device.
   #[test]
   fn the_size_preflight_matches_the_rewritten_sample_exactly() {
      let samples: [(&[u8], usize); 4] = [
         (&[0, 0, 0, 2, 0x65, 0x88, 0, 0, 0, 1, 0x41], 4),
         (&[0, 0, 0, 1, 0x06], 4),
         (&[0, 0, 0, 3, 0x67, 0x42, 0x1e], 4),
         (&[2, 0x65, 0x88, 1, 0x41], 1),
      ];
      let mut output = Vec::new();

      for (sample, length_size) in samples {
         sample_to_annex_b(sample, length_size, &mut output).expect("valid AVC sample");
         assert_eq!(annex_b_sample_len(sample, length_size), Ok(output.len()));
      }
   }

   #[test]
   fn prepares_the_largest_post_conversion_input_size() {
      let config = AvcConfig {
         length_size: 1,
         sps: Vec::new(),
         pps: Vec::new(),
         color: AvcColorMetadata::default(),
         display_width: 2,
         display_height: 2,
         max_input_size: None,
         resolved_full_range: None,
      };
      let samples = vec![vec![1, 0x41], vec![2, 0x65, 0x88, 1, 0x41]];

      let prepared = config_with_max_input_size(&config, &samples).expect("valid samples");

      assert_eq!(prepared.max_input_size, Some(11));
      assert_eq!(config.max_input_size, None);
   }

   #[test]
   fn rejects_empty_nals_as_bitstream_errors() {
      let mut output = vec![0xff; 8];

      assert!(matches!(
         sample_to_annex_b(&[], 4, &mut output),
         Err(DecodeError::Bitstream(_))
      ));
      assert!(output.is_empty());

      assert!(matches!(
         sample_to_annex_b(&[0, 0, 0, 0], 4, &mut output),
         Err(DecodeError::Bitstream(_))
      ));
   }

   #[test]
   fn serializes_parameter_sets_and_allows_empty_avc3_configuration() {
      let config = AvcConfig {
         length_size: 4,
         sps: vec![vec![0x67, 0x42]],
         pps: vec![vec![0x68, 0xce]],
         color: AvcColorMetadata::default(),
         display_width: 2,
         display_height: 2,
         max_input_size: None,
         resolved_full_range: None,
      };
      assert_eq!(
         parameter_sets_annex_b(&config).expect("valid parameter sets"),
         [0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x68, 0xce]
      );

      let avc3 = AvcConfig {
         length_size: 4,
         sps: Vec::new(),
         pps: Vec::new(),
         color: AvcColorMetadata::default(),
         display_width: 2,
         display_height: 2,
         max_input_size: None,
         resolved_full_range: None,
      };
      assert_eq!(parameter_sets_annex_b(&avc3), Ok(Vec::new()));
   }

   #[test]
   fn serializes_parameter_set_groups_for_separate_csd_buffers() {
      assert_eq!(
         nals_annex_b(&[vec![0x67, 0x42], vec![0x67, 0x64]]),
         Ok(vec![0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x67, 0x64,])
      );
      assert_eq!(nals_annex_b(&[]), Ok(Vec::new()));
   }

   fn avcc_sample(length_size: usize, nals: &[&[u8]]) -> Vec<u8> {
      let mut sample = Vec::new();
      for nal in nals {
         let length = nal.len().to_be_bytes();
         sample.extend_from_slice(&length[length.len() - length_size..]);
         sample.extend_from_slice(nal);
      }
      sample
   }

   fn parameter_config(length_size: usize, sps: Vec<Vec<u8>>, pps: Vec<Vec<u8>>) -> AvcConfig {
      AvcConfig {
         length_size,
         sps,
         pps,
         color: AvcColorMetadata::default(),
         display_width: 2,
         display_height: 2,
         max_input_size: None,
         resolved_full_range: None,
      }
   }

   #[test]
   fn fused_preflight_observes_config_and_in_band_sps() {
      let config = parameter_config(2, vec![vec![0x67, 0x11]], Vec::new());
      let sample = avcc_sample(2, &[&[0x67, 0x22], &[0x65, 0x88]]);
      let mut observed = Vec::new();

      let prepared = config_with_max_input_size_and_sps(&config, &[sample], |sps| {
         observed.push(sps.to_vec());
         Ok(())
      })
      .expect("valid fused preflight");

      assert_eq!(observed, [vec![0x67, 0x11], vec![0x67, 0x22]]);
      assert_eq!(prepared.max_input_size, Some(12));
   }

   #[test]
   fn fused_preflight_preserves_callback_error_identity() {
      let config = parameter_config(1, vec![vec![0x67, 0x11]], Vec::new());
      let expected = DecodeError::ResourceLimit("sentinel SPS limit".to_string());

      let error = config_with_max_input_size_and_sps(&config, &[vec![1, 0x65]], |_| {
         Err(DecodeError::ResourceLimit("sentinel SPS limit".to_string()))
      })
      .expect_err("callback failure must escape unchanged");

      assert_eq!(error, expected);
   }

   #[test]
   fn fused_preflight_visits_sps_during_the_size_pass() {
      let config = parameter_config(2, Vec::new(), Vec::new());
      let malformed_after_sps = vec![0, 2, 0x67, 0x42, 0];
      let mut visits = 0;

      let error = config_with_max_input_size_and_sps(&config, &[malformed_after_sps], |_| {
         visits += 1;
         Ok(())
      })
      .expect_err("the trailing length field is truncated");

      assert_eq!(visits, 1);
      assert!(matches!(error, DecodeError::Bitstream(_)));
   }

   #[test]
   fn reserved_annex_b_stays_exact_and_stable_for_all_length_sizes() {
      for length_size in 1..=4 {
         let sample = avcc_sample(length_size, &[&[0x67, 0x42], &[0x65, 0x88, 0x99]]);
         let config = parameter_config(length_size, Vec::new(), Vec::new());
         let mut sps_visits = 0;
         let prepared =
            config_with_max_input_size_and_sps(&config, std::slice::from_ref(&sample), |_| {
               sps_visits += 1;
               Ok(())
            })
            .expect("valid fused preflight");
         let expected = annex_b_sample_len(&sample, length_size).expect("valid size");
         assert_eq!(prepared.max_input_size, Some(expected));
         assert_eq!(sps_visits, 1);

         let mut output = Vec::new();
         output.try_reserve_exact(expected).expect("test allocation");
         let capacity = output.capacity();
         sample_to_annex_b_into_reserved(&sample, length_size, &mut output)
            .expect("reserved rewrite");

         assert_eq!(output.len(), expected);
         assert_eq!(output.capacity(), capacity);
      }
   }

   #[test]
   fn fused_preflight_processes_repeated_sps_without_collecting_them() {
      let sps = [0x67, 0x42];
      let nals = vec![sps.as_slice(); 1_024];
      let sample = avcc_sample(2, &nals);
      let config = parameter_config(2, Vec::new(), Vec::new());
      let mut visits = 0;

      let prepared =
         config_with_max_input_size_and_sps(&config, std::slice::from_ref(&sample), |_| {
            visits += 1;
            Ok(())
         })
         .expect("repeated SPS remain a streaming preflight");

      assert_eq!(visits, 1_024);
      assert_eq!(
         prepared.max_input_size,
         Some(annex_b_sample_len(&sample, 2).unwrap())
      );
   }

   #[test]
   fn collects_and_exactly_deduplicates_out_of_band_and_in_band_parameter_sets() {
      let config = parameter_config(2, vec![vec![0x67, 0x42]], vec![]);
      let sample = avcc_sample(
         2,
         &[
            &[0x67, 0x42],
            &[0x68, 0xce],
            &[0x67, 0x64],
            &[0x68, 0xce],
            &[0x65, 0x88],
         ],
      );

      let sets = collect_avc_parameter_sets(&config, &sample).expect("complete merged sets");

      assert_eq!(sets.sps, [vec![0x67, 0x42], vec![0x67, 0x64]]);
      assert_eq!(sets.pps, [vec![0x68, 0xce]]);
   }

   #[test]
   fn completes_parameter_sets_for_core_media_supported_avcc_length_widths() {
      for length_size in [1, 2, 4] {
         let config = parameter_config(length_size, vec![], vec![vec![0x68, 0xce]]);
         let sample = avcc_sample(length_size, &[&[0x67, 0x42], &[0x68, 0xce]]);

         let sets = collect_avc_parameter_sets(&config, &sample).expect("complete parameter sets");

         assert_eq!(sets.sps, [vec![0x67, 0x42]]);
         assert_eq!(sets.pps, [vec![0x68, 0xce]]);
      }
   }

   #[test]
   fn collector_visitor_accepts_three_byte_avcc_length_fields() {
      let config = parameter_config(3, vec![], vec![vec![0x68, 0xce]]);
      let sample = avcc_sample(3, &[&[0x67, 0x42], &[0x68, 0xce]]);

      let sets = collect_avc_parameter_sets(&config, &sample).expect("complete parameter sets");

      assert_eq!(sets.sps, [vec![0x67, 0x42]]);
      assert_eq!(sets.pps, [vec![0x68, 0xce]]);
   }

   #[test]
   fn rejects_missing_empty_and_truncated_in_band_parameter_sets() {
      let missing_pps = parameter_config(1, vec![], vec![]);
      let sps_only = avcc_sample(1, &[&[0x67, 0x42]]);
      assert!(matches!(
         collect_avc_parameter_sets(&missing_pps, &sps_only),
         Err(DecodeError::Bitstream(message)) if message.contains("SPS and PPS")
      ));

      assert!(matches!(
         collect_avc_parameter_sets(&missing_pps, &[]),
         Err(DecodeError::Bitstream(message)) if message.contains("no NAL")
      ));
      assert!(matches!(
         collect_avc_parameter_sets(&missing_pps, &[2, 0x67]),
         Err(DecodeError::Bitstream(message)) if message.contains("truncated")
      ));
   }

   #[test]
   fn rejects_unique_parameter_sets_above_each_count_limit() {
      let sps_at_limit = (0u8..32).map(|id| vec![0x67, id]).collect();
      let sps_config = parameter_config(2, sps_at_limit, vec![vec![0x68, 0]]);
      let extra_sps = avcc_sample(2, &[&[0x67, 32]]);
      assert!(matches!(
         collect_avc_parameter_sets(&sps_config, &extra_sps),
         Err(DecodeError::ResourceLimit(message)) if message.contains("SPS")
      ));

      let pps_at_limit = (0u16..256)
         .map(|id| vec![0x68, (id >> 8) as u8, id as u8])
         .collect();
      let pps_config = parameter_config(2, vec![vec![0x67, 0]], pps_at_limit);
      let extra_pps = avcc_sample(2, &[&[0x68, 1, 0]]);
      assert!(matches!(
         collect_avc_parameter_sets(&pps_config, &extra_pps),
         Err(DecodeError::ResourceLimit(message)) if message.contains("PPS")
      ));
   }

   #[test]
   fn rejects_unique_parameter_sets_above_the_total_byte_limit() {
      const LIMIT: usize = MAX_AVC_PARAMETER_SET_BYTES;

      let config = parameter_config(4, vec![vec![0x67; LIMIT - 2]], vec![vec![0x68]]);
      let extra_sps = avcc_sample(4, &[&[0x67, 0x42]]);

      assert!(matches!(
         collect_avc_parameter_sets(&config, &extra_sps),
         Err(DecodeError::ResourceLimit(message)) if message.contains("bytes")
      ));
   }

   #[test]
   fn accepts_many_duplicate_parameter_sets_without_counting_them_twice() {
      let sps = [0x67, 0x42];
      let pps = [0x68, 0xce];
      let mut nals = vec![sps.as_slice(); 1024];
      nals.extend(std::iter::repeat_n(pps.as_slice(), 1024));
      let sample = avcc_sample(2, &nals);
      let config = parameter_config(2, Vec::new(), Vec::new());

      let sets = collect_avc_parameter_sets(&config, &sample).expect("duplicates stay bounded");

      assert_eq!(sets.sps, [sps]);
      assert_eq!(sets.pps, [pps]);
   }
}
