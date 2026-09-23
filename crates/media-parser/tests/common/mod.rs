#[allow(dead_code)]
pub(crate) mod native_h264;

use std::path::PathBuf;

#[allow(dead_code)]
pub(crate) fn fixtures_dir() -> PathBuf {
   std::env::var_os("MEDIA_PARSER_TEST_FIXTURES")
      .map(PathBuf::from)
      .unwrap_or_else(|| {
         PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
      })
}
