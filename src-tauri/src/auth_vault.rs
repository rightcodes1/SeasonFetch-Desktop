use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use seasonfetch_core::validation::is_valid_auth_storage_key;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_VALUE_LEN: usize = 32_768;

pub struct AuthVault {
    file_path: PathBuf,
    memory_fallback: Mutex<HashMap<String, String>>,
}

impl AuthVault {
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path,
            memory_fallback: Mutex::new(HashMap::new()),
        }
    }

    pub fn is_persistent_encryption_available(&self) -> bool {
        cfg!(windows)
    }

    fn read_disk(&self) -> HashMap<String, String> {
        if !self.file_path.exists() {
            return HashMap::new();
        }
        fs::read_to_string(&self.file_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn write_disk(&self, records: &HashMap<String, String>) -> Result<(), String> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = self.file_path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(records).map_err(|e| e.to_string())?;
        fs::write(&tmp, format!("{json}\n")).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.file_path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_item(&self, key: &str) -> Option<String> {
        if !is_valid_auth_storage_key(key) {
            return None;
        }
        #[cfg(windows)]
        {
            let records = self.read_disk();
            let encoded = records.get(key)?;
            let encrypted = BASE64.decode(encoded).ok()?;
            dpapi_unprotect(&encrypted).ok()
        }
        #[cfg(not(windows))]
        {
            self.memory_fallback
                .lock()
                .ok()
                .and_then(|map| map.get(key).cloned())
        }
    }

    pub fn set_item(&self, key: &str, value: &str) -> Result<bool, String> {
        if !is_valid_auth_storage_key(key) {
            return Err("Invalid auth storage key.".to_string());
        }
        if value.len() > MAX_VALUE_LEN {
            return Err("Auth storage value exceeds maximum allowed size.".to_string());
        }
        #[cfg(windows)]
        {
            let encrypted = dpapi_protect(value.as_bytes())?;
            let encoded = BASE64.encode(encrypted);
            let mut records = self.read_disk();
            records.insert(key.to_string(), encoded);
            self.write_disk(&records)?;
            Ok(true)
        }
        #[cfg(not(windows))]
        {
            // Never write unencrypted tokens to disk on non-Windows targets without an OS keystore.
            if let Ok(mut map) = self.memory_fallback.lock() {
                map.insert(key.to_string(), value.to_string());
            }
            Ok(false)
        }
    }

    pub fn remove_item(&self, key: &str) -> bool {
        if !is_valid_auth_storage_key(key) {
            return false;
        }
        if let Ok(mut map) = self.memory_fallback.lock() {
            map.remove(key);
        }
        #[cfg(windows)]
        {
            let mut records = self.read_disk();
            if records.remove(key).is_some() {
                let _ = self.write_disk(&records);
            }
        }
        true
    }
}

#[cfg(windows)]
fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err("Windows DPAPI CryptProtectData failed.".to_string());
    }
    let slice = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let result = slice.to_vec();
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(result)
}

#[cfg(windows)]
fn dpapi_unprotect(ciphertext: &[u8]) -> Result<String, String> {
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err("Windows DPAPI CryptUnprotectData failed.".to_string());
    }
    let slice = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let text = String::from_utf8(slice.to_vec()).map_err(|e| e.to_string());
    unsafe {
        LocalFree(output.pbData as _);
    }
    text
}
