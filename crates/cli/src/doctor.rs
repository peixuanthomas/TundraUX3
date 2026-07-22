use std::io::Write;
use std::path::Path;

use platform::{
    AppPaths, CapabilityStatus, CheckStatus, EnvironmentCheck, PathCheck, Platform, PlatformKind,
};
use storage::StorageManager;

use crate::path_report::{write_path_templates, write_resolved_paths};

pub(crate) fn run_doctor<Stdout: Write, Stderr: Write>(
    platform: &dyn Platform,
    stdout: &mut Stdout,
    stderr: &mut Stderr,
    asset_root: Option<&Path>,
) -> i32 {
    let _ = writeln!(stdout, "TundraUX3 doctor");
    let _ = writeln!(stdout, "Platform kind: {}", platform.kind().as_str());
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "Path templates:");
    write_path_templates(stdout);

    match platform::run_doctor_with(platform) {
        Ok(report) => {
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "Resolved paths:");
            write_resolved_paths(stdout, &report.app_paths);
            write_doctor_checks(stdout, &report.environment_checks, &report.path_checks);

            let storage_check = run_storage_check(&report.app_paths);
            write_storage_check(stdout, &storage_check);
            let asset_theme_id = asset_theme_id_from_storage(storage_check.theme_id.as_deref());
            let asset_check = run_asset_check(asset_root, &asset_theme_id);
            write_asset_check(stdout, &asset_check);

            if report.has_failures() || storage_check.status == CheckStatus::Fail {
                let _ = writeln!(stderr, "Doctor result: FAIL");
                1
            } else {
                let _ = writeln!(stdout, "Doctor result: PASS");
                0
            }
        }
        Err(error) => {
            write_fallback_doctor_checks(stdout, platform, &error);
            let asset_check = run_asset_check(asset_root, ascii_assets::DEFAULT_THEME_ID);
            write_asset_check(stdout, &asset_check);
            let _ = writeln!(stderr, "Doctor result: FAIL");
            1
        }
    }
}

fn write_doctor_checks(
    output: &mut impl Write,
    environment_checks: &[EnvironmentCheck],
    path_checks: &[PathCheck],
) {
    let _ = writeln!(output);
    let _ = writeln!(output, "Checks:");

    let _ = writeln!(output);
    let _ = writeln!(output, "Platform checks:");
    for check in environment_checks
        .iter()
        .filter(|check| is_platform_check(check))
    {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Terminal check:");
    for check in environment_checks
        .iter()
        .filter(|check| is_terminal_check(check))
    {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Capability checks:");
    for check in environment_checks
        .iter()
        .filter(|check| is_capability_check(check))
    {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Path checks:");
    for check in path_checks {
        write_path_check(output, check);
    }
}

fn write_storage_check(output: &mut impl Write, check: &StorageCheck) {
    let _ = writeln!(output);
    let _ = writeln!(output, "Storage checks:");
    let _ = writeln!(
        output,
        "[{}] {}: {}",
        check.status.as_str(),
        check.label,
        check.message
    );
}

fn write_asset_check(output: &mut impl Write, check: &AsciiAssetCheck) {
    let _ = writeln!(output);
    let _ = writeln!(output, "Asset checks:");
    let _ = writeln!(
        output,
        "[{}] Required ASCII assets (theme {}): {}",
        check.status.as_str(),
        check.theme_id,
        check.message
    );
    for detail in &check.details {
        let _ = writeln!(output, "  {detail}");
    }
}

fn write_environment_check(output: &mut impl Write, check: &EnvironmentCheck) {
    let _ = writeln!(
        output,
        "[{}] {}: {}",
        check.status.as_str(),
        check.label,
        check.message
    );
}

fn write_path_check(output: &mut impl Write, check: &PathCheck) {
    let _ = writeln!(
        output,
        "[{}] {}: {} - {}",
        check.status.as_str(),
        check.label,
        check.path.display(),
        check.message
    );
}

fn write_fallback_doctor_checks(
    output: &mut impl Write,
    platform: &dyn Platform,
    error: &platform::PlatformError,
) {
    let terminal_check = fallback_terminal_check(platform.kind());
    let capability_checks = fallback_capability_checks(platform);

    let _ = writeln!(output);
    let _ = writeln!(output, "Checks:");

    let _ = writeln!(output);
    let _ = writeln!(output, "Terminal check:");
    write_environment_check(output, &terminal_check);

    let _ = writeln!(output);
    let _ = writeln!(output, "Capability checks:");
    for check in &capability_checks {
        write_environment_check(output, check);
    }

    let _ = writeln!(output);
    let _ = writeln!(output, "Path checks:");
    let _ = writeln!(output, "[FAIL] App paths: {error}");
}

fn fallback_terminal_check(kind: PlatformKind) -> EnvironmentCheck {
    platform::terminal_environment_check(kind)
}

fn fallback_capability_checks(platform: &dyn Platform) -> Vec<EnvironmentCheck> {
    platform
        .capabilities()
        .checks()
        .into_iter()
        .map(|(name, status)| EnvironmentCheck {
            label: format!("Capability: {name}"),
            status: check_status_for_capability(status),
            message: status.as_str().to_string(),
        })
        .collect()
}

fn check_status_for_capability(status: CapabilityStatus) -> CheckStatus {
    match status {
        CapabilityStatus::Supported => CheckStatus::Pass,
        CapabilityStatus::BestEffort => CheckStatus::Warning,
        CapabilityStatus::Unsupported => CheckStatus::Warning,
    }
}

fn is_platform_check(check: &EnvironmentCheck) -> bool {
    !is_terminal_check(check) && !is_capability_check(check)
}

fn is_terminal_check(check: &EnvironmentCheck) -> bool {
    check.label == "Terminal"
}

fn is_capability_check(check: &EnvironmentCheck) -> bool {
    check.label.starts_with("Capability: ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageCheck {
    label: &'static str,
    status: CheckStatus,
    message: String,
    theme_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AsciiAssetCheck {
    status: CheckStatus,
    theme_id: String,
    message: String,
    details: Vec<String>,
}

fn run_storage_check(paths: &AppPaths) -> StorageCheck {
    match StorageManager::open(paths.clone()) {
        Ok(opened) => {
            let theme_id = opened.manager.load_config().ok().map(|config| config.theme);
            if opened.report.warnings.is_empty() && opened.report.migrated_files.is_empty() {
                StorageCheck {
                    label: "Storage bootstrap",
                    status: CheckStatus::Pass,
                    message: "storage initialized and loaded cleanly".to_string(),
                    theme_id,
                }
            } else {
                StorageCheck {
                    label: "Storage bootstrap",
                    status: CheckStatus::Warning,
                    message: storage_warning_message(&opened.report),
                    theme_id,
                }
            }
        }
        Err(error) => StorageCheck {
            label: "Storage bootstrap",
            status: CheckStatus::Fail,
            message: error.to_string(),
            theme_id: None,
        },
    }
}

fn run_asset_check(asset_root: Option<&Path>, theme_id: &str) -> AsciiAssetCheck {
    let theme_id = normalized_asset_theme_id(theme_id);
    let root = match asset_root {
        Some(root) => Ok(root.to_path_buf()),
        None => ascii_assets::asset_root_from_env_or_current_exe(),
    };

    let root = match root {
        Ok(root) => root,
        Err(error) => {
            return AsciiAssetCheck {
                status: CheckStatus::Warning,
                theme_id,
                message: format!("could not resolve asset root: {error}"),
                details: Vec::new(),
            };
        }
    };

    let report = ascii_assets::check_required_assets(&root, &theme_id);
    if report.is_ok() {
        return AsciiAssetCheck {
            status: CheckStatus::Pass,
            theme_id,
            message: format!(
                "{} assets present and valid at {}",
                report.checks.len(),
                root.display()
            ),
            details: Vec::new(),
        };
    }

    let missing = report.missing_assets();
    let unreadable = report.unreadable_assets();
    let invalid = report.invalid_assets();
    let mut details = Vec::new();
    for check in &missing {
        details.push(format!("missing: {} ({})", check.key, check.path.display()));
    }
    for check in &unreadable {
        details.push(format!(
            "unreadable: {} ({})",
            check.key,
            check.path.display()
        ));
    }
    for check in &invalid {
        details.push(format!(
            "invalid: {} ({}) - {}",
            check.key,
            check.path.display(),
            check.message
        ));
    }

    AsciiAssetCheck {
        status: CheckStatus::Warning,
        theme_id,
        message: format!(
            "{}; {}; {} at {}",
            asset_count_message(missing.len(), "missing"),
            asset_count_message(unreadable.len(), "unreadable"),
            asset_count_message(invalid.len(), "invalid"),
            root.display()
        ),
        details,
    }
}

fn asset_theme_id_from_storage(theme_id: Option<&str>) -> String {
    normalized_asset_theme_id(theme_id.unwrap_or(ascii_assets::DEFAULT_THEME_ID))
}

fn normalized_asset_theme_id(theme_id: &str) -> String {
    match theme_id.trim() {
        "" | "dark" | "light" => ascii_assets::DEFAULT_THEME_ID.to_string(),
        other => other.to_string(),
    }
}

fn asset_count_message(count: usize, label: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{count} {label} asset{suffix}")
}

fn storage_warning_message(report: &storage::StorageLoadReport) -> String {
    let mut warnings = report.warnings.clone();
    if !report.migrated_files.is_empty() {
        warnings.push(format!(
            "migrated {} storage files",
            report.migrated_files.len()
        ));
    }

    if warnings.is_empty() {
        "storage initialized with warnings".to_string()
    } else {
        format!("storage initialized with warnings: {}", warnings.join("; "))
    }
}
