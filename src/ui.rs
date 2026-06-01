use crate::backends::directinput::DirectInput;
use crate::config::{ControllerConfig, FfbParamsConfig, load_controllers};
use crate::core::domain::{DeviceControlCommand, WheelCommand};
use crate::diagnostics::{
    BridgeDiagnosticEvent, DiagnosticCategory, DiagnosticField, DiagnosticLevel, DiagnosticsLog,
    format_event,
};
use crate::inputs::{WinmmDeviceInfo, WinmmJoystick};
use crate::profile::{FfbProfile, load_profile, save_profile};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use slint::{
    CloseRequestResponse, ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel,
};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

slint::include_modules!();

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const DIAGNOSTIC_EVENT_LIMIT: usize = 48;
const OBSERVABILITY_EVENT_LIMIT: usize = 128;
const AUTO_CALIBRATION_SWEEP_INTERVAL_MS: u64 = 200;
const AUTO_CALIBRATION_SWEEP_SAMPLE_TARGET: usize = 20;
const DEFAULT_VJOY_DEVICE_ID: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
struct ProfileEditorState {
    profile_name: String,
    notes: String,
    poll_ms: f32,
    steering_device: f32,
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
    calibration_preset_label: String,
    calibration_output_gain: f32,
    calibration_const_gain: f32,
    calibration_sine_gain: f32,
    calibration_spring_gain: f32,
    calibration_damper_gain: f32,
    calibration_steering_center_offset: f32,
    calibration_steering_range: f32,
    calibration_steering_curve: f32,
    experimental_slip_enabled: bool,
    experimental_slip_steering_rate_threshold: f32,
    experimental_slip_steering_angle_threshold: f32,
    experimental_slip_force_drop_threshold: f32,
    experimental_slip_release_strength: f32,
    experimental_slip_attack_ms: f32,
    experimental_slip_recovery_ms: f32,
    experimental_slip_min_force_floor: f32,
    experimental_slip_apply_constant: bool,
    experimental_slip_apply_spring: bool,
    experimental_slip_apply_damper: bool,
    experimental_torque_steer_enabled: bool,
    experimental_torque_steer_strength: f32,
    experimental_torque_steer_threshold: f32,
    experimental_brake_imbalance_enabled: bool,
    experimental_brake_imbalance_strength: f32,
    experimental_brake_imbalance_threshold: f32,
    experimental_understeer_scrub_enabled: bool,
    experimental_understeer_scrub_strength: f32,
    experimental_understeer_scrub_threshold: f32,
    experimental_understeer_scrub_attack_ms: f32,
    experimental_understeer_scrub_recovery_ms: f32,
    experimental_rear_lightness_enabled: bool,
    experimental_rear_lightness_strength: f32,
    experimental_rear_lightness_threshold: f32,
    experimental_rear_lightness_attack_ms: f32,
    experimental_rear_lightness_recovery_ms: f32,
    experimental_curb_asymmetry_enabled: bool,
    experimental_curb_asymmetry_strength: f32,
    experimental_curb_asymmetry_threshold: f32,
    experimental_snap_oversteer_enabled: bool,
    experimental_snap_oversteer_strength: f32,
    experimental_snap_oversteer_threshold: f32,
    experimental_snap_oversteer_attack_ms: f32,
    experimental_snap_oversteer_recovery_ms: f32,
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
            steering_device: profile
                .steering_device
                .map(|value| value as f32)
                .unwrap_or(-1.0),
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
            calibration_preset_label: profile
                .ffb_parameters
                .calibration
                .preset
                .clone()
                .unwrap_or_else(|| "Custom".to_string()),
            calibration_output_gain: profile.ffb_parameters.calibration.output_gain,
            calibration_const_gain: profile.ffb_parameters.calibration.const_gain,
            calibration_sine_gain: profile.ffb_parameters.calibration.sine_gain,
            calibration_spring_gain: profile.ffb_parameters.calibration.spring_gain,
            calibration_damper_gain: profile.ffb_parameters.calibration.damper_gain,
            calibration_steering_center_offset: profile
                .ffb_parameters
                .calibration
                .steering_center_offset,
            calibration_steering_range: profile.ffb_parameters.calibration.steering_range,
            calibration_steering_curve: profile.ffb_parameters.calibration.steering_curve,
            experimental_slip_enabled: profile.ffb_parameters.experimental.traction_loss.enabled,
            experimental_slip_steering_rate_threshold: profile
                .ffb_parameters
                .experimental
                .traction_loss
                .steering_rate_threshold,
            experimental_slip_steering_angle_threshold: profile
                .ffb_parameters
                .experimental
                .traction_loss
                .steering_angle_threshold,
            experimental_slip_force_drop_threshold: profile
                .ffb_parameters
                .experimental
                .traction_loss
                .force_drop_threshold,
            experimental_slip_release_strength: profile
                .ffb_parameters
                .experimental
                .traction_loss
                .release_strength,
            experimental_slip_attack_ms: profile.ffb_parameters.experimental.traction_loss.attack_ms
                as f32,
            experimental_slip_recovery_ms: profile
                .ffb_parameters
                .experimental
                .traction_loss
                .recovery_ms as f32,
            experimental_slip_min_force_floor: profile
                .ffb_parameters
                .experimental
                .traction_loss
                .min_force_floor,
            experimental_slip_apply_constant: profile
                .ffb_parameters
                .experimental
                .traction_loss
                .apply_constant,
            experimental_slip_apply_spring: profile
                .ffb_parameters
                .experimental
                .traction_loss
                .apply_spring,
            experimental_slip_apply_damper: profile
                .ffb_parameters
                .experimental
                .traction_loss
                .apply_damper,
            experimental_torque_steer_enabled: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .torque_steer
                .enabled,
            experimental_torque_steer_strength: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .torque_steer
                .strength,
            experimental_torque_steer_threshold: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .torque_steer
                .trigger_threshold,
            experimental_brake_imbalance_enabled: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .brake_imbalance
                .enabled,
            experimental_brake_imbalance_strength: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .brake_imbalance
                .strength,
            experimental_brake_imbalance_threshold: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .brake_imbalance
                .trigger_threshold,
            experimental_understeer_scrub_enabled: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .understeer_scrub
                .enabled,
            experimental_understeer_scrub_strength: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .understeer_scrub
                .strength,
            experimental_understeer_scrub_threshold: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .understeer_scrub
                .trigger_threshold,
            experimental_understeer_scrub_attack_ms: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .understeer_scrub
                .attack_ms as f32,
            experimental_understeer_scrub_recovery_ms: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .understeer_scrub
                .recovery_ms as f32,
            experimental_rear_lightness_enabled: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .rear_lightness
                .enabled,
            experimental_rear_lightness_strength: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .rear_lightness
                .strength,
            experimental_rear_lightness_threshold: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .rear_lightness
                .trigger_threshold,
            experimental_rear_lightness_attack_ms: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .rear_lightness
                .attack_ms as f32,
            experimental_rear_lightness_recovery_ms: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .rear_lightness
                .recovery_ms as f32,
            experimental_curb_asymmetry_enabled: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .curb_asymmetry
                .enabled,
            experimental_curb_asymmetry_strength: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .curb_asymmetry
                .strength,
            experimental_curb_asymmetry_threshold: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .curb_asymmetry
                .trigger_threshold,
            experimental_snap_oversteer_enabled: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .snap_oversteer
                .enabled,
            experimental_snap_oversteer_strength: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .snap_oversteer
                .strength,
            experimental_snap_oversteer_threshold: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .snap_oversteer
                .trigger_threshold,
            experimental_snap_oversteer_attack_ms: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .snap_oversteer
                .attack_ms as f32,
            experimental_snap_oversteer_recovery_ms: profile
                .ffb_parameters
                .experimental
                .inferred_dynamics
                .snap_oversteer
                .recovery_ms as f32,
        }
    }
}

impl ProfileEditorState {
    fn apply_to_window(&self, window: &ProfileEditorWindow, profile_path: &Path) {
        window.set_profile_name(SharedString::from(self.profile_name.clone()));
        window.set_notes(SharedString::from(self.notes.clone()));
        window.set_profile_path(SharedString::from(profile_path.display().to_string()));
        window.set_poll_ms(self.poll_ms);
        window.set_steering_device(self.steering_device);
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
        window.set_calibration_preset_label(SharedString::from(
            self.calibration_preset_label.clone(),
        ));
        window.set_calibration_output_gain(self.calibration_output_gain);
        window.set_calibration_const_gain(self.calibration_const_gain);
        window.set_calibration_sine_gain(self.calibration_sine_gain);
        window.set_calibration_spring_gain(self.calibration_spring_gain);
        window.set_calibration_damper_gain(self.calibration_damper_gain);
        window.set_calibration_steering_center_offset(self.calibration_steering_center_offset);
        window.set_calibration_steering_range(self.calibration_steering_range);
        window.set_calibration_steering_curve(self.calibration_steering_curve);
        window.set_experimental_slip_enabled(self.experimental_slip_enabled);
        window.set_experimental_slip_steering_rate_threshold(
            self.experimental_slip_steering_rate_threshold,
        );
        window.set_experimental_slip_steering_angle_threshold(
            self.experimental_slip_steering_angle_threshold,
        );
        window.set_experimental_slip_force_drop_threshold(
            self.experimental_slip_force_drop_threshold,
        );
        window.set_experimental_slip_release_strength(self.experimental_slip_release_strength);
        window.set_experimental_slip_attack_ms(self.experimental_slip_attack_ms);
        window.set_experimental_slip_recovery_ms(self.experimental_slip_recovery_ms);
        window.set_experimental_slip_min_force_floor(self.experimental_slip_min_force_floor);
        window.set_experimental_slip_apply_constant(self.experimental_slip_apply_constant);
        window.set_experimental_slip_apply_spring(self.experimental_slip_apply_spring);
        window.set_experimental_slip_apply_damper(self.experimental_slip_apply_damper);
        window.set_experimental_torque_steer_enabled(self.experimental_torque_steer_enabled);
        window.set_experimental_torque_steer_strength(self.experimental_torque_steer_strength);
        window.set_experimental_torque_steer_threshold(self.experimental_torque_steer_threshold);
        window.set_experimental_brake_imbalance_enabled(self.experimental_brake_imbalance_enabled);
        window
            .set_experimental_brake_imbalance_strength(self.experimental_brake_imbalance_strength);
        window.set_experimental_brake_imbalance_threshold(
            self.experimental_brake_imbalance_threshold,
        );
        window
            .set_experimental_understeer_scrub_enabled(self.experimental_understeer_scrub_enabled);
        window.set_experimental_understeer_scrub_strength(
            self.experimental_understeer_scrub_strength,
        );
        window.set_experimental_understeer_scrub_threshold(
            self.experimental_understeer_scrub_threshold,
        );
        window.set_experimental_understeer_scrub_attack_ms(
            self.experimental_understeer_scrub_attack_ms,
        );
        window.set_experimental_understeer_scrub_recovery_ms(
            self.experimental_understeer_scrub_recovery_ms,
        );
        window.set_experimental_rear_lightness_enabled(self.experimental_rear_lightness_enabled);
        window.set_experimental_rear_lightness_strength(self.experimental_rear_lightness_strength);
        window
            .set_experimental_rear_lightness_threshold(self.experimental_rear_lightness_threshold);
        window
            .set_experimental_rear_lightness_attack_ms(self.experimental_rear_lightness_attack_ms);
        window.set_experimental_rear_lightness_recovery_ms(
            self.experimental_rear_lightness_recovery_ms,
        );
        window.set_experimental_curb_asymmetry_enabled(self.experimental_curb_asymmetry_enabled);
        window.set_experimental_curb_asymmetry_strength(self.experimental_curb_asymmetry_strength);
        window
            .set_experimental_curb_asymmetry_threshold(self.experimental_curb_asymmetry_threshold);
        window.set_experimental_snap_oversteer_enabled(self.experimental_snap_oversteer_enabled);
        window.set_experimental_snap_oversteer_strength(self.experimental_snap_oversteer_strength);
        window
            .set_experimental_snap_oversteer_threshold(self.experimental_snap_oversteer_threshold);
        window
            .set_experimental_snap_oversteer_attack_ms(self.experimental_snap_oversteer_attack_ms);
        window.set_experimental_snap_oversteer_recovery_ms(
            self.experimental_snap_oversteer_recovery_ms,
        );
    }

    fn from_window(window: &ProfileEditorWindow) -> Self {
        Self {
            profile_name: window.get_profile_name().to_string(),
            notes: window.get_notes().to_string(),
            poll_ms: window.get_poll_ms(),
            steering_device: window.get_steering_device(),
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
            calibration_preset_label: window.get_calibration_preset_label().to_string(),
            calibration_output_gain: window.get_calibration_output_gain(),
            calibration_const_gain: window.get_calibration_const_gain(),
            calibration_sine_gain: window.get_calibration_sine_gain(),
            calibration_spring_gain: window.get_calibration_spring_gain(),
            calibration_damper_gain: window.get_calibration_damper_gain(),
            calibration_steering_center_offset: window.get_calibration_steering_center_offset(),
            calibration_steering_range: window.get_calibration_steering_range(),
            calibration_steering_curve: window.get_calibration_steering_curve(),
            experimental_slip_enabled: window.get_experimental_slip_enabled(),
            experimental_slip_steering_rate_threshold: window
                .get_experimental_slip_steering_rate_threshold(),
            experimental_slip_steering_angle_threshold: window
                .get_experimental_slip_steering_angle_threshold(),
            experimental_slip_force_drop_threshold: window
                .get_experimental_slip_force_drop_threshold(),
            experimental_slip_release_strength: window.get_experimental_slip_release_strength(),
            experimental_slip_attack_ms: window.get_experimental_slip_attack_ms(),
            experimental_slip_recovery_ms: window.get_experimental_slip_recovery_ms(),
            experimental_slip_min_force_floor: window.get_experimental_slip_min_force_floor(),
            experimental_slip_apply_constant: window.get_experimental_slip_apply_constant(),
            experimental_slip_apply_spring: window.get_experimental_slip_apply_spring(),
            experimental_slip_apply_damper: window.get_experimental_slip_apply_damper(),
            experimental_torque_steer_enabled: window.get_experimental_torque_steer_enabled(),
            experimental_torque_steer_strength: window.get_experimental_torque_steer_strength(),
            experimental_torque_steer_threshold: window.get_experimental_torque_steer_threshold(),
            experimental_brake_imbalance_enabled: window.get_experimental_brake_imbalance_enabled(),
            experimental_brake_imbalance_strength: window
                .get_experimental_brake_imbalance_strength(),
            experimental_brake_imbalance_threshold: window
                .get_experimental_brake_imbalance_threshold(),
            experimental_understeer_scrub_enabled: window
                .get_experimental_understeer_scrub_enabled(),
            experimental_understeer_scrub_strength: window
                .get_experimental_understeer_scrub_strength(),
            experimental_understeer_scrub_threshold: window
                .get_experimental_understeer_scrub_threshold(),
            experimental_understeer_scrub_attack_ms: window
                .get_experimental_understeer_scrub_attack_ms(),
            experimental_understeer_scrub_recovery_ms: window
                .get_experimental_understeer_scrub_recovery_ms(),
            experimental_rear_lightness_enabled: window.get_experimental_rear_lightness_enabled(),
            experimental_rear_lightness_strength: window.get_experimental_rear_lightness_strength(),
            experimental_rear_lightness_threshold: window
                .get_experimental_rear_lightness_threshold(),
            experimental_rear_lightness_attack_ms: window
                .get_experimental_rear_lightness_attack_ms(),
            experimental_rear_lightness_recovery_ms: window
                .get_experimental_rear_lightness_recovery_ms(),
            experimental_curb_asymmetry_enabled: window.get_experimental_curb_asymmetry_enabled(),
            experimental_curb_asymmetry_strength: window.get_experimental_curb_asymmetry_strength(),
            experimental_curb_asymmetry_threshold: window
                .get_experimental_curb_asymmetry_threshold(),
            experimental_snap_oversteer_enabled: window.get_experimental_snap_oversteer_enabled(),
            experimental_snap_oversteer_strength: window.get_experimental_snap_oversteer_strength(),
            experimental_snap_oversteer_threshold: window
                .get_experimental_snap_oversteer_threshold(),
            experimental_snap_oversteer_attack_ms: window
                .get_experimental_snap_oversteer_attack_ms(),
            experimental_snap_oversteer_recovery_ms: window
                .get_experimental_snap_oversteer_recovery_ms(),
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
        profile.runtime_config_path = None;
        profile.poll_ms = Some(self.poll_ms.round().clamp(1.0, 20.0) as u64);
        profile.steering_device = if self.steering_device < 0.0 {
            None
        } else {
            Some(self.steering_device.round().clamp(0.0, 32.0) as u32)
        };
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
        let preset_label = self.calibration_preset_label.trim();
        profile.ffb_parameters.calibration.preset =
            if preset_label.is_empty() || preset_label.eq_ignore_ascii_case("custom") {
                None
            } else {
                Some(preset_label.to_string())
            };
        profile.ffb_parameters.calibration.output_gain =
            self.calibration_output_gain.clamp(0.0, 2.0);
        profile.ffb_parameters.calibration.const_gain = self.calibration_const_gain.clamp(0.0, 2.0);
        profile.ffb_parameters.calibration.sine_gain = self.calibration_sine_gain.clamp(0.0, 2.0);
        profile.ffb_parameters.calibration.spring_gain =
            self.calibration_spring_gain.clamp(0.0, 2.0);
        profile.ffb_parameters.calibration.damper_gain =
            self.calibration_damper_gain.clamp(0.0, 2.0);
        profile.ffb_parameters.calibration.steering_center_offset =
            self.calibration_steering_center_offset.clamp(-0.5, 0.5);
        profile.ffb_parameters.calibration.steering_range =
            self.calibration_steering_range.clamp(0.25, 2.0);
        profile.ffb_parameters.calibration.steering_curve =
            self.calibration_steering_curve.clamp(0.25, 3.0);
        profile.ffb_parameters.experimental.traction_loss.enabled = self.experimental_slip_enabled;
        profile
            .ffb_parameters
            .experimental
            .traction_loss
            .steering_rate_threshold = self
            .experimental_slip_steering_rate_threshold
            .clamp(0.0, 8.0);
        profile
            .ffb_parameters
            .experimental
            .traction_loss
            .steering_angle_threshold = self
            .experimental_slip_steering_angle_threshold
            .clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .traction_loss
            .force_drop_threshold = self.experimental_slip_force_drop_threshold.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .traction_loss
            .release_strength = self.experimental_slip_release_strength.clamp(0.0, 1.0);
        profile.ffb_parameters.experimental.traction_loss.attack_ms =
            self.experimental_slip_attack_ms
                .round()
                .clamp(10.0, 2_000.0) as u32;
        profile
            .ffb_parameters
            .experimental
            .traction_loss
            .recovery_ms = self
            .experimental_slip_recovery_ms
            .round()
            .clamp(10.0, 4_000.0) as u32;
        profile
            .ffb_parameters
            .experimental
            .traction_loss
            .min_force_floor = self.experimental_slip_min_force_floor.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .traction_loss
            .apply_constant = self.experimental_slip_apply_constant;
        profile
            .ffb_parameters
            .experimental
            .traction_loss
            .apply_spring = self.experimental_slip_apply_spring;
        profile
            .ffb_parameters
            .experimental
            .traction_loss
            .apply_damper = self.experimental_slip_apply_damper;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .torque_steer
            .enabled = self.experimental_torque_steer_enabled;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .torque_steer
            .strength = self.experimental_torque_steer_strength.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .torque_steer
            .trigger_threshold = self.experimental_torque_steer_threshold.clamp(0.0, 8.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .brake_imbalance
            .enabled = self.experimental_brake_imbalance_enabled;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .brake_imbalance
            .strength = self.experimental_brake_imbalance_strength.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .brake_imbalance
            .trigger_threshold = self.experimental_brake_imbalance_threshold.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .enabled = self.experimental_understeer_scrub_enabled;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .strength = self.experimental_understeer_scrub_strength.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .trigger_threshold = self.experimental_understeer_scrub_threshold.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .attack_ms = self
            .experimental_understeer_scrub_attack_ms
            .round()
            .clamp(10.0, 2_000.0) as u32;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .recovery_ms = self
            .experimental_understeer_scrub_recovery_ms
            .round()
            .clamp(10.0, 4_000.0) as u32;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .rear_lightness
            .enabled = self.experimental_rear_lightness_enabled;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .rear_lightness
            .strength = self.experimental_rear_lightness_strength.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .rear_lightness
            .trigger_threshold = self.experimental_rear_lightness_threshold.clamp(0.0, 8.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .rear_lightness
            .attack_ms = self
            .experimental_rear_lightness_attack_ms
            .round()
            .clamp(10.0, 2_000.0) as u32;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .rear_lightness
            .recovery_ms = self
            .experimental_rear_lightness_recovery_ms
            .round()
            .clamp(10.0, 4_000.0) as u32;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .curb_asymmetry
            .enabled = self.experimental_curb_asymmetry_enabled;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .curb_asymmetry
            .strength = self.experimental_curb_asymmetry_strength.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .curb_asymmetry
            .trigger_threshold = self.experimental_curb_asymmetry_threshold.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .snap_oversteer
            .enabled = self.experimental_snap_oversteer_enabled;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .snap_oversteer
            .strength = self.experimental_snap_oversteer_strength.clamp(0.0, 1.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .snap_oversteer
            .trigger_threshold = self.experimental_snap_oversteer_threshold.clamp(0.0, 8.0);
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .snap_oversteer
            .attack_ms = self
            .experimental_snap_oversteer_attack_ms
            .round()
            .clamp(10.0, 2_000.0) as u32;
        profile
            .ffb_parameters
            .experimental
            .inferred_dynamics
            .snap_oversteer
            .recovery_ms = self
            .experimental_snap_oversteer_recovery_ms
            .round()
            .clamp(10.0, 4_000.0) as u32;
        profile
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct EditorSettings {
    hot_reload_enabled: bool,
    close_to_taskbar_enabled: bool,
    last_profile_path: Option<String>,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            hot_reload_enabled: true,
            close_to_taskbar_enabled: false,
            last_profile_path: None,
        }
    }
}

impl EditorSettings {
    fn apply_to_window(&self, window: &ProfileEditorWindow) {
        window.set_hot_reload_enabled(self.hot_reload_enabled);
        window.set_close_to_taskbar_enabled(self.close_to_taskbar_enabled);
    }

    fn from_window(window: &ProfileEditorWindow) -> Self {
        Self {
            hot_reload_enabled: window.get_hot_reload_enabled(),
            close_to_taskbar_enabled: window.get_close_to_taskbar_enabled(),
            last_profile_path: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct DeviceCatalog {
    devices: Vec<WinmmDeviceInfo>,
    load_error: Option<String>,
}

impl DeviceCatalog {
    fn load() -> Self {
        match WinmmJoystick::list_devices() {
            Ok(devices) => Self {
                devices,
                load_error: None,
            },
            Err(error) => Self {
                devices: Vec::new(),
                load_error: Some(error.to_string()),
            },
        }
    }

    fn model(&self) -> ModelRc<SharedString> {
        let mut options = vec![SharedString::from("No steering device")];
        options.extend(
            self.devices
                .iter()
                .map(|device| SharedString::from(Self::device_label(device))),
        );
        ModelRc::new(VecModel::from(options))
    }

    fn empty_message(&self) -> SharedString {
        if self.devices.is_empty() {
            SharedString::from(
                self.load_error
                    .as_ref()
                    .map(|error| format!("Device names unavailable: {error}"))
                    .unwrap_or_else(|| "No WinMM devices found.".to_string()),
            )
        } else {
            SharedString::from("")
        }
    }

    fn selected_index(&self, device_id: Option<u32>) -> i32 {
        device_id
            .and_then(|id| self.devices.iter().position(|device| device.id == id))
            .map(|index| index as i32 + 1)
            .unwrap_or(0)
    }

    fn device_id_for_index(&self, index: i32) -> Option<u32> {
        let row = usize::try_from(index).ok()?;
        if row == 0 {
            return None;
        }

        self.devices.get(row - 1).map(|device| device.id)
    }

    fn label_for_device(&self, device_id: Option<u32>) -> String {
        match device_id {
            None => "No steering device".to_string(),
            Some(id) => self
                .devices
                .iter()
                .find(|device| device.id == id)
                .map(Self::device_label)
                .unwrap_or_else(|| format!("WinMM #{} (not found)", id)),
        }
    }

    fn device_label(device: &WinmmDeviceInfo) -> String {
        format!("{} [WinMM #{}]", device.name, device.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ObservabilitySource {
    Live,
    Replay(PathBuf),
}

#[derive(Debug, Clone)]
struct ObservabilityController {
    live_log: DiagnosticsLog,
    source: ObservabilitySource,
}

impl ObservabilityController {
    fn new(live_log: DiagnosticsLog) -> Self {
        Self {
            live_log,
            source: ObservabilitySource::Live,
        }
    }

    fn current_log(&self) -> DiagnosticsLog {
        match &self.source {
            ObservabilitySource::Live => self.live_log.clone(),
            ObservabilitySource::Replay(path) => DiagnosticsLog::new(path),
        }
    }

    fn source_label(&self) -> String {
        match &self.source {
            ObservabilitySource::Live => "Source: live session log".to_string(),
            ObservabilitySource::Replay(path) => {
                format!("Source: replaying {}", replay_source_label(path))
            }
        }
    }

    fn export_live_session(&self) -> Result<PathBuf> {
        self.live_log.export_session()
    }

    fn replay_latest_session(&mut self) -> Result<PathBuf> {
        let replay_path = match self.live_log.latest_exported_session() {
            Ok(path) => path,
            Err(_) => self.live_log.export_session()?,
        };
        self.source = ObservabilitySource::Replay(replay_path.clone());
        Ok(replay_path)
    }

    fn return_to_live(&mut self) -> bool {
        let was_replaying = matches!(self.source, ObservabilitySource::Replay(_));
        self.source = ObservabilitySource::Live;
        was_replaying
    }
}

fn replay_source_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[derive(Debug, Clone, Copy)]
struct CalibrationPresetDef {
    label: &'static str,
    output_gain: f32,
    steering_center_offset: f32,
    steering_range: f32,
    steering_curve: f32,
    summary: &'static str,
}

const CALIBRATION_PRESETS: &[CalibrationPresetDef] = &[
    CalibrationPresetDef {
        label: "FFBeast Precision",
        output_gain: 1.1,
        steering_center_offset: 0.02,
        steering_range: 0.9,
        steering_curve: 1.25,
        summary: "Balanced baseline for FFBeast road feel with steady center response.",
    },
    CalibrationPresetDef {
        label: "FFBeast Drift",
        output_gain: 1.25,
        steering_center_offset: 0.0,
        steering_range: 1.2,
        steering_curve: 0.85,
        summary: "Faster build-up away from center with looser on-center feel.",
    },
    CalibrationPresetDef {
        label: "FFBeast Endurance",
        output_gain: 0.95,
        steering_center_offset: 0.01,
        steering_range: 1.05,
        steering_curve: 1.45,
        summary: "Long-session profile with softer center and reduced sustained load.",
    },
];

#[derive(Debug, Clone, Copy)]
struct CalibrationWorkflowStepDef {
    title: &'static str,
    detail: &'static str,
}

const CALIBRATION_WORKFLOW_STEPS: &[CalibrationWorkflowStepDef] = &[
    CalibrationWorkflowStepDef {
        title: "Step 1: Align Center",
        detail: "Drive straight and use Center Offset so wheel center and in-game straight-ahead agree.",
    },
    CalibrationWorkflowStepDef {
        title: "Step 2: Set Steering Range",
        detail: "Sweep lock-to-lock and tune Steering Range so steering-filter force reaches full weight near your preferred lock.",
    },
    CalibrationWorkflowStepDef {
        title: "Step 3: Shape Center Feel",
        detail: "Adjust Steering Curve: lower values ramp force sooner, higher values keep center softer.",
    },
    CalibrationWorkflowStepDef {
        title: "Step 4: Set Output Headroom",
        detail: "Use Output Gain against peak force telemetry: reduce if clipping, increase if the wheel feels underpowered.",
    },
];

fn calibration_preset_options() -> ModelRc<SharedString> {
    let mut labels = vec![SharedString::from("Custom")];
    labels.extend(
        CALIBRATION_PRESETS
            .iter()
            .map(|preset| SharedString::from(preset.label)),
    );
    ModelRc::new(VecModel::from(labels))
}

fn calibration_preset_index(label: &str) -> i32 {
    CALIBRATION_PRESETS
        .iter()
        .position(|preset| preset.label.eq_ignore_ascii_case(label))
        .map(|idx| idx as i32 + 1)
        .unwrap_or(0)
}

fn calibration_preset_for_index(index: i32) -> Option<&'static CalibrationPresetDef> {
    let row = usize::try_from(index).ok()?;
    if row == 0 {
        return None;
    }

    CALIBRATION_PRESETS.get(row - 1)
}

fn apply_calibration_preset(window: &ProfileEditorWindow, preset: &CalibrationPresetDef) {
    window.set_calibration_preset_label(SharedString::from(preset.label));
    window.set_calibration_output_gain(preset.output_gain);
    window.set_calibration_steering_center_offset(preset.steering_center_offset);
    window.set_calibration_steering_range(preset.steering_range);
    window.set_calibration_steering_curve(preset.steering_curve);
    window.set_selected_calibration_preset_index(calibration_preset_index(preset.label));
}

fn calibration_workflow_step_count() -> i32 {
    CALIBRATION_WORKFLOW_STEPS.len() as i32
}

fn clamp_workflow_step(step: i32) -> i32 {
    step.clamp(0, calibration_workflow_step_count().saturating_sub(1))
}

fn apply_workflow_step_labels(window: &ProfileEditorWindow) {
    let step_index = clamp_workflow_step(window.get_calibration_workflow_step()) as usize;
    let step = CALIBRATION_WORKFLOW_STEPS
        .get(step_index)
        .unwrap_or(&CALIBRATION_WORKFLOW_STEPS[0]);
    window.set_calibration_workflow_title(SharedString::from(step.title));
    window.set_calibration_workflow_detail(SharedString::from(step.detail));
    window.set_calibration_workflow_step_label(SharedString::from(format!(
        "Step {} of {}",
        step_index + 1,
        CALIBRATION_WORKFLOW_STEPS.len()
    )));
}

fn parse_percent_value(text: &str) -> Option<f32> {
    text.trim().trim_end_matches('%').parse::<f32>().ok()
}

fn parse_steering_raw_value(text: &str) -> Option<i32> {
    let start = text.find('(')?;
    let end = text[start + 1..].find(')')? + start + 1;
    text[start + 1..end].trim().parse::<i32>().ok()
}

#[derive(Debug, Clone, Copy)]
struct AutoCalibrationInputs {
    const_magnitude: f32,
    const_maximum_force: f32,
    sine_maximum_force: f32,
    spring_saturation: f32,
    damper_saturation: f32,
    telemetry_peak_force_percent: Option<f32>,
    telemetry_filter_percent: Option<f32>,
    telemetry_steering_raw: Option<i32>,
    existing_center_offset: f32,
}

#[derive(Debug, Clone)]
struct AutoCalibrationRecommendation {
    preset_label: &'static str,
    output_gain: f32,
    steering_center_offset: f32,
    steering_range: f32,
    steering_curve: f32,
    status_message: String,
}

#[derive(Debug, Clone, Copy)]
struct AutoCalibrationSweepSample {
    peak_force_percent: Option<f32>,
    filter_percent: Option<f32>,
    steering_raw: Option<i32>,
}

#[derive(Debug, Clone, Default)]
struct AutoCalibrationSweepState {
    samples: Vec<AutoCalibrationSweepSample>,
}

impl AutoCalibrationSweepState {
    fn push_from_window(&mut self, window: &ProfileEditorWindow) {
        self.samples.push(AutoCalibrationSweepSample {
            peak_force_percent: parse_percent_value(&window.get_telemetry_peak_force().to_string()),
            filter_percent: parse_percent_value(
                &window.get_telemetry_filter_coefficient().to_string(),
            ),
            steering_raw: parse_steering_raw_value(&window.get_telemetry_steering().to_string()),
        });
    }
}

fn round_two_decimals(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

fn compute_auto_calibration(inputs: AutoCalibrationInputs) -> AutoCalibrationRecommendation {
    let authority = (inputs.const_magnitude.clamp(0.0, 2.0) / 2.0) * 0.35
        + inputs.const_maximum_force.clamp(0.0, 1.0) * 0.35
        + inputs.sine_maximum_force.clamp(0.0, 1.0) * 0.20
        + ((inputs.spring_saturation.clamp(0.0, 1.0) + inputs.damper_saturation.clamp(0.0, 1.0))
            * 0.5)
            * 0.10;

    let mut output_gain = (1.2 - authority * 0.45).clamp(0.75, 1.25);
    if let Some(peak_force_percent) = inputs.telemetry_peak_force_percent {
        if peak_force_percent > 96.0 {
            output_gain -= 0.08;
        } else if peak_force_percent < 70.0 {
            output_gain += 0.08;
        }
    }
    output_gain = round_two_decimals(output_gain.clamp(0.0, 2.0));

    let mut steering_range = if let Some(filter_percent) = inputs.telemetry_filter_percent {
        if filter_percent > 90.0 {
            1.10
        } else if filter_percent < 65.0 {
            0.90
        } else {
            1.0
        }
    } else {
        (1.0 + (authority - 0.5) * 0.2).clamp(0.85, 1.15)
    };
    steering_range = round_two_decimals(steering_range.clamp(0.25, 2.0));

    let mut steering_curve = if let Some(filter_percent) = inputs.telemetry_filter_percent {
        if filter_percent > 85.0 {
            1.30
        } else if filter_percent < 60.0 {
            0.90
        } else {
            1.10
        }
    } else {
        (1.0 + authority * 0.2).clamp(0.9, 1.4)
    };
    steering_curve = round_two_decimals(steering_curve.clamp(0.25, 3.0));

    let steering_center_offset = if let Some(raw_steering) = inputs.telemetry_steering_raw {
        round_two_decimals(((raw_steering as f32 - 32_767.0) / 32_767.0).clamp(-0.25, 0.25))
    } else {
        round_two_decimals(inputs.existing_center_offset.clamp(-0.25, 0.25))
    };

    let status_message = match (
        inputs.telemetry_peak_force_percent,
        inputs.telemetry_filter_percent,
    ) {
        (Some(peak), Some(filter)) => format!(
            "Auto calibration applied from live telemetry (peak {:.0}%, filter {:.0}%). Gain {:.2}x, range {:.2}x, curve {:.2}, center {:+.0}%.",
            peak,
            filter,
            output_gain,
            steering_range,
            steering_curve,
            steering_center_offset * 100.0
        ),
        _ => format!(
            "Auto calibration applied using profile defaults. Gain {:.2}x, range {:.2}x, curve {:.2}, center {:+.0}%.",
            output_gain,
            steering_range,
            steering_curve,
            steering_center_offset * 100.0
        ),
    };

    AutoCalibrationRecommendation {
        preset_label: "Auto Quick",
        output_gain,
        steering_center_offset,
        steering_range,
        steering_curve,
        status_message,
    }
}

fn recommend_auto_calibration(window: &ProfileEditorWindow) -> AutoCalibrationRecommendation {
    let inputs = AutoCalibrationInputs {
        const_magnitude: window.get_const_magnitude(),
        const_maximum_force: window.get_const_maximum_force(),
        sine_maximum_force: window.get_sine_maximum_force(),
        spring_saturation: window.get_spring_saturation(),
        damper_saturation: window.get_damper_saturation(),
        telemetry_peak_force_percent: parse_percent_value(
            &window.get_telemetry_peak_force().to_string(),
        ),
        telemetry_filter_percent: parse_percent_value(
            &window.get_telemetry_filter_coefficient().to_string(),
        ),
        telemetry_steering_raw: parse_steering_raw_value(
            &window.get_telemetry_steering().to_string(),
        ),
        existing_center_offset: window.get_calibration_steering_center_offset(),
    };

    compute_auto_calibration(inputs)
}

fn sweep_summary(state: &AutoCalibrationSweepState) -> Option<(f32, f32, f32, f32, usize)> {
    if state.samples.is_empty() {
        return None;
    }

    let peak_values = state
        .samples
        .iter()
        .filter_map(|sample| sample.peak_force_percent)
        .collect::<Vec<_>>();
    let filter_values = state
        .samples
        .iter()
        .filter_map(|sample| sample.filter_percent)
        .collect::<Vec<_>>();
    let steering_values = state
        .samples
        .iter()
        .filter_map(|sample| sample.steering_raw)
        .collect::<Vec<_>>();

    if peak_values.len() < 4 || filter_values.len() < 4 || steering_values.len() < 4 {
        return None;
    }

    let peak_max = peak_values.iter().copied().fold(0.0_f32, f32::max);
    let peak_avg = peak_values.iter().sum::<f32>() / peak_values.len() as f32;
    let filter_avg = filter_values.iter().sum::<f32>() / filter_values.len() as f32;
    let steering_avg = steering_values
        .iter()
        .map(|value| *value as f32)
        .sum::<f32>()
        / steering_values.len() as f32;

    Some((
        peak_max,
        peak_avg,
        filter_avg,
        steering_avg,
        state.samples.len(),
    ))
}

fn apply_auto_calibration(
    window: &ProfileEditorWindow,
    recommendation: &AutoCalibrationRecommendation,
) {
    window.set_calibration_preset_label(SharedString::from(recommendation.preset_label));
    window.set_selected_calibration_preset_index(0);
    window.set_calibration_output_gain(recommendation.output_gain);
    window.set_calibration_steering_center_offset(recommendation.steering_center_offset);
    window.set_calibration_steering_range(recommendation.steering_range);
    window.set_calibration_steering_curve(recommendation.steering_curve);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TelemetryPanelView {
    status: String,
    uptime: String,
    packet_rate: String,
    command_rate: String,
    steering: String,
    peak_force: String,
    calibration_profile: String,
    calibration_gain: String,
    filter_coefficient: String,
    clamp_status: String,
    saturation_status: String,
    packet_trend: String,
    command_trend: String,
    force_trend: String,
    runtime_detail: String,
    last_update: String,
    last_commands: String,
}

impl TelemetryPanelView {
    fn waiting(source_label: &str) -> Self {
        Self {
            status: format!("{source_label}. Waiting for bridge telemetry."),
            uptime: "Idle".to_string(),
            packet_rate: "0.0/s".to_string(),
            command_rate: "0.0/s".to_string(),
            steering: "Unavailable".to_string(),
            peak_force: "0%".to_string(),
            calibration_profile: "Custom".to_string(),
            calibration_gain: "1.00x".to_string(),
            filter_coefficient: "Unavailable".to_string(),
            clamp_status: "No active force output.".to_string(),
            saturation_status: "No condition saturation activity.".to_string(),
            packet_trend: "No packet-rate trend yet.".to_string(),
            command_trend: "No command-rate trend yet.".to_string(),
            force_trend: "No force trend yet.".to_string(),
            runtime_detail:
                "Launch the bridge to capture input, output, poll interval, and hot-reload state."
                    .to_string(),
            last_update: "No translated update yet.".to_string(),
            last_commands: "No applied command summary yet.".to_string(),
        }
    }

    fn failed(source_label: &str, error: &str) -> Self {
        Self {
            status: format!("{source_label}. Telemetry unavailable."),
            uptime: "Read failed".to_string(),
            packet_rate: "Read failed".to_string(),
            command_rate: "Read failed".to_string(),
            steering: "Read failed".to_string(),
            peak_force: "Read failed".to_string(),
            calibration_profile: "Read failed".to_string(),
            calibration_gain: "Read failed".to_string(),
            filter_coefficient: "Read failed".to_string(),
            clamp_status: format!("Failed to read telemetry log: {error}"),
            saturation_status: format!("Failed to read telemetry log: {error}"),
            packet_trend: format!("Failed to read telemetry log: {error}"),
            command_trend: format!("Failed to read telemetry log: {error}"),
            force_trend: format!("Failed to read telemetry log: {error}"),
            runtime_detail: format!("Failed to read telemetry log: {error}"),
            last_update: format!("Failed to read telemetry log: {error}"),
            last_commands: format!("Failed to read telemetry log: {error}"),
        }
    }

    fn from_events(events: &[BridgeDiagnosticEvent], source_label: &str) -> Self {
        let telemetry_events = events
            .iter()
            .filter(|event| event.category == DiagnosticCategory::Telemetry)
            .collect::<Vec<_>>();
        let startup_event = events
            .iter()
            .rev()
            .find(|event| event.category == DiagnosticCategory::Startup);
        let Some(event) = telemetry_events.last().copied() else {
            return Self::waiting(source_label);
        };

        let trend_events = if telemetry_events.len() > 24 {
            &telemetry_events[telemetry_events.len() - 24..]
        } else {
            &telemetry_events
        };

        let uptime = telemetry_field(event, "uptime", "Waiting");
        let packet_rate = telemetry_field(event, "packet_rate", "0.0/s");
        let command_rate = telemetry_field(event, "command_rate", "0.0/s");
        let steering = telemetry_field(event, "steering", "Unavailable");
        let peak_force = telemetry_field(event, "peak_force", "0%");
        let calibration_profile = telemetry_field(event, "calibration_preset", "Custom");
        let calibration_gain = telemetry_field(event, "calibration_output_gain", "1.00x");
        let filter_coefficient = telemetry_field(event, "filter_coefficient", "Unavailable");
        let clamp_status = telemetry_field(event, "clamp_status", "No active force output.");
        let saturation_status = telemetry_field(
            event,
            "saturation_status",
            "No condition saturation activity.",
        );
        let last_packet_age = telemetry_field(event, "last_packet_age", "No packet yet");
        let input = telemetry_field(event, "input", "No steering input");
        let output = telemetry_field(event, "output", "Unknown output");
        let poll_ms = telemetry_field(event, "poll_ms", "Unknown poll");
        let hot_reload = telemetry_field(event, "hot_reload", "Unknown");
        let updates_total = telemetry_field(event, "updates_total", "0");
        let commands_total = telemetry_field(event, "commands_total", "0");
        let const_gain = telemetry_field(event, "calibration_const_gain", "1.00x");
        let sine_gain = telemetry_field(event, "calibration_sine_gain", "1.00x");
        let spring_gain = telemetry_field(event, "calibration_spring_gain", "1.00x");
        let damper_gain = telemetry_field(event, "calibration_damper_gain", "1.00x");
        let startup_profile = startup_event
            .and_then(|entry| entry.field_value("profile"))
            .unwrap_or("No startup snapshot yet");
        let startup_routing = startup_event
            .and_then(|entry| entry.field_value("routing_source"))
            .unwrap_or("Routing source unavailable");
        let startup_preflight = startup_event
            .and_then(|entry| entry.field_value("preflight_status"))
            .unwrap_or("Preflight status unavailable");

        Self {
            status: format!("{source_label}. Last packet {last_packet_age}."),
            uptime,
            packet_rate,
            command_rate,
            steering,
            peak_force,
            calibration_profile,
            calibration_gain,
            filter_coefficient,
            clamp_status,
            saturation_status,
            packet_trend: telemetry_rate_trend(trend_events, "packet_rate_value", "/s"),
            command_trend: telemetry_rate_trend(trend_events, "command_rate_value", "/s"),
            force_trend: telemetry_percent_trend(trend_events, "peak_force_ratio"),
            runtime_detail: format!(
                "Startup profile: {startup_profile}\nStartup routing: {startup_routing}\nStartup preflight: {startup_preflight}\nInput: {input}\nOutput: {output}\nPoll interval: {poll_ms}\nHot reload: {hot_reload}\nPer-effect gain: const {const_gain}, periodic {sine_gain}, spring {spring_gain}, damper {damper_gain}\nTotals: {updates_total} updates / {commands_total} commands"
            ),
            last_update: telemetry_field(
                event,
                "last_update",
                "No translated update captured yet.",
            ),
            last_commands: telemetry_field(
                event,
                "last_commands",
                "No applied commands captured yet.",
            ),
        }
    }

    fn apply_to_window(&self, window: &ProfileEditorWindow) {
        window.set_telemetry_status(SharedString::from(self.status.clone()));
        window.set_telemetry_uptime(SharedString::from(self.uptime.clone()));
        window.set_telemetry_packet_rate(SharedString::from(self.packet_rate.clone()));
        window.set_telemetry_command_rate(SharedString::from(self.command_rate.clone()));
        window.set_telemetry_steering(SharedString::from(self.steering.clone()));
        window.set_telemetry_peak_force(SharedString::from(self.peak_force.clone()));
        window.set_telemetry_calibration_profile(SharedString::from(
            self.calibration_profile.clone(),
        ));
        window.set_telemetry_calibration_gain(SharedString::from(self.calibration_gain.clone()));
        window
            .set_telemetry_filter_coefficient(SharedString::from(self.filter_coefficient.clone()));
        window.set_telemetry_clamp_status(SharedString::from(self.clamp_status.clone()));
        window.set_telemetry_saturation_status(SharedString::from(self.saturation_status.clone()));
        window.set_telemetry_packet_trend(SharedString::from(self.packet_trend.clone()));
        window.set_telemetry_command_trend(SharedString::from(self.command_trend.clone()));
        window.set_telemetry_force_trend(SharedString::from(self.force_trend.clone()));
        window.set_telemetry_runtime_detail(SharedString::from(self.runtime_detail.clone()));
        window.set_telemetry_last_update(SharedString::from(self.last_update.clone()));
        window.set_telemetry_last_commands(SharedString::from(self.last_commands.clone()));
    }
}

fn telemetry_field(event: &BridgeDiagnosticEvent, key: &str, fallback: &str) -> String {
    event.field_value(key).unwrap_or(fallback).to_string()
}

fn telemetry_numeric_field(event: &BridgeDiagnosticEvent, key: &str) -> Option<f32> {
    event.field_value(key)?.parse::<f32>().ok()
}

fn telemetry_numeric_series(events: &[&BridgeDiagnosticEvent], key: &str) -> Vec<f32> {
    events
        .iter()
        .filter_map(|event| telemetry_numeric_field(event, key))
        .collect()
}

fn telemetry_rate_trend(events: &[&BridgeDiagnosticEvent], key: &str, unit: &str) -> String {
    let series = telemetry_numeric_series(events, key);
    if series.is_empty() {
        return "No trend yet.".to_string();
    }

    let peak = series.iter().copied().fold(0.0f32, f32::max);
    format!("{}  peak {:.1}{unit}", ascii_sparkline(&series), peak)
}

fn telemetry_percent_trend(events: &[&BridgeDiagnosticEvent], key: &str) -> String {
    let series = telemetry_numeric_series(events, key);
    if series.is_empty() {
        return "No trend yet.".to_string();
    }

    let peak = series.iter().copied().fold(0.0f32, f32::max) * 100.0;
    format!("{}  peak {:.0}%", ascii_sparkline(&series), peak)
}

fn ascii_sparkline(samples: &[f32]) -> String {
    const GLYPHS: &[u8] = b" .:-=+*#%@";

    if samples.is_empty() {
        return "-".to_string();
    }

    let max_value = samples.iter().copied().fold(0.0f32, f32::max).max(1.0);
    samples
        .iter()
        .map(|sample| {
            let normalized = (*sample / max_value).clamp(0.0, 1.0);
            let index = (normalized * (GLYPHS.len() - 1) as f32).round() as usize;
            GLYPHS[index] as char
        })
        .collect()
}

fn selected_steering_device(window: &ProfileEditorWindow) -> Option<u32> {
    let steering_device = window.get_steering_device().round() as i32;
    (steering_device >= 0).then_some(steering_device as u32)
}

fn hot_reload_summary(enabled: bool) -> &'static str {
    if enabled { "On save" } else { "Manual" }
}

fn editor_settings_path() -> Result<PathBuf> {
    if let Some(base_dir) = std::env::var_os("APPDATA").or_else(|| std::env::var_os("LOCALAPPDATA"))
    {
        return Ok(PathBuf::from(base_dir)
            .join("Torquebridge")
            .join("editor-settings.json"));
    }

    Ok(std::env::current_dir()
        .context("failed to resolve current directory for editor settings")?
        .join(".torquebridge-editor-settings.json"))
}

fn load_editor_settings(path: &Path) -> (EditorSettings, Option<String>) {
    match fs::read_to_string(path) {
        Ok(json) => {
            match serde_json::from_str::<EditorSettings>(json.trim_start_matches('\u{feff}')) {
                Ok(settings) => (settings, None),
                Err(_) => (
                    EditorSettings::default(),
                    Some("Editor settings reset to defaults.".to_string()),
                ),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (EditorSettings::default(), None)
        }
        Err(_) => (
            EditorSettings::default(),
            Some("Editor settings unavailable; using defaults.".to_string()),
        ),
    }
}

fn save_editor_settings(path: &Path, settings: &EditorSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create editor settings directory")?;
    }

    let json =
        serde_json::to_string_pretty(settings).context("failed to serialize editor settings")?;
    fs::write(path, format!("{json}\n")).context("failed to write editor settings")?;
    Ok(())
}

fn save_window_editor_settings(
    window: &ProfileEditorWindow,
    editor_settings: &RefCell<EditorSettings>,
    path: &Path,
) -> Result<EditorSettings> {
    let mut settings = EditorSettings::from_window(window);
    settings.last_profile_path = editor_settings.borrow().last_profile_path.clone();
    save_editor_settings(path, &settings)?;
    *editor_settings.borrow_mut() = settings.clone();
    Ok(settings)
}

fn save_last_profile_path(
    editor_settings: &RefCell<EditorSettings>,
    settings_path: &Path,
    profile_path: &Path,
) -> Result<()> {
    let mut next = editor_settings.borrow().clone();
    next.last_profile_path = Some(normalize_workspace_path_display(profile_path));
    save_editor_settings(settings_path, &next)?;
    *editor_settings.borrow_mut() = next;
    Ok(())
}

fn initial_status_message(
    settings_notice: Option<String>,
    device_catalog: &DeviceCatalog,
) -> String {
    match (settings_notice, device_catalog.load_error.as_ref()) {
        (Some(settings_notice), Some(_)) => format!("{settings_notice} Device names unavailable."),
        (Some(settings_notice), None) => settings_notice,
        (None, Some(_)) => "Save writes the profile. Device names unavailable.".to_string(),
        (None, None) => "Save writes the profile.".to_string(),
    }
}

#[derive(Debug)]
struct BridgeController {
    seed_config_path: Option<PathBuf>,
    profile_path: PathBuf,
    diagnostics_log: DiagnosticsLog,
    child: Option<Child>,
    detail: String,
}

#[derive(Debug, Clone)]
struct StartupSnapshot {
    profile_path: String,
    routing_status: String,
    routing_source: String,
    preflight_status: String,
    selected_input: String,
    poll_ms: String,
    hot_reload: String,
}

impl BridgeController {
    fn new(
        seed_config_path: Option<PathBuf>,
        profile_path: PathBuf,
        diagnostics_log: DiagnosticsLog,
    ) -> Self {
        Self {
            seed_config_path,
            profile_path,
            diagnostics_log,
            child: None,
            detail: "Idle.".to_string(),
        }
    }

    fn is_running(&self) -> bool {
        self.child.is_some()
    }

    fn status_label(&self) -> &'static str {
        if self.is_running() {
            "RUNNING"
        } else {
            "STOPPED"
        }
    }

    fn detail(&self) -> &str {
        &self.detail
    }

    fn set_profile_path(&mut self, profile_path: PathBuf) {
        self.profile_path = profile_path;
    }

    fn start(
        &mut self,
        hot_reload_enabled: bool,
        startup_snapshot: &StartupSnapshot,
    ) -> Result<()> {
        if self.child.is_some() {
            self.detail = "Already running.".to_string();
            record_ui_event(
                &self.diagnostics_log,
                DiagnosticLevel::Warning,
                DiagnosticCategory::Lifecycle,
                "Bridge launch requested while bridge is already running",
                Vec::new(),
            );
            return Ok(());
        }

        self.diagnostics_log.clear()?;
        let safety_detail = apply_bridge_safety_reset(
            self.profile_path.as_path(),
            self.seed_config_path.as_deref(),
            &self.diagnostics_log,
            "Prelaunch",
        )?;

        let executable = std::env::current_exe().context("failed to locate current executable")?;
        let mut command = Command::new(executable);
        command
            .arg("ffb-bridge")
            .arg("--config")
            .arg(
                self.seed_config_path
                    .as_deref()
                    .unwrap_or_else(|| Path::new("configuration.json")),
            )
            .arg("--profile")
            .arg(&self.profile_path)
            .arg("--diagnostics-log")
            .arg(self.diagnostics_log.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if !hot_reload_enabled {
            command.arg("--no-profile-watch");
        }

        #[cfg(windows)]
        {
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let child = command.spawn().context("failed to launch bridge process")?;
        let pid = child.id();
        self.child = Some(child);
        self.detail = if hot_reload_enabled {
            format!("PID {pid}. Hot reload on. {safety_detail}")
        } else {
            format!("PID {pid}. Hot reload off. {safety_detail}")
        };
        record_ui_event(
            &self.diagnostics_log,
            DiagnosticLevel::Info,
            DiagnosticCategory::Startup,
            "Startup snapshot captured",
            vec![
                DiagnosticField::new("pid", pid.to_string()),
                DiagnosticField::new("profile", startup_snapshot.profile_path.clone()),
                DiagnosticField::new("routing_status", startup_snapshot.routing_status.clone()),
                DiagnosticField::new("routing_source", startup_snapshot.routing_source.clone()),
                DiagnosticField::new(
                    "preflight_status",
                    startup_snapshot.preflight_status.clone(),
                ),
                DiagnosticField::new("selected_input", startup_snapshot.selected_input.clone()),
                DiagnosticField::new("poll_ms", startup_snapshot.poll_ms.clone()),
                DiagnosticField::new("hot_reload", startup_snapshot.hot_reload.clone()),
            ],
        );
        record_ui_event(
            &self.diagnostics_log,
            DiagnosticLevel::Info,
            DiagnosticCategory::Lifecycle,
            "Launched bridge process",
            vec![
                DiagnosticField::new("pid", pid.to_string()),
                DiagnosticField::new(
                    "hot_reload",
                    if hot_reload_enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                ),
                DiagnosticField::new("profile", self.profile_path.display().to_string()),
            ],
        );
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            let pid = child.id();
            if child
                .try_wait()
                .context("failed to check bridge process before stopping")?
                .is_none()
            {
                child.kill().context("failed to stop bridge process")?;
            }
            let _ = child.wait();

            let safety_detail = apply_bridge_safety_reset(
                self.profile_path.as_path(),
                self.seed_config_path.as_deref(),
                &self.diagnostics_log,
                "Shutdown",
            )?;
            self.detail = format!("Stopped PID {pid}. {safety_detail}");
            record_ui_event(
                &self.diagnostics_log,
                DiagnosticLevel::Info,
                DiagnosticCategory::Lifecycle,
                "Stopped bridge process",
                vec![DiagnosticField::new("pid", pid.to_string())],
            );
        } else {
            self.detail = "Already stopped.".to_string();
        }
        Ok(())
    }

    fn poll(&mut self) -> Result<Option<String>> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };

        if let Some(status) = child.try_wait().context("failed to poll bridge process")? {
            let pid = child.id();
            self.child = None;
            let safety_detail = apply_bridge_safety_reset(
                self.profile_path.as_path(),
                self.seed_config_path.as_deref(),
                &self.diagnostics_log,
                "Exit",
            )?;
            self.detail = format!("Exited: {status}. {safety_detail}");
            record_ui_event(
                &self.diagnostics_log,
                DiagnosticLevel::Warning,
                DiagnosticCategory::Lifecycle,
                "Bridge process exited",
                vec![
                    DiagnosticField::new("pid", pid.to_string()),
                    DiagnosticField::new("status", status.to_string()),
                ],
            );
            return Ok(Some(self.detail.clone()));
        }

        Ok(None)
    }
}

pub fn run_profile_editor(config_path: Option<&str>, profile_path: Option<&str>) -> Result<()> {
    let cli_config_path = config_path.map(|value| resolve_runtime_path(Path::new(value)));
    let seed_config_path = cli_config_path.or_else(discover_seed_config_path);
    let settings_path = Rc::new(editor_settings_path()?);
    let (loaded_editor_settings, settings_notice) = load_editor_settings(settings_path.as_path());
    let startup_profile_input =
        profile_path.or(loaded_editor_settings.last_profile_path.as_deref());
    let (target_path, profile) =
        load_profile_for_editor(seed_config_path.as_deref(), startup_profile_input)?;
    let window = ProfileEditorWindow::new().context("failed to create Slint profile editor")?;
    let profile_state = Rc::new(RefCell::new(profile));
    let path = Rc::new(RefCell::new(target_path));
    let diagnostics_log = Rc::new(DiagnosticsLog::new(DiagnosticsLog::default_path()?));
    let observability = Rc::new(RefCell::new(ObservabilityController::new(
        (*diagnostics_log).clone(),
    )));
    let editor_settings = Rc::new(RefCell::new(loaded_editor_settings));
    let device_catalog = Rc::new(DeviceCatalog::load());
    let bridge_controller = Rc::new(RefCell::new(BridgeController::new(
        seed_config_path.clone(),
        path.borrow().clone(),
        (*diagnostics_log).clone(),
    )));

    ProfileEditorState::from(&*profile_state.borrow())
        .apply_to_window(&window, path.borrow().as_path());
    let profile_options = Rc::new(RefCell::new(discover_profile_options()));
    let compare_profile_options = Rc::new(RefCell::new(discover_compare_profile_options(
        path.borrow().as_path(),
        profile_options.borrow().as_slice(),
    )));
    window.set_profile_options(profile_options_model(profile_options.borrow().as_slice()));
    window.set_compare_profile_options(compare_profile_options_model(
        compare_profile_options.borrow().as_slice(),
    ));
    window.set_compare_show_all(false);
    window.set_selected_compare_profile_index(0);
    window.set_compare_result(SharedString::from(
        "Pick a profile and run A/B diff. Showing changed fields only.",
    ));
    window.set_selected_profile_index(profile_option_index(
        path.borrow().as_path(),
        profile_options.borrow().as_slice(),
    ));
    editor_settings.borrow().apply_to_window(&window);
    window.set_steering_device_options(device_catalog.model());
    window.set_calibration_preset_options(calibration_preset_options());
    let calibration_label = window.get_calibration_preset_label().to_string();
    window.set_selected_calibration_preset_index(calibration_preset_index(&calibration_label));
    window.set_calibration_workflow_step(0);
    apply_workflow_step_labels(&window);
    window.set_device_catalog_message(device_catalog.empty_message());
    window.set_diagnostics_workflow_hint(SharedString::from(
        "Workflow: Export session -> Replay latest export -> Return to live.",
    ));
    window.set_diagnostics_last_session_export(SharedString::from("Session export: none yet."));
    window.set_diagnostics_last_bundle_export(SharedString::from("Debug bundle: none yet."));
    window.set_status_message(SharedString::from(initial_status_message(
        settings_notice,
        device_catalog.as_ref(),
    )));

    if let Err(error) = save_last_profile_path(
        &editor_settings,
        settings_path.as_path(),
        path.borrow().as_path(),
    ) {
        window.set_status_message(SharedString::from(format!(
            "Profile loaded, but saving last-used profile failed: {error}"
        )));
    }
    refresh_runtime_summaries(&window, device_catalog.as_ref());
    refresh_runtime_routing_preflight(
        &window,
        path.borrow().as_path(),
        seed_config_path.as_deref(),
    );
    if let Some(issue) = vjoy_runtime_issue_nonblocking(DEFAULT_VJOY_DEVICE_ID) {
        window.set_status_message(SharedString::from(format!("vJoy prerequisite: {issue}")));
    }
    sync_bridge_panel(&window, &bridge_controller.borrow());
    refresh_diagnostics_panel(&window, &observability.borrow());

    let weak_window = window.as_weak();
    let profile_options_handle = Rc::clone(&profile_options);
    let compare_profile_options_handle = Rc::clone(&compare_profile_options);
    let profile_state_handle = Rc::clone(&profile_state);
    let path_handle = Rc::clone(&path);
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let editor_settings_handle = Rc::clone(&editor_settings);
    let settings_path_handle = Rc::clone(&settings_path);
    window.on_select_profile_requested(move |index| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let Some(selected_profile_path) =
            profile_option_path_for_index(index, profile_options_handle.borrow().as_slice())
        else {
            return;
        };

        match load_profile(&selected_profile_path) {
            Ok(profile) => {
                *profile_state_handle.borrow_mut() = profile.clone();
                *path_handle.borrow_mut() = selected_profile_path.clone();
                bridge_controller_handle
                    .borrow_mut()
                    .set_profile_path(selected_profile_path.clone());
                ProfileEditorState::from(&profile)
                    .apply_to_window(&window, selected_profile_path.as_path());
                window.set_selected_profile_index(index);
                if let Err(error) = save_last_profile_path(
                    &editor_settings_handle,
                    settings_path_handle.as_path(),
                    selected_profile_path.as_path(),
                ) {
                    window.set_status_message(SharedString::from(format!(
                        "Loaded profile {}, but saving last-used profile failed: {error}",
                        selected_profile_path.display()
                    )));
                } else {
                    window.set_status_message(SharedString::from(format!(
                        "Loaded profile {}.",
                        selected_profile_path.display()
                    )));
                }
                refresh_runtime_routing_preflight(
                    &window,
                    selected_profile_path.as_path(),
                    bridge_controller_handle
                        .borrow()
                        .seed_config_path
                        .as_deref(),
                );
                *compare_profile_options_handle.borrow_mut() = discover_compare_profile_options(
                    selected_profile_path.as_path(),
                    profile_options_handle.borrow().as_slice(),
                );
                window.set_compare_profile_options(compare_profile_options_model(
                    compare_profile_options_handle.borrow().as_slice(),
                ));
                window.set_selected_compare_profile_index(0);
                window.set_compare_result(SharedString::from(if window.get_compare_show_all() {
                    "Pick a profile and run A/B diff. Showing all fields."
                } else {
                    "Pick a profile and run A/B diff. Showing changed fields only."
                }));
            }
            Err(error) => {
                window.set_status_message(SharedString::from(format!(
                    "Failed to load selected profile: {error}"
                )));
            }
        }
    });

    let weak_window = window.as_weak();
    window.window().on_close_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return CloseRequestResponse::HideWindow;
        };

        if window.get_close_to_taskbar_enabled() {
            window.window().set_minimized(true);
            window.set_status_message(SharedString::from("Window minimized to taskbar."));
            CloseRequestResponse::KeepWindowShown
        } else {
            CloseRequestResponse::HideWindow
        }
    });

    let weak_window = window.as_weak();
    window.on_apply_calibration_preset_requested(move |index| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        if let Some(preset) = calibration_preset_for_index(index) {
            apply_calibration_preset(&window, preset);
            window.set_status_message(SharedString::from(format!(
                "Applied calibration preset {}. {}",
                preset.label, preset.summary
            )));
        } else {
            window.set_calibration_preset_label(SharedString::from("Custom"));
            window.set_selected_calibration_preset_index(0);
            window.set_status_message(SharedString::from(
                "Preset set to Custom. Manual calibration controls are active.",
            ));
        }
    });

    let weak_window = window.as_weak();
    window.on_calibration_guide_previous_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let next_step = clamp_workflow_step(window.get_calibration_workflow_step() - 1);
        window.set_calibration_workflow_step(next_step);
        apply_workflow_step_labels(&window);
    });

    let weak_window = window.as_weak();
    window.on_calibration_guide_next_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let next_step = clamp_workflow_step(window.get_calibration_workflow_step() + 1);
        window.set_calibration_workflow_step(next_step);
        apply_workflow_step_labels(&window);
    });

    let weak_window = window.as_weak();
    window.on_calibration_guide_apply_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        window.set_calibration_preset_label(SharedString::from("Custom"));
        window.set_selected_calibration_preset_index(0);

        let message = match clamp_workflow_step(window.get_calibration_workflow_step()) {
            0 => {
                if let Some(raw) = parse_steering_raw_value(&window.get_telemetry_steering().to_string()) {
                    let offset = ((raw as f32 - 32_767.0) / 32_767.0).clamp(-0.5, 0.5);
                    window.set_calibration_steering_center_offset(offset);
                    format!("Center alignment assist set offset to {:+.2}% from live steering.", offset * 100.0)
                } else {
                    "Center alignment assist needs live steering telemetry. Drive briefly, then retry.".to_string()
                }
            }
            1 => {
                let next_range = if let Some(filter_percent) =
                    parse_percent_value(&window.get_telemetry_filter_coefficient().to_string())
                {
                    if filter_percent > 90.0 {
                        (window.get_calibration_steering_range() + 0.05).clamp(0.25, 2.0)
                    } else if filter_percent < 70.0 {
                        (window.get_calibration_steering_range() - 0.05).clamp(0.25, 2.0)
                    } else {
                        window.get_calibration_steering_range()
                    }
                } else {
                    window.get_calibration_steering_range()
                };
                window.set_calibration_steering_range(next_range);
                format!("Range sweep assist set steering range to {:.2}x.", next_range)
            }
            2 => {
                let next_curve = if let Some(filter_percent) =
                    parse_percent_value(&window.get_telemetry_filter_coefficient().to_string())
                {
                    if filter_percent > 85.0 {
                        (window.get_calibration_steering_curve() + 0.1).clamp(0.25, 3.0)
                    } else if filter_percent < 65.0 {
                        (window.get_calibration_steering_curve() - 0.1).clamp(0.25, 3.0)
                    } else {
                        window.get_calibration_steering_curve()
                    }
                } else {
                    window.get_calibration_steering_curve()
                };
                window.set_calibration_steering_curve(next_curve);
                format!("Curve shaping assist set steering curve to {:.2}.", next_curve)
            }
            _ => {
                let next_gain = if let Some(peak_percent) =
                    parse_percent_value(&window.get_telemetry_peak_force().to_string())
                {
                    if peak_percent > 96.0 {
                        (window.get_calibration_output_gain() - 0.05).clamp(0.0, 2.0)
                    } else if peak_percent < 70.0 {
                        (window.get_calibration_output_gain() + 0.05).clamp(0.0, 2.0)
                    } else {
                        window.get_calibration_output_gain()
                    }
                } else {
                    window.get_calibration_output_gain()
                };
                window.set_calibration_output_gain(next_gain);
                format!("Headroom assist set output gain to {:.2}x.", next_gain)
            }
        };

        window.set_status_message(SharedString::from(message));
    });

    let weak_window = window.as_weak();
    window.on_auto_calibration_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let recommendation = recommend_auto_calibration(&window);
        apply_auto_calibration(&window, &recommendation);
        window.set_status_message(SharedString::from(recommendation.status_message));
    });

    let sweep_timer = Rc::new(Timer::default());
    let sweep_state = Rc::new(RefCell::new(None::<AutoCalibrationSweepState>));
    let weak_window = window.as_weak();
    let sweep_timer_handle = Rc::clone(&sweep_timer);
    let sweep_state_handle = Rc::clone(&sweep_state);
    let diagnostics_log_handle = Rc::clone(&diagnostics_log);
    let observability_handle = Rc::clone(&observability);
    window.on_run_calibration_sweep_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        if sweep_state_handle.borrow().is_some() {
            window.set_status_message(SharedString::from(
                "Calibration sweep is already running. Hold steering for a few seconds.",
            ));
            return;
        }

        *sweep_state_handle.borrow_mut() = Some(AutoCalibrationSweepState::default());
        window.set_status_message(SharedString::from(
            "Running calibration sweep for 4 seconds. Keep driving through center and light corners.",
        ));

        let weak_window = weak_window.clone();
        let sweep_timer_inner = Rc::clone(&sweep_timer_handle);
        let sweep_state_inner = Rc::clone(&sweep_state_handle);
        let diagnostics_log_inner = Rc::clone(&diagnostics_log_handle);
        let observability_inner = Rc::clone(&observability_handle);
        sweep_timer_handle.start(
            TimerMode::Repeated,
            Duration::from_millis(AUTO_CALIBRATION_SWEEP_INTERVAL_MS),
            move || {
                let Some(window) = weak_window.upgrade() else {
                    sweep_timer_inner.stop();
                    *sweep_state_inner.borrow_mut() = None;
                    return;
                };

                {
                    let mut state_guard = sweep_state_inner.borrow_mut();
                    let Some(state) = state_guard.as_mut() else {
                        sweep_timer_inner.stop();
                        return;
                    };

                    state.push_from_window(&window);
                    if state.samples.len() < AUTO_CALIBRATION_SWEEP_SAMPLE_TARGET {
                        return;
                    }
                }

                sweep_timer_inner.stop();
                let finished_state = sweep_state_inner.borrow_mut().take().unwrap_or_default();

                if let Some((peak_max, peak_avg, filter_avg, steering_avg, sample_count)) =
                    sweep_summary(&finished_state)
                {
                    let recommendation = compute_auto_calibration(AutoCalibrationInputs {
                        const_magnitude: window.get_const_magnitude(),
                        const_maximum_force: window.get_const_maximum_force(),
                        sine_maximum_force: window.get_sine_maximum_force(),
                        spring_saturation: window.get_spring_saturation(),
                        damper_saturation: window.get_damper_saturation(),
                        telemetry_peak_force_percent: Some(peak_max),
                        telemetry_filter_percent: Some(filter_avg),
                        telemetry_steering_raw: Some(steering_avg.round() as i32),
                        existing_center_offset: window.get_calibration_steering_center_offset(),
                    });

                    apply_auto_calibration(&window, &recommendation);
                    window.set_calibration_preset_label(SharedString::from("Auto Sweep"));
                    window.set_status_message(SharedString::from(format!(
                        "Sweep complete ({} samples). Peak max {:.0}% avg {:.0}%, filter avg {:.0}%. Gain {:.2}x range {:.2}x curve {:.2} center {:+.0}%.",
                        sample_count,
                        peak_max,
                        peak_avg,
                        filter_avg,
                        recommendation.output_gain,
                        recommendation.steering_range,
                        recommendation.steering_curve,
                        recommendation.steering_center_offset * 100.0,
                    )));

                    record_ui_event(
                        diagnostics_log_inner.as_ref(),
                        DiagnosticLevel::Info,
                        DiagnosticCategory::Telemetry,
                        "Calibration sweep recommendation applied",
                        vec![
                            DiagnosticField::new("samples", sample_count.to_string()),
                            DiagnosticField::new("peak_force_max_percent", format!("{:.1}", peak_max)),
                            DiagnosticField::new("peak_force_avg_percent", format!("{:.1}", peak_avg)),
                            DiagnosticField::new("filter_avg_percent", format!("{:.1}", filter_avg)),
                            DiagnosticField::new("steering_avg_raw", format!("{:.0}", steering_avg)),
                            DiagnosticField::new(
                                "recommended_output_gain",
                                format!("{:.2}", recommendation.output_gain),
                            ),
                            DiagnosticField::new(
                                "recommended_steering_range",
                                format!("{:.2}", recommendation.steering_range),
                            ),
                            DiagnosticField::new(
                                "recommended_steering_curve",
                                format!("{:.2}", recommendation.steering_curve),
                            ),
                            DiagnosticField::new(
                                "recommended_center_offset",
                                format!("{:.2}", recommendation.steering_center_offset),
                            ),
                        ],
                    );
                    refresh_diagnostics_panel(&window, &observability_inner.borrow());
                } else {
                    window.set_status_message(SharedString::from(
                        "Sweep finished but telemetry was too sparse. Drive with bridge running and retry.",
                    ));
                    record_ui_event(
                        diagnostics_log_inner.as_ref(),
                        DiagnosticLevel::Warning,
                        DiagnosticCategory::Telemetry,
                        "Calibration sweep skipped due to sparse telemetry",
                        vec![DiagnosticField::new(
                            "samples",
                            finished_state.samples.len().to_string(),
                        )],
                    );
                    refresh_diagnostics_panel(&window, &observability_inner.borrow());
                }
            },
        );
    });

    let weak_window = window.as_weak();
    let profile_state_handle = Rc::clone(&profile_state);
    let path_handle = Rc::clone(&path);
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let device_catalog_handle = Rc::clone(&device_catalog);
    let profile_options_handle = Rc::clone(&profile_options);
    let compare_profile_options_handle = Rc::clone(&compare_profile_options);
    let seed_config_path_handle = seed_config_path.clone();
    let editor_settings_handle = Rc::clone(&editor_settings);
    let settings_path_handle = Rc::clone(&settings_path);
    window.on_save_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let current_path = path_handle.borrow().clone();
        let save_path = derive_profile_path_from_window_name(
            current_path.as_path(),
            &window.get_profile_name().to_string(),
        );
        match save_window_profile(
            &window,
            &profile_state_handle,
            save_path.as_path(),
            seed_config_path_handle.as_deref(),
        ) {
            Ok(_) => {
                *path_handle.borrow_mut() = save_path.clone();
                bridge_controller_handle
                    .borrow_mut()
                    .set_profile_path(save_path.clone());
                window.set_profile_path(SharedString::from(save_path.display().to_string()));
                if let Err(error) = save_last_profile_path(
                    &editor_settings_handle,
                    settings_path_handle.as_path(),
                    save_path.as_path(),
                ) {
                    window.set_status_message(SharedString::from(format!(
                        "Saved profile, but failed to store last-used profile: {error}"
                    )));
                    return;
                }
                *profile_options_handle.borrow_mut() = discover_profile_options();
                window.set_profile_options(profile_options_model(
                    profile_options_handle.borrow().as_slice(),
                ));
                *compare_profile_options_handle.borrow_mut() = discover_compare_profile_options(
                    save_path.as_path(),
                    profile_options_handle.borrow().as_slice(),
                );
                window.set_compare_profile_options(compare_profile_options_model(
                    compare_profile_options_handle.borrow().as_slice(),
                ));
                window.set_selected_compare_profile_index(0);
                window.set_compare_result(SharedString::from(if window.get_compare_show_all() {
                    "Pick a profile and run A/B diff. Showing all fields."
                } else {
                    "Pick a profile and run A/B diff. Showing changed fields only."
                }));
                window.set_selected_profile_index(profile_option_index(
                    save_path.as_path(),
                    profile_options_handle.borrow().as_slice(),
                ));

                let auto_heal_notice = match auto_heal_profile_runtime_controllers(
                    save_path.as_path(),
                    seed_config_path_handle.as_deref(),
                ) {
                    Ok(true) => Some(" Runtime routing was auto-embedded."),
                    Ok(false) => None,
                    Err(error) => {
                        window.set_status_message(SharedString::from(format!(
                            "Profile save completed, but auto-heal failed: {error}"
                        )));
                        return;
                    }
                };
                refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
                refresh_runtime_routing_preflight(
                    &window,
                    save_path.as_path(),
                    seed_config_path_handle.as_deref(),
                );

                let bridge_running = bridge_controller_handle.borrow().is_running();
                let message = if bridge_running {
                    if window.get_hot_reload_enabled() {
                        "Saved. Bridge will reload."
                    } else {
                        "Saved. Restart bridge to apply."
                    }
                } else {
                    "Profile saved."
                };
                window.set_status_message(SharedString::from(format!(
                    "{}{}",
                    message,
                    auto_heal_notice.unwrap_or("")
                )));
            }
            Err(error) => {
                window.set_status_message(SharedString::from(format!("Save failed: {error}")));
            }
        }
    });

    let weak_window = window.as_weak();
    let profile_state_handle = Rc::clone(&profile_state);
    let path_handle = Rc::clone(&path);
    let device_catalog_handle = Rc::clone(&device_catalog);
    let seed_config_path_handle = seed_config_path.clone();
    window.on_reset_defaults_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let default_profile =
            match load_default_profile_template(seed_config_path_handle.as_deref()) {
                Ok(profile) => profile,
                Err(error) => {
                    window.set_status_message(SharedString::from(format!(
                        "Reset defaults failed: {error}"
                    )));
                    return;
                }
            };

        let mut next = profile_state_handle.borrow().clone();
        next.poll_ms = default_profile.poll_ms;
        next.ffb_parameters = default_profile.ffb_parameters;
        *profile_state_handle.borrow_mut() = next.clone();

        ProfileEditorState::from(&next).apply_to_window(&window, path_handle.borrow().as_path());
        refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
        refresh_runtime_routing_preflight(
            &window,
            path_handle.borrow().as_path(),
            seed_config_path_handle.as_deref(),
        );
        window.set_status_message(SharedString::from(
            "Reset to defaults loaded baseline tuning. Save Profile to persist.",
        ));
    });

    let weak_window = window.as_weak();
    let profile_state_handle = Rc::clone(&profile_state);
    let path_handle = Rc::clone(&path);
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let device_catalog_handle = Rc::clone(&device_catalog);
    let observability_handle = Rc::clone(&observability);
    let profile_options_handle = Rc::clone(&profile_options);
    let compare_profile_options_handle = Rc::clone(&compare_profile_options);
    let seed_config_path_handle = seed_config_path.clone();
    window.on_start_bridge_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let current_path = path_handle.borrow().clone();
        let save_path = derive_profile_path_from_window_name(
            current_path.as_path(),
            &window.get_profile_name().to_string(),
        );
        if let Err(error) = save_window_profile(
            &window,
            &profile_state_handle,
            save_path.as_path(),
            seed_config_path_handle.as_deref(),
        ) {
            window.set_status_message(SharedString::from(format!("Save failed: {error}")));
            return;
        }

        *path_handle.borrow_mut() = save_path.clone();
        bridge_controller_handle
            .borrow_mut()
            .set_profile_path(save_path.clone());
        window.set_profile_path(SharedString::from(save_path.display().to_string()));
        *profile_options_handle.borrow_mut() = discover_profile_options();
        window.set_profile_options(profile_options_model(
            profile_options_handle.borrow().as_slice(),
        ));
        *compare_profile_options_handle.borrow_mut() = discover_compare_profile_options(
            save_path.as_path(),
            profile_options_handle.borrow().as_slice(),
        );
        window.set_compare_profile_options(compare_profile_options_model(
            compare_profile_options_handle.borrow().as_slice(),
        ));
        window.set_selected_compare_profile_index(0);
        window.set_compare_result(SharedString::from(if window.get_compare_show_all() {
            "Pick a profile and run A/B diff. Showing all fields."
        } else {
            "Pick a profile and run A/B diff. Showing changed fields only."
        }));
        window.set_selected_profile_index(profile_option_index(
            save_path.as_path(),
            profile_options_handle.borrow().as_slice(),
        ));

        match auto_heal_profile_runtime_controllers(
            save_path.as_path(),
            seed_config_path_handle.as_deref(),
        ) {
            Ok(true) => {
                window.set_status_message(SharedString::from(
                    "Profile auto-healed with runtime controllers from matching routing source.",
                ));
            }
            Ok(false) => {}
            Err(error) => {
                window.set_status_message(SharedString::from(format!(
                    "Profile auto-heal failed: {error}"
                )));
                return;
            }
        }

        if let Err(error) = validate_profile_launch_readiness(
            save_path.as_path(),
            seed_config_path_handle.as_deref(),
        ) {
            window.set_status_message(SharedString::from(format!(
                "Bridge launch blocked: {error}"
            )));
            refresh_runtime_routing_preflight(
                &window,
                save_path.as_path(),
                seed_config_path_handle.as_deref(),
            );
            return;
        }

        refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
        refresh_runtime_routing_preflight(
            &window,
            save_path.as_path(),
            seed_config_path_handle.as_deref(),
        );

        let startup_snapshot = StartupSnapshot {
            profile_path: normalize_workspace_path_display(save_path.as_path()),
            routing_status: window.get_runtime_routing_status().to_string(),
            routing_source: window.get_runtime_routing_source().to_string(),
            preflight_status: window.get_runtime_preflight_status().to_string(),
            selected_input: window.get_selected_steering_device_label().to_string(),
            poll_ms: format!("{} ms", window.get_poll_ms().round() as i32),
            hot_reload: if window.get_hot_reload_enabled() {
                "enabled".to_string()
            } else {
                "disabled".to_string()
            },
        };

        let mut bridge_controller = bridge_controller_handle.borrow_mut();
        match bridge_controller.start(window.get_hot_reload_enabled(), &startup_snapshot) {
            Ok(()) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(if window.get_hot_reload_enabled() {
                    "Bridge started. Hot reload on."
                } else {
                    "Bridge started. Hot reload off."
                }));
            }
            Err(error) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!(
                    "Bridge launch failed: {error}"
                )));
            }
        }
    });

    let weak_window = window.as_weak();
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let observability_handle = Rc::clone(&observability);
    window.on_stop_bridge_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let mut bridge_controller = bridge_controller_handle.borrow_mut();
        match bridge_controller.stop() {
            Ok(()) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from("Bridge stopped."));
            }
            Err(error) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window
                    .set_status_message(SharedString::from(format!("Bridge stop failed: {error}")));
            }
        }
    });

    let weak_window = window.as_weak();
    let device_catalog_handle = Rc::clone(&device_catalog);
    let path_handle = Rc::clone(&path);
    let seed_config_path_handle = seed_config_path.clone();
    window.on_select_steering_device_requested(move |index| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        match device_catalog_handle.device_id_for_index(index) {
            Some(device_id) => {
                window.set_steering_device(device_id as f32);
                refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
                refresh_runtime_routing_preflight(
                    &window,
                    path_handle.borrow().as_path(),
                    seed_config_path_handle.as_deref(),
                );
                window.set_status_message(SharedString::from(format!(
                    "Selected {}.",
                    device_catalog_handle.label_for_device(Some(device_id))
                )));
            }
            None => {
                window.set_steering_device(-1.0);
                refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
                refresh_runtime_routing_preflight(
                    &window,
                    path_handle.borrow().as_path(),
                    seed_config_path_handle.as_deref(),
                );
                window.set_status_message(SharedString::from("Steering input cleared."));
            }
        }
    });

    let weak_window = window.as_weak();
    let device_catalog_handle = Rc::clone(&device_catalog);
    let path_handle = Rc::clone(&path);
    let seed_config_path_handle = seed_config_path.clone();
    window.on_clear_steering_device_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        window.set_steering_device(-1.0);
        refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
        refresh_runtime_routing_preflight(
            &window,
            path_handle.borrow().as_path(),
            seed_config_path_handle.as_deref(),
        );
        window.set_status_message(SharedString::from("Steering input cleared."));
    });

    let weak_window = window.as_weak();
    let path_handle = Rc::clone(&path);
    let seed_config_path_handle = seed_config_path.clone();
    window.on_install_vjoy_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let status_message = install_vjoy_dependency(DEFAULT_VJOY_DEVICE_ID);
        window.set_status_message(SharedString::from(status_message));
        refresh_runtime_routing_preflight(
            &window,
            path_handle.borrow().as_path(),
            seed_config_path_handle.as_deref(),
        );
    });

    let weak_window = window.as_weak();
    window.on_open_vjoy_config_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        window.set_status_message(SharedString::from(open_vjoy_configuration()));
    });

    let weak_window = window.as_weak();
    window.on_select_compare_profile_requested(move |index| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };
        window.set_selected_compare_profile_index(index.max(0));
    });

    let weak_window = window.as_weak();
    let path_handle = Rc::clone(&path);
    let compare_profile_options_handle = Rc::clone(&compare_profile_options);
    window.on_toggle_compare_show_all_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let show_all = !window.get_compare_show_all();
        window.set_compare_show_all(show_all);
        let mode_label = if show_all {
            "all fields"
        } else {
            "changed fields only"
        };

        let selected = window.get_selected_compare_profile_index();
        if let Some(compare_path) = compare_profile_option_path_for_index(
            selected,
            compare_profile_options_handle.borrow().as_slice(),
        ) {
            let current_profile = path_handle.borrow().clone();
            match profile_compare_report(
                current_profile.as_path(),
                compare_path.as_path(),
                show_all,
            ) {
                Ok(report) => {
                    window.set_compare_result(SharedString::from(report));
                    window.set_status_message(SharedString::from(format!(
                        "Compare view switched to {mode_label}."
                    )));
                }
                Err(error) => {
                    window.set_compare_result(SharedString::from(format!(
                        "A/B diff failed: {error}"
                    )));
                    window.set_status_message(SharedString::from(format!(
                        "A/B diff failed: {error}"
                    )));
                }
            }
        } else {
            window.set_compare_result(SharedString::from(format!(
                "Pick a profile and run A/B diff. Showing {mode_label}."
            )));
            window.set_status_message(SharedString::from(format!(
                "Compare view switched to {mode_label}."
            )));
        }
    });

    let weak_window = window.as_weak();
    let path_handle = Rc::clone(&path);
    let compare_profile_options_handle = Rc::clone(&compare_profile_options);
    window.on_run_profile_compare_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let selected = window.get_selected_compare_profile_index();
        let Some(compare_path) = compare_profile_option_path_for_index(
            selected,
            compare_profile_options_handle.borrow().as_slice(),
        ) else {
            window.set_compare_result(SharedString::from(
                "Choose a profile in Compare before running A/B diff.",
            ));
            return;
        };

        let current_profile = path_handle.borrow().clone();
        match profile_compare_report(
            current_profile.as_path(),
            compare_path.as_path(),
            window.get_compare_show_all(),
        ) {
            Ok(report) => {
                window.set_compare_result(SharedString::from(report));
                window.set_status_message(SharedString::from(if window.get_compare_show_all() {
                    "A/B profile diff updated (all fields)."
                } else {
                    "A/B profile diff updated (changed fields only)."
                }));
            }
            Err(error) => {
                window.set_compare_result(SharedString::from(format!("A/B diff failed: {error}")));
                window.set_status_message(SharedString::from(format!("A/B diff failed: {error}")));
            }
        }
    });

    let weak_window = window.as_weak();
    let observability_handle = Rc::clone(&observability);
    window.on_export_session_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        match observability_handle.borrow().export_live_session() {
            Ok(path) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_diagnostics_last_session_export(SharedString::from(format!(
                    "Session export: {}",
                    path.display()
                )));
                window.set_diagnostics_workflow_hint(SharedString::from(
                    "Session exported. Run Replay Latest Export to inspect it, then Return To Live.",
                ));
                window.set_status_message(SharedString::from(format!(
                    "Exported session to {}.",
                    path.display()
                )));
            }
            Err(error) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_diagnostics_workflow_hint(SharedString::from(
                    "Session export failed. Verify diagnostics logging and retry.",
                ));
                window.set_status_message(SharedString::from(format!(
                    "Session export failed: {error}"
                )));
            }
        }
    });

    let weak_window = window.as_weak();
    let diagnostics_log_handle = Rc::clone(&diagnostics_log);
    let path_handle = Rc::clone(&path);
    window.on_export_bundle_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let active_profile_path = path_handle.borrow().clone();
        match export_debug_bundle(
            active_profile_path.as_path(),
            diagnostics_log_handle.as_ref(),
            &window.get_runtime_routing_status().to_string(),
            &window.get_runtime_preflight_status().to_string(),
        ) {
            Ok(path) => {
                window.set_diagnostics_last_bundle_export(SharedString::from(format!(
                    "Debug bundle: {}",
                    path.display()
                )));
                window.set_diagnostics_workflow_hint(SharedString::from(
                    "Debug bundle exported. Attach it with the matching session export for bug reports.",
                ));
                window.set_status_message(SharedString::from(format!(
                    "Exported debug bundle to {}.",
                    path.display()
                )));
            }
            Err(error) => {
                window.set_diagnostics_workflow_hint(SharedString::from(
                    "Debug bundle export failed. Check profile path and diagnostics log availability.",
                ));
                window.set_status_message(SharedString::from(format!(
                    "Debug bundle export failed: {error}"
                )));
            }
        }
    });

    let weak_window = window.as_weak();
    let observability_handle = Rc::clone(&observability);
    window.on_replay_latest_session_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        match observability_handle.borrow_mut().replay_latest_session() {
            Ok(path) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_diagnostics_workflow_hint(SharedString::from(
                    "Replay active. Compare telemetry/diagnostics, then use Return To Live.",
                ));
                window.set_status_message(SharedString::from(format!(
                    "Replaying exported session {}.",
                    path.display()
                )));
            }
            Err(error) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_diagnostics_workflow_hint(SharedString::from(
                    "Replay failed. Export a session first, then retry Replay Latest Export.",
                ));
                window.set_status_message(SharedString::from(format!("Replay failed: {error}")));
            }
        }
    });

    let weak_window = window.as_weak();
    let observability_handle = Rc::clone(&observability);
    window.on_return_to_live_session_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let returned = observability_handle.borrow_mut().return_to_live();
        refresh_diagnostics_panel(&window, &observability_handle.borrow());
        window.set_diagnostics_workflow_hint(SharedString::from(if returned {
            "Back on live diagnostics stream. Export again whenever you want a fresh comparison."
        } else {
            "Already on live diagnostics stream. Export session when you need a replay baseline."
        }));
        window.set_status_message(SharedString::from(if returned {
            "Returned to the live session log."
        } else {
            "Already viewing the live session log."
        }));
    });

    let weak_window = window.as_weak();
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let device_catalog_handle = Rc::clone(&device_catalog);
    let editor_settings_handle = Rc::clone(&editor_settings);
    let settings_path_handle = Rc::clone(&settings_path);
    window.on_toggle_hot_reload_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let enabled = !window.get_hot_reload_enabled();
        window.set_hot_reload_enabled(enabled);

        if let Err(error) = save_window_editor_settings(
            &window,
            &editor_settings_handle,
            settings_path_handle.as_path(),
        ) {
            window.set_hot_reload_enabled(!enabled);
            refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
            window.set_status_message(SharedString::from(format!(
                "Failed to save settings: {error}"
            )));
            return;
        }

        refresh_runtime_summaries(&window, device_catalog_handle.as_ref());

        let message = if bridge_controller_handle.borrow().is_running() {
            if enabled {
                "Hot reload on. Restart bridge."
            } else {
                "Hot reload off. Restart bridge."
            }
        } else if enabled {
            "Hot reload on."
        } else {
            "Hot reload off."
        };
        window.set_status_message(SharedString::from(message));
    });

    let weak_window = window.as_weak();
    let editor_settings_handle = Rc::clone(&editor_settings);
    let settings_path_handle = Rc::clone(&settings_path);
    window.on_toggle_close_to_taskbar_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let enabled = !window.get_close_to_taskbar_enabled();
        window.set_close_to_taskbar_enabled(enabled);

        if let Err(error) = save_window_editor_settings(
            &window,
            &editor_settings_handle,
            settings_path_handle.as_path(),
        ) {
            window.set_close_to_taskbar_enabled(!enabled);
            window.set_status_message(SharedString::from(format!(
                "Failed to save settings: {error}"
            )));
            return;
        }

        window.set_status_message(SharedString::from(if enabled {
            "Close now minimizes to taskbar."
        } else {
            "Close now exits Torquebridge."
        }));
    });

    let weak_window = window.as_weak();
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let observability_handle = Rc::clone(&observability);
    window.on_quit_and_shutdown_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let stop_result = {
            let mut bridge_controller = bridge_controller_handle.borrow_mut();
            let result = bridge_controller.stop();
            sync_bridge_panel(&window, &bridge_controller);
            result
        };

        match stop_result {
            Ok(()) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from("Shutting down Torquebridge."));
                let _ = window.hide();
            }
            Err(error) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!("Shutdown failed: {error}")));
            }
        }
    });

    let bridge_poll_timer = Timer::default();
    let weak_window = window.as_weak();
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let observability_handle = Rc::clone(&observability);
    bridge_poll_timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let mut bridge_controller = bridge_controller_handle.borrow_mut();
        match bridge_controller.poll() {
            Ok(Some(message)) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(message));
            }
            Ok(None) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
            }
            Err(error) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!(
                    "Bridge runtime check failed: {error}"
                )));
            }
        }
    });

    let result = window.run().context("profile editor exited with an error");

    if let Err(error) = bridge_controller.borrow_mut().stop() {
        eprintln!("failed to stop bridge while closing editor: {error}");
    }

    result
}

fn load_profile_for_editor(
    config_path: Option<&Path>,
    profile_path: Option<&str>,
) -> Result<(PathBuf, FfbProfile)> {
    let target_path = resolve_profile_target_path(profile_path)?;

    let profile = if target_path.exists() {
        let mut loaded = load_profile(&target_path)?;
        if loaded
            .runtime_controllers
            .as_ref()
            .map_or(true, |controllers| controllers.is_empty())
        {
            if let Some(config_path) = config_path {
                if let Ok(controllers) = load_controllers(config_path) {
                    loaded.runtime_controllers = Some(controllers);
                }
            }
        }
        loaded
    } else {
        if let Some(config_path) = config_path {
            let controllers = load_controllers(config_path)?;
            let mut profile = FfbProfile::from_ffb_settings(load_ffb_settings(&controllers)?);
            profile.runtime_controllers = Some(controllers);
            profile
        } else {
            return Err(anyhow!(
                "profile {} does not exist yet and no config path was provided to seed runtime controllers",
                target_path.display()
            ));
        }
    };

    Ok((target_path, profile))
}

fn resolve_profile_target_path(profile_path: Option<&str>) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    let profiles_dir = cwd.join("profiles");

    let Some(raw_path) = profile_path else {
        return Ok(profiles_dir.join("baseline.json"));
    };

    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Ok(profiles_dir.join("baseline.json"));
    }

    let normalized = if Path::new(trimmed).extension().is_some() {
        trimmed.to_string()
    } else {
        format!("{trimmed}.json")
    };

    let candidate = PathBuf::from(&normalized);
    if candidate.is_absolute() {
        return Ok(candidate);
    }

    if candidate.components().count() == 1 {
        Ok(profiles_dir.join(candidate))
    } else {
        Ok(cwd.join(candidate))
    }
}

fn load_ffb_settings(controllers: &[ControllerConfig]) -> Result<FfbParamsConfig> {
    controllers
        .iter()
        .find_map(|controller| controller.ffb_parameters.clone())
        .ok_or_else(|| anyhow!("no FFBParameters entry found in config"))
}

fn load_default_profile_template(seed_config_path: Option<&Path>) -> Result<FfbProfile> {
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    let baseline_path = cwd.join("profiles").join("baseline.json");
    if baseline_path.exists() {
        return load_profile(&baseline_path).with_context(|| {
            format!(
                "failed to load baseline profile {}",
                baseline_path.display()
            )
        });
    }

    if let Some(config_path) = seed_config_path {
        let controllers = load_controllers(config_path)?;
        let mut profile = FfbProfile::from_ffb_settings(load_ffb_settings(&controllers)?);
        profile.runtime_controllers = Some(controllers);
        return Ok(profile);
    }

    Err(anyhow!(
        "baseline defaults are unavailable (missing profiles/baseline.json and no --config seed)"
    ))
}

fn save_window_profile(
    window: &ProfileEditorWindow,
    profile_state: &RefCell<FfbProfile>,
    path: &Path,
    seed_config_path: Option<&Path>,
) -> Result<FfbProfile> {
    let mut updated = {
        let current = profile_state.borrow();
        ProfileEditorState::from_window(window).into_profile(&current)
    };

    if updated.steering_device.is_some()
        && updated
            .runtime_controllers
            .as_ref()
            .map_or(true, |controllers| controllers.is_empty())
    {
        if let Some(controllers) =
            discover_runtime_controllers_from_profiles(updated.steering_device, Some(path))
        {
            updated.runtime_controllers = Some(controllers);
        } else if let Some(seed_config_path) = seed_config_path {
            if let Ok(controllers) = load_controllers(seed_config_path) {
                updated.runtime_controllers = Some(controllers);
            }
        }
    }
    updated.runtime_config_path = None;

    save_profile(path, &updated)?;
    *profile_state.borrow_mut() = updated.clone();
    Ok(updated)
}

fn auto_heal_profile_runtime_controllers(
    profile_path: &Path,
    seed_config_path: Option<&Path>,
) -> Result<bool> {
    let mut profile = load_profile(profile_path)
        .with_context(|| format!("failed to load profile {}", profile_path.display()))?;

    let has_controllers = profile
        .runtime_controllers
        .as_ref()
        .map(|controllers| !controllers.is_empty())
        .unwrap_or(false);
    if has_controllers {
        return Ok(false);
    }

    if profile.steering_device.is_none() {
        return Ok(false);
    }

    let recovered =
        discover_runtime_controllers_from_profiles(profile.steering_device, Some(profile_path))
            .or_else(|| seed_config_path.and_then(|path| load_controllers(path).ok()));

    let Some(controllers) = recovered else {
        return Ok(false);
    };

    profile.runtime_controllers = Some(controllers);
    profile.runtime_config_path = None;
    save_profile(profile_path, &profile)
        .with_context(|| format!("failed to save healed profile {}", profile_path.display()))?;
    Ok(true)
}

fn validate_profile_launch_readiness(
    profile_path: &Path,
    seed_config_path: Option<&Path>,
) -> Result<()> {
    let report = analyze_launch_readiness(profile_path, seed_config_path);
    if report.is_blocked {
        Err(anyhow!(report.message))
    } else {
        Ok(())
    }
}

fn refresh_runtime_summaries(window: &ProfileEditorWindow, device_catalog: &DeviceCatalog) {
    let steering_summary = device_catalog.label_for_device(selected_steering_device(window));
    window.set_runtime_poll_summary(SharedString::from(format!(
        "{} ms",
        window.get_poll_ms().round() as i32
    )));
    window.set_selected_steering_device_index(
        device_catalog.selected_index(selected_steering_device(window)),
    );
    window.set_selected_steering_device_label(SharedString::from(steering_summary.clone()));
    window.set_steering_device_summary(SharedString::from(steering_summary));
    window.set_hot_reload_summary(SharedString::from(hot_reload_summary(
        window.get_hot_reload_enabled(),
    )));
}

fn refresh_runtime_routing_preflight(
    window: &ProfileEditorWindow,
    profile_path: &Path,
    seed_config_path: Option<&Path>,
) {
    let analysis = runtime_routing_analysis(profile_path, seed_config_path);
    window.set_runtime_routing_status(SharedString::from(analysis.status_line));
    window.set_runtime_routing_source(SharedString::from(analysis.source_line));

    let readiness = analyze_launch_readiness(profile_path, seed_config_path);
    let preflight = if readiness.is_blocked {
        format!("Preflight: Blocked ({})", readiness.message)
    } else if let Some(warning) = readiness.warning {
        format!("Preflight: Ready with warning ({warning})")
    } else {
        "Preflight: Ready to launch.".to_string()
    };
    window.set_runtime_preflight_status(SharedString::from(preflight));
}

#[derive(Debug, Clone)]
struct LaunchReadiness {
    is_blocked: bool,
    message: String,
    warning: Option<String>,
}

fn analyze_launch_readiness(
    profile_path: &Path,
    seed_config_path: Option<&Path>,
) -> LaunchReadiness {
    if let Some(issue) = vjoy_runtime_issue_nonblocking(DEFAULT_VJOY_DEVICE_ID) {
        return LaunchReadiness {
            is_blocked: true,
            message: issue,
            warning: None,
        };
    }

    let profile = match load_profile(profile_path) {
        Ok(profile) => profile,
        Err(error) => {
            return LaunchReadiness {
                is_blocked: true,
                message: format!(
                    "profile {} is unreadable: {}",
                    normalize_workspace_path_display(profile_path),
                    error
                ),
                warning: None,
            };
        }
    };

    if profile.steering_device.is_none() {
        return LaunchReadiness {
            is_blocked: true,
            message: "no steering input is assigned in the profile".to_string(),
            warning: None,
        };
    }

    let resolution = resolve_runtime_routing(profile_path, seed_config_path);
    if matches!(resolution.source, RuntimeRoutingSource::EmbeddedProfile) {
        return LaunchReadiness {
            is_blocked: false,
            message: "ready".to_string(),
            warning: None,
        };
    }

    match resolution.source {
        RuntimeRoutingSource::MatchingProfile(path) => {
            return LaunchReadiness {
                is_blocked: false,
                message: "ready".to_string(),
                warning: Some(format!(
                    "runtime routing will be recovered from matching profile {}",
                    normalize_workspace_path_display(path.as_path())
                )),
            };
        }
        RuntimeRoutingSource::FallbackConfig(path) => {
            return LaunchReadiness {
                is_blocked: false,
                message: "ready".to_string(),
                warning: Some(format!(
                    "runtime routing depends on fallback config {}",
                    normalize_workspace_path_display(path.as_path())
                )),
            };
        }
        RuntimeRoutingSource::Unreadable(error) => {
            return LaunchReadiness {
                is_blocked: true,
                message: format!(
                    "profile {} is unreadable: {}",
                    normalize_workspace_path_display(profile_path),
                    error
                ),
                warning: None,
            };
        }
        RuntimeRoutingSource::EmbeddedProfile | RuntimeRoutingSource::Missing => {}
    }

    LaunchReadiness {
        is_blocked: true,
        message: format!(
            "profile {} is missing runtime controller routing; save after selecting a steering device or provide --config for first-time seeding",
            normalize_workspace_path_display(profile_path)
        ),
        warning: None,
    }
}

fn vjoy_interface_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("vJoyInterface.dll"),
        PathBuf::from(".\\vJoyInterface.dll"),
        PathBuf::from("C:\\Windows\\System32\\vJoyInterface.dll"),
    ];

    for program_files in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(program_files) {
            let root = PathBuf::from(root);
            candidates.push(root.join("vJoy").join("x64").join("vJoyInterface.dll"));
            candidates.push(root.join("vJoy").join("x86").join("vJoyInterface.dll"));
            candidates.push(root.join("vJoy").join("vJoyInterface.dll"));
        }
    }

    candidates
}

fn vjoy_runtime_issue_nonblocking(device_id: u32) -> Option<String> {
    let has_interface = vjoy_interface_candidates()
        .into_iter()
        .any(|candidate| candidate.exists());

    if !has_interface {
        return Some(
            "vJoy is not installed. Click Install vJoy, then open vJoyConf and configure device 1."
                .to_string(),
        );
    }

    let _ = device_id;
    None
}

fn install_vjoy_dependency(device_id: u32) -> String {
    if vjoy_runtime_issue_nonblocking(device_id).is_none() {
        return "vJoy is already installed and ready.".to_string();
    }

    #[cfg(windows)]
    {
        let output = Command::new("winget")
            .args([
                "install",
                "--id",
                "ShaulEizikovich.vJoy",
                "-e",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let output = match output {
            Ok(output) => output,
            Err(_) => {
                return "Automatic install unavailable: winget was not found. Install vJoy manually, then restart Torquebridge."
                    .to_string();
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("exit code {}", output.status)
            };

            return format!(
                "Automatic vJoy install failed ({detail}). Install vJoy manually, then restart Torquebridge."
            );
        }

        if let Some(issue) = vjoy_runtime_issue_nonblocking(device_id) {
            return format!("Install command completed, but vJoy still needs setup: {issue}");
        }

        "vJoy install completed and prerequisite checks are passing. You can launch the bridge now."
            .to_string()
    }

    #[cfg(not(windows))]
    {
        "Automatic vJoy install is only supported on Windows. Install vJoy manually, then retry."
            .to_string()
    }
}

fn open_vjoy_configuration() -> String {
    #[cfg(windows)]
    {
        fn launch_elevated(file_path: &str) -> Result<(), String> {
            let escaped = file_path.replace('\'', "''");
            let command = format!("Start-Process -FilePath '{escaped}' -Verb RunAs");

            let status = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    &command,
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status()
                .map_err(|error| format!("failed to invoke elevation prompt: {error}"))?;

            if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "elevation request was declined or failed ({status})"
                ))
            }
        }

        let mut candidates = vec![
            PathBuf::from("C:\\Program Files\\vJoy\\x64\\vJoyConf.exe"),
            PathBuf::from("C:\\Program Files (x86)\\vJoy\\x86\\vJoyConf.exe"),
            PathBuf::from("C:\\Program Files\\vJoy\\vJoyConf.exe"),
            PathBuf::from("C:\\Program Files (x86)\\vJoy\\vJoyConf.exe"),
        ];

        for program_files in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(program_files) {
                let root = PathBuf::from(root);
                candidates.push(root.join("vJoy").join("x64").join("vJoyConf.exe"));
                candidates.push(root.join("vJoy").join("x86").join("vJoyConf.exe"));
                candidates.push(root.join("vJoy").join("vJoyConf.exe"));
            }
        }

        for candidate in candidates {
            if candidate.exists() {
                let candidate_path = candidate.to_string_lossy().to_string();
                match launch_elevated(candidate_path.as_str()) {
                    Ok(_) => {
                        return "Requested elevated vJoyConf launch. Accept UAC, configure or enable device 1, then return and launch the bridge."
                            .to_string();
                    }
                    Err(error) => {
                        return format!(
                            "Found vJoyConf but could not request elevation ({error}). Run vJoyConf as administrator and configure device 1."
                        );
                    }
                }
            }
        }

        match launch_elevated("vJoyConf.exe") {
            Ok(_) => {
                "Requested elevated vJoyConf launch. Accept UAC, configure or enable device 1, then return and launch the bridge."
                    .to_string()
            }
            Err(error) => {
                format!(
                    "Could not locate vJoyConf or request elevation ({error}). Install or repair vJoy, then run vJoyConf as administrator manually."
                )
            }
        }
    }

    #[cfg(not(windows))]
    {
        "vJoy configuration is only available on Windows.".to_string()
    }
}

#[derive(Debug, Clone)]
struct RuntimeRoutingAnalysis {
    status_line: String,
    source_line: String,
}

#[derive(Debug, Clone)]
enum RuntimeRoutingSource {
    EmbeddedProfile,
    MatchingProfile(PathBuf),
    FallbackConfig(PathBuf),
    Missing,
    Unreadable(String),
}

#[derive(Debug, Clone)]
struct RuntimeRoutingResolution {
    source: RuntimeRoutingSource,
    controllers: Option<Vec<ControllerConfig>>,
}

fn resolve_runtime_routing(
    profile_path: &Path,
    seed_config_path: Option<&Path>,
) -> RuntimeRoutingResolution {
    let profile = match load_profile(profile_path) {
        Ok(profile) => profile,
        Err(error) => {
            return RuntimeRoutingResolution {
                source: RuntimeRoutingSource::Unreadable(error.to_string()),
                controllers: None,
            };
        }
    };

    if let Some(controllers) = profile.runtime_controllers {
        if !controllers.is_empty() {
            return RuntimeRoutingResolution {
                source: RuntimeRoutingSource::EmbeddedProfile,
                controllers: Some(controllers),
            };
        }
    }

    if let Some(path) =
        discover_matching_runtime_profile_path(profile.steering_device, Some(profile_path))
    {
        if let Ok(matched_profile) = load_profile(path.as_path()) {
            if let Some(controllers) = matched_profile.runtime_controllers {
                if !controllers.is_empty() {
                    return RuntimeRoutingResolution {
                        source: RuntimeRoutingSource::MatchingProfile(path),
                        controllers: Some(controllers),
                    };
                }
            }
        }
    }

    if let Some(path) = seed_config_path {
        if let Ok(controllers) = load_controllers(path) {
            if !controllers.is_empty() {
                return RuntimeRoutingResolution {
                    source: RuntimeRoutingSource::FallbackConfig(path.to_path_buf()),
                    controllers: Some(controllers),
                };
            }
        }
    }

    RuntimeRoutingResolution {
        source: RuntimeRoutingSource::Missing,
        controllers: None,
    }
}

fn runtime_routing_analysis(
    profile_path: &Path,
    seed_config_path: Option<&Path>,
) -> RuntimeRoutingAnalysis {
    let resolution = resolve_runtime_routing(profile_path, seed_config_path);
    match resolution.source {
        RuntimeRoutingSource::EmbeddedProfile => RuntimeRoutingAnalysis {
            status_line: "Routing: Embedded runtime controllers present in selected profile."
                .to_string(),
            source_line: "Routing source: Embedded profile runtime_controllers.".to_string(),
        },
        RuntimeRoutingSource::MatchingProfile(path) => RuntimeRoutingAnalysis {
            status_line: format!(
                "Routing: Will recover from matching profile {}.",
                normalize_workspace_path_display(path.as_path())
            ),
            source_line: format!(
                "Routing source: Matching profile {}.",
                normalize_workspace_path_display(path.as_path())
            ),
        },
        RuntimeRoutingSource::FallbackConfig(path) => RuntimeRoutingAnalysis {
            status_line: format!(
                "Routing: Will fall back to config {}.",
                normalize_workspace_path_display(path.as_path())
            ),
            source_line: format!(
                "Routing source: Fallback config {}.",
                normalize_workspace_path_display(path.as_path())
            ),
        },
        RuntimeRoutingSource::Unreadable(_) => RuntimeRoutingAnalysis {
            status_line: format!(
                "Routing: Profile {} is unreadable.",
                normalize_workspace_path_display(profile_path)
            ),
            source_line: "Routing source: Unreadable profile.".to_string(),
        },
        RuntimeRoutingSource::Missing => RuntimeRoutingAnalysis {
            status_line: "Routing: Missing runtime controllers and no fallback config source."
                .to_string(),
            source_line: "Routing source: None available.".to_string(),
        },
    }
}

fn export_debug_bundle(
    profile_path: &Path,
    diagnostics_log: &DiagnosticsLog,
    routing_status: &str,
    preflight_status: &str,
) -> Result<PathBuf> {
    let bundle_dir = diagnostics_log
        .archive_dir()
        .join(format!("bundle-{}", unix_timestamp_ms()));
    fs::create_dir_all(&bundle_dir)
        .with_context(|| format!("failed to create debug bundle dir {}", bundle_dir.display()))?;

    let profile_copy = bundle_dir.join("profile.json");
    fs::copy(profile_path, &profile_copy).with_context(|| {
        format!(
            "failed to copy profile {} into debug bundle",
            profile_path.display()
        )
    })?;

    if diagnostics_log.path().exists() {
        let diagnostics_copy = bundle_dir.join("live-diagnostics.jsonl");
        fs::copy(diagnostics_log.path(), &diagnostics_copy).with_context(|| {
            format!(
                "failed to copy diagnostics {} into debug bundle",
                diagnostics_log.path().display()
            )
        })?;
    }

    let recent_events = diagnostics_log.read_recent(128).unwrap_or_default();
    let event_tail = if recent_events.is_empty() {
        "No diagnostics events captured yet.".to_string()
    } else {
        recent_events
            .iter()
            .map(format_event)
            .collect::<Vec<_>>()
            .join("\n")
    };
    fs::write(bundle_dir.join("event-tail.txt"), format!("{event_tail}\n"))
        .context("failed to write event tail in debug bundle")?;

    let summary = format!(
        "Torquebridge Debug Bundle\nCreated: {}\nProfile: {}\nRouting: {}\nPreflight: {}\nDiagnostics log: {}\n",
        unix_timestamp_ms(),
        profile_path.display(),
        routing_status,
        preflight_status,
        diagnostics_log.path().display()
    );
    fs::write(bundle_dir.join("summary.txt"), summary).context("failed to write bundle summary")?;

    Ok(bundle_dir)
}

fn unix_timestamp_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

fn refresh_diagnostics_panel(
    window: &ProfileEditorWindow,
    observability: &ObservabilityController,
) {
    let source_label = observability.source_label();
    let diagnostics_log = observability.current_log();

    window.set_observability_source_label(SharedString::from(source_label.clone()));
    window.set_diagnostics_log_path(SharedString::from(
        diagnostics_log.path().display().to_string(),
    ));

    match diagnostics_log.read_recent(OBSERVABILITY_EVENT_LIMIT) {
        Ok(events) => {
            apply_diagnostics_view(window, &events);
            TelemetryPanelView::from_events(&events, &source_label).apply_to_window(window);
        }
        Err(error) => {
            apply_diagnostics_error(window, &error.to_string());
            TelemetryPanelView::failed(&source_label, &error.to_string()).apply_to_window(window);
        }
    }
}

fn apply_diagnostics_view(window: &ProfileEditorWindow, events: &[BridgeDiagnosticEvent]) {
    let filtered_events = events
        .iter()
        .filter(|event| event.category != DiagnosticCategory::Telemetry)
        .collect::<Vec<_>>();
    let diagnostics_events = if filtered_events.len() > DIAGNOSTIC_EVENT_LIMIT {
        &filtered_events[filtered_events.len() - DIAGNOSTIC_EVENT_LIMIT..]
    } else {
        &filtered_events
    };

    let diagnostics_text = if diagnostics_events.is_empty() {
        "No bridge diagnostics yet. Launch the bridge to capture startup, apply, telemetry, and safety events."
            .to_string()
    } else {
        diagnostics_events
            .iter()
            .rev()
            .map(|event| format_event(event))
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    window.set_diagnostics_status(SharedString::from(if diagnostics_events.is_empty() {
        "Waiting for session"
    } else {
        "Tail updated"
    }));
    window.set_diagnostics_event_count(SharedString::from(format!(
        "{} recent event{}",
        diagnostics_events.len(),
        if diagnostics_events.len() == 1 {
            ""
        } else {
            "s"
        }
    )));
    window.set_diagnostics_events(SharedString::from(diagnostics_text));
}

fn apply_diagnostics_error(window: &ProfileEditorWindow, error: &str) {
    window.set_diagnostics_status(SharedString::from("Read failed"));
    window.set_diagnostics_event_count(SharedString::from("Diagnostics unavailable"));
    window.set_diagnostics_events(SharedString::from(format!(
        "Failed to read diagnostics log: {error}"
    )));
}

fn sync_bridge_panel(window: &ProfileEditorWindow, bridge_controller: &BridgeController) {
    window.set_bridge_running(bridge_controller.is_running());
    window.set_bridge_status(SharedString::from(bridge_controller.status_label()));
    window.set_bridge_detail(SharedString::from(bridge_controller.detail().to_string()));
}

fn apply_bridge_safety_reset(
    profile_path: &Path,
    seed_config_path: Option<&Path>,
    diagnostics_log: &DiagnosticsLog,
    phase: &str,
) -> Result<String> {
    let result = (|| -> Result<String> {
        let controllers = load_runtime_controllers_for_bridge(profile_path, seed_config_path)?;
        let direct_input = DirectInput::create()?;
        let mut output = direct_input.open_configured_ffb_device(&controllers)?;
        output.apply_commands(&[
            WheelCommand::DeviceControl(DeviceControlCommand::StopAll),
            WheelCommand::DeviceControl(DeviceControlCommand::Reset),
        ])?;

        Ok(format!(
            "Safety reset sent to '{}'.",
            output.info().instance_name,
        ))
    })();

    match result {
        Ok(detail) => {
            record_ui_event(
                diagnostics_log,
                DiagnosticLevel::Info,
                DiagnosticCategory::Safety,
                format!("{phase} safety reset applied"),
                vec![DiagnosticField::new("detail", detail.clone())],
            );
            Ok(detail)
        }
        Err(error) => {
            record_ui_event(
                diagnostics_log,
                DiagnosticLevel::Error,
                DiagnosticCategory::Error,
                format!("{phase} safety reset failed"),
                vec![DiagnosticField::new("error", error.to_string())],
            );
            Err(error)
        }
    }
}

fn load_runtime_controllers_for_bridge(
    profile_path: &Path,
    seed_config_path: Option<&Path>,
) -> Result<Vec<ControllerConfig>> {
    let resolution = resolve_runtime_routing(profile_path, seed_config_path);
    if let Some(controllers) = resolution.controllers {
        return Ok(controllers);
    }

    match resolution.source {
        RuntimeRoutingSource::Unreadable(error) => Err(anyhow!(
            "failed to load profile {}: {}",
            normalize_workspace_path_display(profile_path),
            error
        )),
        _ => Err(anyhow!(
            "profile {} has no embedded runtime controllers and no fallback config path was provided",
            normalize_workspace_path_display(profile_path)
        )),
    }
}

fn discover_runtime_controllers_from_profiles(
    steering_device: Option<u32>,
    exclude_path: Option<&Path>,
) -> Option<Vec<ControllerConfig>> {
    let profile_path = discover_matching_runtime_profile_path(steering_device, exclude_path)?;
    let profile = load_profile(&profile_path).ok()?;
    let controllers = profile.runtime_controllers?;
    if controllers.is_empty() {
        return None;
    }
    Some(controllers)
}

fn discover_matching_runtime_profile_path(
    steering_device: Option<u32>,
    exclude_path: Option<&Path>,
) -> Option<PathBuf> {
    let steering_device = steering_device?;
    let cwd = std::env::current_dir().ok()?;
    let profiles_dir = cwd.join("profiles");
    let entries = fs::read_dir(profiles_dir).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if exclude_path
            .map(|excluded| excluded == path.as_path())
            .unwrap_or(false)
        {
            continue;
        }

        let Ok(profile) = load_profile(&path) else {
            continue;
        };

        if profile.steering_device != Some(steering_device) {
            continue;
        }

        if let Some(controllers) = profile.runtime_controllers {
            if !controllers.is_empty() {
                return Some(path);
            }
        }
    }

    None
}

fn resolve_runtime_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn discover_seed_config_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("configuration.json"));
    }

    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        candidates.push(
            PathBuf::from(home)
                .join("Torquebridge")
                .join("configuration.json"),
        );
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProfileOption {
    path: PathBuf,
    label: String,
}

fn profile_options_model(options: &[ProfileOption]) -> ModelRc<SharedString> {
    let mut entries = vec![SharedString::from("Select profile")];
    entries.extend(
        options
            .iter()
            .map(|option| SharedString::from(option.label.as_str())),
    );
    ModelRc::new(VecModel::from(entries))
}

fn compare_profile_options_model(options: &[ProfileOption]) -> ModelRc<SharedString> {
    let mut entries = vec![SharedString::from("Select profile to compare")];
    entries.extend(
        options
            .iter()
            .map(|option| SharedString::from(option.label.as_str())),
    );
    ModelRc::new(VecModel::from(entries))
}

fn profile_option_index(current_path: &Path, options: &[ProfileOption]) -> i32 {
    let current = normalize_workspace_path_display(current_path);
    options
        .iter()
        .position(|entry| {
            normalize_workspace_path_display(entry.path.as_path()).eq_ignore_ascii_case(&current)
        })
        .map(|idx| idx as i32 + 1)
        .unwrap_or(0)
}

fn profile_option_path_for_index(index: i32, options: &[ProfileOption]) -> Option<PathBuf> {
    let row = usize::try_from(index).ok()?;
    if row == 0 {
        return None;
    }
    Some(options.get(row - 1)?.path.clone())
}

fn compare_profile_option_path_for_index(index: i32, options: &[ProfileOption]) -> Option<PathBuf> {
    let row = usize::try_from(index).ok()?;
    if row == 0 {
        return None;
    }
    Some(options.get(row - 1)?.path.clone())
}

fn normalize_workspace_path_display(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(relative) = path.strip_prefix(&cwd) {
            return relative.to_string_lossy().replace('\\', "/");
        }
    }
    path.to_string_lossy().replace('\\', "/")
}

fn discover_profile_options() -> Vec<ProfileOption> {
    let mut options = Vec::<ProfileOption>::new();
    let Ok(cwd) = std::env::current_dir() else {
        return options;
    };

    let profiles_dir = cwd.join("profiles");
    let Ok(entries) = fs::read_dir(&profiles_dir) else {
        return options;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(profile) = load_profile(&path) else {
            continue;
        };
        let fallback = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Unnamed Profile")
            .to_string();
        let label = profile
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string())
            .unwrap_or(fallback);
        options.push(ProfileOption { path, label });
    }

    options.sort_by(|left, right| {
        left.label
            .to_ascii_lowercase()
            .cmp(&right.label.to_ascii_lowercase())
    });
    options
}

fn discover_compare_profile_options(
    current_path: &Path,
    options: &[ProfileOption],
) -> Vec<ProfileOption> {
    let current = normalize_workspace_path_display(current_path);
    options
        .iter()
        .filter(|option| {
            !normalize_workspace_path_display(option.path.as_path()).eq_ignore_ascii_case(&current)
        })
        .cloned()
        .collect::<Vec<_>>()
}

#[derive(Debug, Clone, Default)]
struct CompareSection {
    title: &'static str,
    lines: Vec<String>,
}

impl CompareSection {
    fn new(title: &'static str) -> Self {
        Self {
            title,
            lines: Vec::new(),
        }
    }
}

fn profile_compare_report(
    primary_path: &Path,
    secondary_path: &Path,
    show_all: bool,
) -> Result<String> {
    let primary = load_profile(primary_path)
        .with_context(|| format!("failed to load profile {}", primary_path.display()))?;
    let secondary = load_profile(secondary_path)
        .with_context(|| format!("failed to load profile {}", secondary_path.display()))?;

    let mut runtime = CompareSection::new("Runtime");
    let mut constant = CompareSection::new("Constant");
    let mut periodic = CompareSection::new("Periodic");
    let mut condition = CompareSection::new("Condition");
    let mut calibration = CompareSection::new("Calibration");
    let mut changed_fields = 0usize;

    if push_value_delta(
        &mut runtime.lines,
        "Poll ms",
        primary.poll_ms.unwrap_or(5) as f32,
        secondary.poll_ms.unwrap_or(5) as f32,
        show_all,
    ) {
        changed_fields += 1;
    }

    let primary_steering = primary.steering_device.unwrap_or(u32::MAX);
    let secondary_steering = secondary.steering_device.unwrap_or(u32::MAX);
    if show_all || primary_steering != secondary_steering {
        runtime.lines.push(format!(
            "Steering device: {} -> {}",
            steering_device_label_for_report(primary.steering_device),
            steering_device_label_for_report(secondary.steering_device)
        ));
    }
    if primary_steering != secondary_steering {
        changed_fields += 1;
    }

    let primary_runtime_count = primary
        .runtime_controllers
        .as_ref()
        .map(|controllers| controllers.len())
        .unwrap_or(0);
    let secondary_runtime_count = secondary
        .runtime_controllers
        .as_ref()
        .map(|controllers| controllers.len())
        .unwrap_or(0);
    if show_all || primary_runtime_count != secondary_runtime_count {
        runtime.lines.push(format!(
            "Runtime controllers: {} -> {}",
            primary_runtime_count, secondary_runtime_count
        ));
    }
    if primary_runtime_count != secondary_runtime_count {
        changed_fields += 1;
    }

    let p = &primary.ffb_parameters;
    let s = &secondary.ffb_parameters;
    if push_value_delta(
        &mut constant.lines,
        "Const magnitude",
        p.r#const.magnitude,
        s.r#const.magnitude,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut constant.lines,
        "Const max force",
        p.r#const.maximum_force,
        s.r#const.maximum_force,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut constant.lines,
        "Const min force",
        p.r#const.minimum_force,
        s.r#const.minimum_force,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut constant.lines,
        "Const threshold",
        p.r#const.filter_threshold,
        s.r#const.filter_threshold,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut constant.lines,
        "Const min coefficient",
        p.r#const.minimum_coefficient,
        s.r#const.minimum_coefficient,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut periodic.lines,
        "Sine magnitude",
        p.sine.magnitude,
        s.sine.magnitude,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut periodic.lines,
        "Sine frequency",
        p.sine.frequency,
        s.sine.frequency,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut periodic.lines,
        "Sine max force",
        p.sine.maximum_force,
        s.sine.maximum_force,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut periodic.lines,
        "Sine phase",
        p.sine.phase,
        s.sine.phase,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut periodic.lines,
        "Engine vibration",
        p.sine.engine_vibrations.strength,
        s.sine.engine_vibrations.strength,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut periodic.lines,
        "Gear shift vibration",
        p.sine.gear_shift_vibrations.strength,
        s.sine.gear_shift_vibrations.strength,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut condition.lines,
        "Spring coefficient",
        p.spring.coefficient,
        s.spring.coefficient,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut condition.lines,
        "Spring saturation",
        p.spring.saturation,
        s.spring.saturation,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut condition.lines,
        "Damper coefficient",
        p.damper.coefficient,
        s.damper.coefficient,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut condition.lines,
        "Damper saturation",
        p.damper.saturation,
        s.damper.saturation,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut calibration.lines,
        "Cal output gain",
        p.calibration.output_gain,
        s.calibration.output_gain,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut calibration.lines,
        "Cal const gain",
        p.calibration.const_gain,
        s.calibration.const_gain,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut calibration.lines,
        "Cal periodic gain",
        p.calibration.sine_gain,
        s.calibration.sine_gain,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut calibration.lines,
        "Cal spring gain",
        p.calibration.spring_gain,
        s.calibration.spring_gain,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut calibration.lines,
        "Cal damper gain",
        p.calibration.damper_gain,
        s.calibration.damper_gain,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut calibration.lines,
        "Cal center offset",
        p.calibration.steering_center_offset,
        s.calibration.steering_center_offset,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut calibration.lines,
        "Cal steering range",
        p.calibration.steering_range,
        s.calibration.steering_range,
        show_all,
    ) {
        changed_fields += 1;
    }
    if push_value_delta(
        &mut calibration.lines,
        "Cal steering curve",
        p.calibration.steering_curve,
        s.calibration.steering_curve,
        show_all,
    ) {
        changed_fields += 1;
    }

    let sections = vec![runtime, constant, periodic, condition, calibration];

    let mut report = vec![
        format!(
            "Primary: {}",
            normalize_workspace_path_display(primary_path)
        ),
        format!(
            "Compare: {}",
            normalize_workspace_path_display(secondary_path)
        ),
        String::new(),
    ];

    if changed_fields == 0 {
        report.push("No differences found across compared runtime and force fields.".to_string());
    } else {
        if show_all {
            report.push("Mode: All fields".to_string());
        } else {
            report.push("Mode: Changed fields only".to_string());
        }
        report.push(format!("Changed fields: {}", changed_fields));
        report.push(String::new());
        for section in sections {
            if section.lines.is_empty() {
                continue;
            }
            report.push(format!("[{}]", section.title));
            report.extend(section.lines);
            report.push(String::new());
        }
    }

    Ok(report.join("\n"))
}

fn steering_device_label_for_report(device: Option<u32>) -> String {
    match device {
        Some(id) => format!("WinMM #{}", id),
        None => "None".to_string(),
    }
}

fn push_value_delta(
    lines: &mut Vec<String>,
    label: &str,
    primary: f32,
    secondary: f32,
    show_all: bool,
) -> bool {
    let changed = (primary - secondary).abs() >= 0.0001;
    if !show_all && !changed {
        return false;
    }
    let delta = secondary - primary;
    lines.push(format!(
        "{}: {:.4} -> {:.4} (delta {:+.4})",
        label, primary, secondary, delta
    ));
    changed
}

fn derive_profile_path_from_window_name(current_path: &Path, profile_name: &str) -> PathBuf {
    let file_name = profile_filename_from_name(profile_name);
    if let Some(parent) = current_path.parent() {
        return parent.join(file_name);
    }

    if let Ok(cwd) = std::env::current_dir() {
        return cwd.join("profiles").join(file_name);
    }

    PathBuf::from("profiles").join(file_name)
}

fn profile_filename_from_name(profile_name: &str) -> String {
    let mut compact = profile_name
        .trim()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();

    if compact.is_empty() {
        compact = "profile".to_string();
    }

    let sanitized = compact
        .chars()
        .map(|ch| if "<>:\"/\\|?*".contains(ch) { '-' } else { ch })
        .collect::<String>();

    format!("{sanitized}.json")
}

fn record_ui_event(
    diagnostics_log: &DiagnosticsLog,
    level: DiagnosticLevel,
    category: DiagnosticCategory,
    message: impl Into<String>,
    fields: Vec<DiagnosticField>,
) {
    if let Err(error) = diagnostics_log.append(level, category, message, fields) {
        eprintln!("failed to write UI diagnostics event: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        CalibrationConfig, ConditionFfbConfig, ConstFfbConfig, ExperimentalConfig,
        PeriodicFfbConfig, VibrationConfig,
    };

    fn device(id: u32, name: &str) -> WinmmDeviceInfo {
        WinmmDeviceInfo {
            id,
            name: name.to_string(),
            axis_count: 4,
            button_count: 8,
        }
    }

    fn profile() -> FfbProfile {
        FfbProfile {
            version: 1,
            name: Some("Baseline Compatibility Bridge".to_string()),
            notes: Some("Hot-reload tuned profile".to_string()),
            steering_device: Some(0),
            poll_ms: Some(5),
            runtime_config_path: None,
            runtime_controllers: None,
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
                calibration: CalibrationConfig {
                    preset: Some("FFBeast Precision".to_string()),
                    output_gain: 1.1,
                    steering_center_offset: 0.02,
                    steering_range: 0.9,
                    steering_curve: 1.25,
                    const_gain: 1.05,
                    sine_gain: 0.9,
                    spring_gain: 1.15,
                    damper_gain: 0.85,
                },
                experimental: ExperimentalConfig::default(),
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
        assert_eq!(restored.steering_device, original.steering_device);
        assert_eq!(restored.runtime_config_path, None);
        assert!((restored.ffb_parameters.r#const.maximum_force - 0.92).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.sine.phase - 0.375).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.spring.coefficient - 0.03).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.calibration.output_gain - 1.1).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.calibration.const_gain - 1.05).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.calibration.sine_gain - 0.9).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.calibration.spring_gain - 1.15).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.calibration.damper_gain - 0.85).abs() < f32::EPSILON);
        assert!(
            (restored.ffb_parameters.calibration.steering_center_offset - 0.02).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn editor_state_clamps_out_of_range_values() {
        let original = profile();
        let mut state = ProfileEditorState::from(&original);
        state.poll_ms = 99.0;
        state.steering_device = -4.0;
        state.const_magnitude = 4.0;
        state.damper_saturation = -2.0;
        state.calibration_output_gain = 4.0;
        state.calibration_const_gain = 4.0;
        state.calibration_sine_gain = -2.0;
        state.calibration_spring_gain = 3.2;
        state.calibration_damper_gain = -1.0;
        state.calibration_steering_center_offset = -2.0;
        state.calibration_steering_range = 0.1;
        state.calibration_steering_curve = 9.0;

        let restored = state.into_profile(&original);
        assert_eq!(restored.poll_ms, Some(20));
        assert_eq!(restored.steering_device, None);
        assert_eq!(restored.ffb_parameters.r#const.magnitude, 2.0);
        assert_eq!(restored.ffb_parameters.damper.saturation, 0.0);
        assert_eq!(restored.ffb_parameters.calibration.output_gain, 2.0);
        assert_eq!(restored.ffb_parameters.calibration.const_gain, 2.0);
        assert_eq!(restored.ffb_parameters.calibration.sine_gain, 0.0);
        assert_eq!(restored.ffb_parameters.calibration.spring_gain, 2.0);
        assert_eq!(restored.ffb_parameters.calibration.damper_gain, 0.0);
        assert_eq!(
            restored.ffb_parameters.calibration.steering_center_offset,
            -0.5
        );
        assert_eq!(restored.ffb_parameters.calibration.steering_range, 0.25);
        assert_eq!(restored.ffb_parameters.calibration.steering_curve, 3.0);
    }

    #[test]
    fn device_catalog_uses_names_for_selected_device() {
        let catalog = DeviceCatalog {
            devices: vec![device(0, "FFBeast Wheel"), device(2, "Gamepad")],
            load_error: None,
        };

        assert_eq!(catalog.selected_index(None), 0);
        assert_eq!(catalog.selected_index(Some(2)), 2);
        assert_eq!(catalog.selected_index(Some(7)), 0);
        assert_eq!(catalog.device_id_for_index(0), None);
        assert_eq!(catalog.device_id_for_index(2), Some(2));
        assert_eq!(
            catalog.label_for_device(Some(0)),
            "FFBeast Wheel [WinMM #0]"
        );
        assert_eq!(catalog.label_for_device(None), "No steering device");
        assert_eq!(hot_reload_summary(true), "On save");
        assert_eq!(hot_reload_summary(false), "Manual");
    }

    #[test]
    fn editor_settings_round_trip_json() {
        let settings = EditorSettings {
            hot_reload_enabled: false,
            close_to_taskbar_enabled: true,
            last_profile_path: Some("profiles/baseline.json".to_string()),
        };

        let json = serde_json::to_string(&settings).expect("serialize settings");
        let restored: EditorSettings = serde_json::from_str(&json).expect("deserialize settings");

        assert_eq!(restored, settings);
    }

    #[test]
    fn telemetry_panel_prefers_latest_telemetry_snapshot() {
        let older = BridgeDiagnosticEvent {
            timestamp_ms: 1,
            level: DiagnosticLevel::Info,
            category: DiagnosticCategory::Telemetry,
            message: "Runtime telemetry snapshot".to_string(),
            fields: vec![
                DiagnosticField::new("uptime", "0.5 s"),
                DiagnosticField::new("packet_rate", "1.0/s"),
                DiagnosticField::new("packet_rate_value", "1.0"),
                DiagnosticField::new("command_rate", "2.0/s"),
                DiagnosticField::new("command_rate_value", "2.0"),
                DiagnosticField::new("steering", "+0% (32768)"),
                DiagnosticField::new("peak_force", "18%"),
                DiagnosticField::new("calibration_preset", "FFBeast Precision"),
                DiagnosticField::new("calibration_output_gain", "1.10x"),
                DiagnosticField::new("filter_coefficient", "55%"),
                DiagnosticField::new("peak_force_ratio", "0.1800"),
                DiagnosticField::new("clamp_status", "Constant has headroom at 18% peak."),
                DiagnosticField::new("saturation_status", "Spring saturation is 40%."),
                DiagnosticField::new("saturation_ratio", "0.4000"),
                DiagnosticField::new("last_packet_age", "500 ms ago"),
                DiagnosticField::new("input", "Older Input"),
                DiagnosticField::new("output", "Older Output"),
                DiagnosticField::new("poll_ms", "5 ms"),
                DiagnosticField::new("hot_reload", "On save"),
                DiagnosticField::new("updates_total", "1"),
                DiagnosticField::new("commands_total", "2"),
                DiagnosticField::new("last_update", "old update"),
                DiagnosticField::new("last_commands", "old commands"),
            ],
        };
        let latest = BridgeDiagnosticEvent {
            timestamp_ms: 2,
            level: DiagnosticLevel::Info,
            category: DiagnosticCategory::Telemetry,
            message: "Runtime telemetry snapshot".to_string(),
            fields: vec![
                DiagnosticField::new("uptime", "2.0 s"),
                DiagnosticField::new("packet_rate", "8.0/s"),
                DiagnosticField::new("packet_rate_value", "8.0"),
                DiagnosticField::new("command_rate", "16.0/s"),
                DiagnosticField::new("command_rate_value", "16.0"),
                DiagnosticField::new("steering", "+12% (36600)"),
                DiagnosticField::new("peak_force", "92%"),
                DiagnosticField::new("calibration_preset", "FFBeast Drift"),
                DiagnosticField::new("calibration_output_gain", "1.25x"),
                DiagnosticField::new("filter_coefficient", "83%"),
                DiagnosticField::new("peak_force_ratio", "0.9200"),
                DiagnosticField::new("clamp_status", "Periodic is near clamp at 92%."),
                DiagnosticField::new("saturation_status", "Spring saturation is capped at 100%."),
                DiagnosticField::new("saturation_ratio", "1.0000"),
                DiagnosticField::new("last_packet_age", "80 ms ago"),
                DiagnosticField::new("input", "FFBeast Wheel [WinMM #0]"),
                DiagnosticField::new("output", "FFBeast Racing Wheel"),
                DiagnosticField::new("poll_ms", "4 ms"),
                DiagnosticField::new("hot_reload", "Manual"),
                DiagnosticField::new("updates_total", "12"),
                DiagnosticField::new("commands_total", "24"),
                DiagnosticField::new("last_update", "constant(magnitude=4000)"),
                DiagnosticField::new("last_commands", "constant(magnitude=-2500)"),
            ],
        };

        let view = TelemetryPanelView::from_events(
            &[
                BridgeDiagnosticEvent {
                    timestamp_ms: 0,
                    level: DiagnosticLevel::Info,
                    category: DiagnosticCategory::Startup,
                    message: "Startup snapshot captured".to_string(),
                    fields: vec![
                        DiagnosticField::new(
                            "profile",
                            "profiles/BaselineCompatibilityBridge.json",
                        ),
                        DiagnosticField::new(
                            "routing_source",
                            "Routing source: Embedded profile runtime_controllers.",
                        ),
                        DiagnosticField::new("preflight_status", "Preflight: Ready to launch."),
                    ],
                },
                older,
                latest,
            ],
            "Source: live session log",
        );

        assert_eq!(view.uptime, "2.0 s");
        assert_eq!(view.packet_rate, "8.0/s");
        assert_eq!(view.command_rate, "16.0/s");
        assert_eq!(view.steering, "+12% (36600)");
        assert_eq!(view.peak_force, "92%");
        assert_eq!(view.calibration_profile, "FFBeast Drift");
        assert_eq!(view.calibration_gain, "1.25x");
        assert_eq!(view.filter_coefficient, "83%");
        assert!(view.status.contains("80 ms ago"));
        assert!(view.status.contains("Source: live session log"));
        assert_eq!(view.clamp_status, "Periodic is near clamp at 92%.");
        assert_eq!(
            view.saturation_status,
            "Spring saturation is capped at 100%."
        );
        assert!(view.runtime_detail.contains("FFBeast Wheel [WinMM #0]"));
        assert!(view.runtime_detail.contains("FFBeast Racing Wheel"));
        assert!(
            view.runtime_detail
                .contains("profiles/BaselineCompatibilityBridge.json")
        );
        assert!(
            view.runtime_detail
                .contains("Embedded profile runtime_controllers")
        );
        assert!(view.runtime_detail.contains("Preflight: Ready to launch."));
        assert!(view.runtime_detail.contains("4 ms"));
        assert!(view.runtime_detail.contains("Manual"));
        assert!(view.runtime_detail.contains("12 updates / 24 commands"));
        assert!(view.packet_trend.contains("peak 8.0/s"));
        assert!(view.command_trend.contains("peak 16.0/s"));
        assert!(view.force_trend.contains("peak 92%"));
        assert_eq!(view.last_update, "constant(magnitude=4000)");
        assert_eq!(view.last_commands, "constant(magnitude=-2500)");
    }

    #[test]
    fn auto_calibration_reduces_gain_when_peak_is_high() {
        let recommendation = compute_auto_calibration(AutoCalibrationInputs {
            const_magnitude: 1.0,
            const_maximum_force: 1.0,
            sine_maximum_force: 0.8,
            spring_saturation: 0.5,
            damper_saturation: 0.5,
            telemetry_peak_force_percent: Some(99.0),
            telemetry_filter_percent: Some(92.0),
            telemetry_steering_raw: Some(32_767),
            existing_center_offset: 0.0,
        });

        assert!(recommendation.output_gain < 1.0);
        assert!((recommendation.steering_range - 1.10).abs() < f32::EPSILON);
        assert!((recommendation.steering_curve - 1.30).abs() < f32::EPSILON);
    }

    #[test]
    fn auto_calibration_falls_back_without_telemetry() {
        let recommendation = compute_auto_calibration(AutoCalibrationInputs {
            const_magnitude: 0.6,
            const_maximum_force: 0.7,
            sine_maximum_force: 0.5,
            spring_saturation: 0.0,
            damper_saturation: 0.0,
            telemetry_peak_force_percent: None,
            telemetry_filter_percent: None,
            telemetry_steering_raw: None,
            existing_center_offset: 0.08,
        });

        assert_eq!(recommendation.preset_label, "Auto Quick");
        assert!(recommendation.output_gain >= 0.75);
        assert!(recommendation.output_gain <= 1.25);
        assert!((recommendation.steering_center_offset - 0.08).abs() < f32::EPSILON);
        assert!(recommendation.status_message.contains("profile defaults"));
    }

    #[test]
    fn sweep_summary_aggregates_peak_filter_and_steering() {
        let mut state = AutoCalibrationSweepState::default();
        state.samples = vec![
            AutoCalibrationSweepSample {
                peak_force_percent: Some(72.0),
                filter_percent: Some(60.0),
                steering_raw: Some(32000),
            },
            AutoCalibrationSweepSample {
                peak_force_percent: Some(88.0),
                filter_percent: Some(80.0),
                steering_raw: Some(34000),
            },
            AutoCalibrationSweepSample {
                peak_force_percent: Some(95.0),
                filter_percent: Some(85.0),
                steering_raw: Some(33000),
            },
            AutoCalibrationSweepSample {
                peak_force_percent: Some(90.0),
                filter_percent: Some(75.0),
                steering_raw: Some(33500),
            },
        ];

        let (peak_max, peak_avg, filter_avg, steering_avg, count) =
            sweep_summary(&state).expect("summary should be available");

        assert_eq!(count, 4);
        assert!((peak_max - 95.0).abs() < f32::EPSILON);
        assert!((peak_avg - 86.25).abs() < 0.01);
        assert!((filter_avg - 75.0).abs() < 0.01);
        assert!((steering_avg - 33_125.0).abs() < 0.1);
    }
}
