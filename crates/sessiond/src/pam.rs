//! One PAM transaction per dedicated worker process. No handle crosses fork/exec.
use session_protocol::SystemUser;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    io, ptr,
};
const SUCCESS: c_int = 0;
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
type Check = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type Start = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *const Conversation,
    *mut *mut c_void,
) -> c_int;
type SetItem = unsafe extern "C" fn(*mut c_void, c_int, *const c_void) -> c_int;
type GetItem = unsafe extern "C" fn(*const c_void, c_int, *mut *const c_void) -> c_int;
type PutEnv = unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int;
type GetEnvList = unsafe extern "C" fn(*mut c_void) -> *mut *mut c_char;
struct Api {
    library: *mut c_void,
    start: Start,
    auth: Check,
    account: Check,
    change: Check,
    cred: Check,
    open: Check,
    close: Check,
    end: Check,
    set: SetItem,
    get: GetItem,
    put: PutEnv,
    env: GetEnvList,
}
impl Api {
    fn load() -> io::Result<Self> {
        unsafe {
            let library = libc::dlopen(c"libpam.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if library.is_null() {
                return Err(io::Error::other("libpam.so.0 unavailable"));
            }
            macro_rules! sym {
                ($n:literal,$t:ty) => {{
                    let p = libc::dlsym(library, $n.as_ptr());
                    if p.is_null() {
                        libc::dlclose(library);
                        return Err(io::Error::other("required PAM symbol missing"));
                    }
                    std::mem::transmute::<*mut c_void, $t>(p)
                }};
            }
            Ok(Self {
                library,
                start: sym!(c"pam_start", Start),
                auth: sym!(c"pam_authenticate", Check),
                account: sym!(c"pam_acct_mgmt", Check),
                change: sym!(c"pam_chauthtok", Check),
                cred: sym!(c"pam_setcred", Check),
                open: sym!(c"pam_open_session", Check),
                close: sym!(c"pam_close_session", Check),
                end: sym!(c"pam_end", Check),
                set: sym!(c"pam_set_item", SetItem),
                get: sym!(c"pam_get_item", GetItem),
                put: sym!(c"pam_putenv", PutEnv),
                env: sym!(c"pam_getenvlist", GetEnvList),
            })
        }
    }
}
impl Drop for Api {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.library);
        }
    }
}
type Prompt = Box<dyn FnMut(i32, &str) -> io::Result<String>>;
unsafe fn release(responses: *mut Response, count: usize) {
    unsafe {
        for n in 0..count {
            let s = (*responses.add(n)).text;
            if !s.is_null() {
                for i in 0..libc::strlen(s) {
                    ptr::write_volatile(s.add(i), 0);
                }
                libc::free(s.cast());
            }
        }
        libc::free(responses.cast());
    }
}
unsafe extern "C" fn converse(
    count: c_int,
    messages: *const *const Message,
    out: *mut *mut Response,
    data: *mut c_void,
) -> c_int {
    if out.is_null() {
        return 19;
    }
    unsafe {
        *out = ptr::null_mut();
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        if !(1..=32).contains(&count) || messages.is_null() || data.is_null() {
            return 19;
        }
        let prompt = &mut *data.cast::<Prompt>();
        let responses = libc::calloc(count as usize, size_of::<Response>()).cast::<Response>();
        if responses.is_null() {
            return 19;
        }
        for n in 0..count as usize {
            let msg = *messages.add(n);
            if msg.is_null() || !matches!((*msg).style, 1..=4) {
                release(responses, count as usize);
                return 19;
            }
            let text = if (*msg).text.is_null() {
                "".into()
            } else {
                CStr::from_ptr((*msg).text).to_string_lossy()
            };
            let mut reply = match prompt((*msg).style, &text) {
                Ok(r) => zeroize::Zeroizing::new(r),
                Err(_) => {
                    release(responses, count as usize);
                    return 19;
                }
            };
            if reply.len() > 16384 || reply.as_bytes().contains(&0) {
                release(responses, count as usize);
                return 19;
            }
            if (*msg).style <= 2 {
                let p = libc::calloc(reply.len() + 1, 1).cast::<c_char>();
                if p.is_null() {
                    release(responses, count as usize);
                    return 19;
                }
                ptr::copy_nonoverlapping(reply.as_ptr(), p.cast(), reply.len());
                (*responses.add(n)).text = p;
            }
            for byte in reply.as_bytes_mut() {
                ptr::write_volatile(byte, 0);
            }
        }
        *out = responses;
        SUCCESS
    }));
    result.unwrap_or(19)
}
fn check(code: c_int) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "PAM rejected operation (status {code})"
        )))
    }
}
pub struct Pam {
    api: Api,
    handle: *mut c_void,
    _prompt: Box<Prompt>,
    credentials: bool,
    opened: bool,
    last: c_int,
}
impl Pam {
    pub fn start(service: &str, user: &SystemUser, tty: u32, prompt: Prompt) -> io::Result<Self> {
        let api = Api::load()?;
        let mut prompt = Box::new(prompt);
        let conv = Conversation {
            callback: converse,
            data: (&mut *prompt as *mut Prompt).cast(),
        };
        let service = CString::new(service)?;
        let username = CString::new(user.username.as_str())?;
        let mut handle = ptr::null_mut();
        check(unsafe { (api.start)(service.as_ptr(), username.as_ptr(), &conv, &mut handle) })?;
        if handle.is_null() {
            return Err(io::Error::other("PAM returned null handle"));
        }
        let mut pam = Self {
            api,
            handle,
            _prompt: prompt,
            credentials: false,
            opened: false,
            last: 0,
        };
        let tty = CString::new(format!("tty{tty}"))?;
        pam.last = unsafe { (pam.api.set)(handle, 3, tty.as_ptr().cast()) };
        check(pam.last)?;
        for entry in [
            "XDG_SEAT=seat0".to_owned(),
            format!(
                "XDG_VTNR={}",
                tty.to_string_lossy().trim_start_matches("tty")
            ),
            "XDG_SESSION_TYPE=tty".into(),
            "XDG_SESSION_CLASS=user".into(),
        ] {
            pam.put(&entry)?;
        }
        Ok(pam)
    }
    pub fn put(&mut self, entry: &str) -> io::Result<()> {
        let entry = CString::new(entry)?;
        self.last = unsafe { (self.api.put)(self.handle, entry.as_ptr()) };
        check(self.last)
    }
    pub fn authenticate(&mut self, user: &SystemUser) -> io::Result<()> {
        self.last = unsafe { (self.api.auth)(self.handle, 1) };
        check(self.last)?;
        self.last = unsafe { (self.api.account)(self.handle, 0) };
        if self.last == 12 {
            self.last = unsafe { (self.api.change)(self.handle, 0x20) };
            check(self.last)?;
            self.last = unsafe { (self.api.account)(self.handle, 0) };
        }
        check(self.last)?;
        let mut actual = ptr::null();
        self.last = unsafe { (self.api.get)(self.handle, 2, &mut actual) };
        check(self.last)?;
        if actual.is_null()
            || unsafe { CStr::from_ptr(actual.cast()) }.to_bytes() != user.username.as_bytes()
        {
            return Err(io::Error::other("PAM changed selected user"));
        }
        Ok(())
    }
    pub fn open(&mut self, user: &SystemUser) -> io::Result<()> {
        let name = CString::new(user.username.as_str())?;
        if unsafe { libc::initgroups(name.as_ptr(), user.gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        self.last = unsafe { (self.api.cred)(self.handle, 0x2) };
        check(self.last)?;
        self.credentials = true;
        self.last = unsafe { (self.api.open)(self.handle, 0) };
        check(self.last)?;
        self.opened = true;
        Ok(())
    }
    pub fn environment(&self) -> Vec<(String, String)> {
        unsafe {
            let list = (self.api.env)(self.handle);
            if list.is_null() {
                return vec![];
            }
            let mut result = vec![];
            let mut n = 0;
            while !(*list.add(n)).is_null() {
                let p = *list.add(n);
                let text = CStr::from_ptr(p).to_string_lossy();
                if let Some((k, v)) = text.split_once('=') {
                    result.push((k.into(), v.into()));
                }
                libc::free(p.cast());
                n += 1;
            }
            libc::free(list.cast());
            result
        }
    }
    pub fn close(&mut self) -> io::Result<()> {
        let mut failure = 0;
        if self.opened {
            let s = unsafe { (self.api.close)(self.handle, 0) };
            self.opened = false;
            if s != 0 {
                failure = s;
            }
        }
        if self.credentials {
            let s = unsafe { (self.api.cred)(self.handle, 0x4) };
            self.credentials = false;
            if s != 0 {
                failure = s;
            }
        }
        check(failure)
    }
}
impl Drop for Pam {
    fn drop(&mut self) {
        if let Err(e) = self.close() {
            eprintln!("PAM cleanup: {e}");
        }
        unsafe {
            (self.api.end)(self.handle, self.last);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_secret_prompt_has_an_independent_response() {
        let mut sequence = 0;
        let mut prompt: Prompt = Box::new(move |style, _| {
            assert_eq!(style, 1);
            sequence += 1;
            Ok(format!("secret-{sequence}"))
        });
        let first = Message {
            style: 1,
            text: c"Password".as_ptr(),
        };
        let second = Message {
            style: 1,
            text: c"OTP".as_ptr(),
        };
        let messages = [&first as *const Message, &second as *const Message];
        let mut responses = ptr::null_mut();
        unsafe {
            assert_eq!(
                converse(
                    2,
                    messages.as_ptr(),
                    &mut responses,
                    (&mut prompt as *mut Prompt).cast()
                ),
                0
            );
            assert_eq!(CStr::from_ptr((*responses).text).to_bytes(), b"secret-1");
            assert_eq!(
                CStr::from_ptr((*responses.add(1)).text).to_bytes(),
                b"secret-2"
            );
            release(responses, 2);
        }
    }
    #[test]
    fn cancelled_conversation_returns_no_partial_credentials() {
        let mut prompt: Prompt = Box::new(|_, _| Err(io::Error::other("cancelled")));
        let message = Message {
            style: 1,
            text: c"Password".as_ptr(),
        };
        let messages = [&message as *const Message];
        let mut responses = ptr::null_mut();
        unsafe {
            assert_eq!(
                converse(
                    1,
                    messages.as_ptr(),
                    &mut responses,
                    (&mut prompt as *mut Prompt).cast()
                ),
                19
            );
        }
        assert!(responses.is_null());
    }
    #[test]
    fn all_pam_failures_remain_failures() {
        for status in 1..32 {
            assert!(check(status).is_err())
        }
        assert!(check(0).is_ok());
    }
}
