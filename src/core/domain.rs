#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceControlCommand {
    Reset,
    StopAll,
    Pause,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    None,
    Constant,
    Ramp,
    Square,
    Sine,
    Triangle,
    SawUp,
    SawDown,
    Spring,
    Damper,
    Inertia,
    Friction,
    Custom,
    Unknown(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectMetadata {
    pub duration_ms: i32,
    pub gain: i32,
    pub raw_gain: u8,
    pub direction: u8,
    pub sample_period: u16,
    pub trigger_button: i32,
    pub trigger_repeat_interval: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEffect {
    Constant {
        metadata: EffectMetadata,
        magnitude: i32,
    },
    Periodic {
        metadata: EffectMetadata,
        magnitude: i32,
        offset: i16,
        period: i32,
        phase: i32,
    },
    Condition {
        metadata: EffectMetadata,
        kind: EffectKind,
        dead_band: i32,
        center_point_offset: i16,
        positive_coefficient: i32,
        negative_coefficient: i32,
        positive_saturation: i32,
        negative_saturation: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectUpdate {
    Ignore,
    DeviceControl(DeviceControlCommand),
    StopEffect(EffectKind),
    Apply(GameEffect),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantCommand {
    pub metadata: EffectMetadata,
    pub magnitude: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeriodicCommand {
    pub metadata: EffectMetadata,
    pub magnitude: i32,
    pub offset: i16,
    pub period: i32,
    pub phase: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionCommand {
    pub metadata: EffectMetadata,
    pub kind: EffectKind,
    pub dead_band: i32,
    pub center_point_offset: i16,
    pub positive_coefficient: i32,
    pub negative_coefficient: i32,
    pub positive_saturation: i32,
    pub negative_saturation: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WheelCommand {
    Ignore,
    DeviceControl(DeviceControlCommand),
    StopEffect(EffectKind),
    Constant(ConstantCommand),
    Periodic(PeriodicCommand),
    Condition(ConditionCommand),
}

#[derive(Debug, Clone, Default)]
pub struct InputFrame {
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

impl InputFrame {
    pub fn axis_value(&self, axis_index: i32) -> Option<i32> {
        match axis_index {
            0 => Some(self.x),
            1 => Some(self.y),
            2 => Some(self.z),
            3 => Some(self.rotation_x),
            4 => Some(self.rotation_y),
            5 => Some(self.rotation_z),
            6 => Some(self.sliders[0]),
            7 => Some(self.sliders[1]),
            _ => None,
        }
    }
}
