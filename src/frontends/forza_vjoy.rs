pub use crate::core::domain::InputFrame;
pub use crate::feeder::{InputMapper, InputMapping};
pub use crate::ffb_packet::{PacketProcessResult, PacketReader};
pub use crate::vjoy::{
    HidUsage, VJoyApi, VJoyDevice, VJoyDeviceError, VJoyError, VirtualJoystickReport, VjdStatus,
};

use crate::config::FfbParamsConfig;
use crate::core::domain::{EffectUpdate, WheelCommand};
use crate::effect_engine::EffectEngine;
use std::ffi::c_void;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeUpdate {
    pub updates: Vec<EffectUpdate>,
    pub commands: Vec<WheelCommand>,
}

#[derive(Debug)]
pub struct ForzaVJoyRuntime {
    packet_reader: PacketReader,
    effect_engine: EffectEngine,
    steering_state: i32,
    pending_update: Option<RuntimeUpdate>,
}

impl ForzaVJoyRuntime {
    pub fn new(settings: FfbParamsConfig) -> Self {
        Self {
            packet_reader: PacketReader::default(),
            effect_engine: EffectEngine::new(settings),
            steering_state: -1,
            pending_update: None,
        }
    }

    pub fn process_ffb_packet(
        &mut self,
        api: &VJoyApi,
        packet: *const c_void,
    ) -> Result<Option<RuntimeUpdate>, VJoyError> {
        let result = self.packet_reader.process_packet(api, packet)?;
        let update = self.handle_packet_result(result);
        if let Some(update) = &update {
            self.pending_update = Some(update.clone());
        }
        Ok(update)
    }

    pub fn handle_packet_result(&mut self, result: PacketProcessResult) -> Option<RuntimeUpdate> {
        let PacketProcessResult::EffectUpdates(updates) = result else {
            return None;
        };

        let mut commands = Vec::new();
        for update in &updates {
            commands.extend(self.effect_engine.translate(update, self.steering_state));
        }

        Some(RuntimeUpdate { updates, commands })
    }

    pub fn update_steering_state(&mut self, steering_state: i32) {
        self.steering_state = steering_state;
    }

    pub fn replace_settings(&mut self, settings: FfbParamsConfig) {
        self.effect_engine = EffectEngine::new(settings);
    }

    pub fn apply_input_frames(
        &mut self,
        mapper: &mut InputMapper,
        frames: &[InputFrame],
        vjoy: &mut VJoyDevice,
    ) -> Result<(), VJoyDeviceError> {
        mapper.apply_input_frames(frames, vjoy)?;
        self.update_steering_state(mapper.steering_state);
        Ok(())
    }

    pub fn take_pending_update(&mut self) -> Option<RuntimeUpdate> {
        self.pending_update.take()
    }
}

#[derive(Debug, Error)]
pub enum ForzaVJoyRuntimeError {
    #[error(transparent)]
    VJoy(#[from] VJoyError),
    #[error("ffb runtime lock poisoned")]
    LockPoisoned,
}

struct CallbackContext {
    api: VJoyApi,
    runtime: Mutex<ForzaVJoyRuntime>,
}

pub struct RegisteredFfbCallback {
    vjoy: VJoyDevice,
    callback_context: &'static CallbackContext,
}

impl RegisteredFfbCallback {
    pub fn register(vjoy: VJoyDevice, settings: FfbParamsConfig) -> Result<Self, VJoyError> {
        let callback_context = Box::leak(Box::new(CallbackContext {
            api: VJoyApi::load()?,
            runtime: Mutex::new(ForzaVJoyRuntime::new(settings)),
        }));

        vjoy.api().register_ffb_callback(
            forza_vjoy_callback,
            callback_context as *const CallbackContext as *mut c_void,
        );

        Ok(Self {
            vjoy,
            callback_context,
        })
    }

    pub fn id(&self) -> u32 {
        self.vjoy.id()
    }

    pub fn is_ffb_capable(&self) -> bool {
        self.vjoy.is_ffb_capable()
    }

    pub fn update_steering_state(&self, steering_state: i32) -> Result<(), ForzaVJoyRuntimeError> {
        let mut runtime = self
            .callback_context
            .runtime
            .lock()
            .map_err(|_| ForzaVJoyRuntimeError::LockPoisoned)?;
        runtime.update_steering_state(steering_state);
        Ok(())
    }

    pub fn take_pending_update(&self) -> Result<Option<RuntimeUpdate>, ForzaVJoyRuntimeError> {
        let mut runtime = self
            .callback_context
            .runtime
            .lock()
            .map_err(|_| ForzaVJoyRuntimeError::LockPoisoned)?;
        Ok(runtime.take_pending_update())
    }

    pub fn replace_settings(&self, settings: FfbParamsConfig) -> Result<(), ForzaVJoyRuntimeError> {
        let mut runtime = self
            .callback_context
            .runtime
            .lock()
            .map_err(|_| ForzaVJoyRuntimeError::LockPoisoned)?;
        runtime.replace_settings(settings);
        Ok(())
    }
}

unsafe extern "C" fn forza_vjoy_callback(packet: *const c_void, user_data: *mut c_void) {
    if packet.is_null() || user_data.is_null() {
        return;
    }

    let callback_context = unsafe { &*(user_data as *const CallbackContext) };
    if let Ok(mut runtime) = callback_context.runtime.lock() {
        let _ = runtime.process_ffb_packet(&callback_context.api, packet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConditionFfbConfig, ConstFfbConfig, PeriodicFfbConfig, VibrationConfig};
    use crate::core::domain::{EffectKind, EffectMetadata, GameEffect};

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

    fn metadata(raw_gain: u8) -> EffectMetadata {
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
    fn constant_update_emits_wheel_commands() {
        let mut runtime = ForzaVJoyRuntime::new(settings());
        let update = runtime
            .handle_packet_result(PacketProcessResult::EffectUpdates(vec![
                EffectUpdate::Apply(GameEffect::Constant {
                    metadata: metadata(255),
                    magnitude: 8_000,
                }),
            ]))
            .expect("runtime update");

        assert_eq!(update.commands.len(), 1);
        match &update.commands[0] {
            WheelCommand::Constant(command) => assert_eq!(command.magnitude, -5_000),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn stop_effect_emits_stop_command() {
        let mut runtime = ForzaVJoyRuntime::new(settings());
        let update = runtime
            .handle_packet_result(PacketProcessResult::EffectUpdates(vec![
                EffectUpdate::StopEffect(EffectKind::Constant),
            ]))
            .expect("runtime update");

        assert_eq!(
            update.commands,
            vec![WheelCommand::StopEffect(EffectKind::Constant)]
        );
    }

    #[test]
    fn steering_state_affects_translated_constant_commands() {
        let mut runtime = ForzaVJoyRuntime::new(settings());
        runtime.update_steering_state(45_000);

        let update = runtime
            .handle_packet_result(PacketProcessResult::EffectUpdates(vec![
                EffectUpdate::Apply(GameEffect::Constant {
                    metadata: metadata(255),
                    magnitude: 8_000,
                }),
            ]))
            .expect("runtime update");

        match &update.commands[0] {
            WheelCommand::Constant(command) => assert!(command.magnitude.abs() < 5_000),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn pending_updates_are_recorded_when_processing_packet_results() {
        let mut runtime = ForzaVJoyRuntime::new(settings());
        assert!(runtime.take_pending_update().is_none());

        let emitted = runtime
            .handle_packet_result(PacketProcessResult::EffectUpdates(vec![
                EffectUpdate::Apply(GameEffect::Condition {
                    metadata: metadata(200),
                    kind: EffectKind::Spring,
                    dead_band: 0,
                    center_point_offset: 0,
                    positive_coefficient: 10_000,
                    negative_coefficient: -10_000,
                    positive_saturation: 10_000,
                    negative_saturation: 10_000,
                }),
            ]))
            .expect("runtime update");
        runtime.pending_update = Some(emitted.clone());
        assert_eq!(runtime.take_pending_update(), Some(emitted));
    }
}
