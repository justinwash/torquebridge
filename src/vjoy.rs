#![allow(dead_code)]

pub(crate) mod ffi;

use self::ffi::{RawApi, RawJoystickState};

pub use self::ffi::VJoyError;
use std::ffi::c_void;
use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub enum HidUsage {
    X,
    Y,
    Z,
    Rx,
    Ry,
    Rz,
    Sl0,
    Sl1,
}

impl HidUsage {
    pub const REQUIRED_AXES: [HidUsage; 8] = [
        HidUsage::X,
        HidUsage::Y,
        HidUsage::Z,
        HidUsage::Rx,
        HidUsage::Ry,
        HidUsage::Rz,
        HidUsage::Sl0,
        HidUsage::Sl1,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HidUsage::X => "X",
            HidUsage::Y => "Y",
            HidUsage::Z => "Z",
            HidUsage::Rx => "RX",
            HidUsage::Ry => "RY",
            HidUsage::Rz => "RZ",
            HidUsage::Sl0 => "Slider",
            HidUsage::Sl1 => "Dial/Slider1",
        }
    }

    fn raw(self) -> u32 {
        match self {
            HidUsage::X => 48,
            HidUsage::Y => 49,
            HidUsage::Z => 50,
            HidUsage::Rx => 51,
            HidUsage::Ry => 52,
            HidUsage::Rz => 53,
            HidUsage::Sl0 => 54,
            HidUsage::Sl1 => 55,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VjdStatus {
    Owned,
    Free,
    Busy,
    Missing,
    Unknown(i32),
}

impl VjdStatus {
    fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Owned,
            1 => Self::Free,
            2 => Self::Busy,
            3 => Self::Missing,
            _ => Self::Unknown(raw),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VirtualJoystickReport {
    pub device_id: u8,
    pub throttle: i32,
    pub rudder: i32,
    pub aileron: i32,
    pub axis_x: i32,
    pub axis_y: i32,
    pub axis_z: i32,
    pub axis_x_rot: i32,
    pub axis_y_rot: i32,
    pub axis_z_rot: i32,
    pub slider: i32,
    pub dial: i32,
    pub wheel: i32,
    pub axis_vx: i32,
    pub axis_vy: i32,
    pub axis_vz: i32,
    pub axis_vbrx: i32,
    pub axis_vbry: i32,
    pub axis_vbrz: i32,
    pub buttons: u32,
    pub hats: u32,
    pub hats_ex1: u32,
    pub hats_ex2: u32,
    pub hats_ex3: u32,
    pub buttons_ex1: u32,
    pub buttons_ex2: u32,
    pub buttons_ex3: u32,
}

impl From<&VirtualJoystickReport> for RawJoystickState {
    fn from(report: &VirtualJoystickReport) -> Self {
        Self {
            b_device: report.device_id,
            throttle: report.throttle,
            rudder: report.rudder,
            aileron: report.aileron,
            axis_x: report.axis_x,
            axis_y: report.axis_y,
            axis_z: report.axis_z,
            axis_x_rot: report.axis_x_rot,
            axis_y_rot: report.axis_y_rot,
            axis_z_rot: report.axis_z_rot,
            slider: report.slider,
            dial: report.dial,
            wheel: report.wheel,
            axis_vx: report.axis_vx,
            axis_vy: report.axis_vy,
            axis_vz: report.axis_vz,
            axis_vbrx: report.axis_vbrx,
            axis_vbry: report.axis_vbry,
            axis_vbrz: report.axis_vbrz,
            buttons: report.buttons,
            b_hats: report.hats,
            b_hats_ex1: report.hats_ex1,
            b_hats_ex2: report.hats_ex2,
            b_hats_ex3: report.hats_ex3,
            buttons_ex1: report.buttons_ex1,
            buttons_ex2: report.buttons_ex2,
            buttons_ex3: report.buttons_ex3,
        }
    }
}

pub struct VJoyApi {
    raw: RawApi,
}

impl VJoyApi {
    pub fn load() -> Result<Self, VJoyError> {
        Ok(Self {
            raw: RawApi::load()?,
        })
    }

    pub fn vjoy_enabled(&self) -> bool {
        self.raw.vjoy_enabled()
    }

    pub fn get_vjd_status(&self, id: u32) -> VjdStatus {
        VjdStatus::from_raw(self.raw.get_vjd_status(id))
    }

    pub fn axis_exists(&self, id: u32, usage: HidUsage) -> bool {
        self.raw.axis_exists(id, usage.raw())
    }

    pub fn button_count(&self, id: u32) -> i32 {
        self.raw.button_count(id)
    }

    pub fn disc_pov_count(&self, id: u32) -> i32 {
        self.raw.disc_pov_count(id)
    }

    pub fn cont_pov_count(&self, id: u32) -> i32 {
        self.raw.cont_pov_count(id)
    }

    pub fn driver_match(&self) -> (bool, u32, u32) {
        self.raw.driver_match()
    }

    pub fn acquire_vjd(&self, id: u32) -> bool {
        self.raw.acquire_vjd(id)
    }

    pub fn relinquish_vjd(&self, id: u32) {
        self.raw.relinquish_vjd(id)
    }

    pub fn is_device_ffb(&self, id: u32) -> bool {
        self.raw.is_device_ffb(id)
    }

    pub fn register_ffb_callback(&self, callback: ffi::FfbGenCallback, user_data: *mut c_void) {
        self.raw.register_ffb_callback(callback, user_data)
    }

    pub(crate) fn update_vjd(&self, id: u32, report: &VirtualJoystickReport) -> bool {
        let mut raw_report = RawJoystickState::from(report);
        self.raw.update_vjd(id, &mut raw_report)
    }

    pub(crate) fn parse_packet_type(
        &self,
        packet: *const c_void,
    ) -> Result<ffi::RawPacketType, VJoyError> {
        self.raw.parse_packet_type(packet)
    }

    pub(crate) fn parse_effect_report(
        &self,
        packet: *const c_void,
    ) -> Result<ffi::RawEffReport, VJoyError> {
        self.raw.parse_effect_report(packet)
    }

    pub(crate) fn parse_envelope_report(
        &self,
        packet: *const c_void,
    ) -> Result<ffi::RawEffEnvelope, VJoyError> {
        self.raw.parse_envelope_report(packet)
    }

    pub(crate) fn parse_condition_report(
        &self,
        packet: *const c_void,
    ) -> Result<ffi::RawEffCond, VJoyError> {
        self.raw.parse_condition_report(packet)
    }

    pub(crate) fn parse_periodic_report(
        &self,
        packet: *const c_void,
    ) -> Result<ffi::RawEffPeriod, VJoyError> {
        self.raw.parse_periodic_report(packet)
    }

    pub(crate) fn parse_constant_report(
        &self,
        packet: *const c_void,
    ) -> Result<ffi::RawEffConstant, VJoyError> {
        self.raw.parse_constant_report(packet)
    }

    pub(crate) fn parse_ramp_report(
        &self,
        packet: *const c_void,
    ) -> Result<ffi::RawEffRamp, VJoyError> {
        self.raw.parse_ramp_report(packet)
    }

    pub(crate) fn parse_op_report(
        &self,
        packet: *const c_void,
    ) -> Result<ffi::RawEffOp, VJoyError> {
        self.raw.parse_op_report(packet)
    }

    pub(crate) fn parse_gain_report(&self, packet: *const c_void) -> Result<u8, VJoyError> {
        self.raw.parse_gain_report(packet)
    }

    pub(crate) fn parse_control_report(
        &self,
        packet: *const c_void,
    ) -> Result<ffi::RawDeviceControl, VJoyError> {
        self.raw.parse_control_report(packet)
    }
}

#[derive(Debug, Error)]
pub enum VJoyDeviceError {
    #[error(transparent)]
    Api(#[from] VJoyError),
    #[error("vJoy is not enabled")]
    NotEnabled,
    #[error("vJoy device {0} is already owned by this feeder")]
    AlreadyOwned(u32),
    #[error("vJoy device {0} is owned by another feeder")]
    Busy(u32),
    #[error("vJoy device {0} is missing or disabled")]
    Missing(u32),
    #[error("vJoy device {0} status is unknown")]
    UnknownStatus(u32),
    #[error("vJoy device {device_id} is missing required axis {axis}")]
    MissingAxis { device_id: u32, axis: &'static str },
    #[error("vJoy device {device_id} has {actual} buttons, expected {expected}")]
    ButtonCount {
        device_id: u32,
        expected: i32,
        actual: i32,
    },
    #[error("vJoy device {device_id} has {actual} discrete POVs, expected {expected}")]
    DiscPovCount {
        device_id: u32,
        expected: i32,
        actual: i32,
    },
    #[error("vJoy device {device_id} has continuous POVs but only discrete POV is supported")]
    ContinuousPovUnsupported { device_id: u32 },
    #[error("vJoy driver version mismatch between dll ({dll}) and driver ({driver})")]
    DriverMismatch { dll: u32, driver: u32 },
    #[error("failed to acquire vJoy device {0}")]
    AcquireFailed(u32),
    #[error("failed to update vJoy device {0}")]
    UpdateFailed(u32),
}

pub struct VJoyDevice {
    api: VJoyApi,
    id: u32,
    acquired: bool,
    report: VirtualJoystickReport,
}

impl VJoyDevice {
    pub fn initialize(id: u32) -> Result<Self, VJoyDeviceError> {
        let api = VJoyApi::load()?;

        if !api.vjoy_enabled() {
            return Err(VJoyDeviceError::NotEnabled);
        }

        match api.get_vjd_status(id) {
            VjdStatus::Owned => return Err(VJoyDeviceError::AlreadyOwned(id)),
            VjdStatus::Busy => return Err(VJoyDeviceError::Busy(id)),
            VjdStatus::Missing => return Err(VJoyDeviceError::Missing(id)),
            VjdStatus::Unknown(_) => return Err(VJoyDeviceError::UnknownStatus(id)),
            VjdStatus::Free => {}
        }

        for usage in HidUsage::REQUIRED_AXES {
            if !api.axis_exists(id, usage) {
                return Err(VJoyDeviceError::MissingAxis {
                    device_id: id,
                    axis: usage.label(),
                });
            }
        }

        let buttons = api.button_count(id);
        if buttons != 128 {
            return Err(VJoyDeviceError::ButtonCount {
                device_id: id,
                expected: 128,
                actual: buttons,
            });
        }

        let disc_povs = api.disc_pov_count(id);
        if disc_povs != 1 {
            return Err(VJoyDeviceError::DiscPovCount {
                device_id: id,
                expected: 1,
                actual: disc_povs,
            });
        }

        if api.cont_pov_count(id) > 0 {
            return Err(VJoyDeviceError::ContinuousPovUnsupported { device_id: id });
        }

        let (match_ok, dll, driver) = api.driver_match();
        if !match_ok {
            return Err(VJoyDeviceError::DriverMismatch { dll, driver });
        }

        if !api.acquire_vjd(id) {
            return Err(VJoyDeviceError::AcquireFailed(id));
        }

        let mut report = VirtualJoystickReport::default();
        Self::reset_report_state(id, &mut report);

        Ok(Self {
            api,
            id,
            acquired: true,
            report,
        })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn api(&self) -> &VJoyApi {
        &self.api
    }

    pub fn report_mut(&mut self) -> &mut VirtualJoystickReport {
        &mut self.report
    }

    pub fn is_ffb_capable(&self) -> bool {
        self.api.is_device_ffb(self.id)
    }

    pub fn update(&mut self) -> Result<(), VJoyDeviceError> {
        if self.api.update_vjd(self.id, &self.report) {
            return Ok(());
        }

        if !self.api.acquire_vjd(self.id) {
            return Err(VJoyDeviceError::AcquireFailed(self.id));
        }

        self.acquired = true;
        if !self.api.update_vjd(self.id, &self.report) {
            return Err(VJoyDeviceError::UpdateFailed(self.id));
        }

        Ok(())
    }

    pub fn reset_report(&mut self) {
        Self::reset_report_state(self.id, &mut self.report);
    }

    pub fn reset_report_state(id: u32, report: &mut VirtualJoystickReport) {
        report.device_id = id as u8;
        report.axis_x = 16384;
        report.axis_y = 32767;
        report.axis_z = 32767;
        report.axis_x_rot = 32767;
        report.axis_y_rot = 32767;
        report.axis_z_rot = 32767;
        report.slider = 32767;
        report.dial = 32767;
        report.buttons = 0;
        report.hats = 0b0000_1111;
    }
}

impl Drop for VJoyDevice {
    fn drop(&mut self) {
        if self.acquired {
            self.api.relinquish_vjd(self.id);
            self.acquired = false;
        }
    }
}
