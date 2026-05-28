use crate::config::{ControllerConfig, FfbParamsConfig, load_controllers};
use crate::profile::{FfbProfile, load_profile, save_profile};
use anyhow::{Context, Result, anyhow};
use slint::{ComponentHandle, SharedString};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

slint::include_modules!();

#[derive(Debug, Clone, PartialEq)]
struct ProfileEditorState {
    profile_name: String,
    notes: String,
    poll_ms: f32,
    const_magnitude: f32,
    const_maximum_force: f32,
    const_minimum_force: f32,
    const_filter_threshold: f32,
    const_minimum_coefficient: f32,
    sine_magnitude: f32,
    sine_frequency: f32,
    sine_maximum_force: f32,
    sine_phase: f32,
    engine_vibration_strength: f32,
    gear_shift_strength: f32,
    spring_coefficient: f32,
    spring_saturation: f32,
    damper_coefficient: f32,
    damper_saturation: f32,
}

impl From<&FfbProfile> for ProfileEditorState {
    fn from(profile: &FfbProfile) -> Self {
        Self {
            profile_name: profile
                .name
                .clone()
                .unwrap_or_else(|| "Untitled Profile".to_string()),
            notes: profile.notes.clone().unwrap_or_default(),
            poll_ms: profile.poll_ms.unwrap_or(5) as f32,
            const_magnitude: profile.ffb_parameters.r#const.magnitude,
            const_maximum_force: profile.ffb_parameters.r#const.maximum_force,
            const_minimum_force: profile.ffb_parameters.r#const.minimum_force,
            const_filter_threshold: profile.ffb_parameters.r#const.filter_threshold,
            const_minimum_coefficient: profile.ffb_parameters.r#const.minimum_coefficient,
            sine_magnitude: profile.ffb_parameters.sine.magnitude,
            sine_frequency: profile.ffb_parameters.sine.frequency,
            sine_maximum_force: profile.ffb_parameters.sine.maximum_force,
            sine_phase: profile.ffb_parameters.sine.phase,
            engine_vibration_strength: profile.ffb_parameters.sine.engine_vibrations.strength,
            gear_shift_strength: profile.ffb_parameters.sine.gear_shift_vibrations.strength,
            spring_coefficient: profile.ffb_parameters.spring.coefficient,
            spring_saturation: profile.ffb_parameters.spring.saturation,
            damper_coefficient: profile.ffb_parameters.damper.coefficient,
            damper_saturation: profile.ffb_parameters.damper.saturation,
        }
    }
}

impl ProfileEditorState {
    fn apply_to_window(&self, window: &ProfileEditorWindow, profile_path: &Path) {
        window.set_profile_name(SharedString::from(self.profile_name.clone()));
        window.set_notes(SharedString::from(self.notes.clone()));
        window.set_profile_path(SharedString::from(profile_path.display().to_string()));
        window.set_poll_ms(self.poll_ms);
        window.set_const_magnitude(self.const_magnitude);
        window.set_const_maximum_force(self.const_maximum_force);
        window.set_const_minimum_force(self.const_minimum_force);
        window.set_const_filter_threshold(self.const_filter_threshold);
        window.set_const_minimum_coefficient(self.const_minimum_coefficient);
        window.set_sine_magnitude(self.sine_magnitude);
        window.set_sine_frequency(self.sine_frequency);
        window.set_sine_maximum_force(self.sine_maximum_force);
        window.set_sine_phase(self.sine_phase);
        window.set_engine_vibration_strength(self.engine_vibration_strength);
        window.set_gear_shift_strength(self.gear_shift_strength);
        window.set_spring_coefficient(self.spring_coefficient);
        window.set_spring_saturation(self.spring_saturation);
        window.set_damper_coefficient(self.damper_coefficient);
        window.set_damper_saturation(self.damper_saturation);
    }

    fn from_window(window: &ProfileEditorWindow) -> Self {
        Self {
            profile_name: window.get_profile_name().to_string(),
            notes: window.get_notes().to_string(),
            poll_ms: window.get_poll_ms(),
            const_magnitude: window.get_const_magnitude(),
            const_maximum_force: window.get_const_maximum_force(),
            const_minimum_force: window.get_const_minimum_force(),
            const_filter_threshold: window.get_const_filter_threshold(),
            const_minimum_coefficient: window.get_const_minimum_coefficient(),
            sine_magnitude: window.get_sine_magnitude(),
            sine_frequency: window.get_sine_frequency(),
            sine_maximum_force: window.get_sine_maximum_force(),
            sine_phase: window.get_sine_phase(),
            engine_vibration_strength: window.get_engine_vibration_strength(),
            gear_shift_strength: window.get_gear_shift_strength(),
            spring_coefficient: window.get_spring_coefficient(),
            spring_saturation: window.get_spring_saturation(),
            damper_coefficient: window.get_damper_coefficient(),
            damper_saturation: window.get_damper_saturation(),
        }
    }

    fn into_profile(self, base: &FfbProfile) -> FfbProfile {
        let mut profile = base.clone();
        profile.name = if self.profile_name.trim().is_empty() {
            None
        } else {
            Some(self.profile_name.trim().to_string())
        };
        profile.notes = if self.notes.trim().is_empty() {
            None
        } else {
            Some(self.notes.trim().to_string())
        };
        profile.poll_ms = Some(self.poll_ms.round().clamp(1.0, 20.0) as u64);
        profile.ffb_parameters.r#const.magnitude = self.const_magnitude.clamp(0.0, 2.0);
        profile.ffb_parameters.r#const.maximum_force = self.const_maximum_force.clamp(0.0, 1.0);
        profile.ffb_parameters.r#const.minimum_force = self.const_minimum_force.clamp(0.0, 0.25);
        profile.ffb_parameters.r#const.filter_threshold =
            self.const_filter_threshold.clamp(0.0, 1.0);
        profile.ffb_parameters.r#const.minimum_coefficient =
            self.const_minimum_coefficient.clamp(0.0, 1.0);
        profile.ffb_parameters.sine.magnitude = self.sine_magnitude.clamp(0.0, 1.5);
        profile.ffb_parameters.sine.frequency = self.sine_frequency.clamp(0.25, 4.0);
        profile.ffb_parameters.sine.maximum_force = self.sine_maximum_force.clamp(0.0, 1.0);
        profile.ffb_parameters.sine.phase = self.sine_phase.clamp(0.0, 1.0);
        profile.ffb_parameters.sine.engine_vibrations.strength =
            self.engine_vibration_strength.clamp(0.0, 0.25);
        profile.ffb_parameters.sine.gear_shift_vibrations.strength =
            self.gear_shift_strength.clamp(0.0, 0.25);
        profile.ffb_parameters.spring.coefficient = self.spring_coefficient.clamp(0.0, 0.25);
        profile.ffb_parameters.spring.saturation = self.spring_saturation.clamp(0.0, 1.0);
        profile.ffb_parameters.damper.coefficient = self.damper_coefficient.clamp(0.0, 0.25);
        profile.ffb_parameters.damper.saturation = self.damper_saturation.clamp(0.0, 1.0);
        profile
    }
}

pub fn run_profile_editor(config_path: &str, profile_path: Option<&str>) -> Result<()> {
    let (target_path, profile) = load_profile_for_editor(config_path, profile_path)?;
    let window = ProfileEditorWindow::new().context("failed to create Slint profile editor")?;
    let profile_state = Rc::new(RefCell::new(profile));
    let path = Rc::new(target_path);

    ProfileEditorState::from(&*profile_state.borrow()).apply_to_window(&window, path.as_path());
    window.set_status_message(SharedString::from(
        "Saving writes the profile file immediately. The running bridge will hot reload it.",
    ));

    let weak_window = window.as_weak();
    let profile_state_handle = Rc::clone(&profile_state);
    let path_handle = Rc::clone(&path);
    window.on_save_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let updated = {
            let current = profile_state_handle.borrow();
            ProfileEditorState::from_window(&window).into_profile(&current)
        };

        match save_profile(path_handle.as_path(), &updated) {
            Ok(()) => {
                *profile_state_handle.borrow_mut() = updated;
                window.set_status_message(SharedString::from(
                    "Profile saved. ffb-bridge will apply the changes automatically.",
                ));
            }
            Err(error) => {
                window.set_status_message(SharedString::from(format!("Save failed: {error}")));
            }
        }
    });

    window.run().context("profile editor exited with an error")
}

fn load_profile_for_editor(
    config_path: &str,
    profile_path: Option<&str>,
) -> Result<(PathBuf, FfbProfile)> {
    let target_path = profile_path.map(PathBuf::from).unwrap_or(
        std::env::current_dir()
            .context("failed to resolve current working directory")?
            .join("profiles")
            .join("baseline.json"),
    );

    let profile = if target_path.exists() {
        load_profile(&target_path)?
    } else {
        let controllers = load_controllers(config_path)?;
        FfbProfile::from_ffb_settings(load_ffb_settings(&controllers)?)
    };

    Ok((target_path, profile))
}

fn load_ffb_settings(controllers: &[ControllerConfig]) -> Result<FfbParamsConfig> {
    controllers
        .iter()
        .find_map(|controller| controller.ffb_parameters.clone())
        .ok_or_else(|| anyhow!("no FFBParameters entry found in config"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConditionFfbConfig, ConstFfbConfig, PeriodicFfbConfig, VibrationConfig};

    fn profile() -> FfbProfile {
        FfbProfile {
            version: 1,
            name: Some("Baseline Compatibility Bridge".to_string()),
            notes: Some("Hot-reload tuned profile".to_string()),
            steering_device: Some(0),
            poll_ms: Some(5),
            ffb_parameters: FfbParamsConfig {
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
            },
        }
    }

    #[test]
    fn editor_state_round_trips_profile_values() {
        let original = profile();
        let state = ProfileEditorState::from(&original);
        let restored = state.into_profile(&original);

        assert_eq!(restored.name, original.name);
        assert_eq!(restored.notes, original.notes);
        assert_eq!(restored.poll_ms, original.poll_ms);
        assert!((restored.ffb_parameters.r#const.maximum_force - 0.92).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.sine.phase - 0.375).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.spring.coefficient - 0.03).abs() < f32::EPSILON);
    }

    #[test]
    fn editor_state_clamps_out_of_range_values() {
        let original = profile();
        let mut state = ProfileEditorState::from(&original);
        state.poll_ms = 99.0;
        state.const_magnitude = 4.0;
        state.damper_saturation = -2.0;

        let restored = state.into_profile(&original);
        assert_eq!(restored.poll_ms, Some(20));
        assert_eq!(restored.ffb_parameters.r#const.magnitude, 2.0);
        assert_eq!(restored.ffb_parameters.damper.saturation, 0.0);
    }
}
