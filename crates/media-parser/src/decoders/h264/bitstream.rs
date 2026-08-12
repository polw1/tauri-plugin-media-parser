use super::color::visit_avc_nals;
use super::{AvcConfig, DecodeError};

const MAX_ANNEX_B_SAMPLE_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn config_with_max_input_size(
   config: &AvcConfig,
   samples: &[Vec<u8>],
) -> Result<AvcConfig, DecodeError> {
   let max_input_size = samples
      .iter()
      .map(|sample| annex_b_sample_len(sample, config.length_size))
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

pub(crate) fn annex_b_sample_len(sample: &[u8], length_size: usize) -> Result<usize, DecodeError> {
   let mut total = 0usize;
   visit_avc_nals(sample, length_size, |nal| {
      if nal.is_empty() {
         return Err("empty H.264 NAL unit".to_string());
      }
      total = total
         .checked_add(4)
         .and_then(|total| total.checked_add(nal.len()))
         .filter(|total| *total <= MAX_ANNEX_B_SAMPLE_BYTES)
         .ok_or_else(|| "H.264 sample exceeds the decode size limit".to_string())?;
      Ok(())
   })
   .map_err(DecodeError::Bitstream)?;
   Ok(total)
}

pub(crate) fn parameter_sets_annex_b(config: &AvcConfig) -> Result<Vec<u8>, DecodeError> {
   let mut data = Vec::new();
   for parameter_set in config.sps.iter().chain(&config.pps) {
      append_annex_b_nal(&mut data, parameter_set)?;
   }
   Ok(data)
}

/// Rewrites a length-prefixed AVC sample into `output` as Annex B. `output` is
/// cleared first, so callers can reuse one buffer across a whole GOP.
pub(crate) fn sample_to_annex_b(
   sample: &[u8],
   length_size: usize,
   output: &mut Vec<u8>,
) -> Result<(), DecodeError> {
   output.clear();
   output
      .try_reserve(sample.len().saturating_add(4))
      .map_err(|_| DecodeError::ResourceLimit("H.264 sample allocation failed".to_string()))?;
   visit_avc_nals(sample, length_size, |nal| {
      append_annex_b_nal(output, nal).map_err(|error| error.to_string())
   })
   .map_err(DecodeError::Bitstream)
}

fn append_annex_b_nal(output: &mut Vec<u8>, nal: &[u8]) -> Result<(), DecodeError> {
   if nal.is_empty() {
      return Err(DecodeError::Bitstream("empty H.264 NAL unit".to_string()));
   }
   let additional = 4usize
      .checked_add(nal.len())
      .ok_or_else(|| DecodeError::Bitstream("H.264 NAL size overflow".to_string()))?;
   let total = output
      .len()
      .checked_add(additional)
      .ok_or_else(|| DecodeError::Bitstream("H.264 sample size overflow".to_string()))?;
   if total > MAX_ANNEX_B_SAMPLE_BYTES {
      return Err(DecodeError::Bitstream(
         "H.264 sample exceeds the decode size limit".to_string(),
      ));
   }
   output
      .try_reserve(additional)
      .map_err(|_| DecodeError::ResourceLimit("H.264 sample allocation failed".to_string()))?;
   output.extend_from_slice(&[0, 0, 0, 1]);
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
   fn computes_the_post_conversion_annex_b_length() {
      let sample = [2, 0x65, 0x88, 1, 0x41];

      assert_eq!(annex_b_sample_len(&sample, 1), Ok(11));
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
      };
      let samples = vec![vec![1, 0x41], vec![2, 0x65, 0x88, 1, 0x41]];

      let prepared = config_with_max_input_size(&config, &samples).expect("valid samples");

      assert_eq!(prepared.max_input_size, Some(11));
      assert_eq!(config.max_input_size, None);
   }

   #[test]
   fn rejects_empty_nals_as_bitstream_errors() {
      let mut output = Vec::new();

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
      };
      assert_eq!(parameter_sets_annex_b(&avc3), Ok(Vec::new()));
   }
}
