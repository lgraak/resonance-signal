//! Per-user Windows startup registration owned by Resonance Signal.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const VALUE_NAME: &str = "ResonanceSignal";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationState {
    Enabled,
    Missing,
    Stale,
}

pub(crate) trait StartupStore {
    fn read(&self) -> Result<Option<OsString>, String>;
    fn write(&self, command: &OsStr) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

pub(crate) struct StartupRegistration<S = RegistryStore> {
    store: S,
}

impl StartupRegistration<RegistryStore> {
    pub fn current_user() -> Self {
        Self {
            store: RegistryStore,
        }
    }
}

impl<S: StartupStore> StartupRegistration<S> {
    pub fn state(&self, executable: &Path) -> Result<RegistrationState, String> {
        let expected = startup_command(executable)?;
        match self.store.read()? {
            None => Ok(RegistrationState::Missing),
            Some(actual) if commands_match(&actual, &expected) => Ok(RegistrationState::Enabled),
            Some(_) => Ok(RegistrationState::Stale),
        }
    }

    pub fn enable(&self, executable: &Path) -> Result<(), String> {
        let command = startup_command(executable)?;
        self.store.write(&command)
    }

    pub fn disable(&self) -> Result<(), String> {
        self.store.delete()
    }
}

fn startup_command(executable: &Path) -> Result<OsString, String> {
    if !executable.is_absolute() {
        return Err("startup executable path must be absolute".to_string());
    }
    if executable
        .as_os_str()
        .encode_wide()
        .any(|unit| unit == b'"' as u16)
    {
        return Err("startup executable path contains an invalid quote".to_string());
    }
    let mut command = OsString::from("\"");
    command.push(executable.as_os_str());
    command.push("\" --tray");
    Ok(command)
}

fn commands_match(actual: &OsStr, expected: &OsStr) -> bool {
    let actual = actual.encode_wide().collect::<Vec<_>>();
    let expected = expected.encode_wide().collect::<Vec<_>>();
    if actual.len() > i32::MAX as usize || expected.len() > i32::MAX as usize {
        return false;
    }
    unsafe {
        CompareStringOrdinal(
            actual.as_ptr(),
            actual.len() as i32,
            expected.as_ptr(),
            expected.len() as i32,
            1,
        ) == CSTR_EQUAL
    }
}

pub struct RegistryStore;

impl StartupStore for RegistryStore {
    fn read(&self) -> Result<Option<OsString>, String> {
        let mut key = 0 as HKEY;
        let run_key = wide_null(RUN_KEY);
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                run_key.as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut key,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check_status(status, "open per-user startup key")?;
        let result = read_value(key);
        unsafe { RegCloseKey(key) };
        result
    }

    fn write(&self, command: &OsStr) -> Result<(), String> {
        let mut key = 0 as HKEY;
        let run_key = wide_null(RUN_KEY);
        let mut disposition = 0;
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                run_key.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                &mut disposition,
            )
        };
        check_status(status, "open per-user startup key for writing")?;
        let value_name = wide_null(VALUE_NAME);
        let data = wide_null_os(command);
        let status = unsafe {
            RegSetValueExW(
                key,
                value_name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * size_of::<u16>()) as u32,
            )
        };
        unsafe { RegCloseKey(key) };
        check_status(status, "write Resonance Signal startup registration")
    }

    fn delete(&self) -> Result<(), String> {
        let mut key = 0 as HKEY;
        let run_key = wide_null(RUN_KEY);
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                run_key.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check_status(status, "open per-user startup key for removal")?;
        let value_name = wide_null(VALUE_NAME);
        let status = unsafe { RegDeleteValueW(key, value_name.as_ptr()) };
        unsafe { RegCloseKey(key) };
        if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check_status(status, "remove Resonance Signal startup registration")
        }
    }
}

fn read_value(key: HKEY) -> Result<Option<OsString>, String> {
    let value_name = wide_null(VALUE_NAME);
    let mut kind = 0;
    let mut byte_count = 0;
    let status = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut byte_count,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    check_status(status, "read Resonance Signal startup registration size")?;
    if kind != REG_SZ || byte_count % 2 != 0 {
        return Ok(Some(OsString::from("<invalid-registration>")));
    }
    let mut data = vec![0_u16; byte_count as usize / 2];
    let status = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut byte_count,
        )
    };
    check_status(status, "read Resonance Signal startup registration")?;
    if data.last() == Some(&0) {
        data.pop();
    }
    Ok(Some(OsString::from_wide(&data)))
}

fn check_status(status: u32, operation: &str) -> Result<(), String> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("failed to {operation}: Windows error {status}"))
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    wide_null_os(OsStr::new(value))
}

fn wide_null_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[derive(Default)]
    struct MemoryStore {
        value: RefCell<Option<OsString>>,
    }

    impl StartupStore for MemoryStore {
        fn read(&self) -> Result<Option<OsString>, String> {
            Ok(self.value.borrow().clone())
        }

        fn write(&self, command: &OsStr) -> Result<(), String> {
            self.value.replace(Some(command.to_os_string()));
            Ok(())
        }

        fn delete(&self) -> Result<(), String> {
            self.value.replace(None);
            Ok(())
        }
    }

    fn executable() -> PathBuf {
        PathBuf::from(r"C:\Program Files\Resonance Signal\resonance-agent.exe")
    }

    #[test]
    fn command_quotes_paths_and_uses_explicit_tray_mode() {
        assert_eq!(
            startup_command(&executable()).unwrap(),
            OsString::from(r#""C:\Program Files\Resonance Signal\resonance-agent.exe" --tray"#)
        );
    }

    #[test]
    fn state_is_enabled_only_for_the_exact_current_command() {
        let store = MemoryStore::default();
        let registration = StartupRegistration { store };
        assert_eq!(
            registration.state(&executable()).unwrap(),
            RegistrationState::Missing
        );

        registration.store.value.replace(Some(OsString::from(
            r#""C:\Old\resonance-agent.exe" --tray"#,
        )));
        assert_eq!(
            registration.state(&executable()).unwrap(),
            RegistrationState::Stale
        );

        registration.store.value.replace(Some(OsString::from(
            r"C:\Program Files\Resonance Signal\resonance-agent.exe --tray",
        )));
        assert_eq!(
            registration.state(&executable()).unwrap(),
            RegistrationState::Stale
        );

        registration.enable(&executable()).unwrap();
        assert_eq!(
            registration.state(&executable()).unwrap(),
            RegistrationState::Enabled
        );
    }

    #[test]
    fn explicit_enable_and_disable_update_only_the_owned_value() {
        let registration = StartupRegistration {
            store: MemoryStore::default(),
        };
        registration.enable(&executable()).unwrap();
        assert!(registration.store.value.borrow().is_some());
        registration.disable().unwrap();
        assert!(registration.store.value.borrow().is_none());
    }

    #[test]
    #[ignore = "mutates the current user's owned Windows startup value"]
    fn real_current_user_registration_round_trip() {
        let executable = std::env::var_os("RESONANCE_STARTUP_VALIDATION_EXE")
            .map(PathBuf::from)
            .expect("set RESONANCE_STARTUP_VALIDATION_EXE to the packaged executable");
        let registration = StartupRegistration::current_user();
        assert_eq!(
            registration.state(&executable).unwrap(),
            RegistrationState::Missing,
            "refusing to overwrite an existing Resonance Signal startup value"
        );

        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = StartupRegistration::current_user().disable();
            }
        }
        let _cleanup = Cleanup;

        registration
            .store
            .write(OsStr::new(r#""C:\stale\resonance-agent.exe" --tray"#))
            .unwrap();
        assert_eq!(
            registration.state(&executable).unwrap(),
            RegistrationState::Stale
        );

        registration.enable(&executable).unwrap();
        assert_eq!(
            registration.state(&executable).unwrap(),
            RegistrationState::Enabled
        );

        registration.disable().unwrap();
        assert_eq!(
            registration.state(&executable).unwrap(),
            RegistrationState::Missing
        );
    }
}
