use serde::Serialize;

/// Container format detected from the file
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ContainerFormat {
    MP4,
    M4V,
    ThreeGP,
    ThreeG2,
    MOV,
    MP3,
    Unknown(String),
}

impl ContainerFormat {
    pub fn name(&self) -> &str {
        match self {
            ContainerFormat::MP4 => "MP4",
            ContainerFormat::M4V => "M4V",
            ContainerFormat::ThreeGP => "3GP",
            ContainerFormat::ThreeG2 => "3G2",
            ContainerFormat::MOV => "MOV",
            ContainerFormat::MP3 => "MP3",
            ContainerFormat::Unknown(s) => s,
        }
    }

    pub fn is_mp4_family(&self) -> bool {
        matches!(
            self,
            ContainerFormat::MP4
                | ContainerFormat::M4V
                | ContainerFormat::ThreeGP
                | ContainerFormat::ThreeG2
                | ContainerFormat::MOV
        )
    }
}

/// Basic metadata extracted from a media file
#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Metadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub copyright: Option<String>,
    pub duration: Option<f64>,
    pub size: u64,
    pub format: Option<ContainerFormat>,
}

/// Stream information compatible with FFmpeg format
#[derive(Serialize, Debug)]
pub struct StreamInfo {
    pub index: usize,
    #[serde(rename = "type")]
    pub kind: String,
    pub codec_id: String,
    pub frame_rate: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub channels: Option<u16>,
    pub language: Option<String>,
}

/// Complete metadata with streams information
#[derive(Serialize, Debug)]
pub struct CompleteMetadata {
    pub duration: f64,
    pub title: Option<String>,
    pub streams: Vec<StreamInfo>,
    pub format: ContainerFormat,
}

/// Probe result containing basic file information
#[derive(Serialize, Debug)]
pub struct ProbeResult {
    pub format: ContainerFormat,
    pub size: u64,
    pub is_valid: bool,
    pub error: Option<String>,
}
