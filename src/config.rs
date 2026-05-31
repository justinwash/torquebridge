#![allow(dead_code)]

use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AxisConfig {
    pub id: String,
    pub axis_index: i32,
    pub inverted: i32,
    pub deadzone: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ButtonConfig {
    pub id: serde_json::Value,
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DPadConfig {
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct FfbParamsConfig {
    pub r#const: ConstFfbConfig,
    pub sine: PeriodicFfbConfig,
    pub spring: ConditionFfbConfig,
    pub damper: ConditionFfbConfig,
    #[serde(default)]
    pub calibration: CalibrationConfig,
    #[serde(default)]
    pub experimental: ExperimentalConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ExperimentalConfig {
    #[serde(default)]
    pub traction_loss: TractionLossConfig,
    #[serde(default)]
    pub inferred_dynamics: InferredDynamicsConfig,
}

impl Default for ExperimentalConfig {
    fn default() -> Self {
        Self {
            traction_loss: TractionLossConfig::default(),
            inferred_dynamics: InferredDynamicsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct InferredDynamicsConfig {
    #[serde(default)]
    pub torque_steer: BiasEffectConfig,
    #[serde(default)]
    pub brake_imbalance: BiasEffectConfig,
    #[serde(default)]
    pub understeer_scrub: ScalarEffectConfig,
    #[serde(default)]
    pub rear_lightness: ScalarEffectConfig,
    #[serde(default)]
    pub curb_asymmetry: BiasEffectConfig,
    #[serde(default)]
    pub snap_oversteer: ScalarEffectConfig,
}

impl Default for InferredDynamicsConfig {
    fn default() -> Self {
        Self {
            torque_steer: BiasEffectConfig::default(),
            brake_imbalance: BiasEffectConfig::default(),
            understeer_scrub: ScalarEffectConfig::default(),
            rear_lightness: ScalarEffectConfig::default(),
            curb_asymmetry: BiasEffectConfig::default(),
            snap_oversteer: ScalarEffectConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct BiasEffectConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub strength: f32,
    #[serde(default = "default_slip_threshold")]
    pub trigger_threshold: f32,
}

impl Default for BiasEffectConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 0.0,
            trigger_threshold: default_slip_threshold(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ScalarEffectConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub strength: f32,
    #[serde(default = "default_slip_threshold")]
    pub trigger_threshold: f32,
    #[serde(default = "default_attack_ms")]
    pub attack_ms: u32,
    #[serde(default = "default_recovery_ms")]
    pub recovery_ms: u32,
}

impl Default for ScalarEffectConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 0.0,
            trigger_threshold: default_slip_threshold(),
            attack_ms: default_attack_ms(),
            recovery_ms: default_recovery_ms(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct TractionLossConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_slip_threshold")]
    pub steering_rate_threshold: f32,
    #[serde(default = "default_slip_threshold")]
    pub steering_angle_threshold: f32,
    #[serde(default = "default_slip_threshold")]
    pub force_drop_threshold: f32,
    #[serde(default = "default_output_gain")]
    pub release_strength: f32,
    #[serde(default = "default_attack_ms")]
    pub attack_ms: u32,
    #[serde(default = "default_recovery_ms")]
    pub recovery_ms: u32,
    #[serde(default)]
    pub min_force_floor: f32,
    #[serde(default = "default_apply_true")]
    pub apply_constant: bool,
    #[serde(default = "default_apply_true")]
    pub apply_spring: bool,
    #[serde(default)]
    pub apply_damper: bool,
}

impl Default for TractionLossConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            steering_rate_threshold: default_slip_threshold(),
            steering_angle_threshold: default_slip_threshold(),
            force_drop_threshold: default_slip_threshold(),
            release_strength: default_output_gain(),
            attack_ms: default_attack_ms(),
            recovery_ms: default_recovery_ms(),
            min_force_floor: 0.0,
            apply_constant: true,
            apply_spring: true,
            apply_damper: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct CalibrationConfig {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default = "default_output_gain")]
    pub output_gain: f32,
    #[serde(default)]
    pub steering_center_offset: f32,
    #[serde(default = "default_steering_range")]
    pub steering_range: f32,
    #[serde(default = "default_steering_curve")]
    pub steering_curve: f32,
    #[serde(default = "default_per_effect_gain")]
    pub const_gain: f32,
    #[serde(default = "default_per_effect_gain")]
    pub sine_gain: f32,
    #[serde(default = "default_per_effect_gain")]
    pub spring_gain: f32,
    #[serde(default = "default_per_effect_gain")]
    pub damper_gain: f32,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            preset: None,
            output_gain: default_output_gain(),
            steering_center_offset: 0.0,
            steering_range: default_steering_range(),
            steering_curve: default_steering_curve(),
            const_gain: default_per_effect_gain(),
            sine_gain: default_per_effect_gain(),
            spring_gain: default_per_effect_gain(),
            damper_gain: default_per_effect_gain(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConstFfbConfig {
    pub magnitude: f32,
    pub maximum_force: f32,
    pub minimum_force: f32,
    pub filter_threshold: f32,
    pub minimum_coefficient: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct VibrationConfig {
    pub frequency: f32,
    pub strength: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConditionFfbConfig {
    pub coefficient: f32,
    pub saturation: f32,
}

const fn default_output_gain() -> f32 {
    1.0
}

const fn default_steering_range() -> f32 {
    1.0
}

const fn default_steering_curve() -> f32 {
    1.0
}

const fn default_per_effect_gain() -> f32 {
    1.0
}

const fn default_slip_threshold() -> f32 {
    0.35
}

const fn default_attack_ms() -> u32 {
    120
}

const fn default_recovery_ms() -> u32 {
    450
}

const fn default_apply_true() -> bool {
    true
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
