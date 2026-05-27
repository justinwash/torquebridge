#![allow(dead_code)]

use serde::Deserialize;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ControllerConfig {
    pub product_guid: String,
    pub instance_guid: String,
    pub instance_name: String,
    pub product_name: String,
    pub axes: Option<Vec<AxisConfig>>,
    pub buttons: Option<Vec<ButtonConfig>>,
    #[serde(rename = "DPad")]
    pub d_pad: Option<DPadConfig>,
    #[serde(rename = "FFBParameters")]
    pub ffb_parameters: Option<FfbParamsConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AxisConfig {
    pub id: String,
    pub axis_index: i32,
    pub inverted: i32,
    pub deadzone: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ButtonConfig {
    pub id: serde_json::Value,
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DPadConfig {
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FfbParamsConfig {
    pub r#const: ConstFfbConfig,
    pub sine: PeriodicFfbConfig,
    pub spring: ConditionFfbConfig,
    pub damper: ConditionFfbConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConstFfbConfig {
    pub magnitude: f32,
    pub maximum_force: f32,
    pub minimum_force: f32,
    pub filter_threshold: f32,
    pub minimum_coefficient: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PeriodicFfbConfig {
    pub magnitude: f32,
    pub frequency: f32,
    pub maximum_force: f32,
    pub minimum_force: f32,
    pub phase: f32,
    pub engine_vibrations: VibrationConfig,
    pub gear_shift_vibrations: VibrationConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct VibrationConfig {
    pub frequency: f32,
    pub strength: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConditionFfbConfig {
    pub coefficient: f32,
    pub saturation: f32,
}

pub fn load_controllers(path: impl AsRef<Path>) -> Result<Vec<ControllerConfig>, ConfigError> {
    let path_ref = path.as_ref();
    let path_label = path_ref.display().to_string();
    let json = fs::read_to_string(path_ref).map_err(|source| ConfigError::Read {
        path: path_label.clone(),
        source,
    })?;
    let json = json.trim_start_matches('\u{feff}');

    serde_json::from_str::<Vec<ControllerConfig>>(&json).map_err(|source| ConfigError::Parse {
        path: path_label,
        source,
    })
}
