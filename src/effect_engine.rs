use crate::config::FfbParamsConfig;
use crate::core::domain::{
    ConditionCommand, ConstantCommand, EffectKind, EffectUpdate, GameEffect, PeriodicCommand,
    WheelCommand,
};
use std::time::Instant;

const STEERING_CENTER: f32 = 32_767.0;
const STEERING_FULL_SCALE: f32 = 32_767.0;

#[derive(Debug, Clone)]
pub struct EffectEngine {
    settings: FfbParamsConfig,
    sine_engine_bug: bool,
    sine_gear_shift_bug: bool,
    traction_release: f32,
    torque_steer_bias: f32,
    brake_imbalance_bias: f32,
    understeer_release: f32,
    rear_lightness_release: f32,
    snap_oversteer_boost: f32,
    curb_asymmetry_bias: f32,
    last_steering_delta_sign: f32,
    last_steering_state: Option<i32>,
    last_update: Option<Instant>,
    last_force_proxy: f32,
}

impl EffectEngine {
    pub fn new(settings: FfbParamsConfig) -> Self {
        Self {
            settings,
            sine_engine_bug: false,
            sine_gear_shift_bug: false,
            traction_release: 0.0,
            torque_steer_bias: 0.0,
            brake_imbalance_bias: 0.0,
            understeer_release: 0.0,
            rear_lightness_release: 0.0,
            snap_oversteer_boost: 0.0,
            curb_asymmetry_bias: 0.0,
            last_steering_delta_sign: 0.0,
            last_steering_state: None,
            last_update: None,
            last_force_proxy: 0.0,
        }
    }

    pub fn translate(&mut self, update: &EffectUpdate, steering_state: i32) -> Vec<WheelCommand> {
        match update {
            EffectUpdate::Ignore => vec![WheelCommand::Ignore],
            EffectUpdate::DeviceControl(command) => {
                vec![WheelCommand::DeviceControl(*command)]
            }
            EffectUpdate::StopEffect(kind) => vec![WheelCommand::StopEffect(*kind)],
            EffectUpdate::Apply(effect) => {
                self.update_experimental_state(effect, steering_state);
                match effect {
                    GameEffect::Constant {
                        metadata,
                        magnitude,
                    } => vec![WheelCommand::Constant(self.translate_constant(
                        metadata.clone(),
                        *magnitude,
                        steering_state,
                        self.traction_multiplier_for(EffectKind::Constant),
                    ))],
                    GameEffect::Periodic {
                        metadata,
                        magnitude,
                        offset,
                        period,
                        phase,
                    } => vec![WheelCommand::Periodic(self.translate_periodic(
                        metadata.clone(),
                        *magnitude,
                        *offset,
                        *period,
                        *phase,
                        steering_state,
                    ))],
                    GameEffect::Condition {
                        metadata,
                        kind,
                        dead_band,
                        center_point_offset,
                        positive_coefficient,
                        negative_coefficient,
                        positive_saturation,
                        negative_saturation,
                    } => vec![WheelCommand::Condition(self.translate_condition(
                        metadata.clone(),
                        *kind,
                        *dead_band,
                        *center_point_offset,
                        *positive_coefficient,
                        *negative_coefficient,
                        *positive_saturation,
                        *negative_saturation,
                        self.traction_multiplier_for(*kind),
                    ))],
                }
            }
        }
    }

    fn translate_constant(
        &self,
        metadata: crate::core::domain::EffectMetadata,
        magnitude: i32,
        steering_state: i32,
        traction_multiplier: f32,
    ) -> ConstantCommand {
        let output_gain = calibration_output_gain(&self.settings)
            * self.settings.calibration.const_gain.clamp(0.0, 2.0);
        let mut magnitude =
            ((magnitude as f32) * self.settings.r#const.magnitude * output_gain).round() as i32;
        let max_force = (self.settings.r#const.maximum_force * 10_000.0).round() as i32;
        if magnitude.abs() > max_force {
            magnitude = max_force * magnitude.signum();
        }

        let min_force = (self.settings.r#const.minimum_force * 10_000.0) as i32;
        if magnitude.abs() < min_force {
            magnitude = min_force * magnitude.signum();
        }

        if magnitude.abs() > 10_000 {
            magnitude = 10_000 * magnitude.signum();
        }

        if (magnitude.abs() as f32) > self.settings.r#const.filter_threshold * 10_000.0
            && steering_state != -1
        {
            magnitude = apply_steering_filter(
                magnitude,
                steering_state,
                self.settings.r#const.minimum_coefficient,
                &self.settings,
            );
        }

        magnitude = apply_traction_release(
            magnitude,
            traction_multiplier,
            self.settings
                .experimental
                .traction_loss
                .min_force_floor
                .clamp(0.0, 1.0),
        );
        magnitude = apply_signed_bias(
            magnitude,
            self.torque_steer_bias + self.brake_imbalance_bias,
        );

        ConstantCommand {
            metadata,
            magnitude: -magnitude,
        }
    }

    fn translate_periodic(
        &mut self,
        metadata: crate::core::domain::EffectMetadata,
        _magnitude: i32,
        offset: i16,
        period: i32,
        _phase: i32,
        steering_state: i32,
    ) -> PeriodicCommand {
        let output_gain = calibration_output_gain(&self.settings)
            * self.settings.calibration.sine_gain.clamp(0.0, 2.0);
        let mut period = ((period as f32) * 1000.0 / self.settings.sine.frequency).round() as i32;
        let mut magnitude = ((metadata.raw_gain as f32 / 255.0)
            * 10_000.0
            * self.settings.sine.magnitude
            * output_gain)
            .round() as i32;
        let sine_gain_max = (self.settings.sine.maximum_force * 10_000.0).round() as i32;
        if magnitude > sine_gain_max {
            magnitude = sine_gain_max;
        }

        let minimum = (self.settings.sine.minimum_force * 10_000.0).round() as i32;
        if magnitude < minimum && magnitude > 0 {
            magnitude = minimum;
        }

        if metadata.raw_gain == 32 {
            self.sine_gear_shift_bug = true;
            magnitude = (self.settings.sine.gear_shift_vibrations.strength * output_gain * 10_000.0)
                .round() as i32;
            period = ((period as f32) / self.settings.sine.gear_shift_vibrations.frequency).round()
                as i32;
        } else if self.sine_gear_shift_bug && metadata.raw_gain == 0 {
            magnitude = (self.settings.sine.gear_shift_vibrations.strength * output_gain * 10_000.0)
                .round() as i32;
            period = ((period as f32) / self.settings.sine.gear_shift_vibrations.frequency).round()
                as i32;
            self.sine_gear_shift_bug = false;
        }

        if metadata.raw_gain == 1 {
            self.sine_engine_bug = true;
            magnitude = (self.settings.sine.engine_vibrations.strength * output_gain * 10_000.0)
                .round() as i32;
            period =
                ((period as f32) / self.settings.sine.engine_vibrations.frequency).round() as i32;
        } else if self.sine_engine_bug && metadata.raw_gain == 0 {
            magnitude = (self.settings.sine.engine_vibrations.strength * output_gain * 10_000.0)
                .round() as i32;
            period =
                ((period as f32) / self.settings.sine.engine_vibrations.frequency).round() as i32;
            self.sine_engine_bug = false;
        }

        if magnitude > 10_000 {
            magnitude = 10_000;
        }

        let periodic_multiplier = self.periodic_multiplier(metadata.direction, steering_state);
        magnitude = ((magnitude as f32) * periodic_multiplier).round() as i32;
        magnitude = magnitude.clamp(-10_000, 10_000);

        PeriodicCommand {
            metadata,
            magnitude,
            offset,
            period,
            phase: (self.settings.sine.phase * 35_999.0) as i32,
        }
    }

    fn translate_condition(
        &self,
        metadata: crate::core::domain::EffectMetadata,
        kind: EffectKind,
        dead_band: i32,
        center_point_offset: i16,
        positive_coefficient: i32,
        negative_coefficient: i32,
        positive_saturation: i32,
        negative_saturation: i32,
        traction_multiplier: f32,
    ) -> ConditionCommand {
        let output_gain = calibration_output_gain(&self.settings);
        let (coefficient_scale, saturation_scale) = match kind {
            EffectKind::Damper => (
                self.settings.damper.coefficient,
                self.settings.damper.saturation,
            ),
            EffectKind::Spring => (
                self.settings.spring.coefficient,
                self.settings.spring.saturation,
            ),
            _ => (1.0, 1.0),
        };
        let effect_gain = match kind {
            EffectKind::Damper => self.settings.calibration.damper_gain.clamp(0.0, 2.0),
            EffectKind::Spring => self.settings.calibration.spring_gain.clamp(0.0, 2.0),
            _ => 1.0,
        };
        let calibrated_gain = output_gain
            * effect_gain
            * traction_multiplier
            * self.condition_dynamics_multiplier(kind);

        ConditionCommand {
            metadata,
            kind,
            dead_band,
            center_point_offset,
            positive_coefficient: ((positive_coefficient as f32)
                * coefficient_scale
                * calibrated_gain)
                .round() as i32,
            negative_coefficient: ((negative_coefficient as f32)
                * coefficient_scale
                * calibrated_gain)
                .round() as i32,
            positive_saturation: (((positive_saturation as f32)
                * saturation_scale
                * calibrated_gain)
                .round() as i32)
                .min(10_000),
            negative_saturation: (((negative_saturation as f32)
                * saturation_scale
                * calibrated_gain)
                .round() as i32)
                .min(10_000),
        }
    }

    fn update_experimental_state(&mut self, effect: &GameEffect, steering_state: i32) {
        let config = &self.settings.experimental.traction_loss;
        let inferred = &self.settings.experimental.inferred_dynamics;
        let now = Instant::now();
        let dt = self
            .last_update
            .map(|last| now.duration_since(last).as_secs_f32())
            .unwrap_or(0.0)
            .max(0.0);

        if !config.enabled {
            self.traction_release = 0.0;
        }

        let previous_steering = self.last_steering_state.unwrap_or(steering_state) as f32;
        let steering_delta = steering_state as f32 - previous_steering;
        let steering_rate = if dt > 0.0 {
            (steering_delta.abs() / STEERING_FULL_SCALE) / dt
        } else {
            0.0
        };
        let steering_delta_sign = steering_delta.signum();
        let steering_angle = (steering_state as f32 / STEERING_FULL_SCALE).abs();
        let steering_sign = (steering_state as f32).signum();

        let new_force_proxy = force_proxy(effect).unwrap_or(self.last_force_proxy);
        let force_drop = (self.last_force_proxy - new_force_proxy).max(0.0);
        let force_change = (new_force_proxy - self.last_force_proxy).abs();

        if config.enabled {
            let triggered = steering_rate >= config.steering_rate_threshold.clamp(0.0, 8.0)
                && steering_angle >= config.steering_angle_threshold.clamp(0.0, 1.0)
                && force_change >= config.force_drop_threshold.clamp(0.0, 1.0);
            self.traction_release = evolve_scalar(
                self.traction_release,
                triggered,
                config.attack_ms,
                config.recovery_ms,
                dt,
            );
        } else {
            self.traction_release = 0.0;
        }

        if inferred.torque_steer.enabled {
            let threshold = inferred.torque_steer.trigger_threshold.clamp(0.0, 8.0);
            let target = if steering_rate >= threshold {
                steering_delta_sign * inferred.torque_steer.strength.clamp(0.0, 1.0)
            } else {
                0.0
            };
            self.torque_steer_bias = smooth_toward(self.torque_steer_bias, target, dt, 220);
        } else {
            self.torque_steer_bias = 0.0;
        }

        if inferred.brake_imbalance.enabled {
            let threshold = inferred.brake_imbalance.trigger_threshold.clamp(0.0, 1.0);
            let target = if force_drop >= threshold && steering_angle > 0.12 {
                steering_sign * inferred.brake_imbalance.strength.clamp(0.0, 1.0)
            } else {
                0.0
            };
            self.brake_imbalance_bias = smooth_toward(self.brake_imbalance_bias, target, dt, 320);
        } else {
            self.brake_imbalance_bias = 0.0;
        }

        if inferred.understeer_scrub.enabled {
            let threshold = inferred.understeer_scrub.trigger_threshold.clamp(0.0, 1.0);
            let trigger = steering_angle >= threshold && force_drop <= 0.03;
            self.understeer_release = evolve_scalar(
                self.understeer_release,
                trigger,
                inferred.understeer_scrub.attack_ms,
                inferred.understeer_scrub.recovery_ms,
                dt,
            );
        } else {
            self.understeer_release = 0.0;
        }

        if inferred.rear_lightness.enabled {
            let threshold = inferred.rear_lightness.trigger_threshold.clamp(0.0, 8.0);
            let reversed = steering_delta_sign != 0.0
                && self.last_steering_delta_sign != 0.0
                && steering_delta_sign != self.last_steering_delta_sign;
            let trigger = reversed && steering_rate >= threshold;
            self.rear_lightness_release = evolve_scalar(
                self.rear_lightness_release,
                trigger,
                inferred.rear_lightness.attack_ms,
                inferred.rear_lightness.recovery_ms,
                dt,
            );
        } else {
            self.rear_lightness_release = 0.0;
        }

        if inferred.snap_oversteer.enabled {
            let threshold = inferred.snap_oversteer.trigger_threshold.clamp(0.0, 8.0);
            let recovering = config.enabled && self.traction_release < 0.25 && force_drop < 0.02;
            let trigger = recovering && steering_rate >= threshold;
            self.snap_oversteer_boost = evolve_scalar(
                self.snap_oversteer_boost,
                trigger,
                inferred.snap_oversteer.attack_ms,
                inferred.snap_oversteer.recovery_ms,
                dt,
            );
        } else {
            self.snap_oversteer_boost = 0.0;
        }

        if inferred.curb_asymmetry.enabled {
            let target = if matches!(effect, GameEffect::Periodic { period, .. } if *period <= 140)
                && steering_angle >= inferred.curb_asymmetry.trigger_threshold.clamp(0.0, 1.0)
            {
                steering_sign * inferred.curb_asymmetry.strength.clamp(0.0, 1.0)
            } else {
                0.0
            };
            self.curb_asymmetry_bias = smooth_toward(self.curb_asymmetry_bias, target, dt, 180);
        } else {
            self.curb_asymmetry_bias = 0.0;
        }

        self.last_force_proxy = new_force_proxy;
        self.last_steering_delta_sign = steering_delta_sign;
        self.last_update = Some(now);
        self.last_steering_state = Some(steering_state);
    }

    fn traction_multiplier_for(&self, kind: EffectKind) -> f32 {
        let config = &self.settings.experimental.traction_loss;
        if !config.enabled {
            return 1.0;
        }

        let applies = match kind {
            EffectKind::Constant => config.apply_constant,
            EffectKind::Spring => config.apply_spring,
            EffectKind::Damper => config.apply_damper,
            _ => false,
        };
        if !applies {
            return 1.0;
        }

        let active_release = self.traction_release * config.release_strength.clamp(0.0, 1.0);
        (1.0 - active_release).clamp(0.0, 1.0)
    }

    fn condition_dynamics_multiplier(&self, kind: EffectKind) -> f32 {
        if !matches!(kind, EffectKind::Spring | EffectKind::Damper) {
            return 1.0;
        }

        let inferred = &self.settings.experimental.inferred_dynamics;
        let mut multiplier = 1.0;
        if inferred.understeer_scrub.enabled {
            let release =
                self.understeer_release * inferred.understeer_scrub.strength.clamp(0.0, 1.0);
            multiplier *= (1.0 - release).clamp(0.1, 1.0);
        }
        if inferred.rear_lightness.enabled {
            let release =
                self.rear_lightness_release * inferred.rear_lightness.strength.clamp(0.0, 1.0);
            multiplier *= (1.0 - release).clamp(0.1, 1.0);
        }
        if inferred.snap_oversteer.enabled {
            let boost =
                self.snap_oversteer_boost * inferred.snap_oversteer.strength.clamp(0.0, 1.0);
            multiplier *= (1.0 + boost).clamp(1.0, 1.5);
        }

        multiplier.clamp(0.1, 1.5)
    }

    fn periodic_multiplier(&self, direction: u8, steering_state: i32) -> f32 {
        let inferred = &self.settings.experimental.inferred_dynamics;
        if !inferred.curb_asymmetry.enabled {
            return 1.0;
        }

        let direction_sign = if direction > 127 { -1.0 } else { 1.0 };
        let steering_sign = (steering_state as f32).signum();
        let alignment = direction_sign * steering_sign;
        let bias = self.curb_asymmetry_bias * alignment;
        (1.0 + bias).clamp(0.4, 1.6)
    }
}

fn calibration_output_gain(settings: &FfbParamsConfig) -> f32 {
    settings.calibration.output_gain.clamp(0.0, 2.0)
}

fn force_proxy(effect: &GameEffect) -> Option<f32> {
    match effect {
        GameEffect::Constant { magnitude, .. } => {
            Some(((*magnitude as f32).abs() / 10_000.0).clamp(0.0, 1.0))
        }
        GameEffect::Condition {
            positive_saturation,
            negative_saturation,
            ..
        } => {
            let avg = ((*positive_saturation as f32).abs() + (*negative_saturation as f32).abs())
                / 20_000.0;
            Some(avg.clamp(0.0, 1.0))
        }
        GameEffect::Periodic { .. } => None,
    }
}

fn apply_traction_release(magnitude: i32, multiplier: f32, minimum_floor: f32) -> i32 {
    let magnitude_sign = magnitude.signum();
    let scaled = ((magnitude as f32) * multiplier.clamp(0.0, 1.0)).round() as i32;
    let floor = ((magnitude.abs() as f32) * minimum_floor.clamp(0.0, 1.0)).round() as i32;
    if magnitude_sign != 0 && scaled.abs() < floor {
        floor * magnitude_sign
    } else {
        scaled
    }
}

fn apply_signed_bias(magnitude: i32, bias: f32) -> i32 {
    let signed_offset = ((magnitude.abs() as f32) * bias.clamp(-1.0, 1.0)).round() as i32;
    (magnitude + signed_offset).clamp(-10_000, 10_000)
}

fn evolve_scalar(current: f32, trigger: bool, attack_ms: u32, recovery_ms: u32, dt: f32) -> f32 {
    let attack_secs = (attack_ms.max(1) as f32) / 1000.0;
    let recovery_secs = (recovery_ms.max(1) as f32) / 1000.0;
    if trigger {
        (current + dt / attack_secs).clamp(0.0, 1.0)
    } else {
        (current - dt / recovery_secs).clamp(0.0, 1.0)
    }
}

fn smooth_toward(current: f32, target: f32, dt: f32, time_constant_ms: u32) -> f32 {
    if dt <= 0.0 {
        return current;
    }
    let tau = (time_constant_ms.max(1) as f32) / 1000.0;
    let alpha = (dt / tau).clamp(0.0, 1.0);
    current + (target - current) * alpha
}

fn apply_steering_filter(
    magnitude: i32,
    steering_state: i32,
    minimum_coefficient: f32,
    settings: &FfbParamsConfig,
) -> i32 {
    let coeff = steering_filter_coefficient(settings, steering_state, minimum_coefficient);

    ((magnitude as f32) * coeff) as i32
}

pub fn steering_filter_coefficient(
    settings: &FfbParamsConfig,
    steering_state: i32,
    minimum_coefficient: f32,
) -> f32 {
    let minimum_coefficient = minimum_coefficient.clamp(0.0, 1.0);
    let center = STEERING_CENTER
        + settings.calibration.steering_center_offset.clamp(-0.5, 0.5) * STEERING_FULL_SCALE;
    let range =
        (STEERING_FULL_SCALE * settings.calibration.steering_range.clamp(0.25, 2.0)).max(1.0);
    let curve = settings.calibration.steering_curve.clamp(0.25, 3.0);
    let normalized_distance = ((steering_state as f32 - center).abs() / range).clamp(0.0, 1.0);

    minimum_coefficient + (1.0 - minimum_coefficient) * normalized_distance.powf(curve)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        CalibrationConfig, ConditionFfbConfig, ConstFfbConfig, ExperimentalConfig, FfbParamsConfig,
        PeriodicFfbConfig, VibrationConfig,
    };
    use crate::core::domain::{DeviceControlCommand, EffectMetadata, EffectUpdate, GameEffect};
    use std::thread::sleep;
    use std::time::Duration;

    fn settings() -> FfbParamsConfig {
        FfbParamsConfig {
            r#const: ConstFfbConfig {
                magnitude: 1.0,
                maximum_force: 0.5,
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
                    frequency: 2.0,
                    strength: 0.015,
                },
                gear_shift_vibrations: VibrationConfig {
                    frequency: 4.0,
                    strength: 0.06,
                },
            },
            spring: ConditionFfbConfig {
                coefficient: 0.03,
                saturation: 0.5,
            },
            damper: ConditionFfbConfig {
                coefficient: 0.02,
                saturation: 0.25,
            },
            calibration: CalibrationConfig::default(),
            experimental: ExperimentalConfig::default(),
        }
    }

    fn metadata(raw_gain: u8) -> crate::core::domain::EffectMetadata {
        EffectMetadata {
            duration_ms: 100_000,
            gain: ((raw_gain as f32 / 255.0) * 10_000.0).round() as i32,
            raw_gain,
            direction: 0,
            sample_period: 0,
            trigger_button: -1,
            trigger_repeat_interval: 0,
        }
    }

    #[test]
    fn constant_translation_applies_cap_and_sign_flip() {
        let mut engine = EffectEngine::new(settings());
        let update = EffectUpdate::Apply(GameEffect::Constant {
            metadata: metadata(255),
            magnitude: 8000,
        });

        let result = engine.translate(&update, -1);
        assert_eq!(result.len(), 1);
        match &result[0] {
            WheelCommand::Constant(command) => {
                assert_eq!(command.magnitude, -5000);
                assert_eq!(command.metadata.duration_ms, 100_000);
                assert_eq!(command.metadata.gain, 10_000);
                assert_eq!(command.metadata.trigger_button, -1);
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn periodic_translation_uses_engine_vibration_special_case() {
        let mut engine = EffectEngine::new(settings());
        let update = EffectUpdate::Apply(GameEffect::Periodic {
            metadata: metadata(1),
            magnitude: 0,
            offset: 10,
            period: 50,
            phase: 0,
        });

        let result = engine.translate(&update, -1);
        match &result[0] {
            WheelCommand::Periodic(command) => {
                assert_eq!(command.magnitude, 150);
                assert_eq!(command.period, 25_000);
                assert_eq!(command.offset, 10);
                assert_eq!(command.phase, 13_499);
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn condition_translation_scales_damper_coefficients() {
        let mut engine = EffectEngine::new(settings());
        let update = EffectUpdate::Apply(GameEffect::Condition {
            metadata: metadata(200),
            kind: EffectKind::Damper,
            dead_band: 250,
            center_point_offset: 5,
            positive_coefficient: 10_000,
            negative_coefficient: -10_000,
            positive_saturation: 10_000,
            negative_saturation: 8_000,
        });

        let result = engine.translate(&update, -1);
        match &result[0] {
            WheelCommand::Condition(command) => {
                assert_eq!(command.positive_coefficient, 200);
                assert_eq!(command.negative_coefficient, -200);
                assert_eq!(command.positive_saturation, 2500);
                assert_eq!(command.negative_saturation, 2000);
                assert_eq!(command.dead_band, 250);
                assert_eq!(command.center_point_offset, 5);
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn constant_translation_respects_calibrated_center_offset() {
        let mut tuned = settings();
        tuned.r#const.maximum_force = 1.0;
        tuned.r#const.minimum_force = 0.0;
        tuned.r#const.filter_threshold = 0.0;
        tuned.r#const.minimum_coefficient = 0.0;
        tuned.calibration.steering_center_offset = 0.25;
        let mut engine = EffectEngine::new(tuned);
        let update = EffectUpdate::Apply(GameEffect::Constant {
            metadata: metadata(255),
            magnitude: 8_000,
        });
        let centered_state = (STEERING_CENTER + (STEERING_FULL_SCALE * 0.25)).round() as i32;

        let result = engine.translate(&update, centered_state);
        match &result[0] {
            WheelCommand::Constant(command) => assert_eq!(command.magnitude, 0),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn condition_translation_applies_output_gain_calibration() {
        let mut tuned = settings();
        tuned.spring.coefficient = 1.0;
        tuned.spring.saturation = 1.0;
        tuned.calibration.output_gain = 0.5;
        let mut engine = EffectEngine::new(tuned);
        let update = EffectUpdate::Apply(GameEffect::Condition {
            metadata: metadata(200),
            kind: EffectKind::Spring,
            dead_band: 100,
            center_point_offset: 0,
            positive_coefficient: 4_000,
            negative_coefficient: -2_000,
            positive_saturation: 6_000,
            negative_saturation: 4_000,
        });

        let result = engine.translate(&update, -1);
        match &result[0] {
            WheelCommand::Condition(command) => {
                assert_eq!(command.positive_coefficient, 2_000);
                assert_eq!(command.negative_coefficient, -1_000);
                assert_eq!(command.positive_saturation, 3_000);
                assert_eq!(command.negative_saturation, 2_000);
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn control_translation_maps_stop_all() {
        let mut engine = EffectEngine::new(settings());
        let result = engine.translate(
            &EffectUpdate::DeviceControl(DeviceControlCommand::StopAll),
            -1,
        );
        assert_eq!(
            result,
            vec![WheelCommand::DeviceControl(DeviceControlCommand::StopAll)]
        );
    }

    #[test]
    fn stop_effect_is_forwarded() {
        let mut engine = EffectEngine::new(settings());
        let result = engine.translate(&EffectUpdate::StopEffect(EffectKind::Constant), -1);
        assert_eq!(result, vec![WheelCommand::StopEffect(EffectKind::Constant)]);
    }

    #[test]
    fn traction_release_reduces_constant_force_when_triggered() {
        let mut tuned = settings();
        tuned.r#const.maximum_force = 1.0;
        tuned.r#const.minimum_force = 0.0;
        tuned.r#const.filter_threshold = 1.0;
        tuned.experimental.traction_loss.enabled = true;
        tuned.experimental.traction_loss.steering_rate_threshold = 0.0;
        tuned.experimental.traction_loss.steering_angle_threshold = 0.0;
        tuned.experimental.traction_loss.force_drop_threshold = 0.0;
        tuned.experimental.traction_loss.release_strength = 1.0;
        tuned.experimental.traction_loss.attack_ms = 1;
        tuned.experimental.traction_loss.recovery_ms = 1_000;
        tuned.experimental.traction_loss.min_force_floor = 0.0;
        tuned.experimental.traction_loss.apply_constant = true;
        let mut engine = EffectEngine::new(tuned);

        let first = engine.translate(
            &EffectUpdate::Apply(GameEffect::Constant {
                metadata: metadata(255),
                magnitude: 8_000,
            }),
            16_000,
        );
        sleep(Duration::from_millis(5));
        let second = engine.translate(
            &EffectUpdate::Apply(GameEffect::Constant {
                metadata: metadata(255),
                magnitude: 8_000,
            }),
            -16_000,
        );

        let first_mag = match &first[0] {
            WheelCommand::Constant(command) => command.magnitude.abs(),
            other => panic!("unexpected result: {other:?}"),
        };
        let second_mag = match &second[0] {
            WheelCommand::Constant(command) => command.magnitude.abs(),
            other => panic!("unexpected result: {other:?}"),
        };
        assert!(second_mag < first_mag);
    }

    #[test]
    fn traction_release_triggers_on_force_increase() {
        let mut baseline = settings();
        baseline.r#const.maximum_force = 1.0;
        baseline.r#const.minimum_force = 0.0;
        baseline.r#const.filter_threshold = 1.0;

        let mut tuned = baseline.clone();
        tuned.experimental.traction_loss.enabled = true;
        tuned.experimental.traction_loss.steering_rate_threshold = 0.0;
        tuned.experimental.traction_loss.steering_angle_threshold = 0.0;
        tuned.experimental.traction_loss.force_drop_threshold = 0.3;
        tuned.experimental.traction_loss.release_strength = 1.0;
        tuned.experimental.traction_loss.attack_ms = 1;
        tuned.experimental.traction_loss.recovery_ms = 1_000;
        tuned.experimental.traction_loss.min_force_floor = 0.0;
        tuned.experimental.traction_loss.apply_constant = true;

        let mut control = EffectEngine::new(baseline);
        let mut release = EffectEngine::new(tuned);

        let priming = EffectUpdate::Apply(GameEffect::Constant {
            metadata: metadata(255),
            magnitude: 1_000,
        });
        control.translate(&priming, 12_000);
        release.translate(&priming, 12_000);

        sleep(Duration::from_millis(5));

        let spike = EffectUpdate::Apply(GameEffect::Constant {
            metadata: metadata(255),
            magnitude: 8_000,
        });
        let control_second = control.translate(&spike, -12_000);
        let release_second = release.translate(&spike, -12_000);

        let control_mag = match &control_second[0] {
            WheelCommand::Constant(command) => command.magnitude.abs(),
            other => panic!("unexpected result: {other:?}"),
        };
        let release_mag = match &release_second[0] {
            WheelCommand::Constant(command) => command.magnitude.abs(),
            other => panic!("unexpected result: {other:?}"),
        };

        assert!(release_mag < control_mag);
    }

    #[test]
    fn torque_steer_adds_directional_bias_to_constant_force() {
        let mut tuned = settings();
        tuned.r#const.maximum_force = 1.0;
        tuned.r#const.minimum_force = 0.0;
        tuned.r#const.filter_threshold = 1.0;
        tuned.experimental.inferred_dynamics.torque_steer.enabled = true;
        tuned.experimental.inferred_dynamics.torque_steer.strength = 0.25;
        tuned
            .experimental
            .inferred_dynamics
            .torque_steer
            .trigger_threshold = 0.0;
        let mut engine = EffectEngine::new(tuned);

        let first = engine.translate(
            &EffectUpdate::Apply(GameEffect::Constant {
                metadata: metadata(255),
                magnitude: 5_000,
            }),
            10_000,
        );
        sleep(Duration::from_millis(8));
        let second = engine.translate(
            &EffectUpdate::Apply(GameEffect::Constant {
                metadata: metadata(255),
                magnitude: 5_000,
            }),
            20_000,
        );

        let first_mag = match &first[0] {
            WheelCommand::Constant(command) => command.magnitude.abs(),
            other => panic!("unexpected result: {other:?}"),
        };
        let second_mag = match &second[0] {
            WheelCommand::Constant(command) => command.magnitude.abs(),
            other => panic!("unexpected result: {other:?}"),
        };
        assert!(second_mag > first_mag);
    }

    #[test]
    fn understeer_scrub_reduces_spring_strength_when_triggered() {
        let mut tuned = settings();
        tuned.spring.coefficient = 1.0;
        tuned.spring.saturation = 1.0;
        tuned
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .enabled = true;
        tuned
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .strength = 0.6;
        tuned
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .trigger_threshold = 0.2;
        tuned
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .attack_ms = 1;
        tuned
            .experimental
            .inferred_dynamics
            .understeer_scrub
            .recovery_ms = 800;
        let mut engine = EffectEngine::new(tuned);

        let baseline = engine.translate(
            &EffectUpdate::Apply(GameEffect::Condition {
                metadata: metadata(200),
                kind: EffectKind::Spring,
                dead_band: 0,
                center_point_offset: 0,
                positive_coefficient: 5_000,
                negative_coefficient: -5_000,
                positive_saturation: 5_000,
                negative_saturation: 5_000,
            }),
            5_000,
        );
        sleep(Duration::from_millis(5));
        let scrubbed = engine.translate(
            &EffectUpdate::Apply(GameEffect::Condition {
                metadata: metadata(200),
                kind: EffectKind::Spring,
                dead_band: 0,
                center_point_offset: 0,
                positive_coefficient: 5_000,
                negative_coefficient: -5_000,
                positive_saturation: 5_000,
                negative_saturation: 5_000,
            }),
            24_000,
        );

        let baseline_coeff = match &baseline[0] {
            WheelCommand::Condition(command) => command.positive_coefficient.abs(),
            other => panic!("unexpected result: {other:?}"),
        };
        let scrubbed_coeff = match &scrubbed[0] {
            WheelCommand::Condition(command) => command.positive_coefficient.abs(),
            other => panic!("unexpected result: {other:?}"),
        };
        assert!(scrubbed_coeff < baseline_coeff);
    }
}
