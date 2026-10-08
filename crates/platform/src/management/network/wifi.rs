use super::*;
use serde::{Deserialize, Serialize};

/// Access points are read as typed D-Bus properties; SSIDs need not be UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiAccessPoint {
    pub interface: String,
    pub path: String,
    pub ssid: Vec<u8>,
    pub signal: u8,
    pub security: String,
    pub connected: bool,
    pub saved_uuid: Option<String>,
    pub password_saved: bool,
}

pub(super) fn security(flags: u32, wpa: u32, rsn: u32) -> &'static str {
    const PSK: u32 = 0x100;
    const EAP: u32 = 0x200;
    const SAE: u32 = 0x400;
    const OWE: u32 = 0x800 | 0x1000;
    if (wpa | rsn) & EAP != 0 && (wpa | rsn) & (PSK | SAE) == 0 {
        "enterprise"
    } else if rsn & SAE != 0 {
        "wpa3"
    } else if (wpa | rsn) & PSK != 0 {
        "wpa2"
    } else if rsn & OWE != 0 {
        "unsupported"
    } else if flags & 1 == 0 {
        "open"
    } else {
        "unsupported"
    }
}

fn saved_security_supported(mode: &str, flags: u32, wpa: u32, rsn: u32) -> bool {
    match mode {
        "wpa2" => (wpa | rsn) & 0x100 != 0,
        "wpa3" => rsn & 0x400 != 0,
        "open" => security(flags, wpa, rsn) == "open",
        _ => false,
    }
}

fn connection_security(flags: u32, wpa: u32, rsn: u32, saved: Option<&str>) -> String {
    saved
        .filter(|mode| saved_security_supported(mode, flags, wpa, rsn))
        .unwrap_or_else(|| security(flags, wpa, rsn))
        .into()
}

pub fn scan_wifi(
    name: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<WifiAccessPoint>, ManagementError> {
    validate_interface(name)?;
    let bus = bus()?;
    let device = nm_device(name)?;
    let wireless = Proxy::new(
        &bus,
        NM,
        device.path.as_str(),
        "org.freedesktop.NetworkManager.Device.Wireless",
    )
    .map_err(dbus_error)?;
    let active: OwnedObjectPath = wireless
        .get_property("ActiveAccessPoint")
        .map_err(dbus_error)?;
    let paths: Vec<OwnedObjectPath> = wireless
        .call("GetAllAccessPoints", &())
        .map_err(dbus_error)?;
    let profiles = saved_wifi_profiles()?;
    let mut output = Vec::new();
    for path in paths.into_iter().take(1024) {
        check_cancelled(cancelled)?;
        let ap = Proxy::new(
            &bus,
            NM,
            path.as_str(),
            "org.freedesktop.NetworkManager.AccessPoint",
        )
        .map_err(dbus_error)?;
        let ssid: Vec<u8> = ap.get_property("Ssid").map_err(dbus_error)?;
        if ssid.is_empty() {
            continue;
        }
        let flags: u32 = ap.get_property("Flags").map_err(dbus_error)?;
        let wpa: u32 = ap.get_property("WpaFlags").map_err(dbus_error)?;
        let rsn: u32 = ap.get_property("RsnFlags").map_err(dbus_error)?;
        let saved = profiles.iter().find(|profile| {
            profile.ssid == ssid
                && saved_security_supported(&profile.security, flags, wpa, rsn)
                && (profile.interface.is_empty() || profile.interface == name)
        });
        // Transition access points can accept both PSK and SAE. Retain the
        // saved profile's actual mode so helper revalidation and activation
        // agree, instead of treating a valid WPA2 profile as unsaved.
        let security = connection_security(flags, wpa, rsn, saved.map(|p| p.security.as_str()));
        output.push(WifiAccessPoint {
            interface: name.into(),
            path: path.to_string(),
            ssid,
            signal: ap.get_property::<u8>("Strength").map_err(dbus_error)?,
            security,
            connected: active == path,
            saved_uuid: saved.map(|p| p.uuid.clone()),
            password_saved: saved.is_some_and(|p| p.password_saved),
        });
    }
    output.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.cmp(&b.ssid))
    });
    Ok(output)
}

pub(super) fn encode_ssid(ssid: &[u8]) -> String {
    ssid.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn display_ssid(ssid: &[u8]) -> String {
    String::from_utf8_lossy(ssid)
        .chars()
        .map(|character| {
            if character.is_control() {
                format!("\\u{{{:x}}}", u32::from(character))
            } else {
                character.to_string()
            }
        })
        .collect()
}

pub(super) fn decode_ssid(ssid: &str) -> Result<Vec<u8>, ManagementError> {
    if ssid.is_empty()
        || ssid.len() > 64
        || !ssid.len().is_multiple_of(2)
        || !ssid.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(ManagementError::InvalidInput(
            "Invalid Wi-Fi network. Refresh the list.".into(),
        ));
    }
    (0..ssid.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&ssid[index..index + 2], 16)
                .map_err(|_| ManagementError::InvalidInput("Invalid SSID".into()))
        })
        .collect()
}

pub(super) fn row(
    ap: WifiAccessPoint,
    interface: &ManagementRow,
    reason: Option<String>,
) -> ManagementRow {
    let mut identity = interface.identity.clone();
    identity.extend(BTreeMap::from([
        ("interface".into(), ap.interface.clone()),
        ("ap_path".into(), ap.path.clone()),
        ("ssid_hex".into(), encode_ssid(&ap.ssid)),
        ("security".into(), ap.security.clone()),
        (
            "saved_uuid".into(),
            ap.saved_uuid.clone().unwrap_or_default(),
        ),
    ]));
    let unsupported = !matches!(ap.security.as_str(), "open" | "wpa2" | "wpa3");
    let password = ManagementField {
        secret: true,
        ..field("password", "Password", "", true, &[])
    };
    let mut actions = vec![ManagementAction {
        id: "wifi-connect".into(),
        label: "Connect".into(),
        privileged: true,
        primary: true,
        group: "connection".into(),
        fields: if ap.security == "open" || ap.password_saved {
            Vec::new()
        } else {
            vec![password.clone()]
        },
        disabled_reason: if unsupported {
            Some(
                "This network needs an unsupported security type. Use the system network settings."
                    .into(),
            )
        } else {
            reason
        },
        ..Default::default()
    }];
    if ap.password_saved && ap.security != "open" {
        actions.push(ManagementAction {
            id: "wifi-connect-password".into(),
            label: "Enter another password".into(),
            privileged: true,
            fields: vec![password],
            ..Default::default()
        });
    }
    ManagementRow {
        id: format!(
            "wifi:{}:{}",
            ap.interface,
            ap.path.rsplit('/').next().unwrap_or_default()
        ),
        cells: vec![
            display_ssid(&ap.ssid),
            if ap.connected {
                "Connected"
            } else if ap.saved_uuid.is_some() {
                "Saved"
            } else {
                "Available"
            }
            .into(),
            format!("{}%", ap.signal),
            ap.security.clone(),
        ],
        detail: vec![
            ("Interface".into(), ap.interface),
            ("Signal".into(), format!("{}%", ap.signal)),
            ("Security".into(), ap.security),
            ("Saved".into(), ap.saved_uuid.is_some().to_string()),
            ("Connected".into(), ap.connected.to_string()),
        ],
        identity,
        actions,
    }
}

pub(super) fn selected_values(
    command: &ManagementCommand,
    name: &str,
    cancelled: &AtomicBool,
) -> Result<BTreeMap<String, String>, ManagementError> {
    let mut values = command.values.clone();
    let Some(path) = command.identity.get("ap_path") else {
        return Ok(values);
    };
    let ap = scan_wifi(name, cancelled)?
        .into_iter()
        .find(|ap| &ap.path == path)
        .ok_or_else(|| {
            ManagementError::Conflict("The Wi-Fi network disappeared. Refresh the list.".into())
        })?;
    if command.identity.get("ssid_hex") != Some(&encode_ssid(&ap.ssid))
        || command.identity.get("security") != Some(&ap.security)
        || command
            .identity
            .get("saved_uuid")
            .map(String::as_str)
            .unwrap_or("")
            != ap.saved_uuid.as_deref().unwrap_or("")
    {
        return Err(ManagementError::Conflict(
            "The Wi-Fi network changed. Refresh the list.".into(),
        ));
    }
    values.insert("ssid_hex".into(), encode_ssid(&ap.ssid));
    values.insert("ssid".into(), display_ssid(&ap.ssid));
    values.insert("security".into(), ap.security);
    values.insert("ap_path".into(), ap.path);
    if command.action != "wifi-connect-password"
        && !values.contains_key("password")
        && ap.password_saved
    {
        if let Some(uuid) = ap.saved_uuid {
            values.insert("saved_uuid".into(), uuid);
        }
    }
    Ok(values)
}

pub(super) fn request_scan(name: &str) -> Result<(), ManagementError> {
    let bus = bus()?;
    let device = nm_device(name)?;
    let proxy = Proxy::new(
        &bus,
        NM,
        device.path.as_str(),
        "org.freedesktop.NetworkManager.Device.Wireless",
    )
    .map_err(dbus_error)?;
    proxy
        .call::<_, _, ()>("RequestScan", &(HashMap::<String, Value<'_>>::new(),))
        .map_err(dbus_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssid_roundtrip_preserves_binary_and_delimiters() {
        let ssid = b"a:b\\c\xff\0";
        assert_eq!(decode_ssid(&encode_ssid(ssid)).unwrap(), ssid);
        assert!(decode_ssid("a").is_err());
        assert!(decode_ssid("zz").is_err());
    }
    #[test]
    fn security_keeps_enterprise_and_owe_out_of_personal_form() {
        assert_eq!(security(0, 0, 0), "open");
        assert_eq!(security(1, 0, 0x100), "wpa2");
        assert_eq!(security(1, 0, 0x500), "wpa3");
        assert_eq!(security(1, 0, 0x200), "enterprise");
        assert_eq!(security(1, 0, 0x800), "unsupported");
        assert_eq!(security(1, 0, 0), "unsupported");
    }
    #[test]
    fn selected_saved_and_open_networks_do_not_ask_for_password() {
        for (security, password_saved, count) in
            [("open", false, 0), ("wpa2", true, 0), ("wpa2", false, 1)]
        {
            let row = row(
                WifiAccessPoint {
                    interface: "wlan0".into(),
                    path: "/ap/1".into(),
                    ssid: b"network".to_vec(),
                    signal: 70,
                    security: security.into(),
                    connected: false,
                    saved_uuid: password_saved.then(|| "uuid".into()),
                    password_saved,
                },
                &ManagementRow::default(),
                None,
            );
            assert_eq!(row.actions[0].fields.len(), count);
            assert!(!row.actions[0].confirm);
        }
    }

    #[test]
    fn saved_wpa2_transition_profile_remains_usable_without_password_input() {
        assert!(saved_security_supported("wpa2", 1, 0, 0x500));
        assert!(saved_security_supported("wpa3", 1, 0, 0x500));
        assert!(!saved_security_supported("wpa2", 1, 0, 0x400));
        assert!(!saved_security_supported("wpa2", 1, 0, 0x200));
        assert!(!saved_security_supported("open", 1, 0, 0x500));
        assert_eq!(connection_security(1, 0, 0x500, None), "wpa3");
        let mode = connection_security(1, 0, 0x500, Some("wpa2"));
        assert_eq!(mode, "wpa2");
        let row = row(
            WifiAccessPoint {
                interface: "wlan0".into(),
                path: "/ap/1".into(),
                ssid: b"transition-network".to_vec(),
                signal: 70,
                security: mode,
                connected: false,
                saved_uuid: Some("saved-wpa2".into()),
                password_saved: true,
            },
            &ManagementRow::default(),
            None,
        );
        assert!(row.actions[0].fields.is_empty());
        assert_eq!(row.identity["security"], "wpa2");
        assert_eq!(row.identity["saved_uuid"], "saved-wpa2");
    }
}
