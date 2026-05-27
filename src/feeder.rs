use crate::config::{ButtonConfig, ControllerConfig};
use crate::core::domain::InputFrame;
use crate::vjoy::{VJoyDevice, VJoyDeviceError, VirtualJoystickReport};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisKind {
    X,
    Y,
    Z,
    RotationX,
    RotationY,
    RotationZ,
    Sliders0,
    Sliders1,
}

impl AxisKind {
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::X),
            1 => Some(Self::Y),
            2 => Some(Self::Z),
            3 => Some(Self::RotationX),
            4 => Some(Self::RotationY),
            5 => Some(Self::RotationZ),
            6 => Some(Self::Sliders0),
            7 => Some(Self::Sliders1),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AxisData {
    pub axis_kind: AxisKind,
    pub controller_index: usize,
    pub inverted: i32,
    pub deadzone: f32,
}

#[derive(Debug, Clone)]
pub struct ButtonData {
    pub controller_index: usize,
    pub button_index: usize,
    pub button_bind: usize,
}

#[derive(Debug, Clone)]
pub struct DPadData {
    pub controller_index: usize,
    pub dpad_index: usize,
}

#[derive(Debug, Clone, Default)]
pub struct InputMapping {
    pub steering: Option<AxisData>,
    pub combined: Option<AxisData>,
    pub throttle: Option<AxisData>,
    pub brake: Option<AxisData>,
    pub clutch: Option<AxisData>,
    pub handbrake: Option<AxisData>,
    pub buttons: Vec<ButtonData>,
    pub dpad: Option<DPadData>,
}

#[derive(Debug, Clone, Default)]
struct InputSnapshot {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub rotation_x: i32,
    pub rotation_y: i32,
    pub rotation_z: i32,
    pub sliders: [i32; 2],
    pub buttons: Vec<bool>,
    pub point_of_view_controllers: Vec<i32>,
}

impl InputSnapshot {
    pub fn axis_value(&self, axis: AxisKind) -> i32 {
        match axis {
            AxisKind::X => self.x,
            AxisKind::Y => self.y,
            AxisKind::Z => self.z,
            AxisKind::RotationX => self.rotation_x,
            AxisKind::RotationY => self.rotation_y,
            AxisKind::RotationZ => self.rotation_z,
            AxisKind::Sliders0 => self.sliders[0],
            AxisKind::Sliders1 => self.sliders[1],
        }
    }
}

impl From<&InputFrame> for InputSnapshot {
    fn from(frame: &InputFrame) -> Self {
        Self {
            x: frame.x,
            y: frame.y,
            z: frame.z,
            rotation_x: frame.rotation_x,
            rotation_y: frame.rotation_y,
            rotation_z: frame.rotation_z,
            sliders: frame.sliders,
            buttons: frame.buttons.clone(),
            point_of_view_controllers: frame.point_of_view_controllers.clone(),
        }
    }
}

pub struct InputMapper {
    pub steering_state: i32,
    pub mapping: InputMapping,
}

impl InputMapper {
    pub fn from_config(controllers: &[ControllerConfig]) -> Self {
        Self {
            steering_state: -1,
            mapping: InputMapping::from_config(controllers),
        }
    }

    pub fn reset_vjoy_state(&mut self, vjoy: &mut VJoyDevice) {
        vjoy.reset_report();
    }

    fn apply_snapshots(
        &mut self,
        snapshots: &[InputSnapshot],
        vjoy: &mut VJoyDevice,
    ) -> Result<(), VJoyDeviceError> {
        let report = vjoy.report_mut();

        if let Some(mapping) = &self.mapping.steering {
            let mut value = snapshots[mapping.controller_index].axis_value(mapping.axis_kind);
            self.steering_state = value;
            value = deadzone_and_invert(value, mapping, true);
            report.axis_x = (value as f32 / 2.0).round() as i32;
        }

        if let Some(mapping) = &self.mapping.combined {
            let value = snapshots[mapping.controller_index].axis_value(mapping.axis_kind);
            let inverted = mapping.inverted;
            if value > 32768 {
                let split = combined_axis_split(value, mapping, inverted);
                report.axis_x_rot = (split as f32 / 2.0).round() as i32;
                report.axis_z = if inverted == 1 { 0 } else { 65535 };
            } else if value < 32768 {
                let split = combined_axis_split(value, mapping, inverted);
                report.axis_z = (split as f32 / 2.0).round() as i32;
                report.axis_x_rot = if inverted == 1 { 0 } else { 65535 };
            }
        }

        set_axis_target(report, snapshots, &self.mapping.throttle, AxisTarget::AxisZ);
        set_axis_target(report, snapshots, &self.mapping.brake, AxisTarget::AxisXRot);
        set_axis_target(
            report,
            snapshots,
            &self.mapping.clutch,
            AxisTarget::AxisYRot,
        );
        set_axis_target(
            report,
            snapshots,
            &self.mapping.handbrake,
            AxisTarget::AxisZRot,
        );

        if !self.mapping.buttons.is_empty() {
            let buttons = get_button_array(snapshots, &self.mapping.buttons);
            report.buttons = pack_bools_in_u32(&buttons);
        }

        if let Some(dpad) = &self.mapping.dpad {
            let pov = get_pov_array(snapshots, dpad);
            report.hats = pack_bools(&pov).first().copied().unwrap_or(0).into();
        }

        vjoy.update()
    }

    pub fn apply_input_frames(
        &mut self,
        frames: &[InputFrame],
        vjoy: &mut VJoyDevice,
    ) -> Result<(), VJoyDeviceError> {
        let snapshots: Vec<InputSnapshot> = frames.iter().map(InputSnapshot::from).collect();
        self.apply_snapshots(&snapshots, vjoy)
    }
}

enum AxisTarget {
    AxisZ,
    AxisXRot,
    AxisYRot,
    AxisZRot,
}

fn set_axis_target(
    report: &mut VirtualJoystickReport,
    snapshots: &[InputSnapshot],
    mapping: &Option<AxisData>,
    target: AxisTarget,
) {
    let Some(mapping) = mapping else {
        return;
    };

    let mut value = snapshots[mapping.controller_index].axis_value(mapping.axis_kind);
    value = deadzone_and_invert(value, mapping, false);
    let scaled = (value as f32 / 2.0).round() as i32;

    match target {
        AxisTarget::AxisZ => report.axis_z = scaled,
        AxisTarget::AxisXRot => report.axis_x_rot = scaled,
        AxisTarget::AxisYRot => report.axis_y_rot = scaled,
        AxisTarget::AxisZRot => report.axis_z_rot = scaled,
    }
}

impl InputMapping {
    pub fn from_config(controllers: &[ControllerConfig]) -> Self {
        let mut mapping = InputMapping::default();
        let mut grouped_buttons: BTreeMap<usize, Vec<ButtonData>> = BTreeMap::new();

        for (controller_index, controller) in controllers.iter().enumerate() {
            if let Some(axes) = &controller.axes {
                for axis in axes {
                    if let Some(axis_kind) = AxisKind::from_index(axis.axis_index) {
                        let axis_data = AxisData {
                            axis_kind,
                            controller_index,
                            inverted: axis.inverted,
                            deadzone: axis.deadzone,
                        };

                        match axis.id.as_str() {
                            "Steering" => mapping.steering = Some(axis_data),
                            "Combined" => mapping.combined = Some(axis_data),
                            "Throttle" => mapping.throttle = Some(axis_data),
                            "Brake" => mapping.brake = Some(axis_data),
                            "Clutch" => mapping.clutch = Some(axis_data),
                            "Handbrake" => mapping.handbrake = Some(axis_data),
                            _ => {}
                        }
                    }
                }
            }

            if let Some(buttons) = &controller.buttons {
                for button in buttons {
                    if let Some(bind) = button_bind(button) {
                        grouped_buttons.entry(bind).or_default().push(ButtonData {
                            controller_index,
                            button_index: button.index,
                            button_bind: bind,
                        });
                    }
                }
            }

            if let Some(dpad) = &controller.d_pad {
                mapping.dpad = Some(DPadData {
                    controller_index,
                    dpad_index: dpad.index,
                });
            }
        }

        mapping.buttons = grouped_buttons.into_values().flatten().collect();
        mapping
    }
}

fn button_bind(button: &ButtonConfig) -> Option<usize> {
    if let Some(number) = button.id.as_i64() {
        return usize::try_from(number).ok();
    }

    let name = button.id.as_str()?;
    Some(match name {
        "XboxView" => 0,
        "XboxMenu" => 1,
        "XBoxA" => 2,
        "XBoxB" => 3,
        "XBoxX" => 4,
        "XBoxY" => 5,
        "Button7" => 6,
        "Button8" => 7,
        "Button9" => 8,
        "Button10" => 9,
        "Button11" => 10,
        "Button12" => 11,
        "Button13" => 12,
        "Button14" => 13,
        "Button15" => 14,
        "Button16" => 15,
        "PreviousGear" => 16,
        "NextGear" => 17,
        "ReverseGear" => 18,
        "ForwardGear1" => 19,
        "ForwardGear2" => 20,
        "ForwardGear3" => 21,
        "ForwardGear4" => 22,
        "ForwardGear5" => 23,
        "ForwardGear6" => 24,
        "ForwardGear7" => 25,
        "Button27" => 26,
        "Button28" => 27,
        "Button29" => 28,
        "Button30" => 29,
        "Button31" => 30,
        "Button32" => 31,
        _ => return None,
    })
}

pub fn combined_axis_split(value: i32, mapping: &AxisData, inverted: i32) -> i32 {
    let mut value = value;
    if value > 32767 + (mapping.deadzone * 65535.0 / 2.0) as i32 {
        value = (((value as f64 - (mapping.deadzone * 65535.0) as f64)
            / (1.0 - mapping.deadzone as f64)
            - 32767.0)
            * 2.0)
            .round() as i32;
        if inverted == 0 {
            value = (value - 65535).abs();
        }
    } else if value >= 32767 - (mapping.deadzone * 65535.0 / 2.0) as i32 {
        value = if inverted != 1 { 65535 } else { 0 };
    } else {
        value = (value - 65535).abs();
        value = (((value as f64 - (mapping.deadzone * 65535.0) as f64)
            / (1.0 - mapping.deadzone as f64)
            - 32767.0)
            * 2.0)
            .round() as i32;
        if inverted == 0 {
            value = (value - 65535).abs();
        }
    }
    value
}

pub fn deadzone_and_invert(mut value: i32, mapping: &AxisData, steering: bool) -> i32 {
    if mapping.inverted != 0 {
        value = (value - 65535).abs();
    }

    if steering {
        if value < 32767 && (value - 32767).abs() < (mapping.deadzone * 32768.0) as i32 {
            value = 32767;
        } else if value > 32767 && value - 32767 < (mapping.deadzone * 32768.0) as i32 {
            value = 32767;
        } else if value < 32767 - (mapping.deadzone * 65535.0 / 2.0) as i32 {
            value = (value - 65535).abs();
            let span = value - (mapping.deadzone * 65535.0) as i32;
            let range = 65535 - (mapping.deadzone * 65535.0) as i32;
            value = ((span as f32 / range as f32) * 65535.0).round() as i32;
            value = (value - 65535).abs();
        } else if value > 32767 + (mapping.deadzone * 65535.0 / 2.0) as i32 {
            let span = value - (mapping.deadzone * 65535.0) as i32;
            let range = 65535 - (mapping.deadzone * 65535.0) as i32;
            value = ((span as f32 / range as f32) * 65535.0).round() as i32;
        }
    } else if mapping.inverted == 0
        && value > (mapping.deadzone * 65535.0).abs() as i32
        && mapping.deadzone != 0.0
    {
        value = 65535;
    } else if mapping.inverted == 1
        && value < (mapping.deadzone * 65535.0) as i32
        && mapping.deadzone != 0.0
    {
        value = 0;
    } else if mapping.inverted == 0 || mapping.inverted == -1 {
        value = (value - 65535).abs();
        let span = value - (mapping.deadzone * 65535.0) as i32;
        let range = 65535 - (mapping.deadzone * 65535.0) as i32;
        value = ((span as f32 / range as f32) * 65535.0).round() as i32;
        value = (value - 65535).abs();
    } else {
        let span = value - (mapping.deadzone * 65535.0) as i32;
        let range = 65535 - (mapping.deadzone * 65535.0) as i32;
        value = ((span as f32 / range as f32) * 65535.0).round() as i32;
    }

    value
}

fn get_pov_array(snapshots: &[InputSnapshot], mapping: &DPadData) -> [bool; 4] {
    let pov = snapshots
        .get(mapping.controller_index)
        .and_then(|snapshot| snapshot.point_of_view_controllers.get(mapping.dpad_index))
        .copied()
        .unwrap_or(-1);

    let mut out = [true, true, true, true];
    if pov != -1 {
        out = [false, false, false, false];
        if pov == 9000 {
            out[0] = true;
        } else if pov == 18000 {
            out[1] = true;
        } else if pov == 27000 {
            out[0] = true;
            out[1] = true;
        }
    }

    out
}

fn get_button_array(snapshots: &[InputSnapshot], mapping: &[ButtonData]) -> [bool; 128] {
    let mut out = [false; 128];
    for button in mapping {
        if !out[button.button_bind] {
            let pressed = snapshots
                .get(button.controller_index)
                .and_then(|snapshot| snapshot.buttons.get(button.button_index))
                .copied()
                .unwrap_or(false);
            out[button.button_bind] = pressed;
        }
    }
    out
}

fn pack_bools_in_u32(values: &[bool; 128]) -> u32 {
    let packed = pack_bools(values);
    let mut first = [0u8; 4];
    first.copy_from_slice(&packed[..4]);
    u32::from_le_bytes(first)
}

fn pack_bools(values: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; values.len().div_ceil(8)];
    for (index, value) in values.iter().enumerate() {
        if *value {
            out[index >> 3] |= 1 << (index & 7);
        }
    }
    out
}
