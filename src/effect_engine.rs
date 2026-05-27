use crate::config::FfbParamsConfig;
use crate::core::domain::{
    ConditionCommand, ConstantCommand, EffectKind, EffectUpdate, GameEffect, PeriodicCommand,
    WheelCommand,
};

#[derive(Debug, Clone)]
pub struct EffectEngine {
    settings: FfbParamsConfig,
    sine_engine_bug: bool,
    sine_gear_shift_bug: bool,
}

impl EffectEngine {
    pub fn new(settings: FfbParamsConfig) -> Self {
        Self {
            settings,
            sine_engine_bug: false,
            sine_gear_shift_bug: false,
        }
    }

    pub fn translate(&mut self, update: &EffectUpdate, steering_state: i32) -> Vec<WheelCommand> {
        match update {
            EffectUpdate::Ignore => vec![WheelCommand::Ignore],
            EffectUpdate::DeviceControl(command) => {
                vec![WheelCommand::DeviceControl(*command)]
            }
            EffectUpdate::StopEffect(kind) => vec![WheelCommand::StopEffect(*kind)],
            EffectUpdate::Apply(effect) => match effect {
                GameEffect::Constant {
                    metadata,
                    magnitude,
                } => vec![WheelCommand::Constant(self.translate_constant(
                    metadata.clone(),
                    *magnitude,
                    steering_state,
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
                ))],
            },
        }
    }

    fn translate_constant(
        &self,
        metadata: crate::core::domain::EffectMetadata,
        magnitude: i32,
        steering_state: i32,
    ) -> ConstantCommand {
        let mut magnitude = ((magnitude as f32) * self.settings.r#const.magnitude).round() as i32;
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
            );
        }

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
    ) -> PeriodicCommand {
        let mut period = ((period as f32) * 1000.0 / self.settings.sine.frequency).round() as i32;
        let mut magnitude =
            ((metadata.raw_gain as f32 / 255.0) * 10_000.0 * self.settings.sine.magnitude).round()
                as i32;
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
            magnitude =
                (self.settings.sine.gear_shift_vibrations.strength * 10_000.0).round() as i32;
            period = ((period as f32) / self.settings.sine.gear_shift_vibrations.frequency).round()
                as i32;
        } else if self.sine_gear_shift_bug && metadata.raw_gain == 0 {
            magnitude =
                (self.settings.sine.gear_shift_vibrations.strength * 10_000.0).round() as i32;
            period = ((period as f32) / self.settings.sine.gear_shift_vibrations.frequency).round()
                as i32;
            self.sine_gear_shift_bug = false;
        }

        if metadata.raw_gain == 1 {
            self.sine_engine_bug = true;
            magnitude = (self.settings.sine.engine_vibrations.strength * 10_000.0).round() as i32;
            period =
                ((period as f32) / self.settings.sine.engine_vibrations.frequency).round() as i32;
        } else if self.sine_engine_bug && metadata.raw_gain == 0 {
            magnitude = (self.settings.sine.engine_vibrations.strength * 10_000.0).round() as i32;
            period =
                ((period as f32) / self.settings.sine.engine_vibrations.frequency).round() as i32;
            self.sine_engine_bug = false;
        }

        if magnitude > 10_000 {
            magnitude = 10_000;
        }

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
    ) -> ConditionCommand {
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

        ConditionCommand {
            metadata,
            kind,
            dead_band,
            center_point_offset,
            positive_coefficient: ((positive_coefficient as f32) * coefficient_scale).round()
                as i32,
            negative_coefficient: ((negative_coefficient as f32) * coefficient_scale).round()
                as i32,
            positive_saturation: (((positive_saturation as f32) * saturation_scale).round() as i32)
                .min(10_000),
            negative_saturation: (((negative_saturation as f32) * saturation_scale).round() as i32)
                .min(10_000),
        }
    }
}

fn apply_steering_filter(magnitude: i32, steering_state: i32, minimum_coefficient: f32) -> i32 {
    let mut coeff = if steering_state < 32_767 {
        1.0 - (steering_state as f32 / 32_767.0)
    } else if steering_state > 32_767 {
        (steering_state - 32_767) as f32 / 32_767.0
    } else {
        minimum_coefficient
    };

    if coeff < minimum_coefficient {
        coeff = minimum_coefficient;
    }

    ((magnitude as f32) * coeff) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ConditionFfbConfig, ConstFfbConfig, FfbParamsConfig, PeriodicFfbConfig, VibrationConfig,
    };
    use crate::core::domain::{DeviceControlCommand, EffectMetadata, EffectUpdate, GameEffect};

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
}
