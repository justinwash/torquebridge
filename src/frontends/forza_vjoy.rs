pub use crate::core::domain::InputFrame;
pub use crate::feeder::{InputMapper, InputMapping};
pub use crate::ffb_packet::{PacketProcessResult, PacketReader};
pub use crate::vjoy::{
    HidUsage, VJoyApi, VJoyDevice, VJoyDeviceError, VJoyError, VirtualJoystickReport, VjdStatus,
};

use crate::config::FfbParamsConfig;
use crate::core::domain::{
    ConditionCommand, DeviceControlCommand, EffectKind, EffectUpdate, WheelCommand,
};
use crate::effect_engine::EffectEngine;
use crate::protocol::DirectControl;
use std::ffi::c_void;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeUpdate {
    pub updates: Vec<EffectUpdate>,
    pub commands: Vec<WheelCommand>,
    pub direct_control: DirectControl,
}

#[derive(Debug, Default, Clone)]
struct ForceState {
    constant_force: i16,
    periodic_force: i16,
    spring_force: i16,
    spring_condition: Option<ConditionCommand>,
    damper_condition: Option<ConditionCommand>,
    last_steering_state: i32,
}

impl ForceState {
    fn apply_command(&mut self, command: &WheelCommand, steering_state: i32) {
        match command {
            WheelCommand::Ignore => {}
            WheelCommand::DeviceControl(command) => self.apply_device_control(*command),
            WheelCommand::StopEffect(kind) => self.stop_effect(*kind),
            WheelCommand::Constant(command) => {
                self.constant_force = clamp_force(command.magnitude);
            }
            WheelCommand::Periodic(command) => {
                self.periodic_force = clamp_force(command.magnitude);
            }
            WheelCommand::Condition(command) => match command.kind {
                EffectKind::Spring => self.spring_condition = Some(command.clone()),
                EffectKind::Damper => self.damper_condition = Some(command.clone()),
                _ => {}
            },
        }

        self.refresh_condition_force(steering_state);
    }

    fn update_steering_state(&mut self, steering_state: i32) {
        self.refresh_condition_force(steering_state);
    }

    fn direct_control(&self) -> DirectControl {
        DirectControl {
            spring_force: self.spring_force,
            constant_force: self.constant_force,
            periodic_force: self.periodic_force,
            force_drop: 0,
        }
    }

    fn apply_device_control(&mut self, command: DeviceControlCommand) {
        match command {
            DeviceControlCommand::Reset
            | DeviceControlCommand::StopAll
            | DeviceControlCommand::Pause => {
                self.constant_force = 0;
                self.periodic_force = 0;
                self.spring_force = 0;
                self.spring_condition = None;
                self.damper_condition = None;
            }
            DeviceControlCommand::Continue => {}
        }
    }

    fn stop_effect(&mut self, kind: EffectKind) {
        match kind {
            EffectKind::Constant => self.constant_force = 0,
            EffectKind::Sine
            | EffectKind::Square
            | EffectKind::Triangle
            | EffectKind::SawUp
            | EffectKind::SawDown => self.periodic_force = 0,
            EffectKind::Spring => self.spring_condition = None,
            EffectKind::Damper => self.damper_condition = None,
            EffectKind::None
            | EffectKind::Ramp
            | EffectKind::Inertia
            | EffectKind::Friction
            | EffectKind::Custom
            | EffectKind::Unknown(_) => {}
        }
    }

    fn refresh_condition_force(&mut self, steering_state: i32) {
        let spring_force = self
            .spring_condition
            .as_ref()
            .map(|command| spring_force(command, steering_state))
            .unwrap_or(0);
        let damper_force = self
            .damper_condition
            .as_ref()
            .map(|command| damper_force(command, self.last_steering_state, steering_state))
            .unwrap_or(0);

        self.spring_force = clamp_force(spring_force.saturating_add(damper_force));
        self.last_steering_state = steering_state;
    }
}

#[derive(Debug)]
pub struct ForzaVJoyRuntime {
    packet_reader: PacketReader,
    effect_engine: EffectEngine,
    forces: ForceState,
    steering_state: i32,
    last_output: DirectControl,
    pending_output: Option<DirectControl>,
}

impl ForzaVJoyRuntime {
    pub fn new(settings: FfbParamsConfig) -> Self {
        Self {
            packet_reader: PacketReader::default(),
            effect_engine: EffectEngine::new(settings),
            forces: ForceState {
                last_steering_state: -1,
                ..ForceState::default()
            },
            steering_state: -1,
            last_output: DirectControl::default(),
            pending_output: None,
        }
    }

    pub fn process_ffb_packet(
        &mut self,
        api: &VJoyApi,
        packet: *const c_void,
    ) -> Result<Option<RuntimeUpdate>, VJoyError> {
        let result = self.packet_reader.process_packet(api, packet)?;
        Ok(self.handle_packet_result(result))
    }

    pub fn handle_packet_result(&mut self, result: PacketProcessResult) -> Option<RuntimeUpdate> {
        let PacketProcessResult::EffectUpdates(updates) = result else {
            return None;
        };

        let mut commands = Vec::new();
        for update in &updates {
            commands.extend(self.effect_engine.translate(update, self.steering_state));
        }

        for command in &commands {
            self.forces.apply_command(command, self.steering_state);
        }

        let direct_control = self.record_output(self.forces.direct_control())?;
        Some(RuntimeUpdate {
            updates,
            commands,
            direct_control,
        })
    }

    pub fn update_steering_state(&mut self, steering_state: i32) -> Option<DirectControl> {
        self.steering_state = steering_state;
        self.forces.update_steering_state(steering_state);
        self.record_output(self.forces.direct_control())
    }

    pub fn apply_input_frames(
        &mut self,
        mapper: &mut InputMapper,
        frames: &[InputFrame],
        vjoy: &mut VJoyDevice,
    ) -> Result<Option<DirectControl>, VJoyDeviceError> {
        mapper.apply_input_frames(frames, vjoy)?;
        Ok(self.update_steering_state(mapper.steering_state))
    }

    pub fn current_output(&self) -> DirectControl {
        self.last_output
    }

    pub fn take_pending_output(&mut self) -> Option<DirectControl> {
        self.pending_output.take()
    }

    fn record_output(&mut self, control: DirectControl) -> Option<DirectControl> {
        let control = control.clamped();
        if control == self.last_output {
            return None;
        }

        self.last_output = control;
        self.pending_output = Some(control);
        Some(control)
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

    pub fn update_steering_state(
        &self,
        steering_state: i32,
    ) -> Result<Option<DirectControl>, ForzaVJoyRuntimeError> {
        let mut runtime = self
            .callback_context
            .runtime
            .lock()
            .map_err(|_| ForzaVJoyRuntimeError::LockPoisoned)?;
        Ok(runtime.update_steering_state(steering_state))
    }

    pub fn current_output(&self) -> Result<DirectControl, ForzaVJoyRuntimeError> {
        let runtime = self
            .callback_context
            .runtime
            .lock()
            .map_err(|_| ForzaVJoyRuntimeError::LockPoisoned)?;
        Ok(runtime.current_output())
    }

    pub fn take_pending_output(&self) -> Result<Option<DirectControl>, ForzaVJoyRuntimeError> {
        let mut runtime = self
            .callback_context
            .runtime
            .lock()
            .map_err(|_| ForzaVJoyRuntimeError::LockPoisoned)?;
        Ok(runtime.take_pending_output())
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

fn spring_force(command: &ConditionCommand, steering_state: i32) -> i32 {
    if steering_state < 0 {
        return 0;
    }

    let centered = steering_state - 32_767 - i32::from(command.center_point_offset);
    let adjusted = apply_dead_band(centered, command.dead_band);
    if adjusted == 0 {
        return 0;
    }

    if adjusted > 0 {
        let force = adjusted.saturating_mul(command.positive_coefficient.abs()) / 32_767;
        -force.clamp(0, command.positive_saturation)
    } else {
        let force = adjusted
            .abs()
            .saturating_mul(command.negative_coefficient.abs())
            / 32_767;
        force.clamp(0, command.negative_saturation)
    }
}

fn damper_force(command: &ConditionCommand, last_steering_state: i32, steering_state: i32) -> i32 {
    if last_steering_state < 0 || steering_state < 0 {
        return 0;
    }

    let velocity = steering_state - last_steering_state;
    if velocity == 0 {
        return 0;
    }

    if velocity > 0 {
        let force = velocity.saturating_mul(command.positive_coefficient.abs()) / 32_767;
        -force.clamp(0, command.positive_saturation)
    } else {
        let force = velocity
            .abs()
            .saturating_mul(command.negative_coefficient.abs())
            / 32_767;
        force.clamp(0, command.negative_saturation)
    }
}

fn apply_dead_band(value: i32, dead_band: i32) -> i32 {
    if value.abs() <= dead_band {
        0
    } else if value > 0 {
        value - dead_band
    } else {
        value + dead_band
    }
}

fn clamp_force(force: i32) -> i16 {
    force.clamp(-10_000, 10_000) as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConditionFfbConfig, ConstFfbConfig, PeriodicFfbConfig, VibrationConfig};
    use crate::core::domain::{EffectMetadata, GameEffect};

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
    fn constant_update_sets_pending_direct_control() {
        let mut runtime = ForzaVJoyRuntime::new(settings());
        let update = runtime
            .handle_packet_result(PacketProcessResult::EffectUpdates(vec![
                EffectUpdate::Apply(GameEffect::Constant {
                    metadata: metadata(255),
                    magnitude: 8_000,
                }),
            ]))
            .expect("runtime update");

        assert_eq!(update.direct_control.constant_force, -5_000);
        assert_eq!(runtime.take_pending_output(), Some(update.direct_control));
    }

    #[test]
    fn stop_effect_clears_constant_force() {
        let mut runtime = ForzaVJoyRuntime::new(settings());
        let _ = runtime.handle_packet_result(PacketProcessResult::EffectUpdates(vec![
            EffectUpdate::Apply(GameEffect::Constant {
                metadata: metadata(255),
                magnitude: 8_000,
            }),
        ]));

        let update = runtime
            .handle_packet_result(PacketProcessResult::EffectUpdates(vec![
                EffectUpdate::StopEffect(EffectKind::Constant),
            ]))
            .expect("runtime update");

        assert_eq!(update.direct_control.constant_force, 0);
    }

    #[test]
    fn steering_update_recomputes_spring_force() {
        let mut runtime = ForzaVJoyRuntime::new(settings());
        let _ = runtime.handle_packet_result(PacketProcessResult::EffectUpdates(vec![
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
        ]));

        let control = runtime
            .update_steering_state(45_000)
            .expect("pending control from steering update");

        assert!(control.spring_force < 0);
    }
}
