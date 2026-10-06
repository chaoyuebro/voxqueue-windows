//! Read the current user's DashScope key from Windows Credential Manager.
//!
//! The value is never passed through a command line, environment variable, or log.

#![cfg(windows)]

use std::ffi::c_void;
use std::io;
use std::ptr;

const TARGET: &str = "EasyCodexInput/DASHSCOPE_API_KEY";
const MINIMAX_TARGET: &str = "EasyCodexInput/MINIMAX_API_KEY";
const DEVICE_SECRET_TARGET: &str = "EasyCodexInput/DEVICE_SECRET_V1";
const CRED_TYPE_GENERIC: u32 = 1;
const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;
const ERROR_NOT_FOUND: i32 = 1168;
const MAX_BLOB_BYTES: usize = 4096;

#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

#[repr(C)]
struct CredentialW {
    flags: u32,
    kind: u32,
    target_name: *mut u16,
    comment: *mut u16,
    last_written: FileTime,
    blob_size: u32,
    blob: *mut u8,
    persist: u32,
    attribute_count: u32,
    attributes: *mut c_void,
    target_alias: *mut u16,
    user_name: *mut u16,
}

#[link(name = "Advapi32")]
unsafe extern "system" {
    fn CredReadW(target: *const u16, kind: u32, flags: u32, result: *mut *mut CredentialW) -> i32;
    fn CredWriteW(credential: *const CredentialW, flags: u32) -> i32;
    fn CredFree(buffer: *mut c_void);
}

struct CredentialGuard(*mut CredentialW);

impl Drop for CredentialGuard {
    fn drop(&mut self) {
        // SAFETY: CredReadW returned this pointer on success, and the guard owns it once.
        unsafe { CredFree(self.0.cast()) };
    }
}

fn read_credential(target_name: &str) -> io::Result<Option<Vec<u8>>> {
    let target: Vec<u16> = target_name.encode_utf16().chain(Some(0)).collect();
    let mut result = ptr::null_mut();
    // SAFETY: target is NUL terminated and result points to writable pointer storage.
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut result) } == 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_NOT_FOUND) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if result.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty credential result",
        ));
    }
    let _guard = CredentialGuard(result);
    // SAFETY: CredReadW returned a valid CREDENTIALW, kept alive by _guard.
    let record = unsafe { &*result };
    let length = record.blob_size as usize;
    if length == 0 || length > MAX_BLOB_BYTES || record.blob.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid credential size",
        ));
    }
    // SAFETY: CredReadW allocated blob_size bytes inside its live result allocation.
    let bytes = unsafe { std::slice::from_raw_parts(record.blob, length) };
    Ok(Some(bytes.to_vec()))
}

pub fn read_device_secret() -> io::Result<Option<[u8; 32]>> {
    let Some(mut bytes) = read_credential(DEVICE_SECRET_TARGET)? else {
        return Ok(None);
    };
    if bytes.len() != 32 || bytes.iter().all(|byte| *byte == 0) {
        bytes.fill(0);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid device secret",
        ));
    }
    let mut secret = [0_u8; 32];
    secret.copy_from_slice(&bytes);
    bytes.fill(0);
    Ok(Some(secret))
}

pub fn store_device_secret_once(secret: &[u8; 32]) -> io::Result<()> {
    if secret.iter().all(|byte| *byte == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid device secret",
        ));
    }
    if let Some(existing) = read_device_secret()? {
        return if existing == *secret {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "different device secret exists",
            ))
        };
    }
    let mut target: Vec<u16> = DEVICE_SECRET_TARGET.encode_utf16().chain(Some(0)).collect();
    let mut blob = zeroize::Zeroizing::new(secret.to_vec());
    let credential = CredentialW {
        flags: 0,
        kind: CRED_TYPE_GENERIC,
        target_name: target.as_mut_ptr(),
        comment: ptr::null_mut(),
        last_written: FileTime { low: 0, high: 0 },
        blob_size: blob.len() as u32,
        blob: blob.as_mut_ptr(),
        persist: CRED_PERSIST_LOCAL_MACHINE,
        attribute_count: 0,
        attributes: ptr::null_mut(),
        target_alias: ptr::null_mut(),
        user_name: ptr::null_mut(),
    };
    // SAFETY: all pointers reference live local buffers for the duration of the call.
    if unsafe { CredWriteW(&credential, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn read_dashscope_key() -> io::Result<Option<Vec<u8>>> {
    let Some(bytes) = read_credential(TARGET)? else {
        return Ok(None);
    };
    let bytes = zeroize::Zeroizing::new(bytes);
    let length = bytes.len();
    if length % 2 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid credential size",
        ));
    }
    let mut units = Vec::with_capacity(length / 2);
    for pair in bytes.chunks_exact(2) {
        units.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    let text = String::from_utf16(&units)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "credential encoding"));
    units.fill(0);
    let text = text?;
    if !text.starts_with("sk-") || text.len() < 20 || text.chars().any(char::is_whitespace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "credential format",
        ));
    }
    Ok(Some(text.into_bytes()))
}

pub fn read_minimax_key() -> io::Result<Option<zeroize::Zeroizing<Vec<u8>>>> {
    let Some(bytes) = read_credential(MINIMAX_TARGET)? else {
        return Ok(None);
    };
    let bytes = zeroize::Zeroizing::new(bytes);
    if bytes.len() % 2 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid credential size",
        ));
    }
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        units.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    let text = String::from_utf16(&units)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "credential encoding"));
    units.fill(0);
    let text = text?;
    if text.len() < 20 || text.len() > 2048 || !text.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "credential format",
        ));
    }
    Ok(Some(zeroize::Zeroizing::new(text.into_bytes())))
}

const LAN_RECOVERY_TARGET: &str = "EasyCodexInput/LAN_RECOVERY_V1";
pub fn read_lan_recovery() -> io::Result<Option<zeroize::Zeroizing<Vec<u8>>>> {
    Ok(read_credential(LAN_RECOVERY_TARGET)?.map(zeroize::Zeroizing::new))
}
pub fn store_lan_recovery(bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_BLOB_BYTES { return Err(io::Error::new(io::ErrorKind::InvalidInput,"invalid recovery profile")); }
    let mut target: Vec<u16> = LAN_RECOVERY_TARGET.encode_utf16().chain(Some(0)).collect();
    let mut blob = zeroize::Zeroizing::new(bytes.to_vec());
    let record = CredentialW { flags:0,kind:CRED_TYPE_GENERIC,target_name:target.as_mut_ptr(),comment:ptr::null_mut(),last_written:FileTime{low:0,high:0},blob_size:blob.len() as u32,blob:blob.as_mut_ptr(),persist:CRED_PERSIST_LOCAL_MACHINE,attribute_count:0,attributes:ptr::null_mut(),target_alias:ptr::null_mut(),user_name:ptr::null_mut() };
    if unsafe { CredWriteW(&record,0) } == 0 { return Err(io::Error::last_os_error()); }
    Ok(())
}
