use std::ffi::{OsStr, OsString, c_void};
use std::fs;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::ptr;

use crate::{
    AppPaths, FileAttributes, Platform, PlatformCapabilities, PlatformError, PlatformKind,
    ProcessExit, ProcessSpec, UserDirs, build_windows_app_paths,
};

const SW_SHOWNORMAL: i32 = 1;
const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x0002;
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x0002;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x0004;
const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x0020;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000000C;

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Windows
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::native_supported()
    }

    fn user_dirs(&self) -> Result<UserDirs, PlatformError> {
        UserDirs::new(
            known_folder_path(&FOLDERID_DESKTOP)?,
            known_folder_path(&FOLDERID_DOCUMENTS)?,
            known_folder_path(&FOLDERID_DOWNLOADS)?,
            known_folder_path(&FOLDERID_PICTURES)?,
            known_folder_path(&FOLDERID_VIDEOS)?,
            known_folder_path(&FOLDERID_MUSIC)?,
            known_folder_path(&FOLDERID_ROAMING_APP_DATA)?,
        )
        .map_err(Into::into)
    }

    fn app_paths(&self) -> Result<AppPaths, PlatformError> {
        build_windows_app_paths(
            known_folder_path(&FOLDERID_ROAMING_APP_DATA)?,
            known_folder_path(&FOLDERID_LOCAL_APP_DATA)?,
            std::env::temp_dir(),
        )
        .map_err(Into::into)
    }

    fn open_path(&self, path: &Path) -> Result<(), PlatformError> {
        shell_execute("open", path.as_os_str(), None)
    }

    fn open_with(&self, path: &Path, application: &Path) -> Result<(), PlatformError> {
        shell_execute(
            "open",
            application.as_os_str(),
            Some(&quote_windows_argument(path.as_os_str())),
        )
    }

    fn open_uri(&self, uri: &str) -> Result<(), PlatformError> {
        if uri.trim().is_empty() {
            return Err(PlatformError::InvalidInput {
                message: "URI must not be empty".to_string(),
            });
        }

        shell_execute("open", OsStr::new(uri), None)
    }

    fn spawn_detached(&self, spec: &ProcessSpec) -> Result<(), PlatformError> {
        crate::process::spawn_detached_impl(spec, true)
    }

    fn spawn_wait(&self, spec: &ProcessSpec) -> Result<ProcessExit, PlatformError> {
        crate::process::spawn_wait_impl(spec, true)
    }

    fn read_clipboard_text(&self) -> Result<String, PlatformError> {
        let _guard = ClipboardGuard::open()?;
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if handle.is_null() {
            return Err(PlatformError::Native {
                operation: "read clipboard",
                message: "CF_UNICODETEXT is unavailable".to_string(),
            });
        }

        let locked = unsafe { GlobalLock(handle) } as *const u16;
        if locked.is_null() {
            return Err(PlatformError::Native {
                operation: "read clipboard",
                message: "GlobalLock returned null".to_string(),
            });
        }

        let mut len = 0usize;
        unsafe {
            while *locked.add(len) != 0 {
                len += 1;
            }
        }

        let text = unsafe {
            let slice = std::slice::from_raw_parts(locked, len);
            String::from_utf16_lossy(slice)
        };
        unsafe {
            GlobalUnlock(handle);
        }

        Ok(text)
    }

    fn write_clipboard_text(&self, text: &str) -> Result<(), PlatformError> {
        let _guard = ClipboardGuard::open()?;
        if unsafe { EmptyClipboard() } == 0 {
            return Err(PlatformError::Native {
                operation: "write clipboard",
                message: "EmptyClipboard failed".to_string(),
            });
        }

        let mut encoded: Vec<u16> = text.encode_utf16().collect();
        encoded.push(0);
        let byte_len = encoded.len() * std::mem::size_of::<u16>();
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) };
        if handle.is_null() {
            return Err(PlatformError::Native {
                operation: "write clipboard",
                message: "GlobalAlloc failed".to_string(),
            });
        }

        let locked = unsafe { GlobalLock(handle) } as *mut u16;
        if locked.is_null() {
            unsafe {
                GlobalFree(handle);
            }
            return Err(PlatformError::Native {
                operation: "write clipboard",
                message: "GlobalLock returned null".to_string(),
            });
        }

        unsafe {
            ptr::copy_nonoverlapping(encoded.as_ptr(), locked, encoded.len());
            GlobalUnlock(handle);
        }

        if unsafe { SetClipboardData(CF_UNICODETEXT, handle) }.is_null() {
            unsafe {
                GlobalFree(handle);
            }
            return Err(PlatformError::Native {
                operation: "write clipboard",
                message: "SetClipboardData failed".to_string(),
            });
        }

        Ok(())
    }

    fn file_attributes(&self, path: &Path) -> Result<FileAttributes, PlatformError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| PlatformError::Io {
            operation: "read file attributes",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        })?;
        let file_attributes = metadata.file_attributes();
        let reparse_tag = reparse_tag(path);
        let mut attributes = crate::default_file_attributes(path)?;

        attributes.hidden = file_attributes & FILE_ATTRIBUTE_HIDDEN != 0;
        attributes.system = file_attributes & FILE_ATTRIBUTE_SYSTEM != 0;
        attributes.archive = file_attributes & FILE_ATTRIBUTE_ARCHIVE != 0;
        attributes.reparse_point = file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
        attributes.symlink = attributes.symlink || reparse_tag == Some(IO_REPARSE_TAG_SYMLINK);
        attributes.junction = reparse_tag == Some(IO_REPARSE_TAG_MOUNT_POINT);
        attributes.shortcut = is_shortcut(path);

        Ok(attributes)
    }

    fn external_open_policy(
        &self,
        path: &Path,
        attributes: &FileAttributes,
    ) -> crate::ExternalOpenPolicy {
        crate::platform::windows_external_open_policy(path, attributes)
    }
}

pub fn current_windows_build() -> Result<u32, String> {
    let mut version: RtlOsVersionInfoW = unsafe { std::mem::zeroed() };
    version.dw_os_version_info_size = std::mem::size_of::<RtlOsVersionInfoW>() as u32;

    let status = unsafe { RtlGetVersion(&mut version) };
    if status >= 0 {
        Ok(version.dw_build_number)
    } else {
        Err(format!("RtlGetVersion failed with NTSTATUS {status}"))
    }
}

fn shell_execute(
    operation: &str,
    file: &OsStr,
    parameters: Option<&str>,
) -> Result<(), PlatformError> {
    let operation = to_wide(OsStr::new(operation));
    let file = to_wide(file);
    let parameters = parameters.map(|parameters| to_wide(OsStr::new(parameters)));

    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            parameters
                .as_ref()
                .map(|value| value.as_ptr())
                .unwrap_or(ptr::null()),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    if result as isize > 32 {
        Ok(())
    } else {
        Err(PlatformError::Native {
            operation: "ShellExecuteW",
            message: format!("ShellExecuteW returned {result:?}"),
        })
    }
}

fn known_folder_path(folder_id: &Guid) -> Result<PathBuf, PlatformError> {
    let mut raw_path: *mut u16 = ptr::null_mut();
    let status = unsafe {
        SHGetKnownFolderPath(
            folder_id as *const Guid,
            0,
            ptr::null_mut(),
            &mut raw_path as *mut *mut u16,
        )
    };

    if status < 0 {
        return Err(PlatformError::Native {
            operation: "SHGetKnownFolderPath",
            message: format!("HRESULT {status:#x}"),
        });
    }

    if raw_path.is_null() {
        return Err(PlatformError::Native {
            operation: "SHGetKnownFolderPath",
            message: "returned null path".to_string(),
        });
    }

    let mut len = 0usize;
    unsafe {
        while *raw_path.add(len) != 0 {
            len += 1;
        }
    }

    let path = unsafe {
        let slice = std::slice::from_raw_parts(raw_path, len);
        PathBuf::from(OsString::from_wide(slice))
    };
    unsafe {
        CoTaskMemFree(raw_path.cast());
    }

    Ok(path)
}

fn to_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn quote_windows_argument(value: &OsStr) -> String {
    let text = value.to_string_lossy();
    format!("\"{}\"", text.replace('"', "\\\""))
}

fn reparse_tag(path: &Path) -> Option<u32> {
    let path = to_wide(path.as_os_str());
    let mut data: Win32FindDataW = unsafe { std::mem::zeroed() };
    let handle = unsafe { FindFirstFileW(path.as_ptr(), &mut data) };
    if handle as isize == -1 {
        return None;
    }

    unsafe {
        FindClose(handle);
    }

    if data.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Some(data.reserved0)
    } else {
        None
    }
}

fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
}

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<Self, PlatformError> {
        if unsafe { OpenClipboard(ptr::null_mut()) } == 0 {
            Err(PlatformError::Native {
                operation: "open clipboard",
                message: "OpenClipboard failed".to_string(),
            })
        } else {
            Ok(Self)
        }
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

#[repr(C)]
struct RtlOsVersionInfoW {
    dw_os_version_info_size: u32,
    dw_major_version: u32,
    dw_minor_version: u32,
    dw_build_number: u32,
    dw_platform_id: u32,
    sz_csd_version: [u16; 128],
}

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
struct FileTime {
    low_date_time: u32,
    high_date_time: u32,
}

#[repr(C)]
struct Win32FindDataW {
    file_attributes: u32,
    creation_time: FileTime,
    last_access_time: FileTime,
    last_write_time: FileTime,
    file_size_high: u32,
    file_size_low: u32,
    reserved0: u32,
    reserved1: u32,
    file_name: [u16; 260],
    alternate_file_name: [u16; 14],
}

const FOLDERID_DESKTOP: Guid = Guid {
    data1: 0xB4BFCC3A,
    data2: 0xDB2C,
    data3: 0x424C,
    data4: [0xB0, 0x29, 0x7F, 0xE9, 0x9A, 0x87, 0xC6, 0x41],
};
const FOLDERID_DOCUMENTS: Guid = Guid {
    data1: 0xFDD39AD0,
    data2: 0x238F,
    data3: 0x46AF,
    data4: [0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7],
};
const FOLDERID_DOWNLOADS: Guid = Guid {
    data1: 0x374DE290,
    data2: 0x123F,
    data3: 0x4565,
    data4: [0x91, 0x64, 0x39, 0xC4, 0x92, 0x5E, 0x46, 0x7B],
};
const FOLDERID_PICTURES: Guid = Guid {
    data1: 0x33E28130,
    data2: 0x4E1E,
    data3: 0x4676,
    data4: [0x83, 0x5A, 0x98, 0x39, 0x5C, 0x3B, 0xC3, 0xBB],
};
const FOLDERID_VIDEOS: Guid = Guid {
    data1: 0x18989B1D,
    data2: 0x99B5,
    data3: 0x455B,
    data4: [0x84, 0x1C, 0xAB, 0x7C, 0x74, 0xE4, 0xDD, 0xFC],
};
const FOLDERID_MUSIC: Guid = Guid {
    data1: 0x4BD8D571,
    data2: 0x6D19,
    data3: 0x48D3,
    data4: [0xBE, 0x97, 0x42, 0x22, 0x20, 0x08, 0x0E, 0x43],
};
const FOLDERID_ROAMING_APP_DATA: Guid = Guid {
    data1: 0x3EB685DB,
    data2: 0x65F9,
    data3: 0x4CF6,
    data4: [0xA0, 0x3A, 0xE3, 0xEF, 0x65, 0x72, 0x9F, 0x3D],
};
const FOLDERID_LOCAL_APP_DATA: Guid = Guid {
    data1: 0xF1B32785,
    data2: 0x6FBA,
    data3: 0x4FCF,
    data4: [0x9D, 0x55, 0x7B, 0x8E, 0x7F, 0x15, 0x70, 0x91],
};

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlGetVersion(version_information: *mut RtlOsVersionInfoW) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: *mut c_void,
        lp_operation: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show_cmd: i32,
    ) -> *mut c_void;

    fn SHGetKnownFolderPath(
        rfid: *const Guid,
        dw_flags: u32,
        h_token: *mut c_void,
        ppsz_path: *mut *mut u16,
    ) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(pv: *mut c_void);
}

#[link(name = "user32")]
unsafe extern "system" {
    fn OpenClipboard(h_wnd_new_owner: *mut c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(u_format: u32) -> *mut c_void;
    fn SetClipboardData(u_format: u32, h_mem: *mut c_void) -> *mut c_void;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn FindFirstFileW(
        lp_file_name: *const u16,
        lp_find_file_data: *mut Win32FindDataW,
    ) -> *mut c_void;
    fn FindClose(h_find_file: *mut c_void) -> i32;
    fn GlobalAlloc(u_flags: u32, dw_bytes: usize) -> *mut c_void;
    fn GlobalLock(h_mem: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(h_mem: *mut c_void) -> i32;
    fn GlobalFree(h_mem: *mut c_void) -> *mut c_void;
}
