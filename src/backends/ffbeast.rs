pub use crate::device::{DeviceError, FFBeastDevice};
pub use crate::protocol::{DeviceState, DirectControl};

pub struct FFBeastBackend {
    device: FFBeastDevice,
}

impl FFBeastBackend {
    pub fn connect() -> Result<Self, DeviceError> {
        Ok(Self {
            device: FFBeastDevice::connect()?,
        })
    }

    pub fn send_direct_control(&self, control: DirectControl) -> Result<(), DeviceError> {
        self.device.send_direct_control(control)
    }

    pub fn set_device_gain(&self, gain_percent: u8) -> Result<(), DeviceError> {
        self.device.set_device_gain(gain_percent)
    }

    pub fn read_state_blocking(&self, timeout_ms: i32) -> Result<DeviceState, DeviceError> {
        self.device.read_state_blocking(timeout_ms)
    }

    pub fn device(&self) -> &FFBeastDevice {
        &self.device
    }
}
