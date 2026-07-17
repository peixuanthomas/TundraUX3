use std::ffi::{OsStr, OsString, c_void};
use std::fs;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{
    AppPaths, DirectoryEntryMetadata, DirectoryListing, DirectoryListingWarning, FileAttributes,
    FileOpenPolicy, LocalVolume, Platform, PlatformCapabilities, PlatformError, PlatformKind,
    ProcessExit, ProcessSpec, TrashEntry, TrashEntryId, TrashRestoreTarget, TrashStats, UserDirs,
    VolumeKind, build_windows_app_paths,
};

const SW_SHOWNORMAL: i32 = 1;
const MB_OK: u32 = 0x0000_0000;
const MB_ICONERROR: u32 = 0x0000_0010;
const MB_TASKMODAL: u32 = 0x0000_2000;
const MB_SETFOREGROUND: u32 = 0x0001_0000;
const MB_TOPMOST: u32 = 0x0004_0000;
const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x0002;
const FILE_ATTRIBUTE_READONLY: u32 = 0x0001;
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x0002;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x0004;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x0020;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000000C;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NO_MORE_FILES: u32 = 18;
const ERROR_INVALID_PARAMETER: u32 = 87;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x0010_0000;
const WAIT_OBJECT_0: u32 = 0x0000_0000;
const WAIT_TIMEOUT: u32 = 0x0000_0102;
const WAIT_FAILED: u32 = 0xffff_ffff;
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const S_FALSE: i32 = 1;
const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
const COINIT_APARTMENTTHREADED: u32 = 0x2;
const CLSCTX_INPROC_SERVER: u32 = 0x1;
const SIGDN_NORMALDISPLAY: u32 = 0;
const SIGDN_DESKTOPABSOLUTEPARSING: u32 = 0x8002_8000;
const SFGAO_FOLDER: u32 = 0x2000_0000;
const FOF_SILENT: u32 = 0x0004;
const FOF_NOCONFIRMATION: u32 = 0x0010;
const FOF_ALLOWUNDO: u32 = 0x0040;
const FOF_NOERRORUI: u32 = 0x0400;
const FOFX_RECYCLEONDELETE: u32 = 0x0008_0000;
const FOFX_EARLYFAILURE: u32 = 0x0010_0000;
const FILE_OPERATION_FLAGS: u32 =
    FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOFX_EARLYFAILURE;
const RECYCLE_OPERATION_FLAGS: u32 = FILE_OPERATION_FLAGS | FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE;
const SHERB_NOCONFIRMATION: u32 = 0x1;
const SHERB_NOPROGRESSUI: u32 = 0x2;
const SHERB_NOSOUND: u32 = 0x4;

#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Windows
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::native_supported()
    }

    fn is_native_backend(&self) -> bool {
        true
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

    fn show_critical_error(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        windows_show_critical_error(title, body)
    }

    fn is_process_alive(&self, pid: u32) -> Result<bool, PlatformError> {
        windows_is_process_alive(pid)
    }

    fn local_volumes(&self) -> Result<Vec<LocalVolume>, PlatformError> {
        windows_local_volumes()
    }

    fn list_trash(&self) -> Result<Vec<TrashEntry>, PlatformError> {
        windows_list_trash()
    }

    fn trash_stats(&self) -> Result<TrashStats, PlatformError> {
        windows_trash_stats()
    }

    fn move_to_trash(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        windows_move_to_trash(paths)
    }

    fn empty_trash(&self) -> Result<(), PlatformError> {
        windows_empty_trash()
    }

    fn restore_trash_item(
        &self,
        id: &TrashEntryId,
        target: TrashRestoreTarget,
    ) -> Result<PathBuf, PlatformError> {
        windows_restore_trash_item(id, target)
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

    fn read_directory(&self, path: &Path) -> Result<DirectoryListing, PlatformError> {
        windows_read_directory(path)
    }

    fn file_open_policy(&self, path: &Path, attributes: &FileAttributes) -> FileOpenPolicy {
        crate::default_file_open_policy(PlatformKind::Windows, path, attributes)
    }

    fn external_open_policy(
        &self,
        path: &Path,
        attributes: &FileAttributes,
    ) -> crate::ExternalOpenPolicy {
        crate::platform::windows_external_open_policy(path, attributes)
    }
}

fn windows_show_critical_error(title: &str, body: &str) -> Result<(), PlatformError> {
    if title.contains('\0') || body.contains('\0') {
        return Err(PlatformError::InvalidInput {
            message: "critical error title and body must not contain NUL characters".to_string(),
        });
    }

    let title = to_wide(OsStr::new(title));
    let body = to_wide(OsStr::new(body));
    let result = unsafe {
        MessageBoxW(
            ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_TASKMODAL | MB_SETFOREGROUND | MB_TOPMOST,
        )
    };
    if result == 0 {
        Err(last_windows_error("MessageBoxW", None))
    } else {
        Ok(())
    }
}

fn windows_is_process_alive(pid: u32) -> Result<bool, PlatformError> {
    if pid == 0 {
        return Ok(false);
    }

    let handle = unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        let code = unsafe { GetLastError() };
        return match code {
            ERROR_INVALID_PARAMETER => Ok(false),
            // Windows reports access denied only after resolving an existing
            // protected process, which is sufficient for a liveness probe.
            ERROR_ACCESS_DENIED => Ok(true),
            _ => Err(windows_error_from_code("OpenProcess", code)),
        };
    }

    let wait_result = unsafe { WaitForSingleObject(handle, 0) };
    let wait_error = (wait_result == WAIT_FAILED).then(|| unsafe { GetLastError() });
    unsafe {
        CloseHandle(handle);
    }

    match wait_result {
        WAIT_TIMEOUT => Ok(true),
        WAIT_OBJECT_0 => Ok(false),
        WAIT_FAILED => Err(windows_error_from_code(
            "WaitForSingleObject",
            wait_error.unwrap_or_default(),
        )),
        result => Err(PlatformError::Native {
            operation: "WaitForSingleObject",
            message: format!("unexpected wait result {result:#010x}"),
        }),
    }
}

fn windows_local_volumes() -> Result<Vec<LocalVolume>, PlatformError> {
    let required = unsafe { GetLogicalDriveStringsW(0, ptr::null_mut()) };
    if required == 0 {
        return Err(last_windows_error("GetLogicalDriveStringsW", None));
    }
    let mut buffer = vec![0u16; required as usize + 1];
    let written = unsafe { GetLogicalDriveStringsW(buffer.len() as u32, buffer.as_mut_ptr()) };
    if written == 0 || written as usize >= buffer.len() {
        return Err(last_windows_error("GetLogicalDriveStringsW", None));
    }

    let mut volumes = Vec::new();
    let mut start = 0usize;
    while start < written as usize && buffer[start] != 0 {
        let relative_end = buffer[start..]
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len() - start);
        let end = start + relative_end;
        let root = PathBuf::from(OsString::from_wide(&buffer[start..end]));
        let root_wide = to_wide(root.as_os_str());
        let kind = match unsafe { GetDriveTypeW(root_wide.as_ptr()) } {
            DRIVE_FIXED => VolumeKind::Fixed,
            DRIVE_REMOVABLE => VolumeKind::Removable,
            _ => {
                start = end + 1;
                continue;
            }
        };

        let mut label_buffer = [0u16; 261];
        let label = if unsafe {
            GetVolumeInformationW(
                root_wide.as_ptr(),
                label_buffer.as_mut_ptr(),
                label_buffer.len() as u32,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            )
        } != 0
        {
            let value = os_string_from_null_terminated(&label_buffer)
                .to_string_lossy()
                .into_owned();
            (!value.is_empty()).then_some(value)
        } else {
            None
        };

        let mut available = 0u64;
        let mut total = 0u64;
        let mut free = 0u64;
        let has_space = unsafe {
            GetDiskFreeSpaceExW(root_wide.as_ptr(), &mut available, &mut total, &mut free)
        } != 0;
        volumes.push(LocalVolume {
            root,
            label,
            kind,
            total_bytes: has_space.then_some(total),
            available_bytes: has_space.then_some(available),
        });
        start = end + 1;
    }
    volumes.sort_by(|left, right| left.root.cmp(&right.root));
    Ok(volumes)
}

fn windows_list_trash() -> Result<Vec<TrashEntry>, PlatformError> {
    let _apartment = ComApartment::enter()?;
    let mut entries = enumerate_recycle_bin_items()?
        .iter()
        .map(trash_entry_from_shell_item)
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(entries)
}

fn windows_trash_stats() -> Result<TrashStats, PlatformError> {
    let mut info = ShQueryRecycleBinInfo {
        cb_size: std::mem::size_of::<ShQueryRecycleBinInfo>() as u32,
        size: 0,
        item_count: 0,
    };
    let status = unsafe { SHQueryRecycleBinW(ptr::null(), &mut info) };
    check_hresult(status, "SHQueryRecycleBinW")?;
    Ok(TrashStats {
        item_count: info.item_count.max(0) as u64,
        total_bytes: info.size.max(0) as u64,
    })
}

fn windows_move_to_trash(paths: &[PathBuf]) -> Result<(), PlatformError> {
    if paths.is_empty() {
        return Err(PlatformError::InvalidInput {
            message: "at least one path is required to move to the Recycle Bin".to_string(),
        });
    }
    for path in paths {
        validate_recycle_source(path)?;
    }

    let _apartment = ComApartment::enter()?;
    let sources = paths
        .iter()
        .map(|path| shell_item_from_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    let operation = create_file_operation()?;
    // DeleteItem plus FOFX_RECYCLEONDELETE is the documented IFileOperation
    // contract for a recycle-only delete. Moving an item to the virtual
    // Recycle Bin folder is not equivalent and is rejected by some Shell
    // namespace implementations. There is deliberately no permanent-delete
    // fallback if the volume cannot recycle an item.
    file_operation_set_flags(&operation, RECYCLE_OPERATION_FLAGS)?;
    let table = unsafe { operation.vtable::<FileOperationVTable>() };
    for source in &sources {
        let status = unsafe { (table.delete_item)(operation.raw(), source.raw(), ptr::null_mut()) };
        check_hresult(status, "IFileOperation::DeleteItem(Recycle Bin)")?;
    }
    perform_file_operation(&operation, "move items to Recycle Bin")
}

fn windows_empty_trash() -> Result<(), PlatformError> {
    let status = unsafe {
        SHEmptyRecycleBinW(
            ptr::null_mut(),
            ptr::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    };
    check_hresult(status, "SHEmptyRecycleBinW")
}

fn windows_restore_trash_item(
    id: &TrashEntryId,
    target: TrashRestoreTarget,
) -> Result<PathBuf, PlatformError> {
    let _apartment = ComApartment::enter()?;
    let mut found = None;
    for item in enumerate_recycle_bin_items()? {
        if shell_item_id(&item)? == *id {
            found = Some(item);
            break;
        }
    }
    let item = found.ok_or_else(|| PlatformError::InvalidInput {
        message: "Recycle Bin item no longer exists".to_string(),
    })?;
    let destination = match target {
        TrashRestoreTarget::OriginalLocation => {
            original_path_from_shell_item(&item)?.ok_or_else(|| PlatformError::InvalidInput {
                message: "Recycle Bin item has no original path".to_string(),
            })?
        }
        TrashRestoreTarget::DestinationPath(path) => path,
    };
    validate_restore_destination(&destination)?;
    let parent_path = destination.parent().expect("validated destination parent");
    let destination_folder = shell_item_from_path(parent_path)?;
    let new_name = to_wide(
        destination
            .file_name()
            .expect("validated destination file name"),
    );

    let operation = create_file_operation()?;
    file_operation_set_flags(&operation, FILE_OPERATION_FLAGS)?;
    let table = unsafe { operation.vtable::<FileOperationVTable>() };
    let status = unsafe {
        (table.move_item)(
            operation.raw(),
            item.raw(),
            destination_folder.raw(),
            new_name.as_ptr(),
            ptr::null_mut(),
        )
    };
    check_hresult(status, "IFileOperation::MoveItem(restore)")?;
    perform_file_operation(&operation, "restore Recycle Bin item")?;
    Ok(destination)
}

fn validate_recycle_source(path: &Path) -> Result<(), PlatformError> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(PlatformError::InvalidInput {
            message: format!(
                "Recycle Bin source must be a complete absolute path: {}",
                path.display()
            ),
        });
    }
    fs::symlink_metadata(path).map_err(|error| PlatformError::Io {
        operation: "locate item to move to Recycle Bin",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    Ok(())
}

fn validate_restore_destination(destination: &Path) -> Result<(), PlatformError> {
    if !destination.is_absolute() || destination.file_name().is_none() {
        return Err(PlatformError::InvalidInput {
            message: "restore destination must be a complete absolute path".to_string(),
        });
    }
    match fs::symlink_metadata(destination) {
        Ok(_) => {
            return Err(PlatformError::InvalidInput {
                message: format!(
                    "restore destination already exists: {}",
                    destination.display()
                ),
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(PlatformError::Io {
                operation: "check restore destination",
                path: Some(destination.to_path_buf()),
                message: error.to_string(),
            });
        }
    }
    let parent = destination
        .parent()
        .ok_or_else(|| PlatformError::InvalidInput {
            message: "restore destination has no parent directory".to_string(),
        })?;
    if !fs::metadata(parent).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(PlatformError::InvalidInput {
            message: format!(
                "restore destination parent is not a directory: {}",
                parent.display()
            ),
        });
    }
    Ok(())
}

fn trash_entry_from_shell_item(item: &ComPtr) -> Result<TrashEntry, PlatformError> {
    let item2 = shell_item2(item)?;
    let original_name = shell_item2_string(&item2, &PKEY_ORIGINAL_FILE_NAME)
        .or_else(|| shell_item_display_name(item, SIGDN_NORMALDISPLAY).ok())
        .unwrap_or_else(|| OsString::from("Unknown item"));
    let display_name = original_name.to_string_lossy().into_owned();
    let original_path = shell_item2_string(&item2, &PKEY_RECYCLE_DELETED_FROM)
        .map(PathBuf::from)
        .map(|parent| parent.join(&original_name));
    let deleted_at = shell_item2_file_time(&item2, &PKEY_RECYCLE_DATE_DELETED);
    let size = shell_item2_u64(&item2, &PKEY_SIZE).unwrap_or(0);
    let table = unsafe { item.vtable::<ShellItemVTable>() };
    let mut attributes = 0u32;
    let is_directory = unsafe {
        (table.get_attributes)(item.raw(), SFGAO_FOLDER, &mut attributes) >= 0
            && attributes & SFGAO_FOLDER != 0
    };
    Ok(TrashEntry {
        id: shell_item_id(item)?,
        display_name,
        original_path,
        deleted_at,
        size,
        is_directory,
    })
}

fn original_path_from_shell_item(item: &ComPtr) -> Result<Option<PathBuf>, PlatformError> {
    let item2 = shell_item2(item)?;
    let Some(parent) = shell_item2_string(&item2, &PKEY_RECYCLE_DELETED_FROM) else {
        return Ok(None);
    };
    let name = shell_item2_string(&item2, &PKEY_ORIGINAL_FILE_NAME)
        .or_else(|| shell_item_display_name(item, SIGDN_NORMALDISPLAY).ok());
    Ok(name.map(|name| PathBuf::from(parent).join(name)))
}

fn shell_item_id(item: &ComPtr) -> Result<TrashEntryId, PlatformError> {
    let parsing_name = shell_item_display_name(item, SIGDN_DESKTOPABSOLUTEPARSING)?;
    let encoded: String = parsing_name
        .encode_wide()
        .map(|unit| format!("{unit:04x}"))
        .collect();
    Ok(TrashEntryId::from_native(format!("win-shell-v1-{encoded}")))
}

fn enumerate_recycle_bin_items() -> Result<Vec<ComPtr>, PlatformError> {
    let recycle_bin = known_folder_item(&FOLDERID_RECYCLE_BIN)?;
    let table = unsafe { recycle_bin.vtable::<ShellItemVTable>() };
    let mut raw_enumerator = ptr::null_mut();
    let status = unsafe {
        (table.bind_to_handler)(
            recycle_bin.raw(),
            ptr::null_mut(),
            &BHID_ENUM_ITEMS,
            &IID_IENUM_SHELL_ITEMS,
            &mut raw_enumerator,
        )
    };
    check_hresult(status, "IShellItem::BindToHandler(BHID_EnumItems)")?;
    let enumerator = ComPtr::new(raw_enumerator, "Recycle Bin enumerator")?;
    let table = unsafe { enumerator.vtable::<EnumShellItemsVTable>() };
    let mut items = Vec::new();
    loop {
        let mut raw_item = ptr::null_mut();
        let mut fetched = 0u32;
        let status = unsafe { (table.next)(enumerator.raw(), 1, &mut raw_item, &mut fetched) };
        if status == S_FALSE {
            break;
        }
        check_hresult(status, "IEnumShellItems::Next")?;
        if fetched == 0 {
            return Err(PlatformError::Native {
                operation: "IEnumShellItems::Next",
                message: "returned success without an item".to_string(),
            });
        }
        items.push(ComPtr::new(raw_item, "Recycle Bin item")?);
    }
    Ok(items)
}

fn known_folder_item(folder_id: &Guid) -> Result<ComPtr, PlatformError> {
    let mut raw = ptr::null_mut();
    let status =
        unsafe { SHGetKnownFolderItem(folder_id, 0, ptr::null_mut(), &IID_ISHELL_ITEM, &mut raw) };
    check_hresult(status, "SHGetKnownFolderItem")?;
    ComPtr::new(raw, "known folder shell item")
}

fn shell_item_from_path(path: &Path) -> Result<ComPtr, PlatformError> {
    let path_wide = shell_parsing_name_wide(path)?;
    let mut raw = ptr::null_mut();
    let status = unsafe {
        SHCreateItemFromParsingName(
            path_wide.as_ptr(),
            ptr::null_mut(),
            &IID_ISHELL_ITEM,
            &mut raw,
        )
    };
    check_hresult(status, "SHCreateItemFromParsingName")?;
    ComPtr::new(raw, "filesystem shell item")
}

fn shell_item2(item: &ComPtr) -> Result<ComPtr, PlatformError> {
    let table = unsafe { item.vtable::<ShellItemVTable>() };
    let mut raw = ptr::null_mut();
    let status = unsafe { (table.query_interface)(item.raw(), &IID_ISHELL_ITEM2, &mut raw) };
    check_hresult(status, "IShellItem::QueryInterface(IShellItem2)")?;
    ComPtr::new(raw, "IShellItem2")
}

fn shell_item_display_name(item: &ComPtr, kind: u32) -> Result<OsString, PlatformError> {
    let table = unsafe { item.vtable::<ShellItemVTable>() };
    let mut raw = ptr::null_mut();
    let status = unsafe { (table.get_display_name)(item.raw(), kind, &mut raw) };
    check_hresult(status, "IShellItem::GetDisplayName")?;
    if raw.is_null() {
        return Err(PlatformError::Native {
            operation: "IShellItem::GetDisplayName",
            message: "returned a null string".to_string(),
        });
    }
    let value = unsafe { os_string_from_wide_ptr(raw) };
    unsafe { CoTaskMemFree(raw.cast()) };
    Ok(value)
}

fn shell_item2_string(item: &ComPtr, key: &PropertyKey) -> Option<OsString> {
    let table = unsafe { item.vtable::<ShellItem2VTable>() };
    let mut raw = ptr::null_mut();
    let status = unsafe { (table.get_string)(item.raw(), key, &mut raw) };
    if status < 0 || raw.is_null() {
        return None;
    }
    let value = unsafe { os_string_from_wide_ptr(raw) };
    unsafe { CoTaskMemFree(raw.cast()) };
    Some(value)
}

fn shell_item2_file_time(item: &ComPtr, key: &PropertyKey) -> Option<SystemTime> {
    let table = unsafe { item.vtable::<ShellItem2VTable>() };
    let mut value = FileTime {
        low_date_time: 0,
        high_date_time: 0,
    };
    let status = unsafe { (table.get_file_time)(item.raw(), key, &mut value) };
    (status >= 0)
        .then(|| file_time_to_system_time(value))
        .flatten()
}

fn shell_item2_u64(item: &ComPtr, key: &PropertyKey) -> Option<u64> {
    let table = unsafe { item.vtable::<ShellItem2VTable>() };
    let mut value = 0u64;
    let status = unsafe { (table.get_uint64)(item.raw(), key, &mut value) };
    (status >= 0).then_some(value)
}

fn create_file_operation() -> Result<ComPtr, PlatformError> {
    let mut raw = ptr::null_mut();
    let status = unsafe {
        CoCreateInstance(
            &CLSID_FILE_OPERATION,
            ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_IFILE_OPERATION,
            &mut raw,
        )
    };
    check_hresult(status, "CoCreateInstance(FileOperation)")?;
    ComPtr::new(raw, "IFileOperation")
}

fn file_operation_set_flags(operation: &ComPtr, flags: u32) -> Result<(), PlatformError> {
    let table = unsafe { operation.vtable::<FileOperationVTable>() };
    let status = unsafe { (table.set_operation_flags)(operation.raw(), flags) };
    check_hresult(status, "IFileOperation::SetOperationFlags")
}

fn perform_file_operation(operation: &ComPtr, name: &'static str) -> Result<(), PlatformError> {
    let table = unsafe { operation.vtable::<FileOperationVTable>() };
    let status = unsafe { (table.perform_operations)(operation.raw()) };
    check_hresult(status, "IFileOperation::PerformOperations")?;
    let mut aborted = 0i32;
    let status = unsafe { (table.get_any_operations_aborted)(operation.raw(), &mut aborted) };
    check_hresult(status, "IFileOperation::GetAnyOperationsAborted")?;
    if aborted != 0 {
        Err(PlatformError::Native {
            operation: name,
            message: "the Shell aborted one or more operations".to_string(),
        })
    } else {
        Ok(())
    }
}

fn check_hresult(status: i32, operation: &'static str) -> Result<(), PlatformError> {
    if status >= 0 {
        Ok(())
    } else {
        Err(PlatformError::Native {
            operation,
            message: format!("HRESULT {:#010x}", status as u32),
        })
    }
}

fn last_windows_error(operation: &'static str, path: Option<&Path>) -> PlatformError {
    let code = unsafe { GetLastError() };
    windows_error_from_code_with_path(operation, path, code)
}

fn windows_error_from_code(operation: &'static str, code: u32) -> PlatformError {
    windows_error_from_code_with_path(operation, None, code)
}

fn windows_error_from_code_with_path(
    operation: &'static str,
    path: Option<&Path>,
    code: u32,
) -> PlatformError {
    PlatformError::Io {
        operation,
        path: path.map(Path::to_path_buf),
        message: std::io::Error::from_raw_os_error(code as i32).to_string(),
    }
}

unsafe fn os_string_from_wide_ptr(raw: *const u16) -> OsString {
    let mut len = 0usize;
    unsafe {
        while *raw.add(len) != 0 {
            len += 1;
        }
        OsString::from_wide(std::slice::from_raw_parts(raw, len))
    }
}

struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn enter() -> Result<Self, PlatformError> {
        let status = unsafe { CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED) };
        if status >= 0 {
            Ok(Self { uninitialize: true })
        } else if status == RPC_E_CHANGED_MODE {
            Ok(Self {
                uninitialize: false,
            })
        } else {
            Err(PlatformError::Native {
                operation: "CoInitializeEx",
                message: format!("HRESULT {:#010x}", status as u32),
            })
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

struct ComPtr {
    raw: *mut c_void,
}

impl ComPtr {
    fn new(raw: *mut c_void, name: &'static str) -> Result<Self, PlatformError> {
        if raw.is_null() {
            Err(PlatformError::Native {
                operation: name,
                message: "COM returned a null interface".to_string(),
            })
        } else {
            Ok(Self { raw })
        }
    }

    fn raw(&self) -> *mut c_void {
        self.raw
    }

    unsafe fn vtable<T>(&self) -> &T {
        unsafe {
            let table = *(self.raw as *const *const T);
            &*table
        }
    }
}

impl Drop for ComPtr {
    fn drop(&mut self) {
        unsafe {
            let table = *(self.raw as *const *const IUnknownVTable);
            ((*table).release)(self.raw);
        }
    }
}

fn windows_read_directory(path: &Path) -> Result<DirectoryListing, PlatformError> {
    let search_path = path.join("*");
    let search_path = to_wide(search_path.as_os_str());
    let mut data: Win32FindDataW = unsafe { std::mem::zeroed() };
    let handle = unsafe { FindFirstFileW(search_path.as_ptr(), &mut data) };

    if handle as isize == -1 {
        let error_code = unsafe { GetLastError() };
        if error_code == ERROR_FILE_NOT_FOUND && path.is_dir() {
            return Ok(DirectoryListing {
                path: path.to_path_buf(),
                entries: Vec::new(),
                warnings: Vec::new(),
            });
        }
        return Err(PlatformError::Io {
            operation: "read directory",
            path: Some(path.to_path_buf()),
            message: std::io::Error::from_raw_os_error(error_code as i32).to_string(),
        });
    }

    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    loop {
        let name = os_string_from_null_terminated(&data.file_name);
        if name != OsStr::new(".") && name != OsStr::new("..") {
            let entry_path = path.join(&name);
            let attributes = file_attributes_from_find_data(entry_path.clone(), &data);
            let open_policy =
                crate::default_file_open_policy(PlatformKind::Windows, &entry_path, &attributes);
            entries.push(DirectoryEntryMetadata {
                path: entry_path,
                name: name.to_string_lossy().into_owned(),
                attributes: Some(attributes),
                open_policy,
            });
        }

        data = unsafe { std::mem::zeroed() };
        if unsafe { FindNextFileW(handle, &mut data) } == 0 {
            let error_code = unsafe { GetLastError() };
            if error_code != ERROR_NO_MORE_FILES {
                warnings.push(DirectoryListingWarning {
                    path: path.to_path_buf(),
                    message: std::io::Error::from_raw_os_error(error_code as i32).to_string(),
                });
            }
            break;
        }
    }

    unsafe {
        FindClose(handle);
    }

    Ok(DirectoryListing {
        path: path.to_path_buf(),
        entries,
        warnings,
    })
}

fn file_attributes_from_find_data(path: PathBuf, data: &Win32FindDataW) -> FileAttributes {
    let native = data.file_attributes;
    let is_dir = native & FILE_ATTRIBUTE_DIRECTORY != 0;
    let reparse_point = native & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    FileAttributes {
        path: path.clone(),
        is_file: !is_dir,
        is_dir,
        len: (u64::from(data.file_size_high) << 32) | u64::from(data.file_size_low),
        readonly: native & FILE_ATTRIBUTE_READONLY != 0,
        modified: file_time_to_system_time(data.last_write_time),
        hidden: native & FILE_ATTRIBUTE_HIDDEN != 0,
        system: native & FILE_ATTRIBUTE_SYSTEM != 0,
        archive: native & FILE_ATTRIBUTE_ARCHIVE != 0,
        symlink: reparse_point && data.reserved0 == IO_REPARSE_TAG_SYMLINK,
        junction: reparse_point && data.reserved0 == IO_REPARSE_TAG_MOUNT_POINT,
        reparse_point,
        shortcut: is_shortcut(&path),
    }
}

fn os_string_from_null_terminated(value: &[u16]) -> OsString {
    let len = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    OsString::from_wide(&value[..len])
}

fn file_time_to_system_time(value: FileTime) -> Option<SystemTime> {
    const WINDOWS_TO_UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;

    let ticks = (u64::from(value.high_date_time) << 32) | u64::from(value.low_date_time);
    if ticks == 0 {
        return None;
    }
    let duration_from_epoch = |delta: u64| Duration::from_nanos(delta.saturating_mul(100));
    if ticks >= WINDOWS_TO_UNIX_EPOCH_TICKS {
        UNIX_EPOCH.checked_add(duration_from_epoch(ticks - WINDOWS_TO_UNIX_EPOCH_TICKS))
    } else {
        UNIX_EPOCH.checked_sub(duration_from_epoch(WINDOWS_TO_UNIX_EPOCH_TICKS - ticks))
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

fn shell_parsing_name_wide(path: &Path) -> Result<Vec<u16>, PlatformError> {
    const BACKSLASH: u16 = b'\\' as u16;
    const VERBATIM_PREFIX: [u16; 4] = [BACKSLASH, BACKSLASH, b'?' as u16, BACKSLASH];

    let wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(PlatformError::InvalidInput {
            message: "Shell paths must not contain NUL characters".to_string(),
        });
    }

    let mut normalized = if !wide.starts_with(&VERBATIM_PREFIX) {
        wide
    } else if wide.len() >= 8
        && ascii_u16_eq_ignore_case(wide[4], b'U')
        && ascii_u16_eq_ignore_case(wide[5], b'N')
        && ascii_u16_eq_ignore_case(wide[6], b'C')
        && wide[7] == BACKSLASH
    {
        let mut normalized = Vec::with_capacity(wide.len() - 2);
        normalized.extend_from_slice(&[BACKSLASH, BACKSLASH]);
        normalized.extend_from_slice(&wide[8..]);
        normalized
    } else if wide.len() >= 7
        && ascii_u16_is_drive_letter(wide[4])
        && wide[5] == b':' as u16
        && wide[6] == BACKSLASH
    {
        wide[4..].to_vec()
    } else {
        return Err(PlatformError::InvalidInput {
            message: format!(
                "Windows Shell does not support this verbatim path: {}",
                path.display()
            ),
        });
    };

    normalized.push(0);
    Ok(normalized)
}

fn ascii_u16_eq_ignore_case(value: u16, expected: u8) -> bool {
    u8::try_from(value).is_ok_and(|value| value.eq_ignore_ascii_case(&expected))
}

fn ascii_u16_is_drive_letter(value: u16) -> bool {
    u8::try_from(value).is_ok_and(|value| value.is_ascii_alphabetic())
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

#[derive(Debug, Clone, Copy)]
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

#[repr(C)]
struct ShQueryRecycleBinInfo {
    cb_size: u32,
    size: i64,
    item_count: i64,
}

#[repr(C)]
struct PropertyKey {
    format_id: Guid,
    property_id: u32,
}

#[repr(C)]
struct IUnknownVTable {
    _query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    _add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct ShellItemVTable {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    _add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    _release: unsafe extern "system" fn(*mut c_void) -> u32,
    bind_to_handler: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *const Guid,
        *const Guid,
        *mut *mut c_void,
    ) -> i32,
    _get_parent: usize,
    get_display_name: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> i32,
    get_attributes: unsafe extern "system" fn(*mut c_void, u32, *mut u32) -> i32,
    _compare: usize,
}

#[repr(C)]
struct EnumShellItemsVTable {
    _query_interface: usize,
    _add_ref: usize,
    _release: usize,
    next: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void, *mut u32) -> i32,
    _skip: usize,
    _reset: usize,
    _clone: usize,
}

#[repr(C)]
struct ShellItem2VTable {
    _query_interface: usize,
    _add_ref: usize,
    _release: usize,
    _base_and_property_methods: [usize; 12],
    get_file_time: unsafe extern "system" fn(*mut c_void, *const PropertyKey, *mut FileTime) -> i32,
    _get_int32: usize,
    get_string: unsafe extern "system" fn(*mut c_void, *const PropertyKey, *mut *mut u16) -> i32,
    _get_uint32: usize,
    get_uint64: unsafe extern "system" fn(*mut c_void, *const PropertyKey, *mut u64) -> i32,
    _get_bool: usize,
}

#[repr(C)]
struct FileOperationVTable {
    _query_interface: usize,
    _add_ref: usize,
    _release: usize,
    _advise_methods: [usize; 2],
    set_operation_flags: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    _before_move: [usize; 8],
    move_item: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *const u16,
        *mut c_void,
    ) -> i32,
    _move_items: usize,
    _copy_item: usize,
    _copy_items: usize,
    delete_item: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
    _delete_items: usize,
    _new_item: usize,
    perform_operations: unsafe extern "system" fn(*mut c_void) -> i32,
    get_any_operations_aborted: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
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
const FOLDERID_RECYCLE_BIN: Guid = Guid {
    data1: 0xB7534046,
    data2: 0x3ECB,
    data3: 0x4C18,
    data4: [0xBE, 0x4E, 0x64, 0xCD, 0x4C, 0xB7, 0xD6, 0xAC],
};
const IID_ISHELL_ITEM: Guid = Guid {
    data1: 0x43826D1E,
    data2: 0xE718,
    data3: 0x42EE,
    data4: [0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE],
};
const IID_ISHELL_ITEM2: Guid = Guid {
    data1: 0x7E9FB0D3,
    data2: 0x919F,
    data3: 0x4307,
    data4: [0xAB, 0x2E, 0x9B, 0x18, 0x60, 0x31, 0x0C, 0x93],
};
const BHID_ENUM_ITEMS: Guid = Guid {
    data1: 0x94F60519,
    data2: 0x2850,
    data3: 0x4924,
    data4: [0xAA, 0x5A, 0xD1, 0x5E, 0x84, 0x86, 0x80, 0x39],
};
const IID_IENUM_SHELL_ITEMS: Guid = Guid {
    data1: 0x70629033,
    data2: 0xE363,
    data3: 0x4A28,
    data4: [0xA5, 0x67, 0x0D, 0xB7, 0x80, 0x06, 0xE6, 0xD7],
};
const CLSID_FILE_OPERATION: Guid = Guid {
    data1: 0x3AD05575,
    data2: 0x8857,
    data3: 0x4850,
    data4: [0x92, 0x77, 0x11, 0xB8, 0x5B, 0xDB, 0x8E, 0x09],
};
const IID_IFILE_OPERATION: Guid = Guid {
    data1: 0x947AAB5F,
    data2: 0x0A5C,
    data3: 0x4C13,
    data4: [0xB4, 0xD6, 0x4B, 0xF7, 0x83, 0x6F, 0xC9, 0xF8],
};
const PKEY_ORIGINAL_FILE_NAME: PropertyKey = PropertyKey {
    format_id: Guid {
        data1: 0x0CEF7D53,
        data2: 0xFA64,
        data3: 0x11D1,
        data4: [0xA2, 0x03, 0x00, 0x00, 0xF8, 0x1F, 0xED, 0xEE],
    },
    property_id: 6,
};
const PKEY_RECYCLE_DELETED_FROM: PropertyKey = PropertyKey {
    format_id: Guid {
        data1: 0x9B174B33,
        data2: 0x40FF,
        data3: 0x11D2,
        data4: [0xA2, 0x7E, 0x00, 0xC0, 0x4F, 0xC3, 0x08, 0x71],
    },
    property_id: 2,
};
const PKEY_RECYCLE_DATE_DELETED: PropertyKey = PropertyKey {
    format_id: Guid {
        data1: 0x9B174B33,
        data2: 0x40FF,
        data3: 0x11D2,
        data4: [0xA2, 0x7E, 0x00, 0xC0, 0x4F, 0xC3, 0x08, 0x71],
    },
    property_id: 3,
};
const PKEY_SIZE: PropertyKey = PropertyKey {
    format_id: Guid {
        data1: 0xB725F130,
        data2: 0x47EF,
        data3: 0x101A,
        data4: [0xA5, 0xF1, 0x02, 0x60, 0x8C, 0x9E, 0xEB, 0xAC],
    },
    property_id: 12,
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

    fn SHGetKnownFolderItem(
        rfid: *const Guid,
        flags: u32,
        token: *mut c_void,
        riid: *const Guid,
        item: *mut *mut c_void,
    ) -> i32;

    fn SHCreateItemFromParsingName(
        path: *const u16,
        bind_context: *mut c_void,
        riid: *const Guid,
        item: *mut *mut c_void,
    ) -> i32;

    fn SHQueryRecycleBinW(root_path: *const u16, info: *mut ShQueryRecycleBinInfo) -> i32;
    fn SHEmptyRecycleBinW(hwnd: *mut c_void, root_path: *const u16, flags: u32) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(pv: *mut c_void);
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        class_id: *const Guid,
        outer: *mut c_void,
        context: u32,
        interface_id: *const Guid,
        object: *mut *mut c_void,
    ) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(
        h_wnd: *mut c_void,
        lp_text: *const u16,
        lp_caption: *const u16,
        u_type: u32,
    ) -> i32;
    fn OpenClipboard(h_wnd_new_owner: *mut c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(u_format: u32) -> *mut c_void;
    fn SetClipboardData(u_format: u32, h_mem: *mut c_void) -> *mut c_void;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn CloseHandle(object: *mut c_void) -> i32;
    fn GetLogicalDriveStringsW(buffer_length: u32, buffer: *mut u16) -> u32;
    fn GetDriveTypeW(root_path_name: *const u16) -> u32;
    fn GetVolumeInformationW(
        root_path_name: *const u16,
        volume_name_buffer: *mut u16,
        volume_name_size: u32,
        volume_serial_number: *mut u32,
        maximum_component_length: *mut u32,
        file_system_flags: *mut u32,
        file_system_name_buffer: *mut u16,
        file_system_name_size: u32,
    ) -> i32;
    fn GetDiskFreeSpaceExW(
        directory_name: *const u16,
        free_bytes_available: *mut u64,
        total_number_of_bytes: *mut u64,
        total_number_of_free_bytes: *mut u64,
    ) -> i32;
    fn FindFirstFileW(
        lp_file_name: *const u16,
        lp_find_file_data: *mut Win32FindDataW,
    ) -> *mut c_void;
    fn FindNextFileW(h_find_file: *mut c_void, lp_find_file_data: *mut Win32FindDataW) -> i32;
    fn FindClose(h_find_file: *mut c_void) -> i32;
    fn GetLastError() -> u32;
    fn GlobalAlloc(u_flags: u32, dw_bytes: usize) -> *mut c_void;
    fn GlobalLock(h_mem: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(h_mem: *mut c_void) -> i32;
    fn GlobalFree(h_mem: *mut c_void) -> *mut c_void;
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::{ComApartment, create_file_operation, shell_parsing_name_wide, to_wide};

    #[test]
    fn file_operation_com_class_exposes_the_declared_interface() {
        let _apartment = ComApartment::enter().expect("initialize COM");
        create_file_operation().expect("FileOperation should expose IFileOperation");
    }

    #[test]
    fn shell_parsing_name_removes_verbatim_drive_prefix() {
        assert_eq!(
            shell_parsing_name_wide(Path::new(r"\\?\C:\Users\Example\file.txt"))
                .expect("convert verbatim drive path"),
            to_wide(OsStr::new(r"C:\Users\Example\file.txt"))
        );
    }

    #[test]
    fn shell_parsing_name_converts_verbatim_unc_prefix() {
        assert_eq!(
            shell_parsing_name_wide(Path::new(r"\\?\UNC\server\share\file.txt"))
                .expect("convert verbatim UNC path"),
            to_wide(OsStr::new(r"\\server\share\file.txt"))
        );
    }

    #[test]
    fn shell_parsing_name_preserves_regular_paths() {
        assert_eq!(
            shell_parsing_name_wide(Path::new(r"C:\Users\Example\file.txt"))
                .expect("preserve regular path"),
            to_wide(OsStr::new(r"C:\Users\Example\file.txt"))
        );
    }
}
