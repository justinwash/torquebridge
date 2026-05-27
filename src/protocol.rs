use crate::constants::{DATA_OVERRIDE_DIRECT_CONTROL, REPORT_SIZE};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirectControl {
    pub spring_force: i16,
    pub constant_force: i16,
    pub periodic_force: i16,
    pub force_drop: u8,
}

impl DirectControl {
    pub fn clamped(self) -> Self {
        Self {
            spring_force: self.spring_force.clamp(-10_000, 10_000),
            constant_force: self.constant_force.clamp(-10_000, 10_000),
            periodic_force: self.periodic_force.clamp(-10_000, 10_000),
            force_drop: self.force_drop.min(100),
        }
    }

    pub fn to_report_payload(self) -> [u8; REPORT_SIZE] {
        let c = self.clamped();
        let mut payload = [0u8; REPORT_SIZE];
        payload[0] = DATA_OVERRIDE_DIRECT_CONTROL;
        payload[1..3].copy_from_slice(&c.spring_force.to_le_bytes());
        payload[3..5].copy_from_slice(&c.constant_force.to_le_bytes());
        payload[5..7].copy_from_slice(&c.periodic_force.to_le_bytes());
        payload[7] = c.force_drop;
        payload
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceState {
    pub firmware_raw: [u8; 4],
    pub is_registered: u8,
    pub position_raw: i16,
    pub torque_raw: i16,
}

impl DeviceState {
    pub fn from_payload(payload: &[u8; REPORT_SIZE]) -> Self {
        Self {
            firmware_raw: [payload[0], payload[1], payload[2], payload[3]],
            is_registered: payload[4],
            position_raw: i16::from_le_bytes([payload[5], payload[6]]),
            torque_raw: i16::from_le_bytes([payload[7], payload[8]]),
        }
    }

    pub fn position_norm(self) -> f32 {
        self.position_raw as f32 / 10_000.0
    }

    pub fn torque_norm(self) -> f32 {
        self.torque_raw as f32 / 10_000.0
    }
}
