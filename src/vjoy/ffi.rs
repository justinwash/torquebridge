use libloading::{Library, Symbol};
use std::ffi::c_void;
use thiserror::Error;

const ERROR_SUCCESS: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawPacketType {
    EffRep,
    EnvRep,
    CondRep,
    PridRep,
    ConstRep,
    RampRep,
    CstmRep,
    SmplRep,
    EfOpRep,
    BlkFrRep,
    CtrlRep,
    GainRep,
    SetCRep,
    NewEfRep,
    BlkLdRep,
    PoolRep,
    Unknown(i32),
}

impl RawPacketType {
    pub(crate) fn from_raw(raw: i32) -> Self {
        match raw {
            1 => Self::EffRep,
            2 => Self::EnvRep,
            3 => Self::CondRep,
            4 => Self::PridRep,
            5 => Self::ConstRep,
            6 => Self::RampRep,
            7 => Self::CstmRep,
            8 => Self::SmplRep,
            10 => Self::EfOpRep,
            11 => Self::BlkFrRep,
            12 => Self::CtrlRep,
            13 => Self::GainRep,
            14 => Self::SetCRep,
            17 => Self::NewEfRep,
            18 => Self::BlkLdRep,
            19 => Self::PoolRep,
            _ => Self::Unknown(raw),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawEffectType {
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

impl RawEffectType {
    pub(crate) fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::None,
            1 => Self::Constant,
            2 => Self::Ramp,
            3 => Self::Square,
            4 => Self::Sine,
            5 => Self::Triangle,
            6 => Self::SawUp,
            7 => Self::SawDown,
            8 => Self::Spring,
            9 => Self::Damper,
            10 => Self::Inertia,
            11 => Self::Friction,
            12 => Self::Custom,
            _ => Self::Unknown(raw),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawDeviceControl {
    EnAct,
    DisAct,
    StopAll,
    DevRst,
    DevPause,
    DevCont,
    Unknown(i32),
}

impl RawDeviceControl {
    pub(crate) fn from_raw(raw: i32) -> Self {
        match raw {
            1 => Self::EnAct,
            2 => Self::DisAct,
            3 => Self::StopAll,
            4 => Self::DevRst,
            5 => Self::DevPause,
            6 => Self::DevCont,
            _ => Self::Unknown(raw),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawEffectOperation {
    Start,
    Solo,
    Stop,
    Unknown(i32),
}

impl RawEffectOperation {
    pub(crate) fn from_raw(raw: i32) -> Self {
        match raw {
            1 => Self::Start,
            2 => Self::Solo,
            3 => Self::Stop,
            _ => Self::Unknown(raw),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawJoystickState {
    pub b_device: u8,
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
    pub b_hats: u32,
    pub b_hats_ex1: u32,
    pub b_hats_ex2: u32,
    pub b_hats_ex3: u32,
    pub buttons_ex1: u32,
    pub buttons_ex2: u32,
    pub buttons_ex3: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawEffConstant {
    pub effect_block_index: u8,
    pub _pad0: [u8; 3],
    pub magnitude: i16,
    pub _pad1: [u8; 2],
}

impl RawEffConstant {
    pub(crate) fn semantic_eq(self, other: Self) -> bool {
        self.effect_block_index == other.effect_block_index && self.magnitude == other.magnitude
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawEffReport {
    pub effect_block_index: u8,
    pub _pad0: [u8; 3],
    pub effect_type_raw: i32,
    pub duration: u16,
    pub triger_rpt: u16,
    pub sample_prd: u16,
    pub gain: u8,
    pub triger_btn: u8,
    pub polar: u8,
    pub _pad1: [u8; 3],
    pub direction: u8,
    pub dir_y: u8,
    pub _pad2: [u8; 2],
}

impl RawEffReport {
    pub(crate) fn effect_type(self) -> RawEffectType {
        RawEffectType::from_raw(self.effect_type_raw)
    }

    pub(crate) fn semantic_eq(self, other: Self) -> bool {
        self.effect_block_index == other.effect_block_index
            && self.effect_type_raw == other.effect_type_raw
            && self.duration == other.duration
            && self.triger_rpt == other.triger_rpt
            && self.sample_prd == other.sample_prd
            && self.gain == other.gain
            && self.triger_btn == other.triger_btn
            && self.polar == other.polar
            && self.direction == other.direction
            && self.dir_y == other.dir_y
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawEffOp {
    pub effect_block_index: u8,
    pub _pad0: [u8; 3],
    pub effect_op_raw: i32,
    pub loop_count: u8,
    pub _pad1: [u8; 3],
}

impl RawEffOp {
    pub(crate) fn effect_op(self) -> RawEffectOperation {
        RawEffectOperation::from_raw(self.effect_op_raw)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawEffCond {
    pub effect_block_index: u8,
    pub _pad0: [u8; 3],
    pub is_y: u8,
    pub _pad1: [u8; 3],
    pub center_point_offset: i16,
    pub _pad2: [u8; 2],
    pub pos_coeff: i16,
    pub _pad3: [u8; 2],
    pub neg_coeff: i16,
    pub _pad4: [u8; 2],
    pub pos_satur: u32,
    pub neg_satur: u32,
    pub dead_band: i32,
}

impl RawEffCond {
    pub(crate) fn semantic_eq(self, other: Self) -> bool {
        self.effect_block_index == other.effect_block_index
            && self.is_y == other.is_y
            && self.center_point_offset == other.center_point_offset
            && self.pos_coeff == other.pos_coeff
            && self.neg_coeff == other.neg_coeff
            && self.pos_satur == other.pos_satur
            && self.neg_satur == other.neg_satur
            && self.dead_band == other.dead_band
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawEffEnvelope {
    pub effect_block_index: u8,
    pub _pad0: [u8; 3],
    pub attack_level: u16,
    pub _pad1: [u8; 2],
    pub fade_level: u16,
    pub _pad2: [u8; 2],
    pub attack_time: u32,
    pub fade_time: u32,
}

impl RawEffEnvelope {
    pub(crate) fn semantic_eq(self, other: Self) -> bool {
        self.effect_block_index == other.effect_block_index
            && self.attack_level == other.attack_level
            && self.fade_level == other.fade_level
            && self.attack_time == other.attack_time
            && self.fade_time == other.fade_time
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawEffPeriod {
    pub effect_block_index: u8,
    pub _pad0: [u8; 3],
    pub magnitude: u32,
    pub offset: i16,
    pub _pad1: [u8; 2],
    pub phase: u32,
    pub period: u32,
}

impl RawEffPeriod {
    pub(crate) fn semantic_eq(self, other: Self) -> bool {
        self.effect_block_index == other.effect_block_index
            && self.magnitude == other.magnitude
            && self.offset == other.offset
            && self.phase == other.phase
            && self.period == other.period
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawEffRamp {
    pub effect_block_index: u8,
    pub _pad0: [u8; 3],
    pub start: i16,
    pub _pad1: [u8; 2],
    pub end: i16,
    pub _pad2: [u8; 2],
}

impl RawEffRamp {
    pub(crate) fn semantic_eq(self, other: Self) -> bool {
        self.effect_block_index == other.effect_block_index
            && self.start == other.start
            && self.end == other.end
    }
}

pub type FfbGenCallback = unsafe extern "C" fn(*const c_void, *mut c_void);

type FnVJoyEnabled = unsafe extern "system" fn() -> bool;
type FnGetVjdStatus = unsafe extern "system" fn(u32) -> i32;
type FnGetVjdAxisExist = unsafe extern "system" fn(u32, u32) -> u32;
type FnGetVjdButtonNumber = unsafe extern "system" fn(u32) -> i32;
type FnGetVjdDiscPovNumber = unsafe extern "system" fn(u32) -> i32;
type FnGetVjdContPovNumber = unsafe extern "system" fn(u32) -> i32;
type FnDriverMatch = unsafe extern "system" fn(*mut u32, *mut u32) -> bool;
type FnAcquireVjd = unsafe extern "system" fn(u32) -> bool;
type FnRelinquishVjd = unsafe extern "system" fn(u32);
type FnUpdateVjd = unsafe extern "system" fn(u32, *mut RawJoystickState) -> bool;
type FnIsDeviceFfb = unsafe extern "system" fn(u32) -> bool;
type FnFfbRegisterGenCb = unsafe extern "C" fn(FfbGenCallback, *mut c_void);
type FnFfbHType = unsafe extern "system" fn(*const c_void, *mut i32) -> u32;
type FnFfbHEffReport = unsafe extern "system" fn(*const c_void, *mut RawEffReport) -> u32;
type FnFfbHEffEnvlp = unsafe extern "system" fn(*const c_void, *mut RawEffEnvelope) -> u32;
type FnFfbHEffCond = unsafe extern "system" fn(*const c_void, *mut RawEffCond) -> u32;
type FnFfbHEffPeriod = unsafe extern "system" fn(*const c_void, *mut RawEffPeriod) -> u32;
type FnFfbHEffConstant = unsafe extern "system" fn(*const c_void, *mut RawEffConstant) -> u32;
type FnFfbHEffRamp = unsafe extern "system" fn(*const c_void, *mut RawEffRamp) -> u32;
type FnFfbHEffOp = unsafe extern "system" fn(*const c_void, *mut RawEffOp) -> u32;
type FnFfbHDevGain = unsafe extern "system" fn(*const c_void, *mut u8) -> u32;
type FnFfbHDevCtrl = unsafe extern "system" fn(*const c_void, *mut i32) -> u32;

struct RawFns {
    vjoy_enabled: FnVJoyEnabled,
    get_vjd_status: FnGetVjdStatus,
    get_vjd_axis_exist: FnGetVjdAxisExist,
    get_vjd_button_number: FnGetVjdButtonNumber,
    get_vjd_disc_pov_number: FnGetVjdDiscPovNumber,
    get_vjd_cont_pov_number: FnGetVjdContPovNumber,
    driver_match: FnDriverMatch,
    acquire_vjd: FnAcquireVjd,
    relinquish_vjd: FnRelinquishVjd,
    update_vjd: FnUpdateVjd,
    is_device_ffb: FnIsDeviceFfb,
    ffb_register_gen_cb: FnFfbRegisterGenCb,
    ffb_h_type: FnFfbHType,
    ffb_h_eff_report: FnFfbHEffReport,
    ffb_h_eff_envlp: FnFfbHEffEnvlp,
    ffb_h_eff_cond: FnFfbHEffCond,
    ffb_h_eff_period: FnFfbHEffPeriod,
    ffb_h_eff_constant: FnFfbHEffConstant,
    ffb_h_eff_ramp: FnFfbHEffRamp,
    ffb_h_eff_op: FnFfbHEffOp,
    ffb_h_dev_gain: FnFfbHDevGain,
    ffb_h_dev_ctrl: FnFfbHDevCtrl,
}

#[derive(Debug, Error)]
pub enum VJoyError {
    #[error("failed to load vJoyInterface.dll")]
    LibraryLoad,
    #[error("failed to load symbol {symbol}: {source}")]
    SymbolLoad {
        symbol: &'static str,
        #[source]
        source: libloading::Error,
    },
    #[error("vJoy parser call {function} failed with code {code}")]
    Parser { function: &'static str, code: u32 },
}

fn load_symbol<T: Copy>(lib: &Library, symbol: &'static str) -> Result<T, VJoyError> {
    let mut symbol_with_nul = symbol.as_bytes().to_vec();
    symbol_with_nul.push(0);

    let loaded: Symbol<'_, T> = unsafe {
        lib.get(&symbol_with_nul)
            .map_err(|source| VJoyError::SymbolLoad { symbol, source })?
    };

    Ok(*loaded)
}

pub(crate) struct RawApi {
    _lib: Library,
    fns: RawFns,
}

impl RawApi {
    pub(crate) fn load() -> Result<Self, VJoyError> {
        let candidates = [
            "vJoyInterface.dll",
            "./vJoyInterface.dll",
            "C:\\Windows\\System32\\vJoyInterface.dll",
        ];

        let mut loaded = None;
        for candidate in candidates {
            if let Ok(lib) = unsafe { Library::new(candidate) } {
                loaded = Some(lib);
                break;
            }
        }

        let lib = loaded.ok_or(VJoyError::LibraryLoad)?;
        let fns = RawFns {
            vjoy_enabled: load_symbol(&lib, "vJoyEnabled")?,
            get_vjd_status: load_symbol(&lib, "GetVJDStatus")?,
            get_vjd_axis_exist: load_symbol(&lib, "GetVJDAxisExist")?,
            get_vjd_button_number: load_symbol(&lib, "GetVJDButtonNumber")?,
            get_vjd_disc_pov_number: load_symbol(&lib, "GetVJDDiscPovNumber")?,
            get_vjd_cont_pov_number: load_symbol(&lib, "GetVJDContPovNumber")?,
            driver_match: load_symbol(&lib, "DriverMatch")?,
            acquire_vjd: load_symbol(&lib, "AcquireVJD")?,
            relinquish_vjd: load_symbol(&lib, "RelinquishVJD")?,
            update_vjd: load_symbol(&lib, "UpdateVJD")?,
            is_device_ffb: load_symbol(&lib, "IsDeviceFfb")?,
            ffb_register_gen_cb: load_symbol(&lib, "FfbRegisterGenCB")?,
            ffb_h_type: load_symbol(&lib, "Ffb_h_Type")?,
            ffb_h_eff_report: load_symbol(&lib, "Ffb_h_Eff_Report")?,
            ffb_h_eff_envlp: load_symbol(&lib, "Ffb_h_Eff_Envlp")?,
            ffb_h_eff_cond: load_symbol(&lib, "Ffb_h_Eff_Cond")?,
            ffb_h_eff_period: load_symbol(&lib, "Ffb_h_Eff_Period")?,
            ffb_h_eff_constant: load_symbol(&lib, "Ffb_h_Eff_Constant")?,
            ffb_h_eff_ramp: load_symbol(&lib, "Ffb_h_Eff_Ramp")?,
            ffb_h_eff_op: load_symbol(&lib, "Ffb_h_EffOp")?,
            ffb_h_dev_gain: load_symbol(&lib, "Ffb_h_DevGain")?,
            ffb_h_dev_ctrl: load_symbol(&lib, "Ffb_h_DevCtrl")?,
        };

        Ok(Self { _lib: lib, fns })
    }

    pub(crate) fn vjoy_enabled(&self) -> bool {
        unsafe { (self.fns.vjoy_enabled)() }
    }

    pub(crate) fn get_vjd_status(&self, id: u32) -> i32 {
        unsafe { (self.fns.get_vjd_status)(id) }
    }

    pub(crate) fn axis_exists(&self, id: u32, usage: u32) -> bool {
        unsafe { (self.fns.get_vjd_axis_exist)(id, usage) == 1 }
    }

    pub(crate) fn button_count(&self, id: u32) -> i32 {
        unsafe { (self.fns.get_vjd_button_number)(id) }
    }

    pub(crate) fn disc_pov_count(&self, id: u32) -> i32 {
        unsafe { (self.fns.get_vjd_disc_pov_number)(id) }
    }

    pub(crate) fn cont_pov_count(&self, id: u32) -> i32 {
        unsafe { (self.fns.get_vjd_cont_pov_number)(id) }
    }

    pub(crate) fn driver_match(&self) -> (bool, u32, u32) {
        let mut dll_ver = 0u32;
        let mut drv_ver = 0u32;
        let ok = unsafe { (self.fns.driver_match)(&mut dll_ver, &mut drv_ver) };
        (ok, dll_ver, drv_ver)
    }

    pub(crate) fn acquire_vjd(&self, id: u32) -> bool {
        unsafe { (self.fns.acquire_vjd)(id) }
    }

    pub(crate) fn relinquish_vjd(&self, id: u32) {
        unsafe { (self.fns.relinquish_vjd)(id) }
    }

    pub(crate) fn update_vjd(&self, id: u32, report: &mut RawJoystickState) -> bool {
        unsafe { (self.fns.update_vjd)(id, report as *mut RawJoystickState) }
    }

    pub(crate) fn is_device_ffb(&self, id: u32) -> bool {
        unsafe { (self.fns.is_device_ffb)(id) }
    }

    pub(crate) fn register_ffb_callback(&self, callback: FfbGenCallback, user_data: *mut c_void) {
        unsafe { (self.fns.ffb_register_gen_cb)(callback, user_data) }
    }

    pub(crate) fn parse_packet_type(
        &self,
        packet: *const c_void,
    ) -> Result<RawPacketType, VJoyError> {
        let mut raw = 0i32;
        let code = unsafe { (self.fns.ffb_h_type)(packet, &mut raw) };
        parse_status("Ffb_h_Type", code)?;
        Ok(RawPacketType::from_raw(raw))
    }

    pub(crate) fn parse_effect_report(
        &self,
        packet: *const c_void,
    ) -> Result<RawEffReport, VJoyError> {
        let mut report = RawEffReport::default();
        let code = unsafe { (self.fns.ffb_h_eff_report)(packet, &mut report) };
        parse_status("Ffb_h_Eff_Report", code)?;
        Ok(report)
    }

    pub(crate) fn parse_envelope_report(
        &self,
        packet: *const c_void,
    ) -> Result<RawEffEnvelope, VJoyError> {
        let mut report = RawEffEnvelope::default();
        let code = unsafe { (self.fns.ffb_h_eff_envlp)(packet, &mut report) };
        parse_status("Ffb_h_Eff_Envlp", code)?;
        Ok(report)
    }

    pub(crate) fn parse_condition_report(
        &self,
        packet: *const c_void,
    ) -> Result<RawEffCond, VJoyError> {
        let mut report = RawEffCond::default();
        let code = unsafe { (self.fns.ffb_h_eff_cond)(packet, &mut report) };
        parse_status("Ffb_h_Eff_Cond", code)?;
        Ok(report)
    }

    pub(crate) fn parse_periodic_report(
        &self,
        packet: *const c_void,
    ) -> Result<RawEffPeriod, VJoyError> {
        let mut report = RawEffPeriod::default();
        let code = unsafe { (self.fns.ffb_h_eff_period)(packet, &mut report) };
        parse_status("Ffb_h_Eff_Period", code)?;
        Ok(report)
    }

    pub(crate) fn parse_constant_report(
        &self,
        packet: *const c_void,
    ) -> Result<RawEffConstant, VJoyError> {
        let mut report = RawEffConstant::default();
        let code = unsafe { (self.fns.ffb_h_eff_constant)(packet, &mut report) };
        parse_status("Ffb_h_Eff_Constant", code)?;
        Ok(report)
    }

    pub(crate) fn parse_ramp_report(&self, packet: *const c_void) -> Result<RawEffRamp, VJoyError> {
        let mut report = RawEffRamp::default();
        let code = unsafe { (self.fns.ffb_h_eff_ramp)(packet, &mut report) };
        parse_status("Ffb_h_Eff_Ramp", code)?;
        Ok(report)
    }

    pub(crate) fn parse_op_report(&self, packet: *const c_void) -> Result<RawEffOp, VJoyError> {
        let mut report = RawEffOp::default();
        let code = unsafe { (self.fns.ffb_h_eff_op)(packet, &mut report) };
        parse_status("Ffb_h_EffOp", code)?;
        Ok(report)
    }

    pub(crate) fn parse_gain_report(&self, packet: *const c_void) -> Result<u8, VJoyError> {
        let mut gain = 0u8;
        let code = unsafe { (self.fns.ffb_h_dev_gain)(packet, &mut gain) };
        parse_status("Ffb_h_DevGain", code)?;
        Ok(gain)
    }

    pub(crate) fn parse_control_report(
        &self,
        packet: *const c_void,
    ) -> Result<RawDeviceControl, VJoyError> {
        let mut raw = 0i32;
        let code = unsafe { (self.fns.ffb_h_dev_ctrl)(packet, &mut raw) };
        parse_status("Ffb_h_DevCtrl", code)?;
        Ok(RawDeviceControl::from_raw(raw))
    }
}

fn parse_status(function: &'static str, code: u32) -> Result<(), VJoyError> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(VJoyError::Parser { function, code })
    }
}
