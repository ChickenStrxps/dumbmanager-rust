//! Atomic service documents and current-user Windows DPAPI credentials.
//! These paths never point at another installation's settings or secrets.
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use base64::{engine::general_purpose::STANDARD, Engine};
use dumbmanager_core::legacy::{LegacyError, LegacyResult};
use serde_json::{json, Value};
use zeroize::Zeroize;

const MAX_DOCUMENT: u64 = 8 * 1024 * 1024;

pub struct Runtime {
    root: PathBuf,
    lock: Mutex<()>,
}

impl Runtime {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            lock: Mutex::new(()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn directory(&self) -> LegacyResult<()> {
        if let Ok(meta) = fs::symlink_metadata(&self.root) {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(LegacyError::invalid(
                    "The service directory must be an ordinary folder.",
                ));
            }
        } else {
            fs::create_dir_all(&self.root).map_err(|_| storage())?;
        }
        let parent = self.root.parent().ok_or_else(storage)?;
        if fs::canonicalize(&self.root)
            .map_err(|_| storage())?
            .parent()
            != Some(fs::canonicalize(parent).map_err(|_| storage())?.as_path())
        {
            return Err(LegacyError::invalid(
                "Service storage resolves outside this app's data folder.",
            ));
        }
        Ok(())
    }

    fn path(&self, name: &str) -> LegacyResult<PathBuf> {
        if name.is_empty()
            || name.len() > 80
            || name.starts_with('.')
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            || name.contains("..")
        {
            return Err(LegacyError::invalid("Invalid service document name."));
        }
        self.directory()?;
        let path = self.root.join(name);
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink() || !m.is_file()) {
            return Err(LegacyError::invalid(
                "Service files cannot be links or directories.",
            ));
        }
        Ok(path)
    }

    fn read_locked(&self, name: &str) -> LegacyResult<Value> {
        let path = self.path(name)?;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
            Err(_) => return Err(storage()),
        };
        let mut raw = Vec::new();
        file.take(MAX_DOCUMENT + 1)
            .read_to_end(&mut raw)
            .map_err(|_| storage())?;
        if raw.len() as u64 > MAX_DOCUMENT {
            return Err(storage());
        }
        serde_json::from_slice(&raw).map_err(|_| storage())
    }

    fn write_locked(&self, name: &str, value: &Value) -> LegacyResult<()> {
        let path = self.path(name)?;
        let raw = serde_json::to_vec(value).map_err(|_| storage())?;
        if raw.len() as u64 > MAX_DOCUMENT {
            return Err(LegacyError::invalid(
                "Service storage is full; export or remove old records first.",
            ));
        }
        let temp = self
            .root
            .join(format!("write-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|_| storage())?;
            file.write_all(&raw)
                .and_then(|()| file.sync_all())
                .map_err(|_| storage())?;
            drop(file);
            fs::rename(&temp, path).map_err(|_| storage())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }

    pub fn read_document(&self, name: &str) -> LegacyResult<Value> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.read_locked(name)
    }

    pub fn write_document(&self, name: &str, value: &Value) -> LegacyResult<()> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.write_locked(name, value)
    }

    /// The callback runs under the document lock. It must not call Runtime again
    /// or perform network work. The returned value is persisted atomically.
    pub fn update_document<F>(&self, name: &str, update: F) -> LegacyResult<Value>
    where
        F: FnOnce(&mut Value) -> LegacyResult<()>,
    {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut value = self.read_locked(name)?;
        update(&mut value)?;
        self.write_locked(name, &value)?;
        Ok(value)
    }

    pub fn set_secret(&self, name: &str, secret: &str) -> LegacyResult<()> {
        if secret.is_empty() {
            return self.remove_secret(name);
        }
        if secret.len() > 32768 || secret.contains('\0') {
            return Err(LegacyError::invalid(
                "Invalid credential length or content.",
            ));
        }
        let mut input = secret.as_bytes().to_vec();
        let protected = protect(&input, false);
        input.zeroize();
        let mut encrypted = protected?;
        let value = json!({"version":1,"protection":"windows-dpapi-current-user","data":STANDARD.encode(&encrypted)});
        encrypted.zeroize();
        self.write_document(&format!("secret-{name}.json"), &value)
    }

    pub fn secret(&self, name: &str) -> LegacyResult<Option<String>> {
        let value = self.read_document(&format!("secret-{name}.json"))?;
        let Some(raw) = value.get("data").and_then(Value::as_str) else {
            return Ok(None);
        };
        if value["version"] != 1 || value["protection"] != "windows-dpapi-current-user" {
            return Err(LegacyError::invalid(
                "Unsupported credential protection; enter the key again.",
            ));
        }
        let mut encrypted = STANDARD.decode(raw).map_err(|_| storage())?;
        let result = protect(&encrypted, true);
        encrypted.zeroize();
        let clear = result?;
        String::from_utf8(clear).map(Some).map_err(|_| storage())
    }

    pub fn remove_secret(&self, name: &str) -> LegacyResult<()> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let path = self.path(&format!("secret-{name}.json"))?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(storage()),
        }
    }
}

fn storage() -> LegacyError {
    LegacyError {
        error: "Could not read or save this app's service data. Existing records were kept.".into(),
        code: "service_storage".into(),
        status: 500,
    }
}

#[cfg(windows)]
fn protect(input: &[u8], decrypt: bool) -> LegacyResult<Vec<u8>> {
    use std::ffi::c_void;
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    #[link(name = "Crypt32")]
    extern "system" {
        fn CryptProtectData(
            data: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *const c_void,
            prompt: *const c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            data: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *const c_void,
            prompt: *const c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "Kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    let data = Blob {
        len: input.len() as u32,
        data: input.as_ptr().cast_mut(),
    };
    let entropy_bytes = b"io.dumbmanager.rust.service-credentials.v1";
    let entropy = Blob {
        len: entropy_bytes.len() as u32,
        data: entropy_bytes.as_ptr().cast_mut(),
    };
    let mut output = Blob {
        len: 0,
        data: std::ptr::null_mut(),
    };
    // SAFETY: all blobs are live, correctly sized buffers. DPAPI owns output;
    // copy it, wipe its plaintext (on decrypt) and free using LocalFree.
    let success = unsafe {
        if decrypt {
            CryptUnprotectData(
                &data,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                1,
                &mut output,
            )
        } else {
            CryptProtectData(
                &data,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                1,
                &mut output,
            )
        }
    };
    if success == 0 {
        return Err(LegacyError {
            error: "Windows could not unlock this credential. Re-enter it in this app's settings."
                .into(),
            code: "credential_protection".into(),
            status: 503,
        });
    }
    let result = unsafe {
        let bytes = std::slice::from_raw_parts_mut(output.data, output.len as usize);
        let copy = bytes.to_vec();
        bytes.zeroize();
        LocalFree(output.data.cast());
        copy
    };
    Ok(result)
}

#[cfg(not(windows))]
fn protect(_input: &[u8], _decrypt: bool) -> LegacyResult<Vec<u8>> {
    Err(LegacyError::unavailable(
        "Persistent credentials require Windows DPAPI in this build.",
    ))
}
