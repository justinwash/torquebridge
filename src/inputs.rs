use crate::core::domain::InputFrame;
use thiserror::Error;

#[cfg(windows)]
mod winmm {
    use super::*;
    use std::mem::size_of;

    const MAX_PNAME_LEN: usize = 32;
    const MAX_OEM_VXD_NAME_LEN: usize = 260;

    const JOYERR_NOERROR: u32 = 0;
    const JOY_RETURNX: u32 = 0x0000_0001;
    const JOY_RETURNY: u32 = 0x0000_0002;
    const JOY_RETURNZ: u32 = 0x0000_0004;
    const JOY_RETURNR: u32 = 0x0000_0008;
    const JOY_RETURNU: u32 = 0x0000_0010;
    const JOY_RETURNV: u32 = 0x0000_0020;
    const JOY_RETURNPOV: u32 = 0x0000_0040;
    const JOY_RETURNBUTTONS: u32 = 0x0000_0080;
    const JOY_RETURNALL: u32 = JOY_RETURNX
        | JOY_RETURNY
        | JOY_RETURNZ
        | JOY_RETURNR
        | JOY_RETURNU
        | JOY_RETURNV
        | JOY_RETURNPOV
        | JOY_RETURNBUTTONS;
    const JOY_POVCENTERED: u32 = 0x0000_FFFF;

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct JoyCapsW {
        w_mid: u16,
        w_pid: u16,
        sz_pname: [u16; MAX_PNAME_LEN],
        w_xmin: u32,
        w_xmax: u32,
        w_ymin: u32,
        w_ymax: u32,
        w_zmin: u32,
        w_zmax: u32,
        w_num_buttons: u32,
        w_period_min: u32,
        w_period_max: u32,
        w_rmin: u32,
        w_rmax: u32,
        w_umin: u32,
        w_umax: u32,
        w_vmin: u32,
        w_vmax: u32,
        w_caps: u32,
        w_max_axes: u32,
        w_num_axes: u32,
        w_max_buttons: u32,
        sz_reg_key: [u16; MAX_PNAME_LEN],
        sz_oem_vxd: [u16; MAX_OEM_VXD_NAME_LEN],
    }

    impl Default for JoyCapsW {
        fn default() -> Self {
            Self {
                w_mid: 0,
                w_pid: 0,
                sz_pname: [0; MAX_PNAME_LEN],
                w_xmin: 0,
                w_xmax: 0,
                w_ymin: 0,
                w_ymax: 0,
                w_zmin: 0,
                w_zmax: 0,
                w_num_buttons: 0,
                w_period_min: 0,
                w_period_max: 0,
                w_rmin: 0,
                w_rmax: 0,
                w_umin: 0,
                w_umax: 0,
                w_vmin: 0,
                w_vmax: 0,
                w_caps: 0,
                w_max_axes: 0,
                w_num_axes: 0,
                w_max_buttons: 0,
                sz_reg_key: [0; MAX_PNAME_LEN],
                sz_oem_vxd: [0; MAX_OEM_VXD_NAME_LEN],
            }
        }
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy, Default)]
    struct JoyInfoEx {
        dw_size: u32,
        dw_flags: u32,
        dw_xpos: u32,
        dw_ypos: u32,
        dw_zpos: u32,
        dw_rpos: u32,
        dw_upos: u32,
        dw_vpos: u32,
        dw_buttons: u32,
        dw_button_number: u32,
        dw_pov: u32,
        dw_reserved1: u32,
        dw_reserved2: u32,
    }

    #[link(name = "winmm")]
    unsafe extern "system" {
        fn joyGetNumDevs() -> u32;
        fn joyGetDevCapsW(id: usize, caps: *mut JoyCapsW, caps_len: u32) -> u32;
        fn joyGetPosEx(id: u32, info: *mut JoyInfoEx) -> u32;
    }

    #[derive(Debug, Clone)]
    pub struct WinmmDeviceInfo {
        pub id: u32,
        pub name: String,
        pub axis_count: u32,
        pub button_count: u32,
    }

    #[derive(Debug, Error)]
    pub enum WinmmError {
        #[error("WinMM call {function} failed for device {device_id} with code {code}")]
        Api {
            function: &'static str,
            device_id: u32,
            code: u32,
        },
    }

    pub struct WinmmJoystick {
        id: u32,
        name: String,
        caps: JoyCapsW,
    }

    impl WinmmJoystick {
        pub fn list_devices() -> Result<Vec<WinmmDeviceInfo>, WinmmError> {
            let count = unsafe { joyGetNumDevs() };
            let mut devices = Vec::new();
            for id in 0..count {
                if let Some(caps) = read_caps(id)? {
                    devices.push(WinmmDeviceInfo {
                        id,
                        name: utf16_to_string(&caps.sz_pname),
                        axis_count: caps.w_num_axes,
                        button_count: caps.w_num_buttons,
                    });
                }
            }
            Ok(devices)
        }

        pub fn open(id: u32) -> Result<Self, WinmmError> {
            let caps = read_caps(id)?.ok_or(WinmmError::Api {
                function: "joyGetDevCapsW",
                device_id: id,
                code: 0,
            })?;

            Ok(Self {
                id,
                name: utf16_to_string(&caps.sz_pname),
                caps,
            })
        }

        pub fn id(&self) -> u32 {
            self.id
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn poll_input_frame(&mut self) -> Result<InputFrame, WinmmError> {
            let mut info = JoyInfoEx {
                dw_size: size_of::<JoyInfoEx>() as u32,
                dw_flags: JOY_RETURNALL,
                ..JoyInfoEx::default()
            };

            let code = unsafe { joyGetPosEx(self.id, &mut info) };
            if code != JOYERR_NOERROR {
                return Err(WinmmError::Api {
                    function: "joyGetPosEx",
                    device_id: self.id,
                    code,
                });
            }

            Ok(InputFrame {
                x: scale_axis(info.dw_xpos, self.caps.w_xmin, self.caps.w_xmax),
                y: scale_axis(info.dw_ypos, self.caps.w_ymin, self.caps.w_ymax),
                z: scale_axis(info.dw_zpos, self.caps.w_zmin, self.caps.w_zmax),
                rotation_x: scale_axis(info.dw_rpos, self.caps.w_rmin, self.caps.w_rmax),
                rotation_y: scale_axis(info.dw_upos, self.caps.w_umin, self.caps.w_umax),
                rotation_z: scale_axis(info.dw_vpos, self.caps.w_vmin, self.caps.w_vmax),
                sliders: [32_767, 32_767],
                buttons: decode_buttons(info.dw_buttons, self.caps.w_num_buttons as usize),
                point_of_view_controllers: vec![decode_pov(info.dw_pov)],
            })
        }
    }

    fn read_caps(id: u32) -> Result<Option<JoyCapsW>, WinmmError> {
        let mut caps = JoyCapsW::default();
        let code = unsafe { joyGetDevCapsW(id as usize, &mut caps, size_of::<JoyCapsW>() as u32) };
        if code == JOYERR_NOERROR {
            Ok(Some(caps))
        } else if code == 165 {
            Ok(None)
        } else {
            Err(WinmmError::Api {
                function: "joyGetDevCapsW",
                device_id: id,
                code,
            })
        }
    }

    fn utf16_to_string(raw: &[u16]) -> String {
        let end = raw.iter().position(|ch| *ch == 0).unwrap_or(raw.len());
        String::from_utf16_lossy(&raw[..end])
    }

    fn scale_axis(value: u32, min: u32, max: u32) -> i32 {
        if max <= min {
            return 32_767;
        }

        let clamped = value.clamp(min, max);
        let normalized = (clamped - min) as f64 / (max - min) as f64;
        (normalized * 65_535.0).round() as i32
    }

    fn decode_buttons(bits: u32, button_count: usize) -> Vec<bool> {
        let count = button_count.max(32);
        (0..count).map(|index| bits & (1 << index) != 0).collect()
    }

    fn decode_pov(pov: u32) -> i32 {
        if pov == JOY_POVCENTERED {
            -1
        } else {
            pov as i32
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn scale_axis_maps_range_into_directinput_style_range() {
            assert_eq!(scale_axis(0, 0, 1000), 0);
            assert_eq!(scale_axis(500, 0, 1000), 32_768);
            assert_eq!(scale_axis(1000, 0, 1000), 65_535);
        }

        #[test]
        fn decode_pov_maps_centered_to_minus_one() {
            assert_eq!(decode_pov(JOY_POVCENTERED), -1);
            assert_eq!(decode_pov(9000), 9000);
        }
    }
}

#[cfg(windows)]
pub use winmm::{WinmmDeviceInfo, WinmmError, WinmmJoystick};

#[cfg(not(windows))]
#[derive(Debug, Clone)]
pub struct WinmmDeviceInfo {
    pub id: u32,
    pub name: String,
    pub axis_count: u32,
    pub button_count: u32,
}

#[cfg(not(windows))]
#[derive(Debug, Error)]
pub enum WinmmError {
    #[error("WinMM input polling is only supported on Windows")]
    UnsupportedPlatform,
}

#[cfg(not(windows))]
pub struct WinmmJoystick;

#[cfg(not(windows))]
impl WinmmJoystick {
    pub fn list_devices() -> Result<Vec<WinmmDeviceInfo>, WinmmError> {
        Err(WinmmError::UnsupportedPlatform)
    }

    pub fn open(_id: u32) -> Result<Self, WinmmError> {
        Err(WinmmError::UnsupportedPlatform)
    }

    pub fn id(&self) -> u32 {
        0
    }

    pub fn name(&self) -> &str {
        ""
    }

    pub fn poll_input_frame(&mut self) -> Result<InputFrame, WinmmError> {
        Err(WinmmError::UnsupportedPlatform)
    }
}
