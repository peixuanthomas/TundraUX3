use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskHealth {
    pub state: String,
    pub summary: String,
    pub source: String,
    pub temperature_celsius: Option<f64>,
    pub bad_sectors: Option<u64>,
    pub updated: Option<u64>,
    pub detail: Vec<(String, String)>,
}

impl DiskHealth {
    fn unknown(reason: String) -> Self {
        Self {
            state: "Unknown".into(),
            summary: "Disk health is unknown. Check the details or install smartmontools.".into(),
            source: "Unavailable".into(),
            temperature_celsius: None,
            bad_sectors: None,
            updated: None,
            detail: vec![("Reason".into(), reason)],
        }
    }
    pub(super) fn snapshot(&self, target: &str) -> ManagementSnapshot {
        let mut detail = self.detail.clone();
        detail.extend([
            ("Source".into(), self.source.clone()),
            (
                "Temperature Celsius".into(),
                self.temperature_celsius
                    .map(|n| format!("{n:.1}"))
                    .unwrap_or_else(|| "Unknown".into()),
            ),
            (
                "Bad sectors".into(),
                self.bad_sectors
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "Unknown".into()),
            ),
            (
                "Health sample time".into(),
                self.updated
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "Unknown".into()),
            ),
        ]);
        ManagementSnapshot {
            columns: vec!["Device".into(), "Disk health".into()],
            rows: vec![ManagementRow {
                id: target.into(),
                cells: vec![target.into(), self.state.clone()],
                detail,
                ..Default::default()
            }],
            notices: vec![self.summary.clone()],
            backend: self.source.clone(),
            ..Default::default()
        }
    }
}

/// Reads health data only; never starts a self-test or changes drive settings.
pub fn query_health(
    target: &Path,
    expected: &BTreeMap<String, String>,
    cancelled: &AtomicBool,
) -> Result<DiskHealth, ManagementError> {
    let devices = inventory(cancelled)?;
    let mounted = mounts()?;
    let device = devices
        .iter()
        .find(|d| {
            d["name"]
                .as_str()
                .is_some_and(|name| Path::new(name) == target)
        })
        .ok_or_else(|| {
            ManagementError::Conflict("The disk disappeared. Refresh the list.".into())
        })?;
    if ["device", "uuid", "size"]
        .iter()
        .any(|key| identity(device, &mounted).get(*key) != expected.get(*key))
    {
        return Err(ManagementError::Conflict(
            "The disk changed. Refresh the list.".into(),
        ));
    }
    if text(device, "type") != "disk" {
        return Err(ManagementError::InvalidInput(
            "Select the physical disk to read its health.".into(),
        ));
    }
    let canonical = fs::canonicalize(target).map_err(io_error)?;
    let metadata = fs::metadata(&canonical).map_err(io_error)?;
    if !canonical.starts_with("/dev")
        || !metadata.file_type().is_block_device()
        || expected.get("device")
            != Some(&format!(
                "{}:{}",
                libc::major(metadata.rdev()),
                libc::minor(metadata.rdev())
            ))
    {
        return Err(ManagementError::Conflict(
            "The device node changed. Refresh the list.".into(),
        ));
    }
    if let Ok(health) = udisks_health(&canonical) {
        return Ok(health);
    }
    if program("smartctl").is_err() {
        return Ok(DiskHealth::unknown(
            "The drive exposes no UDisks SMART data and smartctl is not installed.".into(),
        ));
    }
    let target = canonical
        .to_str()
        .ok_or_else(|| ManagementError::InvalidInput("Invalid device path.".into()))?;
    match super::super::network::run_status(
        "smartctl",
        &[
            "--json",
            "--health",
            "--attributes",
            "--nocheck=standby",
            target,
        ],
        cancelled,
    ) {
        Ok((output, errors, status)) => Ok(parse_smartctl(&output, status)
            .unwrap_or_else(|error| DiskHealth::unknown(format!("{error}\n{errors}")))),
        Err(ManagementError::Cancelled) => Err(ManagementError::Cancelled),
        Err(error) => Ok(DiskHealth::unknown(error.to_string())),
    }
}

fn udisks_health(target: &Path) -> Result<DiskHealth, ManagementError> {
    let bus = disk_bus()?;
    let manager = Proxy::new(
        &bus,
        UDISKS,
        "/org/freedesktop/UDisks2/Manager",
        "org.freedesktop.UDisks2.Manager",
    )
    .map_err(udisks_error)?;
    let paths: Vec<OwnedObjectPath> = manager
        .call(
            "ResolveDevice",
            &(
                HashMap::from([(
                    "path",
                    Value::from(target.to_str().ok_or_else(|| {
                        ManagementError::InvalidInput("Invalid disk path".into())
                    })?),
                )]),
                HashMap::<String, Value<'_>>::new(),
            ),
        )
        .map_err(udisks_error)?;
    let path = paths
        .first()
        .filter(|_| paths.len() == 1)
        .ok_or_else(|| ManagementError::Unavailable("No unique UDisks device".into()))?;
    let block = Proxy::new(&bus, UDISKS, path.as_str(), "org.freedesktop.UDisks2.Block")
        .map_err(udisks_error)?;
    let drive: OwnedObjectPath = block.get_property("Drive").map_err(udisks_error)?;
    let ata = Proxy::new(
        &bus,
        UDISKS,
        drive.as_str(),
        "org.freedesktop.UDisks2.Drive.Ata",
    )
    .map_err(udisks_error)?;
    if !ata
        .get_property::<bool>("SmartSupported")
        .map_err(udisks_error)?
        || !ata
            .get_property::<bool>("SmartEnabled")
            .map_err(udisks_error)?
    {
        return Err(ManagementError::Unavailable(
            "SMART is unavailable or disabled".into(),
        ));
    }
    let updated: u64 = ata.get_property("SmartUpdated").map_err(udisks_error)?;
    if updated == 0 {
        return Err(ManagementError::Unavailable(
            "UDisks has not collected a SMART sample".into(),
        ));
    }
    let failing: bool = ata.get_property("SmartFailing").map_err(udisks_error)?;
    let bad: i64 = ata
        .get_property("SmartNumBadSectors")
        .map_err(udisks_error)?;
    let temperature: f64 = ata.get_property("SmartTemperature").map_err(udisks_error)?;
    let attributes: i32 = ata
        .get_property("SmartNumAttributesFailing")
        .map_err(udisks_error)?;
    let warning = failing || attributes > 0 || bad > 0;
    Ok(DiskHealth {
        state: if warning { "Warning" } else { "Passed" }.into(),
        summary: if warning {
            "Disk health reports a problem. Back up files and inspect SMART details."
        } else {
            "The last SMART sample reported no health failure."
        }
        .into(),
        source: "UDisks2 SMART".into(),
        temperature_celsius: (temperature > 0.0).then_some(temperature - 273.15),
        bad_sectors: u64::try_from(bad).ok(),
        updated: Some(updated),
        detail: vec![
            ("SMART failing".into(), failing.to_string()),
            ("Failing attributes".into(), attributes.to_string()),
        ],
    })
}

fn parse_smartctl(output: &str, status: i32) -> Result<DiskHealth, ManagementError> {
    let json: Json = serde_json::from_str(output)
        .map_err(|e| ManagementError::Failed(format!("Cannot read SMART output: {e}")))?;
    let passed = json["smart_status"]["passed"].as_bool();
    let messages = json["smartctl"]["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["string"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    // Bits 0–2 describe command/read failure; bits 3–7 report health findings.
    // A nonzero exit status must not erase a readable failure report.
    if status & 7 != 0 || passed.is_none() {
        return Ok(DiskHealth::unknown(format!(
            "smartctl exit {status}: {messages}"
        )));
    }
    let mut bad = None;
    let mut detail = vec![("Native exit code".into(), status.to_string())];
    for attribute in json["ata_smart_attributes"]["table"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if matches!(attribute["id"].as_u64(), Some(5 | 197 | 198)) {
            let count = attribute["raw"]["value"].as_u64();
            if attribute["id"].as_u64() == Some(5) {
                bad = count;
            }
            detail.push((
                attribute["name"]
                    .as_str()
                    .unwrap_or("SMART attribute")
                    .into(),
                count
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "Unknown".into()),
            ));
        }
    }
    if let Some(warning) = json["nvme_smart_health_information_log"]["critical_warning"].as_u64() {
        detail.push(("NVMe critical warning".into(), warning.to_string()));
    }
    let warning = passed == Some(false)
        || status & 0xf8 != 0
        || bad.is_some_and(|n| n > 0)
        || json["nvme_smart_health_information_log"]["critical_warning"]
            .as_u64()
            .is_some_and(|n| n != 0);
    if !messages.is_empty() {
        detail.push(("Tool details".into(), messages));
    }
    Ok(DiskHealth {
        state: if warning { "Warning" } else { "Passed" }.into(),
        summary: if warning {
            "Disk health reports a problem. Back up files and inspect SMART details."
        } else {
            "SMART reported no health failure."
        }
        .into(),
        source: "smartctl".into(),
        temperature_celsius: json["temperature"]["current"].as_f64(),
        bad_sectors: bad,
        updated: json["local_time"]["time_t"].as_u64(),
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failing_smart_exit_retains_the_actual_health_result() {
        let health = parse_smartctl(r#"{"smart_status":{"passed":false},"temperature":{"current":45},"ata_smart_attributes":{"table":[{"id":5,"name":"Reallocated_Sector_Ct","raw":{"value":3}}]}}"#, 8).unwrap();
        assert_eq!(health.state, "Warning");
        assert_eq!(health.temperature_celsius, Some(45.0));
        assert_eq!(health.bad_sectors, Some(3));
    }
    #[test]
    fn missing_or_unreadable_health_is_not_a_zero_or_success() {
        for (json, exit) in [("{}", 0), (r#"{"smart_status":{"passed":true}}"#, 2)] {
            assert_eq!(parse_smartctl(json, exit).unwrap().state, "Unknown");
        }
        let health = parse_smartctl(r#"{"smart_status":{"passed":true}}"#, 0).unwrap();
        assert_eq!(health.state, "Passed");
        assert_eq!(health.bad_sectors, None);
    }
}
