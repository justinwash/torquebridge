use crate::constants::{
    FFBEAST_PID, FFBEAST_VID, OUTPUT_REPORT_SIZE, REPORT_DEVICE_STATE, REPORT_SIZE,
};
use crate::protocol::{DeviceState, DirectControl};
use hidapi::{HidApi, HidDevice};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("FFBeast wheel not found (VID:PID {0:04x}:{1:04x})")]
    DeviceNotFound(u16, u16),
    #[error("hidapi error: {0}")]
    Hid(#[from] hidapi::HidError),
}

pub struct FFBeastDevice {
    handle: HidDevice,
}

impl FFBeastDevice {
    pub fn connect() -> Result<Self, DeviceError> {
        let api = HidApi::new()?;
        let info = api
            .device_list()
            .find(|d| d.vendor_id() == FFBEAST_VID && d.product_id() == FFBEAST_PID)
            .ok_or(DeviceError::DeviceNotFound(FFBEAST_VID, FFBEAST_PID))?;

        let handle = info.open_device(&api)?;
        Ok(Self { handle })
    }

    pub fn send_output_report(
        &self,
        report_id: u8,
        payload: &[u8; REPORT_SIZE],
    ) -> Result<(), DeviceError> {
        let mut report = [0u8; OUTPUT_REPORT_SIZE];
        report[0] = report_id;
        report[1..].copy_from_slice(payload);
        self.handle.write(&report)?;
        Ok(())
    }

    pub fn send_direct_control(&self, control: DirectControl) -> Result<(), DeviceError> {
        self.send_output_report(REPORT_DEVICE_STATE, &control.to_report_payload())
    }

    pub fn set_device_gain(&self, gain_percent: u8) -> Result<(), DeviceError> {
        let mut payload = [0u8; REPORT_SIZE];
        payload[0] = gain_percent.min(100);
        self.send_output_report(crate::constants::REPORT_DEVICE_GAIN, &payload)
    }

    pub fn read_state_blocking(&self, timeout_ms: i32) -> Result<DeviceState, DeviceError> {
        let mut buf = [0u8; OUTPUT_REPORT_SIZE];
        let bytes = self.handle.read_timeout(&mut buf, timeout_ms)?;
        if bytes == 0 {
            return Ok(DeviceState {
                firmware_raw: [0, 0, 0, 0],
                is_registered: 0,
                position_raw: 0,
                torque_raw: 0,
            });
        }

        let offset = if buf[0] == REPORT_DEVICE_STATE { 1 } else { 0 };
        let mut payload = [0u8; REPORT_SIZE];
        let available = bytes.saturating_sub(offset).min(REPORT_SIZE);
        payload[..available].copy_from_slice(&buf[offset..offset + available]);
        Ok(DeviceState::from_payload(&payload))
    }
}
