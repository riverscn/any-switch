use crate::backup::sha256_hex;
use crate::paths::{ensure_dir_private, write_private};
use anyhow::{anyhow, Result};
use std::fs;
use std::path::PathBuf;

pub fn read_generic_password(service: &str, account: &str) -> Result<Vec<u8>> {
    if let Some(path) = fixture_entry_path(service, account)? {
        if !path.exists() {
            return Err(anyhow!(
                "KeychainUnavailable: fixture entry not found for service={service} account={account}"
            ));
        }
        return Ok(fs::read(path)?);
    }
    read_platform_generic_password(service, account)
}

pub fn generic_password_exists(service: &str, account: &str) -> Result<bool> {
    if let Some(path) = fixture_entry_path(service, account)? {
        return Ok(path.exists());
    }
    platform_generic_password_exists(service, account)
}

pub fn write_generic_password(service: &str, account: &str, bytes: &[u8]) -> Result<()> {
    if let Some(path) = fixture_entry_path(service, account)? {
        write_private(&path, bytes)?;
        return Ok(());
    }
    write_platform_generic_password(service, account, bytes)
}

fn fixture_entry_path(service: &str, account: &str) -> Result<Option<PathBuf>> {
    let Some(root) = std::env::var_os("ANY_SWITCH_KEYCHAIN_FIXTURE_DIR") else {
        return Ok(None);
    };
    let root = PathBuf::from(root);
    ensure_dir_private(&root)?;
    let digest = sha256_hex(format!("{service}\0{account}").as_bytes());
    Ok(Some(root.join(format!("{digest}.secret"))))
}

#[cfg(target_os = "macos")]
fn read_platform_generic_password(service: &str, account: &str) -> Result<Vec<u8>> {
    macos::read_generic_password(service, account)
}

#[cfg(target_os = "macos")]
fn platform_generic_password_exists(service: &str, account: &str) -> Result<bool> {
    macos::generic_password_exists(service, account)
}

#[cfg(not(target_os = "macos"))]
fn read_platform_generic_password(service: &str, account: &str) -> Result<Vec<u8>> {
    let _ = (service, account);
    Err(anyhow!(
        "KeychainUnavailable: secret_entry is only available on macOS in this build"
    ))
}

#[cfg(not(target_os = "macos"))]
fn platform_generic_password_exists(service: &str, account: &str) -> Result<bool> {
    let _ = (service, account);
    Err(anyhow!(
        "KeychainUnavailable: secret_entry is only available on macOS in this build"
    ))
}

#[cfg(target_os = "macos")]
fn write_platform_generic_password(service: &str, account: &str, bytes: &[u8]) -> Result<()> {
    macos::write_generic_password(service, account, bytes)
}

#[cfg(not(target_os = "macos"))]
fn write_platform_generic_password(service: &str, account: &str, bytes: &[u8]) -> Result<()> {
    let _ = (service, account, bytes);
    Err(anyhow!(
        "KeychainUnavailable: secret_entry is only available on macOS in this build"
    ))
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use anyhow::Context;
    use libc::{c_char, c_void};
    use std::ptr;

    const ERR_SEC_SUCCESS: i32 = 0;
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn SecKeychainFindGenericPassword(
            keychain_or_array: *mut c_void,
            service_name_length: u32,
            service_name: *const c_char,
            account_name_length: u32,
            account_name: *const c_char,
            password_length: *mut u32,
            password_data: *mut *mut c_void,
            item_ref: *mut *mut c_void,
        ) -> i32;

        fn SecKeychainAddGenericPassword(
            keychain: *mut c_void,
            service_name_length: u32,
            service_name: *const c_char,
            account_name_length: u32,
            account_name: *const c_char,
            password_length: u32,
            password_data: *const c_void,
            item_ref: *mut *mut c_void,
        ) -> i32;

        fn SecKeychainItemModifyAttributesAndData(
            item_ref: *mut c_void,
            attr_list: *const c_void,
            length: u32,
            data: *const c_void,
        ) -> i32;

        fn SecKeychainItemFreeContent(attr_list: *mut c_void, data: *mut c_void) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    pub fn read_generic_password(service: &str, account: &str) -> Result<Vec<u8>> {
        let service_len = ffi_len(service, "service")?;
        let account_len = ffi_len(account, "account")?;
        let mut password_len = 0u32;
        let mut password_data = ptr::null_mut();

        let status = unsafe {
            // SAFETY: Security.framework consumes byte pointers together with explicit lengths.
            // The pointers are valid for this call, output pointers are initialized above, and
            // returned password data is released with SecKeychainItemFreeContent below.
            SecKeychainFindGenericPassword(
                ptr::null_mut(),
                service_len,
                service.as_ptr().cast(),
                account_len,
                account.as_ptr().cast(),
                &mut password_len,
                &mut password_data,
                ptr::null_mut(),
            )
        };
        if status != ERR_SEC_SUCCESS {
            return Err(keychain_error(
                "find generic password",
                service,
                account,
                status,
            ));
        }

        let bytes = unsafe {
            // SAFETY: On success, Security.framework returns password_data/password_len as a
            // readable buffer. Copy it immediately before freeing the framework-owned memory.
            std::slice::from_raw_parts(password_data.cast::<u8>(), password_len as usize).to_vec()
        };
        free_password_data(password_data)?;
        Ok(bytes)
    }

    pub fn generic_password_exists(service: &str, account: &str) -> Result<bool> {
        let service_len = ffi_len(service, "service")?;
        let account_len = ffi_len(account, "account")?;
        let mut item_ref = ptr::null_mut();

        let status = unsafe {
            // SAFETY: Passing NULL for password length/data asks Security.framework to return
            // only an item reference, avoiding secret byte retrieval for existence diagnostics.
            SecKeychainFindGenericPassword(
                ptr::null_mut(),
                service_len,
                service.as_ptr().cast(),
                account_len,
                account.as_ptr().cast(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut item_ref,
            )
        };
        match status {
            ERR_SEC_SUCCESS => {
                release_item(item_ref);
                Ok(true)
            }
            ERR_SEC_ITEM_NOT_FOUND => Ok(false),
            status => Err(keychain_error(
                "find generic password item",
                service,
                account,
                status,
            )),
        }
    }

    pub fn write_generic_password(service: &str, account: &str, bytes: &[u8]) -> Result<()> {
        let service_len = ffi_len(service, "service")?;
        let account_len = ffi_len(account, "account")?;
        let password_len = ffi_len(bytes, "secret_entry")?;
        let mut existing_len = 0u32;
        let mut existing_data = ptr::null_mut();
        let mut item_ref = ptr::null_mut();

        let find_status = unsafe {
            // SAFETY: See read_generic_password. Requesting item_ref lets us update the existing
            // generic password without putting secret bytes in process argv.
            SecKeychainFindGenericPassword(
                ptr::null_mut(),
                service_len,
                service.as_ptr().cast(),
                account_len,
                account.as_ptr().cast(),
                &mut existing_len,
                &mut existing_data,
                &mut item_ref,
            )
        };

        match find_status {
            ERR_SEC_SUCCESS => {
                free_password_data(existing_data)?;
                let status = unsafe {
                    // SAFETY: item_ref was returned by Security.framework and remains valid until
                    // released below. bytes is a valid input buffer for the duration of the call.
                    SecKeychainItemModifyAttributesAndData(
                        item_ref,
                        ptr::null(),
                        password_len,
                        bytes.as_ptr().cast(),
                    )
                };
                release_item(item_ref);
                if status != ERR_SEC_SUCCESS {
                    return Err(keychain_error(
                        "update generic password",
                        service,
                        account,
                        status,
                    ));
                }
                Ok(())
            }
            ERR_SEC_ITEM_NOT_FOUND => {
                let mut new_item_ref = ptr::null_mut();
                let status = unsafe {
                    // SAFETY: Security.framework reads the provided service/account/password
                    // buffers using explicit lengths during this call.
                    SecKeychainAddGenericPassword(
                        ptr::null_mut(),
                        service_len,
                        service.as_ptr().cast(),
                        account_len,
                        account.as_ptr().cast(),
                        password_len,
                        bytes.as_ptr().cast(),
                        &mut new_item_ref,
                    )
                };
                release_item(new_item_ref);
                if status != ERR_SEC_SUCCESS {
                    return Err(keychain_error(
                        "add generic password",
                        service,
                        account,
                        status,
                    ));
                }
                Ok(())
            }
            status => Err(keychain_error(
                "find generic password before update",
                service,
                account,
                status,
            )),
        }
    }

    fn ffi_len<T>(value: impl AsRef<[T]>, name: &str) -> Result<u32> {
        value
            .as_ref()
            .len()
            .try_into()
            .with_context(|| format!("KeychainUnavailable: {name} is too large"))
    }

    fn free_password_data(data: *mut c_void) -> Result<()> {
        if data.is_null() {
            return Ok(());
        }
        let status = unsafe {
            // SAFETY: data was returned by SecKeychainFindGenericPassword and has not been freed.
            SecKeychainItemFreeContent(ptr::null_mut(), data)
        };
        if status == ERR_SEC_SUCCESS {
            Ok(())
        } else {
            Err(anyhow!(
                "KeychainUnavailable: free generic password content failed with OSStatus {status}"
            ))
        }
    }

    fn release_item(item_ref: *mut c_void) {
        if !item_ref.is_null() {
            unsafe {
                // SAFETY: item_ref is a CoreFoundation object returned by Security.framework.
                CFRelease(item_ref.cast_const());
            }
        }
    }

    fn keychain_error(action: &str, service: &str, account: &str, status: i32) -> anyhow::Error {
        anyhow!(
            "KeychainUnavailable: Security.framework {action} failed for service={service} account={account} with OSStatus {status}"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn fixture_backend_round_trips_secret_entry() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = tempdir().unwrap();
        std::env::set_var("ANY_SWITCH_KEYCHAIN_FIXTURE_DIR", dir.path());
        write_generic_password("Claude Code-credentials", "alice", br#"{"token":"a"}"#).unwrap();
        assert!(generic_password_exists("Claude Code-credentials", "alice").unwrap());
        let bytes = read_generic_password("Claude Code-credentials", "alice").unwrap();
        std::env::remove_var("ANY_SWITCH_KEYCHAIN_FIXTURE_DIR");
        assert_eq!(bytes, br#"{"token":"a"}"#);
    }

    #[test]
    fn fixture_backend_preserves_non_utf8_secret_bytes() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = tempdir().unwrap();
        std::env::set_var("ANY_SWITCH_KEYCHAIN_FIXTURE_DIR", dir.path());
        write_generic_password("Claude Code-credentials", "binary", &[0, 159, 255]).unwrap();
        let bytes = read_generic_password("Claude Code-credentials", "binary").unwrap();
        std::env::remove_var("ANY_SWITCH_KEYCHAIN_FIXTURE_DIR");
        assert_eq!(bytes, [0, 159, 255]);
    }
}
