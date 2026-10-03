use super::*;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;

const CAPTURE: &str = "__TUNDRA_CAPTURE_SCRIPT";
const CMD: &str = "__TUNDRA_CAPTURE_CMD";
const ENVIRONMENT: &str = "__TUNDRA_CAPTURE_ENV";
const DIRECTORY: &str = "__TUNDRA_CAPTURE_CWD";
const COMPLETE: &str = "__TUNDRA_CAPTURE_COMPLETE";
const STATUS: &str = "__TUNDRA_CAPTURE_STATUS";
const INTERNAL_NAMES: [&str; 6] = [CAPTURE, CMD, ENVIRONMENT, DIRECTORY, COMPLETE, STATUS];

impl SystemCommandSession {
    pub(super) fn run_windows(&mut self, command: &str) -> io::Result<SystemCommandResult> {
        let files = tempfile::tempdir()?;
        let script = files.path().join("capture.cmd");
        let environment = files.path().join("environment");
        let directory = files.path().join("directory");
        let complete = files.path().join("complete");
        fs::write(&script, CAPTURE_SCRIPT)?;
        // Resolve the interpreter independently of the session's PATH,
        // ComSpec and SystemRoot, which commands may edit or remove.
        let interpreter = native_cmd()?;
        let status = Command::new(&interpreter)
            .args(["/D", "/S", "/C"])
            // Keep command-prompt syntax (e.g. FOR %i), not batch syntax.
            // A separate group keeps capture outside IF/FOR bodies. Do not
            // add trailing spaces to an unquoted SET value.
            .raw_arg(format!("\"({command})&\"%{CAPTURE}%\"\""))
            .current_dir(&self.directory)
            .env_clear()
            .envs(&self.environment)
            .env(CAPTURE, &script)
            .env(CMD, &interpreter)
            .env(ENVIRONMENT, &environment)
            .env(DIRECTORY, &directory)
            .env(COMPLETE, &complete)
            .status()?;
        let snapshot = (|| {
            if fs::read(&complete)? != b"complete\r\n" {
                return Err(io::Error::other("incomplete command state"));
            }
            parse_snapshot(&fs::read(environment)?, &fs::read(directory)?)
        })();
        let state_error = match snapshot {
            Ok((mut environment, directory)) => {
                environment.retain(|name, _| !is_internal(name));
                // Preserve coincidentally named user variables, not the
                // capture protocol's temporary values.
                for (name, value) in &self.environment {
                    if is_internal(name) {
                        environment.insert(name.clone(), value.clone());
                    }
                }
                self.environment = environment;
                self.directory = directory;
                None
            }
            Err(error) => Some(error),
        };
        Ok(SystemCommandResult {
            exit_code: status.code().unwrap_or(1),
            state_error,
        })
    }
}

// Expand ERRORLEVEL after the command without enabling delayed expansion
// and changing literal ! in input. /U only affects snapshot subprocesses.
const CAPTURE_SCRIPT: &str = "@set \"__TUNDRA_CAPTURE_STATUS=%errorlevel%\"\r\n\
@\"%__TUNDRA_CAPTURE_CMD%\" /D /U /C cd > \"%__TUNDRA_CAPTURE_CWD%\"\r\n\
@if errorlevel 1 exit /b %__TUNDRA_CAPTURE_STATUS%\r\n\
@\"%__TUNDRA_CAPTURE_CMD%\" /D /U /C set > \"%__TUNDRA_CAPTURE_ENV%\"\r\n\
@if errorlevel 1 exit /b %__TUNDRA_CAPTURE_STATUS%\r\n\
@echo complete>\"%__TUNDRA_CAPTURE_COMPLETE%\"\r\n\
@exit /b %__TUNDRA_CAPTURE_STATUS%\r\n";

fn native_cmd() -> io::Result<PathBuf> {
    let mut buffer = [0u16; 32768];
    // SAFETY: buffer is writable and its size is passed accurately.
    let length = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length >= buffer.len() {
        return Err(io::Error::other("system directory path is too long"));
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])).join("cmd.exe"))
}

fn is_internal(name: &std::ffi::OsStr) -> bool {
    INTERNAL_NAMES
        .iter()
        .any(|internal| name.eq_ignore_ascii_case(internal))
}

fn parse_snapshot(
    environment: &[u8],
    directory: &[u8],
) -> io::Result<(BTreeMap<OsString, OsString>, PathBuf)> {
    let invalid = || io::Error::other("incomplete command environment/directory snapshot");
    let decode = |bytes: &[u8]| -> io::Result<Vec<u16>> {
        if bytes.len() % 2 != 0 {
            return Err(invalid());
        }
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        if units.contains(&0) {
            return Err(invalid());
        }
        Ok(units)
    };
    let directory = decode(directory)?;
    let directory = directory.strip_suffix(&[13, 10]).ok_or_else(invalid)?;
    let directory = PathBuf::from(OsString::from_wide(directory));
    if !directory.is_absolute() {
        return Err(invalid());
    }
    let mut values = BTreeMap::new();
    for record in decode(environment)?.split_inclusive(|unit| *unit == 10) {
        let record = record.strip_suffix(&[13, 10]).ok_or_else(invalid)?;
        let separator = record
            .iter()
            .position(|unit| *unit == u16::from(b'='))
            .ok_or_else(invalid)?;
        if separator == 0 {
            // Ignore hidden drive bookkeeping; the session tracks the
            // active working directory returned by CD.
            continue;
        }
        values.insert(
            OsString::from_wide(&record[..separator]),
            OsString::from_wide(&record[separator + 1..]),
        );
    }
    Ok((values, directory))
}
