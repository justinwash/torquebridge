#![allow(dead_code)]

use crate::core::domain::{
    DeviceControlCommand, EffectKind, EffectMetadata, EffectUpdate, GameEffect,
};
use crate::vjoy::{VJoyApi, VJoyError, ffi};
use std::ffi::c_void;

#[derive(Debug, Clone, Copy, Default)]
struct RawPacketState {
    effect_report: Option<ffi::RawEffReport>,
    envelope_report: Option<ffi::RawEffEnvelope>,
    conditional_report: Option<ffi::RawEffCond>,
    periodic_report: Option<ffi::RawEffPeriod>,
    constant_force_report: Option<ffi::RawEffConstant>,
    ramp_force_report: Option<ffi::RawEffRamp>,
    op: Option<ffi::RawEffOp>,
    gain: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketProcessResult {
    IgnoredDuplicate,
    Updated,
    EffectUpdates(Vec<EffectUpdate>),
}

#[derive(Debug, Default)]
pub struct PacketReader {
    current: RawPacketState,
    old_effect_report: Option<ffi::RawEffReport>,
    old_conditional_report: Option<ffi::RawEffCond>,
    old_periodic_report: Option<ffi::RawEffPeriod>,
    old_constant_force_report: Option<ffi::RawEffConstant>,
    old_envelope_report: Option<ffi::RawEffEnvelope>,
    old_ramp_force_report: Option<ffi::RawEffRamp>,
    old_gain: Option<u8>,
}

pub type ProcessResult = PacketProcessResult;
pub type FfbPacketHandlerState = PacketReader;

impl PacketReader {
    pub fn process_packet(
        &mut self,
        api: &VJoyApi,
        packet: *const c_void,
    ) -> Result<PacketProcessResult, VJoyError> {
        let packet_type = api.parse_packet_type(packet)?;

        match packet_type {
            ffi::RawPacketType::EffRep => {
                let report = api.parse_effect_report(packet)?;
                if self
                    .old_effect_report
                    .is_some_and(|old| old.semantic_eq(report))
                {
                    return Ok(PacketProcessResult::IgnoredDuplicate);
                }

                self.old_effect_report = Some(report);
                self.current.effect_report = Some(report);
                self.emit_current_updates()
            }
            ffi::RawPacketType::EnvRep => {
                let report = api.parse_envelope_report(packet)?;
                if self
                    .old_envelope_report
                    .is_some_and(|old| old.semantic_eq(report))
                {
                    return Ok(PacketProcessResult::IgnoredDuplicate);
                }

                self.old_envelope_report = Some(report);
                self.current.envelope_report = Some(report);
                Ok(PacketProcessResult::Updated)
            }
            ffi::RawPacketType::CondRep => {
                let report = api.parse_condition_report(packet)?;
                if self
                    .old_conditional_report
                    .is_some_and(|old| old.semantic_eq(report))
                {
                    return Ok(PacketProcessResult::IgnoredDuplicate);
                }

                self.old_conditional_report = Some(report);
                self.current.conditional_report = Some(report);
                self.emit_current_updates()
            }
            ffi::RawPacketType::PridRep => {
                let report = api.parse_periodic_report(packet)?;
                if self
                    .old_periodic_report
                    .is_some_and(|old| old.semantic_eq(report))
                {
                    return Ok(PacketProcessResult::IgnoredDuplicate);
                }

                self.old_periodic_report = Some(report);
                self.current.periodic_report = Some(report);
                self.emit_current_updates()
            }
            ffi::RawPacketType::ConstRep => {
                let report = api.parse_constant_report(packet)?;
                if self
                    .old_constant_force_report
                    .is_some_and(|old| old.semantic_eq(report))
                {
                    return Ok(PacketProcessResult::IgnoredDuplicate);
                }

                self.old_constant_force_report = Some(report);
                self.current.constant_force_report = Some(report);
                self.emit_current_updates()
            }
            ffi::RawPacketType::RampRep => {
                let report = api.parse_ramp_report(packet)?;
                if self
                    .old_ramp_force_report
                    .is_some_and(|old| old.semantic_eq(report))
                {
                    return Ok(PacketProcessResult::IgnoredDuplicate);
                }

                self.old_ramp_force_report = Some(report);
                self.current.ramp_force_report = Some(report);
                Ok(PacketProcessResult::Updated)
            }
            ffi::RawPacketType::EfOpRep => {
                let report = api.parse_op_report(packet)?;
                self.current.op = Some(report);
                self.emit_current_updates()
            }
            ffi::RawPacketType::GainRep => {
                let gain = api.parse_gain_report(packet)?;
                if self.old_gain == Some(gain) {
                    return Ok(PacketProcessResult::IgnoredDuplicate);
                }

                self.old_gain = Some(gain);
                self.current.gain = Some(gain);
                Ok(PacketProcessResult::Updated)
            }
            ffi::RawPacketType::CtrlRep => {
                let control = api.parse_control_report(packet)?;
                Ok(match map_control_update(control) {
                    Some(update) => PacketProcessResult::EffectUpdates(vec![update]),
                    None => PacketProcessResult::Updated,
                })
            }
            _ => Ok(PacketProcessResult::Updated),
        }
    }

    fn emit_current_updates(&self) -> Result<PacketProcessResult, VJoyError> {
        let Some(effect_report) = self.current.effect_report else {
            return Ok(PacketProcessResult::Updated);
        };

        let effect_kind = map_effect_kind(effect_report.effect_type());
        if matches!(effect_kind, EffectKind::None) {
            return Ok(PacketProcessResult::Updated);
        }

        let mut updates = Vec::new();

        if self.current.op.map(|op| op.effect_op()) == Some(ffi::RawEffectOperation::Stop) {
            updates.push(EffectUpdate::StopEffect(effect_kind));
        }

        let metadata = effect_metadata(effect_report);
        match effect_kind {
            EffectKind::Constant => {
                if let Some(report) = self.current.constant_force_report {
                    updates.push(EffectUpdate::Apply(GameEffect::Constant {
                        metadata,
                        magnitude: i32::from(report.magnitude),
                    }));
                }
            }
            EffectKind::Sine
            | EffectKind::Square
            | EffectKind::Triangle
            | EffectKind::SawUp
            | EffectKind::SawDown => {
                if let Some(report) = self.current.periodic_report {
                    updates.push(EffectUpdate::Apply(GameEffect::Periodic {
                        metadata,
                        magnitude: report.magnitude as i32,
                        offset: report.offset,
                        period: report.period as i32,
                        phase: report.phase as i32,
                    }));
                }
            }
            EffectKind::Spring
            | EffectKind::Damper
            | EffectKind::Inertia
            | EffectKind::Friction => {
                if let Some(report) = self.current.conditional_report {
                    updates.push(EffectUpdate::Apply(GameEffect::Condition {
                        metadata,
                        kind: effect_kind,
                        dead_band: report.dead_band,
                        center_point_offset: report.center_point_offset,
                        positive_coefficient: i32::from(report.pos_coeff),
                        negative_coefficient: i32::from(report.neg_coeff),
                        positive_saturation: report.pos_satur as i32,
                        negative_saturation: report.neg_satur as i32,
                    }));
                }
            }
            EffectKind::Ramp | EffectKind::Custom | EffectKind::Unknown(_) => {}
            EffectKind::None => {}
        }

        if updates.is_empty() {
            Ok(PacketProcessResult::Updated)
        } else {
            Ok(PacketProcessResult::EffectUpdates(updates))
        }
    }
}

fn map_control_update(control: ffi::RawDeviceControl) -> Option<EffectUpdate> {
    match control {
        ffi::RawDeviceControl::StopAll | ffi::RawDeviceControl::DisAct => {
            Some(EffectUpdate::DeviceControl(DeviceControlCommand::StopAll))
        }
        ffi::RawDeviceControl::DevRst => {
            Some(EffectUpdate::DeviceControl(DeviceControlCommand::Reset))
        }
        ffi::RawDeviceControl::DevPause => {
            Some(EffectUpdate::DeviceControl(DeviceControlCommand::Pause))
        }
        ffi::RawDeviceControl::DevCont => {
            Some(EffectUpdate::DeviceControl(DeviceControlCommand::Continue))
        }
        ffi::RawDeviceControl::EnAct | ffi::RawDeviceControl::Unknown(_) => None,
    }
}

fn map_effect_kind(effect_type: ffi::RawEffectType) -> EffectKind {
    match effect_type {
        ffi::RawEffectType::None => EffectKind::None,
        ffi::RawEffectType::Constant => EffectKind::Constant,
        ffi::RawEffectType::Ramp => EffectKind::Ramp,
        ffi::RawEffectType::Square => EffectKind::Square,
        ffi::RawEffectType::Sine => EffectKind::Sine,
        ffi::RawEffectType::Triangle => EffectKind::Triangle,
        ffi::RawEffectType::SawUp => EffectKind::SawUp,
        ffi::RawEffectType::SawDown => EffectKind::SawDown,
        ffi::RawEffectType::Spring => EffectKind::Spring,
        ffi::RawEffectType::Damper => EffectKind::Damper,
        ffi::RawEffectType::Inertia => EffectKind::Inertia,
        ffi::RawEffectType::Friction => EffectKind::Friction,
        ffi::RawEffectType::Custom => EffectKind::Custom,
        ffi::RawEffectType::Unknown(raw) => EffectKind::Unknown(raw),
    }
}

fn effect_metadata(effect_report: ffi::RawEffReport) -> EffectMetadata {
    let duration_ms = if effect_report.duration == u16::MAX {
        -1
    } else {
        effect_report.duration as i32
    };

    let gain = ((effect_report.gain as f32 / 255.0) * 10_000.0).round() as i32;
    let trigger_button = if effect_report.triger_btn == u8::MAX {
        -1
    } else {
        effect_report.triger_btn as i32
    };

    EffectMetadata {
        duration_ms,
        gain,
        raw_gain: effect_report.gain,
        direction: effect_report.direction,
        sample_period: effect_report.sample_prd,
        trigger_button,
        trigger_repeat_interval: effect_report.triger_rpt,
    }
}
