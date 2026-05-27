#[cfg(target_os = "windows")]
mod imp {
    #![allow(non_snake_case)]

    use crate::config::ControllerConfig;
    use libloading::Library;
    use std::ffi::OsString;
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStringExt;
    use std::ptr::{null, null_mut};
    use thiserror::Error;
    use winapi::shared::guiddef::GUID;
    use winapi::shared::minwindef::{BOOL, DWORD, HMODULE, LPVOID, TRUE, ULONG};
    use winapi::shared::ntdef::{HANDLE, HRESULT, WCHAR};
    use winapi::shared::windef::HWND;
    use winapi::um::dinput::IID_IDirectInput8W;
    use winapi::um::libloaderapi::GetModuleHandleW;
    use winapi::um::winuser::{CreateWindowExW, DestroyWindow, WS_OVERLAPPED};

    const VJOY_PRODUCT_GUID: &str = "bead1234000000000000504944564944";
    const DIRECTINPUT_VERSION: DWORD = 0x0800;
    const DI8DEVCLASS_GAMECTRL: DWORD = 4;
    const DIEDFL_ATTACHEDONLY: DWORD = 0x0000_0001;
    const DIDC_FORCEFEEDBACK: DWORD = 0x0000_0100;
    const DISCL_EXCLUSIVE: DWORD = 0x0000_0001;
    const DISCL_BACKGROUND: DWORD = 0x0000_0008;
    const MAX_PATH_WCHARS: usize = 260;

    #[repr(C)]
    struct DIDEVCAPS {
        dwSize: DWORD,
        dwFlags: DWORD,
        dwDevType: DWORD,
        dwAxes: DWORD,
        dwButtons: DWORD,
        dwPOVs: DWORD,
    }

    #[repr(C)]
    struct DIDEVICEINSTANCEW {
        dwSize: DWORD,
        guidInstance: GUID,
        guidProduct: GUID,
        dwDevType: DWORD,
        tszInstanceName: [WCHAR; MAX_PATH_WCHARS],
        tszProductName: [WCHAR; MAX_PATH_WCHARS],
    }

    type LPDIENUMDEVICESCALLBACKW =
        Option<unsafe extern "system" fn(*const DIDEVICEINSTANCEW, LPVOID) -> BOOL>;

    #[repr(C)]
    struct IUnknownVtbl {
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut LPVOID) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> ULONG,
        release: unsafe extern "system" fn(*mut c_void) -> ULONG,
    }

    #[repr(C)]
    struct IDirectInput8WVtbl {
        parent: IUnknownVtbl,
        create_device: unsafe extern "system" fn(
            *mut IDirectInput8W,
            *const GUID,
            *mut *mut IDirectInputDevice8W,
            *mut c_void,
        ) -> HRESULT,
        enum_devices: unsafe extern "system" fn(
            *mut IDirectInput8W,
            DWORD,
            LPDIENUMDEVICESCALLBACKW,
            LPVOID,
            DWORD,
        ) -> HRESULT,
        get_device_status: unsafe extern "system" fn(*mut IDirectInput8W, *const GUID) -> HRESULT,
        run_control_panel: unsafe extern "system" fn(*mut IDirectInput8W, HWND, DWORD) -> HRESULT,
        initialize: unsafe extern "system" fn(*mut IDirectInput8W, HMODULE, DWORD) -> HRESULT,
    }

    #[repr(C)]
    struct IDirectInput8W {
        lpVtbl: *const IDirectInput8WVtbl,
    }

    #[repr(C)]
    struct IDirectInputDevice8WVtbl {
        parent: IUnknownVtbl,
        get_capabilities:
            unsafe extern "system" fn(*mut IDirectInputDevice8W, *mut DIDEVCAPS) -> HRESULT,
        enum_objects: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            *mut c_void,
            LPVOID,
            DWORD,
        ) -> HRESULT,
        get_property: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            *const GUID,
            *mut c_void,
        ) -> HRESULT,
        set_property: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            *const GUID,
            *const c_void,
        ) -> HRESULT,
        acquire: unsafe extern "system" fn(*mut IDirectInputDevice8W) -> HRESULT,
        unacquire: unsafe extern "system" fn(*mut IDirectInputDevice8W) -> HRESULT,
        get_device_state:
            unsafe extern "system" fn(*mut IDirectInputDevice8W, DWORD, LPVOID) -> HRESULT,
        get_device_data: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            DWORD,
            LPVOID,
            *mut DWORD,
            DWORD,
        ) -> HRESULT,
        set_data_format:
            unsafe extern "system" fn(*mut IDirectInputDevice8W, *const c_void) -> HRESULT,
        set_event_notification:
            unsafe extern "system" fn(*mut IDirectInputDevice8W, HANDLE) -> HRESULT,
        set_cooperative_level:
            unsafe extern "system" fn(*mut IDirectInputDevice8W, HWND, DWORD) -> HRESULT,
    }

    #[repr(C)]
    struct IDirectInputDevice8W {
        lpVtbl: *const IDirectInputDevice8WVtbl,
    }

    #[link(name = "dinput8")]
    unsafe extern "system" {
        fn DirectInput8Create(
            hinst: HMODULE,
            dwVersion: DWORD,
            riidltf: *const GUID,
            ppvOut: *mut LPVOID,
            punkOuter: *mut c_void,
        ) -> HRESULT;
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DirectInputDeviceInfo {
        pub instance_guid: String,
        pub product_guid: String,
        pub instance_name: String,
        pub product_name: String,
        pub configured: bool,
        pub configured_ffb: bool,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DirectInputCapabilities {
        pub force_feedback_capable: bool,
    }

    pub struct DirectInput {
        raw: *mut IDirectInput8W,
    }

    pub struct OpenedDirectInputDevice {
        raw: *mut IDirectInputDevice8W,
        info: DirectInputDeviceInfo,
        capabilities: DirectInputCapabilities,
        window: HiddenWindow,
    }

    struct HiddenWindow {
        hwnd: HWND,
    }

    #[derive(Debug, Error)]
    pub enum DirectInputError {
        #[error("failed to create DirectInput interface: HRESULT 0x{hr:08X}")]
        CreateDirectInput { hr: u32 },
        #[error("failed to enumerate DirectInput devices: HRESULT 0x{hr:08X}")]
        EnumDevices { hr: u32 },
        #[error("no FFBParameters entry found in config")]
        NoConfiguredFfbDevice,
        #[error("configured FFB device '{instance_name}' ({instance_guid}) is not attached")]
        ConfiguredFfbDeviceNotAttached {
            instance_name: String,
            instance_guid: String,
        },
        #[error(
            "failed to create DirectInput device '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        CreateDevice {
            instance_name: String,
            instance_guid: String,
            hr: u32,
        },
        #[error(
            "failed to set cooperative level for '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        SetCooperativeLevel {
            instance_name: String,
            instance_guid: String,
            hr: u32,
        },
        #[error("failed to load DirectInput joystick data format helper: {message}")]
        LoadDataFormat { message: String },
        #[error(
            "failed to query capabilities for '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        GetCapabilities {
            instance_name: String,
            instance_guid: String,
            hr: u32,
        },
        #[error("failed to acquire '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}")]
        Acquire {
            instance_name: String,
            instance_guid: String,
            hr: u32,
        },
        #[error("failed to create hidden DirectInput window: {message}")]
        CreateWindow { message: String },
    }

    #[derive(Clone)]
    struct RawDeviceInstance {
        instance_guid: GUID,
        product_guid: GUID,
        instance_name: String,
        product_name: String,
    }

    #[derive(Default)]
    struct EnumDevicesContext {
        devices: Vec<RawDeviceInstance>,
    }

    impl DirectInput {
        pub fn create() -> Result<Self, DirectInputError> {
            let module: HMODULE = unsafe { GetModuleHandleW(null()) };
            let mut raw: *mut IDirectInput8W = null_mut();
            let hr = unsafe {
                DirectInput8Create(
                    module,
                    DIRECTINPUT_VERSION,
                    &IID_IDirectInput8W,
                    &mut raw as *mut _ as *mut LPVOID,
                    null_mut(),
                )
            };
            if failed(hr) {
                return Err(DirectInputError::CreateDirectInput { hr: hr as u32 });
            }

            Ok(Self { raw })
        }

        pub fn list_devices(
            &self,
            controllers: &[ControllerConfig],
        ) -> Result<Vec<DirectInputDeviceInfo>, DirectInputError> {
            let devices = self.enumerate_game_controllers()?;
            let mut infos = Vec::new();

            for device in devices {
                if normalize_guid_token(&guid_to_string(&device.product_guid)) == VJOY_PRODUCT_GUID
                {
                    continue;
                }

                let instance_guid = guid_to_string(&device.instance_guid);
                let product_guid = guid_to_string(&device.product_guid);
                let configured = controllers.iter().any(|controller| {
                    normalize_guid_token(&controller.instance_guid)
                        == normalize_guid_token(&instance_guid)
                });
                let configured_ffb = controllers.iter().any(|controller| {
                    controller.ffb_parameters.is_some()
                        && normalize_guid_token(&controller.instance_guid)
                            == normalize_guid_token(&instance_guid)
                });

                infos.push(DirectInputDeviceInfo {
                    instance_guid,
                    product_guid,
                    instance_name: device.instance_name,
                    product_name: device.product_name,
                    configured,
                    configured_ffb,
                });
            }

            Ok(infos)
        }

        pub fn open_configured_ffb_device(
            &self,
            controllers: &[ControllerConfig],
        ) -> Result<OpenedDirectInputDevice, DirectInputError> {
            let configured = controllers
                .iter()
                .find(|controller| controller.ffb_parameters.is_some())
                .ok_or(DirectInputError::NoConfiguredFfbDevice)?;
            let devices = self.enumerate_game_controllers()?;

            let device = devices
                .into_iter()
                .find(|device| {
                    normalize_guid_token(&guid_to_string(&device.instance_guid))
                        == normalize_guid_token(&configured.instance_guid)
                })
                .ok_or_else(|| DirectInputError::ConfiguredFfbDeviceNotAttached {
                    instance_name: configured.instance_name.clone(),
                    instance_guid: configured.instance_guid.clone(),
                })?;

            let info = DirectInputDeviceInfo {
                instance_guid: guid_to_string(&device.instance_guid),
                product_guid: guid_to_string(&device.product_guid),
                instance_name: device.instance_name.clone(),
                product_name: device.product_name.clone(),
                configured: true,
                configured_ffb: true,
            };

            let window = HiddenWindow::create()?;
            let mut raw_device = null_mut();
            let hr = unsafe {
                ((*(*self.raw).lpVtbl).create_device)(
                    self.raw,
                    &device.instance_guid,
                    &mut raw_device,
                    null_mut(),
                )
            };
            if failed(hr) {
                return Err(DirectInputError::CreateDevice {
                    instance_name: info.instance_name.clone(),
                    instance_guid: info.instance_guid.clone(),
                    hr: hr as u32,
                });
            }

            let mut opened = OpenedDirectInputDevice {
                raw: raw_device,
                info,
                capabilities: DirectInputCapabilities {
                    force_feedback_capable: false,
                },
                window,
            };

            opened.set_cooperative_level(opened.window.handle())?;
            opened.set_data_format()?;
            let capabilities = opened.capabilities()?;
            opened.acquire()?;
            opened.capabilities = capabilities;

            Ok(opened)
        }

        fn enumerate_game_controllers(&self) -> Result<Vec<RawDeviceInstance>, DirectInputError> {
            let mut context = EnumDevicesContext::default();
            let hr = unsafe {
                ((*(*self.raw).lpVtbl).enum_devices)(
                    self.raw,
                    DI8DEVCLASS_GAMECTRL,
                    Some(enum_devices_callback),
                    &mut context as *mut _ as LPVOID,
                    DIEDFL_ATTACHEDONLY,
                )
            };

            if failed(hr) {
                return Err(DirectInputError::EnumDevices { hr: hr as u32 });
            }

            Ok(context.devices)
        }
    }

    impl Drop for DirectInput {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                unsafe {
                    ((*(*self.raw).lpVtbl).parent.release)(self.raw.cast());
                }
            }
        }
    }

    impl OpenedDirectInputDevice {
        pub fn info(&self) -> &DirectInputDeviceInfo {
            &self.info
        }

        pub fn capabilities(&self) -> Result<DirectInputCapabilities, DirectInputError> {
            let mut caps: DIDEVCAPS = unsafe { zeroed() };
            caps.dwSize = size_of::<DIDEVCAPS>() as DWORD;
            let hr = unsafe { ((*(*self.raw).lpVtbl).get_capabilities)(self.raw, &mut caps) };
            if failed(hr) {
                return Err(DirectInputError::GetCapabilities {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                    hr: hr as u32,
                });
            }

            Ok(DirectInputCapabilities {
                force_feedback_capable: (caps.dwFlags & DIDC_FORCEFEEDBACK) != 0,
            })
        }

        pub fn cached_capabilities(&self) -> DirectInputCapabilities {
            self.capabilities
        }

        fn set_cooperative_level(&self, hwnd: HWND) -> Result<(), DirectInputError> {
            let hr = unsafe {
                ((*(*self.raw).lpVtbl).set_cooperative_level)(
                    self.raw,
                    hwnd,
                    DISCL_EXCLUSIVE | DISCL_BACKGROUND,
                )
            };
            if failed(hr) {
                return Err(DirectInputError::SetCooperativeLevel {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                    hr: hr as u32,
                });
            }

            Ok(())
        }

        fn set_data_format(&self) -> Result<(), DirectInputError> {
            type GetJoystickDataFormat = unsafe extern "system" fn() -> *const c_void;

            let library = unsafe { Library::new("dinput8.dll") }.map_err(|source| {
                DirectInputError::LoadDataFormat {
                    message: source.to_string(),
                }
            })?;
            let get_data_format =
                unsafe { library.get::<GetJoystickDataFormat>(b"GetdfDIJoystick\0") }.map_err(
                    |source| DirectInputError::LoadDataFormat {
                        message: source.to_string(),
                    },
                )?;
            let format = unsafe { get_data_format() };
            if format.is_null() {
                return Err(DirectInputError::LoadDataFormat {
                    message: "GetdfDIJoystick returned a null data-format pointer".to_string(),
                });
            }

            let hr = unsafe { ((*(*self.raw).lpVtbl).set_data_format)(self.raw, format) };
            if failed(hr) {
                return Err(DirectInputError::LoadDataFormat {
                    message: format!(
                        "SetDataFormat failed for '{}' ({}) with HRESULT 0x{hr:08X}",
                        self.info.instance_name, self.info.instance_guid,
                    ),
                });
            }

            Ok(())
        }

        fn acquire(&self) -> Result<(), DirectInputError> {
            let hr = unsafe { ((*(*self.raw).lpVtbl).acquire)(self.raw) };
            if failed(hr) {
                return Err(DirectInputError::Acquire {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                    hr: hr as u32,
                });
            }

            Ok(())
        }
    }

    impl HiddenWindow {
        fn create() -> Result<Self, DirectInputError> {
            let module = unsafe { GetModuleHandleW(null()) };
            let class_name = wide_null("STATIC");
            let title = wide_null("forzabeast-directinput");
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    title.as_ptr(),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    module,
                    null_mut(),
                )
            };
            if hwnd.is_null() {
                return Err(DirectInputError::CreateWindow {
                    message: std::io::Error::last_os_error().to_string(),
                });
            }

            Ok(Self { hwnd })
        }

        fn handle(&self) -> HWND {
            self.hwnd
        }
    }

    impl Drop for HiddenWindow {
        fn drop(&mut self) {
            if !self.hwnd.is_null() {
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
        }
    }

    impl Drop for OpenedDirectInputDevice {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                unsafe {
                    let _ = ((*(*self.raw).lpVtbl).unacquire)(self.raw);
                    ((*(*self.raw).lpVtbl).parent.release)(self.raw.cast());
                }
            }
        }
    }

    unsafe extern "system" fn enum_devices_callback(
        device: *const DIDEVICEINSTANCEW,
        context: LPVOID,
    ) -> BOOL {
        if device.is_null() || context.is_null() {
            return 0;
        }

        let device = unsafe { &*device };
        let context = unsafe { &mut *(context as *mut EnumDevicesContext) };
        context.devices.push(RawDeviceInstance {
            instance_guid: device.guidInstance,
            product_guid: device.guidProduct,
            instance_name: wide_to_string(&device.tszInstanceName),
            product_name: wide_to_string(&device.tszProductName),
        });
        TRUE
    }

    fn wide_to_string(value: &[u16]) -> String {
        let end = value.iter().position(|&ch| ch == 0).unwrap_or(value.len());
        OsString::from_wide(&value[..end])
            .to_string_lossy()
            .into_owned()
    }

    fn guid_to_string(guid: &GUID) -> String {
        format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            guid.Data1,
            guid.Data2,
            guid.Data3,
            guid.Data4[0],
            guid.Data4[1],
            guid.Data4[2],
            guid.Data4[3],
            guid.Data4[4],
            guid.Data4[5],
            guid.Data4[6],
            guid.Data4[7],
        )
    }

    fn normalize_guid_token(value: &str) -> String {
        value
            .chars()
            .filter(|ch| ch.is_ascii_hexdigit())
            .map(|ch| ch.to_ascii_lowercase())
            .collect()
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn failed(hr: HRESULT) -> bool {
        hr < 0
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn normalize_guid_token_strips_punctuation() {
            assert_eq!(
                normalize_guid_token("{1F577B90-4000-11F0-8001-444553540000}"),
                "1f577b90400011f08001444553540000"
            );
        }

        #[test]
        fn guid_to_string_uses_windows_layout() {
            let guid = GUID {
                Data1: 0x1f577b90,
                Data2: 0x4000,
                Data3: 0x11f0,
                Data4: [0x80, 0x01, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
            };

            assert_eq!(
                guid_to_string(&guid),
                "1f577b90-4000-11f0-8001-444553540000"
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use crate::config::ControllerConfig;
    use thiserror::Error;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DirectInputDeviceInfo {
        pub instance_guid: String,
        pub product_guid: String,
        pub instance_name: String,
        pub product_name: String,
        pub configured: bool,
        pub configured_ffb: bool,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DirectInputCapabilities {
        pub force_feedback_capable: bool,
    }

    pub struct DirectInput;

    pub struct OpenedDirectInputDevice;

    #[derive(Debug, Error)]
    pub enum DirectInputError {
        #[error("DirectInput backend is only available on Windows")]
        Unsupported,
    }

    impl DirectInput {
        pub fn create() -> Result<Self, DirectInputError> {
            Err(DirectInputError::Unsupported)
        }

        pub fn list_devices(
            &self,
            _controllers: &[ControllerConfig],
        ) -> Result<Vec<DirectInputDeviceInfo>, DirectInputError> {
            Err(DirectInputError::Unsupported)
        }

        pub fn open_configured_ffb_device(
            &self,
            _controllers: &[ControllerConfig],
        ) -> Result<OpenedDirectInputDevice, DirectInputError> {
            Err(DirectInputError::Unsupported)
        }
    }

    impl OpenedDirectInputDevice {
        pub fn info(&self) -> &DirectInputDeviceInfo {
            unreachable!()
        }

        pub fn cached_capabilities(&self) -> DirectInputCapabilities {
            unreachable!()
        }
    }
}

pub use imp::*;
