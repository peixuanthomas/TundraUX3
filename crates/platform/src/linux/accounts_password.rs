//! Use the distribution's libcrypt; never pass passwords through process arguments or files.
use crate::service::ServiceError;
use std::ffi::{CStr, CString};

pub(super) fn password_hash(password: &str) -> Result<String, ServiceError> {
    let password = CString::new(password).map_err(|_| ServiceError::Unsupported)?;
    if password.as_bytes().is_empty() {
        return Err(ServiceError::Unsupported);
    }
    // libcrypt.so.1 is provided by libxcrypt on the supported Linux distributions.
    // Loading at runtime lets the application start even when account tools are absent.
    unsafe {
        let library = libc::dlopen(c"libcrypt.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if library.is_null() {
            return Err(ServiceError::ServiceUnavailable);
        }
        let result = hash_with_library(library, &password);
        libc::dlclose(library);
        result
    }
}

unsafe fn hash_with_library(
    library: *mut libc::c_void,
    password: &CStr,
) -> Result<String, ServiceError> {
    type Salt = unsafe extern "C" fn(
        *const libc::c_char,
        libc::c_ulong,
        *const libc::c_char,
        libc::c_int,
        *mut libc::c_char,
        libc::c_int,
    ) -> *mut libc::c_char;
    type Hash = unsafe extern "C" fn(
        *const libc::c_char,
        *const libc::c_char,
        *mut *mut libc::c_void,
        *mut libc::c_int,
    ) -> *mut libc::c_char;
    // SAFETY: resolve the public libxcrypt ABI; check symbols before converting them.
    let salt = unsafe { libc::dlsym(library, c"crypt_gensalt_rn".as_ptr()) };
    let hash = unsafe { libc::dlsym(library, c"crypt_ra".as_ptr()) };
    if salt.is_null() || hash.is_null() {
        return Err(ServiceError::Unsupported);
    }
    let salt: Salt = unsafe { std::mem::transmute(salt) };
    let hash: Hash = unsafe { std::mem::transmute(hash) };
    let mut setting = [0 as libc::c_char; 256];
    // A null random input requests entropy from the OS. $6$ is SHA-512 crypt,
    // supported by both Ubuntu and Fedora; it is not the application's Argon2 format.
    if unsafe {
        salt(
            c"$6$".as_ptr(),
            100_000,
            std::ptr::null(),
            0,
            setting.as_mut_ptr(),
            setting.len() as i32,
        )
    }
    .is_null()
    {
        return Err(ServiceError::Unknown);
    }
    let mut data = std::ptr::null_mut();
    let mut size = 0;
    let output = unsafe { hash(password.as_ptr(), setting.as_ptr(), &mut data, &mut size) };
    let result = if output.is_null() {
        Err(ServiceError::Unknown)
    } else {
        let value = unsafe { CStr::from_ptr(output) }
            .to_string_lossy()
            .into_owned();
        if value.starts_with("$6$") {
            Ok(value)
        } else {
            Err(ServiceError::Unknown)
        }
    };
    // crypt_ra allocates with malloc and may leave intermediate password data behind.
    if !data.is_null() {
        for offset in 0..size.max(0) as usize {
            unsafe {
                std::ptr::write_volatile(data.cast::<u8>().add(offset), 0);
            }
        }
        unsafe {
            libc::free(data);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_hash_uses_fresh_salt_and_rejects_nul() {
        let first = password_hash("TestPassword123!").unwrap();
        let second = password_hash("TestPassword123!").unwrap();
        assert!(first.starts_with("$6$rounds=100000$"));
        assert_ne!(first, second);
        assert!(!first.contains("TestPassword"));
        assert!(password_hash("bad\0password").is_err());
        assert!(password_hash("").is_err());
    }
}
