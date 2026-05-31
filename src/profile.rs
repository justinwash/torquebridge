use crate::config::{ControllerConfig, FfbParamsConfig};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

const CURRENT_PROFILE_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("failed to read profile file {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse profile file {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported profile version {version} in {path}; expected {expected}")]
    UnsupportedVersion {
        path: String,
        version: u32,
        expected: u32,
    },
    #[error("failed to read metadata for profile file {path}: {source}")]
    Metadata {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read modified time for profile file {path}: {source}")]
    ModifiedTime {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize profile file {path}: {source}")]
    Serialize {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to create profile directory for {path}: {source}")]
    CreateDirectory {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write profile file {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FfbProfile {
    #[serde(default = "default_profile_version")]
    pub version: u32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub steering_device: Option<u32>,
    #[serde(default)]
    pub poll_ms: Option<u64>,
    #[serde(default)]
    pub runtime_config_path: Option<String>,
    #[serde(default)]
    pub runtime_controllers: Option<Vec<ControllerConfig>>,
    #[serde(rename = "FFBParameters")]
    pub ffb_parameters: FfbParamsConfig,
}

#[derive(Debug, Clone)]
pub struct ProfileWatcher {
    path: PathBuf,
    last_modified: Option<SystemTime>,
}

impl FfbProfile {
    pub fn from_ffb_settings(ffb_parameters: FfbParamsConfig) -> Self {
        Self {
            version: CURRENT_PROFILE_VERSION,
            name: None,
            notes: None,
            steering_device: None,
            poll_ms: None,
            runtime_config_path: None,
            runtime_controllers: None,
            ffb_parameters,
        }
    }

    pub fn resolved_steering_device(&self, cli_steering_device: Option<u32>) -> Option<u32> {
        cli_steering_device.or(self.steering_device)
    }

    pub fn resolved_poll_ms(&self, cli_poll_ms: Option<u64>, default_poll_ms: u64) -> u64 {
        cli_poll_ms.or(self.poll_ms).unwrap_or(default_poll_ms)
    }
}

impl ProfileWatcher {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, ProfileError> {
        let path = path.into();
        let last_modified = Some(read_modified_time(&path)?);
        Ok(Self {
            path,
            last_modified,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn reload_if_changed(&mut self) -> Result<Option<FfbProfile>, ProfileError> {
        let modified = read_modified_time(&self.path)?;
        if self.last_modified == Some(modified) {
            return Ok(None);
        }

        let profile = load_profile(&self.path)?;
        self.last_modified = Some(modified);
        Ok(Some(profile))
    }
}

pub fn load_profile(path: impl AsRef<Path>) -> Result<FfbProfile, ProfileError> {
    let path_ref = path.as_ref();
    let path_label = path_ref.display().to_string();
    let json = fs::read_to_string(path_ref).map_err(|source| ProfileError::Read {
        path: path_label.clone(),
        source,
    })?;
    let json = json.trim_start_matches('\u{feff}');

    let profile =
        serde_json::from_str::<FfbProfile>(json).map_err(|source| ProfileError::Parse {
            path: path_label.clone(),
            source,
        })?;

    if profile.version != CURRENT_PROFILE_VERSION {
        return Err(ProfileError::UnsupportedVersion {
            path: path_label,
            version: profile.version,
            expected: CURRENT_PROFILE_VERSION,
        });
    }

    Ok(profile)
}

pub fn save_profile(path: impl AsRef<Path>, profile: &FfbProfile) -> Result<(), ProfileError> {
    let path_ref = path.as_ref();
    let path_label = path_ref.display().to_string();

    if let Some(parent) = path_ref.parent() {
        fs::create_dir_all(parent).map_err(|source| ProfileError::CreateDirectory {
            path: path_label.clone(),
            source,
        })?;
    }

    let json = serde_json::to_string_pretty(profile).map_err(|source| ProfileError::Serialize {
        path: path_label.clone(),
        source,
    })?;
    fs::write(path_ref, format!("{json}\n")).map_err(|source| ProfileError::Write {
        path: path_label,
        source,
    })
}

const fn default_profile_version() -> u32 {
    CURRENT_PROFILE_VERSION
}

fn read_modified_time(path: &Path) -> Result<SystemTime, ProfileError> {
    let path_label = path.display().to_string();
    let metadata = fs::metadata(path).map_err(|source| ProfileError::Metadata {
        path: path_label.clone(),
        source,
    })?;
    metadata
        .modified()
        .map_err(|source| ProfileError::ModifiedTime {
            path: path_label,
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        CalibrationConfig, ConditionFfbConfig, ConstFfbConfig, ExperimentalConfig,
        PeriodicFfbConfig, VibrationConfig,
    };
    use std::time::Duration;

    fn settings() -> FfbParamsConfig {
        FfbParamsConfig {
            r#const: ConstFfbConfig {
                magnitude: 1.0,
                maximum_force: 0.92,
                minimum_force: 0.03,
                filter_threshold: 0.18,
                minimum_coefficient: 0.55,
            },
            sine: PeriodicFfbConfig {
                magnitude: 0.75,
                frequency: 1.0,
                maximum_force: 0.75,
                minimum_force: 0.0,
                phase: 0.375,
                engine_vibrations: VibrationConfig {
                    frequency: 1.0,
                    strength: 0.015,
                },
                gear_shift_vibrations: VibrationConfig {
                    frequency: 1.0,
                    strength: 0.06,
                },
            },
            spring: ConditionFfbConfig {
                coefficient: 0.03,
                saturation: 0.0,
            },
            damper: ConditionFfbConfig {
                coefficient: 0.02,
                saturation: 0.0,
            },
            calibration: CalibrationConfig::default(),
            experimental: ExperimentalConfig::default(),
        }
    }

    #[test]
    fn resolved_runtime_fields_prefer_cli_over_profile() {
        let profile = FfbProfile {
            version: CURRENT_PROFILE_VERSION,
            name: Some("baseline".to_string()),
            notes: None,
            steering_device: Some(4),
            poll_ms: Some(8),
            runtime_config_path: Some("configuration.json".to_string()),
            runtime_controllers: None,
            ffb_parameters: settings(),
        };

        assert_eq!(profile.resolved_steering_device(Some(7)), Some(7));
        assert_eq!(profile.resolved_poll_ms(Some(3), 5), 3);
    }

    #[test]
    fn from_ffb_settings_defaults_runtime_fields() {
        let profile = FfbProfile::from_ffb_settings(settings());

        assert_eq!(profile.version, CURRENT_PROFILE_VERSION);
        assert_eq!(profile.steering_device, None);
        assert_eq!(profile.poll_ms, None);
        assert_eq!(profile.runtime_config_path, None);
        assert!(profile.runtime_controllers.is_none());
        assert!((profile.ffb_parameters.r#const.maximum_force - 0.92).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.output_gain - 1.0).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.const_gain - 1.0).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.sine_gain - 1.0).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.spring_gain - 1.0).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.damper_gain - 1.0).abs() < f32::EPSILON);
        assert!(!profile.ffb_parameters.experimental.traction_loss.enabled);
    }

    #[test]
    fn profile_deserialization_defaults_missing_calibration_block() {
        let json = r#"
                {
                    "Version": 1,
                    "Name": "Baseline",
                    "PollMs": 5,
                    "FFBParameters": {
                        "Const": {
                            "Magnitude": 1.0,
                            "MaximumForce": 0.92,
                            "MinimumForce": 0.03,
                            "FilterThreshold": 0.18,
                            "MinimumCoefficient": 0.55
                        },
                        "Sine": {
                            "Magnitude": 0.75,
                            "Frequency": 1.0,
                            "MaximumForce": 0.75,
                            "MinimumForce": 0.0,
                            "Phase": 0.375,
                            "EngineVibrations": {
                                "Frequency": 1.0,
                                "Strength": 0.015
                            },
                            "GearShiftVibrations": {
                                "Frequency": 1.0,
                                "Strength": 0.06
                            }
                        },
                        "Spring": {
                            "Coefficient": 0.03,
                            "Saturation": 0.0
                        },
                        "Damper": {
                            "Coefficient": 0.02,
                            "Saturation": 0.0
                        }
                    }
                }
                "#;

        let profile = serde_json::from_str::<FfbProfile>(json).expect("parse profile json");

        assert!((profile.ffb_parameters.calibration.output_gain - 1.0).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.steering_range - 1.0).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.const_gain - 1.0).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.sine_gain - 1.0).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.spring_gain - 1.0).abs() < f32::EPSILON);
        assert!((profile.ffb_parameters.calibration.damper_gain - 1.0).abs() < f32::EPSILON);
        assert!(!profile.ffb_parameters.experimental.traction_loss.enabled);
    }

    #[test]
    fn watcher_reports_no_change_when_timestamp_matches() {
        let temp_path = std::env::temp_dir().join(format!(
            "Torquebridge-profile-test-{}.json",
            std::process::id()
        ));
        let profile = FfbProfile::from_ffb_settings(settings());
        fs::write(
            &temp_path,
            serde_json::to_vec(&profile).expect("serialize profile"),
        )
        .expect("write profile");

        let mut watcher = ProfileWatcher::new(&temp_path).expect("create watcher");
        assert!(
            watcher
                .reload_if_changed()
                .expect("check watcher")
                .is_none()
        );

        let _ = fs::remove_file(&temp_path);
    }

    #[test]
    fn watcher_reloads_when_file_changes() {
        let temp_path = std::env::temp_dir().join(format!(
            "Torquebridge-profile-reload-test-{}.json",
            std::process::id()
        ));
        let mut profile = FfbProfile::from_ffb_settings(settings());
        fs::write(
            &temp_path,
            serde_json::to_vec(&profile).expect("serialize profile"),
        )
        .expect("write profile");

        let mut watcher = ProfileWatcher::new(&temp_path).expect("create watcher");

        std::thread::sleep(Duration::from_millis(5));
        profile.name = Some("reloaded".to_string());
        profile.version = CURRENT_PROFILE_VERSION;
        fs::write(
            &temp_path,
            serde_json::to_vec(&profile).expect("serialize profile"),
        )
        .expect("rewrite profile");

        let reloaded = watcher
            .reload_if_changed()
            .expect("check watcher")
            .expect("profile reload");
        assert_eq!(reloaded.name.as_deref(), Some("reloaded"));

        let _ = fs::remove_file(&temp_path);
    }
}
