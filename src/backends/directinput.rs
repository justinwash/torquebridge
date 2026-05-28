#[cfg(target_os = "windows")]
mod imp {
    #![allow(non_snake_case)]

    use crate::config::ControllerConfig;
    use crate::core::domain::{ConditionCommand, DeviceControlCommand, EffectKind, WheelCommand};
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
    const DIDFT_AXIS: DWORD = 0x0000_0003;
    const DIDFT_FFACTUATOR: DWORD = 0x0100_0000;
    const DIDC_FORCEFEEDBACK: DWORD = 0x0000_0100;
    const DISCL_EXCLUSIVE: DWORD = 0x0000_0001;
    const DISCL_BACKGROUND: DWORD = 0x0000_0008;
    const DIEFF_OBJECTIDS: DWORD = 0x0000_0001;
    const DIEFF_CARTESIAN: DWORD = 0x0000_0010;
    const DIEP_TYPESPECIFICPARAMS: DWORD = 0x0000_0100;
    const DIES_PLAYING: DWORD = 0x0000_0001;
    const DISFFC_RESET: DWORD = 0x0000_0001;
    const DISFFC_STOPALL: DWORD = 0x0000_0002;
    const DISFFC_PAUSE: DWORD = 0x0000_0004;
    const DISFFC_CONTINUE: DWORD = 0x0000_0008;
    const DIEB_NOTRIGGER: DWORD = u32::MAX;
    const INFINITE_DURATION: DWORD = u32::MAX;
    const MAX_PATH_WCHARS: usize = 260;

    #[repr(C)]
    struct DICONSTANTFORCE {
        lMagnitude: i32,
    }

    #[repr(C)]
    struct DIPERIODIC {
        dwMagnitude: DWORD,
        lOffset: i32,
        dwPhase: DWORD,
        dwPeriod: DWORD,
    }

    #[repr(C)]
    struct DICONDITION {
        lOffset: i32,
        lPositiveCoefficient: i32,
        lNegativeCoefficient: i32,
        dwPositiveSaturation: DWORD,
        dwNegativeSaturation: DWORD,
        lDeadBand: i32,
    }

    #[repr(C)]
    struct DIEFFECT {
        dwSize: DWORD,
        dwFlags: DWORD,
        dwDuration: DWORD,
        dwSamplePeriod: DWORD,
        dwGain: DWORD,
        dwTriggerButton: DWORD,
        dwTriggerRepeatInterval: DWORD,
        cAxes: DWORD,
        rgdwAxes: *mut DWORD,
        rglDirection: *mut i32,
        lpEnvelope: *mut c_void,
        cbTypeSpecificParams: DWORD,
        lpvTypeSpecificParams: *mut c_void,
        dwStartDelay: DWORD,
    }

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

    #[repr(C)]
    struct DIDEVICEOBJECTINSTANCEW {
        dwSize: DWORD,
        guidType: GUID,
        dwOfs: DWORD,
        dwType: DWORD,
        dwFlags: DWORD,
        tszName: [WCHAR; MAX_PATH_WCHARS],
        dwFFMaxForce: DWORD,
        dwFFForceResolution: DWORD,
        wCollectionNumber: u16,
        wDesignatorIndex: u16,
        wUsagePage: u16,
        wUsage: u16,
        dwDimension: DWORD,
        wExponent: u16,
        wReportId: u16,
    }

    type LPDIENUMDEVICESCALLBACKW =
        Option<unsafe extern "system" fn(*const DIDEVICEINSTANCEW, LPVOID) -> BOOL>;
    type LPDIENUMDEVICEOBJECTSCALLBACKW =
        Option<unsafe extern "system" fn(*const DIDEVICEOBJECTINSTANCEW, LPVOID) -> BOOL>;

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
            LPDIENUMDEVICEOBJECTSCALLBACKW,
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
        get_object_info: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            *mut c_void,
            DWORD,
            DWORD,
        ) -> HRESULT,
        get_device_info:
            unsafe extern "system" fn(*mut IDirectInputDevice8W, *mut c_void) -> HRESULT,
        run_control_panel:
            unsafe extern "system" fn(*mut IDirectInputDevice8W, HWND, DWORD) -> HRESULT,
        initialize: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            HMODULE,
            DWORD,
            *const GUID,
        ) -> HRESULT,
        create_effect: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            *const GUID,
            *const DIEFFECT,
            *mut *mut IDirectInputEffect,
            *mut c_void,
        ) -> HRESULT,
        enum_effects: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            *mut c_void,
            LPVOID,
            DWORD,
        ) -> HRESULT,
        get_effect_info: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            *mut c_void,
            *const GUID,
        ) -> HRESULT,
        get_force_feedback_state:
            unsafe extern "system" fn(*mut IDirectInputDevice8W, *mut DWORD) -> HRESULT,
        send_force_feedback_command:
            unsafe extern "system" fn(*mut IDirectInputDevice8W, DWORD) -> HRESULT,
        enum_created_effect_objects: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            *mut c_void,
            LPVOID,
            DWORD,
        ) -> HRESULT,
        escape: unsafe extern "system" fn(*mut IDirectInputDevice8W, *mut c_void) -> HRESULT,
        poll: unsafe extern "system" fn(*mut IDirectInputDevice8W) -> HRESULT,
        send_device_data: unsafe extern "system" fn(
            *mut IDirectInputDevice8W,
            DWORD,
            *const c_void,
            *mut DWORD,
            DWORD,
        ) -> HRESULT,
    }

    #[repr(C)]
    struct IDirectInputDevice8W {
        lpVtbl: *const IDirectInputDevice8WVtbl,
    }

    #[repr(C)]
    struct IDirectInputEffectVtbl {
        parent: IUnknownVtbl,
        initialize: unsafe extern "system" fn(
            *mut IDirectInputEffect,
            HMODULE,
            DWORD,
            *const GUID,
        ) -> HRESULT,
        get_effect_guid: unsafe extern "system" fn(*mut IDirectInputEffect, *mut GUID) -> HRESULT,
        get_parameters:
            unsafe extern "system" fn(*mut IDirectInputEffect, *mut DIEFFECT, DWORD) -> HRESULT,
        set_parameters:
            unsafe extern "system" fn(*mut IDirectInputEffect, *const DIEFFECT, DWORD) -> HRESULT,
        start: unsafe extern "system" fn(*mut IDirectInputEffect, DWORD, DWORD) -> HRESULT,
        stop: unsafe extern "system" fn(*mut IDirectInputEffect) -> HRESULT,
        get_effect_status:
            unsafe extern "system" fn(*mut IDirectInputEffect, *mut DWORD) -> HRESULT,
        download: unsafe extern "system" fn(*mut IDirectInputEffect) -> HRESULT,
        unload: unsafe extern "system" fn(*mut IDirectInputEffect) -> HRESULT,
        escape: unsafe extern "system" fn(*mut IDirectInputEffect, *mut c_void) -> HRESULT,
    }

    #[repr(C)]
    struct IDirectInputEffect {
        lpVtbl: *const IDirectInputEffectVtbl,
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
        actuator_object_ids: Vec<DWORD>,
        effects: LoadedEffects,
        window: HiddenWindow,
    }

    struct LoadedEffects {
        constant: *mut IDirectInputEffect,
        sine: *mut IDirectInputEffect,
        spring: *mut IDirectInputEffect,
        damper: *mut IDirectInputEffect,
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
        #[error(
            "failed to create DirectInput effect '{effect}' for '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        CreateEffect {
            instance_name: String,
            instance_guid: String,
            effect: &'static str,
            hr: u32,
        },
        #[error(
            "failed to update DirectInput effect '{effect}' for '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        SetEffect {
            instance_name: String,
            instance_guid: String,
            effect: &'static str,
            hr: u32,
        },
        #[error(
            "failed to query DirectInput effect status '{effect}' for '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        EffectStatus {
            instance_name: String,
            instance_guid: String,
            effect: &'static str,
            hr: u32,
        },
        #[error(
            "failed to start DirectInput effect '{effect}' for '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        StartEffect {
            instance_name: String,
            instance_guid: String,
            effect: &'static str,
            hr: u32,
        },
        #[error(
            "failed to stop DirectInput effect '{effect}' for '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        StopEffect {
            instance_name: String,
            instance_guid: String,
            effect: &'static str,
            hr: u32,
        },
        #[error(
            "failed to send DirectInput device control command {command} for '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        SendDeviceControl {
            instance_name: String,
            instance_guid: String,
            command: DWORD,
            hr: u32,
        },
        #[error("failed to create hidden DirectInput window: {message}")]
        CreateWindow { message: String },
        #[error(
            "failed to enumerate DirectInput actuator objects for '{instance_name}' ({instance_guid}): HRESULT 0x{hr:08X}"
        )]
        EnumActuators {
            instance_name: String,
            instance_guid: String,
            hr: u32,
        },
        #[error("no DirectInput actuator objects found for '{instance_name}' ({instance_guid})")]
        NoActuators {
            instance_name: String,
            instance_guid: String,
        },
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

    #[derive(Default)]
    struct EnumActuatorsContext {
        object_ids: Vec<DWORD>,
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
                actuator_object_ids: Vec::new(),
                effects: LoadedEffects::default(),
                window,
            };

            opened.set_cooperative_level(opened.window.handle())?;
            opened.set_data_format()?;
            let capabilities = opened.capabilities()?;
            opened.acquire()?;
            opened.actuator_object_ids = opened.enumerate_actuator_object_ids()?;
            opened.effects = opened.load_effects()?;
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

        pub fn actuator_count(&self) -> usize {
            self.actuator_object_ids.len()
        }

        pub fn actuator_object_ids(&self) -> Vec<u32> {
            self.actuator_object_ids.clone()
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

        pub fn apply_commands(
            &mut self,
            commands: &[WheelCommand],
        ) -> Result<(), DirectInputError> {
            for command in commands {
                match command {
                    WheelCommand::Ignore => {}
                    WheelCommand::DeviceControl(control) => self.apply_device_control(*control)?,
                    WheelCommand::StopEffect(kind) => self.stop_effect(*kind)?,
                    WheelCommand::Constant(command) => {
                        self.update_constant(
                            command.metadata.direction as i32,
                            command.magnitude,
                            command.metadata.gain,
                            command.metadata.duration_ms,
                            command.metadata.sample_period as DWORD,
                            command.metadata.trigger_button,
                            command.metadata.trigger_repeat_interval as DWORD,
                        )?;
                    }
                    WheelCommand::Periodic(command) => {
                        self.update_periodic(
                            command.metadata.direction as i32,
                            command.magnitude,
                            command.offset as i32,
                            command.period,
                            command.phase,
                            command.metadata.gain,
                            command.metadata.duration_ms,
                            command.metadata.sample_period as DWORD,
                            command.metadata.trigger_button,
                            command.metadata.trigger_repeat_interval as DWORD,
                        )?;
                    }
                    WheelCommand::Condition(command) => {
                        self.update_condition(command)?;
                    }
                }
            }

            Ok(())
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

        fn load_effects(&self) -> Result<LoadedEffects, DirectInputError> {
            Ok(LoadedEffects {
                constant: self.create_effect(
                    &winapi::um::dinput::GUID_ConstantForce,
                    "constant",
                    &mut DICONSTANTFORCE { lMagnitude: 0 },
                    size_of::<DICONSTANTFORCE>() as DWORD,
                    0,
                    10_000,
                    -1,
                    0,
                    -1,
                    0,
                    0,
                )?,
                sine: self.create_effect(
                    &winapi::um::dinput::GUID_Sine,
                    "sine",
                    &mut DIPERIODIC {
                        dwMagnitude: 0,
                        lOffset: 0,
                        dwPhase: 0,
                        dwPeriod: 1000,
                    },
                    size_of::<DIPERIODIC>() as DWORD,
                    0,
                    10_000,
                    -1,
                    0,
                    -1,
                    0,
                    0,
                )?,
                spring: self.create_effect(
                    &winapi::um::dinput::GUID_Spring,
                    "spring",
                    &mut [DICONDITION {
                        lOffset: 0,
                        lPositiveCoefficient: 0,
                        lNegativeCoefficient: 0,
                        dwPositiveSaturation: 0,
                        dwNegativeSaturation: 0,
                        lDeadBand: 0,
                    }; 1],
                    size_of::<DICONDITION>() as DWORD,
                    0,
                    10_000,
                    -1,
                    0,
                    -1,
                    0,
                    0,
                )?,
                damper: self.create_effect(
                    &winapi::um::dinput::GUID_Damper,
                    "damper",
                    &mut [DICONDITION {
                        lOffset: 0,
                        lPositiveCoefficient: 0,
                        lNegativeCoefficient: 0,
                        dwPositiveSaturation: 0,
                        dwNegativeSaturation: 0,
                        lDeadBand: 0,
                    }; 1],
                    size_of::<DICONDITION>() as DWORD,
                    0,
                    10_000,
                    -1,
                    0,
                    -1,
                    0,
                    0,
                )?,
            })
        }

        #[allow(clippy::too_many_arguments)]
        fn create_effect(
            &self,
            effect_guid: &GUID,
            effect_name: &'static str,
            type_specific: &mut impl Sized,
            type_size: DWORD,
            direction: i32,
            gain: i32,
            duration_ms: i32,
            sample_period: DWORD,
            trigger_button: i32,
            trigger_repeat_interval: DWORD,
            actuator_index: usize,
        ) -> Result<*mut IDirectInputEffect, DirectInputError> {
            let mut axes: [DWORD; 1] = [self.actuator_object_id(actuator_index)?];
            let mut directions: [i32; 1] = [direction];
            let effect = make_effect(
                type_specific as *mut _ as *mut c_void,
                type_size,
                &mut axes,
                &mut directions,
                gain,
                duration_ms,
                sample_period,
                trigger_button,
                trigger_repeat_interval,
                1,
            );

            let mut raw_effect = null_mut();
            let hr = unsafe {
                ((*(*self.raw).lpVtbl).create_effect)(
                    self.raw,
                    effect_guid,
                    &effect,
                    &mut raw_effect,
                    null_mut(),
                )
            };
            if failed(hr) {
                return Err(DirectInputError::CreateEffect {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                    effect: effect_name,
                    hr: hr as u32,
                });
            }

            Ok(raw_effect)
        }

        fn apply_device_control(
            &self,
            control: DeviceControlCommand,
        ) -> Result<(), DirectInputError> {
            let command = match control {
                DeviceControlCommand::Reset => DISFFC_RESET,
                DeviceControlCommand::StopAll => DISFFC_STOPALL,
                DeviceControlCommand::Pause => DISFFC_PAUSE,
                DeviceControlCommand::Continue => DISFFC_CONTINUE,
            };
            let hr =
                unsafe { ((*(*self.raw).lpVtbl).send_force_feedback_command)(self.raw, command) };
            if failed(hr) {
                return Err(DirectInputError::SendDeviceControl {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                    command,
                    hr: hr as u32,
                });
            }

            Ok(())
        }

        fn stop_effect(&self, kind: EffectKind) -> Result<(), DirectInputError> {
            let Some((effect, name)) = self.effect_for_kind(kind) else {
                return Ok(());
            };
            let hr = unsafe { ((*(*effect).lpVtbl).stop)(effect) };
            if failed(hr) {
                return Err(DirectInputError::StopEffect {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                    effect: name,
                    hr: hr as u32,
                });
            }

            Ok(())
        }

        fn update_constant(
            &self,
            direction: i32,
            magnitude: i32,
            gain: i32,
            duration_ms: i32,
            sample_period: DWORD,
            trigger_button: i32,
            trigger_repeat_interval: DWORD,
        ) -> Result<(), DirectInputError> {
            let mut force = DICONSTANTFORCE {
                lMagnitude: clamp_signed_10k(magnitude),
            };
            self.set_effect_and_start_if_needed(
                self.effects.constant,
                "constant",
                direction,
                &mut force as *mut _ as *mut c_void,
                size_of::<DICONSTANTFORCE>() as DWORD,
                gain,
                duration_ms,
                sample_period,
                trigger_button,
                trigger_repeat_interval,
                0,
            )
        }

        fn update_periodic(
            &self,
            direction: i32,
            magnitude: i32,
            offset: i32,
            period_ms: i32,
            phase: i32,
            gain: i32,
            duration_ms: i32,
            sample_period: DWORD,
            trigger_button: i32,
            trigger_repeat_interval: DWORD,
        ) -> Result<(), DirectInputError> {
            let mut periodic = DIPERIODIC {
                dwMagnitude: clamp_unsigned_10k(magnitude),
                lOffset: clamp_signed_10k(offset),
                dwPhase: phase.max(0) as DWORD,
                dwPeriod: ms_to_us(period_ms),
            };
            self.set_effect_and_start_if_needed(
                self.effects.sine,
                "sine",
                direction,
                &mut periodic as *mut _ as *mut c_void,
                size_of::<DIPERIODIC>() as DWORD,
                gain,
                duration_ms,
                sample_period,
                trigger_button,
                trigger_repeat_interval,
                0,
            )
        }

        fn update_condition(&self, condition: &ConditionCommand) -> Result<(), DirectInputError> {
            let Some((effect, name)) = self.effect_for_kind(condition.kind) else {
                return Ok(());
            };
            let mut conditions = [DICONDITION {
                lOffset: i32::from(condition.center_point_offset),
                lPositiveCoefficient: condition.positive_coefficient,
                lNegativeCoefficient: condition.negative_coefficient,
                dwPositiveSaturation: clamp_unsigned_10k(condition.positive_saturation),
                dwNegativeSaturation: clamp_unsigned_10k(condition.negative_saturation),
                lDeadBand: condition.dead_band,
            }];

            self.set_effect_and_start_if_needed(
                effect,
                name,
                condition.metadata.direction as i32,
                conditions.as_mut_ptr() as *mut c_void,
                size_of::<DICONDITION>() as DWORD,
                condition.metadata.gain,
                condition.metadata.duration_ms,
                condition.metadata.sample_period as DWORD,
                condition.metadata.trigger_button,
                condition.metadata.trigger_repeat_interval as DWORD,
                0,
            )
        }

        #[allow(clippy::too_many_arguments)]
        fn set_effect_and_start_if_needed(
            &self,
            effect: *mut IDirectInputEffect,
            effect_name: &'static str,
            direction: i32,
            type_specific: *mut c_void,
            type_size: DWORD,
            gain: i32,
            duration_ms: i32,
            sample_period: DWORD,
            trigger_button: i32,
            trigger_repeat_interval: DWORD,
            actuator_index: usize,
        ) -> Result<(), DirectInputError> {
            let mut axes: [DWORD; 1] = [self.actuator_object_id(actuator_index)?];
            let mut directions: [i32; 1] = [direction];
            let effect_params = make_effect(
                type_specific,
                type_size,
                &mut axes,
                &mut directions,
                gain,
                duration_ms,
                sample_period,
                trigger_button,
                trigger_repeat_interval,
                1,
            );

            let hr = unsafe {
                ((*(*effect).lpVtbl).set_parameters)(
                    effect,
                    &effect_params,
                    DIEP_TYPESPECIFICPARAMS,
                )
            };
            if failed(hr) {
                return Err(DirectInputError::SetEffect {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                    effect: effect_name,
                    hr: hr as u32,
                });
            }

            let mut status: DWORD = 0;
            let hr = unsafe { ((*(*effect).lpVtbl).get_effect_status)(effect, &mut status) };
            if failed(hr) {
                return Err(DirectInputError::EffectStatus {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                    effect: effect_name,
                    hr: hr as u32,
                });
            }

            if (status & DIES_PLAYING) == 0 {
                let hr = unsafe { ((*(*effect).lpVtbl).start)(effect, 1, 0) };
                if failed(hr) {
                    return Err(DirectInputError::StartEffect {
                        instance_name: self.info.instance_name.clone(),
                        instance_guid: self.info.instance_guid.clone(),
                        effect: effect_name,
                        hr: hr as u32,
                    });
                }
            }

            Ok(())
        }

        fn enumerate_actuator_object_ids(&self) -> Result<Vec<DWORD>, DirectInputError> {
            let mut context = EnumActuatorsContext::default();
            let hr = unsafe {
                ((*(*self.raw).lpVtbl).enum_objects)(
                    self.raw,
                    Some(enum_actuator_objects_callback),
                    &mut context as *mut _ as LPVOID,
                    DIDFT_AXIS | DIDFT_FFACTUATOR,
                )
            };
            if failed(hr) {
                return Err(DirectInputError::EnumActuators {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                    hr: hr as u32,
                });
            }
            if context.object_ids.is_empty() {
                return Err(DirectInputError::NoActuators {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                });
            }

            Ok(context.object_ids)
        }

        fn actuator_object_id(&self, index: usize) -> Result<DWORD, DirectInputError> {
            self.actuator_object_ids.get(index).copied().ok_or_else(|| {
                DirectInputError::NoActuators {
                    instance_name: self.info.instance_name.clone(),
                    instance_guid: self.info.instance_guid.clone(),
                }
            })
        }

        fn effect_for_kind(
            &self,
            kind: EffectKind,
        ) -> Option<(*mut IDirectInputEffect, &'static str)> {
            match kind {
                EffectKind::Constant => Some((self.effects.constant, "constant")),
                EffectKind::Sine
                | EffectKind::Square
                | EffectKind::Triangle
                | EffectKind::SawUp
                | EffectKind::SawDown => Some((self.effects.sine, "sine")),
                EffectKind::Spring => Some((self.effects.spring, "spring")),
                EffectKind::Damper => Some((self.effects.damper, "damper")),
                EffectKind::None
                | EffectKind::Ramp
                | EffectKind::Inertia
                | EffectKind::Friction
                | EffectKind::Custom
                | EffectKind::Unknown(_) => None,
            }
        }
    }

    impl HiddenWindow {
        fn create() -> Result<Self, DirectInputError> {
            let module = unsafe { GetModuleHandleW(null()) };
            let class_name = wide_null("STATIC");
            let title = wide_null("Torquebridge-directinput");
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
            self.effects.release_all();
            if !self.raw.is_null() {
                unsafe {
                    let _ = ((*(*self.raw).lpVtbl).unacquire)(self.raw);
                    ((*(*self.raw).lpVtbl).parent.release)(self.raw.cast());
                }
            }
        }
    }

    impl Default for LoadedEffects {
        fn default() -> Self {
            Self {
                constant: null_mut(),
                sine: null_mut(),
                spring: null_mut(),
                damper: null_mut(),
            }
        }
    }

    impl LoadedEffects {
        fn release_all(&mut self) {
            release_effect(&mut self.constant);
            release_effect(&mut self.sine);
            release_effect(&mut self.spring);
            release_effect(&mut self.damper);
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

    unsafe extern "system" fn enum_actuator_objects_callback(
        object: *const DIDEVICEOBJECTINSTANCEW,
        context: LPVOID,
    ) -> BOOL {
        if object.is_null() || context.is_null() {
            return 0;
        }

        let object = unsafe { &*object };
        let context = unsafe { &mut *(context as *mut EnumActuatorsContext) };
        context.object_ids.push(object.dwType);
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

    fn release_effect(effect: &mut *mut IDirectInputEffect) {
        if !effect.is_null() {
            unsafe {
                ((*(*(*effect)).lpVtbl).parent.release)((*effect).cast());
            }
            *effect = null_mut();
        }
    }

    fn clamp_signed_10k(value: i32) -> i32 {
        value.clamp(-10_000, 10_000)
    }

    fn clamp_unsigned_10k(value: i32) -> DWORD {
        value.clamp(0, 10_000) as DWORD
    }

    fn ms_to_us(value_ms: i32) -> DWORD {
        if value_ms < 0 {
            INFINITE_DURATION
        } else {
            (value_ms as DWORD).saturating_mul(1000)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_effect(
        type_specific: *mut c_void,
        type_size: DWORD,
        axes: &mut [DWORD; 1],
        directions: &mut [i32; 1],
        gain: i32,
        duration_ms: i32,
        sample_period: DWORD,
        trigger_button: i32,
        trigger_repeat_interval: DWORD,
        axis_count: DWORD,
    ) -> DIEFFECT {
        DIEFFECT {
            dwSize: size_of::<DIEFFECT>() as DWORD,
            dwFlags: DIEFF_OBJECTIDS | DIEFF_CARTESIAN,
            dwDuration: ms_to_us(duration_ms),
            dwSamplePeriod: sample_period,
            dwGain: clamp_unsigned_10k(gain),
            dwTriggerButton: if trigger_button < 0 {
                DIEB_NOTRIGGER
            } else {
                trigger_button as DWORD
            },
            dwTriggerRepeatInterval: trigger_repeat_interval,
            cAxes: axis_count,
            rgdwAxes: axes.as_mut_ptr(),
            rglDirection: directions.as_mut_ptr(),
            lpEnvelope: null_mut(),
            cbTypeSpecificParams: type_size,
            lpvTypeSpecificParams: type_specific,
            dwStartDelay: 0,
        }
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

        #[test]
        fn make_effect_uses_object_ids_flag() {
            let mut axes = [1234u32; 1];
            let mut directions = [0i32; 1];
            let mut payload = DICONSTANTFORCE { lMagnitude: 0 };
            let effect = make_effect(
                &mut payload as *mut _ as *mut c_void,
                size_of::<DICONSTANTFORCE>() as DWORD,
                &mut axes,
                &mut directions,
                10_000,
                -1,
                0,
                -1,
                0,
                1,
            );

            assert_eq!(effect.dwFlags, DIEFF_OBJECTIDS | DIEFF_CARTESIAN);
            assert_eq!(effect.cAxes, 1);
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

        pub fn actuator_count(&self) -> usize {
            0
        }

        pub fn actuator_object_ids(&self) -> Vec<u32> {
            Vec::new()
        }

        pub fn cached_capabilities(&self) -> DirectInputCapabilities {
            unreachable!()
        }

        pub fn apply_commands(
            &mut self,
            _commands: &[WheelCommand],
        ) -> Result<(), DirectInputError> {
            Err(DirectInputError::Unsupported)
        }
    }
}

pub use imp::*;
