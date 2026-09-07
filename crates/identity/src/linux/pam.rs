//! Minimal Linux-PAM ABI, loaded at runtime so builds need no PAM headers.
//! See https://www.linux-pam.org/Linux-PAM-html/adg-interface.html.
use super::system_error;
use crate::CoreError;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

const SUCCESS: c_int = 0;
const CONV_ERR: c_int = 19;
const DISALLOW_NULL_AUTHTOK: c_int = 1;

#[repr(C)]
struct Message {
    style: c_int,
    text: *const c_char,
}
#[repr(C)]
struct Response {
    text: *mut c_char,
    code: c_int,
}
#[repr(C)]
struct Conversation {
    callback: unsafe extern "C" fn(
        c_int,
        *const *const Message,
        *mut *mut Response,
        *mut c_void,
    ) -> c_int,
    data: *mut c_void,
}
struct Credentials {
    username: CString,
    password: Vec<u8>,
    password_sent: bool,
}
impl Drop for Credentials {
    fn drop(&mut self) {
        for byte in &mut self.password {
            // Volatile writes keep secret erasure from being optimized away.
            unsafe { ptr::write_volatile(byte, 0) };
        }
    }
}

unsafe fn free_responses(responses: *mut Response, count: usize) {
    // SAFETY: caller owns a calloc-allocated array with count initialized entries.
    unsafe {
        for index in 0..count {
            let value = (*responses.add(index)).text;
            if !value.is_null() {
                for offset in 0..libc::strlen(value) {
                    ptr::write_volatile(value.add(offset), 0);
                }
                libc::free(value.cast());
            }
        }
        libc::free(responses.cast());
    }
}

unsafe extern "C" fn converse(
    count: c_int,
    messages: *const *const Message,
    output: *mut *mut Response,
    data: *mut c_void,
) -> c_int {
    if output.is_null() {
        return CONV_ERR;
    }
    // SAFETY: PAM supplies output, message pointers and the live Credentials
    // passed to pam_start. No Rust allocation crosses the C ownership boundary.
    unsafe {
        *output = ptr::null_mut();
        if !(1..=32).contains(&count) || messages.is_null() || data.is_null() {
            return CONV_ERR;
        }
        let credentials = &mut *data.cast::<Credentials>();
        let responses = libc::calloc(count as usize, size_of::<Response>()).cast::<Response>();
        if responses.is_null() {
            return CONV_ERR;
        }
        for index in 0..count as usize {
            let message = *messages.add(index);
            if message.is_null() {
                free_responses(responses, count as usize);
                return CONV_ERR;
            }
            let value = match (*message).style {
                1 if !credentials.password_sent => {
                    credentials.password_sent = true;
                    credentials.password.as_ptr().cast::<c_char>()
                }
                2 => credentials.username.as_ptr(),
                3 | 4 => continue,
                // A second secret prompt could be MFA. Never replay the password
                // as an OTP or pretend to support an interactive conversation.
                _ => {
                    free_responses(responses, count as usize);
                    return CONV_ERR;
                }
            };
            (*responses.add(index)).text = libc::strdup(value);
            if (*responses.add(index)).text.is_null() {
                free_responses(responses, count as usize);
                return CONV_ERR;
            }
        }
        *output = responses;
    }
    SUCCESS
}

type Start = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *const Conversation,
    *mut *mut c_void,
) -> c_int;
type Check = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type End = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type GetItem = unsafe extern "C" fn(*const c_void, c_int, *mut *const c_void) -> c_int;
type SetItem = unsafe extern "C" fn(*mut c_void, c_int, *const c_void) -> c_int;

struct Library(*mut c_void);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.0);
        }
    }
}

fn check_status(status: c_int) -> Result<(), CoreError> {
    match status {
        SUCCESS => Ok(()),
        6 | 7 | 10 | 11 => Err(CoreError::InvalidCredentials),
        12 | 27 => Err(system_error(
            "Linux password has expired. Change it with passwd before signing in.",
        )),
        13 => Err(CoreError::AccountDisabled),
        CONV_ERR => Err(system_error(
            "Linux authentication requires an unsupported prompt (for example MFA).",
        )),
        _ => Err(system_error(format!(
            "Linux PAM authentication unavailable (status {status}). Check the PAM service configuration."
        ))),
    }
}

pub(super) fn authenticate(username: &str, password: &str) -> Result<(), CoreError> {
    if password.is_empty() || password.as_bytes().contains(&0) {
        return Err(CoreError::InvalidCredentials);
    }
    let mut credentials = Credentials {
        username: CString::new(username).map_err(|_| CoreError::InvalidCredentials)?,
        password: password.as_bytes().to_vec(),
        password_sent: false,
    };
    credentials.password.push(0);
    // SAFETY: ABI declarations above match Linux-PAM's public pam_appl.h and
    // _pam_types.h. The library outlives all function pointers and PAM handles.
    unsafe {
        let library = Library(libc::dlopen(
            c"libpam.so.0".as_ptr(),
            libc::RTLD_NOW | libc::RTLD_LOCAL,
        ));
        if library.0.is_null() {
            // Do not drop a null dlopen handle.
            std::mem::forget(library);
            return Err(system_error(
                "Linux PAM is missing. Install libpam0g (Debian/Ubuntu) or pam (Fedora/Arch).",
            ));
        }
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let pointer = libc::dlsym(library.0, $name.as_ptr());
                if pointer.is_null() {
                    return Err(system_error(
                        "Linux PAM library is missing a required symbol",
                    ));
                }
                std::mem::transmute::<*mut c_void, $ty>(pointer)
            }};
        }
        let start = symbol!(c"pam_start", Start);
        let auth = symbol!(c"pam_authenticate", Check);
        let account = symbol!(c"pam_acct_mgmt", Check);
        let end = symbol!(c"pam_end", End);
        let get_item = symbol!(c"pam_get_item", GetItem);
        let set_item = symbol!(c"pam_set_item", SetItem);
        let conversation = Conversation {
            callback: converse,
            data: (&mut credentials as *mut Credentials).cast(),
        };
        let service = if std::path::Path::new("/etc/pam.d/tundraux3").is_file() {
            c"tundraux3"
        } else {
            c"login"
        };
        let mut handle = ptr::null_mut();
        let status = start(
            service.as_ptr(),
            credentials.username.as_ptr(),
            &conversation,
            &mut handle,
        );
        check_status(status)?;
        if handle.is_null() {
            return Err(system_error("PAM returned an empty handle"));
        }
        let mut last_status = SUCCESS;
        let result = (|| {
            let mut tty = [0 as c_char; 512];
            if libc::ttyname_r(libc::STDIN_FILENO, tty.as_mut_ptr(), tty.len()) == 0 {
                last_status = set_item(handle, 3, tty.as_ptr().cast()); // PAM_TTY
                check_status(last_status)?;
            }
            last_status = auth(handle, DISALLOW_NULL_AUTHTOK);
            check_status(last_status)?;
            last_status = account(handle, 0);
            check_status(last_status)?;
            let mut actual_user = ptr::null();
            last_status = get_item(handle, 2, &mut actual_user); // PAM_USER
            check_status(last_status)?;
            if actual_user.is_null()
                || CStr::from_ptr(actual_user.cast()) != credentials.username.as_c_str()
            {
                last_status = 7;
                return Err(CoreError::InvalidCredentials);
            }
            Ok(())
        })();
        // Always finish a successfully started transaction, including failures.
        let end_status = end(handle, last_status);
        result?;
        check_status(end_status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_handles_batched_prompts_and_c_owned_responses() {
        let mut credentials = Credentials {
            username: CString::new("alice").unwrap(),
            password: b"secret\0".to_vec(),
            password_sent: false,
        };
        let messages = [
            Message {
                style: 4,
                text: ptr::null(),
            },
            Message {
                style: 2,
                text: ptr::null(),
            },
            Message {
                style: 1,
                text: ptr::null(),
            },
        ];
        let pointers = messages.each_ref().map(|message| message as *const Message);
        let mut output = ptr::null_mut();
        unsafe {
            assert_eq!(
                converse(
                    3,
                    pointers.as_ptr(),
                    &mut output,
                    (&mut credentials as *mut Credentials).cast()
                ),
                SUCCESS
            );
            assert!((*output).text.is_null());
            assert_eq!(CStr::from_ptr((*output.add(1)).text).to_bytes(), b"alice");
            assert_eq!(CStr::from_ptr((*output.add(2)).text).to_bytes(), b"secret");
            free_responses(output, 3);
            assert_eq!(
                converse(
                    1,
                    pointers[2..].as_ptr(),
                    &mut output,
                    (&mut credentials as *mut Credentials).cast()
                ),
                CONV_ERR
            );
            assert!(output.is_null());
        }
    }

    #[test]
    fn unsupported_and_invalid_conversations_fail_closed() {
        let mut credentials = Credentials {
            username: CString::new("alice").unwrap(),
            password: b"secret\0".to_vec(),
            password_sent: false,
        };
        let messages = [
            Message {
                style: 1,
                text: ptr::null(),
            },
            Message {
                style: 7,
                text: ptr::null(),
            },
        ];
        let pointers = messages.each_ref().map(|message| message as *const Message);
        let mut output = ptr::null_mut();
        unsafe {
            assert_eq!(
                converse(
                    2,
                    pointers.as_ptr(),
                    &mut output,
                    (&mut credentials as *mut Credentials).cast()
                ),
                CONV_ERR
            );
            assert!(output.is_null());
            assert_eq!(
                converse(0, ptr::null(), &mut output, ptr::null_mut()),
                CONV_ERR
            );
        }
    }

    #[test]
    fn pam_denials_and_expired_passwords_never_succeed() {
        for status in 1..=31 {
            assert!(check_status(status).is_err());
        }
        assert!(check_status(SUCCESS).is_ok());
    }
}
